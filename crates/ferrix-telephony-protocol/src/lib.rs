//! Wire contract and Unix-socket client for `ferrix-telephonyd`.
//!
//! Line-delimited JSON, schema-versioned through
//! [`ferrix_telephony_api::SCHEMA_VERSION`]. A [`Request`] (`{"request": ...}`)
//! always receives exactly one [`Reply`]; after `subscribe` the stream also
//! carries pushed [`Event`]s (`{"event": ...}`). Every request type rejects
//! unknown fields; a malformed line gets `{"reply":"protocol-error"}` and
//! never mutates state.
//!
//! Callers are identified by the daemon through their kernel peer pid (see
//! ADR-004), never by anything claimed in a payload; `client_kind` in the
//! hello is a diagnostic hint only.
//!
//! Clients with only `telephony-state` receive redacted forms: the daemon
//! serves [`Reply::State`] and [`Event`]s built from
//! `RedactedTelephonyState`/`RedactedTelephonyEvent`. Both forms share the
//! same tags, so a subscriber folds them with the matching `apply_to`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_telephony_api::{
    AudioRoute, Call, CallId, DataProfile, Dtmf, MessageId, RadioPower, RedactedTelephonyEvent,
    RedactedTelephonyState, SCHEMA_VERSION, Sms, TelephonyError, TelephonyEvent, TelephonyState,
};
use serde::{Deserialize, Serialize};

/// Default socket path inside the guest.
pub const DEFAULT_SOCKET: &str = "/run/ferrix/telephonyd.sock";
/// Environment variable overriding the socket path.
pub const SOCKET_ENV: &str = "FERRIX_TELEPHONYD_SOCKET";

/// Resolves the socket path: `FERRIX_TELEPHONYD_SOCKET` or the default.
pub fn socket_path() -> PathBuf {
    std::env::var(SOCKET_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
}

/// Diagnostic hint about the caller; never used for authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    Application,
    Home,
    Tool,
}

/// Filter for `list-sms`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmsFilter {
    /// Only messages exchanged with this party.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party: Option<ferrix_telephony_api::E164>,
    /// Only unread (`received`) messages.
    #[serde(default)]
    pub unread_only: bool,
    /// Maximum number of messages, newest first. `0` means no limit.
    #[serde(default)]
    pub limit: usize,
}

/// Messages that receive exactly one [`Reply`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    /// Must be first. The reply carries the daemon schema and a snapshot.
    Hello {
        schema_version: u32,
        client_kind: ClientKind,
    },
    /// Full snapshot (`telephony-state`). An empty struct variant (not a
    /// unit variant) so `deny_unknown_fields` is enforced by serde.
    State {},
    /// Receive [`Event`]s on this stream from now on (`telephony-state`).
    Subscribe {},
    // --- telephony-call ---
    Dial {
        number: String,
    },
    Answer {
        call_id: CallId,
    },
    Reject {
        call_id: CallId,
    },
    Hangup {
        call_id: CallId,
    },
    Hold {
        call_id: CallId,
    },
    Resume {
        call_id: CallId,
    },
    Dtmf {
        call_id: CallId,
        digit: Dtmf,
    },
    SetAudioRoute {
        route: AudioRoute,
    },
    // --- telephony-sms ---
    SendSms {
        to: String,
        body: String,
    },
    ListSms {
        #[serde(default)]
        filter: SmsFilter,
    },
    MarkRead {
        id: MessageId,
    },
    DeleteSms {
        id: MessageId,
    },
    // --- telephony-data ---
    ConnectData {
        #[serde(flatten)]
        profile: DataProfile,
    },
    DisconnectData {},
    // --- telephony-call (radio) / telephony-call (sim) ---
    SetRadioPower {
        power: RadioPower,
    },
    EnterPin {
        pin: String,
    },
}

