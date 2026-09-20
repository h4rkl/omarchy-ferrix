//! Caller identity (ADR-004): who is pid P and which capabilities were
//! granted to it. `ferrix-appd` is the only authority; this module speaks
//! its `service` role over raw JSON lines so the daemon does not depend on
//! protocol types that are still being added there.
//!
//! Every source implements [`Identity`]. Socket requests and subscription
//! events resolve again each time, so revocation is immediate and an idle
//! connection cannot retain grants across an appd restart. `generation`
//! remains observable for authority lifecycle diagnostics and test controls.

use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

/// Read-only access: snapshot and subscription.
pub const CAP_STATE: &str = "network-state";
/// Control: radio, scan, connect, disconnect, forget.
pub const CAP_CONTROL: &str = "network-control";

/// Identity of one peer as resolved by the identity source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerIdentity {
    /// Verified application id, or `None` for an unknown process.
    pub app: Option<String>,
    /// Effective grants (`declared ∩ granted`).
    pub granted: BTreeSet<String>,
}

impl PeerIdentity {
    pub fn granted(app: Option<&str>, capabilities: &[&str]) -> Self {
        Self {
            app: app.map(str::to_owned),
            granted: capabilities.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    pub fn has(&self, capability: &str) -> bool {
        self.granted.contains(capability)
    }
}

/// Resolves kernel peer pids to identities.
pub trait Identity: Send + Sync {
    /// Short name for the `networkd-identity` record.
    fn name(&self) -> &'static str;

    /// Identity of `pid`; an unknown or missing pid yields no grants.
    fn identify(&self, pid: Option<u32>) -> PeerIdentity;

    /// Moves whenever the backing authority connection is re-established.
    fn generation(&self) -> u64;
}

/// Nobody is granted anything (`--appd none`).
pub struct DenyAll;

impl Identity for DenyAll {
    fn name(&self) -> &'static str {
        "deny-all"
    }

    fn identify(&self, _pid: Option<u32>) -> PeerIdentity {
        PeerIdentity::default()
    }

    fn generation(&self) -> u64 {
        0
    }
}

/// Everyone is granted everything (`--allow-unauthenticated`, nested only).
pub struct AllowAll;

impl Identity for AllowAll {
    fn name(&self) -> &'static str {
        "allow-all"
    }

    fn identify(&self, _pid: Option<u32>) -> PeerIdentity {
        PeerIdentity::granted(None, &[CAP_STATE, CAP_CONTROL])
    }

    fn generation(&self) -> u64 {
        0
    }
}

/// A fixed pid table; for tests.
#[derive(Default)]
pub struct StaticIdentity {
    table: Mutex<HashMap<u32, PeerIdentity>>,
    generation: AtomicU64,
}

impl StaticIdentity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(&self, pid: u32, identity: PeerIdentity) {
        self.table
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(pid, identity);
    }

    pub fn revoke(&self, pid: u32) {
        self.table
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&pid);
    }

    /// Simulates an identity-source restart: cached identities go stale.
    pub fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

impl Identity for StaticIdentity {
    fn name(&self) -> &'static str {
        "static"
    }

    fn identify(&self, pid: Option<u32>) -> PeerIdentity {
        pid.and_then(|pid| {
            self.table
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&pid)
                .cloned()
        })
        .unwrap_or_default()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

