//! Unix-socket front end: one thread per connection, line-delimited JSON,
//! hello ordering, schema check, per-request authorization, and
//! subscription hand-off. The dispatch step is a pure function over a
//! [`Connection`] so every gate is unit-tested without sockets.

use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use ferrix_network_api::{NetworkError, SCHEMA_VERSION};
use ferrix_network_protocol::{Reply, Request};

use crate::identity::PeerIdentity;
use crate::service::{Service, write_line};

/// Per-connection protocol state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    /// Kernel-reported peer pid when available (Linux/QEMU).
    pub peer_pid: Option<u32>,
    pub hello_done: bool,
    pub subscribed: bool,
    /// Host mock-control has an explicitly trusted synthetic identity and no
    /// kernel peer; ordinary socket connections are never pinned.
    pub preauthenticated: bool,
    /// Most recently observed identity and authority generation.
    pub identity: Option<(u64, PeerIdentity)>,
}

impl Connection {
    pub fn with_pid(pid: Option<u32>) -> Self {
        Self {
            peer_pid: pid,
            ..Self::default()
        }
    }
}

/// What the connection thread does with a parsed line.
// One short-lived value per line; a `Reply::State` snapshot is large but
// boxing it would only add an allocation on the hot path.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Reply(Reply),
    /// Register the stream as a subscriber (the service writes the snapshot).
    Subscribe,
}

fn protocol_error(reason: impl Into<String>) -> Outcome {
    Outcome::Reply(Reply::ProtocolError {
        reason: reason.into(),
    })
}

/// Resolves the peer for this request.
pub fn resolve_identity(service: &Service, connection: &mut Connection) -> PeerIdentity {
    if connection.preauthenticated
        && let Some((_, identity)) = &connection.identity
    {
        return identity.clone();
    }
    let generation = service.identity().generation();
    // Resolve on every request. Generation invalidates test/static caches,
    // but only an actual exchange can discover that appd restarted and
    // closed an otherwise-idle identity connection.
    let identity = service.identify(connection.peer_pid);
    connection.identity = Some((generation, identity.clone()));
    identity
}

/// Applies one raw line. Malformed lines never touch state.
pub fn dispatch_line(service: &Service, connection: &mut Connection, line: &str) -> Outcome {
    match serde_json::from_str::<Request>(line) {
        Ok(request) => dispatch(service, connection, request),
        Err(error) => {
            service
                .recorder()
                .request_rejected(None, &format!("invalid message: {error}"));
            protocol_error(format!("invalid message: {error}"))
        }
    }
}

/// Applies one parsed request: hello first (with schema check), then
/// identity resolution and a capability check per request.
pub fn dispatch(service: &Service, connection: &mut Connection, request: Request) -> Outcome {
    let name = request.name();
    if connection.subscribed {
        return protocol_error("connection is subscribed; requests are not accepted");
    }
    if let Request::Hello { schema_version, .. } = request {
        if connection.hello_done {
            service
                .recorder()
                .request_rejected(Some(name), "hello was already sent");
            return protocol_error("hello was already sent on this connection");
        }
        if schema_version != SCHEMA_VERSION {
            let reason = format!(
                "schema {schema_version} is incompatible with service schema {SCHEMA_VERSION}"
            );
            service.recorder().request_rejected(Some(name), &reason);
            return protocol_error(reason);
        }
        connection.hello_done = true;
        let identity = resolve_identity(service, connection);
        return Outcome::Reply(service.hello_reply(&identity));
    }
    if !connection.hello_done {
        service
            .recorder()
            .request_rejected(Some(name), "hello must come first");
        return protocol_error("hello must come first");
    }
    let identity = resolve_identity(service, connection);
    if let Some(capability) = Service::required_capability(&request)
        && !identity.has(capability)
    {
        service
            .recorder()
            .request_denied(identity.app.as_deref(), capability, name);
        return Outcome::Reply(Reply::Error {
            error: NetworkError::Unauthorized {
                capability: capability.to_owned(),
            },
        });
    }
    match request {
        Request::Hello { .. } => unreachable!("handled above"),
        Request::State {} => Outcome::Reply(Reply::State {
            state: service.state(),
        }),
        Request::Subscribe {} => {
            connection.subscribed = true;
            Outcome::Subscribe
        }
        control => Outcome::Reply(match service.control(&control) {
            Ok(()) => Reply::Ok,
            Err(error) => Reply::Error { error },
        }),
    }
}

pub struct Server {
    service: Arc<Service>,
}

impl Server {
    pub fn new(service: Arc<Service>) -> Self {
        Self { service }
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

    /// Accept loop; one thread per connection.
    pub fn serve(self: Arc<Self>, listener: UnixListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let server = Arc::clone(&self);
                    let spawned = thread::Builder::new()
                        .name("ferrix-networkd-conn".to_owned())
                        .spawn(move || server.handle(stream));
                    if let Err(error) = spawned {
                        self.service
                            .recorder()
                            .socket_error(&format!("cannot spawn connection thread: {error}"));
                    }
                }
                Err(error) => self.service.recorder().socket_error(&error.to_string()),
            }
        }
    }

    /// Serves one connection on the current thread; for tests and the accept loop.
    pub fn handle(&self, stream: UnixStream) {
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut connection = Connection::with_pid(peer_pid(&stream));
        let mut writer = stream;
        let mut lines = BufReader::new(read_half).lines();
        while let Some(Ok(line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match dispatch_line(&self.service, &mut connection, line) {
                Outcome::Reply(reply) => {
                    if write_line(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Outcome::Subscribe => {
                    let Ok(sink) = writer.try_clone() else {
                        return;
                    };
                    if self
                        .service
                        .subscribe_authorized(connection.peer_pid, Box::new(sink))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    rustix::net::sockopt::socket_peercred(stream)
        .ok()
        .and_then(|credentials| u32::try_from(credentials.pid.as_raw_pid()).ok())
}

#[cfg(not(target_os = "linux"))]
fn peer_pid(_stream: &UnixStream) -> Option<u32> {
    None
}