impl Request {
    pub fn name(&self) -> &'static str {
        match self {
            Request::Hello { .. } => "hello",
            Request::State {} => "state",
            Request::Subscribe {} => "subscribe",
            Request::Dial { .. } => "dial",
            Request::Answer { .. } => "answer",
            Request::Reject { .. } => "reject",
            Request::Hangup { .. } => "hangup",
            Request::Hold { .. } => "hold",
            Request::Resume { .. } => "resume",
            Request::Dtmf { .. } => "dtmf",
            Request::SetAudioRoute { .. } => "set-audio-route",
            Request::SendSms { .. } => "send-sms",
            Request::ListSms { .. } => "list-sms",
            Request::MarkRead { .. } => "mark-read",
            Request::DeleteSms { .. } => "delete-sms",
            Request::ConnectData { .. } => "connect-data",
            Request::DisconnectData {} => "disconnect-data",
            Request::SetRadioPower { .. } => "set-radio-power",
            Request::EnterPin { .. } => "enter-pin",
        }
    }
}

/// A snapshot in either form; the daemon picks by capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StateView {
    Full(TelephonyState),
    Redacted(RedactedTelephonyState),
}

impl StateView {
    /// The redacted projection, whichever form was received.
    pub fn redacted(&self) -> RedactedTelephonyState {
        match self {
            StateView::Full(s) => s.redacted(),
            StateView::Redacted(s) => s.clone(),
        }
    }

    pub fn schema_version(&self) -> u32 {
        match self {
            StateView::Full(s) => s.schema_version,
            StateView::Redacted(s) => s.schema_version,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "kebab-case")]
pub enum Reply {
    Hello {
        schema_version: u32,
        evidence: String,
        state: StateView,
    },
    State {
        state: StateView,
    },
    Ok,
    Call {
        call: Call,
    },
    Sms {
        sms: Sms,
    },
    SmsList {
        messages: Vec<Sms>,
        total: usize,
    },
    Error {
        error: TelephonyError,
    },
    ProtocolError {
        reason: String,
    },
}

/// Pushed to subscribed streams. Full and redacted events share tags, so
/// the wire form is a [`TelephonyEvent`] or a [`RedactedTelephonyEvent`]
/// depending on the subscriber's capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Event {
    Full(TelephonyEvent),
    Redacted(RedactedTelephonyEvent),
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::Full(e) => e.name(),
            Event::Redacted(e) => e.name(),
        }
    }

    /// Folds into a redacted local snapshot regardless of the form received.
    pub fn apply_redacted(&self, state: &mut RedactedTelephonyState) {
        match self {
            Event::Full(e) => e.redacted().apply_to(state),
            Event::Redacted(e) => e.apply_to(state),
        }
    }
}

/// One inbound line on a subscribed stream: a reply or a pushed event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Inbound {
    Reply(Reply),
    Event(Event),
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

fn connect_stream(path: &Path, timeout: Duration) -> Result<UnixStream, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "ferrix-telephonyd socket {} unavailable: {error}",
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
        .map_err(|e| format!("write ferrix-telephonyd message: {e}"))
}

fn read_reply(reader: &mut BufReader<UnixStream>) -> Result<Reply, String> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|e| format!("read ferrix-telephonyd reply: {e}"))?;
    if bytes == 0 {
        return Err("ferrix-telephonyd closed before replying".to_owned());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("decode ferrix-telephonyd reply: {e}"))
}

/// A request/reply connection. Not subscribed; every request gets a reply.
pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
    hello_state: StateView,
    evidence: String,
}

