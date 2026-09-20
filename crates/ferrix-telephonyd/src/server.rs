//! Unix-socket front end: one thread per connection, line-delimited JSON,
//! `ferrix_telephony_protocol::Request` parsing, hello ordering, schema check,
//! per-request capability gating (ADR-004), and subscription hand-off.
//!
//! [`dispatch`] is a pure function over a [`Connection`] so gating is unit
//! tested without sockets.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use ferrix_telephony_api::{SCHEMA_VERSION, TelephonyError};
use ferrix_telephony_protocol::{Reply, Request};

use crate::identity::{CAP_CALL, CAP_SMS, CAP_STATE, Identity, IdentityInfo};
use crate::service::Service;

/// Per-connection protocol state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    pub hello_done: bool,
    pub subscribed: bool,
    /// True only for the daemon's local mock-control path.
    pub preauthenticated: bool,
    /// Kernel-reported peer pid when available (Linux/QEMU).
    pub peer_pid: Option<u32>,
    /// Most recently observed identity and authority generation.
    pub identity: Option<(IdentityInfo, u64)>,
}

impl Connection {
    pub fn with_peer(pid: Option<u32>) -> Self {
        Self {
            peer_pid: pid,
            ..Self::default()
        }
    }

    /// A connection whose hello and identity are already settled; used by
    /// the host mock-control channel, which is not a socket peer. The
    /// identity is pinned to `generation` of the source it is used with.
    pub fn preauthenticated(identity: IdentityInfo, generation: u64) -> Self {
        Self {
            hello_done: true,
            subscribed: false,
            preauthenticated: true,
            peer_pid: None,
            identity: Some((identity, generation)),
        }
    }
}

/// What the connection thread does with a parsed line.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Write this reply (boxed: replies carry whole snapshots).
    Reply(Box<Reply>),
    /// Register the stream as a subscriber with this identity (the service
    /// writes the `ok`).
    Subscribe(IdentityInfo),
}

/// The capability every request needs.
pub fn required_capability(request: &Request) -> &'static str {
    match request {
        Request::Hello { .. } | Request::State {} | Request::Subscribe {} => CAP_STATE,
        Request::Dial { .. }
        | Request::Answer { .. }
        | Request::Reject { .. }
        | Request::Hangup { .. }
        | Request::Hold { .. }
        | Request::Resume { .. }
        | Request::Dtmf { .. }
        | Request::SetAudioRoute { .. }
        | Request::SetRadioPower { .. }
        | Request::EnterPin { .. } => CAP_CALL,
        Request::SendSms { .. }
        | Request::ListSms { .. }
        | Request::MarkRead { .. }
        | Request::DeleteSms { .. } => CAP_SMS,
    }
}

/// Resolves the connection identity for this request.
fn identity_for(connection: &mut Connection, source: &dyn Identity) -> IdentityInfo {
    if connection.preauthenticated
        && let Some((info, _)) = &connection.identity
    {
        return info.clone();
    }
    let generation = source.generation();
    // Resolve on every request; this is what detects a closed appd link.
    // A generation check alone cannot observe a restart while the link is
    // idle and would retain stale grants indefinitely.
    let info = source.identify(connection.peer_pid);
    connection.identity = Some((info.clone(), generation));
    info
}

fn protocol_error(reason: &str) -> Outcome {
    Outcome::Reply(Box::new(Reply::ProtocolError {
        reason: reason.to_owned(),
    }))
}

fn error(error: TelephonyError) -> Outcome {
    Outcome::Reply(Box::new(Reply::Error { error }))
}

