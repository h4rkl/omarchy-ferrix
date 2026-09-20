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
    AppDescriptor, AppError, AppEvent, AppId, AppIdentity, AppInstance, SCHEMA_VERSION,
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
}

/// Fire-and-forget messages from the compositor (and from applications
/// yielding the foreground). Never replied to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "report", rename_all = "kebab-case")]
pub enum Report {
    /// A toplevel committed its first buffer. `pid` is the client's process
    /// id as seen by the compositor; `app_id` is the untrusted xdg app id.
    SurfaceMapped {
        pid: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// The last toplevel of that client went away.
    SurfaceUnmapped { pid: u32 },
    /// Keyboard/visual focus moved to the client with `pid` (or to nothing).
    FocusChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
    },
    /// The compositor recognised the global home gesture.
    HomeGesture,
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
    /// Compositor directive: raise the toplevel(s) of the client with `pid`.
    Activate { pid: u32, app_id: AppId },
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

#[cfg(test)]
mod tests {
    use super::*;

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
            Inbound::Report(Report::FocusChanged { pid: None })
        );
        assert!(serde_json::from_str::<Inbound>(r#"{"bogus":1}"#).is_err());
    }

    #[test]
    fn notifications_are_tagged() {
        let text = serde_json::to_string(&Notification::Activate {
            pid: 7,
            app_id: AppId::new("org.ferrix.home").unwrap(),
        })
        .unwrap();
        assert_eq!(
            text,
            r#"{"notification":"activate","pid":7,"app_id":"org.ferrix.home"}"#
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
