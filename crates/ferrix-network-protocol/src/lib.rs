//! Wire contract and Unix-socket client for the networking service.
//!
//! Line-delimited JSON, schema-versioned through
//! [`ferrix_network_api::SCHEMA_VERSION`]. A [`Request`] (`{"request": ...}`)
//! always receives exactly one [`Reply`] (`{"reply": ...}`); after
//! `subscribe` the stream carries pushed [`Event`]s (`{"event": ...}`) and
//! accepts no further requests.
//!
//! Callers are identified by the service from kernel peer credentials, never
//! by anything in a payload: `client_kind` is a hint for records only.
//! Passphrases travel only in `connect` and are never echoed back.

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_network_api::{
    ApId, NetworkError, NetworkEvent, NetworkState, Passphrase, ProfileId, SCHEMA_VERSION,
};
pub use ferrix_network_api::{DEFAULT_NETWORKD_SOCKET as DEFAULT_SOCKET, NETWORKD_SOCKET_ENV};
use serde::{Deserialize, Serialize};

/// Resolves the service socket: `FERRIX_NETWORKD_SOCKET` or the default.
pub fn socket_path() -> PathBuf {
    std::env::var(NETWORKD_SOCKET_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_SOCKET), PathBuf::from)
}

/// Informational: what kind of process opened the connection. Authorization
/// never depends on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    Application,
    Tool,
}

impl ClientKind {
    pub fn name(self) -> &'static str {
        match self {
            ClientKind::Application => "application",
            ClientKind::Tool => "tool",
        }
    }
}

/// Messages that receive exactly one [`Reply`]. Unknown fields are rejected.
///
/// Variants without payload are empty struct variants (`Request::Scan {}`)
/// so that `deny_unknown_fields` applies to them too: `{"request":"scan",
/// "x":1}` is a protocol error, not a scan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    /// Must be the first message. `schema_version` is checked by both ends.
    Hello {
        schema_version: u32,
        client_kind: ClientKind,
    },
    /// Full snapshot (`network-state`).
    State {},
    /// Switch this connection to a push stream (`network-state`). The reply
    /// is [`Reply::State`], the snapshot every later event applies to.
    Subscribe {},
    SetRadio {
        enabled: bool,
    },
    Scan {},
    /// Join a visible access point. `passphrase` is consumed by the backend
    /// and never stored by the service beyond the attempt.
    Connect {
        ap: ApId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<Passphrase>,
    },
    ConnectProfile {
        profile: ProfileId,
    },
    Disconnect {},
    Forget {
        profile: ProfileId,
    },
}

