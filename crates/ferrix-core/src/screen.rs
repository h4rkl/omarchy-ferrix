//! Screen and suspend policy owned by the core: the user's idle timeout, the
//! live inhibitors (calls, video, navigation, audio, downloads), the shell's
//! screen report, and the board's suspend flag.
//!
//! The shell enforces the screen half (it owns input and the DRM output);
//! the core owns the decision inputs so applications never talk to the
//! compositor. Inhibitors from the state socket are tied to the requesting
//! connection (`server.rs` releases them when it closes); inhibitors from the
//! mock-control channel are named by holder and released explicitly.
//!
//! Suspend (plans/todos/04 deliverable 3) is the core's own decision. It
//! runs only when every one of these holds:
//!
//! * the effective mode is not `off`: the mock-control override, else the
//!   per-boot opt-in file (`/run/ferrix/suspend-opt-in`, contents `s2idle`
//!   or `deep`), else the profile's `[lifecycle] suspend`;
//! * the target offers that sleep state (`Lifecycle::sleep_states`);
//! * the kill switch (`/run/ferrix/no-suspend`) does not exist;
//! * the shell reported the screen off and that report's connection is
//!   still open (a crashed shell can never leave the core suspending);
//! * `after_blank_secs` passed since the screen went off (or since the last
//!   inhibitor was released, or since the last resume) with no inhibitor
//!   held.
//!
//! Then the worker calls `Lifecycle::suspend`, which blocks until resume,
//! counts the cycle, and re-arms: a background wake (RTC, radio) with the
//! screen still off suspends again after another full delay. The deadline
//! passing while an inhibitor or the kill switch blocks it is a
//! `suspend-blocked` record, never a suspend.
//!
//! Every change is one `screen-policy` record on stdout and one
//! [`PolicyUpdate`] line to every watcher. Only the idle timeout persists
//! (`--screen-state`); inhibitors, screen reports, and suspend overrides
//! never outlive the core.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ferrix_hardware_api::{
    Capability, HardwareError, InhibitReason, Lifecycle, SCHEMA_VERSION, ScreenInhibitor,
    ScreenPolicy, SleepState, SuspendMode, SuspendOutcome, SuspendPhase, SuspendReport,
    SuspendSource, SuspendStatus, WakeSource, validate_idle_timeout, validate_suspend_after_blank,
};
use ferrix_protocol::{PolicyUpdate, Reply};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Longest holder label kept (evidence and Settings caption only).
const MAX_HOLDER_LEN: usize = 64;

/// Default kill switch: while this file exists the core never suspends.
pub const DEFAULT_NO_SUSPEND: &str = "/run/ferrix/no-suspend";
/// Default per-boot opt-in: `s2idle` or `deep` here overrides the profile
/// until the file is removed or the device reboots (`/run` is tmpfs).
pub const DEFAULT_SUSPEND_OPT_IN: &str = "/run/ferrix/suspend-opt-in";

/// What persists across core restarts: the user's choice only.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    idle_timeout_secs: Option<u32>,
}

/// The suspend policy's fixed inputs.
pub struct SuspendConfig {
    /// The profile's `[lifecycle] suspend`.
    pub mode: SuspendMode,
    pub after_blank_secs: u32,
    pub kill_switch: PathBuf,
    /// `None` disables the opt-in file.
    pub opt_in: Option<PathBuf>,
    pub lifecycle: Arc<dyn Lifecycle>,
}

struct Inner {
    policy: ScreenPolicy,
    next_id: u64,
    profile_mode: SuspendMode,
    profile_after_blank: u32,
    /// Mock-control overrides (mocked targets only).
    control_mode: Option<SuspendMode>,
    control_after_blank: Option<u32>,
    sleep_states: Vec<SleepState>,
    /// The connection whose screen report is current.
    reporter: Option<u64>,
    /// When the blank-to-suspend delay started; `None` when not counting.
    armed_at: Option<Instant>,
    /// A suspend call is in flight.
    busy: bool,
    was_eligible: bool,
    was_inhibited: bool,
}

