//! State socket: a Unix listener serving [`Request`]s to shells and
//! applications, plus the fan-out of hardware events to subscribers.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use ferrix_hardware_api::{
    Capability, HardwareCommand, HardwareError, HardwareEvent, SCHEMA_VERSION,
};
use ferrix_protocol::{Reply, Request};
use ferrix_session::Session;
use serde_json::json;

use crate::screen::ScreenPolicyStore;

/// Streams that asked for events. Guarded so that a subscription snapshot and
/// the subsequent event stream are consistent.
pub type Subscribers = Arc<Mutex<Vec<UnixStream>>>;

pub struct Server {
    session: Arc<Session>,
    subscribers: Subscribers,
    screen: Arc<ScreenPolicyStore>,
    power: crate::power::PowerHandoff,
    /// Connection ids (the screen report is tied to one connection).
    next_connection: AtomicU64,
}

impl Server {
    pub fn new(
        session: Arc<Session>,
        screen: Arc<ScreenPolicyStore>,
        power: crate::power::PowerHandoff,
    ) -> Self {
        Self {
            session,
            subscribers: Arc::new(Mutex::new(Vec::new())),
            screen,
            power,
            next_connection: AtomicU64::new(1),
        }
    }

    pub fn subscribers(&self) -> Subscribers {
        Arc::clone(&self.subscribers)
    }