impl Request {
    pub fn name(&self) -> &'static str {
        match self {
            Request::Hello { .. } => "hello",
            Request::State {} => "state",
            Request::Subscribe {} => "subscribe",
            Request::SetRadio { .. } => "set-radio",
            Request::Scan {} => "scan",
            Request::Connect { .. } => "connect",
            Request::ConnectProfile { .. } => "connect-profile",
            Request::Disconnect {} => "disconnect",
            Request::Forget { .. } => "forget",
        }
    }

    /// Whether the request only reads state (`network-state`) as opposed to
    /// changing it (`network-control`). `hello` needs neither.
    pub fn is_control(&self) -> bool {
        matches!(
            self,
            Request::SetRadio { .. }
                | Request::Scan {}
                | Request::Connect { .. }
                | Request::ConnectProfile { .. }
                | Request::Disconnect {}
                | Request::Forget { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "kebab-case")]
pub enum Reply {
    /// Answer to `hello`. `state` is present only when the caller holds
    /// `network-state`; the handshake itself needs no capability.
    Hello {
        schema_version: u32,
        evidence: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<NetworkState>,
    },
    State {
        state: NetworkState,
    },
    Ok,
    Error {
        error: NetworkError,
    },
    ProtocolError {
        reason: String,
    },
}

/// Pushed to subscribed streams.
pub type Event = NetworkEvent;

/// Failures of the blocking client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    /// The socket could not be reached within the timeout.
    Unavailable { socket: PathBuf, reason: String },
    /// A read or write failed or the service closed the stream.
    Io { reason: String },
    /// The service speaks another schema.
    SchemaMismatch { service: u32, client: u32 },
    /// The service rejected a message or answered out of contract.
    Protocol { reason: String },
    /// A typed service error.
    Network(NetworkError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Unavailable { socket, reason } => {
                write!(
                    f,
                    "networkd socket {} unavailable: {reason}",
                    socket.display()
                )
            }
            ClientError::Io { reason } => write!(f, "networkd i/o: {reason}"),
            ClientError::SchemaMismatch { service, client } => write!(
                f,
                "networkd schema {service} is incompatible with client schema {client}"
            ),
            ClientError::Protocol { reason } => write!(f, "networkd protocol: {reason}"),
            ClientError::Network(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ClientError {}

fn connect_stream(path: &Path, timeout: Duration) -> Result<UnixStream, ClientError> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() >= deadline => {
                return Err(ClientError::Unavailable {
                    socket: path.to_path_buf(),
                    reason: error.to_string(),
                });
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Writes one JSON line.
pub fn write_line(stream: &mut UnixStream, value: &impl Serialize) -> Result<(), ClientError> {
    let mut text = serde_json::to_string(value).map_err(|e| ClientError::Io {
        reason: format!("encode: {e}"),
    })?;
    text.push('\n');
    stream
        .write_all(text.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|e| ClientError::Io {
            reason: format!("write: {e}"),
        })
}

fn read_reply(reader: &mut BufReader<UnixStream>) -> Result<Reply, ClientError> {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).map_err(|e| ClientError::Io {
        reason: format!("read: {e}"),
    })?;
    if bytes == 0 {
        return Err(ClientError::Io {
            reason: "networkd closed before replying".to_owned(),
        });
    }
    serde_json::from_str(line.trim()).map_err(|e| ClientError::Protocol {
        reason: format!("decode reply: {e}"),
    })
}

/// Turns a reply into the unit outcome of a control request.
fn expect_ok(reply: Reply) -> Result<(), ClientError> {
    match reply {
        Reply::Ok => Ok(()),
        Reply::Error { error } => Err(ClientError::Network(error)),
        Reply::ProtocolError { reason } => Err(ClientError::Protocol { reason }),
        other => Err(ClientError::Protocol {
            reason: format!("unexpected reply {other:?}"),
        }),
    }
}

/// A request/reply connection. Every request gets a reply.
pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
    evidence: String,
    hello_state: Option<NetworkState>,
}

impl Client {
    /// Connects and performs the schema handshake.
    pub fn connect(path: &Path, kind: ClientKind, timeout: Duration) -> Result<Self, ClientError> {
        let stream = connect_stream(path, timeout)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| ClientError::Io {
                reason: format!("set timeout: {e}"),
            })?;
        let writer = stream.try_clone().map_err(|e| ClientError::Io {
            reason: format!("clone socket: {e}"),
        })?;
        let mut client = Self {
            writer,
            reader: BufReader::new(stream),
            socket: path.to_path_buf(),
            evidence: String::new(),
            hello_state: None,
        };
        let hello = Request::Hello {
            schema_version: SCHEMA_VERSION,
            client_kind: kind,
        };
        match client.request(&hello)? {
            Reply::Hello {
                schema_version,
                evidence,
                state,
            } if schema_version == SCHEMA_VERSION => {
                client.evidence = evidence;
                client.hello_state = state;
                Ok(client)
            }
            Reply::Hello { schema_version, .. } => Err(ClientError::SchemaMismatch {
                service: schema_version,
                client: SCHEMA_VERSION,
            }),
            Reply::ProtocolError { reason } => Err(ClientError::Protocol { reason }),
            other => Err(ClientError::Protocol {
                reason: format!("unexpected hello reply {other:?}"),
            }),
        }
    }

    /// Connects to [`socket_path`].
    pub fn connect_default(kind: ClientKind, timeout: Duration) -> Result<Self, ClientError> {
        Self::connect(&socket_path(), kind, timeout)
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Evidence label the service reported (`QEMU` or `mock`).
    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    /// Snapshot delivered with the hello reply, when the caller was allowed
    /// to see it.
    pub fn hello_state(&self) -> Option<&NetworkState> {
        self.hello_state.as_ref()
    }

    pub fn request(&mut self, request: &Request) -> Result<Reply, ClientError> {
        write_line(&mut self.writer, request)?;
        read_reply(&mut self.reader)
    }

    pub fn state(&mut self) -> Result<NetworkState, ClientError> {
        match self.request(&Request::State {})? {
            Reply::State { state } => Ok(state),
            Reply::Error { error } => Err(ClientError::Network(error)),
            Reply::ProtocolError { reason } => Err(ClientError::Protocol { reason }),
            other => Err(ClientError::Protocol {
                reason: format!("unexpected state reply {other:?}"),
            }),
        }
    }

    pub fn set_radio(&mut self, enabled: bool) -> Result<(), ClientError> {
        self.request(&Request::SetRadio { enabled })
            .and_then(expect_ok)
    }

    pub fn scan(&mut self) -> Result<(), ClientError> {
        self.request(&Request::Scan {}).and_then(expect_ok)
    }

    pub fn connect_ap(&mut self, ap: &ApId, passphrase: Option<&str>) -> Result<(), ClientError> {
        self.request(&Request::Connect {
            ap: ap.clone(),
            passphrase: passphrase.map(Passphrase::new),
        })
        .and_then(expect_ok)
    }

    pub fn connect_profile(&mut self, profile: &ProfileId) -> Result<(), ClientError> {
        self.request(&Request::ConnectProfile {
            profile: profile.clone(),
        })
        .and_then(expect_ok)
    }

    pub fn disconnect(&mut self) -> Result<(), ClientError> {
        self.request(&Request::Disconnect {}).and_then(expect_ok)
    }

    pub fn forget(&mut self, profile: &ProfileId) -> Result<(), ClientError> {
        self.request(&Request::Forget {
            profile: profile.clone(),
        })
        .and_then(expect_ok)
    }

    /// Switches this connection into a subscription. The reply carries the
    /// snapshot that every following event applies to.
    pub fn subscribe(mut self) -> Result<Subscription, ClientError> {
        let state = match self.request(&Request::Subscribe {})? {
            Reply::State { state } => state,
            Reply::Error { error } => return Err(ClientError::Network(error)),
            Reply::ProtocolError { reason } => return Err(ClientError::Protocol { reason }),
            other => {
                return Err(ClientError::Protocol {
                    reason: format!("unexpected subscribe reply {other:?}"),
                });
            }
        };
        self.reader
            .get_ref()
            .set_read_timeout(None)
            .map_err(|e| ClientError::Io {
                reason: format!("clear timeout: {e}"),
            })?;
        Ok(Subscription {
            state,
            evidence: self.evidence,
            reader: self.reader,
            _writer: self.writer,
            socket: self.socket,
        })
    }
}

/// A subscribed stream: the snapshot plus pushed [`Event`]s.
pub struct Subscription {
    /// Snapshot at subscription time; apply every received event to it.
    pub state: NetworkState,
    pub evidence: String,
    reader: BufReader<UnixStream>,
    _writer: UnixStream,
    socket: PathBuf,
}

impl Subscription {
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Blocks for the next event. `Ok(None)` when the service closed the
    /// stream. Lines that are not events (late replies) are skipped.
    pub fn next_event(&mut self) -> Result<Option<Event>, ClientError> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self
                .reader
                .read_line(&mut line)
                .map_err(|e| ClientError::Io {
                    reason: format!("read: {e}"),
                })?;
            if bytes == 0 {
                return Ok(None);
            }
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<Event>(text) {
                return Ok(Some(event));
            }
        }
    }

    /// Starts the event reader. The channel closes when the service
    /// disconnects; `wake` is called after every delivery and on close so an
    /// event loop can poll the receiver.
    pub fn spawn(self, wake: Arc<dyn Fn() + Send + Sync>) -> (NetworkState, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let state = self.state.clone();
        let mut subscription = self;
        thread::Builder::new()
            .name("ferrix-networkd-events".to_owned())
            .spawn(move || {
                while let Ok(Some(event)) = subscription.next_event() {
                    if tx.send(event).is_err() {
                        return;
                    }
                    wake();
                }
                drop(tx);
                wake();
            })
            .expect("spawn ferrix-networkd event reader");
        (state, rx)
    }
}

