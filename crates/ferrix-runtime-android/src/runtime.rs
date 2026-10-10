//! [`AndroidRuntime`]: the [`ferrix_app_runtime_api::Runtime`] for
//! [`RuntimeKind::Android`], generic over an [`AndroidHost`].

use std::collections::BTreeSet;
use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferrix_app_api::{AppDescriptor, AppError, AppId, ExitReason, RuntimeKind};
use ferrix_app_runtime_api::{LaunchContext, RunningApp, Runtime, SurfaceMatch};
use serde_json::{Value, json};

use crate::config::AndroidLaunch;
use crate::host::{AndroidHost, HostError, LaunchedApp, RuntimeStatus};

/// How long a launch waits for the runtime session to come up. Generous
/// because the guest runs under TCG and the container boots a full Android
/// userspace.
pub const DEFAULT_SESSION_TIMEOUT: Duration = Duration::from_secs(180);
/// How long after `launch` a missing process is still "starting" rather than
/// "gone": `am start` returns before the zygote has forked the application.
const APPEAR_GRACE: Duration = Duration::from_secs(30);
/// Polling interval for `wait`/`terminate`.
const POLL: Duration = Duration::from_millis(100);
/// Consecutive "not running" observations before an exit is declared, so a
/// momentary `/proc` race does not count as a crash.
const GONE_CONFIRMATIONS: u8 = 2;
/// How often a running app re-reads the container's Wayland client pids
/// (multi-window mode may open connections after the launch returned).
const SURFACE_PIDS_REFRESH: Duration = Duration::from_millis(500);

/// Options for [`AndroidRuntime`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidRuntimeOptions {
    /// Evidence label for records (`QEMU`/`mock`); the launch context's label
    /// wins when set.
    pub evidence: String,
    /// Bound for `ensure_session`.
    pub session_timeout: Duration,
    /// Grace period before a never-seen process counts as gone.
    pub appear_grace: Duration,
}

impl Default for AndroidRuntimeOptions {
    fn default() -> Self {
        Self {
            evidence: "mock".to_owned(),
            session_timeout: DEFAULT_SESSION_TIMEOUT,
            appear_grace: APPEAR_GRACE,
        }
    }
}

/// Android compatibility runtime adapter.
pub struct AndroidRuntime {
    host: Arc<dyn AndroidHost>,
    options: AndroidRuntimeOptions,
}

impl AndroidRuntime {
    pub fn new(host: Arc<dyn AndroidHost>, options: AndroidRuntimeOptions) -> Self {
        Self { host, options }
    }

    pub fn options(&self) -> &AndroidRuntimeOptions {
        &self.options
    }

    /// Current runtime status (never starts anything).
    pub fn status(&self) -> RuntimeStatus {
        self.host.status()
    }

    fn evidence<'a>(&'a self, context: &'a LaunchContext) -> &'a str {
        if context.evidence.is_empty() {
            &self.options.evidence
        } else {
            &context.evidence
        }
    }

    /// Emits a runtime-private evidence record on stdout (flows into
    /// `ferrix-appd`'s log like the sandbox's records). Carries package ids
    /// and states only.
    fn record(&self, evidence: &str, event: &str, app: &AppId, extra: Value) {
        emit_record(evidence, self.host.name(), event, app, extra);
    }

    /// Parses and validates the daemon-private `launch` object.
    pub fn parse_launch(app: &AppDescriptor, config: &Value) -> Result<AndroidLaunch, AppError> {
        let launch: AndroidLaunch = serde_json::from_value(config.clone()).map_err(|error| {
            AppError::InvalidDescriptor {
                id: Some(app.id.clone()),
                reason: format!("invalid android launch configuration: {error}"),
            }
        })?;
        launch
            .validate()
            .map_err(|reason| AppError::InvalidDescriptor {
                id: Some(app.id.clone()),
                reason: format!("invalid android launch configuration: {reason}"),
            })?;
        Ok(launch)
    }

