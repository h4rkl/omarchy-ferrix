//! `ferrix-core`: the persistent Rust mobile core.
//!
//! Bootstraps hardware access through `ferrix-session` (never through an
//! adapter directly), then:
//!
//! 1. binds the state socket (`--socket`, `FERRIX_CORE_SOCKET`, default
//!    `/run/ferrix/core.sock`) and only then prints the machine-readable
//!    `boot-health` record — a record is evidence that the services exist;
//! 2. opens the mock-control channel when the target has one
//!    (`--control auto|none|PATH`, `FERRIX_CONTROL_PORT`);
//! 3. publishes every hardware event as a JSON record on stdout (serial in
//!    the guest) and to every subscribed client;
//! 4. raises the system `show-bluetooth-pairing` handler through
//!    `ferrix-appd` (`--appd PATH|none`, `FERRIX_APPD_SOCKET`) when a remote
//!    starts pairing and its request waits on the user.
//!
//! Any startup failure prints a `boot-failed` record and exits non-zero.
//! Nothing here knows about QEMU beyond the neutral control-port name.

mod attention;
mod control;
mod core;
mod protocol;
mod server;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;

use ferrix_session::{BootstrapOptions, Session, bootstrap};
use serde_json::{Value, json};

use crate::protocol::{Ack, ControlCommand};
use crate::server::{Server, Subscribers, broadcast};

const DEFAULT_SOCKET: &str = "/run/ferrix/core.sock";
const DEFAULT_SHUTDOWN_MARKER: &str = "/run/ferrix/shutdown";

/// The target bootstrap supplies an exact redacted evidence label for a
/// physical board. Development defaults stay compatible with the QEMU/mock
/// routes when no label is supplied.
pub(crate) fn evidence_label() -> String {
    std::env::var("FERRIX_EVIDENCE")
        .ok()
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "mock".to_owned())
}

/// Prints one JSON record per line and flushes, so serial captures are exact.
pub(crate) fn emit(value: Value) {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &value);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

struct Args {
    profile: Option<PathBuf>,
    socket: PathBuf,
    control: control::ControlPath,
    shutdown_marker: PathBuf,
    /// `None` disables the incoming-pairing raise.
    appd: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let mut args = Args {
        profile: None,
        socket: PathBuf::from(env("FERRIX_CORE_SOCKET").unwrap_or(DEFAULT_SOCKET.to_owned())),
        control: control::ControlPath::parse(&env("FERRIX_CONTROL_PORT").unwrap_or("auto".into())),
        shutdown_marker: PathBuf::from(
            env("FERRIX_SHUTDOWN_MARKER").unwrap_or(DEFAULT_SHUTDOWN_MARKER.to_owned()),
        ),
        appd: Some(PathBuf::from(
            env(ferrix_app_api::APPD_SOCKET_ENV)
                .unwrap_or(ferrix_app_api::DEFAULT_APPD_SOCKET.to_owned()),
        )),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--socket" => args.socket = PathBuf::from(it.next().ok_or("--socket needs a path")?),
            "--control" => {
                args.control =
                    control::ControlPath::parse(&it.next().ok_or("--control needs auto|none|PATH")?)
            }
            "--shutdown-marker" => {
                args.shutdown_marker =
                    PathBuf::from(it.next().ok_or("--shutdown-marker needs a path")?)
            }
            "--appd" => {
                let text = it.next().ok_or("--appd needs PATH|none")?;
                args.appd = (text != "none").then(|| PathBuf::from(text));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: ferrix-core [profile.toml] [--socket PATH] [--control auto|none|PATH] [--shutdown-marker PATH] [--appd PATH|none]"
                        .to_owned(),
                );
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            path => args.profile = Some(PathBuf::from(path)),
        }
    }
    Ok(args)
}

/// Applies control commands and reports them; shared by the control channel.
struct Controller {
    session: Arc<Session>,
    shutdown_marker: PathBuf,
}

