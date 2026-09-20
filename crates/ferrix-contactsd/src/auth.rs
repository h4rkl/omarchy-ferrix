//! Peer authorization (ADR-004): the socket peer pid is resolved to a Ferrix
//! `AppId` and its granted capabilities through `ferrix-appd`, and every
//! request is checked against the capability it needs.
//!
//! The daemon speaks raw JSON lines to `ferrix-appd` on a `service`-role
//! connection (`{"request":"hello","role":"service"}` then
//! `{"request":"identify","pid":N}`, expecting
//! `{"reply":"identity",...,"app":...,"granted":[...]}`) so this crate does
//! not link the app protocol crate and stays decoupled from its type names.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use ferrix_contacts_api::{CAP_READ, CAP_WRITE};
use ferrix_contacts_protocol::Request;
use serde_json::{Value, json};

/// The verified identity of a connection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identified {
    pub app: Option<String>,
    pub granted: BTreeSet<String>,
}

impl Identified {
    pub fn has(&self, capability: &str) -> bool {
        self.granted.contains(capability)
    }
}

/// Source of peer identities. Implementations must not trust anything the
/// peer sent; only the kernel-reported pid goes in.
pub trait Identity: Send + Sync {
    fn identify(&self, pid: Option<u32>) -> Identified;
    /// For the ready record.
    fn describe(&self) -> String;
}

/// The capability a request needs, or `None` for `Hello`.
pub fn required_capability(request: &Request) -> Option<&'static str> {
    match request {
        Request::Hello { .. } => None,
        Request::Get { .. }
        | Request::List {}
        | Request::Search { .. }
        | Request::Lookup { .. }
        | Request::ExportVcard {}
        | Request::Subscribe {} => Some(CAP_READ),
        Request::Create { .. }
        | Request::Update { .. }
        | Request::Delete { .. }
        | Request::ImportVcard { .. } => Some(CAP_WRITE),
    }
}

/// Denies everyone: `--appd none` without `--allow-unauthenticated`.
pub struct DenyAll;

impl Identity for DenyAll {
    fn identify(&self, _pid: Option<u32>) -> Identified {
        Identified::default()
    }

    fn describe(&self) -> String {
        "deny-all".to_owned()
    }
}

/// Grants read and write to every peer: nested development only, announced
/// by a warning record at start.
pub struct AllowAll;

impl Identity for AllowAll {
    fn identify(&self, _pid: Option<u32>) -> Identified {
        Identified {
            app: None,
            granted: [CAP_READ, CAP_WRITE]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        }
    }

    fn describe(&self) -> String {
        "allow-unauthenticated".to_owned()
    }
}

/// Fixed pid table; for tests.
#[derive(Default)]
pub struct StaticIdentity {
    entries: Vec<(u32, Identified)>,
}

impl StaticIdentity {
    pub fn with(mut self, pid: u32, app: &str, granted: &[&str]) -> Self {
        self.entries.push((
            pid,
            Identified {
                app: Some(app.to_owned()),
                granted: granted.iter().map(|s| (*s).to_owned()).collect(),
            },
        ));
        self
    }
}

impl Identity for StaticIdentity {
    fn identify(&self, pid: Option<u32>) -> Identified {
        pid.and_then(|pid| {
            self.entries
                .iter()
                .find(|(p, _)| *p == pid)
                .map(|(_, id)| id.clone())
        })
        .unwrap_or_default()
    }

    fn describe(&self) -> String {
        format!("static({} pids)", self.entries.len())
    }
}

/// Resolves pids through `ferrix-appd`. One connection is kept and reopened
/// after any failure; an unreachable service yields no grants.
pub struct AppdIdentity {
    socket: PathBuf,
    timeout: Duration,
    link: Mutex<Option<(UnixStream, BufReader<UnixStream>)>>,
}

impl AppdIdentity {
    pub fn new(socket: &Path, timeout: Duration) -> Self {
        Self {
            socket: socket.to_path_buf(),
            timeout,
            link: Mutex::new(None),
        }
    }