struct AppdLink {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

/// Asks `ferrix-appd` as role `service`:
/// `{"request":"hello","role":"service"}` once per connection, then
/// `{"request":"identify","pid":N}` per lookup, expecting
/// `{"reply":"identity","app":...,"granted":[...]}`. Any failure denies and
/// drops the link; the next lookup reconnects and bumps the generation.
pub struct AppdIdentity {
    socket: PathBuf,
    timeout: Duration,
    link: Mutex<Option<AppdLink>>,
    generation: AtomicU64,
}

impl AppdIdentity {
    pub fn new(socket: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket: socket.into(),
            timeout,
            link: Mutex::new(None),
            generation: AtomicU64::new(0),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    fn open(&self) -> Result<AppdLink, String> {
        let stream = UnixStream::connect(&self.socket)
            .map_err(|e| format!("connect {}: {e}", self.socket.display()))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| format!("timeout: {e}"))?;
        let writer = stream.try_clone().map_err(|e| format!("clone: {e}"))?;
        let mut link = AppdLink {
            writer,
            reader: BufReader::new(stream),
        };
        let reply = exchange(&mut link, &json!({"request": "hello", "role": "service"}))?;
        if reply.get("reply").and_then(Value::as_str) != Some("hello") {
            return Err(format!(
                "appd refused the service hello: {}",
                reply.get("reason").and_then(Value::as_str).unwrap_or("?")
            ));
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        Ok(link)
    }

    fn lookup(&self, pid: u32) -> Result<PeerIdentity, String> {
        let mut guard = self.link.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(self.open()?);
        }
        let link = guard.as_mut().expect("link opened above");
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

fn exchange(link: &mut AppdLink, request: &Value) -> Result<Value, String> {
    let mut text = request.to_string();
    text.push('\n');
    link.writer
        .write_all(text.as_bytes())
        .and_then(|()| link.writer.flush())
        .map_err(|e| format!("write appd: {e}"))?;
    let mut line = String::new();
    let bytes = link
        .reader
        .read_line(&mut line)
        .map_err(|e| format!("read appd: {e}"))?;
    if bytes == 0 {
        return Err("appd closed the connection".to_owned());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("decode appd reply: {e}"))
}

/// Parses `{"reply":"identity","app":...,"granted":[...]}`. Anything else
/// (a protocol error, an older appd) denies.
pub fn parse_identity(reply: &Value) -> Result<PeerIdentity, String> {
    if reply.get("reply").and_then(Value::as_str) != Some("identity") {
        return Err(format!(
            "appd did not answer identify: {}",
            reply.get("reply").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    let app = reply.get("app").and_then(Value::as_str).map(str::to_owned);
    let granted = reply
        .get("granted")
        .and_then(Value::as_array)
        .map(|caps| {
            caps.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(PeerIdentity { app, granted })
}

impl Identity for AppdIdentity {
    fn name(&self) -> &'static str {
        "appd"
    }

    fn identify(&self, pid: Option<u32>) -> PeerIdentity {
        let Some(pid) = pid else {
            return PeerIdentity::default();
        };
        match self.lookup(pid) {
            Ok(identity) => identity,
            // One retry covers an appd restart between two lookups.
            Err(_) => self.lookup(pid).unwrap_or_default(),
        }
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_reply_parses_and_others_deny() {
        let identity = parse_identity(&json!({
            "reply": "identity",
            "schema_version": 1,
            "app": "org.ferrix.settings",
            "trust": "privileged",
            "granted": ["network-state", "network-control"],
        }))
        .unwrap();
        assert_eq!(identity.app.as_deref(), Some("org.ferrix.settings"));
        assert!(identity.has(CAP_STATE) && identity.has(CAP_CONTROL));

        let unknown =
            parse_identity(&json!({"reply": "identity", "app": null, "granted": []})).unwrap();
        assert_eq!(unknown, PeerIdentity::default());
        assert!(!unknown.has(CAP_STATE));

        assert!(parse_identity(&json!({"reply": "protocol-error", "reason": "x"})).is_err());
        assert!(parse_identity(&json!({"reply": "ok"})).is_err());
    }

    #[test]
    fn static_table_and_fixed_sources() {
        let table = StaticIdentity::new();
        table.grant(
            41,
            PeerIdentity::granted(Some("org.ferrix.settings"), &[CAP_STATE]),
        );
        assert!(table.identify(Some(41)).has(CAP_STATE));
        assert!(!table.identify(Some(41)).has(CAP_CONTROL));
        assert_eq!(table.identify(Some(42)), PeerIdentity::default());
        assert_eq!(table.identify(None), PeerIdentity::default());
        assert_eq!(table.generation(), 0);
        table.bump();
        assert_eq!(table.generation(), 1);
        table.revoke(41);
        assert_eq!(table.identify(Some(41)), PeerIdentity::default());

        assert!(!DenyAll.identify(Some(1)).has(CAP_STATE));
        assert!(AllowAll.identify(Some(1)).has(CAP_CONTROL));
        assert!(AllowAll.identify(None).has(CAP_STATE));
    }

    #[test]
    fn appd_identity_denies_when_the_socket_is_missing() {
        let dir =
            std::env::temp_dir().join(format!("ferrix-networkd-identity-{}", std::process::id()));
        let missing = dir.join("missing.sock");
        let identity = AppdIdentity::new(&missing, Duration::from_millis(50));
        assert_eq!(identity.identify(Some(7)), PeerIdentity::default());
        assert_eq!(identity.identify(None), PeerIdentity::default());
        assert_eq!(identity.socket(), missing.as_path());
    }

    #[test]
    fn appd_identity_speaks_hello_then_identify_over_raw_json() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("ferrix-networkd-appd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("appd.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut lines = BufReader::new(stream).lines();
            let hello: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            assert_eq!(hello["request"], "hello");
            assert_eq!(hello["role"], "service");
            writeln!(
                writer,
                r#"{{"reply":"hello","schema_version":1,"evidence":"mock"}}"#
            )
            .unwrap();
            let identify: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            assert_eq!(identify["request"], "identify");
            let pid = identify["pid"].as_u64().unwrap();
            let reply = if pid == 100 {
                json!({"reply":"identity","schema_version":1,"app":"org.ferrix.settings","trust":"privileged","granted":["network-state"]})
            } else {
                json!({"reply":"identity","schema_version":1,"app":null,"trust":"untrusted","granted":[]})
            };
            writeln!(writer, "{reply}").unwrap();
            let second: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            assert_eq!(second["request"], "identify");
            writeln!(
                writer,
                r#"{{"reply":"identity","schema_version":1,"app":null,"granted":[]}}"#
            )
            .unwrap();
        });
        let identity = AppdIdentity::new(&path, Duration::from_secs(2));
        assert_eq!(identity.generation(), 0);
        let settings = identity.identify(Some(100));
        assert_eq!(settings.app.as_deref(), Some("org.ferrix.settings"));
        assert!(settings.has(CAP_STATE));
        assert!(!settings.has(CAP_CONTROL));
        assert_eq!(
            identity.generation(),
            1,
            "first connect moves the generation"
        );
        assert_eq!(identity.identify(Some(200)), PeerIdentity::default());
        assert_eq!(identity.generation(), 1, "reuse does not");
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