impl control::Handler for Controller {
    fn handle(&self, command: Result<ControlCommand, String>) -> Ack {
        let command = match command {
            Ok(c) => c,
            Err(reason) => {
                emit(json!({
                    "evidence": evidence_label(),
                    "event": "mock-command",
                    "status": "error",
                    "reason": reason,
                }));
                return Ack::Error { ack: None, reason };
            }
        };
        match core::apply(&self.session.services, &command) {
            Ok(applied) => {
                emit(json!({
                    "evidence": evidence_label(),
                    "event": "mock-command",
                    "cmd": command.name(),
                    "status": "ok",
                    "events": applied.events.iter().map(|e| e.name()).collect::<Vec<_>>(),
                }));
                if applied.shutdown {
                    self.request_shutdown();
                }
                core::ack_for(&self.session.services, &command, &applied)
            }
            Err(reason) => {
                emit(json!({
                    "evidence": evidence_label(),
                    "event": "mock-command",
                    "cmd": command.name(),
                    "status": "error",
                    "reason": reason,
                }));
                Ack::Error {
                    ack: Some(command.name().to_owned()),
                    reason,
                }
            }
        }
    }
}

impl Controller {
    /// The core does not power the machine off itself; it leaves a marker the
    /// init script acts on so that a host-driven check ends deterministically.
    fn request_shutdown(&self) {
        // Report first: init powers off as soon as the marker exists.
        emit(json!({
            "evidence": evidence_label(),
            "event": "shutdown",
            "status": "accepted",
            "marker": self.shutdown_marker.display().to_string(),
        }));
        let result = self
            .shutdown_marker
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&self.shutdown_marker, b"shutdown\n"));
        match result {
            Ok(()) => {}
            Err(e) => emit(json!({
                "evidence": evidence_label(),
                "event": "shutdown",
                "status": "failed",
                "marker": self.shutdown_marker.display().to_string(),
                "error": e.to_string(),
            })),
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let mut options = BootstrapOptions::from_env();
    if let Some(profile) = args.profile {
        options.profile_path = Some(profile);
    }
    let session = Arc::new(bootstrap(options).map_err(|e| e.to_string())?);

    // Subscribe before anything can mutate state so no event is missed.
    let events = session.services.subscribe();
    let listener = Server::bind(&args.socket)?;
    let server = Arc::new(Server::new(Arc::clone(&session)));
    let subscribers: Subscribers = server.subscribers();

    let control_path = args.control.resolve();
    let control_status = match &control_path {
        Some(path) => {
            let controller: Arc<dyn control::Handler> = Arc::new(Controller {
                session: Arc::clone(&session),
                shutdown_marker: args.shutdown_marker.clone(),
            });
            control::spawn(path.clone(), controller)?;
            json!({"status": "ready", "path": path.display().to_string()})
        }
        None => json!({"status": "unavailable"}),
    };

    let state = session.services.snapshot();
    emit(json!({
        "evidence": evidence_label(),
        "event": "boot-health",
        "status": "ok",
        "target": session.target_id,
        "profile": session.profile.name,
        "schema_version": ferrix_hardware_api::SCHEMA_VERSION,
        "battery_percent": state.power.battery_percent,
        "charging": state.power.charging,
        "socket": args.socket.display().to_string(),
        "capabilities": session.capabilities,
    }));
    emit(json!({
        "evidence": evidence_label(),
        "event": "mock-control",
        "status": control_status["status"],
        "path": control_status.get("path").cloned().unwrap_or(Value::Null),
    }));

    thread::Builder::new()
        .name("core-socket".to_owned())
        .spawn(move || server.serve(listener))
        .map_err(|e| format!("cannot start socket thread: {e}"))?;

    // Incoming Bluetooth pairing requests raise Settings system-wide.
    let attention = match args.appd {
        Some(socket) => attention::Attention::spawn(socket, std::time::Duration::from_secs(2))?,
        None => attention::Attention::disabled(),
    };
    let mut incoming = attention::IncomingEdge::default();

    // Event pump: serial record + fan-out. Runs until the adapter goes away.
    for event in events {
        // Records lead with the evidence label, then the event's own fields.
        let mut record = serde_json::Map::new();
        record.insert("evidence".to_owned(), Value::String(evidence_label()));
        if let Ok(Value::Object(fields)) = serde_json::to_value(event) {
            record.extend(fields);
        }
        emit(Value::Object(record));
        broadcast(&subscribers, &event);
        if let ferrix_hardware_api::HardwareEvent::BluetoothSessionChanged(summary) = &event
            && incoming.update(summary)
        {
            attention.incoming_pairing();
        }
    }
    Err("hardware event source closed".to_owned())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            emit(json!({
                "evidence": evidence_label(),
                "event": "boot-failed",
                "status": "failed",
                "reason": error,
            }));
            ExitCode::FAILURE
        }
    }
}