/// What a [`Reconnector`] feed delivers. After every (re)connect the fresh
/// snapshot comes first, so reconciliation is "replace local state".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feed {
    /// A new subscription was established; replace local state with this.
    Snapshot {
        state: NetworkState,
        evidence: String,
    },
    /// A delta to apply to the last snapshot.
    Event(Event),
    /// The service went away; the next attempt happens after `retry_in`.
    Disconnected { reason: String, retry_in: Duration },
}

/// Bounded exponential backoff without wall-clock dependence: the caller
/// decides when to sleep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
    attempt: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(Duration::from_millis(250), Duration::from_secs(5))
    }
}

impl Backoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max: max.max(initial),
            attempt: 0,
        }
    }

    /// Delay before the next attempt; doubles per failure, capped at `max`.
    pub fn next_delay(&mut self) -> Duration {
        let factor = 1u32.checked_shl(self.attempt.min(16)).unwrap_or(u32::MAX);
        let delay = self
            .initial
            .checked_mul(factor)
            .unwrap_or(self.max)
            .min(self.max);
        self.attempt = self.attempt.saturating_add(1);
        delay
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempts(&self) -> u32 {
        self.attempt
    }
}

/// Keeps a subscription alive across service restarts.
pub struct Reconnector {
    socket: PathBuf,
    kind: ClientKind,
    timeout: Duration,
    backoff: Backoff,
}