pub struct ScreenPolicyStore {
    inner: Mutex<Inner>,
    wake: Condvar,
    watchers: Mutex<Vec<UnixStream>>,
    persist: Option<PathBuf>,
    kill_switch: PathBuf,
    opt_in: Option<PathBuf>,
    lifecycle: Option<Arc<dyn Lifecycle>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn holder_label(holder: &str) -> String {
    let clean: String = holder
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_HOLDER_LEN)
        .collect();
    if clean.trim().is_empty() {
        "unnamed".to_owned()
    } else {
        clean
    }
}

fn write_line(stream: &mut UnixStream, value: &impl Serialize) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(std::io::Error::other)?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

/// The outcome of one deadline check.
enum Deadline {
    Suspend(SleepState),
    Blocked(&'static str),
}

impl ScreenPolicyStore {
    /// Starts from the target profile's default timeout, then the saved
    /// user choice at `persist` when it is readable and valid. Suspend is
    /// disabled (tests; the daemon uses [`ScreenPolicyStore::with_suspend`]).
    #[cfg(test)]
    pub fn new(profile_default: Option<u32>, persist: Option<PathBuf>) -> Self {
        Self::build(profile_default, persist, None)
    }

    /// Like [`ScreenPolicyStore::new`], with the suspend policy's inputs.
    /// Call [`ScreenPolicyStore::spawn_suspend`] to run it.
    pub fn with_suspend(
        profile_default: Option<u32>,
        persist: Option<PathBuf>,
        suspend: SuspendConfig,
    ) -> Self {
        Self::build(profile_default, persist, Some(suspend))
    }

    fn build(
        profile_default: Option<u32>,
        persist: Option<PathBuf>,
        suspend: Option<SuspendConfig>,
    ) -> Self {
        let mut idle = profile_default;
        let mut source = "profile";
        if let Some(path) = &persist {
            match std::fs::read_to_string(path) {
                Ok(text) => match serde_json::from_str::<Saved>(&text)
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        validate_idle_timeout(s.idle_timeout_secs).map_err(|e| e.to_string())
                    }) {
                    Ok(saved) => {
                        idle = saved;
                        source = "saved";
                    }
                    Err(reason) => crate::emit(json!({
                        "evidence": crate::evidence_label(),
                        "event": "screen-policy-invalid",
                        "path": path.display().to_string(),
                        "reason": reason,
                    })),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => crate::emit(json!({
                    "evidence": crate::evidence_label(),
                    "event": "screen-policy-invalid",
                    "path": path.display().to_string(),
                    "reason": e.to_string(),
                })),
            }
        }
        let (profile_mode, profile_after_blank, sleep_states) = match &suspend {
            Some(config) => (
                config.mode,
                config.after_blank_secs,
                config.lifecycle.sleep_states(),
            ),
            None => (
                SuspendMode::Off,
                ferrix_hardware_api::DEFAULT_SUSPEND_AFTER_BLANK_SECS,
                Vec::new(),
            ),
        };
        let (kill_switch, opt_in, lifecycle) = match suspend {
            Some(config) => (config.kill_switch, config.opt_in, Some(config.lifecycle)),
            None => (PathBuf::from(DEFAULT_NO_SUSPEND), None, None),
        };
        let store = Self {
            inner: Mutex::new(Inner {
                policy: ScreenPolicy {
                    idle_timeout_secs: idle,
                    inhibitors: Vec::new(),
                    suspend: SuspendStatus {
                        after_blank_secs: profile_after_blank,
                        ..SuspendStatus::default()
                    },
                },
                next_id: 1,
                profile_mode,
                profile_after_blank,
                control_mode: None,
                control_after_blank: None,
                sleep_states,
                reporter: None,
                armed_at: None,
                busy: false,
                was_eligible: false,
                was_inhibited: false,
            }),
            wake: Condvar::new(),
            watchers: Mutex::new(Vec::new()),
            persist,
            kill_switch,
            opt_in,
            lifecycle,
        };
        let policy = {
            let mut inner = lock(&store.inner);
            store.refresh(&mut inner, Instant::now());
            inner.policy.clone()
        };
        store.record(&policy, source, None);
        store
    }

