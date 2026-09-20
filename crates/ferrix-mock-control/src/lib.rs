//! Host/test-only mock-control channel (ADR-006).
//!
//! A mocked daemon opens exactly one channel: a QEMU `virtio-serial` port
//! (guest side `/dev/vportNpM`, resolved from `/sys/class/virtio-ports/*/name`)
//! or a Unix socket (nested host runs). The host writes one JSON command per
//! line and reads one JSON [`Ack`] per line. The channel drives *mock* state
//! only; a production backend must refuse to open it. Nothing here is an
//! application API and nothing here knows which daemon it serves: the daemon
//! supplies a [`Handler`] that parses its own command type.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const SYSFS_PORTS: &str = "/sys/class/virtio-ports";

/// Where the channel lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelPath {
    /// Scan `/sys/class/virtio-ports` for the named port (guest).
    Auto { port_name: String },
    /// No channel: the daemon runs without host control.
    None,
    /// A character device (virtio port) or a Unix socket path.
    Explicit(PathBuf),
}

impl ChannelPath {
    /// `auto`, `none`/empty, or a path. `port_name` is the virtio-serial
    /// port name used for `auto` (for example `ferrix.telephony`).
    pub fn parse(text: &str, port_name: &str) -> Self {
        match text {
            "auto" => ChannelPath::Auto {
                port_name: port_name.to_owned(),
            },
            "none" | "" => ChannelPath::None,
            other => ChannelPath::Explicit(PathBuf::from(other)),
        }
    }

    /// Resolves `Auto` against sysfs. `None` stays `None`.
    pub fn resolve(&self) -> Option<Resolved> {
        match self {
            ChannelPath::None => None,
            ChannelPath::Explicit(path) => Some(classify(path)),
            ChannelPath::Auto { port_name } => detect_port(port_name).map(Resolved::Port),
        }
    }
}

/// A resolved channel endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A virtio-serial character device.
    Port(PathBuf),
    /// A Unix socket the daemon must bind and accept on.
    Socket(PathBuf),
}

impl Resolved {
    pub fn path(&self) -> &Path {
        match self {
            Resolved::Port(p) | Resolved::Socket(p) => p,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Resolved::Port(_) => "virtio-port",
            Resolved::Socket(_) => "unix-socket",
        }
    }
}

fn classify(path: &Path) -> Resolved {
    // An existing character device is a port; everything else is a socket the
    // daemon binds (a stale socket file is removed at bind time).
    match std::fs::metadata(path) {
        Ok(meta) => {
            use std::os::unix::fs::FileTypeExt as _;
            if meta.file_type().is_char_device() {
                Resolved::Port(path.to_path_buf())
            } else {
                Resolved::Socket(path.to_path_buf())
            }
        }
        Err(_) => Resolved::Socket(path.to_path_buf()),
    }
}

fn detect_port(port_name: &str) -> Option<PathBuf> {
    let udev_link = Path::new("/dev/virtio-ports").join(port_name);
    if udev_link.exists() {
        return Some(udev_link);
    }
    let entries = std::fs::read_dir(SYSFS_PORTS).ok()?;
    for entry in entries.flatten() {
        let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
        if name.trim() == port_name {
            let node = Path::new("/dev").join(entry.file_name());
            if node.exists() {
                return Some(node);
            }
        }
    }
    None
}

/// The reply to one command line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Ack {
    Ok {
        /// Command name that was applied.
        ack: String,
        /// Names of the events the command produced, in order.
        #[serde(default)]
        events: Vec<String>,
        /// Daemon-specific state snapshot (ids and states only).
        #[serde(default)]
        state: Value,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ack: Option<String>,
        reason: String,
    },
}

impl Ack {
    pub fn ok(ack: impl Into<String>, events: Vec<String>, state: Value) -> Self {
        Ack::Ok {
            ack: ack.into(),
            events,
            state,
        }
    }

    pub fn error(ack: Option<&str>, reason: impl Into<String>) -> Self {
        Ack::Error {
            ack: ack.map(str::to_owned),
            reason: reason.into(),
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Ack::Ok { .. })
    }
}

/// Parses and applies one command line. The daemon owns the command type;
/// this trait sees the raw JSON so unknown commands can be rejected with the
/// daemon's own message.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, line: &str) -> Ack;
}

/// Records emitted by the serve loop, so the daemon can log them in its own
/// evidence format.
pub trait Observer: Send + Sync + 'static {
    fn ready(&self, resolved: &Resolved);
    fn failed(&self, stage: &str, error: &str);
}

/// Opens the resolved channel and serves it on a background thread until the
/// process exits. Returns after the port is open or the socket is bound.
pub fn spawn(
    resolved: Resolved,
    handler: Arc<dyn Handler>,
    observer: Arc<dyn Observer>,
) -> Result<(), String> {
    match resolved {
        Resolved::Port(path) => {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|e| format!("cannot open control port {}: {e}", path.display()))?;
            observer.ready(&Resolved::Port(path.clone()));
            thread::Builder::new()
                .name("mock-control-port".to_owned())
                .spawn(move || serve_port(file, handler, observer))
                .map_err(|e| format!("cannot start control thread: {e}"))?;
        }
        Resolved::Socket(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("cannot remove stale {}: {e}", path.display()))?;
            }
            let listener = UnixListener::bind(&path)
                .map_err(|e| format!("cannot bind control socket {}: {e}", path.display()))?;
            observer.ready(&Resolved::Socket(path.clone()));
            thread::Builder::new()
                .name("mock-control-socket".to_owned())
                .spawn(move || serve_socket(listener, handler, observer))
                .map_err(|e| format!("cannot start control thread: {e}"))?;
        }
    }
    Ok(())
}

