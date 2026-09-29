//! State socket: a Unix listener serving [`Request`]s to shells and
//! applications, plus the fan-out of hardware events to subscribers.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use ferrix_hardware_api::{HardwareEvent, SCHEMA_VERSION};
use ferrix_protocol::{Reply, Request};
use ferrix_session::Session;
use serde_json::json;

/// Streams that asked for events. Guarded so that a subscription snapshot and
/// the subsequent event stream are consistent.
pub type Subscribers = Arc<Mutex<Vec<UnixStream>>>;

pub struct Server {
    session: Arc<Session>,
    subscribers: Subscribers,
}

impl Server {
    pub fn new(session: Arc<Session>) -> Self {
        Self {
            session,
            subscribers: Arc::new(Mutex::new(Vec::new())),
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

    fn handle(&self, stream: UnixStream) {
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
                Ok(Request::Command { command }) => {
                    let reply = match crate::core::apply_product(&self.session.services, command) {
                        Ok(()) => Reply::Command {
                            schema_version: SCHEMA_VERSION,
                            state: self.session.services.snapshot(),
                        },
                        Err(error) => Reply::Error { error },
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
