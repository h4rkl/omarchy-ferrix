//! [`NativeRuntime`]: the [`ferrix_app_runtime_api::Runtime`] for native applications.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use ferrix_app_api::{AppDescriptor, AppError, AppId, ExitReason, RuntimeKind, TELEPHONY_APN_ENV};
use ferrix_app_runtime_api::{LaunchContext, RunningApp, Runtime};

#[cfg(not(target_os = "linux"))]
use crate::IsolationMode;
use crate::{
    AppliedIsolation, ISOLATION_UNAVAILABLE_STATUS, IsolationPolicy, IsolationReport, NativeLaunch,
    SANDBOX_BIN, SANDBOX_BIN_ENV, exit_reason_from_status, needs_sandbox, parse_probe_output,
    policy_json, sockets_for_grants,
};

/// How long a launch waits for the sandbox's readiness report before giving
/// up on confirmation (the application keeps running either way). Generous
/// because the guest runs under TCG.
pub const READY_TIMEOUT: Duration = Duration::from_secs(12);
/// Polling interval for `terminate` and readiness waits.
const POLL: Duration = Duration::from_millis(50);

/// Options for [`NativeRuntime`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeRuntimeOptions {
    /// Explicit sandbox binary. `None` uses `FERRIX_SANDBOX_BIN`, then
    /// `ferrix-sandbox` on `PATH`.
    pub sandbox_bin: Option<PathBuf>,
    /// Evidence label passed to the sandbox for its records (`QEMU`/`mock`).
    pub evidence: String,
}

/// Resolves the sandbox binary: explicit path, then `FERRIX_SANDBOX_BIN`,
/// then `PATH` lookup.
pub fn resolve_sandbox_bin(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    match std::env::var_os(SANDBOX_BIN_ENV) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from(SANDBOX_BIN),
    }
}