impl Client {
    /// Connects and sends the hello. Fails on a schema mismatch.
    pub fn connect(path: &Path, kind: ClientKind, timeout: Duration) -> Result<Self, String> {
        let stream = connect_stream(path, timeout)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("set ferrix-telephonyd timeout: {e}"))?;
        let writer = stream
            .try_clone()
            .map_err(|e| format!("ferrix-telephonyd socket clone: {e}"))?;
        let mut client = Self {
            writer,
            reader: BufReader::new(stream),
            socket: path.to_path_buf(),
            hello_state: StateView::Redacted(RedactedTelephonyState::default()),
            evidence: String::new(),
        };
        match client.request(&Request::Hello {
            schema_version: SCHEMA_VERSION,
            client_kind: kind,
        })? {
            Reply::Hello {
                schema_version,
                evidence,
                state,
            } if schema_version == SCHEMA_VERSION => {
                client.hello_state = state;
                client.evidence = evidence;
                Ok(client)
            }
            Reply::Hello { schema_version, .. } => Err(format!(
                "ferrix-telephonyd schema {schema_version} is incompatible with client schema {SCHEMA_VERSION}"
            )),
            Reply::ProtocolError { reason } => {
                Err(format!("ferrix-telephonyd rejected hello: {reason}"))
            }
            other => Err(format!(
                "unexpected ferrix-telephonyd hello reply: {other:?}"
            )),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Snapshot delivered with the hello reply.
    pub fn hello_state(&self) -> &StateView {
        &self.hello_state
    }

    /// Evidence label the daemon reported (`QEMU` or `mock`).
    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    pub fn request(&mut self, request: &Request) -> Result<Reply, String> {
        write_line(&mut self.writer, request)?;
        read_reply(&mut self.reader)
    }

    /// A request whose success is `Ok`; typed errors come back as `Err(Ok(e))`.
    pub fn expect_ok(&mut self, request: &Request) -> Result<(), Result<TelephonyError, String>> {
        match self.request(request).map_err(Err)? {
            Reply::Ok => Ok(()),
            Reply::Error { error } => Err(Ok(error)),
            other => Err(Err(format!(
                "unexpected reply to {}: {other:?}",
                request.name()
            ))),
        }
    }

    /// A request whose success is `Call`.
    pub fn expect_call(
        &mut self,
        request: &Request,
    ) -> Result<Call, Result<TelephonyError, String>> {
        match self.request(request).map_err(Err)? {
            Reply::Call { call } => Ok(call),
            Reply::Error { error } => Err(Ok(error)),
            other => Err(Err(format!(
                "unexpected reply to {}: {other:?}",
                request.name()
            ))),
        }
    }

    /// A request whose success is `Sms`.
    pub fn expect_sms(&mut self, request: &Request) -> Result<Sms, Result<TelephonyError, String>> {
        match self.request(request).map_err(Err)? {
            Reply::Sms { sms } => Ok(sms),
            Reply::Error { error } => Err(Ok(error)),
            other => Err(Err(format!(
                "unexpected reply to {}: {other:?}",
                request.name()
            ))),
        }
    }

    pub fn state(&mut self) -> Result<StateView, String> {
        match self.request(&Request::State {})? {
            Reply::State { state } => Ok(state),
            Reply::Error { error } => Err(error.to_string()),
            other => Err(format!(
                "unexpected ferrix-telephonyd state reply: {other:?}"
            )),
        }
    }

    pub fn dial(&mut self, number: &str) -> Result<Call, Result<TelephonyError, String>> {
        self.expect_call(&Request::Dial {
            number: number.to_owned(),
        })
    }

    pub fn send_sms(
        &mut self,
        to: &str,
        body: &str,
    ) -> Result<Sms, Result<TelephonyError, String>> {
        self.expect_sms(&Request::SendSms {
            to: to.to_owned(),
            body: body.to_owned(),
        })
    }

    /// Starts cellular data. An empty profile delegates APN selection to the
    /// backend (ModemManager normally uses its carrier configuration).
    pub fn connect_data(
        &mut self,
        profile: DataProfile,
    ) -> Result<(), Result<TelephonyError, String>> {
        self.expect_ok(&Request::ConnectData { profile })
    }

    pub fn disconnect_data(&mut self) -> Result<(), Result<TelephonyError, String>> {
        self.expect_ok(&Request::DisconnectData {})
    }

    pub fn list_sms(
        &mut self,
        filter: SmsFilter,
    ) -> Result<(Vec<Sms>, usize), Result<TelephonyError, String>> {
        match self.request(&Request::ListSms { filter }).map_err(Err)? {
            Reply::SmsList { messages, total } => Ok((messages, total)),
            Reply::Error { error } => Err(Ok(error)),
            other => Err(Err(format!("unexpected list-sms reply: {other:?}"))),
        }
    }

    /// Switches this connection into a subscription. A fresh snapshot is
    /// fetched first so the subscriber starts from a consistent state.
    pub fn subscribe(mut self) -> Result<Subscription, String> {
        let state = self.state()?;
        match self.request(&Request::Subscribe {})? {
            Reply::Ok => {}
            Reply::Error { error } => return Err(error.to_string()),
            other => {
                return Err(format!(
                    "unexpected ferrix-telephonyd subscribe reply: {other:?}"
                ));
            }
        }
        self.reader
            .get_ref()
            .set_read_timeout(None)
            .map_err(|e| format!("clear ferrix-telephonyd timeout: {e}"))?;
        Ok(Subscription {
            state,
            writer: self.writer,
            reader: self.reader,
            socket: self.socket,
        })
    }
}

/// A subscribed stream: the snapshot taken at subscribe time plus pushed
/// [`Event`]s.
pub struct Subscription {
    pub state: StateView,
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
}

impl Subscription {
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Starts the event reader. The channel closes when the daemon
    /// disconnects; `wake` is called after every delivery and on close.
    pub fn spawn(self, wake: Arc<dyn Fn() + Send + Sync>) -> Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        let mut lines = self.reader.lines();
        let _writer = self.writer;
        thread::Builder::new()
            .name("ferrix-telephonyd-events".to_owned())
            .spawn(move || {
                let _keep_open = _writer;
                while let Some(Ok(line)) = lines.next() {
                    let text = line.trim();
                    if text.is_empty() {
                        continue;
                    }
                    if let Ok(event) = serde_json::from_str::<Event>(text) {
                        if tx.send(event).is_err() {
                            return;
                        }
                        wake();
                    }
                }
                drop(tx);
                wake();
            })
            .expect("spawn ferrix-telephonyd event reader");
        rx
    }
}