    pub fn policy(&self) -> ScreenPolicy {
        lock(&self.inner).policy.clone()
    }

    pub fn reply(&self) -> Reply {
        Reply::ScreenPolicy {
            schema_version: SCHEMA_VERSION,
            policy: self.policy(),
        }
    }

    /// Applies `change` under the lock, re-evaluates the suspend policy,
    /// wakes the worker, and publishes the result.
    fn mutate<T>(
        &self,
        cause: &str,
        holder: Option<&str>,
        change: impl FnOnce(&mut Inner) -> T,
    ) -> (T, ScreenPolicy) {
        let (value, policy) = {
            let mut inner = lock(&self.inner);
            let value = change(&mut inner);
            self.refresh(&mut inner, Instant::now());
            (value, inner.policy.clone())
        };
        self.wake.notify_all();
        self.changed(&policy, cause, holder);
        (value, policy)
    }

    pub fn set_idle_timeout(&self, secs: Option<u32>) -> Result<ScreenPolicy, HardwareError> {
        let secs = validate_idle_timeout(secs)?;
        let ((), policy) = self.mutate("set-idle-timeout", None, |inner| {
            inner.policy.idle_timeout_secs = secs;
        });
        self.save(secs);
        Ok(policy)
    }

    pub fn inhibit(&self, reason: InhibitReason, holder: &str) -> (u64, ScreenPolicy) {
        let holder = holder_label(holder);
        self.mutate("inhibit", Some(&holder.clone()), |inner| {
            let id = inner.next_id;
            inner.next_id += 1;
            inner
                .policy
                .inhibitors
                .push(ScreenInhibitor { id, reason, holder });
            id
        })
    }

    /// Releases one inhibitor; `None` when no such id is held.
    pub fn release(&self, id: u64, cause: &str) -> Option<ScreenPolicy> {
        let holder = {
            let inner = lock(&self.inner);
            inner
                .policy
                .inhibitors
                .iter()
                .find(|i| i.id == id)?
                .holder
                .clone()
        };
        let (removed, policy) = self.mutate(cause, Some(&holder), |inner| {
            let index = inner.policy.inhibitors.iter().position(|i| i.id == id);
            index.map(|index| inner.policy.inhibitors.remove(index))
        });
        removed.map(|_| policy)
    }

    /// Releases every inhibitor `holder` holds (mock-control verb);
    /// returns how many were released.
    pub fn release_holder(&self, holder: &str) -> usize {
        let holder = holder_label(holder);
        let ids: Vec<u64> = lock(&self.inner)
            .policy
            .inhibitors
            .iter()
            .filter(|i| i.holder == holder)
            .map(|i| i.id)
            .collect();
        for id in &ids {
            self.release(*id, "release");
        }
        ids.len()
    }

    /// The shell's screen report on connection `connection`.
    pub fn report_screen(&self, connection: u64, off: bool) -> ScreenPolicy {
        self.mutate("report-screen", None, |inner| {
            inner.reporter = Some(connection);
            inner.policy.suspend.screen_off = off;
        })
        .1
    }

    /// A connection closed: when it carried the current screen report, the
    /// screen counts as on again (never suspend without a live shell).
    pub fn connection_closed(&self, connection: u64) {
        let reporter = lock(&self.inner).reporter == Some(connection);
        if reporter {
            self.mutate("reporter-gone", None, |inner| {
                inner.reporter = None;
                inner.policy.suspend.screen_off = false;
            });
        }
    }

    /// Mock-control override of the board flag (`None` restores the
    /// profile's mode or delay).
    pub fn set_suspend_override(
        &self,
        mode: Option<SuspendMode>,
        after_blank_secs: Option<u32>,
    ) -> Result<ScreenPolicy, HardwareError> {
        if let Some(secs) = after_blank_secs {
            validate_suspend_after_blank(secs)?;
        }
        Ok(self
            .mutate("set-suspend-policy", None, |inner| {
                inner.control_mode = mode;
                inner.control_after_blank = after_blank_secs;
            })
            .1)
    }

