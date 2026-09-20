//! Wire contract and blocking Unix-socket client for `ferrix-contactsd`.
//!
//! Line-delimited JSON, schema-versioned through
//! [`ferrix_contacts_api::SCHEMA_VERSION`]. Every [`Request`] receives exactly
//! one [`Reply`]; after `Subscribe` the stream carries pushed
//! [`ContactEvent`]s (`{"event": ...}`) instead. The first message must be
//! `Hello`; a schema mismatch is rejected there. Unknown fields and unknown
//! request names are protocol errors that never mutate state.
//!
//! Callers are identified by the service from the socket peer pid through
//! `ferrix-appd` (ADR-004), never by anything in a payload.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_contacts_api::{
    Contact, ContactEvent, ContactId, ContactInput, ContactsError, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

/// Default socket path inside the guest.
pub const DEFAULT_SOCKET: &str = "/run/ferrix/contactsd.sock";
/// Environment override for the socket path.
pub const SOCKET_ENV: &str = "FERRIX_CONTACTSD_SOCKET";

/// The socket path from the environment, else [`DEFAULT_SOCKET`].
pub fn socket_path() -> PathBuf {
    std::env::var_os(SOCKET_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
}

/// What kind of process is connecting. Informational: authorization comes
/// from the verified peer identity, not from this field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    Application,
    Tool,
}

/// Messages that receive exactly one [`Reply`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    /// Must be the first message on a connection.
    Hello {
        schema_version: u32,
        client_kind: ClientKind,
    },
    Get {
        id: ContactId,
    },
    /// Empty struct variants (not unit variants) so `deny_unknown_fields`
    /// applies to them too.
    List {},
    Search {
        query: String,
    },
    /// Best contact for a number (E.164 first, then trailing digits).
    Lookup {
        number: String,
    },
    Create {
        contact_input: ContactInput,
    },
    Update {
        id: ContactId,
        contact_input: ContactInput,
    },
    Delete {
        id: ContactId,
    },
    ExportVcard {},
    ImportVcard {
        text: String,
    },
    /// Switch this stream to pushed [`ContactEvent`]s.
    Subscribe {},
}

impl Request {
    /// Stable name for evidence records.
    pub fn name(&self) -> &'static str {
        match self {
            Request::Hello { .. } => "hello",
            Request::Get { .. } => "get",
            Request::List {} => "list",
            Request::Search { .. } => "search",
            Request::Lookup { .. } => "lookup",
            Request::Create { .. } => "create",
            Request::Update { .. } => "update",
            Request::Delete { .. } => "delete",
            Request::ExportVcard {} => "export-vcard",
            Request::ImportVcard { .. } => "import-vcard",
            Request::Subscribe {} => "subscribe",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "kebab-case")]
pub enum Reply {
    Hello {
        schema_version: u32,
        evidence: String,
    },
    Contact {
        contact: Contact,
    },
    /// `List`, `Search`, `ImportVcard` (the imported contacts), and the
    /// snapshot sent before a subscription starts.
    Contacts {
        contacts: Vec<Contact>,
    },
    /// `Lookup` with no match; also `Ok` for `Delete` and `Subscribe`.
    Ok,
    Vcard {
        text: String,
    },
    Error {
        error: ContactsError,
    },
    ProtocolError {
        reason: String,
    },
}

/// Pushed to subscribed streams.
pub type Event = ContactEvent;

fn connect_stream(path: &Path, timeout: Duration) -> Result<UnixStream, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "contactsd socket {} unavailable: {error}",
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
        .map_err(|e| format!("write contactsd message: {e}"))
}

fn read_reply(reader: &mut BufReader<UnixStream>) -> Result<Reply, String> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|e| format!("read contactsd reply: {e}"))?;
    if bytes == 0 {
        return Err("contactsd closed before replying".to_owned());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("decode contactsd reply: {e}"))
}

/// Outcome of a typed request: transport failures are `Err(Err(text))`,
/// service failures `Err(Ok(error))`.
pub type Outcome<T> = Result<T, Result<ContactsError, String>>;

fn unexpected<T>(what: &str, reply: Reply) -> Outcome<T> {
    Err(Err(format!("unexpected contactsd {what} reply: {reply:?}")))
}

/// A request/reply connection. Not subscribed; every request gets a reply.
pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
}