fn serve_port(file: File, handler: Arc<dyn Handler>, observer: Arc<dyn Observer>) {
    let mut writer = match file.try_clone() {
        Ok(w) => w,
        Err(e) => {
            observer.failed("clone", &e.to_string());
            return;
        }
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // EOF: no host client attached yet; virtio-serial reports no data
            // as EOF while the chardev has no peer. Idle and retry.
            Ok(0) => thread::sleep(Duration::from_millis(200)),
            Ok(_) => {
                if let Err(e) = answer(&handler, &line, &mut writer) {
                    observer.failed("ack", &e.to_string());
                }
            }
            Err(e) => {
                observer.failed("read", &e.to_string());
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

fn serve_socket(listener: UnixListener, handler: Arc<dyn Handler>, observer: Arc<dyn Observer>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let handler = Arc::clone(&handler);
                let observer = Arc::clone(&observer);
                let _ = thread::Builder::new()
                    .name("mock-control-conn".to_owned())
                    .spawn(move || serve_stream(stream, handler, observer));
            }
            Err(e) => observer.failed("accept", &e.to_string()),
        }
    }
}

fn serve_stream(stream: UnixStream, handler: Arc<dyn Handler>, observer: Arc<dyn Observer>) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut writer = stream;
    let mut lines = BufReader::new(read_half).lines();
    while let Some(Ok(line)) = lines.next() {
        if let Err(e) = answer(&handler, &line, &mut writer) {
            observer.failed("ack", &e.to_string());
            return;
        }
    }
}

fn answer(handler: &Arc<dyn Handler>, line: &str, writer: &mut impl Write) -> std::io::Result<()> {
    let text = line.trim();
    if text.is_empty() {
        return Ok(());
    }
    let ack = handler.handle(text);
    let mut reply = serde_json::to_string(&ack).unwrap_or_default();
    reply.push('\n');
    writer.write_all(reply.as_bytes())?;
    writer.flush()
}

/// Host-side helper: send one command line to a Unix-socket channel and read
/// the ack. Used by nested tests and by `ferrix-scenario`.
pub fn send(socket: &Path, command: &Value, timeout: Duration) -> Result<Ack, String> {
    let stream = UnixStream::connect(socket)
        .map_err(|e| format!("cannot connect to {}: {e}", socket.display()))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set timeout: {e}"))?;
    let mut writer = stream.try_clone().map_err(|e| format!("clone: {e}"))?;
    let mut text = serde_json::to_string(command).map_err(|e| format!("encode: {e}"))?;
    text.push('\n');
    writer
        .write_all(text.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|e| format!("write: {e}"))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .map_err(|e| format!("read ack: {e}"))?;
    if n == 0 {
        return Err("channel closed before an ack arrived".to_owned());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("decode ack: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Echo;
    impl Handler for Echo {
        fn handle(&self, line: &str) -> Ack {
            match serde_json::from_str::<Value>(line) {
                Ok(v) if v.get("cmd").is_some() => {
                    let name = v["cmd"].as_str().unwrap_or("?").to_owned();
                    Ack::ok(name, vec!["echoed".into()], v)
                }
                Ok(_) => Ack::error(None, "missing cmd"),
                Err(e) => Ack::error(None, format!("invalid json: {e}")),
            }
        }
    }

    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);
    impl Observer for Log {
        fn ready(&self, r: &Resolved) {
            self.0.lock().unwrap().push(format!("ready:{}", r.kind()));
        }
        fn failed(&self, stage: &str, error: &str) {
            self.0
                .lock()
                .unwrap()
                .push(format!("failed:{stage}:{error}"));
        }
    }

    #[test]
    fn parse_forms() {
        assert_eq!(
            ChannelPath::parse("auto", "ferrix.telephony"),
            ChannelPath::Auto {
                port_name: "ferrix.telephony".into()
            }
        );
        assert_eq!(ChannelPath::parse("none", "x"), ChannelPath::None);
        assert_eq!(ChannelPath::parse("", "x"), ChannelPath::None);
        assert_eq!(
            ChannelPath::parse("/tmp/a.sock", "x"),
            ChannelPath::Explicit(PathBuf::from("/tmp/a.sock"))
        );
        assert!(ChannelPath::None.resolve().is_none());
    }

    #[test]
    fn ack_wire_form() {
        let ack = Ack::ok(
            "set-x",
            vec!["x-changed".into()],
            serde_json::json!({"a":1}),
        );
        assert_eq!(
            serde_json::to_string(&ack).unwrap(),
            r#"{"status":"ok","ack":"set-x","events":["x-changed"],"state":{"a":1}}"#
        );
        let err = Ack::error(None, "bad");
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"status":"error","reason":"bad"}"#
        );
        assert!(!err.is_ok());
    }

    #[test]
    fn socket_round_trip_rejects_unknown_and_malformed() {
        let dir = std::env::temp_dir().join(format!("ferrix-mock-control-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ctl.sock");
        let log = Arc::new(Log::default());
        let resolved = ChannelPath::Explicit(path.clone()).resolve().unwrap();
        assert_eq!(resolved.kind(), "unix-socket");
        spawn(resolved, Arc::new(Echo), log.clone()).unwrap();
        let ok = send(
            &path,
            &serde_json::json!({"cmd":"set-x","v":1}),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(ok.is_ok());
        let bad = send(
            &path,
            &serde_json::json!({"nope":1}),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(!bad.is_ok());
        assert!(
            log.0
                .lock()
                .unwrap()
                .iter()
                .any(|l| l == "ready:unix-socket")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
