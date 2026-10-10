//! Wire contract and Unix-socket client for `ferrix_appd`.
//!
//! Line-delimited JSON, schema-versioned through [`ferrix_app_api::SCHEMA_VERSION`].
//! Two inbound message shapes exist:
//!
//! - a [`Request`] (`{"request": ...}`) always receives exactly one [`Reply`];
//! - a [`Report`] (`{"report": ...}`) never receives a reply. Reports are how
//!   the compositor tells `ferrix_appd` about surfaces and focus on a stream that is
//!   also subscribed to pushed [`Notification`]s, so replies and pushes never
//!   interleave ambiguously.
//!
//! Callers are identified by their connection role and, for surface
//! association, by process ids the compositor obtains from the kernel
//! (`SO_PEERCRED`), never by ids an application claims in a payload.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_app_api::{
    AppDescriptor, AppError, AppEvent, AppIconBitmap, AppId, AppIdentity, AppInstance, AppIntent,
    SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

pub use ferrix_app_api::DEFAULT_APPD_SOCKET as DEFAULT_SOCKET;

/// Who is on the other end of a connection. Roles gate what `ferrix_appd` accepts:
/// only a compositor may send surface reports; only the home surface and
/// tools may launch arbitrary apps; only services and tools may resolve a
/// pid to an application identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientRole {
    Compositor,
    Home,
    Application,
    Tool,
    /// A platform service (telephony, network, contacts) that resolves its
    /// own peers through [`Request::Identify`]. Bootstrap-trusted like the
    /// compositor: it is not launched by `ferrix_appd`.
    Service,
}

/// Messages that receive exactly one [`Reply`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    /// Identify the connection. Must precede privileged requests.
    Hello {
        role: ClientRole,
    },
    /// Resolve a kernel pid (the caller's `SO_PEERCRED` peer) to the
    /// application it belongs to and that application's granted
    /// capabilities. Wire form: `{"request":"identify","pid":N}`.
    Identify {
        pid: u32,
    },
    Catalog,
    Instances,
    /// The launcher bitmap of an installed app (ADR-008), when its runtime
    /// provides one. Wire form: `{"request":"app-icon","id":"apk.org.example"}`.
    /// One bounded bitmap per reply ([`ferrix_app_api::APP_ICON_MAX_SIDE`]);
    /// checked-in apps never have one (their glyphs are drawn by the home).
    AppIcon {
        id: AppId,
    },
    /// Start an app, or activate it when a single-instance app already runs.
    Launch {
        id: AppId,
    },
    /// Bring a running app's surface to the foreground.
    Activate {
        id: AppId,
    },
    /// Stop a running app (graceful, then forced).
    Terminate {
        id: AppId,
    },
    /// Return to the home surface.
    Home,
    /// Receive [`Notification`]s on this stream from now on.
    Subscribe,
    /// Open whichever application handles `intent`: launch it with the
    /// intent, or activate the running instance and push
    /// [`Notification::Intent`] to it. The caller never names the handler.
    Open {
        intent: AppIntent,
    },
}

/// Fire-and-forget messages from the compositor (and from applications
/// yielding the foreground). Never replied to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "report", rename_all = "kebab-case")]
pub enum Report {
    /// A toplevel committed its first buffer. `pid` is the client's process
    /// id as seen by the compositor; `app_id` is the untrusted xdg app id.
    /// `surface` is the compositor's opaque id for this one toplevel: several
    /// toplevels (and several Wayland connections) may share a pid, as the
    /// Android runtime's per-task windows do.
    SurfaceMapped {
        pid: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<u64>,
    },
    /// A mapped toplevel went away. With `surface`, exactly that toplevel;
    /// without it (older compositors), every toplevel of `pid`.
    SurfaceUnmapped {
        pid: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<u64>,
    },
    /// Keyboard/visual focus moved to the client with `pid` (or to nothing);
    /// `surface` names the focused toplevel when known.
    FocusChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<u64>,
    },
    /// A client asked (xdg_activation_v1) to raise one of its toplevels.
    /// `ferrix_appd` decides; a linked compositor never raises on its own.
    ActivationRequested { pid: u32, surface: u64 },
    /// The compositor recognised the global home gesture.
    HomeGesture,
    /// The compositor recognised a deliberate dismiss/close gesture.
    CloseGesture,
    /// An application asks to leave the foreground (its own Back control).
    YieldForeground,
}