    /// Explicit installation step for a tool or workflow: installs the APK at
    /// `launch.apk.path` (already verified against its configured pins).
    /// `launch()` never installs.
    pub fn install(&self, app: &AppDescriptor, config: &Value) -> Result<(), AppError> {
        let launch = Self::parse_launch(app, config)?;
        let path = launch
            .apk
            .as_ref()
            .and_then(|apk| apk.path.as_deref())
            .ok_or_else(|| AppError::LaunchFailed {
                id: app.id.clone(),
                reason: "no apk.path in the launch configuration".to_owned(),
            })?;
        let path = std::path::Path::new(path);
        if !path.is_file() {
            return Err(AppError::LaunchFailed {
                id: app.id.clone(),
                reason: format!("apk not found at {}", path.display()),
            });
        }
        self.host
            .install(path)
            .map_err(|error| map_host_error(&app.id, error))
    }
}

/// Writes one runtime record line to stdout.
fn emit_record(evidence: &str, host: &str, event: &str, app: &AppId, extra: Value) {
    let mut record = json!({
        "evidence": evidence,
        "event": event,
        "runtime": "android",
        "host": host,
        "app": app.as_str(),
    });
    if let (Some(target), Some(extra)) = (record.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    // One write(2) for the whole line so another writer sharing the log
    // cannot land inside it.
    let line = format!("{record}\n");
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.flush();
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.flush();
}

fn map_host_error(id: &AppId, error: HostError) -> AppError {
    match error {
        HostError::Absent(_) => AppError::RuntimeUnsupported {
            runtime: RuntimeKind::Android,
        },
        HostError::NotInstalled => AppError::LaunchFailed {
            id: id.clone(),
            reason: "not-installed".to_owned(),
        },
        HostError::SessionFailed(reason) => AppError::LaunchFailed {
            id: id.clone(),
            reason: format!("session-failed: {reason}"),
        },
        HostError::CommandFailed(reason) => AppError::LaunchFailed {
            id: id.clone(),
            reason,
        },
    }
}

impl Runtime for AndroidRuntime {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Android
    }

    fn launch(
        &self,
        app: &AppDescriptor,
        config: &Value,
        context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        if app.runtime != RuntimeKind::Android {
            return Err(AppError::RuntimeUnsupported {
                runtime: app.runtime,
            });
        }
        let launch = Self::parse_launch(app, config)?;
        let evidence = self.evidence(context).to_owned();

        // 1. Runtime present at all?
        match self.host.status() {
            RuntimeStatus::Absent { reason } => {
                self.record(
                    &evidence,
                    "android-runtime-absent",
                    &app.id,
                    json!({ "reason": reason }),
                );
                return Err(AppError::RuntimeUnsupported {
                    runtime: RuntimeKind::Android,
                });
            }
            RuntimeStatus::Failed { reason } => {
                self.record(
                    &evidence,
                    "android-session-failed",
                    &app.id,
                    json!({ "reason": reason }),
                );
                return Err(AppError::LaunchFailed {
                    id: app.id.clone(),
                    reason: format!("session-failed: {reason}"),
                });
            }
            RuntimeStatus::Stopped | RuntimeStatus::Starting | RuntimeStatus::Running { .. } => {}
        }

        // 2. Session/container up.
        let session = match self
            .host
            .ensure_session(&context.env, self.options.session_timeout)
        {
            Ok(status) => status,
            Err(error) => {
                self.record(
                    &evidence,
                    "android-session-failed",
                    &app.id,
                    json!({ "reason": error.to_string() }),
                );
                return Err(map_host_error(&app.id, error));
            }
        };

        // 3. Package installed? Launching never installs.
        let installed = self
            .host
            .is_installed(&launch.package)
            .map_err(|error| map_host_error(&app.id, error))?;
        if !installed {
            self.record(
                &evidence,
                "android-app-not-installed",
                &app.id,
                json!({ "package": launch.package }),
            );
            return Err(AppError::LaunchFailed {
                id: app.id.clone(),
                reason: "not-installed".to_owned(),
            });
        }

        // 4. Start it.
        let launched = self
            .host
            .launch(&launch.package, launch.activity.as_deref())
            .map_err(|error| map_host_error(&app.id, error))?;
        self.record(
            &evidence,
            "android-app-launched",
            &app.id,
            json!({
                "package": launch.package,
                "pid": launched.pid,
                "surface_client_pid": launched.surface_client_pid,
                "window_app_id": self.host.window_app_id(&launch.package),
                "instance": context.instance,
            }),
        );
        let window_app_id = self.host.window_app_id(&launch.package);
        let surface_pids = self.host.surface_client_pids().into_iter().collect();
        Ok(Box::new(AndroidApp {
            id: app.id.clone(),
            host: Arc::clone(&self.host),
            host_name: self.host.name(),
            evidence,
            window_app_id,
            surface_pids,
            surface_pids_read: Instant::now(),
            reopens: 0,
            package: launch.package,
            activity: launch.activity,
            launched,
            session,
            started: Instant::now(),
            appear_grace: self.options.appear_grace,
            seen_running: launched.pid.is_some(),
            gone_observations: 0,
            stop_requested: false,
            finished: None,
        }))
    }
}

