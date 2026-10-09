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
    BluetoothDevice, CapabilityMatrix, HardwareCommand, HardwareError, HardwareEvent,
    HardwareState, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_SOCKET: &str = "/run/ferrix/core.sock";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    State,
    Subscribe,
    Command {
        command: HardwareCommand,
    },
    /// The Bluetooth controller's remembered devices (not part of the
    /// copyable snapshot; clients re-query on `bluetooth-changed`).
    BluetoothDevices,
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
    BluetoothDevices {
        schema_version: u32,
        devices: Vec<BluetoothDevice>,
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

/// What a [`spawn_reconnecting`] subscriber receives.
#[derive(Clone, Debug, PartialEq)]
pub enum Feed {
    /// A (re)subscription succeeded: replace local state.
    Snapshot {
        target: String,
        profile: String,
        capabilities: CapabilityMatrix,
        state: HardwareState,
    },
    Event(HardwareEvent),
    /// The core went away (or could not be reached); reported once per loss.
    Disconnected {
        reason: String,
    },
}

/// Follows `ferrix-core` across restarts: subscribes, forwards every event,
/// and after a loss reports [`Feed::Disconnected`] once, then retries every
/// `retry`. The thread ends when the receiver is dropped.
pub fn spawn_reconnecting(
    path: PathBuf,
    timeout: Duration,
    retry: Duration,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Receiver<Feed> {
    let (tx, rx) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("core-reconnector".to_owned())
        .spawn(move || {
            let mut lost_reported = false;
            loop {
                let reason = match subscribe(&path, timeout) {
                    Ok(subscription) => {
                        lost_reported = false;
                        let snapshot = Feed::Snapshot {
                            target: subscription.target,
                            profile: subscription.profile,
                            capabilities: subscription.capabilities,
                            state: subscription.state,
                        };
                        if tx.send(snapshot).is_err() {
                            return;
                        }
                        wake();
                        for line in subscription.reader.lines() {
                            let Ok(line) = line else { break };
                            if let Ok(event) = serde_json::from_str::<HardwareEvent>(line.trim()) {
                                if tx.send(Feed::Event(event)).is_err() {
                                    return;
                                }
                                wake();
                            }
                        }
                        "ferrix-core closed the subscription".to_owned()
                    }
                    Err(reason) => reason,
                };
                if !lost_reported {
                    lost_reported = true;
                    if tx.send(Feed::Disconnected { reason }).is_err() {
                        return;
                    }
                    wake();
                }
                thread::sleep(retry);
            }
        });
    if let Err(error) = spawned {
        let (dead_tx, dead_rx) = mpsc::channel();
        let _ = dead_tx.send(Feed::Disconnected {
            reason: format!("cannot spawn core reconnector: {error}"),
        });
        return dead_rx;
    }
    rx
}

/// Asks for the Bluetooth device list. Typed adapter errors (an adapter
/// that cannot enumerate devices) come back as their message.
pub fn bluetooth_devices(path: &Path, timeout: Duration) -> Result<Vec<BluetoothDevice>, String> {
    let mut stream = connect_stream(path, timeout)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set core timeout: {e}"))?;
    write_request(&mut stream, &Request::BluetoothDevices)?;
    let mut reader = BufReader::new(stream);
    match read_reply(&mut reader)? {
        Reply::BluetoothDevices {
            schema_version,
            devices,
        } if schema_version == SCHEMA_VERSION => Ok(devices),
        Reply::BluetoothDevices { schema_version, .. } => Err(format!(
            "ferrix-core schema {schema_version} is incompatible with client schema {SCHEMA_VERSION}"
        )),
        Reply::Error { error } => Err(error.to_string()),
        Reply::ProtocolError { reason } => Err(reason),
        other => Err(format!("unexpected core devices reply: {other:?}")),
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
        assert_eq!(
            serde_json::to_string(&Request::BluetoothDevices).unwrap(),
            r#"{"request":"bluetooth-devices"}"#
        );
    }

    fn sample_state() -> HardwareState {
        use ferrix_hardware_api::*;
        HardwareState {
            power: PowerState {
                battery_percent: 50,
                charging: false,
            },
            display: DisplayState {
                width: 1,
                height: 2,
                scale: 1.0,
                orientation: Orientation::Portrait,
                brightness_percent: 40,
                rotation_locked: false,
            },
            input: InputState {
                touch: true,
                rotation: true,
            },
            network: NetworkState {
                wifi: WifiState::Connected,
            },
            bluetooth: BluetoothState {
                present: true,
                enabled: true,
            },
            audio: AudioState {
                route: AudioRoute::Speaker,
                available: AudioRouteAvailability::ALL,
            },
            telephony: TelephonyState {
                registration: ModemRegistration::Registered,
                sim: SimState::Present,
            },
            lifecycle: LifecycleState::default(),
        }
    }

    #[test]
    fn reconnector_reports_snapshot_events_and_one_loss_per_outage() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("ferrix-protocol-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("core.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            // One subscriber served with a snapshot and one event, then gone.
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), r#"{"request":"subscribe"}"#);
            let mut writer = stream;
            let reply = Reply::State {
                schema_version: SCHEMA_VERSION,
                target: "qemu-aarch64".into(),
                profile: "test".into(),
                capabilities: CapabilityMatrix::default(),
                state: sample_state(),
            };
            let event = HardwareEvent::BluetoothChanged(ferrix_hardware_api::BluetoothState {
                present: true,
                enabled: false,
            });
            for value in [
                serde_json::to_string(&reply).unwrap(),
                serde_json::to_string(&event).unwrap(),
            ] {
                writer.write_all(value.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
            }
        });
        let feed = spawn_reconnecting(
            socket.clone(),
            Duration::from_millis(200),
            Duration::from_millis(20),
            Arc::new(|| {}),
        );
        let wait = Duration::from_secs(5);
        assert!(matches!(
            feed.recv_timeout(wait).unwrap(),
            Feed::Snapshot { ref target, .. } if target == "qemu-aarch64"
        ));
        assert!(matches!(
            feed.recv_timeout(wait).unwrap(),
            Feed::Event(HardwareEvent::BluetoothChanged(_))
        ));
        server.join().unwrap();
        assert!(matches!(
            feed.recv_timeout(wait).unwrap(),
            Feed::Disconnected { .. }
        ));
        // Retries against a dead socket do not repeat the loss.
        assert!(feed.recv_timeout(Duration::from_millis(300)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