/// Whether the running launcher is root (decides `UserNamespace::Auto`).
pub fn launcher_is_root() -> bool {
    #[cfg(unix)]
    {
        rustix::process::geteuid().is_root()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// The isolation the sandbox is expected to apply for `policy` on this
/// platform. The authoritative outcome is the `sandbox-ready` record (and the
/// readiness report the runtime receives from it); this is the fallback when
/// no report arrived.
pub fn expected_isolation(policy: &IsolationPolicy) -> IsolationReport {
    #[cfg(target_os = "linux")]
    {
        let namespaces = crate::select_namespaces(policy, launcher_is_root());
        let run_dir = if namespaces.mount {
            crate::RunDirOutcome {
                mode: policy.run_dir.mode,
                sockets: policy.run_dir.sockets.clone(),
                missing: Vec::new(),
            }
        } else {
            crate::RunDirOutcome::default()
        };
        let applied = namespaces.applied(
            policy.private_tmp && namespaces.mount,
            policy.no_new_privs,
            run_dir,
        );
        IsolationReport {
            requested: policy.clone(),
            applied,
            degraded: false,
            reason: None,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        IsolationReport::unisolated(policy.clone(), "not linux")
    }
}

/// Runs `ferrix-sandbox --probe` with the default policy: `Ok` with what it
/// established, `Err` with the reason the default policy cannot be met here.
pub fn probe_sandbox(sandbox_bin: Option<&Path>) -> Result<AppliedIsolation, String> {
    probe_sandbox_with(sandbox_bin, &IsolationPolicy::default())
}

/// [`probe_sandbox`] for an arbitrary policy.
pub fn probe_sandbox_with(
    sandbox_bin: Option<&Path>,
    policy: &IsolationPolicy,
) -> Result<AppliedIsolation, String> {
    let bin = resolve_sandbox_bin(sandbox_bin);
    let output = Command::new(&bin)
        .arg("--probe")
        .arg("--policy")
        .arg(policy_json(policy))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run {}: {e}", bin.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_probe_output(&stdout, output.status.code())
}

/// Native runtime: spawns `ferrix-sandbox` (or the program directly when no
/// isolation is requested) and supervises the process.
pub struct NativeRuntime {
    options: NativeRuntimeOptions,
}

impl NativeRuntime {
    pub fn new(options: NativeRuntimeOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &NativeRuntimeOptions {
        &self.options
    }

    /// The sandbox binary this runtime will run.
    pub fn sandbox_bin(&self) -> PathBuf {
        resolve_sandbox_bin(self.options.sandbox_bin.as_deref())
    }

    #[cfg(target_os = "linux")]
    fn evidence(&self, context: &LaunchContext) -> String {
        if context.evidence.is_empty() {
            self.options.evidence.clone()
        } else {
            context.evidence.clone()
        }
    }

    fn spawn_direct(
        &self,
        app: &AppDescriptor,
        launch: &NativeLaunch,
        context: &LaunchContext,
        reason: &str,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        let mut command = Command::new(&launch.exec[0]);
        command.args(&launch.exec[1..]);
        let child = spawn(command, context).map_err(|e| AppError::LaunchFailed {
            id: app.id.clone(),
            reason: format!("cannot start {}: {e}", launch.exec[0]),
        })?;
        Ok(Box::new(NativeApp {
            id: app.id.clone(),
            child,
            isolation: IsolationReport::unisolated(launch.isolation.clone(), reason),
            sandboxed: false,
            confirmed: false,
            finished: None,
        }))
    }

    #[cfg(target_os = "linux")]
    fn spawn_sandboxed(
        &self,
        app: &AppDescriptor,
        launch: &NativeLaunch,
        context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        use crate::channel::{ReportListener, WaitOutcome};
        use crate::{SandboxReady, sandbox_args};

        let evidence = self.evidence(context);
        let listener = ReportListener::bind("ferrix-launch").ok();
        let args = sandbox_args(
            &launch.isolation,
            &app.id,
            &evidence,
            listener.as_ref().map(ReportListener::name),
            &launch.exec,
        );
        let bin = self.sandbox_bin();
        let mut command = Command::new(&bin);
        command.args(&args);
        let mut child = spawn(command, context).map_err(|e| AppError::LaunchFailed {
            id: app.id.clone(),
            reason: format!("cannot start sandbox {}: {e}", bin.display()),
        })?;

        let mut isolation = expected_isolation(&launch.isolation);
        let mut confirmed = false;
        let mut finished = None;
        match listener.map(|l| l.wait_for_line(&mut child, READY_TIMEOUT, &mut || {})) {
            Some(Ok(line)) => match serde_json::from_str::<SandboxReady>(&line) {
                Ok(ready) => {
                    isolation = ready.isolation_report(launch.isolation.clone());
                    confirmed = true;
                }
                Err(e) => {
                    isolation.degraded = true;
                    isolation.reason = Some(format!("unreadable readiness report: {e}"));
                }
            },
            Some(Err(WaitOutcome::ChildExited(status))) => {
                if status.code() == Some(ISOLATION_UNAVAILABLE_STATUS) {
                    return Err(AppError::IsolationUnavailable {
                        id: app.id.clone(),
                        reason: format!(
                            "sandbox exited with status {ISOLATION_UNAVAILABLE_STATUS} before readiness (see the sandbox-failed record)"
                        ),
                    });
                }
                isolation.degraded = true;
                isolation.reason = Some("sandbox exited before reporting readiness".to_owned());
                finished = Some(Ok(exit_reason_from_status(status)));
            }
            Some(Err(WaitOutcome::TimedOut)) => {
                isolation.degraded = true;
                isolation.reason = Some(format!(
                    "unverified: no readiness report within {}s",
                    READY_TIMEOUT.as_secs()
                ));
            }
            Some(Err(WaitOutcome::Io(error))) => {
                isolation.degraded = true;
                isolation.reason = Some(format!("unverified: readiness channel failed: {error}"));
            }
            None => {
                isolation.degraded = true;
                isolation.reason =
                    Some("unverified: readiness channel could not be created".to_owned());
            }
        }
        Ok(Box::new(NativeApp {
            id: app.id.clone(),
            child,
            isolation,
            sandboxed: true,
            confirmed,
            finished,
        }))
    }
}

impl Runtime for NativeRuntime {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Native
    }

    fn launch(
        &self,
        app: &AppDescriptor,
        config: &serde_json::Value,
        context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        if app.runtime != RuntimeKind::Native {
            return Err(AppError::RuntimeUnsupported {
                runtime: app.runtime,
            });
        }
        let launch: NativeLaunch = serde_json::from_value(config.clone()).map_err(|error| {
            AppError::InvalidDescriptor {
                id: Some(app.id.clone()),
                reason: format!("invalid native launch configuration: {error}"),
            }
        })?;
        if launch.exec.is_empty() || launch.exec[0].is_empty() {
            return Err(AppError::NotLaunchable { id: app.id.clone() });
        }
        if launch.isolation.private_tmp && !launch.isolation.mount_namespace {
            return Err(AppError::InvalidDescriptor {
                id: Some(app.id.clone()),
                reason: "private_tmp requires mount_namespace".to_owned(),
            });
        }
        if launch.isolation.run_dir.is_private() && !launch.isolation.mount_namespace {
            return Err(AppError::InvalidDescriptor {
                id: Some(app.id.clone()),
                reason: "run_dir.mode private requires mount_namespace".to_owned(),
            });
        }
        // Socket visibility follows the effective grants `ferrix_appd`
        // computed, never a list written into the manifest.
        let launch = NativeLaunch {
            exec: launch.exec,
            isolation: with_granted_sockets(launch.isolation, context),
        };
        if !needs_sandbox(&launch.isolation) {
            return self.spawn_direct(app, &launch, context, "not requested");
        }
        #[cfg(target_os = "linux")]
        {
            self.spawn_sandboxed(app, &launch, context)
        }
        #[cfg(not(target_os = "linux"))]
        {
            match launch.isolation.mode {
                IsolationMode::Required => Err(AppError::IsolationUnavailable {
                    id: app.id.clone(),
                    reason: "not linux".to_owned(),
                }),
                IsolationMode::BestEffort => self.spawn_direct(app, &launch, context, "not linux"),
            }
        }
    }
}

/// The policy the sandbox actually receives: with a private socket
/// directory, the visible sockets are exactly those the effective grants
/// imply ([`sockets_for_grants`]); under `inherit` the list is irrelevant and
/// cleared so the record never suggests a filter that was not applied.
pub fn with_granted_sockets(
    mut policy: IsolationPolicy,
    context: &LaunchContext,
) -> IsolationPolicy {
    policy.run_dir.sockets = if policy.run_dir.is_private() {
        sockets_for_grants(&context.granted)
    } else {
        Vec::new()
    };
    policy
}

/// Spawns with the launch environment, its own process group, and inherited
/// stdio (application output flows to the launcher's log).
fn spawn(mut command: Command, context: &LaunchContext) -> std::io::Result<Child> {
    // `ferrix-appd` captures the target APN before it starts workers, but keep
    // this boundary explicit as well: a future caller or test may run a
    // runtime with the value in its own environment. The service re-adds it
    // through `context.env` only for an authorized telephony-data app.
    command.env_remove(TELEPHONY_APN_ENV);
    command.envs(context.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    spawn_on_spawner_thread(command)
}

type SpawnRequest = (Command, mpsc::Sender<std::io::Result<Child>>);

/// Spawns `command` from one thread that lives as long as the process.
/// `ferrix-sandbox` sets a parent-death signal, and Linux delivers it when
/// the spawning *thread* exits, not the process: a launch from a short-lived
/// worker (a home restart, a request handler) would otherwise be SIGKILLed
/// as soon as that worker returned.
fn spawn_on_spawner_thread(command: Command) -> std::io::Result<Child> {
    static SPAWNER: OnceLock<Mutex<mpsc::Sender<SpawnRequest>>> = OnceLock::new();
    let spawner = SPAWNER.get_or_init(|| {
        let (requests, incoming) = mpsc::channel::<SpawnRequest>();
        let _ = std::thread::Builder::new()
            .name("ferrix-runtime-spawner".to_owned())
            .spawn(move || {
                for (mut command, reply) in incoming {
                    let _ = reply.send(command.spawn());
                }
            });
        Mutex::new(requests)
    });
    let (reply, result) = mpsc::channel();
    let unavailable = || std::io::Error::other("the runtime spawner thread is unavailable");
    spawner
        .lock()
        .map_err(|_| unavailable())?
        .send((command, reply))
        .map_err(|_| unavailable())?;
    result.recv().map_err(|_| unavailable())?
}

/// One supervised native process (the sandbox stage 1, or the program itself).
struct NativeApp {
    id: AppId,
    child: Child,
    isolation: IsolationReport,
    sandboxed: bool,
    /// Whether the sandbox confirmed readiness; after that an exit status of
    /// 126 belongs to the application, not to the sandbox.
    confirmed: bool,
    finished: Option<Result<ExitReason, AppError>>,
}

impl NativeApp {
    fn classify(&self, status: ExitStatus) -> Result<ExitReason, AppError> {
        if self.sandboxed && !self.confirmed && status.code() == Some(ISOLATION_UNAVAILABLE_STATUS)
        {
            return Err(AppError::IsolationUnavailable {
                id: self.id.clone(),
                reason: format!(
                    "sandbox exited with status {ISOLATION_UNAVAILABLE_STATUS} (see the sandbox-failed record)"
                ),
            });
        }
        Ok(exit_reason_from_status(status))
    }

    fn record(&mut self, status: ExitStatus) -> Result<ExitReason, AppError> {
        let result = self.classify(status);
        self.finished = Some(result.clone());
        result
    }

    fn signal(&self, signal: i32) {
        #[cfg(unix)]
        {
            use rustix::process::{Pid, Signal, kill_process};
            if let (Some(pid), Some(signal)) = (
                Pid::from_raw(self.child.id() as i32),
                Signal::from_named_raw(signal),
            ) {
                // ESRCH means it is already gone; the wait that follows reports it.
                let _ = kill_process(pid, signal);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = signal;
        }
    }
}

impl RunningApp for NativeApp {
    fn pid(&self) -> Option<u32> {
        Some(self.child.id())
    }

    fn diagnostics(&self) -> serde_json::Value {
        serde_json::to_value(&self.isolation).unwrap_or(serde_json::Value::Null)
    }

    fn try_wait(&mut self) -> Result<Option<ExitReason>, AppError> {
        if let Some(finished) = &self.finished {
            return finished.clone().map(Some);
        }
        match self.child.try_wait()? {
            Some(status) => self.record(status).map(Some),
            None => Ok(None),
        }
    }

    fn wait(&mut self) -> Result<ExitReason, AppError> {
        if let Some(finished) = &self.finished {
            return finished.clone();
        }
        let status = self.child.wait()?;
        self.record(status)
    }

    fn terminate(&mut self, grace: Duration) -> Result<ExitReason, AppError> {
        if let Some(finished) = &self.finished {
            return finished.clone();
        }
        const SIGTERM: i32 = 15;
        self.signal(SIGTERM);
        let deadline = Instant::now() + grace;
        loop {
            if let Some(reason) = self.try_wait()? {
                return Ok(reason);
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(POLL.min(grace));
        }
        // Escalate: SIGKILL the supervisor; its init stage dies with it
        // (parent-death signal) and PID 1's death takes the namespace down.
        self.child.kill()?;
        self.wait()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_app_api::{AppRole, IconRef, SCHEMA_VERSION, TrustLevel};

    fn descriptor() -> AppDescriptor {
        AppDescriptor {
            schema_version: SCHEMA_VERSION,
            id: AppId::new("org.ferrix.test").unwrap(),
            runtime: RuntimeKind::Native,
            name: "Test".to_owned(),
            icon: IconRef::default(),
            launchable: true,
            autostart: false,
            role: AppRole::Application,
            trust: TrustLevel::Untrusted,
            capabilities: Default::default(),
            intents: Default::default(),
            origin: Default::default(),
        }
    }

    fn launch_value(exec: &[&str], isolation: IsolationPolicy) -> serde_json::Value {
        serde_json::to_value(NativeLaunch {
            exec: exec.iter().map(|s| (*s).to_owned()).collect(),
            isolation,
        })
        .unwrap()
    }

    fn context() -> LaunchContext {
        LaunchContext {
            instance: 1,
            env: vec![("FERRIX_APP_ID".to_owned(), "org.ferrix.test".to_owned())],
            evidence: "mock".to_owned(),
            granted: Default::default(),
        }
    }

    #[test]
    fn socket_list_follows_grants_and_a_manifest_list_is_replaced() {
        use crate::{RunDirMode, RunDirPolicy};
        use ferrix_app_api::AppCapability;
        let manifest = IsolationPolicy {
            run_dir: RunDirPolicy {
                mode: RunDirMode::Private,
                // A descriptor trying to see the telephony socket without a grant.
                sockets: vec!["telephonyd.sock".to_owned()],
            },
            ..IsolationPolicy::default()
        };
        let none = with_granted_sockets(manifest.clone(), &context());
        assert_eq!(none.run_dir.sockets, ["ferrix-appd.sock"]);

        let mut granted = context();
        granted.granted = [AppCapability::NetworkState, AppCapability::CoreState]
            .into_iter()
            .collect();
        let some = with_granted_sockets(manifest, &granted);
        assert_eq!(
            some.run_dir.sockets,
            ["ferrix-appd.sock", "core.sock", "networkd.sock"]
        );

        // Inherit: nothing is filtered, so nothing is listed.
        let inherit = with_granted_sockets(IsolationPolicy::default(), &granted);
        assert!(inherit.run_dir.sockets.is_empty());
        assert_eq!(inherit.run_dir.mode, RunDirMode::Inherit);
    }

    #[test]
    fn private_run_dir_requires_the_mount_namespace() {
        use crate::{RunDirMode, RunDirPolicy};
        let app = descriptor();
        let policy = IsolationPolicy {
            run_dir: RunDirPolicy {
                mode: RunDirMode::Private,
                sockets: Vec::new(),
            },
            ..IsolationPolicy::none()
        };
        let config = launch_value(&["sh", "-c", "exit 0"], policy);
        assert!(matches!(
            runtime().launch(&app, &config, &context()),
            Err(AppError::InvalidDescriptor { reason, .. }) if reason.contains("run_dir")
        ));
    }

    fn runtime() -> NativeRuntime {
        NativeRuntime::new(NativeRuntimeOptions {
            sandbox_bin: Some(PathBuf::from("/nonexistent/ferrix-sandbox")),
            evidence: "mock".to_owned(),
        })
    }

    #[test]
    fn sandbox_bin_resolution_prefers_explicit_then_env_then_path() {
        assert_eq!(
            resolve_sandbox_bin(Some(Path::new("/x/sb"))),
            PathBuf::from("/x/sb")
        );
        // With no explicit path the result is either the env override or the
        // bare name; both are valid, so only check the shape.
        let resolved = resolve_sandbox_bin(None);
        assert!(!resolved.as_os_str().is_empty());
        assert_eq!(runtime().kind(), RuntimeKind::Native);
    }

    #[cfg(unix)]
    #[test]
    fn unisolated_policy_spawns_directly_and_forwards_the_exit_code() {
        let app = descriptor();
        let config = launch_value(&["sh", "-c", "exit 3"], IsolationPolicy::none());
        let mut running = runtime().launch(&app, &config, &context()).unwrap();
        assert!(running.pid().is_some());
        let report: IsolationReport = serde_json::from_value(running.diagnostics()).unwrap();
        assert!(!report.degraded);
        assert_eq!(report.applied, AppliedIsolation::default());
        assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 3 });
        // Repeated waits return the cached result.
        assert_eq!(
            running.try_wait().unwrap(),
            Some(ExitReason::Exited { code: 3 })
        );
    }

    #[cfg(unix)]
    #[test]
    fn environment_reaches_the_direct_child() {
        let app = descriptor();
        let config = launch_value(
            &["sh", "-c", "test \"$FERRIX_APP_ID\" = org.ferrix.test"],
            IsolationPolicy::none(),
        );
        let mut running = runtime().launch(&app, &config, &context()).unwrap();
        assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
    }

    #[cfg(unix)]
    #[test]
    fn inherited_apn_is_removed_unless_the_launch_context_allows_it() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "test -z \"${FERRIX_TELEPHONY_APN+x}\""])
            .env(TELEPHONY_APN_ENV, "inherited-from-appd");
        let mut child = spawn(command, &context()).unwrap();
        assert!(child.wait().unwrap().success());

        let mut allowed = context();
        allowed
            .env
            .push((TELEPHONY_APN_ENV.to_owned(), "internet.example".to_owned()));
        let mut command = Command::new("sh");
        command
            .args(["-c", "test \"$FERRIX_TELEPHONY_APN\" = internet.example"])
            .env(TELEPHONY_APN_ENV, "inherited-from-appd");
        let mut child = spawn(command, &allowed).unwrap();
        assert!(child.wait().unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn terminate_sends_term_then_escalates() {
        let app = descriptor();
        let config = launch_value(&["sh", "-c", "sleep 30"], IsolationPolicy::none());
        let mut running = runtime().launch(&app, &config, &context()).unwrap();
        let started = Instant::now();
        let reason = running.terminate(Duration::from_secs(5)).unwrap();
        assert_eq!(reason, ExitReason::Signaled { signal: 15 });
        assert!(started.elapsed() < Duration::from_secs(5));

        let config = launch_value(
            &["sh", "-c", "trap '' TERM; sleep 30 & wait"],
            IsolationPolicy::none(),
        );
        let mut running = runtime().launch(&app, &config, &context()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let reason = running.terminate(Duration::from_millis(300)).unwrap();
        assert_eq!(reason, ExitReason::Signaled { signal: 9 });
    }

    #[test]
    fn missing_program_is_a_launch_failure() {
        let app = descriptor();
        let config = launch_value(&["/nonexistent/program"], IsolationPolicy::none());
        assert!(matches!(
            runtime().launch(&app, &config, &context()),
            Err(AppError::LaunchFailed { .. })
        ));
        let mut empty = descriptor();
        empty.launchable = false;
        let empty_config = launch_value(&[], IsolationPolicy::none());
        assert!(matches!(
            runtime().launch(&empty, &empty_config, &context()),
            Err(AppError::NotLaunchable { .. })
        ));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_reports_the_platform_honestly() {
        let report = expected_isolation(&IsolationPolicy::default());
        assert!(report.degraded);
        assert_eq!(report.reason.as_deref(), Some("not linux"));

        let app = descriptor();
        let config = launch_value(&["sh", "-c", "exit 0"], IsolationPolicy::default());
        assert!(matches!(
            runtime().launch(&app, &config, &context()),
            Err(AppError::IsolationUnavailable { .. })
        ));

        let best_effort = IsolationPolicy {
            mode: IsolationMode::BestEffort,
            ..IsolationPolicy::default()
        };
        let config = launch_value(&["sh", "-c", "exit 0"], best_effort);
        let mut running = runtime().launch(&app, &config, &context()).unwrap();
        let report: IsolationReport = serde_json::from_value(running.diagnostics()).unwrap();
        assert!(report.degraded);
        assert_eq!(report.reason.as_deref(), Some("not linux"));
        assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });

        assert!(probe_sandbox(Some(Path::new("/nonexistent/ferrix-sandbox"))).is_err());
    }
}