/// One line from a client: either shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Inbound {
    Request(Request),
    Report(Report),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "kebab-case")]
pub enum Reply {
    Hello {
        schema_version: u32,
        evidence: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        home: Option<AppId>,
    },
    /// Result of `Identify`. The identity fields are flattened so the wire
    /// form is exactly
    /// `{"reply":"identity","schema_version":1,"app":"org.ferrix.phone"|null,"trust":"untrusted"|null,"granted":["telephony-call",...]}`.
    Identity {
        schema_version: u32,
        #[serde(flatten)]
        identity: AppIdentity,
    },
    Catalog {
        schema_version: u32,
        apps: Vec<AppDescriptor>,
    },
    Instances {
        schema_version: u32,
        instances: Vec<AppInstance>,
    },
    /// Result of `AppIcon`. `icon` is `null` when the app is unknown, not
    /// installed, or its runtime has no usable icon; the caller then draws
    /// its letter glyph.
    AppIcon {
        schema_version: u32,
        id: AppId,
        #[serde(default)]
        icon: Option<AppIconBitmap>,
    },
    /// Result of `Launch`/`Activate`: the (possibly pre-existing) instance.
    Instance {
        schema_version: u32,
        instance: AppInstance,
        /// True when `Launch` found the app already running and activated it.
        activated_existing: bool,
    },
    Ok,
    Error {
        error: AppError,
    },
    ProtocolError {
        reason: String,
    },
}

/// Pushed to subscribed streams.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "notification", rename_all = "kebab-case")]
pub enum Notification {
    /// A lifecycle or catalog event.
    App { event: AppEvent },
    /// Compositor directive: raise the toplevel(s) of the client with `pid`;
    /// with `surface`, exactly that toplevel (revealing it when withheld).
    Activate {
        pid: u32,
        app_id: AppId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<u64>,
    },
    /// Compositor directive: never present this toplevel until an
    /// `Activate` names it. Used for shared-runtime windows that belong to
    /// no running application.
    Withhold { pid: u32, surface: u64 },
    /// Compositor directive: ask this toplevel to close (`xdg_toplevel.close`).
    Close { pid: u32, surface: u64 },
    /// An intent for an already running handler. Sent only to the streams
    /// of the handler application itself.
    Intent { app_id: AppId, intent: AppIntent },
}

fn connect_stream(path: &Path, timeout: Duration) -> Result<UnixStream, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "ferrix-appd socket {} unavailable: {error}",
                    path.display()
                ));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

pub fn write_line(stream: &mut UnixStream, value: &impl Serialize) -> Result<(), String> {
    let mut text = serde_json::to_string(value).map_err(|e| format!("encode: {e}"))?;
    text.push('\n');
    stream
        .write_all(text.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|e| format!("write ferrix-appd message: {e}"))
}

fn read_reply(reader: &mut BufReader<UnixStream>) -> Result<Reply, String> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|e| format!("read ferrix-appd reply: {e}"))?;
    if bytes == 0 {
        return Err("ferrix-appd closed before replying".to_owned());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("decode ferrix-appd reply: {e}"))
}

/// A request/reply connection. Not subscribed; every request gets a reply.
pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
}

