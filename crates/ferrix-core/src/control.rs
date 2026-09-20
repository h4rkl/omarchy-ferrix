//! Mock-control channel.
//!
//! On the generic QEMU target the host exposes a `virtio-serial` port named
//! `ferrix.control`; the guest side appears as `/dev/vportNpM` (and as
//! `/dev/virtio-ports/ferrix.control` when udev runs). The channel carries
//! line-delimited [`ControlCommand`] JSON in and [`Ack`] JSON out. It is
//! test control of mocked state, never a product interface.
//!
//! Path resolution: an explicit path, `none`, or `auto` (scan
//! `/sys/class/virtio-ports/*/name`). When nothing is found the daemon still
//! runs and says so; it never pretends the channel exists.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use crate::protocol::{Ack, ControlCommand};

pub const PORT_NAME: &str = "ferrix.control";
const SYSFS_PORTS: &str = "/sys/class/virtio-ports";
const UDEV_LINK: &str = "/dev/virtio-ports/ferrix.control";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlPath {
    Auto,
    None,
    Explicit(PathBuf),
}

impl ControlPath {
    pub fn parse(text: &str) -> Self {
        match text {
            "auto" => ControlPath::Auto,
            "none" | "" => ControlPath::None,
            other => ControlPath::Explicit(PathBuf::from(other)),
        }
    }

    pub fn resolve(&self) -> Option<PathBuf> {
        match self {
            ControlPath::None => None,
            ControlPath::Explicit(path) => Some(path.clone()),
            ControlPath::Auto => detect(),
        }
    }
}

fn detect() -> Option<PathBuf> {
    if Path::new(UDEV_LINK).exists() {
        return Some(PathBuf::from(UDEV_LINK));
    }
    let entries = std::fs::read_dir(SYSFS_PORTS).ok()?;
    for entry in entries.flatten() {
        let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
        if name.trim() == PORT_NAME {
            let node = Path::new("/dev").join(entry.file_name());
            if node.exists() {
                return Some(node);
            }
        }
    }
    None
}

pub trait Handler: Send + Sync + 'static {
    fn handle(&self, command: Result<ControlCommand, String>) -> Ack;
}

/// Opens the port and serves commands until the process exits.
pub fn spawn(path: PathBuf, handler: Arc<dyn Handler>) -> Result<(), String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("cannot open control port {}: {e}", path.display()))?;
    thread::Builder::new()
        .name("mock-control".to_owned())
        .spawn(move || serve(file, path, handler))
        .map_err(|e| format!("cannot start control thread: {e}"))?;
    Ok(())
}

fn serve(file: File, path: PathBuf, handler: Arc<dyn Handler>) {
    let mut writer = match file.try_clone() {
        Ok(w) => w,
        Err(e) => {
            crate::emit(json!({
                "evidence": "mock",
                "event": "mock-control",
                "status": "failed",
                "path": path.display().to_string(),
                "error": e.to_string(),
            }));
            return;
        }
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // EOF: the host side is not connected yet (virtio-serial reports
            // no data as EOF while the chardev has no client). Idle and retry.
            Ok(0) => thread::sleep(Duration::from_millis(200)),
            Ok(_) => {
                let text = line.trim();
                if text.is_empty() {
                    continue;
                }
                let parsed = serde_json::from_str::<ControlCommand>(text)
                    .map_err(|e| format!("invalid command: {e}"));
                let ack = handler.handle(parsed);
                let mut reply = serde_json::to_string(&ack).unwrap_or_default();
                reply.push('\n');
                if let Err(e) = writer
                    .write_all(reply.as_bytes())
                    .and_then(|()| writer.flush())
                {
                    crate::emit(json!({
                        "evidence": "mock",
                        "event": "mock-control",
                        "status": "ack-failed",
                        "error": e.to_string(),
                    }));
                }
            }
            Err(e) => {
                crate::emit(json!({
                    "evidence": "mock",
                    "event": "mock-control",
                    "status": "read-failed",
                    "error": e.to_string(),
                }));
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
}
