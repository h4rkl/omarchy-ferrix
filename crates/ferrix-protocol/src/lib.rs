//! Shared wire contract and Unix-socket client for `ferrix-core`.
//!
//! Product commands are intentionally distinct from the host-only mock
//! control protocol. Shells and applications can request user-facing hardware
//! operations, but cannot manufacture battery, SIM, modem, or other facts.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_hardware_api::{
    CapabilityMatrix, HardwareCommand, HardwareError, HardwareEvent, HardwareState, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_SOCKET: &str = "/run/ferrix/core.sock";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    State,
    Subscribe,
    Command { command: HardwareCommand },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "kebab-case")]
pub enum Reply {
    State {
        schema_version: u32,
        target: String,
        profile: String,
        capabilities: CapabilityMatrix,
        state: HardwareState,
    },
    Command {
        schema_version: u32,
        state: HardwareState,
    },
    Error {
        error: HardwareError,
    },
    ProtocolError {
        reason: String,
    },
}

pub struct Subscription {
    pub state: HardwareState,
    pub target: String,
    pub profile: String,
    pub capabilities: CapabilityMatrix,
    reader: BufReader<UnixStream>,
    socket: PathBuf,
}

fn connect_stream(path: &Path, timeout: Duration) -> Result<UnixStream, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "ferrix-core socket {} unavailable: {error}",
                    path.display()
                ));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn write_request(stream: &mut UnixStream, request: &Request) -> Result<(), String> {
    serde_json::to_writer(&mut *stream, request).map_err(|e| format!("encode request: {e}"))?;
    stream
        .write_all(b"\n")
        .and_then(|()| stream.flush())
        .map_err(|e| format!("write request: {e}"))
}

fn read_reply(reader: &mut BufReader<UnixStream>) -> Result<Reply, String> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|e| format!("read core reply: {e}"))?;
    if bytes == 0 {
        return Err("ferrix-core closed before replying".to_owned());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("decode core reply: {e}"))
}

pub fn subscribe(path: &Path, timeout: Duration) -> Result<Subscription, String> {
    let stream = connect_stream(path, timeout)?;
    let mut writer = stream
        .try_clone()
        .map_err(|e| format!("core socket clone: {e}"))?;
    write_request(&mut writer, &Request::Subscribe)?;
    let mut reader = BufReader::new(stream);
    match read_reply(&mut reader)? {
        Reply::State {
            schema_version,
            target,
            profile,
            capabilities,
            state,
        } if schema_version == SCHEMA_VERSION => Ok(Subscription {
            state,
            target,
            profile,
            capabilities,
            reader,
            socket: path.to_path_buf(),
        }),
        Reply::State { schema_version, .. } => Err(format!(
            "ferrix-core schema {schema_version} is incompatible with client schema {SCHEMA_VERSION}"
        )),
        other => Err(format!("unexpected core reply: {other:?}")),
    }
}

impl Subscription {
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Starts the event reader. The channel closes when the core disconnects.
    pub fn spawn(self, wake: Arc<dyn Fn() + Send + Sync>) -> Receiver<HardwareEvent> {
        let (tx, rx) = mpsc::channel();
        let mut lines = self.reader.lines();
        thread::Builder::new()
            .name("core-events".to_owned())
            .spawn(move || {
                while let Some(Ok(line)) = lines.next() {
                    let text = line.trim();
                    if let Ok(event) = serde_json::from_str::<HardwareEvent>(text) {
                        if tx.send(event).is_err() {
                            return;
                        }
                        wake();
                    }
                }
                drop(tx);
                wake();
            })
            .expect("spawn core event reader");
        rx
    }
}

pub fn command(
    path: &Path,
    command: HardwareCommand,
    timeout: Duration,
) -> Result<HardwareState, String> {
    let mut stream = connect_stream(path, timeout)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set core timeout: {e}"))?;
    write_request(&mut stream, &Request::Command { command })?;
    let mut reader = BufReader::new(stream);
    match read_reply(&mut reader)? {
        Reply::Command {
            schema_version,
            state,
        } if schema_version == SCHEMA_VERSION => Ok(state),
        Reply::Command { schema_version, .. } => Err(format!(
            "ferrix-core schema {schema_version} is incompatible with client schema {SCHEMA_VERSION}"
        )),
        Reply::Error { error } => Err(error.to_string()),
        Reply::ProtocolError { reason } => Err(reason),
        other => Err(format!("unexpected core command reply: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_command_wire_form_is_distinct_from_mock_control() {
        let request = Request::Command {
            command: HardwareCommand::SetBrightness { percent: 40 },
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"request":"command","command":{"command":"set-brightness","percent":40}}"#
        );
    }
}
