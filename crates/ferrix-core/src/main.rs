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
mod camera;
mod control;
mod core;
mod daemon_kill;
mod power;
mod protocol;
mod screen;
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
use ferrix_protocol::camera::{CAMERA_SOCKET_ENV, DEFAULT_CAMERA_SOCKET};
const DEFAULT_SHUTDOWN_MARKER: &str = "/run/ferrix/shutdown";
const DEFAULT_SCREEN_STATE: &str = "/var/lib/ferrix/screen.json";

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
    // One write(2) per record: several guest processes share the serial
    // console, and a record written in pieces can be split by another
    // writer's line. Serialize first, then hand over the whole line.
    let mut line = serde_json::to_vec(&value).unwrap_or_default();
    line.push(b'\n');
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.flush();
    let _ = stdout.write_all(&line);
    let _ = stdout.flush();
}

struct Args {
    profile: Option<PathBuf>,
    socket: PathBuf,
    /// Camera socket (ADR-009); `None` disables it.
    camera_socket: Option<PathBuf>,
    control: control::ControlPath,
    shutdown_marker: PathBuf,
    /// `None` disables the incoming-pairing raise.
    appd: Option<PathBuf>,
    /// Where an accepted power-off/restart is handed to init; `None` when
    /// the target adapter performs the transition itself.
    power_marker: Option<PathBuf>,
    /// The saved idle timeout; `None` keeps it in memory only.
    screen_state: Option<PathBuf>,
    /// Suspend kill switch: while this file exists the core never suspends.
    no_suspend: PathBuf,
    /// Per-boot suspend opt-in file; `None` disables it.
    suspend_opt_in: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let mut args = Args {
        profile: None,
        socket: PathBuf::from(env("FERRIX_CORE_SOCKET").unwrap_or(DEFAULT_SOCKET.to_owned())),
        camera_socket: Some(PathBuf::from(
            env(CAMERA_SOCKET_ENV).unwrap_or(DEFAULT_CAMERA_SOCKET.to_owned()),
        )),
        control: control::ControlPath::parse(&env("FERRIX_CONTROL_PORT").unwrap_or("auto".into())),
        shutdown_marker: PathBuf::from(
            env("FERRIX_SHUTDOWN_MARKER").unwrap_or(DEFAULT_SHUTDOWN_MARKER.to_owned()),
        ),
        appd: Some(PathBuf::from(
            env(ferrix_app_api::APPD_SOCKET_ENV)
                .unwrap_or(ferrix_app_api::DEFAULT_APPD_SOCKET.to_owned()),
        )),
        power_marker: env("FERRIX_POWER_MARKER").map(PathBuf::from),
        screen_state: Some(PathBuf::from(
            env("FERRIX_SCREEN_STATE").unwrap_or(DEFAULT_SCREEN_STATE.to_owned()),
        )),
        no_suspend: PathBuf::from(
            env("FERRIX_NO_SUSPEND").unwrap_or(screen::DEFAULT_NO_SUSPEND.to_owned()),
        ),
        suspend_opt_in: Some(PathBuf::from(
            env("FERRIX_SUSPEND_OPT_IN").unwrap_or(screen::DEFAULT_SUSPEND_OPT_IN.to_owned()),
        )),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--socket" => args.socket = PathBuf::from(it.next().ok_or("--socket needs a path")?),
            "--camera-socket" => {
                let text = it.next().ok_or("--camera-socket needs PATH|none")?;
                args.camera_socket = (text != "none").then(|| PathBuf::from(text));
            }
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
            "--power-marker" => {
                let text = it.next().ok_or("--power-marker needs PATH|none")?;
                args.power_marker = (text != "none").then(|| PathBuf::from(text));
            }
            "--screen-state" => {
                let text = it.next().ok_or("--screen-state needs PATH|none")?;
                args.screen_state = (text != "none").then(|| PathBuf::from(text));
            }
            "--no-suspend" => {
                args.no_suspend = PathBuf::from(it.next().ok_or("--no-suspend needs a path")?)
            }
            "--suspend-opt-in" => {
                let text = it.next().ok_or("--suspend-opt-in needs PATH|none")?;
                args.suspend_opt_in = (text != "none").then(|| PathBuf::from(text));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: ferrix-core [profile.toml] [--socket PATH] [--camera-socket PATH|none] [--control auto|none|PATH] [--shutdown-marker PATH] [--appd PATH|none] [--power-marker PATH|none] [--screen-state PATH|none] [--no-suspend PATH] [--suspend-opt-in PATH|none]"
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
    screen: Arc<screen::ScreenPolicyStore>,
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
        if let ControlCommand::KillDaemon { name } = &command {
            return self.kill_daemon(name);
        }
        if let Some(ack) = self.screen_command(&command) {
            return ack;
        }
        if let Some(result) = camera::control(&*self.session.services.camera, &command) {
            return self.camera_ack(&command, result);
        }
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
    /// The screen policy verbs (the same store the state socket serves).
    /// `None` for every other command.
    fn screen_command(&self, command: &ControlCommand) -> Option<Ack> {
        let result = match command {
            ControlCommand::SetIdleTimeout { idle_timeout_secs } => self
                .screen
                .set_idle_timeout(*idle_timeout_secs)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            ControlCommand::InhibitBlank { reason, holder } => {
                self.screen.inhibit(*reason, holder);
                Ok(())
            }
            ControlCommand::ReleaseInhibit { holder } => match self.screen.release_holder(holder) {
                0 => Err(format!("{holder:?} holds no inhibitor")),
                _ => Ok(()),
            },
            ControlCommand::SetSuspendPolicy {
                mode,
                after_blank_secs,
            } => {
                if self.session.services.mock_control.is_none() {
                    Err("set-suspend-policy needs the mock-control capability (mocked targets only)".to_owned())
                } else {
                    self.screen
                        .set_suspend_override(*mode, *after_blank_secs)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                }
            }
            _ => return None,
        };
        emit(json!({
            "evidence": evidence_label(),
            "event": "mock-command",
            "cmd": command.name(),
            "status": if result.is_ok() { "ok" } else { "error" },
            "reason": result.as_ref().err(),
        }));
        Some(match result {
            Ok(()) => Ack::Ok {
                ack: command.name().to_owned(),
                events: Vec::new(),
                outcome: None,
                state: self.session.services.snapshot(),
            },
            Err(reason) => Ack::Error {
                ack: Some(command.name().to_owned()),
                reason,
            },
        })
    }

    /// Reports a camera client verb like any other control command.
    fn camera_ack(
        &self,
        command: &ControlCommand,
        result: Result<camera::ControlOutcome, String>,
    ) -> Ack {
        match result {
            Ok(outcome) => {
                for record in &outcome.records {
                    emit(record.clone());
                }
                emit(json!({
                    "evidence": evidence_label(),
                    "event": "mock-command",
                    "cmd": command.name(),
                    "status": "ok",
                    "events": outcome.events(),
                }));
                Ack::Ok {
                    ack: outcome.ack.to_owned(),
                    events: outcome.events(),
                    outcome: None,
                    state: self.session.services.snapshot(),
                }
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

    /// `kill-daemon`: SIGKILL a supervised daemon (mocked targets only). The
    /// restart itself is init's job and shows up as its `daemon-restarted`
    /// record followed by the daemon's own readiness record.
    fn kill_daemon(&self, name: &str) -> Ack {
        let resolved = if self.session.services.mock_control.is_none() {
            Err("kill-daemon needs the mock-control capability (mocked targets only)".to_owned())
        } else {
            daemon_kill::resolve(std::path::Path::new("/proc"), name).map_err(|e| e.reason(name))
        };
        // Reported before the signal: the supervisor's `daemon-restarted`
        // record can only follow it.
        let result = resolved.and_then(|pids| {
            emit(json!({
                "evidence": evidence_label(),
                "event": "daemon-kill",
                "name": name,
                "pids": pids,
                "signal": "KILL",
                "status": "sending",
            }));
            daemon_kill::kill(&pids).map_err(|e| e.reason(name))
        });
        match result {
            Ok(()) => Ack::Ok {
                ack: "kill-daemon".to_owned(),
                events: Vec::new(),
                outcome: None,
                state: self.session.services.snapshot(),
            },
            Err(reason) => {
                emit(json!({
                    "evidence": evidence_label(),
                    "event": "daemon-kill",
                    "name": name,
                    "status": "error",
                    "reason": reason,
                }));
                Ack::Error {
                    ack: Some("kill-daemon".to_owned()),
                    reason,
                }
            }
        }
    }

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
    let lifecycle = session.profile.lifecycle_section();
    let screen = Arc::new(screen::ScreenPolicyStore::with_suspend(
        session.profile.display.idle_timeout(),
        args.screen_state.clone(),
        screen::SuspendConfig {
            mode: lifecycle.suspend,
            after_blank_secs: lifecycle.suspend_after_blank(),
            kill_switch: args.no_suspend.clone(),
            opt_in: args.suspend_opt_in.clone(),
            lifecycle: Arc::clone(&session.services.lifecycle),
        },
    ));
    screen.spawn_suspend()?;
    let server = Arc::new(Server::new(
        Arc::clone(&session),
        Arc::clone(&screen),
        power::PowerHandoff {
            marker: args.power_marker.clone(),
        },
    ));
    let subscribers: Subscribers = server.subscribers();

    let control_path = args.control.resolve();
    let control_status = match &control_path {
        Some(path) => {
            let controller: Arc<dyn control::Handler> = Arc::new(Controller {
                session: Arc::clone(&session),
                shutdown_marker: args.shutdown_marker.clone(),
                screen: Arc::clone(&screen),
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

    // The camera on its own socket, authorized per request through appd.
    if let Some(path) = &args.camera_socket {
        let listener = camera::CameraServer::bind(path)?;
        let auth: Arc<dyn camera::CameraAuth> = match &args.appd {
            Some(socket) => Arc::new(camera::AppdAuth::new(
                socket.clone(),
                std::time::Duration::from_secs(2),
            )),
            None => Arc::new(camera::DenyAll),
        };
        let camera_server = Arc::new(camera::CameraServer::new(Arc::clone(&session), auth));
        emit(camera_server.ready_record(path));
        thread::Builder::new()
            .name("core-camera".to_owned())
            .spawn(move || camera_server.serve(listener))
            .map_err(|e| format!("cannot start camera socket thread: {e}"))?;
    }

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