    /// Re-derives the effective mode, the blockers, the phase, and the
    /// delay's start. Called under the lock after every change.
    fn refresh(&self, inner: &mut Inner, now: Instant) {
        let opt_in = self.opt_in.as_ref().and_then(|path| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| SuspendMode::parse(&text))
        });
        let (mode, source) = match (inner.control_mode, opt_in) {
            (Some(mode), _) => (mode, SuspendSource::Control),
            (None, Some(mode)) => (mode, SuspendSource::OptIn),
            (None, None) => (inner.profile_mode, SuspendSource::Profile),
        };
        let status = &mut inner.policy.suspend;
        status.mode = mode;
        status.source = source;
        status.after_blank_secs = inner
            .control_after_blank
            .unwrap_or(inner.profile_after_blank);
        status.supported = mode
            .sleep_state()
            .is_some_and(|state| inner.sleep_states.contains(&state));
        status.kill_switch = self.lifecycle.is_some() && self.kill_switch.exists();
        let enabled = status.supported && !status.kill_switch;
        let eligible = enabled && status.screen_off && !inner.busy;
        let inhibited = inner.policy.suspend_inhibited();
        if !eligible {
            inner.armed_at = None;
        } else if !inner.was_eligible || (inner.was_inhibited && !inhibited) {
            // Counted from the blank, the last release, or the resume.
            inner.armed_at = Some(now);
        }
        inner.was_eligible = eligible;
        inner.was_inhibited = inhibited;
        let status = &mut inner.policy.suspend;
        status.phase = if inner.busy {
            SuspendPhase::Suspending
        } else if !enabled {
            SuspendPhase::Disabled
        } else if !status.screen_off {
            SuspendPhase::Awake
        } else if inhibited || inner.armed_at.is_none() {
            SuspendPhase::Inhibited
        } else {
            SuspendPhase::Armed
        };
    }

    /// Runs the suspend policy on its own thread. Without a suspend config
    /// (no lifecycle handle) there is nothing to run.
    pub fn spawn_suspend(self: &Arc<Self>) -> Result<(), String> {
        let Some(lifecycle) = self.lifecycle.clone() else {
            return Ok(());
        };
        let store = Arc::clone(self);
        std::thread::Builder::new()
            .name("core-suspend".to_owned())
            .spawn(move || store.run(&*lifecycle))
            .map(|_| ())
            .map_err(|e| format!("cannot start suspend thread: {e}"))
    }

    fn run(&self, lifecycle: &dyn Lifecycle) {
        loop {
            let decision = self.wait_for_deadline();
            match decision {
                Deadline::Blocked(reason) => {
                    let policy = self.policy();
                    crate::emit(json!({
                        "evidence": crate::evidence_label(),
                        "event": "suspend-blocked",
                        "reason": reason,
                        "reasons": policy.reasons().iter().map(|r| r.name()).collect::<Vec<_>>(),
                        "after_blank_secs": policy.suspend.after_blank_secs,
                    }));
                    self.changed(&policy, "suspend-blocked", None);
                }
                Deadline::Suspend(state) => {
                    // The outcome is recorded inside; a failed cycle
                    // re-arms like any other.
                    let _ = self.cycle(lifecycle, state, "policy");
                }
            }
        }
    }

    /// Blocks until the armed delay elapses, then decides under the lock.
    fn wait_for_deadline(&self) -> Deadline {
        let mut inner = lock(&self.inner);
        loop {
            let now = Instant::now();
            let deadline = inner.armed_at.map(|at| {
                at + Duration::from_secs(u64::from(inner.policy.suspend.after_blank_secs))
            });
            match deadline {
                None => {
                    inner = self
                        .wake
                        .wait(inner)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                Some(deadline) if now < deadline => {
                    inner = self
                        .wake
                        .wait_timeout(inner, deadline - now)
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .0;
                }
                Some(_) => {
                    // Re-read the kill switch and opt-in file at the moment
                    // of decision, not only at the last change.
                    self.refresh(&mut inner, now);
                    inner.armed_at = None;
                    if inner.policy.suspend.kill_switch {
                        return Deadline::Blocked("kill-switch");
                    }
                    if inner.policy.suspend_inhibited() {
                        return Deadline::Blocked("inhibited");
                    }
                    if !inner.policy.suspend.screen_off {
                        continue;
                    }
                    match inner.policy.suspend.mode.sleep_state() {
                        Some(state) if inner.policy.suspend.supported => {
                            inner.busy = true;
                            self.refresh(&mut inner, now);
                            return Deadline::Suspend(state);
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    /// One suspend call: `busy` is already set. Records the request and
    /// the resume, counts the cycle, and re-arms.
    fn cycle(
        &self,
        lifecycle: &dyn Lifecycle,
        state: SleepState,
        trigger: &str,
    ) -> Result<SuspendReport, HardwareError> {
        let policy = self.policy();
        crate::emit(json!({
            "evidence": crate::evidence_label(),
            "event": "suspend-request",
            "state": state.name(),
            "trigger": trigger,
            "source": policy.suspend.source.name(),
            "after_blank_secs": policy.suspend.after_blank_secs,
        }));
        self.changed(&policy, "suspend-request", None);
        let result = lifecycle.suspend(state);
        let report = match &result {
            Ok(report) => report.clone(),
            Err(_) => SuspendReport {
                state,
                outcome: SuspendOutcome::Failed,
                wake: WakeSource::Unknown,
                wake_detail: None,
                slept_ms: None,
            },
        };
        crate::emit(json!({
            "evidence": crate::evidence_label(),
            "event": "suspend-resumed",
            "state": state.name(),
            "outcome": report.outcome.name(),
            "wake": report.wake.name(),
            "wake_detail": report.wake_detail,
            "slept_ms": report.slept_ms,
            "reason": result.as_ref().err().map(|e| e.to_string()),
        }));
        self.mutate("resumed", None, |inner| {
            inner.busy = false;
            inner.policy.suspend.cycles = inner.policy.suspend.cycles.saturating_add(1);
            inner.policy.suspend.last = Some(report);
        });
        result
    }

    /// An explicit product `suspend` command: the same gates as the
    /// policy except the screen state and the delay.
    pub fn suspend_now(&self) -> Result<SuspendReport, HardwareError> {
        let lifecycle = self
            .lifecycle
            .clone()
            .ok_or(HardwareError::unsupported(Capability::Lifecycle))?;
        let state = {
            let mut inner = lock(&self.inner);
            self.refresh(&mut inner, Instant::now());
            let status = &inner.policy.suspend;
            let refuse = |reason: &str| Err(HardwareError::invalid(Capability::Lifecycle, reason));
            let state = match status.mode.sleep_state() {
                None => return refuse("suspend is off on this board"),
                Some(_) if !status.supported => {
                    return Err(HardwareError::unsupported(Capability::Lifecycle));
                }
                Some(state) => state,
            };
            if status.kill_switch {
                return refuse("the suspend kill switch exists");
            }
            if inner.busy {
                return refuse("a suspend is already in progress");
            }
            if inner.policy.suspend_inhibited() {
                return refuse("an inhibitor keeps the system awake");
            }
            inner.busy = true;
            self.refresh(&mut inner, Instant::now());
            state
        };
        self.cycle(&*lifecycle, state, "command")
    }

    /// Sends the current policy and registers `stream` for every change.
    /// The watcher lock is held across both so no change is missed.
    pub fn watch(&self, mut stream: UnixStream) -> std::io::Result<()> {
        let mut watchers = lock(&self.watchers);
        write_line(&mut stream, &self.reply())?;
        watchers.push(stream);
        Ok(())
    }

    fn save(&self, secs: Option<u32>) {
        let Some(path) = &self.persist else {
            return;
        };
        let result = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let tmp = path.with_extension("json.tmp");
                let text = serde_json::to_string(&Saved {
                    idle_timeout_secs: secs,
                })
                .map_err(std::io::Error::other)?;
                std::fs::write(&tmp, format!("{text}\n"))?;
                std::fs::rename(&tmp, path)
            });
        if let Err(e) = result {
            crate::emit(json!({
                "evidence": crate::evidence_label(),
                "event": "screen-policy-save-failed",
                "path": path.display().to_string(),
                "reason": e.to_string(),
            }));
        }
    }

    fn record(&self, policy: &ScreenPolicy, cause: &str, holder: Option<&str>) {
        let suspend = &policy.suspend;
        crate::emit(json!({
            "evidence": crate::evidence_label(),
            "event": "screen-policy",
            "cause": cause,
            "holder": holder,
            "idle_timeout_secs": policy.idle_timeout_secs,
            "inhibited": policy.inhibited(),
            "reasons": policy.reasons().iter().map(|r| r.name()).collect::<Vec<_>>(),
            "inhibitors": policy.inhibitors.len(),
            "screen_off": suspend.screen_off,
            "suspend_mode": suspend.mode.name(),
            "suspend_source": suspend.source.name(),
            "suspend_phase": suspend.phase.name(),
            "suspend_after_blank_secs": suspend.after_blank_secs,
            "suspend_kill_switch": suspend.kill_switch,
            "suspend_cycles": suspend.cycles,
        }));
    }

    fn changed(&self, policy: &ScreenPolicy, cause: &str, holder: Option<&str>) {
        self.record(policy, cause, holder);
        let update = PolicyUpdate::ScreenPolicyChanged {
            policy: policy.clone(),
        };
        lock(&self.watchers).retain_mut(|stream| write_line(stream, &update).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_hardware_api::LifecycleState;
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrix-screen-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("screen.json")
    }

    #[test]
    fn idle_timeout_persists_and_inhibitors_do_not() {
        let path = temp("persist");
        let store = ScreenPolicyStore::new(None, Some(path.clone()));
        assert_eq!(store.policy().idle_timeout_secs, None, "profile default");
        store.set_idle_timeout(Some(30)).unwrap();
        let (id, policy) = store.inhibit(InhibitReason::Call, "org.ferrix.phone");
        assert!(policy.inhibited());
        assert_eq!(policy.effective_idle_timeout_secs(), None);
        let restarted = ScreenPolicyStore::new(None, Some(path.clone()));
        let policy = restarted.policy();
        assert_eq!(policy.idle_timeout_secs, Some(30), "the user choice wins");
        assert!(!policy.inhibited(), "inhibitors never outlive the core");
        assert!(store.release(id, "release").is_some());
        assert!(store.release(id, "release").is_none(), "released once");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn invalid_values_are_typed_errors_and_a_bad_file_falls_back() {
        let path = temp("invalid");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"idle_timeout_secs":1}"#).unwrap();
        let store = ScreenPolicyStore::new(Some(60), Some(path.clone()));
        assert_eq!(store.policy().idle_timeout_secs, Some(60));
        assert!(store.set_idle_timeout(Some(2)).is_err());
        assert_eq!(store.policy().idle_timeout_secs, Some(60), "unchanged");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn holders_are_sanitized_and_released_by_name() {
        let store = ScreenPolicyStore::new(Some(60), None);
        store.inhibit(InhibitReason::Video, "scenario");
        store.inhibit(InhibitReason::Navigation, "scenario");
        let (_, policy) = store.inhibit(InhibitReason::Call, "\u{7}");
        assert_eq!(policy.inhibitors[2].holder, "unnamed");
        assert_eq!(store.release_holder("scenario"), 2);
        assert_eq!(store.policy().reasons(), [InhibitReason::Call]);
    }

    #[test]
    fn watchers_get_the_policy_then_every_change() {
        let store = ScreenPolicyStore::new(Some(60), None);
        let (ours, theirs) = UnixStream::pair().unwrap();
        store.watch(ours).unwrap();
        store.set_idle_timeout(Some(15)).unwrap();
        let mut lines = BufReader::new(theirs).lines();
        let first: Reply = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert!(matches!(
            first,
            Reply::ScreenPolicy { policy, .. } if policy.idle_timeout_secs == Some(60)
        ));
        let next: PolicyUpdate = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        let PolicyUpdate::ScreenPolicyChanged { policy } = next;
        assert_eq!(policy.idle_timeout_secs, Some(15));
    }

    /// A lifecycle that records each call and resumes at once.
    struct Recorder {
        calls: Mutex<mpsc::Sender<SleepState>>,
    }

    impl Lifecycle for Recorder {
        fn lifecycle_state(&self) -> LifecycleState {
            LifecycleState::default()
        }
        fn sleep_states(&self) -> Vec<SleepState> {
            vec![SleepState::S2idle]
        }
        fn suspend(&self, state: SleepState) -> Result<SuspendReport, HardwareError> {
            let _ = lock(&self.calls).send(state);
            Ok(SuspendReport {
                state,
                outcome: SuspendOutcome::Resumed,
                wake: WakeSource::Alarm,
                wake_detail: None,
                slept_ms: Some(1),
            })
        }
    }

    struct Rig {
        store: Arc<ScreenPolicyStore>,
        calls: mpsc::Receiver<SleepState>,
        dir: PathBuf,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn rig(name: &str, mode: SuspendMode) -> Rig {
        let dir = temp(name).parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, calls) = mpsc::channel();
        let store = Arc::new(ScreenPolicyStore::with_suspend(
            None,
            None,
            SuspendConfig {
                mode,
                after_blank_secs: 5,
                kill_switch: dir.join("no-suspend"),
                opt_in: Some(dir.join("suspend-opt-in")),
                lifecycle: Arc::new(Recorder {
                    calls: Mutex::new(tx),
                }),
            },
        ));
        Rig { store, calls, dir }
    }

    /// Forces the armed deadline into the past and lets the worker run.
    fn expire(store: &ScreenPolicyStore) {
        {
            let mut inner = lock(&store.inner);
            if let Some(at) = inner.armed_at.as_mut() {
                *at -= Duration::from_secs(3600);
            }
        }
        store.wake.notify_all();
    }

    const WAIT: Duration = Duration::from_secs(5);

    fn wait_phase(store: &ScreenPolicyStore, phase: SuspendPhase) {
        let deadline = Instant::now() + WAIT;
        while store.policy().suspend.phase != phase {
            assert!(Instant::now() < deadline, "phase {phase:?} never came");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn off_by_default_and_without_a_lifecycle() {
        let plain = ScreenPolicyStore::new(None, None);
        plain.report_screen(1, true);
        assert_eq!(plain.policy().suspend.phase, SuspendPhase::Disabled);
        assert!(plain.suspend_now().is_err());
        let rig = rig("off", SuspendMode::Off);
        rig.store.spawn_suspend().unwrap();
        let policy = rig.store.report_screen(1, true);
        assert_eq!(policy.suspend.phase, SuspendPhase::Disabled);
        assert!(lock(&rig.store.inner).armed_at.is_none());
        assert!(
            rig.store.suspend_now().is_err(),
            "off refuses a command too"
        );
        // Deep is not offered by this target: unsupported, never armed.
        rig.store
            .set_suspend_override(Some(SuspendMode::Deep), None)
            .unwrap();
        let policy = rig.store.policy();
        assert!(!policy.suspend.supported);
        assert_eq!(policy.suspend.phase, SuspendPhase::Disabled);
        assert_eq!(
            rig.store.suspend_now(),
            Err(HardwareError::unsupported(Capability::Lifecycle))
        );
        assert!(rig.calls.try_recv().is_err());
    }

    #[test]
    fn blank_delay_suspends_then_rearms_after_resume() {
        let rig = rig("cycle", SuspendMode::S2idle);
        rig.store.spawn_suspend().unwrap();
        assert_eq!(rig.store.policy().suspend.phase, SuspendPhase::Awake);
        rig.store.report_screen(7, true);
        assert_eq!(rig.store.policy().suspend.phase, SuspendPhase::Armed);
        expire(&rig.store);
        assert_eq!(rig.calls.recv_timeout(WAIT), Ok(SleepState::S2idle));
        wait_phase(&rig.store, SuspendPhase::Armed);
        let policy = rig.store.policy();
        assert_eq!(policy.suspend.cycles, 1);
        assert_eq!(
            policy.suspend.last.as_ref().map(|r| r.wake),
            Some(WakeSource::Alarm)
        );
        assert!(
            lock(&rig.store.inner).armed_at.is_some(),
            "screen still off after a background wake: re-armed"
        );
        // The shell's connection closes: the screen counts as on.
        rig.store.connection_closed(99);
        assert_eq!(rig.store.policy().suspend.phase, SuspendPhase::Armed);
        rig.store.connection_closed(7);
        assert_eq!(rig.store.policy().suspend.phase, SuspendPhase::Awake);
        expire(&rig.store);
        std::thread::sleep(Duration::from_millis(50));
        assert!(rig.calls.try_recv().is_err(), "never without a live shell");
    }

    #[test]
    fn inhibitors_block_at_the_deadline_and_release_rearms() {
        let rig = rig("inhibit", SuspendMode::S2idle);
        rig.store.spawn_suspend().unwrap();
        let (id, _) = rig.store.inhibit(InhibitReason::Download, "files");
        let policy = rig.store.report_screen(1, true);
        assert!(!policy.inhibited(), "the screen may still blank");
        assert_eq!(policy.suspend.phase, SuspendPhase::Inhibited);
        expire(&rig.store);
        std::thread::sleep(Duration::from_millis(50));
        assert!(rig.calls.try_recv().is_err(), "a download keeps it awake");
        assert!(rig.store.suspend_now().is_err(), "and refuses a command");
        rig.store.release(id, "release").unwrap();
        assert_eq!(rig.store.policy().suspend.phase, SuspendPhase::Armed);
        expire(&rig.store);
        assert_eq!(rig.calls.recv_timeout(WAIT), Ok(SleepState::S2idle));
    }

    #[test]
    fn kill_switch_wins_and_opt_in_file_enables_for_one_boot() {
        let rig = rig("switches", SuspendMode::Off);
        rig.store.spawn_suspend().unwrap();
        std::fs::write(rig.dir.join("suspend-opt-in"), "s2idle\n").unwrap();
        let policy = rig.store.report_screen(1, true);
        assert_eq!(policy.suspend.mode, SuspendMode::S2idle);
        assert_eq!(policy.suspend.source, SuspendSource::OptIn);
        assert_eq!(policy.suspend.phase, SuspendPhase::Armed);
        // The kill switch appears while armed: checked at the deadline.
        std::fs::write(rig.dir.join("no-suspend"), "").unwrap();
        expire(&rig.store);
        wait_phase(&rig.store, SuspendPhase::Disabled);
        assert!(rig.store.policy().suspend.kill_switch);
        assert!(rig.calls.try_recv().is_err(), "the kill switch always wins");
        assert!(rig.store.suspend_now().is_err());
        std::fs::remove_file(rig.dir.join("no-suspend")).unwrap();
        // Blank again: armed and suspends.
        rig.store.report_screen(1, false);
        rig.store.report_screen(1, true);
        expire(&rig.store);
        assert_eq!(rig.calls.recv_timeout(WAIT), Ok(SleepState::S2idle));
        // An unreadable opt-in value falls back to the profile.
        wait_phase(&rig.store, SuspendPhase::Armed);
        std::fs::write(rig.dir.join("suspend-opt-in"), "mem").unwrap();
        rig.store.report_screen(1, false);
        assert_eq!(rig.store.policy().suspend.mode, SuspendMode::Off);
    }

    #[test]
    fn a_command_suspends_once_with_the_board_mode() {
        let rig = rig("command", SuspendMode::S2idle);
        let report = rig.store.suspend_now().unwrap();
        assert_eq!(report.state, SleepState::S2idle);
        assert_eq!(rig.calls.try_recv(), Ok(SleepState::S2idle));
        let policy = rig.store.policy();
        assert_eq!(policy.suspend.cycles, 1);
        assert_eq!(policy.suspend.phase, SuspendPhase::Awake);
        assert!(
            rig.store.set_suspend_override(None, Some(1)).is_err(),
            "the delay is bounded"
        );
    }
}