impl Reconnector {
    pub fn new(socket: impl Into<PathBuf>, kind: ClientKind) -> Self {
        Self {
            socket: socket.into(),
            kind,
            timeout: Duration::from_secs(5),
            backoff: Backoff::default(),
        }
    }

    pub fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// One connect + subscribe attempt. Success resets the backoff; failure
    /// advances it and returns the delay to wait before retrying.
    pub fn attempt(&mut self) -> Result<Subscription, (ClientError, Duration)> {
        match Client::connect(&self.socket, self.kind, self.timeout).and_then(Client::subscribe) {
            Ok(subscription) => {
                self.backoff.reset();
                Ok(subscription)
            }
            Err(error) => Err((error, self.backoff.next_delay())),
        }
    }

    /// Runs the reconnect loop on a background thread until the receiver is
    /// dropped. Each connection yields `Feed::Snapshot` first, then events,
    /// then `Feed::Disconnected`.
    pub fn spawn(mut self, wake: Arc<dyn Fn() + Send + Sync>) -> Receiver<Feed> {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("ferrix-networkd-reconnect".to_owned())
            .spawn(move || {
                loop {
                    match self.attempt() {
                        Ok(mut subscription) => {
                            let snapshot = Feed::Snapshot {
                                state: subscription.state.clone(),
                                evidence: subscription.evidence.clone(),
                            };
                            if tx.send(snapshot).is_err() {
                                return;
                            }
                            wake();
                            let reason = loop {
                                match subscription.next_event() {
                                    Ok(Some(event)) => {
                                        if tx.send(Feed::Event(event)).is_err() {
                                            return;
                                        }
                                        wake();
                                    }
                                    Ok(None) => break "service closed the stream".to_owned(),
                                    Err(error) => break error.to_string(),
                                }
                            };
                            let retry_in = self.backoff.next_delay();
                            if tx.send(Feed::Disconnected { reason, retry_in }).is_err() {
                                return;
                            }
                            wake();
                            thread::sleep(retry_in);
                        }
                        Err((error, retry_in)) => {
                            if tx
                                .send(Feed::Disconnected {
                                    reason: error.to_string(),
                                    retry_in,
                                })
                                .is_err()
                            {
                                return;
                            }
                            wake();
                            thread::sleep(retry_in);
                        }
                    }
                }
            })
            .expect("spawn ferrix-networkd reconnector");
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_shapes() {
        assert_eq!(
            serde_json::to_string(&Request::Hello {
                schema_version: 1,
                client_kind: ClientKind::Application,
            })
            .unwrap(),
            r#"{"request":"hello","schema_version":1,"client_kind":"application"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Scan {}).unwrap(),
            r#"{"request":"scan"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::SetRadio { enabled: true }).unwrap(),
            r#"{"request":"set-radio","enabled":true}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Connect {
                ap: ApId::new("ap-1"),
                passphrase: None,
            })
            .unwrap(),
            r#"{"request":"connect","ap":"ap-1"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::ConnectProfile {
                profile: ProfileId::new("profile-1"),
            })
            .unwrap(),
            r#"{"request":"connect-profile","profile":"profile-1"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Forget {
                profile: ProfileId::new("profile-1"),
            })
            .unwrap(),
            r#"{"request":"forget","profile":"profile-1"}"#
        );
        for (text, expected) in [
            (r#"{"request":"state"}"#, Request::State {}),
            (r#"{"request":"subscribe"}"#, Request::Subscribe {}),
            (r#"{"request":"disconnect"}"#, Request::Disconnect {}),
            (
                r#"{"request":"connect","ap":"ap-2","passphrase":"mock-passphrase"}"#,
                Request::Connect {
                    ap: ApId::new("ap-2"),
                    passphrase: Some(Passphrase::new("mock-passphrase")),
                },
            ),
        ] {
            assert_eq!(serde_json::from_str::<Request>(text).unwrap(), expected);
        }
        let names: Vec<&str> = [
            Request::State {},
            Request::Subscribe {},
            Request::SetRadio { enabled: false },
            Request::Scan {},
            Request::Disconnect {},
        ]
        .iter()
        .map(Request::name)
        .collect();
        assert_eq!(
            names,
            ["state", "subscribe", "set-radio", "scan", "disconnect"]
        );
        assert!(Request::Scan {}.is_control());
        assert!(!Request::State {}.is_control());
        assert!(!Request::Subscribe {}.is_control());
        assert!(
            !Request::Hello {
                schema_version: 1,
                client_kind: ClientKind::Tool
            }
            .is_control()
        );
    }

    #[test]
    fn unknown_fields_and_unknown_requests_are_rejected() {
        for text in [
            r#"{"request":"scan","extra":1}"#,
            r#"{"request":"state","ap":"ap-1"}"#,
            r#"{"request":"set-radio","enabled":true,"force":true}"#,
            r#"{"request":"connect","ap":"ap-1","bssid":"aa:bb"}"#,
            r#"{"request":"hello","schema_version":1,"client_kind":"application","app":"org.ferrix.x"}"#,
            r#"{"request":"reboot"}"#,
            r#"{"reply":"ok"}"#,
            r#"{"request":"set-radio"}"#,
            "not json",
        ] {
            assert!(serde_json::from_str::<Request>(text).is_err(), "{text}");
        }
    }

    #[test]
    fn reply_wire_shapes() {
        let hello = Reply::Hello {
            schema_version: 1,
            evidence: "mock".into(),
            state: None,
        };
        assert_eq!(
            serde_json::to_string(&hello).unwrap(),
            r#"{"reply":"hello","schema_version":1,"evidence":"mock"}"#
        );
        let with_state = Reply::Hello {
            schema_version: 1,
            evidence: "mock".into(),
            state: Some(NetworkState::default()),
        };
        let wire = serde_json::to_value(&with_state).unwrap();
        assert_eq!(wire["state"]["schema_version"], 1);
        assert_eq!(serde_json::from_value::<Reply>(wire).unwrap(), with_state);
        assert_eq!(
            serde_json::to_string(&Reply::Ok).unwrap(),
            r#"{"reply":"ok"}"#
        );
        assert_eq!(
            serde_json::to_string(&Reply::Error {
                error: NetworkError::RadioDisabled
            })
            .unwrap(),
            r#"{"reply":"error","error":{"kind":"radio-disabled"}}"#
        );
        assert_eq!(
            serde_json::to_string(&Reply::ProtocolError {
                reason: "hello must come first".into()
            })
            .unwrap(),
            r#"{"reply":"protocol-error","reason":"hello must come first"}"#
        );
        let event: Event = NetworkEvent::ScanChanged {
            scan: ferrix_network_api::ScanState::Scanning,
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"event":"scan-changed","scan":{"state":"scanning"}}"#
        );
    }

    #[test]
    fn schema_mismatch_is_typed() {
        let reply = Reply::Hello {
            schema_version: 2,
            evidence: "mock".into(),
            state: None,
        };
        let outcome = match reply {
            Reply::Hello { schema_version, .. } if schema_version == SCHEMA_VERSION => Ok(()),
            Reply::Hello { schema_version, .. } => Err(ClientError::SchemaMismatch {
                service: schema_version,
                client: SCHEMA_VERSION,
            }),
            _ => unreachable!(),
        };
        assert_eq!(
            outcome,
            Err(ClientError::SchemaMismatch {
                service: 2,
                client: 1
            })
        );
        assert!(
            ClientError::SchemaMismatch {
                service: 2,
                client: 1
            }
            .to_string()
            .contains("incompatible")
        );
        assert_eq!(
            expect_ok(Reply::Error {
                error: NetworkError::NoAdapter
            }),
            Err(ClientError::Network(NetworkError::NoAdapter))
        );
        assert!(matches!(
            expect_ok(Reply::State {
                state: NetworkState::default()
            }),
            Err(ClientError::Protocol { .. })
        ));
    }

    #[test]
    fn backoff_is_bounded_and_resets() {
        let mut backoff = Backoff::new(Duration::from_millis(100), Duration::from_millis(1000));
        let delays: Vec<u64> = (0..6)
            .map(|_| backoff.next_delay().as_millis() as u64)
            .collect();
        assert_eq!(delays, [100, 200, 400, 800, 1000, 1000]);
        assert_eq!(backoff.attempts(), 6);
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(100));
        // A max below the initial is raised to the initial.
        let mut odd = Backoff::new(Duration::from_secs(2), Duration::from_secs(1));
        assert_eq!(odd.next_delay(), Duration::from_secs(2));
        // Never overflows even after many failures.
        let mut many = Backoff::default();
        for _ in 0..100 {
            let _ = many.next_delay();
        }
        assert_eq!(many.next_delay(), many.max);
    }

    #[test]
    fn socket_path_defaults_and_reconnector_targets_it() {
        let reconnector = Reconnector::new(DEFAULT_SOCKET, ClientKind::Tool);
        assert_eq!(reconnector.socket(), Path::new("/run/ferrix/networkd.sock"));
        assert_eq!(DEFAULT_SOCKET, "/run/ferrix/networkd.sock");
        assert_eq!(NETWORKD_SOCKET_ENV, "FERRIX_NETWORKD_SOCKET");
    }

    #[test]
    fn reconnector_attempt_fails_fast_on_a_missing_socket() {
        let dir =
            std::env::temp_dir().join(format!("ferrix-network-protocol-{}", std::process::id()));
        let missing = dir.join("missing.sock");
        let mut reconnector = Reconnector::new(&missing, ClientKind::Tool)
            .with_timeout(Duration::from_millis(0))
            .with_backoff(Backoff::new(
                Duration::from_millis(10),
                Duration::from_millis(20),
            ));
        let Err((error, delay)) = reconnector.attempt() else {
            panic!("connect to a missing socket must fail");
        };
        assert!(matches!(error, ClientError::Unavailable { .. }), "{error}");
        assert_eq!(delay, Duration::from_millis(10));
        let Err((_, delay)) = reconnector.attempt() else {
            panic!("second attempt must fail too");
        };
        assert_eq!(delay, Duration::from_millis(20));
    }
}