/// Applies one parsed request. Hello must come first and carry the schema;
/// every request is then checked against the connection's verified grants.
pub fn dispatch(
    service: &Arc<Service>,
    source: &dyn Identity,
    connection: &mut Connection,
    request: Request,
) -> Outcome {
    let name = request.name();
    if let Request::Hello {
        schema_version,
        client_kind: _,
    } = &request
    {
        if connection.hello_done {
            return protocol_error("hello was already sent on this connection");
        }
        if *schema_version != SCHEMA_VERSION {
            let reason = format!("schema {schema_version} is not {SCHEMA_VERSION}");
            service.recorder().request_rejected(Some(name), &reason);
            return protocol_error(&reason);
        }
        let identity = identity_for(connection, source);
        if !identity.has(CAP_STATE) {
            service
                .recorder()
                .request_denied(identity.app.as_deref(), CAP_STATE, name);
            return error(TelephonyError::Unauthorized {
                capability: CAP_STATE.to_owned(),
            });
        }
        connection.hello_done = true;
        return Outcome::Reply(Box::new(service.hello_reply(&identity)));
    }
    if !connection.hello_done {
        service
            .recorder()
            .request_rejected(Some(name), "hello must come first");
        return protocol_error("hello must come first");
    }
    let identity = identity_for(connection, source);
    let capability = required_capability(&request);
    if !identity.has(capability) {
        service
            .recorder()
            .request_denied(identity.app.as_deref(), capability, name);
        return error(TelephonyError::Unauthorized {
            capability: capability.to_owned(),
        });
    }
    let reply = match request {
        Request::Hello { .. } => unreachable!("handled above"),
        Request::State {} => Reply::State {
            state: crate::service::project_state(&service.state(), &identity),
        },
        Request::Subscribe {} => {
            if connection.subscribed {
                return protocol_error("already subscribed");
            }
            connection.subscribed = true;
            return Outcome::Subscribe(identity);
        }
        Request::Dial { number } => call_reply(service.dial(&number)),
        Request::Answer { call_id } => call_reply(service.answer(&call_id)),
        Request::Reject { call_id } => call_reply(service.reject(&call_id)),
        Request::Hangup { call_id } => call_reply(service.hangup(&call_id)),
        Request::Hold { call_id } => call_reply(service.hold(&call_id)),
        Request::Resume { call_id } => call_reply(service.resume(&call_id)),
        Request::Dtmf { call_id, digit } => ok_reply(service.dtmf(&call_id, digit)),
        Request::SetAudioRoute { route } => ok_reply(service.set_audio_route(route)),
        Request::SetRadioPower { power } => ok_reply(service.set_radio_power(power)),
        Request::EnterPin { pin } => ok_reply(service.enter_pin(&pin)),
        Request::SendSms { to, body } => sms_reply(service.send_sms(&to, &body)),
        Request::ListSms { filter } => {
            let (messages, total) = service.list_sms(&filter);
            Reply::SmsList { messages, total }
        }
        Request::MarkRead { id } => sms_reply(service.mark_read(&id)),
        Request::DeleteSms { id } => sms_reply(service.delete_sms(&id)),
    };
    Outcome::Reply(Box::new(reply))
}

fn ok_reply(result: Result<(), TelephonyError>) -> Reply {
    match result {
        Ok(()) => Reply::Ok,
        Err(error) => Reply::Error { error },
    }
}

fn call_reply(result: Result<ferrix_telephony_api::Call, TelephonyError>) -> Reply {
    match result {
        Ok(call) => Reply::Call { call },
        Err(error) => Reply::Error { error },
    }
}

fn sms_reply(result: Result<ferrix_telephony_api::Sms, TelephonyError>) -> Reply {
    match result {
        Ok(sms) => Reply::Sms { sms },
        Err(error) => Reply::Error { error },
    }
}

/// Parses one line. Malformed or unknown-field lines never reach the
/// service.
pub fn parse_line(line: &str) -> Result<Request, String> {
    serde_json::from_str(line).map_err(|e| format!("invalid message: {e}"))
}

pub struct Server {
    service: Arc<Service>,
    identity: Arc<dyn Identity>,
}

impl Server {
    pub fn new(service: Arc<Service>, identity: Arc<dyn Identity>) -> Self {
        Self { service, identity }
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

    /// Accept loop; one thread per connection. Returns only if the listener
    /// breaks.
    pub fn serve(self: Arc<Self>, listener: UnixListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let server = Arc::clone(&self);
                    let spawned = thread::Builder::new()
                        .name("ferrix-telephonyd-conn".to_owned())
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

    fn handle(&self, stream: UnixStream) {
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut connection = Connection::with_peer(peer_pid(&stream));
        let mut writer = stream;
        let mut lines = BufReader::new(read_half).lines();
        while let Some(Ok(line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let outcome = match parse_line(line) {
                Ok(request) => dispatch(
                    &self.service,
                    self.identity.as_ref(),
                    &mut connection,
                    request,
                ),
                Err(reason) => {
                    self.service.recorder().request_rejected(None, &reason);
                    protocol_error(&reason)
                }
            };
            match outcome {
                Outcome::Reply(reply) => {
                    if write_json(&mut writer, reply.as_ref()).is_err() {
                        return;
                    }
                }
                Outcome::Subscribe(identity) => {
                    let Ok(sink) = writer.try_clone() else {
                        return;
                    };
                    if self
                        .service
                        .subscribe_authorized(
                            connection.peer_pid,
                            Arc::clone(&self.identity),
                            identity,
                            Box::new(sink),
                        )
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
pub fn peer_pid(stream: &UnixStream) -> Option<u32> {
    rustix::net::sockopt::socket_peercred(stream)
        .ok()
        .and_then(|credentials| u32::try_from(credentials.pid.as_raw_pid()).ok())
}

#[cfg(not(target_os = "linux"))]
pub fn peer_pid(_stream: &UnixStream) -> Option<u32> {
    None
}

fn write_json(stream: &mut UnixStream, value: &impl serde::Serialize) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(std::io::Error::other)?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}
