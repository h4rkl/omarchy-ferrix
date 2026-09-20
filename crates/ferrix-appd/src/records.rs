//! Machine-readable records: one JSON line per record on stdout, `evidence`
//! first, mirroring `ferrix-core` and `ferrix-shell`.
//!
//! The evidence label is `QEMU` inside the guest and `mock` for nested host
//! runs; neither is a physical-device claim. A [`Recorder`] carries the label
//! and a sink so the service can be tested without reading stdout.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferrix_app_api::{AppCapability, AppError, AppId, ExitReason};
use serde_json::{Map, Value};

/// Evidence label for the QEMU guest.
pub const EVIDENCE_QEMU: &str = "QEMU";
/// Evidence label for nested host runs from the mock profile.
pub const EVIDENCE_MOCK: &str = "mock";

/// Prints one JSON record per line and flushes, so serial captures are exact.
pub fn emit(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, value);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

/// Builds a record with `evidence` first, then `event`, then `fields` in
/// order (the `preserve_order` feature keeps insertion order on the wire).
pub fn record(
    evidence: &str,
    event: &str,
    fields: impl IntoIterator<Item = (&'static str, Value)>,
) -> Value {
    let mut map = Map::new();
    map.insert("evidence".to_owned(), Value::String(evidence.to_owned()));
    map.insert("event".to_owned(), Value::String(event.to_owned()));
    for (key, value) in fields {
        map.insert(key.to_owned(), value);
    }
    Value::Object(map)
}

/// Records captured by [`Recorder::capturing`].
pub type Captured = Arc<Mutex<Vec<Value>>>;

type Sink = Arc<dyn Fn(&Value) + Send + Sync>;

/// Emits records with a fixed evidence label to a sink (stdout by default).
#[derive(Clone)]
pub struct Recorder {
    evidence: Arc<str>,
    sink: Sink,
}

impl Recorder {
    /// Records go to stdout, one line each.
    pub fn stdout(evidence: &str) -> Self {
        Self {
            evidence: Arc::from(evidence),
            sink: Arc::new(emit),
        }
    }

    /// Records are collected in memory; for tests.
    pub fn capturing(evidence: &str) -> (Self, Captured) {
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let recorder = Self {
            evidence: Arc::from(evidence),
            sink: Arc::new(move |value: &Value| {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(value.clone());
            }),
        };
        (recorder, captured)
    }

    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    /// Emits an arbitrary record.
    pub fn emit(&self, event: &str, fields: impl IntoIterator<Item = (&'static str, Value)>) {
        (self.sink)(&record(&self.evidence, event, fields));
    }

    pub fn appd_ready(&self, socket: &Path, apps: usize, home: Option<&AppId>) {
        self.emit(
            "ferrix-appd-ready",
            [
                ("socket", Value::from(socket.display().to_string())),
                ("apps", Value::from(apps)),
                ("home", opt_str(home.map(AppId::as_str))),
            ],
        );
    }

    pub fn appd_failed(&self, reason: &str) {
        self.emit(
            "ferrix-appd-failed",
            [
                ("status", Value::from("failed")),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn appd_socket_error(&self, error: &str) {
        self.emit("ferrix-appd-socket-error", [("error", Value::from(error))]);
    }

    /// A message refused by role gating or ordering rules.
    pub fn request_rejected(&self, role: Option<&str>, message: &str, reason: &str) {
        self.emit(
            "ferrix-appd-request-rejected",
            [
                ("role", opt_str(role)),
                ("message", Value::from(message)),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn app_launching(&self, app: &AppId, instance: u32) {
        self.emit(
            "app-launching",
            [
                ("app", Value::from(app.as_str())),
                ("instance", Value::from(instance)),
            ],
        );
    }

    /// `granted` is the effective set (`declared ∩ granted`);
    /// `denied_capabilities` is what the descriptor declared but the grants
    /// policy withheld.
    pub fn app_started(
        &self,
        app: &AppId,
        instance: u32,
        pid: Option<u32>,
        granted: &BTreeSet<AppCapability>,
        denied: &BTreeSet<AppCapability>,
        diagnostics: &Value,
    ) {
        self.emit(
            "app-started",
            [
                ("app", Value::from(app.as_str())),
                ("instance", Value::from(instance)),
                ("pid", pid.map_or(Value::Null, Value::from)),
                ("granted", capabilities(granted)),
                ("denied_capabilities", capabilities(denied)),
                ("runtime_diagnostics", diagnostics.clone()),
            ],
        );
    }

    /// A `service`/`tool` connection resolved `pid`. Emitted once per
    /// connection per pid by the server so a polling daemon cannot flood
    /// the log.
    pub fn appd_identify(&self, pid: u32, app: Option<&AppId>, granted_count: usize) {
        self.emit(
            "appd-identify",
            [
                ("pid", Value::from(pid)),
                ("app", opt_str(app.map(AppId::as_str))),
                ("granted_count", Value::from(granted_count)),
            ],
        );
    }

    pub fn grants_loaded(&self, apps: usize, path: &Path) {
        self.emit(
            "grants-loaded",
            [
                ("apps", Value::from(apps)),
                ("path", Value::from(path.display().to_string())),
            ],
        );
    }

    /// No grants file: the policy is empty and every application has zero
    /// effective capabilities.
    pub fn grants_missing(&self) {
        self.emit("grants-missing", [("apps", Value::from(0))]);
    }

    pub fn grants_invalid(&self, reason: &str) {
        self.emit("grants-invalid", [("reason", Value::from(reason))]);
    }

    pub fn app_launch_failed(&self, app: &AppId, error: &AppError) {
        self.emit(
            "app-launch-failed",
            [
                ("app", Value::from(app.as_str())),
                ("error", serde_json::to_value(error).unwrap_or(Value::Null)),
                ("reason", Value::from(error.to_string())),
            ],
        );
    }

    pub fn app_surface(&self, app: &AppId, pid: u32, app_id: Option<&str>, title: Option<&str>) {
        self.emit(
            "app-surface",
            [
                ("app", Value::from(app.as_str())),
                ("pid", Value::from(pid)),
                ("app_id", opt_str(app_id)),
                ("title", opt_str(title)),
            ],
        );
    }

    /// A compositor surface whose pid belongs to no tracked instance.
    pub fn app_surface_unknown(&self, pid: u32, app_id: Option<&str>) {
        self.emit(
            "app-surface-unknown",
            [("pid", Value::from(pid)), ("app_id", opt_str(app_id))],
        );
    }

    /// The untrusted xdg `app_id` hint disagrees with the pid association.
    pub fn app_id_mismatch(&self, app: &AppId, pid: u32, reported: &str) {
        self.emit(
            "app-id-mismatch",
            [
                ("app", Value::from(app.as_str())),
                ("pid", Value::from(pid)),
                ("reported", Value::from(reported)),
            ],
        );
    }

    pub fn app_foreground(&self, app: &AppId) {
        self.emit("app-foreground", [("app", Value::from(app.as_str()))]);
    }

    pub fn app_background(&self, app: &AppId) {
        self.emit("app-background", [("app", Value::from(app.as_str()))]);
    }

    pub fn app_terminating(&self, app: &AppId, instance: u32) {
        self.emit(
            "app-terminating",
            [
                ("app", Value::from(app.as_str())),
                ("instance", Value::from(instance)),
            ],
        );
    }

    /// Clean exit or requested stop.
    pub fn app_exited(&self, app: &AppId, instance: u32, reason: ExitReason) {
        self.emit(
            "app-exited",
            [
                ("app", Value::from(app.as_str())),
                ("instance", Value::from(instance)),
                ("code", Value::from(reason.shell_status())),
            ],
        );
    }

    /// Signal death or non-zero exit.
    pub fn app_crashed(&self, app: &AppId, instance: u32, reason: ExitReason) {
        let cause = match reason {
            ExitReason::Signaled { signal } => ("signal", Value::from(signal)),
            ExitReason::Exited { code } => ("code", Value::from(code)),
        };
        self.emit(
            "app-crashed",
            [
                ("app", Value::from(app.as_str())),
                ("instance", Value::from(instance)),
                cause,
            ],
        );
    }

    pub fn app_wait_failed(&self, app: &AppId, instance: u32, error: &AppError) {
        self.emit(
            "app-wait-failed",
            [
                ("app", Value::from(app.as_str())),
                ("instance", Value::from(instance)),
                ("reason", Value::from(error.to_string())),
            ],
        );
    }

    pub fn home_restarting(&self, attempt: u32, delay: Duration) {
        self.emit(
            "home-restarting",
            [
                ("attempt", Value::from(attempt)),
                ("delay_ms", Value::from(delay.as_millis() as u64)),
            ],
        );
    }

    pub fn home_restart_abandoned(&self, exits: usize, window: Duration) {
        self.emit(
            "home-restart-abandoned",
            [
                ("exits", Value::from(exits)),
                ("window_ms", Value::from(window.as_millis() as u64)),
            ],
        );
    }
}

fn opt_str(value: Option<&str>) -> Value {
    value.map_or(Value::Null, Value::from)
}

/// A capability set as a JSON array of wire names, in `Ord` order.
fn capabilities(set: &BTreeSet<AppCapability>) -> Value {
    Value::Array(set.iter().map(|c| Value::from(c.name())).collect())
}

/// Event names of captured records, in order; for tests.
pub fn captured_events(captured: &Captured) -> Vec<String> {
    captured
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter_map(|v| v.get("event").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_lead_with_evidence_then_event() {
        let value = record(EVIDENCE_MOCK, "app-foreground", [("app", Value::from("x"))]);
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"evidence":"mock","event":"app-foreground","app":"x"}"#
        );
    }

    #[test]
    fn crash_records_name_signal_or_code() {
        let (recorder, captured) = Recorder::capturing(EVIDENCE_QEMU);
        let app = AppId::new("org.ferrix.terminal").unwrap();
        recorder.app_crashed(&app, 3, ExitReason::Signaled { signal: 11 });
        recorder.app_crashed(&app, 4, ExitReason::Exited { code: 2 });
        recorder.app_exited(&app, 5, ExitReason::Signaled { signal: 15 });
        let records = captured.lock().unwrap();
        assert_eq!(records[0]["evidence"], "QEMU");
        assert_eq!(records[0]["signal"], 11);
        assert!(records[0].get("code").is_none());
        assert_eq!(records[1]["code"], 2);
        assert_eq!(records[2]["event"], "app-exited");
        assert_eq!(records[2]["code"], 143);
        drop(records);
        assert_eq!(
            captured_events(&captured),
            ["app-crashed", "app-crashed", "app-exited"]
        );
    }
}