/// One launched Android application, supervised by polling the host.
struct AndroidApp {
    id: AppId,
    host: Arc<dyn AndroidHost>,
    host_name: &'static str,
    evidence: String,
    /// xdg `app_id` of this package's task windows.
    window_app_id: String,
    /// Container Wayland client pids last read from the host.
    surface_pids: BTreeSet<u32>,
    surface_pids_read: Instant,
    reopens: u32,
    package: String,
    activity: Option<String>,
    launched: LaunchedApp,
    session: RuntimeStatus,
    started: Instant,
    appear_grace: Duration,
    seen_running: bool,
    gone_observations: u8,
    stop_requested: bool,
    finished: Option<Result<ExitReason, AppError>>,
}

impl AndroidApp {
    /// Classifies the process disappearing: a stop this adapter requested is
    /// a termination; anything else is recorded as a crash because the host
    /// cannot distinguish an Android-side finish from a crash or a low-memory
    /// kill (see `docs/android-runtime.md`, "Lifecycle").
    fn gone(&mut self) -> ExitReason {
        let reason = if self.stop_requested {
            ExitReason::Signaled { signal: 15 }
        } else {
            ExitReason::Exited { code: 1 }
        };
        self.finished = Some(Ok(reason));
        reason
    }

    /// Re-reads the container's Wayland client pids, at most every
    /// [`SURFACE_PIDS_REFRESH`] (or right away while none is known).
    fn refresh_surface_pids(&mut self) {
        if !self.surface_pids.is_empty() && self.surface_pids_read.elapsed() < SURFACE_PIDS_REFRESH
        {
            return;
        }
        self.surface_pids_read = Instant::now();
        let pids: BTreeSet<u32> = self.host.surface_client_pids().into_iter().collect();
        if let Some(first) = pids.first() {
            self.launched.surface_client_pid = Some(*first);
        }
        self.surface_pids = pids;
    }

    fn observe(&mut self) -> Result<Option<ExitReason>, AppError> {
        if let Some(finished) = &self.finished {
            return finished.clone().map(Some);
        }
        self.refresh_surface_pids();
        let running = self
            .host
            .is_running(&self.package)
            .map_err(|error| map_host_error(&self.id, error))?;
        if running {
            self.seen_running = true;
            self.gone_observations = 0;
            if self.launched.pid.is_none() {
                self.launched.pid = self.host.app_pid(&self.package);
            }
            return Ok(None);
        }
        if !self.seen_running && self.started.elapsed() < self.appear_grace {
            return Ok(None);
        }
        self.gone_observations = self.gone_observations.saturating_add(1);
        if self.gone_observations >= GONE_CONFIRMATIONS || self.stop_requested {
            return Ok(Some(self.gone()));
        }
        Ok(None)
    }
}

