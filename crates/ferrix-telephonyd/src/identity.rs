//! Peer identity resolution (ADR-004).
//!
//! `ferrix-appd` is the only authority on "which application is pid P". The
//! daemon asks it over its socket with role `service` for every request and
//! subscription event. The wire form is spoken literally as raw JSON lines
//! (`{"request":"hello","role":"service"}` then
//! `{"request":"identify","pid":N}`, expecting
//! `{"reply":"identity","app":...,"granted":[...]}`) so this crate does not
//! link the application protocol types.
//!
//! A [`generation`](Identity::generation) counter records when the
//! application service connection was re-established; authorization itself
//! is fail-closed and never depends on a stale per-client cache.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Value, json};

/// Capability string required for `state` and `subscribe`.
pub const CAP_STATE: &str = "telephony-state";
/// Capability string required for call verbs, audio route, and PIN.
pub const CAP_CALL: &str = "telephony-call";
/// Capability string required for SMS verbs.
pub const CAP_SMS: &str = "telephony-sms";
/// Capability string required for cellular data bearer control.
pub const CAP_DATA: &str = "telephony-data";
/// Capability string required for cellular radio power control.
pub const CAP_RADIO: &str = "telephony-radio";

/// Every telephony capability, for fully trusted peers.
pub fn all_capabilities() -> BTreeSet<String> {
    [CAP_STATE, CAP_CALL, CAP_SMS, CAP_DATA, CAP_RADIO]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// What the application service said about a pid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentityInfo {
    /// The verified application id, or `None` for an unknown process.
    pub app: Option<String>,
    /// Effective (declared ∩ granted) capability strings.
    pub granted: BTreeSet<String>,
}

impl IdentityInfo {
    pub fn unknown() -> Self {
        Self::default()
    }

    pub fn granted(app: Option<&str>, capabilities: &[&str]) -> Self {
        Self {
            app: app.map(str::to_owned),
            granted: capabilities.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    pub fn has(&self, capability: &str) -> bool {
        self.granted.contains(capability)
    }

    pub fn has_call(&self) -> bool {
        self.has(CAP_CALL)
    }

    pub fn has_sms(&self) -> bool {
        self.has(CAP_SMS)
    }

    pub fn has_data(&self) -> bool {
        self.has(CAP_DATA)
    }
}

/// Source of peer identities.
pub trait Identity: Send + Sync {
    /// Resolves a kernel pid. `None` (no peer credentials available on this
    /// platform) resolves to whatever the source's policy says.
    fn identify(&self, pid: Option<u32>) -> IdentityInfo;
    /// Increases whenever the authority behind this source was re-established.
    fn generation(&self) -> u64 {
        0
    }
    fn name(&self) -> &'static str;
}

/// Denies every peer. Used for `--appd none` without
/// `--allow-unauthenticated`.
pub struct DenyAll;

impl Identity for DenyAll {
    fn identify(&self, _pid: Option<u32>) -> IdentityInfo {
        IdentityInfo::unknown()
    }

    fn name(&self) -> &'static str {
        "deny-all"
    }
}

/// Grants every capability to every peer. Nested development only; the
/// daemon records a warning when this is composed.
pub struct AllowAll;

impl Identity for AllowAll {
    fn identify(&self, _pid: Option<u32>) -> IdentityInfo {
        IdentityInfo {
            app: None,
            granted: all_capabilities(),
        }
    }

    fn name(&self) -> &'static str {
        "allow-all"
    }
}

/// A fixed pid table for tests; unknown pids are denied.
#[derive(Default)]
pub struct StaticIdentity {
    table: Mutex<BTreeMap<u32, IdentityInfo>>,
    generation: AtomicU64,
}

impl StaticIdentity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(self, pid: u32, info: IdentityInfo) -> Self {
        self.set(pid, info);
        self
    }

    pub fn set(&self, pid: u32, info: IdentityInfo) {
        lock(&self.table).insert(pid, info);
    }

    pub fn remove(&self, pid: u32) {
        lock(&self.table).remove(&pid);
    }

    /// Simulates an application-service restart.
    pub fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