    fn open(&self) -> Result<(UnixStream, BufReader<UnixStream>), String> {
        let stream = UnixStream::connect(&self.socket).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut link = (writer, BufReader::new(stream));
        let reply = exchange(&mut link, &json!({"request": "hello", "role": "service"}))?;
        match reply.get("reply").and_then(Value::as_str) {
            Some("hello") => Ok(link),
            other => Err(format!("appd hello reply {other:?}")),
        }
    }

    fn resolve(&self, pid: u32) -> Result<Identified, String> {
        let mut guard = self.link.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(self.open()?);
        }
        let link = guard.as_mut().ok_or("no appd link")?;
        let reply = match exchange(link, &json!({"request": "identify", "pid": pid})) {
            Ok(reply) => reply,
            Err(error) => {
                *guard = None;
                return Err(error);
            }
        };
        parse_identity(&reply)
    }
}

fn exchange(
    link: &mut (UnixStream, BufReader<UnixStream>),
    request: &Value,
) -> Result<Value, String> {
    let mut text = request.to_string();
    text.push('\n');
    link.0
        .write_all(text.as_bytes())
        .and_then(|()| link.0.flush())
        .map_err(|e| format!("write appd: {e}"))?;
    let mut line = String::new();
    let n = link
        .1
        .read_line(&mut line)
        .map_err(|e| format!("read appd: {e}"))?;
    if n == 0 {
        return Err("appd closed".to_owned());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("decode appd: {e}"))
}

/// Parses `{"reply":"identity","app":...,"granted":[...]}`; any other reply
/// (an error, a protocol error) is no identity.
pub fn parse_identity(reply: &Value) -> Result<Identified, String> {
    match reply.get("reply").and_then(Value::as_str) {
        Some("identity") => {}
        other => return Err(format!("appd identify reply {other:?}")),
    }
    let app = reply.get("app").and_then(Value::as_str).map(str::to_owned);
    let granted = reply
        .get("granted")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(Identified { app, granted })
}

impl Identity for AppdIdentity {
    fn identify(&self, pid: Option<u32>) -> Identified {
        let Some(pid) = pid else {
            return Identified::default();
        };
        self.resolve(pid).unwrap_or_default()
    }

    fn describe(&self) -> String {
        format!("appd:{}", self.socket.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_map_to_read_or_write() {
        assert_eq!(required_capability(&Request::List {}), Some(CAP_READ));
        assert_eq!(required_capability(&Request::Subscribe {}), Some(CAP_READ));
        assert_eq!(
            required_capability(&Request::ImportVcard {
                text: String::new()
            }),
            Some(CAP_WRITE)
        );
        assert_eq!(
            required_capability(&Request::Hello {
                schema_version: 1,
                client_kind: ferrix_contacts_protocol::ClientKind::Tool
            }),
            None
        );
    }

    #[test]
    fn identity_replies_parse_and_others_do_not() {
        let id = parse_identity(&json!({
            "reply": "identity", "schema_version": 1, "app": "org.ferrix.phone",
            "trust": "untrusted", "granted": ["telephony-call", "contacts-read"]
        }))
        .unwrap();
        assert_eq!(id.app.as_deref(), Some("org.ferrix.phone"));
        assert!(id.has(CAP_READ) && !id.has(CAP_WRITE));
        let unknown =
            parse_identity(&json!({"reply": "identity", "app": null, "granted": []})).unwrap();
        assert_eq!(unknown, Identified::default());
        assert!(parse_identity(&json!({"reply": "error", "error": {}})).is_err());
        assert_eq!(
            StaticIdentity::default().identify(Some(1)),
            Identified::default()
        );
        assert_eq!(DenyAll.identify(Some(1)), Identified::default());
        assert!(AllowAll.identify(None).has(CAP_WRITE));
    }

    #[test]
    fn unreachable_appd_grants_nothing() {
        let id = AppdIdentity::new(
            Path::new("/nonexistent/ferrix-appd.sock"),
            Duration::from_millis(50),
        );
        assert_eq!(id.identify(Some(4242)), Identified::default());
        assert_eq!(id.identify(None), Identified::default());
    }
}