impl RunningApp for AndroidApp {
    /// Host pid of the Android application process, when visible. Surfaces
    /// are not associated through it: the windows come from the container's
    /// shared Wayland client (see [`RunningApp::surface_match`]).
    fn pid(&self) -> Option<u32> {
        self.launched.pid
    }

    /// Task windows of this package: drawn by the container's Wayland
    /// client(s), one toplevel per Android task, named
    /// [`AndroidHost::window_app_id`].
    fn surface_match(&self) -> SurfaceMatch {
        SurfaceMatch::SharedHost {
            client_pids: self.surface_pids.clone(),
            app_id: self.window_app_id.clone(),
        }
    }

    /// Re-issues the launch so Android creates (or raises) the package's
    /// task when its window is gone.
    fn reopen(&mut self) -> Result<bool, AppError> {
        if self.finished.is_some() {
            return Ok(false);
        }
        let launched = self
            .host
            .launch(&self.package, self.activity.as_deref())
            .map_err(|error| map_host_error(&self.id, error))?;
        self.reopens += 1;
        if launched.pid.is_some() {
            self.launched.pid = launched.pid;
        }
        // A relaunch restarts the appearance window: the process may have
        // been reaped by Android while the task was gone.
        self.started = Instant::now();
        // Re-read the composer pids on the next poll.
        let now = Instant::now();
        self.surface_pids_read = now.checked_sub(SURFACE_PIDS_REFRESH).unwrap_or(now);
        emit_record(
            &self.evidence,
            self.host_name,
            "android-app-reopened",
            &self.id,
            json!({ "package": self.package, "pid": self.launched.pid, "reopens": self.reopens }),
        );
        Ok(true)
    }

    fn diagnostics(&self) -> Value {
        json!({
            "kind": "android",
            "host": self.host_name,
            "package": self.package,
            "activity": self.activity,
            "pid": self.launched.pid,
            "surface_client_pid": self.launched.surface_client_pid,
            "surface_client_pids": self.surface_pids,
            "window_app_id": self.window_app_id,
            "session": self.session,
            "exit_cause": self.finished.as_ref().map(|f| match f {
                Ok(_) if self.stop_requested => "stopped",
                Ok(_) => "process-gone",
                Err(_) => "host-error",
            }),
        })
    }

    fn try_wait(&mut self) -> Result<Option<ExitReason>, AppError> {
        self.observe()
    }

    fn wait(&mut self) -> Result<ExitReason, AppError> {
        loop {
            if let Some(reason) = self.observe()? {
                return Ok(reason);
            }
            std::thread::sleep(POLL);
        }
    }