    /// Binds the socket. Nothing is reported as ready before this succeeds.
    pub fn bind(path: &Path) -> Result<UnixListener, String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("cannot remove stale {}: {e}", path.display()))?;
        }
        UnixListener::bind(path).map_err(|e| format!("cannot bind {}: {e}", path.display()))
    }

    pub fn state_reply(&self) -> Reply {
        Reply::State {
            schema_version: SCHEMA_VERSION,
            target: self.session.target_id.clone(),
            profile: self.session.profile.name.clone(),
            capabilities: self.session.capabilities.clone(),
            state: self.session.services.snapshot(),
        }
    }

    fn bluetooth_session_reply(&self) -> Reply {
        match self.session.services.bluetooth.bluetooth_session() {
            Ok(session) => Reply::BluetoothSession {
                schema_version: SCHEMA_VERSION,
                session,
            },
            Err(error) => Reply::Error { error },
        }
    }

    /// Accept loop; one thread per connection.
    pub fn serve(self: Arc<Self>, listener: UnixListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let server = Arc::clone(&self);
                    thread::spawn(move || server.handle(stream));
                }
                Err(e) => {
                    crate::emit(json!({
                        "evidence": crate::evidence_label(),
                        "event": "core-socket-error",
                        "error": e.to_string(),
                    }));
                }
            }
        }
    }

    /// One connection. Inhibitors it took are released when it ends, however
    /// it ends (hang-up, crash, protocol error).
    fn handle(&self, stream: UnixStream) {
        let connection = self.next_connection.fetch_add(1, Ordering::Relaxed);
        let mut held: Vec<u64> = Vec::new();
        self.serve_connection(stream, connection, &mut held);
        for id in held {
            self.screen.release(id, "holder-gone");
        }
        self.screen.connection_closed(connection);
    }

    fn serve_connection(&self, stream: UnixStream, connection: u64, held: &mut Vec<u64>) {
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut writer = stream;
        let mut lines = BufReader::new(read_half).lines();
        while let Some(Ok(line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Request>(line) {
                Ok(Request::State) => {
                    if write_json(&mut writer, &self.state_reply()).is_err() {
                        return;
                    }
                }
                Ok(Request::Subscribe) => {
                    // Hold the subscriber lock across snapshot + registration so
                    // no event published in between is lost or duplicated.
                    let mut subs = lock(&self.subscribers);
                    if write_json(&mut writer, &self.state_reply()).is_err() {
                        return;
                    }
                    if let Ok(clone) = writer.try_clone() {
                        subs.push(clone);
                    }
                }
                Ok(Request::BluetoothDevices) => {
                    let reply = match self.session.services.bluetooth.bluetooth_devices() {
                        Ok(devices) => Reply::BluetoothDevices {
                            schema_version: SCHEMA_VERSION,
                            devices,
                        },
                        Err(error) => Reply::Error { error },
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Ok(Request::BluetoothSession) => {
                    let reply = self.bluetooth_session_reply();
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Ok(Request::Bluetooth { command }) => {
                    let reply = match command.apply(&*self.session.services.bluetooth) {
                        Ok(()) => self.bluetooth_session_reply(),
                        Err(error) => Reply::Error { error },
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Ok(Request::Command { command }) => {
                    // Suspend never reaches the adapter directly: the
                    // policy store applies the board flag, the kill switch,
                    // and the inhibitors first.
                    let result = match command {
                        HardwareCommand::Suspend => self.screen.suspend_now().map(|_| ()),
                        other => crate::core::apply_product(&self.session.services, other),
                    };
                    if let HardwareCommand::PowerAction { action } = command {
                        self.power.report(action, &result);
                    }
                    let accepted = result.is_ok();
                    let reply = match result {
                        Ok(()) => Reply::Command {
                            schema_version: SCHEMA_VERSION,
                            state: self.session.services.snapshot(),
                        },
                        Err(error) => Reply::Error { error },
                    };
                    let written = write_json(&mut writer, &reply).is_ok();
                    // Only after the reply: the caller learns it was accepted
                    // before init starts taking the system down.
                    if let (HardwareCommand::PowerAction { action }, true) = (command, accepted) {
                        self.power.hand_off(action);
                    }
                    if !written {
                        return;
                    }
                }
                Ok(Request::ScreenPolicy) => {
                    if write_json(&mut writer, &self.screen.reply()).is_err() {
                        return;
                    }
                }
                Ok(Request::WatchScreenPolicy) => match writer.try_clone() {
                    Ok(clone) => {
                        if self.screen.watch(clone).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                },
                Ok(Request::SetIdleTimeout { idle_timeout_secs }) => {
                    let reply = match self.screen.set_idle_timeout(idle_timeout_secs) {
                        Ok(_) => self.screen.reply(),
                        Err(error) => Reply::Error { error },
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Ok(Request::InhibitBlank { reason, holder }) => {
                    let (id, policy) = self.screen.inhibit(reason, &holder);
                    held.push(id);
                    let reply = Reply::Inhibited {
                        schema_version: SCHEMA_VERSION,
                        id,
                        policy,
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Ok(Request::ReportScreen { off }) => {
                    let reply = Reply::ScreenPolicy {
                        schema_version: SCHEMA_VERSION,
                        policy: self.screen.report_screen(connection, off),
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Ok(Request::ReleaseInhibit { id }) => {
                    // Only inhibitors this connection took can be released here.
                    let reply = match held.iter().position(|h| *h == id) {
                        Some(index) => {
                            held.remove(index);
                            self.screen.release(id, "release");
                            self.screen.reply()
                        }
                        None => Reply::Error {
                            error: HardwareError::invalid(
                                Capability::Lifecycle,
                                format!("inhibitor {id} is not held by this connection"),
                            ),
                        },
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let reply = Reply::ProtocolError {
                        reason: format!("invalid request: {e}"),
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
            }
        }
    }
}

/// Fan an event out to every live subscriber; dead ones are dropped.
pub fn broadcast(subscribers: &Subscribers, event: &HardwareEvent) {
    let mut subs = lock(subscribers);
    subs.retain_mut(|stream| write_json(stream, event).is_ok());
}

fn write_json(stream: &mut UnixStream, value: &impl serde::Serialize) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(std::io::Error::other)?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

fn lock(subscribers: &Subscribers) -> std::sync::MutexGuard<'_, Vec<UnixStream>> {
    subscribers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_hardware_api::InhibitReason;
    use ferrix_protocol::PolicyFeed;
    use ferrix_session::{BootstrapOptions, bootstrap};
    use std::time::{Duration, Instant};

    /// The typed client API end to end over a real socket: a watcher sees
    /// the timeout change, an inhibitor, and its release when the holder's
    /// connection goes away (as when the holding app crashes).
    #[test]
    fn inhibitors_die_with_their_connection_and_watchers_follow() {
        let dir = std::env::temp_dir().join(format!("ferrix-core-srv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("core.sock");
        let session = Arc::new(
            bootstrap(BootstrapOptions::default().with_profile_path(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../targets/qemu-aarch64/profile.toml"
            )))
            .unwrap(),
        );
        let screen = Arc::new(ScreenPolicyStore::new(
            session.profile.display.idle_timeout(),
            None,
        ));
        let server = Arc::new(Server::new(session, screen, Default::default()));
        let listener = Server::bind(&socket).unwrap();
        thread::spawn(move || server.serve(listener));
        let t = Duration::from_secs(2);

        let feed = ferrix_protocol::watch_screen_policy(
            socket.clone(),
            t,
            Duration::from_millis(50),
            Arc::new(|| {}),
        );
        let next = || match feed.recv_timeout(Duration::from_secs(5)).unwrap() {
            PolicyFeed::Policy(policy) => policy,
            other => panic!("{other:?}"),
        };
        assert_eq!(next().idle_timeout_secs, None, "QEMU profile: never");

        let policy = ferrix_protocol::set_idle_timeout(&socket, Some(30), t).unwrap();
        assert_eq!(policy.idle_timeout_secs, Some(30));
        assert_eq!(next().idle_timeout_secs, Some(30));
        assert!(ferrix_protocol::set_idle_timeout(&socket, Some(1), t).is_err());

        let call =
            ferrix_protocol::inhibit_blank(&socket, InhibitReason::Call, "phone", t).unwrap();
        assert_eq!(next().reasons(), [InhibitReason::Call]);
        let video =
            ferrix_protocol::inhibit_blank(&socket, InhibitReason::Video, "browser", t).unwrap();
        assert_eq!(next().effective_idle_timeout_secs(), None);
        let policy = video.release(t).unwrap();
        assert_eq!(policy.reasons(), [InhibitReason::Call]);
        assert_eq!(next().reasons(), [InhibitReason::Call]);

        drop(call); // the holder "crashes": its connection closes
        let deadline = Instant::now() + Duration::from_secs(5);
        let released = loop {
            let policy = next();
            if !policy.inhibited() || Instant::now() > deadline {
                break policy;
            }
        };
        assert!(!released.inhibited());
        assert_eq!(released.effective_idle_timeout_secs(), Some(30));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The shell's screen report lives with its connection, and a product
    /// suspend command obeys the board flag (QEMU profile: off).
    #[test]
    fn screen_report_dies_with_its_connection_and_suspend_obeys_the_flag() {
        let dir = std::env::temp_dir().join(format!("ferrix-core-rep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("core.sock");
        let session = Arc::new(
            bootstrap(BootstrapOptions::default().with_profile_path(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../targets/qemu-aarch64/profile.toml"
            )))
            .unwrap(),
        );
        let lifecycle = session.profile.lifecycle_section();
        let screen = Arc::new(ScreenPolicyStore::with_suspend(
            None,
            None,
            crate::screen::SuspendConfig {
                mode: lifecycle.suspend,
                after_blank_secs: lifecycle.suspend_after_blank(),
                kill_switch: dir.join("no-suspend"),
                opt_in: None,
                lifecycle: Arc::clone(&session.services.lifecycle),
            },
        ));
        let server = Arc::new(Server::new(
            session,
            Arc::clone(&screen),
            Default::default(),
        ));
        let listener = Server::bind(&socket).unwrap();
        thread::spawn(move || server.serve(listener));
        let t = Duration::from_secs(2);
        let wait = |off: bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while screen.policy().suspend.screen_off != off {
                assert!(Instant::now() < deadline, "screen_off never became {off}");
                thread::sleep(Duration::from_millis(10));
            }
        };

        let reporter = ferrix_protocol::report_screen(socket.clone(), t, Duration::from_millis(50));
        reporter.send(true).unwrap();
        wait(true);
        assert_eq!(
            screen.policy().suspend.phase,
            ferrix_hardware_api::SuspendPhase::Disabled,
            "off on the QEMU profile"
        );
        let refused = ferrix_protocol::command(&socket, HardwareCommand::Suspend, t);
        assert!(refused.is_err(), "{refused:?}");
        assert_eq!(screen.policy().suspend.cycles, 0);
        drop(reporter); // the shell goes away: the screen counts as on
        wait(false);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