impl Client {
    /// Connects and says hello. Fails on a schema mismatch.
    pub fn connect(path: &Path, kind: ClientKind, timeout: Duration) -> Result<Self, String> {
        let stream = connect_stream(path, timeout)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("set contactsd timeout: {e}"))?;
        let writer = stream
            .try_clone()
            .map_err(|e| format!("contactsd socket clone: {e}"))?;
        let mut client = Self {
            writer,
            reader: BufReader::new(stream),
            socket: path.to_path_buf(),
        };
        match client.request(&Request::Hello {
            schema_version: SCHEMA_VERSION,
            client_kind: kind,
        })? {
            Reply::Hello { schema_version, .. } if schema_version == SCHEMA_VERSION => Ok(client),
            Reply::Hello { schema_version, .. } => Err(format!(
                "contactsd schema {schema_version} is incompatible with client schema {SCHEMA_VERSION}"
            )),
            Reply::ProtocolError { reason } => Err(format!("contactsd rejected hello: {reason}")),
            other => Err(format!("unexpected contactsd hello reply: {other:?}")),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn request(&mut self, request: &Request) -> Result<Reply, String> {
        write_line(&mut self.writer, request)?;
        read_reply(&mut self.reader)
    }

    fn typed(&mut self, request: &Request) -> Outcome<Reply> {
        match self.request(request).map_err(Err)? {
            Reply::Error { error } => Err(Ok(error)),
            Reply::ProtocolError { reason } => Err(Ok(ContactsError::Protocol { reason })),
            reply => Ok(reply),
        }
    }

    pub fn get(&mut self, id: &ContactId) -> Outcome<Contact> {
        match self.typed(&Request::Get { id: id.clone() })? {
            Reply::Contact { contact } => Ok(contact),
            other => unexpected("get", other),
        }
    }

    pub fn list(&mut self) -> Outcome<Vec<Contact>> {
        match self.typed(&Request::List {})? {
            Reply::Contacts { contacts } => Ok(contacts),
            other => unexpected("list", other),
        }
    }

    pub fn search(&mut self, query: &str) -> Outcome<Vec<Contact>> {
        match self.typed(&Request::Search {
            query: query.to_owned(),
        })? {
            Reply::Contacts { contacts } => Ok(contacts),
            other => unexpected("search", other),
        }
    }

    /// `Ok(None)` when nothing matches.
    pub fn lookup(&mut self, number: &str) -> Outcome<Option<Contact>> {
        match self.typed(&Request::Lookup {
            number: number.to_owned(),
        })? {
            Reply::Contact { contact } => Ok(Some(contact)),
            Reply::Ok => Ok(None),
            other => unexpected("lookup", other),
        }
    }

    pub fn create(&mut self, input: ContactInput) -> Outcome<Contact> {
        match self.typed(&Request::Create {
            contact_input: input,
        })? {
            Reply::Contact { contact } => Ok(contact),
            other => unexpected("create", other),
        }
    }

    pub fn update(&mut self, id: &ContactId, input: ContactInput) -> Outcome<Contact> {
        match self.typed(&Request::Update {
            id: id.clone(),
            contact_input: input,
        })? {
            Reply::Contact { contact } => Ok(contact),
            other => unexpected("update", other),
        }
    }

    pub fn delete(&mut self, id: &ContactId) -> Outcome<()> {
        match self.typed(&Request::Delete { id: id.clone() })? {
            Reply::Ok => Ok(()),
            other => unexpected("delete", other),
        }
    }

    pub fn export_vcard(&mut self) -> Outcome<String> {
        match self.typed(&Request::ExportVcard {})? {
            Reply::Vcard { text } => Ok(text),
            other => unexpected("export-vcard", other),
        }
    }

    /// Returns the contacts created by the import.
    pub fn import_vcard(&mut self, text: &str) -> Outcome<Vec<Contact>> {
        match self.typed(&Request::ImportVcard {
            text: text.to_owned(),
        })? {
            Reply::Contacts { contacts } => Ok(contacts),
            other => unexpected("import-vcard", other),
        }
    }

    /// Switches this connection into a subscription. The full contact list
    /// is fetched first so the subscriber starts from a consistent snapshot.
    pub fn subscribe(mut self) -> Result<Subscription, String> {
        let contacts = self.list().map_err(|e| match e {
            Ok(error) => error.to_string(),
            Err(text) => text,
        })?;
        match self.request(&Request::Subscribe {})? {
            Reply::Ok => {}
            Reply::Error { error } => return Err(error.to_string()),
            other => return Err(format!("unexpected contactsd subscribe reply: {other:?}")),
        }
        self.reader
            .get_ref()
            .set_read_timeout(None)
            .map_err(|e| format!("clear contactsd timeout: {e}"))?;
        Ok(Subscription {
            contacts,
            reader: self.reader,
            socket: self.socket,
        })
    }
}

/// A subscribed stream with the snapshot it started from.
pub struct Subscription {
    pub contacts: Vec<Contact>,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
}

impl Subscription {
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Reads events on a thread; the channel closes when the service goes
    /// away. `wake` is called after every delivery and on close.
    pub fn spawn(self, wake: Arc<dyn Fn() + Send + Sync>) -> Receiver<ContactEvent> {
        let (tx, rx) = mpsc::channel();
        let mut lines = self.reader.lines();
        thread::Builder::new()
            .name("contactsd-events".to_owned())
            .spawn(move || {
                while let Some(Ok(line)) = lines.next() {
                    let text = line.trim();
                    if text.is_empty() {
                        continue;
                    }
                    if let Ok(event) = serde_json::from_str::<ContactEvent>(text) {
                        if tx.send(event).is_err() {
                            return;
                        }
                        wake();
                    }
                }
                drop(tx);
                wake();
            })
            .expect("spawn contactsd event reader");
        rx
    }
}

/// What a [`Reconnector`] delivers to its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// A (re)connection succeeded; replace local state with `contacts`.
    Connected {
        contacts: Vec<Contact>,
    },
    Event(ContactEvent),
    /// The stream ended; a reconnect attempt follows.
    Lost {
        reason: String,
    },
}