    fn terminate(&mut self, grace: Duration) -> Result<ExitReason, AppError> {
        if let Some(finished) = &self.finished {
            return finished.clone();
        }
        self.stop_requested = true;
        self.host
            .stop(&self.package)
            .map_err(|error| map_host_error(&self.id, error))?;
        let deadline = Instant::now() + grace;
        loop {
            if let Some(reason) = self.observe()? {
                return Ok(reason);
            }
            if Instant::now() >= deadline {
                // The Android side did not honour force-stop in time; report
                // it rather than pretend.
                let error = AppError::LaunchFailed {
                    id: self.id.clone(),
                    reason: format!(
                        "android app still running {}s after force-stop",
                        grace.as_secs()
                    ),
                };
                self.finished = Some(Err(error.clone()));
                return Err(error);
            }
            std::thread::sleep(POLL.min(grace));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeAndroidHost;
    use ferrix_app_api::{AppCapability, AppRole, IconRef, SCHEMA_VERSION, TrustLevel};

    const PACKAGE: &str = "net.slions.fulguris.full.fdroid";

    fn descriptor() -> AppDescriptor {
        AppDescriptor {
            schema_version: SCHEMA_VERSION,
            id: AppId::new("org.ferrix.browser").unwrap(),
            runtime: RuntimeKind::Android,
            name: "Browser".to_owned(),
            icon: IconRef::default(),
            launchable: true,
            autostart: false,
            role: AppRole::Application,
            trust: TrustLevel::Untrusted,
            capabilities: Default::default(),
            intents: Default::default(),
        }
    }

    fn config() -> Value {
        json!({
            "package": PACKAGE,
            "activity": null,
            "apk": { "pins": "tools/android/fulguris.pins" }
        })
    }

    fn context() -> LaunchContext {
        LaunchContext {
            instance: 1,
            env: vec![("WAYLAND_DISPLAY".to_owned(), "wayland-0".to_owned())],
            evidence: "mock".to_owned(),
            // The browser's effective set: `background` only (ADR-005).
            granted: [AppCapability::Background].into_iter().collect(),
        }
    }

    /// The error of a launch, or a panic when it unexpectedly succeeded.
    fn launch_error(result: Result<Box<dyn RunningApp>, AppError>) -> AppError {
        match result {
            Ok(_) => panic!("launch unexpectedly succeeded"),
            Err(error) => error,
        }
    }

    fn runtime(host: &FakeAndroidHost) -> AndroidRuntime {
        AndroidRuntime::new(
            Arc::new(host.clone()),
            AndroidRuntimeOptions {
                appear_grace: Duration::from_millis(0),
                ..AndroidRuntimeOptions::default()
            },
        )
    }

    #[test]
    fn kind_is_android_and_other_kinds_are_refused() {
        let host = FakeAndroidHost::new();
        let runtime = runtime(&host);
        assert_eq!(runtime.kind(), RuntimeKind::Android);
        let mut native = descriptor();
        native.runtime = RuntimeKind::Native;
        assert!(matches!(
            runtime.launch(&native, &config(), &context()),
            Err(AppError::RuntimeUnsupported {
                runtime: RuntimeKind::Native
            })
        ));
    }

    #[test]
    fn invalid_launch_configuration_is_a_descriptor_error() {
        let host = FakeAndroidHost::new();
        let runtime = runtime(&host);
        for bad in [
            json!({}),
            json!({ "package": "fulguris" }),
            json!({ "package": PACKAGE, "exec": ["x"] }),
            json!({ "package": PACKAGE, "activity": "a b" }),
        ] {
            assert!(
                matches!(
                    runtime.launch(&descriptor(), &bad, &context()),
                    Err(AppError::InvalidDescriptor { .. })
                ),
                "{bad}"
            );
        }
        assert!(host.launches().is_empty());
    }

    #[test]
    fn absent_runtime_is_unsupported() {
        let host = FakeAndroidHost::absent("waydroid not found");
        let runtime = runtime(&host);
        assert!(matches!(runtime.status(), RuntimeStatus::Absent { .. }));
        assert!(matches!(
            runtime.launch(&descriptor(), &config(), &context()),
            Err(AppError::RuntimeUnsupported {
                runtime: RuntimeKind::Android
            })
        ));
        assert_eq!(host.session_starts(), 0);
        assert!(host.launches().is_empty());
    }

    #[test]
    fn failed_session_is_a_launch_failure() {
        let host = FakeAndroidHost::new();
        host.install_package(PACKAGE);
        host.fail_session_start("container failed: lxc-start exited 1");
        let runtime = runtime(&host);
        match launch_error(runtime.launch(&descriptor(), &config(), &context())) {
            AppError::LaunchFailed { id, reason } => {
                assert_eq!(id.as_str(), "org.ferrix.browser");
                assert!(reason.starts_with("session-failed: "), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(host.launches().is_empty());

        // A runtime that reports Failed before any start attempt.
        let host = FakeAndroidHost::new();
        host.set_status(RuntimeStatus::Failed {
            reason: "not-initialized".to_owned(),
        });
        match launch_error(self::tests::runtime(&host).launch(&descriptor(), &config(), &context()))
        {
            AppError::LaunchFailed { reason, .. } => {
                assert_eq!(reason, "session-failed: not-initialized");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn not_installed_is_reported_and_never_auto_installed() {
        let host = FakeAndroidHost::new();
        let runtime = runtime(&host);
        match launch_error(runtime.launch(&descriptor(), &config(), &context())) {
            AppError::LaunchFailed { reason, .. } => assert_eq!(reason, "not-installed"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(host.launches().is_empty());
        assert!(host.installs().is_empty());
        assert!(matches!(host.status(), RuntimeStatus::Running { .. }));
    }

    #[test]
    fn happy_path_launches_and_reports_diagnostics() {
        let host = FakeAndroidHost::new();
        host.install_package(PACKAGE);
        let runtime = runtime(&host);
        let mut running = runtime
            .launch(&descriptor(), &config(), &context())
            .unwrap();
        assert_eq!(host.launches(), vec![(PACKAGE.to_owned(), None)]);
        assert_eq!(host.session_starts(), 1);
        // The instance pid is the Android process; windows are matched by
        // the container's Wayland client pid plus the task's app_id.
        assert_eq!(running.pid(), Some(5000));
        assert_eq!(
            running.surface_match(),
            SurfaceMatch::SharedHost {
                client_pids: [4242].into_iter().collect(),
                app_id: format!("waydroid.{PACKAGE}"),
            }
        );
        let diagnostics = running.diagnostics();
        assert_eq!(diagnostics["kind"], "android");
        assert_eq!(diagnostics["host"], "fake");
        assert_eq!(diagnostics["package"], PACKAGE);
        assert_eq!(diagnostics["pid"], 5000);
        assert_eq!(diagnostics["surface_client_pid"], 4242);
        assert_eq!(diagnostics["session"]["state"], "running");
        assert_eq!(diagnostics["session"]["session_pid"], 777);
        assert_eq!(running.try_wait().unwrap(), None);

        // A second launch reuses the running session.
        let mut second = runtime
            .launch(&descriptor(), &config(), &context())
            .unwrap();
        assert_eq!(host.session_starts(), 1);
        assert_eq!(second.try_wait().unwrap(), None);

        // Explicit activity goes through `am start -n`.
        let with_activity = json!({
            "package": PACKAGE,
            "activity": "com.google.android.apps.chrome.Main"
        });
        runtime
            .launch(&descriptor(), &with_activity, &context())
            .unwrap();
        assert_eq!(
            host.launches().last().unwrap(),
            &(
                PACKAGE.to_owned(),
                Some("com.google.android.apps.chrome.Main".to_owned())
            )
        );
    }

    #[test]
    fn process_disappearing_is_a_crash() {
        let host = FakeAndroidHost::new();
        host.install_package(PACKAGE);
        let runtime = runtime(&host);
        let mut running = runtime
            .launch(&descriptor(), &config(), &context())
            .unwrap();
        assert_eq!(running.try_wait().unwrap(), None);
        assert!(host.kill(PACKAGE));
        // Two confirmations are required before the exit is declared.
        assert_eq!(running.try_wait().unwrap(), None);
        let reason = running.try_wait().unwrap().expect("exit");
        assert!(reason.is_crash());
        assert_eq!(reason, ExitReason::Exited { code: 1 });
        assert_eq!(running.diagnostics()["exit_cause"], "process-gone");
        // Cached afterwards.
        assert_eq!(running.wait().unwrap(), reason);
        assert!(host.stops().is_empty());
    }

    #[test]
    fn terminate_force_stops_and_classifies_as_terminated() {
        let host = FakeAndroidHost::new();
        host.install_package(PACKAGE);
        let runtime = runtime(&host);
        let mut running = runtime
            .launch(&descriptor(), &config(), &context())
            .unwrap();
        let reason = running.terminate(Duration::from_secs(1)).unwrap();
        assert_eq!(reason, ExitReason::Signaled { signal: 15 });
        assert_eq!(host.stops(), vec![PACKAGE.to_owned()]);
        assert!(!host.is_running(PACKAGE));
        assert_eq!(running.diagnostics()["exit_cause"], "stopped");
        assert_eq!(running.try_wait().unwrap(), Some(reason));
        // Idempotent.
        assert_eq!(running.terminate(Duration::from_secs(1)).unwrap(), reason);
        assert_eq!(host.stops().len(), 1);
    }

    #[test]
    fn launch_command_failure_is_a_launch_failure() {
        let host = FakeAndroidHost::new();
        host.install_package(PACKAGE);
        host.fail_launch("am start: Error: Activity class does not exist");
        match launch_error(runtime(&host).launch(&descriptor(), &config(), &context())) {
            AppError::LaunchFailed { reason, .. } => {
                assert!(reason.contains("does not exist"), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn install_is_explicit_and_needs_a_path() {
        let host = FakeAndroidHost::new();
        let runtime = runtime(&host);
        assert!(matches!(
            runtime.install(&descriptor(), &config()),
            Err(AppError::LaunchFailed { .. })
        ));
        let missing = json!({
            "package": PACKAGE,
            "apk": { "pins": "tools/android/fulguris.pins", "path": "/nonexistent/fulguris.apk" }
        });
        assert!(matches!(
            runtime.install(&descriptor(), &missing),
            Err(AppError::LaunchFailed { .. })
        ));
        let dir = std::env::temp_dir().join(format!("ferrix-android-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let apk = dir.join("fake.apk");
        std::fs::write(&apk, b"PK").unwrap();
        let present = json!({
            "package": PACKAGE,
            "apk": { "pins": "tools/android/fulguris.pins", "path": apk.to_str().unwrap() }
        });
        runtime.install(&descriptor(), &present).unwrap();
        assert_eq!(host.installs(), vec![apk.clone()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn surface_client_pids_appear_after_launch_and_never_fall_back_to_the_app() {
        let host = FakeAndroidHost::new();
        host.install_package(PACKAGE);
        host.set_surface_client_pid(None);
        let mut running = runtime(&host)
            .launch(&descriptor(), &config(), &context())
            .unwrap();
        assert_eq!(running.pid(), Some(5000));
        // No composer pid yet: nothing can claim the task window.
        let SurfaceMatch::SharedHost { client_pids, .. } = running.surface_match() else {
            panic!("android windows are shared-host windows");
        };
        assert!(client_pids.is_empty());
        assert!(!running.surface_match().claims(5000, Some("waydroid.x.y")));
        // Multi-window mode: several composer connections, read while polling.
        host.set_surface_client_pids(vec![4300, 4242]);
        assert_eq!(running.try_wait().unwrap(), None);
        let matcher = running.surface_match();
        assert!(matcher.claims(4242, Some(&format!("waydroid.{PACKAGE}"))));
        assert!(matcher.claims(4300, Some(&format!("waydroid.{PACKAGE}"))));
        assert!(!matcher.claims(4242, Some("waydroid.org.lineageos.setupwizard")));
        assert!(!matcher.claims(9999, Some(&format!("waydroid.{PACKAGE}"))));
        assert!(!matcher.claims(4242, None));
    }

    #[test]
    fn reopen_relaunches_the_package_until_the_instance_finished() {
        let host = FakeAndroidHost::new();
        host.install_package(PACKAGE);
        let mut running = runtime(&host)
            .launch(&descriptor(), &config(), &context())
            .unwrap();
        assert_eq!(running.reopen(), Ok(true));
        assert_eq!(
            host.launches(),
            vec![(PACKAGE.to_owned(), None), (PACKAGE.to_owned(), None)]
        );
        running.terminate(Duration::from_secs(1)).unwrap();
        assert_eq!(running.reopen(), Ok(false));
        assert_eq!(host.launches().len(), 2);
    }
}