// ---------------------------------------------------------------------------
// Reconnector
// ---------------------------------------------------------------------------

/// What a [`Reconnector`] hands to its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A (re)connection succeeded; replace local state with this snapshot.
    Snapshot(StateView),
    /// A pushed delta.
    Event(Event),
    /// The daemon went away; a reconnect is being attempted.
    Disconnected,
}

/// Bounded exponential backoff, `100ms..=2s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    current: Duration,
}

impl Backoff {
    pub const MIN: Duration = Duration::from_millis(100);
    pub const MAX: Duration = Duration::from_millis(2000);

    pub fn new() -> Self {
        Self { current: Self::MIN }
    }

    /// The delay to wait now; doubles for next time up to [`Backoff::MAX`].
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = (self.current * 2).min(Self::MAX);
        delay
    }

    pub fn reset(&mut self) {
        self.current = Self::MIN;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

/// Keeps a subscription alive across daemon restarts. On every (re)connect
/// the fresh snapshot is delivered first, then deltas, so the owner's
/// reconciliation is "replace local state with the snapshot".
pub struct Reconnector {
    socket: PathBuf,
    kind: ClientKind,
    timeout: Duration,
}

impl Reconnector {
    pub fn new(socket: PathBuf, kind: ClientKind, timeout: Duration) -> Self {
        Self {
            socket,
            kind,
            timeout,
        }
    }

    /// Runs forever on a background thread. Stops when the receiver is
    /// dropped.
    pub fn spawn(self, wake: Arc<dyn Fn() + Send + Sync>) -> Receiver<Update> {
        let (tx, rx) = mpsc::channel::<Update>();
        thread::Builder::new()
            .name("ferrix-telephonyd-reconnect".to_owned())
            .spawn(move || {
                let mut backoff = Backoff::new();
                loop {
                    match Client::connect(&self.socket, self.kind, self.timeout)
                        .and_then(Client::subscribe)
                    {
                        Ok(subscription) => {
                            backoff.reset();
                            if tx
                                .send(Update::Snapshot(subscription.state.clone()))
                                .is_err()
                            {
                                return;
                            }
                            wake();
                            // The inner reader must not wake the owner: its
                            // wake would fire before the forward below and
                            // the owner would drain an empty channel. Wake
                            // only after each event is in the outer channel.
                            let events = subscription.spawn(Arc::new(|| {}));
                            for event in events {
                                if tx.send(Update::Event(event)).is_err() {
                                    return;
                                }
                                wake();
                            }
                            if tx.send(Update::Disconnected).is_err() {
                                return;
                            }
                            wake();
                        }
                        Err(_) => {
                            // Not connected; keep trying until the owner drops us.
                        }
                    }
                    thread::sleep(backoff.next_delay());
                }
            })
            .expect("spawn ferrix-telephonyd reconnector");
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_telephony_api::{ModemState, TelephonyFeature};

    #[test]
    fn requests_have_kebab_wire_shapes() {
        assert_eq!(
            serde_json::to_string(&Request::Hello {
                schema_version: 1,
                client_kind: ClientKind::Application
            })
            .unwrap(),
            r#"{"request":"hello","schema_version":1,"client_kind":"application"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Dial {
                number: "+15550100042".into()
            })
            .unwrap(),
            r#"{"request":"dial","number":"+15550100042"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Answer {
                call_id: CallId::from("c1")
            })
            .unwrap(),
            r#"{"request":"answer","call_id":"c1"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Dtmf {
                call_id: CallId::from("c1"),
                digit: Dtmf::new('#').unwrap()
            })
            .unwrap(),
            r##"{"request":"dtmf","call_id":"c1","digit":"#"}"##
        );
        assert_eq!(
            serde_json::to_string(&Request::SetAudioRoute {
                route: AudioRoute::Speaker
            })
            .unwrap(),
            r#"{"request":"set-audio-route","route":"speaker"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::SendSms {
                to: "+15550100042".into(),
                body: "hi".into()
            })
            .unwrap(),
            r#"{"request":"send-sms","to":"+15550100042","body":"hi"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::SetRadioPower {
                power: RadioPower::Off
            })
            .unwrap(),
            r#"{"request":"set-radio-power","power":"off"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::ConnectData {
                profile: DataProfile {
                    apn: Some("internet".into()),
                }
            })
            .unwrap(),
            r#"{"request":"connect-data","apn":"internet"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::DisconnectData {}).unwrap(),
            r#"{"request":"disconnect-data"}"#
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"request":"connect-data"}"#).unwrap(),
            Request::ConnectData {
                profile: DataProfile::default()
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"request":"list-sms"}"#).unwrap(),
            Request::ListSms {
                filter: SmsFilter::default()
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(
                r#"{"request":"list-sms","filter":{"unread_only":true,"limit":5}}"#
            )
            .unwrap(),
            Request::ListSms {
                filter: SmsFilter {
                    party: None,
                    unread_only: true,
                    limit: 5
                }
            }
        );
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"request":"state"}"#).unwrap(),
            Request::State {}
        );
        assert_eq!(
            serde_json::to_string(&Request::Subscribe {}).unwrap(),
            r#"{"request":"subscribe"}"#
        );
    }

    #[test]
    fn unknown_fields_and_unknown_requests_are_rejected() {
        assert!(serde_json::from_str::<Request>(r#"{"request":"state","extra":1}"#).is_err());
        assert!(
            serde_json::from_str::<Request>(r#"{"request":"dial","number":"+1","app":"x"}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<Request>(r#"{"request":"reboot"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"bogus":1}"#).is_err());
        assert!(
            serde_json::from_str::<Request>(r#"{"request":"list-sms","filter":{"nope":1}}"#)
                .is_err()
        );
    }

    #[test]
    fn replies_and_events_are_tagged() {
        let reply = Reply::Error {
            error: TelephonyError::Unsupported {
                feature: TelephonyFeature::Emergency,
            },
        };
        assert_eq!(
            serde_json::to_string(&reply).unwrap(),
            r#"{"reply":"error","error":{"kind":"unsupported","feature":"emergency"}}"#
        );
        assert_eq!(
            serde_json::to_string(&Reply::Ok).unwrap(),
            r#"{"reply":"ok"}"#
        );
        let hello = Reply::Hello {
            schema_version: 1,
            evidence: "mock".into(),
            state: StateView::Full(TelephonyState::new()),
        };
        let text = serde_json::to_string(&hello).unwrap();
        assert!(
            text.starts_with(r#"{"reply":"hello","schema_version":1,"evidence":"mock","state":{"#)
        );
        let back: Reply = serde_json::from_str(&text).unwrap();
        assert!(matches!(
            back,
            Reply::Hello {
                schema_version: 1,
                ..
            }
        ));

        let event = Event::Full(TelephonyEvent::ModemChanged {
            modem: ModemState::Ready,
        });
        let text = serde_json::to_string(&event).unwrap();
        assert_eq!(
            text,
            r#"{"event":"modem-changed","modem":{"state":"ready"}}"#
        );
        let inbound: Inbound = serde_json::from_str(&text).unwrap();
        assert!(matches!(inbound, Inbound::Event(_)));
        let inbound: Inbound = serde_json::from_str(r#"{"reply":"ok"}"#).unwrap();
        assert_eq!(inbound, Inbound::Reply(Reply::Ok));
    }

    #[test]
    fn redacted_events_fold_into_a_redacted_snapshot() {
        let mut local = RedactedTelephonyState::default();
        let full = Event::Full(TelephonyEvent::ModemChanged {
            modem: ModemState::Ready,
        });
        full.apply_redacted(&mut local);
        assert_eq!(local.modem, ModemState::Ready);
        let text = serde_json::to_string(&TelephonyState::new().redacted()).unwrap();
        let view: StateView = serde_json::from_str(&text).unwrap();
        assert_eq!(view.schema_version(), SCHEMA_VERSION);
    }

    #[test]
    fn backoff_is_bounded() {
        let mut b = Backoff::new();
        assert_eq!(b.next_delay(), Duration::from_millis(100));
        assert_eq!(b.next_delay(), Duration::from_millis(200));
        assert_eq!(b.next_delay(), Duration::from_millis(400));
        assert_eq!(b.next_delay(), Duration::from_millis(800));
        assert_eq!(b.next_delay(), Duration::from_millis(1600));
        assert_eq!(b.next_delay(), Duration::from_millis(2000));
        assert_eq!(b.next_delay(), Duration::from_millis(2000));
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_millis(100));
    }

    /// A one-shot fake daemon speaking the wire form literally, so the
    /// client's hello check is exercised without the real service.
    fn fake_daemon(schema_version: u32) -> PathBuf {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!(
            "ferrix-telephony-protocol-{}-{schema_version}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut lines = BufReader::new(stream).lines();
            let line = lines.next().unwrap().unwrap();
            let request: Request = serde_json::from_str(&line).unwrap();
            assert!(matches!(request, Request::Hello { .. }));
            let reply = Reply::Hello {
                schema_version,
                evidence: "mock".into(),
                state: StateView::Full(TelephonyState::new()),
            };
            write_line(&mut writer, &reply).unwrap();
        });
        path
    }

    /// A fake daemon that answers hello/state/subscribe and then pushes
    /// `count` events back-to-back on the subscribed stream.
    fn fake_pushing_daemon(count: usize) -> PathBuf {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!(
            "ferrix-telephony-protocol-push-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut lines = BufReader::new(stream).lines();
            while let Some(Ok(line)) = lines.next() {
                let request: Request = serde_json::from_str(&line).unwrap();
                match request {
                    Request::Hello { .. } => write_line(
                        &mut writer,
                        &Reply::Hello {
                            schema_version: SCHEMA_VERSION,
                            evidence: "mock".into(),
                            state: StateView::Full(TelephonyState::new()),
                        },
                    )
                    .unwrap(),
                    Request::State {} => write_line(
                        &mut writer,
                        &Reply::State {
                            state: StateView::Full(TelephonyState::new()),
                        },
                    )
                    .unwrap(),
                    Request::Subscribe {} => {
                        write_line(&mut writer, &Reply::Ok).unwrap();
                        for i in 0..count {
                            let event = Event::Full(TelephonyEvent::SignalChanged {
                                signal: ferrix_telephony_api::Signal::bars(i as u8),
                            });
                            write_line(&mut writer, &event).unwrap();
                        }
                        // Keep the stream open so no Disconnected arrives.
                        thread::sleep(Duration::from_secs(5));
                        return;
                    }
                    _ => unreachable!(),
                }
            }
        });
        path
    }

    /// Regression: every forwarded event must be preceded into the outer
    /// channel by exactly the wake that announces it, so an owner draining
    /// the channel on each wake never leaves an event stranded.
    #[test]
    fn reconnector_wakes_after_every_forwarded_event() {
        use std::sync::Mutex;
        let path = fake_pushing_daemon(2);
        let reconnector = Reconnector::new(path, ClientKind::Tool, Duration::from_secs(2));
        // The owner model: on each wake, drain the receiver and count what
        // was observable. Shared so the wake closure can drain too.
        let seen: Arc<Mutex<(usize, Vec<Update>)>> = Arc::new(Mutex::new((0, Vec::new())));
        let rx_slot: Arc<Mutex<Option<Receiver<Update>>>> = Arc::new(Mutex::new(None));
        let wake = {
            let seen = Arc::clone(&seen);
            let rx_slot = Arc::clone(&rx_slot);
            Arc::new(move || {
                let mut guard = seen.lock().unwrap();
                guard.0 += 1;
                if let Some(rx) = rx_slot.lock().unwrap().as_ref() {
                    guard.1.extend(rx.try_iter());
                }
            }) as Arc<dyn Fn() + Send + Sync>
        };
        *rx_slot.lock().unwrap() = None;
        let rx = reconnector.spawn(wake);
        *rx_slot.lock().unwrap() = Some(rx);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let guard = seen.lock().unwrap();
            let events = guard
                .1
                .iter()
                .filter(|u| matches!(u, Update::Event(_)))
                .count();
            if events >= 2 {
                break;
            }
            drop(guard);
            assert!(Instant::now() < deadline, "events never observed at a wake");
            thread::sleep(Duration::from_millis(10));
        }
        // Nothing may be left in the channel unannounced.
        let leftover = rx_slot.lock().unwrap().as_ref().unwrap().try_iter().count();
        assert_eq!(leftover, 0, "events left unwoken");
        let guard = seen.lock().unwrap();
        assert!(matches!(guard.1[0], Update::Snapshot(_)));
        assert!(
            guard.0 >= 3,
            "one wake per snapshot and per event: {}",
            guard.0
        );
    }

    #[test]
    fn client_accepts_matching_schema_and_rejects_mismatch() {
        let path = fake_daemon(SCHEMA_VERSION);
        let client = Client::connect(&path, ClientKind::Tool, Duration::from_secs(2)).unwrap();
        assert_eq!(client.evidence(), "mock");
        assert_eq!(client.hello_state().schema_version(), SCHEMA_VERSION);
        drop(client);

        let path = fake_daemon(SCHEMA_VERSION + 1);
        let err = Client::connect(&path, ClientKind::Tool, Duration::from_secs(2))
            .err()
            .unwrap();
        assert!(err.contains("incompatible"), "{err}");
    }
}