impl Identity for StaticIdentity {
    fn identify(&self, pid: Option<u32>) -> IdentityInfo {
        pid.and_then(|p| lock(&self.table).get(&p).cloned())
            .unwrap_or_default()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn name(&self) -> &'static str {
        "static"
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Resolves pids through the application service socket. One persistent
/// connection is kept; on any failure it is dropped, the generation bumps,
/// and the next call reconnects. An unreachable service denies.
pub struct AppdIdentity {
    socket: PathBuf,
    timeout: Duration,
    link: Mutex<Option<Link>>,
    generation: AtomicU64,
}

pub(crate) struct Link {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

/// Opens a role-`service` link to the application service.
pub(crate) fn connect_link(socket: &Path, timeout: Duration) -> Result<Link, String> {
    let stream = UnixStream::connect(socket)
        .map_err(|e| format!("appd socket {}: {e}", socket.display()))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("appd timeout: {e}"))?;
    let writer = stream.try_clone().map_err(|e| format!("appd clone: {e}"))?;
    let mut link = Link {
        writer,
        reader: BufReader::new(stream),
    };
    let hello = link.exchange(&json!({"request": "hello", "role": "service"}))?;
    if hello.get("reply").and_then(Value::as_str) != Some("hello") {
        return Err(format!("unexpected appd hello reply: {hello}"));
    }
    Ok(link)
}

impl AppdIdentity {
    pub fn new(socket: PathBuf, timeout: Duration) -> Self {
        Self {
            socket,
            timeout,
            link: Mutex::new(None),
            generation: AtomicU64::new(0),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    fn connect(&self) -> Result<Link, String> {
        connect_link(&self.socket, self.timeout)
    }

    fn with_link<T>(&self, f: impl Fn(&mut Link) -> Result<T, String>) -> Result<T, String> {
        let mut guard = lock(&self.link);
        if guard.is_none() {
            *guard = Some(self.connect()?);
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        let link = guard.as_mut().expect("link present");
        match f(link) {
            Ok(value) => Ok(value),
            Err(error) => {
                *guard = None;
                self.generation.fetch_add(1, Ordering::SeqCst);
                Err(error)
            }
        }
    }
}

impl Link {
    pub(crate) fn exchange(&mut self, request: &Value) -> Result<Value, String> {
        let mut text = request.to_string();
        text.push('\n');
        self.writer
            .write_all(text.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| format!("appd write: {e}"))?;
        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .map_err(|e| format!("appd read: {e}"))?;
        if n == 0 {
            return Err("appd closed the connection".to_owned());
        }
        serde_json::from_str(line.trim()).map_err(|e| format!("appd decode: {e}"))
    }
}

/// Parses `{"reply":"identity","app":...,"granted":[...]}`. Anything else
/// (including an `error` reply) is an unknown identity.
pub fn parse_identity_reply(reply: &Value) -> IdentityInfo {
    if reply.get("reply").and_then(Value::as_str) != Some("identity") {
        return IdentityInfo::unknown();
    }
    IdentityInfo {
        app: reply.get("app").and_then(Value::as_str).map(str::to_owned),
        granted: reply
            .get("granted")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

impl Identity for AppdIdentity {
    fn identify(&self, pid: Option<u32>) -> IdentityInfo {
        let Some(pid) = pid else {
            return IdentityInfo::unknown();
        };
        self.with_link(|link| link.exchange(&json!({"request": "identify", "pid": pid})))
            .map(|reply| parse_identity_reply(&reply))
            .unwrap_or_default()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn name(&self) -> &'static str {
        "appd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn identity_reply_parsing_is_strict_about_the_tag() {
        let info = parse_identity_reply(&json!({
            "reply": "identity", "schema_version": 1, "app": "org.ferrix.phone",
            "trust": "untrusted", "granted": ["telephony-call", "telephony-state"]
        }));
        assert_eq!(info.app.as_deref(), Some("org.ferrix.phone"));
        assert!(info.has_call());
        assert!(!info.has_sms());
        let unknown = parse_identity_reply(&json!({"reply":"identity","app":null,"granted":[]}));
        assert_eq!(unknown, IdentityInfo::unknown());
        let error = parse_identity_reply(&json!({"reply":"error","error":{"kind":"io"}}));
        assert_eq!(error, IdentityInfo::unknown());
    }

    #[test]
    fn static_identity_denies_unknown_pids_and_none() {
        let source = StaticIdentity::new().with(
            7,
            IdentityInfo::granted(Some("org.ferrix.phone"), &[CAP_STATE, CAP_CALL]),
        );
        assert!(source.identify(Some(7)).has_call());
        assert_eq!(source.identify(Some(8)), IdentityInfo::unknown());
        assert_eq!(source.identify(None), IdentityInfo::unknown());
        assert_eq!(DenyAll.identify(Some(7)), IdentityInfo::unknown());
        assert!(AllowAll.identify(None).has_sms());
    }

    /// A fake application service speaking the ADR-004 wire form.
    fn fake_appd(dir_tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferrix-telephonyd-appd-{}-{dir_tag}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("appd.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut writer = stream.try_clone().unwrap();
                let mut lines = BufReader::new(stream).lines();
                let mut said_hello = false;
                while let Some(Ok(line)) = lines.next() {
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let reply = match request["request"].as_str() {
                        Some("hello") => {
                            assert_eq!(request["role"], "service");
                            said_hello = true;
                            json!({"reply":"hello","schema_version":1,"evidence":"mock"})
                        }
                        Some("identify") if said_hello => {
                            if request["pid"] == 4242 {
                                json!({"reply":"identity","schema_version":1,"app":"org.ferrix.messages","trust":"untrusted","granted":["telephony-state","telephony-sms"]})
                            } else {
                                json!({"reply":"identity","schema_version":1,"app":null,"trust":null,"granted":[]})
                            }
                        }
                        _ => json!({"reply":"protocol-error","reason":"hello must come first"}),
                    };
                    let mut text = reply.to_string();
                    text.push('\n');
                    if writer.write_all(text.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        });
        path
    }

    #[test]
    fn appd_identity_speaks_the_wire_form_and_denies_unknown() {
        let path = fake_appd("wire");
        let source = AppdIdentity::new(path, Duration::from_secs(2));
        assert_eq!(source.generation(), 0);
        let info = source.identify(Some(4242));
        assert_eq!(info.app.as_deref(), Some("org.ferrix.messages"));
        assert!(info.has_sms());
        assert!(!info.has_call());
        assert_eq!(source.generation(), 1);
        assert_eq!(source.identify(Some(1)), IdentityInfo::unknown());
        assert_eq!(source.generation(), 1, "one link serves many lookups");
        assert_eq!(source.identify(None), IdentityInfo::unknown());
    }

    #[test]
    fn unreachable_appd_denies() {
        let source = AppdIdentity::new(
            PathBuf::from("/nonexistent/ferrix/appd.sock"),
            Duration::from_millis(200),
        );
        assert_eq!(source.identify(Some(4242)), IdentityInfo::unknown());
    }
}