impl Client {
    /// Connects and identifies with `role`. Fails on a schema mismatch.
    pub fn connect(path: &Path, role: ClientRole, timeout: Duration) -> Result<Self, String> {
        let stream = connect_stream(path, timeout)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("set ferrix-appd timeout: {e}"))?;
        let writer = stream
            .try_clone()
            .map_err(|e| format!("ferrix-appd socket clone: {e}"))?;
        let mut client = Self {
            writer,
            reader: BufReader::new(stream),
            socket: path.to_path_buf(),
        };
        match client.request(&Request::Hello { role })? {
            Reply::Hello { schema_version, .. } if schema_version == SCHEMA_VERSION => Ok(client),
            Reply::Hello { schema_version, .. } => Err(format!(
                "ferrix-appd schema {schema_version} is incompatible with client schema {SCHEMA_VERSION}"
            )),
            other => Err(format!("unexpected ferrix-appd hello reply: {other:?}")),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn request(&mut self, request: &Request) -> Result<Reply, String> {
        write_line(&mut self.writer, request)?;
        read_reply(&mut self.reader)
    }

    /// Sends a report; nothing comes back.
    pub fn report(&mut self, report: &Report) -> Result<(), String> {
        write_line(&mut self.writer, report)
    }

    pub fn catalog(&mut self) -> Result<Vec<AppDescriptor>, String> {
        match self.request(&Request::Catalog)? {
            Reply::Catalog { apps, .. } => Ok(apps),
            Reply::Error { error } => Err(error.to_string()),
            other => Err(format!("unexpected ferrix-appd catalog reply: {other:?}")),
        }
    }

    /// The installed app's launcher bitmap (see [`Request::AppIcon`]).
    /// `Ok(None)` when there is none; an `Err` also covers an older service
    /// that does not know the request.
    pub fn app_icon(&mut self, id: &AppId) -> Result<Option<AppIconBitmap>, String> {
        match self.request(&Request::AppIcon { id: id.clone() })? {
            Reply::AppIcon { id: got, icon, .. } if &got == id => Ok(icon),
            Reply::Error { error } => Err(error.to_string()),
            other => Err(format!("unexpected ferrix-appd app-icon reply: {other:?}")),
        }
    }

    pub fn instances(&mut self) -> Result<Vec<AppInstance>, String> {
        match self.request(&Request::Instances)? {
            Reply::Instances { instances, .. } => Ok(instances),
            Reply::Error { error } => Err(error.to_string()),
            other => Err(format!("unexpected ferrix-appd instances reply: {other:?}")),
        }
    }

    /// Resolves a peer pid to its application identity (`service` and
    /// `tool` roles). An unknown or exited pid yields
    /// [`AppIdentity::unknown`]; a role refusal is an `Err`.
    pub fn identify(&mut self, pid: u32) -> Result<AppIdentity, String> {
        match self.request(&Request::Identify { pid })? {
            Reply::Identity { identity, .. } => Ok(identity),
            Reply::Error { error } => Err(error.to_string()),
            Reply::ProtocolError { reason } => Err(format!("identify refused: {reason}")),
            other => Err(format!("unexpected ferrix-appd identify reply: {other:?}")),
        }
    }

    /// Launches or activates; typed errors come back as `Err(AppError)`.
    pub fn launch(&mut self, id: &AppId) -> Result<(AppInstance, bool), Result<AppError, String>> {
        match self
            .request(&Request::Launch { id: id.clone() })
            .map_err(Err)?
        {
            Reply::Instance {
                instance,
                activated_existing,
                ..
            } => Ok((instance, activated_existing)),
            Reply::Error { error } => Err(Ok(error)),
            other => Err(Err(format!(
                "unexpected ferrix-appd launch reply: {other:?}"
            ))),
        }
    }

    /// Opens the handler of `intent` (see [`Request::Open`]); typed errors
    /// (`no-handler`, `invalid-intent`) come back as `Err(Ok(AppError))`.
    pub fn open(
        &mut self,
        intent: &AppIntent,
    ) -> Result<(AppInstance, bool), Result<AppError, String>> {
        match self
            .request(&Request::Open {
                intent: intent.clone(),
            })
            .map_err(Err)?
        {
            Reply::Instance {
                instance,
                activated_existing,
                ..
            } => Ok((instance, activated_existing)),
            Reply::Error { error } => Err(Ok(error)),
            Reply::ProtocolError { reason } => Err(Err(reason)),
            other => Err(Err(format!("unexpected ferrix-appd open reply: {other:?}"))),
        }
    }

    /// Switches this connection into a subscription. The hello reply's
    /// `home` and the initial catalog/instances are fetched first so the
    /// subscriber starts from a consistent snapshot.
    pub fn subscribe(mut self) -> Result<Subscription, String> {
        let apps = self.catalog()?;
        let instances = self.instances()?;
        match self.request(&Request::Subscribe)? {
            Reply::Ok => {}
            Reply::Error { error } => return Err(error.to_string()),
            other => return Err(format!("unexpected ferrix-appd subscribe reply: {other:?}")),
        }
        self.reader
            .get_ref()
            .set_read_timeout(None)
            .map_err(|e| format!("clear ferrix-appd timeout: {e}"))?;
        Ok(Subscription {
            apps,
            instances,
            writer: self.writer,
            reader: self.reader,
            socket: self.socket,
        })
    }
}

/// A subscribed stream: pushed [`Notification`]s plus a writer for reports.
pub struct Subscription {
    pub apps: Vec<AppDescriptor>,
    pub instances: Vec<AppInstance>,
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
}

/// Write half of a subscription, usable from any thread for [`Report`]s.
#[derive(Clone)]
pub struct Reporter {
    writer: Arc<std::sync::Mutex<UnixStream>>,
}

impl Reporter {
    pub fn report(&self, report: &Report) -> Result<(), String> {
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        write_line(&mut writer, report)
    }
}

impl Subscription {
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Starts the notification reader. The channel closes when `ferrix_appd`
    /// disconnects. Returns the receiver and a reporter for the write half.
    pub fn spawn(self, wake: Arc<dyn Fn() + Send + Sync>) -> (Receiver<Notification>, Reporter) {
        let (tx, rx) = mpsc::channel();
        let mut lines = self.reader.lines();
        thread::Builder::new()
            .name("ferrix-appd-notifications".to_owned())
            .spawn(move || {
                while let Some(Ok(line)) = lines.next() {
                    let text = line.trim();
                    if text.is_empty() {
                        continue;
                    }
                    if let Ok(notification) = serde_json::from_str::<Notification>(text) {
                        if tx.send(notification).is_err() {
                            return;
                        }
                        wake();
                    }
                }
                drop(tx);
                wake();
            })
            .expect("spawn ferrix-appd notification reader");
        (
            rx,
            Reporter {
                writer: Arc::new(std::sync::Mutex::new(self.writer)),
            },
        )
    }
}

/// Follows the intents `ferrix_appd` pushes to this application while it
/// runs: connects with role `application`, subscribes, and forwards only
/// [`Notification::Intent`] payloads. The channel closes with the stream
/// (an application does not outlive `ferrix_appd`).
pub fn spawn_intent_listener(
    socket: &Path,
    timeout: Duration,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Result<Receiver<AppIntent>, String> {
    let subscription = Client::connect(socket, ClientRole::Application, timeout)?.subscribe()?;
    let (notifications, _reporter) = subscription.spawn(Arc::new(|| {}));
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("ferrix-appd-intents".to_owned())
        .spawn(move || {
            for notification in notifications {
                if let Some(intent) = intent_of(notification) {
                    if tx.send(intent).is_err() {
                        return;
                    }
                    wake();
                }
            }
        })
        .map_err(|e| format!("spawn intent listener: {e}"))?;
    Ok(rx)
}

/// Where an application's intents arrive: the one it was launched with
/// ([`AppIntent::from_env`]) and, while it runs, the ones `ferrix_appd`
/// pushes ([`spawn_intent_listener`]). Shared by every intent handler.
pub struct IntentInbox {
    launch: Option<AppIntent>,
    running: Option<Receiver<AppIntent>>,
    /// Why the running listener could not start, when it could not.
    pub unavailable: Option<String>,
}

impl IntentInbox {
    /// Reads the launch intent and, when `socket` is known, subscribes for
    /// running ones. A missing socket (a nested run without `ferrix_appd`)
    /// is not an error: only the launch intent applies then.
    pub fn start(
        socket: Option<&Path>,
        timeout: Duration,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (running, unavailable) = match socket {
            Some(socket) => match spawn_intent_listener(socket, timeout, wake) {
                Ok(rx) => (Some(rx), None),
                Err(reason) => (None, Some(reason)),
            },
            None => (None, None),
        };
        Self {
            launch: AppIntent::from_env(),
            running,
            unavailable,
        }
    }

    /// The launch intent, once.
    pub fn take_launch(&mut self) -> Option<AppIntent> {
        self.launch.take()
    }

    /// Intents pushed since the last call.
    pub fn drain(&self) -> Vec<AppIntent> {
        self.running
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default()
    }

    /// Feeds the launch intent (first call only) and every pushed intent to
    /// `apply`, emitting one `app-intent-received` record line per intent
    /// through `emit` (kind and delivery only, never the number) and, the
    /// first time, `app-intents-unavailable` when the listener could not
    /// start. Returns whether `apply` reported a change.
    pub fn dispatch(
        &mut self,
        evidence: &str,
        app: &str,
        mut emit: impl FnMut(String),
        mut apply: impl FnMut(&AppIntent) -> bool,
    ) -> bool {
        if let Some(reason) = self.unavailable.take() {
            emit(format!(
                "{{\"evidence\":{},\"event\":\"app-intents-unavailable\",\"app\":{},\"reason\":{}}}",
                quoted(evidence),
                quoted(app),
                quoted(&reason)
            ));
        }
        let launch = self.take_launch().map(|i| ("launch", i));
        let running = self.drain().into_iter().map(|i| ("running", i));
        let mut changed = false;
        for (delivery, intent) in launch.into_iter().chain(running) {
            emit(format!(
                "{{\"evidence\":{},\"event\":\"app-intent-received\",\"app\":{},\"kind\":\"{}\",\"delivery\":\"{delivery}\"}}",
                quoted(evidence),
                quoted(app),
                intent.kind().name()
            ));
            changed |= apply(&intent);
        }
        changed
    }
}

/// A JSON string literal (records keep `evidence` first, so they are
/// formatted rather than built from a map).
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The validated intent carried by a notification, if any.
pub fn intent_of(notification: Notification) -> Option<AppIntent> {
    match notification {
        Notification::Intent { intent, .. } if intent.validate().is_ok() => Some(intent),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_request_and_reply_wire_shapes() {
        let id = AppId::new("apk.org.example.notes").unwrap();
        let request = Request::AppIcon { id: id.clone() };
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(
            text,
            r#"{"request":"app-icon","id":"apk.org.example.notes"}"#
        );
        assert_eq!(
            serde_json::from_str::<Inbound>(&text).unwrap(),
            Inbound::Request(request)
        );
        let reply = Reply::AppIcon {
            schema_version: SCHEMA_VERSION,
            id: id.clone(),
            icon: Some(AppIconBitmap::new(1, 1, vec![1, 2, 3, 4]).unwrap()),
        };
        let text = serde_json::to_string(&reply).unwrap();
        assert_eq!(
            text,
            r#"{"reply":"app-icon","schema_version":1,"id":"apk.org.example.notes","icon":{"width":1,"height":1,"rgba":"AQIDBA=="}}"#
        );
        assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), reply);
        let none: Reply = serde_json::from_str(
            r#"{"reply":"app-icon","schema_version":1,"id":"apk.org.example.notes","icon":null}"#,
        )
        .unwrap();
        assert!(matches!(none, Reply::AppIcon { icon: None, .. }));
        // An out-of-bounds bitmap fails the whole reply.
        assert!(
            serde_json::from_str::<Reply>(
                r#"{"reply":"app-icon","schema_version":1,"id":"apk.org.example.notes","icon":{"width":500,"height":1,"rgba":""}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn requests_and_reports_have_distinct_wire_shapes() {
        let launch = Request::Launch {
            id: AppId::new("org.ferrix.terminal").unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&launch).unwrap(),
            r#"{"request":"launch","id":"org.ferrix.terminal"}"#
        );
        let mapped = Report::SurfaceMapped {
            pid: 42,
            app_id: Some("org.ferrix.terminal".to_owned()),
            title: None,
            surface: None,
        };
        assert_eq!(
            serde_json::to_string(&mapped).unwrap(),
            r#"{"report":"surface-mapped","pid":42,"app_id":"org.ferrix.terminal"}"#
        );
        assert_eq!(
            serde_json::from_str::<Inbound>(r#"{"request":"home"}"#).unwrap(),
            Inbound::Request(Request::Home)
        );
        assert_eq!(
            serde_json::from_str::<Inbound>(r#"{"report":"focus-changed"}"#).unwrap(),
            Inbound::Report(Report::FocusChanged {
                pid: None,
                surface: None
            })
        );
        // Per-toplevel ids are optional on the wire: an older compositor's
        // pid-only report still parses.
        assert_eq!(
            serde_json::to_string(&Report::SurfaceUnmapped {
                pid: 9,
                surface: Some(3)
            })
            .unwrap(),
            r#"{"report":"surface-unmapped","pid":9,"surface":3}"#
        );
        assert_eq!(
            serde_json::from_str::<Inbound>(r#"{"report":"surface-unmapped","pid":9}"#).unwrap(),
            Inbound::Report(Report::SurfaceUnmapped {
                pid: 9,
                surface: None
            })
        );
        assert_eq!(
            serde_json::to_string(&Report::ActivationRequested { pid: 9, surface: 3 }).unwrap(),
            r#"{"report":"activation-requested","pid":9,"surface":3}"#
        );
        assert_eq!(
            serde_json::to_string(&Report::CloseGesture).unwrap(),
            r#"{"report":"close-gesture"}"#
        );
        assert!(serde_json::from_str::<Inbound>(r#"{"bogus":1}"#).is_err());
    }

    #[test]
    fn notifications_are_tagged() {
        let text = serde_json::to_string(&Notification::Activate {
            pid: 7,
            app_id: AppId::new("org.ferrix.home").unwrap(),
            surface: None,
        })
        .unwrap();
        assert_eq!(
            text,
            r#"{"notification":"activate","pid":7,"app_id":"org.ferrix.home"}"#
        );
        assert_eq!(
            serde_json::to_string(&Notification::Withhold { pid: 7, surface: 2 }).unwrap(),
            r#"{"notification":"withhold","pid":7,"surface":2}"#
        );
        assert_eq!(
            serde_json::to_string(&Notification::Close { pid: 7, surface: 2 }).unwrap(),
            r#"{"notification":"close","pid":7,"surface":2}"#
        );
        let event = Notification::App {
            event: AppEvent::CatalogChanged { apps: 3 },
        };
        let text = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<Notification>(&text).unwrap(), event);
    }

    #[test]
    fn identify_and_identity_have_the_documented_wire_forms() {
        use ferrix_app_api::{AppCapability, TrustLevel};

        let request = Request::Identify { pid: 4242 };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"request":"identify","pid":4242}"#
        );
        assert_eq!(
            serde_json::from_str::<Inbound>(r#"{"request":"identify","pid":4242}"#).unwrap(),
            Inbound::Request(request)
        );
        assert!(serde_json::from_str::<Inbound>(r#"{"request":"identify"}"#).is_err());

        let known = Reply::Identity {
            schema_version: SCHEMA_VERSION,
            identity: AppIdentity {
                app: Some(AppId::new("org.ferrix.phone").unwrap()),
                trust: Some(TrustLevel::Untrusted),
                granted: [AppCapability::TelephonyCall, AppCapability::TelephonyState]
                    .into_iter()
                    .collect(),
            },
        };
        let text = serde_json::to_string(&known).unwrap();
        assert_eq!(
            text,
            r#"{"reply":"identity","schema_version":1,"app":"org.ferrix.phone","trust":"untrusted","granted":["telephony-state","telephony-call"]}"#
        );
        assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), known);

        let unknown = Reply::Identity {
            schema_version: SCHEMA_VERSION,
            identity: AppIdentity::unknown(),
        };
        let text = serde_json::to_string(&unknown).unwrap();
        assert_eq!(
            text,
            r#"{"reply":"identity","schema_version":1,"app":null,"trust":null,"granted":[]}"#
        );
        assert_eq!(serde_json::from_str::<Reply>(&text).unwrap(), unknown);

        assert_eq!(
            serde_json::to_string(&Request::Hello {
                role: ClientRole::Service
            })
            .unwrap(),
            r#"{"request":"hello","role":"service"}"#
        );
    }

    #[test]
    fn open_and_intent_have_documented_wire_forms() {
        let open = Request::Open {
            intent: AppIntent::Dial {
                number: "+15550100042".to_owned(),
            },
        };
        let text = serde_json::to_string(&open).unwrap();
        assert_eq!(
            text,
            r#"{"request":"open","intent":{"intent":"dial","number":"+15550100042"}}"#
        );
        assert_eq!(
            serde_json::from_str::<Inbound>(
                r#"{"request":"open","intent":{"intent":"show-call"}}"#
            )
            .unwrap(),
            Inbound::Request(Request::Open {
                intent: AppIntent::ShowCall
            })
        );
        let pushed = Notification::Intent {
            app_id: AppId::new("org.ferrix.phone").unwrap(),
            intent: AppIntent::ShowCall,
        };
        let text = serde_json::to_string(&pushed).unwrap();
        assert_eq!(
            text,
            r#"{"notification":"intent","app_id":"org.ferrix.phone","intent":{"intent":"show-call"}}"#
        );
        assert_eq!(intent_of(pushed), Some(AppIntent::ShowCall));
        let bad = Notification::Intent {
            app_id: AppId::new("org.ferrix.phone").unwrap(),
            intent: AppIntent::Dial {
                number: "x".to_owned(),
            },
        };
        assert_eq!(intent_of(bad), None, "invalid payloads are dropped");
        assert_eq!(
            intent_of(Notification::App {
                event: AppEvent::CatalogChanged { apps: 1 }
            }),
            None
        );
    }

    #[test]
    fn inbox_dispatch_records_kind_and_delivery_never_the_number() {
        let (tx, rx) = mpsc::channel();
        tx.send(AppIntent::Dial {
            number: "+15550100042".to_owned(),
        })
        .unwrap();
        let mut inbox = IntentInbox {
            launch: Some(AppIntent::ShowCall),
            running: Some(rx),
            unavailable: None,
        };
        let mut lines = Vec::new();
        let mut seen = Vec::new();
        let changed = inbox.dispatch(
            "mock",
            "org.ferrix.phone",
            |l| lines.push(l),
            |i| {
                seen.push(i.kind());
                true
            },
        );
        assert!(changed);
        assert_eq!(
            seen,
            [
                ferrix_app_api::IntentKind::ShowCall,
                ferrix_app_api::IntentKind::Dial
            ]
        );
        assert_eq!(
            lines[0],
            r#"{"evidence":"mock","event":"app-intent-received","app":"org.ferrix.phone","kind":"show-call","delivery":"launch"}"#
        );
        assert!(lines[1].contains(r#""delivery":"running""#));
        assert!(lines.iter().all(|l| !l.contains("5550100042")));
        // The launch intent applies once.
        assert!(!inbox.dispatch("mock", "x", |_| {}, |_| true));
    }

    #[test]
    fn error_replies_carry_typed_errors() {
        let reply = Reply::Error {
            error: AppError::UnknownApp {
                id: AppId::new("org.ferrix.nope").unwrap(),
            },
        };
        assert_eq!(
            serde_json::to_string(&reply).unwrap(),
            r#"{"reply":"error","error":{"kind":"unknown-app","id":"org.ferrix.nope"}}"#
        );
    }
}