/// Keeps a subscription alive from a background thread: connects, forwards
/// the snapshot and every event, reports the loss, and retries every
/// `retry`. Reconciliation after a service restart is "replace state with
/// the next `Connected` snapshot". Dropping the reconnector stops the thread
/// at its next attempt.
pub struct Reconnector {
    rx: Receiver<Link>,
    stop: Arc<AtomicBool>,
}

impl Reconnector {
    pub fn start(
        socket: PathBuf,
        kind: ClientKind,
        retry: Duration,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        thread::Builder::new()
            .name("contactsd-reconnect".to_owned())
            .spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    let subscription =
                        match Client::connect(&socket, kind, retry).and_then(Client::subscribe) {
                            Ok(subscription) => subscription,
                            Err(_) => {
                                thread::sleep(retry);
                                continue;
                            }
                        };
                    if tx
                        .send(Link::Connected {
                            contacts: subscription.contacts.clone(),
                        })
                        .is_err()
                    {
                        return;
                    }
                    wake();
                    let mut lines = subscription.reader.lines();
                    let mut reason = "stream closed".to_owned();
                    loop {
                        match lines.next() {
                            Some(Ok(line)) => {
                                let text = line.trim();
                                if text.is_empty() {
                                    continue;
                                }
                                if let Ok(event) = serde_json::from_str::<ContactEvent>(text) {
                                    if tx.send(Link::Event(event)).is_err() {
                                        return;
                                    }
                                    wake();
                                }
                            }
                            Some(Err(error)) => {
                                reason = error.to_string();
                                break;
                            }
                            None => break,
                        }
                    }
                    if tx.send(Link::Lost { reason }).is_err() {
                        return;
                    }
                    wake();
                    thread::sleep(retry);
                }
            })
            .expect("spawn contactsd reconnect thread");
        Self { rx, stop }
    }

    /// Everything delivered since the last drain, in order.
    pub fn drain(&self) -> Vec<Link> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(link) => out.push(link),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }
}

impl Drop for Reconnector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_contacts_api::ContactId;

    #[test]
    fn requests_are_tagged_and_reject_unknown_fields() {
        let hello = Request::Hello {
            schema_version: 1,
            client_kind: ClientKind::Application,
        };
        assert_eq!(
            serde_json::to_string(&hello).unwrap(),
            r#"{"request":"hello","schema_version":1,"client_kind":"application"}"#
        );
        let lookup: Request =
            serde_json::from_str(r#"{"request":"lookup","number":"+15550100042"}"#).unwrap();
        assert_eq!(lookup.name(), "lookup");
        assert!(serde_json::from_str::<Request>(r#"{"request":"list","extra":1}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"request":"bogus"}"#).is_err());
        assert!(serde_json::from_str::<Request>(r#"{"reply":"ok"}"#).is_err());
        assert_eq!(
            serde_json::to_string(&Request::List {}).unwrap(),
            r#"{"request":"list"}"#
        );
        let del: Request =
            serde_json::from_str(r#"{"request":"delete","id":"c_0000000000000001"}"#).unwrap();
        assert_eq!(
            del,
            Request::Delete {
                id: ContactId::from_bytes([0, 0, 0, 0, 0, 0, 0, 1])
            }
        );
    }

    #[test]
    fn replies_and_events_have_distinct_shapes() {
        let reply = Reply::Error {
            error: ContactsError::Unauthorized {
                capability: "contacts-write".to_owned(),
            },
        };
        assert_eq!(
            serde_json::to_string(&reply).unwrap(),
            r#"{"reply":"error","error":{"kind":"unauthorized","capability":"contacts-write"}}"#
        );
        assert_eq!(
            serde_json::to_string(&Reply::Ok).unwrap(),
            r#"{"reply":"ok"}"#
        );
        let event: Event = serde_json::from_str(r#"{"event":"service-restarted"}"#).unwrap();
        assert_eq!(event, ContactEvent::ServiceRestarted);
        assert!(serde_json::from_str::<Event>(r#"{"reply":"ok"}"#).is_err());
    }

    #[test]
    fn socket_path_defaults_to_the_guest_location() {
        assert_eq!(DEFAULT_SOCKET, "/run/ferrix/contactsd.sock");
        assert_eq!(SOCKET_ENV, "FERRIX_CONTACTSD_SOCKET");
    }
}
