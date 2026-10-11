//! Linux-only end-to-end tests: the built `ferrix-sandbox` through
//! `NativeRuntime`. Run from the repository root with
//! `FERRIX_SANDBOX_TESTS=required just linux-check test -p ferrix-runtime-native`
//! (privileged container; CI's `sandbox-check` job). When the policy cannot be
//! met (EPERM, or a degraded probe on a hosted runner that denies `uid_map` or
//! mounts) each test prints a `sandbox-test-skipped` record and passes
//! vacuously, unless `FERRIX_SANDBOX_TESTS=required`, which turns every skip
//! into a failure so the gate cannot pass without exercising the sandbox.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ferrix_app_api::{
    AppCapability, AppDescriptor, AppId, AppRole, ExitReason, IconRef, RuntimeKind, SCHEMA_VERSION,
    TrustLevel,
};
use ferrix_app_runtime_api::{LaunchContext, RunningApp, Runtime};
use ferrix_runtime_native::{
    APPD_SOCKET_FILE, ISOLATION_UNAVAILABLE_STATUS, IsolationMode, IsolationPolicy, NativeLaunch,
    NativeRuntime, NativeRuntimeOptions, RUN_DIR_ENV, RunDirMode, RunDirPolicy, SandboxReady,
    TELEPHONYD_SOCKET_FILE, UserNamespace, emit, find_record, probe_sandbox_with, sandbox_plan,
    skipped_record,
};

const SANDBOX: &str = env!("CARGO_BIN_EXE_ferrix-sandbox");
const EVIDENCE: &str = "mock";

fn app_id() -> AppId {
    AppId::new("org.ferrix.test").unwrap()
}

fn descriptor() -> AppDescriptor {
    AppDescriptor {
        schema_version: SCHEMA_VERSION,
        id: app_id(),
        runtime: RuntimeKind::Native,
        name: "Test".to_owned(),
        icon: IconRef::default(),
        launchable: true,
        autostart: false,
        role: AppRole::Application,
        trust: TrustLevel::Untrusted,
        capabilities: Default::default(),
        intents: Default::default(),
    }
}

fn context() -> LaunchContext {
    LaunchContext {
        instance: 1,
        env: vec![("FERRIX_APP_ID".to_owned(), "org.ferrix.test".to_owned())],
        evidence: EVIDENCE.to_owned(),
        granted: Default::default(),
    }
}

fn runtime() -> NativeRuntime {
    NativeRuntime::new(NativeRuntimeOptions {
        sandbox_bin: Some(PathBuf::from(SANDBOX)),
        evidence: EVIDENCE.to_owned(),
    })
}

/// Set to `required` by the privileged gate: an environment that cannot
/// meet a policy is then a test failure instead of a skip.
const REQUIRED_ENV: &str = "FERRIX_SANDBOX_TESTS";

fn sandbox_required() -> bool {
    match std::env::var(REQUIRED_ENV) {
        Ok(value) if value == "required" => true,
        Ok(value) if value.is_empty() => false,
        // A typo must not quietly turn the gate back into skips.
        Ok(value) => panic!("{REQUIRED_ENV}={value:?}: expected \"required\" or unset"),
        Err(_) => false,
    }
}

/// Prints the skip record, or fails the test under `FERRIX_SANDBOX_TESTS=required`.
fn skip(reason: &str) {
    assert!(
        !sandbox_required(),
        "{REQUIRED_ENV}=required but the sandbox test cannot run here: {reason}"
    );
    emit(&skipped_record(EVIDENCE, reason));
}

/// `false` (after printing the skip record) when `policy` cannot be met here.
fn available(policy: &IsolationPolicy) -> bool {
    match probe_sandbox_with(Some(Path::new(SANDBOX)), policy) {
        Ok(_) => true,
        Err(reason) => {
            skip(&reason);
            false
        }
    }
}

fn launch(exec: &[&str], policy: IsolationPolicy) -> Box<dyn RunningApp> {
    launch_with(exec, policy, &context())
}

fn launch_with(
    exec: &[&str],
    policy: IsolationPolicy,
    context: &LaunchContext,
) -> Box<dyn RunningApp> {
    let config = serde_json::to_value(NativeLaunch {
        exec: exec.iter().map(|s| (*s).to_owned()).collect(),
        isolation: policy,
    })
    .unwrap();
    runtime()
        .launch(&descriptor(), &config, context)
        .expect("launch")
}

fn run(exec: &[&str]) -> ExitReason {
    launch(exec, IsolationPolicy::default())
        .wait()
        .expect("wait")
}

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{}-{nanos}", std::process::id())
}

#[test]
fn required_init_refuses_to_exec_when_a_requested_namespace_is_missing() {
    let marker = scratch("ferrix-required-fail-closed");
    let policy = IsolationPolicy {
        pid_namespace: true,
        mode: IsolationMode::Required,
        ..IsolationPolicy::none()
    };
    let output = Command::new(SANDBOX)
        .arg("--init")
        .arg("--policy")
        .arg(serde_json::to_string(&policy).unwrap())
        .arg("--app-id")
        .arg("org.ferrix.test")
        .arg("--evidence")
        .arg(EVIDENCE)
        .arg("--applied")
        .arg("{}")
        .arg("--")
        .args(["sh", "-c", "touch \"$1\"", "sh"])
        .arg(&marker)
        .stdout(Stdio::piped())
        .output()
        .expect("run ferrix-sandbox init stage");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(ISOLATION_UNAVAILABLE_STATUS),
        "{stdout}"
    );
    assert!(find_record(&stdout, "sandbox-failed").is_some(), "{stdout}");
    assert!(!marker.exists(), "required policy executed the application");
}

#[test]
fn exit_code_is_forwarded_exactly() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    assert_eq!(run(&["sh", "-c", "exit 3"]), ExitReason::Exited { code: 3 });
    assert_eq!(run(&["sh", "-c", "exit 0"]), ExitReason::Exited { code: 0 });
}

#[test]
fn signal_death_is_forwarded_as_a_signal() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    assert_eq!(
        run(&["sh", "-c", "kill -SEGV $$"]),
        ExitReason::Signaled { signal: 11 }
    );
}

#[test]
fn application_sees_a_private_pid_namespace_and_proc() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    // PID 1 is the sandbox's init stage and pids restart from 1: only a
    // fresh /proc inside a new PID namespace shows that.
    let reason = run(&[
        "sh",
        "-c",
        r#"test "$(cat /proc/1/comm)" = ferrix-sandbox && [ "$$" -lt 10 ] && [ "$(ls /proc | grep -c '^[0-9]')" -lt 8 ]"#,
    ]);
    assert_eq!(reason, ExitReason::Exited { code: 0 });
}

#[test]
fn tmp_is_private_to_the_application() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let marker = format!("/tmp/{}", unique("ferrix-private-tmp"));
    let reason = run(&["sh", "-c", &format!("touch {marker} && [ -e {marker} ]")]);
    assert_eq!(reason, ExitReason::Exited { code: 0 });
    assert!(
        !Path::new(&marker).exists(),
        "{marker} leaked into the host /tmp"
    );
}

#[test]
fn hostname_and_environment_reach_the_application() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let reason = run(&[
        "sh",
        "-c",
        r#"test "$(cat /proc/sys/kernel/hostname)" = org.ferrix.test && test "$FERRIX_APP_ID" = org.ferrix.test"#,
    ]);
    assert_eq!(reason, ExitReason::Exited { code: 0 });
}

#[test]
fn readiness_report_confirms_the_default_policy() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let mut running = launch(&["sh", "-c", "exit 0"], IsolationPolicy::default());
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(!report.degraded, "{report:?}");
    assert!(report.applied.pid_namespace && report.applied.mount_namespace);
    assert!(report.applied.ipc_namespace && report.applied.uts_namespace);
    assert!(report.applied.private_tmp && report.applied.no_new_privs);
    assert_eq!(report.requested, IsolationPolicy::default());
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

#[test]
fn sandbox_ready_record_proves_namespaces() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let exec: Vec<String> = ["sh", "-c", "exit 0"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let output = Command::new(SANDBOX)
        .args(sandbox_plan(
            &IsolationPolicy::default(),
            &app_id(),
            EVIDENCE,
            &exec,
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .output()
        .expect("run ferrix-sandbox");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let ready = find_record(&stdout, "sandbox-ready").unwrap_or_else(|| panic!("{stdout}"));
    assert_eq!(ready["evidence"], EVIDENCE);
    let ready: SandboxReady = serde_json::from_value(ready).unwrap();
    assert_eq!(ready.app, "org.ferrix.test");
    assert!(ready.init_pid > 0);
    assert!(ready.namespaces.pid && ready.namespaces.mount, "{ready:?}");
    assert!(ready.namespaces.ipc && ready.namespaces.uts, "{ready:?}");
    assert!(
        ready.private_tmp && ready.no_new_privs && !ready.degraded,
        "{ready:?}"
    );
    assert!(
        find_record(&stdout, "sandbox-app-exited").is_some(),
        "{stdout}"
    );
}

#[test]
fn user_namespace_when_always() {
    let policy = IsolationPolicy {
        user_namespace: UserNamespace::Always,
        ..IsolationPolicy::default()
    };
    if !available(&policy) {
        return;
    }
    // A single-entry map (count 1) only exists inside a child user namespace.
    let mut running = launch(
        &[
            "sh",
            "-c",
            r#"read a b c < /proc/self/uid_map && [ "$c" = 1 ]"#,
        ],
        policy,
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(report.applied.user_namespace);
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

#[test]
fn terminate_forwards_term_through_the_stages() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let mut running = launch(&["sh", "-c", "sleep 30"], IsolationPolicy::default());
    let started = Instant::now();
    let reason = running.terminate(Duration::from_secs(5)).unwrap();
    assert_eq!(reason, ExitReason::Signaled { signal: 15 });
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn missing_program_exits_127() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    assert_eq!(
        run(&["/nonexistent/ferrix-program"]),
        ExitReason::Exited { code: 127 }
    );
}

// ---------------------------------------------------------------------------
// Socket-directory visibility (ADR-004 §4)
// ---------------------------------------------------------------------------

/// A scratch directory outside `/tmp`: the sandbox mounts a private tmpfs
/// over `/tmp`, so a socket directory placed there would not exist inside
/// the application's mount namespace.
fn scratch(prefix: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(unique(prefix))
}

/// Longest usable socket path: `sun_path` is 108 bytes including the NUL.
const SUN_PATH_MAX: usize = 107;
/// Room reserved for a socket file name inside a socket directory.
const SOCKET_NAME_ROOM: usize = 32;

/// Like [`scratch`], but short enough to bind sockets in. The target
/// directory sits under the checkout, which on a hosted runner
/// (`/home/runner/work/<repo>/<repo>/target/tmp`) leaves too little of
/// `sun_path` for a unique name plus a socket file. The first root outside
/// `/tmp` that leaves room is used; the name is unique per process.
fn socket_scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!(
        "fxs-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let needed = 1 + name.len() + 1 + SOCKET_NAME_ROOM;
    let roots = [
        Path::new(env!("CARGO_TARGET_TMPDIR")),
        Path::new("/dev/shm"),
        Path::new("/var/tmp"),
    ];
    let root = roots
        .iter()
        .find(|root| root.is_dir() && root.as_os_str().len() + needed <= SUN_PATH_MAX)
        .unwrap_or_else(|| {
            panic!("no scratch root outside /tmp short enough for sockets: {roots:?}")
        });
    root.join(name)
}

/// A throwaway socket directory (stands in for `/run/ferrix` through
/// `FERRIX_RUN_DIR`) holding listening sockets with the given names.
struct SocketDir {
    dir: PathBuf,
    _listeners: Vec<std::os::unix::net::UnixListener>,
}

impl SocketDir {
    fn new(names: &[&str]) -> Self {
        let dir = socket_scratch();
        // A crashed earlier run with the same pid may have left it behind.
        let _ = std::fs::remove_dir_all(&dir);
        Self::at(dir, names)
    }

    fn at(dir: PathBuf, names: &[&str]) -> Self {
        std::fs::create_dir_all(&dir).unwrap();
        // Owned before binding so a failed bind still removes the directory.
        let mut sockets = Self {
            dir,
            _listeners: Vec::new(),
        };
        for name in names {
            let path = sockets.dir.join(name);
            assert!(
                path.as_os_str().len() <= SUN_PATH_MAX,
                "socket path too long for sun_path: {}",
                path.display()
            );
            let listener = std::os::unix::net::UnixListener::bind(&path)
                .unwrap_or_else(|e| panic!("bind {}: {e}", path.display()));
            sockets._listeners.push(listener);
        }
        sockets
    }

    fn path(&self) -> String {
        self.dir.display().to_string()
    }

    fn context(&self, granted: &[AppCapability]) -> LaunchContext {
        let mut context = context();
        context.env.push((RUN_DIR_ENV.to_owned(), self.path()));
        context.granted = granted.iter().copied().collect();
        context
    }
}

impl Drop for SocketDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn private_run_dir(mode: RunDirMode, sockets: &[&str]) -> IsolationPolicy {
    IsolationPolicy {
        run_dir: RunDirPolicy {
            mode,
            sockets: sockets.iter().map(|s| (*s).to_owned()).collect(),
        },
        ..IsolationPolicy::default()
    }
}

/// The sandbox binary directly: the policy lists `a.sock` only, so the
/// application sees `a.sock` and not `b.sock`, and the `sandbox-ready`
/// record says exactly that.
#[test]
fn private_run_dir_shows_only_the_listed_sockets() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let sockets = SocketDir::new(&["a.sock", "b.sock"]);
    let dir = sockets.path();
    let probe = format!("test -S {dir}/a.sock && ! test -e {dir}/b.sock");
    let exec: Vec<String> = ["sh", "-c", probe.as_str()]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let policy = private_run_dir(RunDirMode::Private, &["a.sock", "missing.sock"]);
    let output = Command::new(SANDBOX)
        .args(sandbox_plan(&policy, &app_id(), EVIDENCE, &exec))
        .env(RUN_DIR_ENV, &dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .output()
        .expect("run ferrix-sandbox");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    let ready = find_record(&stdout, "sandbox-ready").unwrap_or_else(|| panic!("{stdout}"));
    assert_eq!(
        ready["run_dir"],
        serde_json::json!({"mode": "private", "sockets": ["a.sock"], "missing": ["missing.sock"]}),
        "{stdout}"
    );
    let ready: SandboxReady = serde_json::from_value(ready).unwrap();
    assert!(!ready.degraded, "{ready:?}");
    // The host's view is untouched: both sockets still exist here.
    assert!(sockets.dir.join("a.sock").exists() && sockets.dir.join("b.sock").exists());
}

/// Through `NativeRuntime`: grants decide the list. Without a telephony
/// grant the telephony socket cannot even be seen (so `connect()` fails
/// with ENOENT); with one it can.
#[test]
fn grants_decide_which_daemon_sockets_an_application_can_reach() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let sockets = SocketDir::new(&[APPD_SOCKET_FILE, TELEPHONYD_SOCKET_FILE]);
    let dir = sockets.path();
    let policy = private_run_dir(RunDirMode::Private, &[]);

    let denied = format!(
        "test -S {dir}/{APPD_SOCKET_FILE} && ! test -e {dir}/{TELEPHONYD_SOCKET_FILE} && ! ls {dir} | grep -q telephonyd"
    );
    let mut running = launch_with(
        &["sh", "-c", &denied],
        policy.clone(),
        &sockets.context(&[AppCapability::Shell, AppCapability::Background]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(
        report.applied.run_dir.mode,
        RunDirMode::Private,
        "{report:?}"
    );
    assert_eq!(report.applied.run_dir.sockets, [APPD_SOCKET_FILE]);
    assert!(!report.degraded, "{report:?}");
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });

    let allowed =
        format!("test -S {dir}/{APPD_SOCKET_FILE} && test -S {dir}/{TELEPHONYD_SOCKET_FILE}");
    let mut running = launch_with(
        &["sh", "-c", &allowed],
        policy,
        &sockets.context(&[AppCapability::TelephonyState]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(
        report.applied.run_dir.sockets,
        [APPD_SOCKET_FILE, TELEPHONYD_SOCKET_FILE]
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

/// A Required policy whose socket directory does not exist refuses to start
/// the application (exit 126 -> `IsolationUnavailable`); BestEffort starts
/// it and reports the degraded view.
#[test]
fn missing_run_dir_is_fatal_under_required_and_degraded_under_best_effort() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let missing = scratch("ferrix-run-missing");
    let mut context = context();
    context
        .env
        .push((RUN_DIR_ENV.to_owned(), missing.display().to_string()));
    let config = serde_json::to_value(NativeLaunch {
        exec: vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()],
        isolation: private_run_dir(RunDirMode::Private, &[]),
    })
    .unwrap();
    assert!(matches!(
        runtime().launch(&descriptor(), &config, &context),
        Err(ferrix_app_api::AppError::IsolationUnavailable { .. })
    ));

    let best_effort = IsolationPolicy {
        mode: IsolationMode::BestEffort,
        ..private_run_dir(RunDirMode::Private, &[])
    };
    let mut running = launch_with(&["sh", "-c", "exit 0"], best_effort, &context);
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(report.degraded, "{report:?}");
    assert_eq!(report.applied.run_dir.mode, RunDirMode::Inherit);
    assert!(
        report
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("run_dir")),
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

/// `private-run`: all of `/run` is replaced; the socket directory (filtered)
/// and `/run/user` come back, nothing else does. Needs a socket directory
/// under `/run`, so it creates one (root in the privileged container) and
/// skips when it cannot (fails under `FERRIX_SANDBOX_TESTS=required`).
#[test]
fn private_run_hides_everything_else_under_run() {
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let dir = Path::new("/run").join(unique("ferrix-test"));
    if let Err(error) = std::fs::create_dir_all(&dir) {
        skip(&format!(
            "cannot create a socket directory under /run: {error}"
        ));
        return;
    }
    let marker = Path::new("/run").join(unique("ferrix-marker"));
    // Constructed first so the directory is removed even if the marker fails.
    let sockets = SocketDir::at(dir, &[APPD_SOCKET_FILE, TELEPHONYD_SOCKET_FILE]);
    std::fs::write(&marker, "x").unwrap();
    let _marker = RemoveOnDrop(marker.clone());
    let dir = sockets.path();
    let probe = format!(
        "test -S {dir}/{APPD_SOCKET_FILE} && ! test -e {dir}/{TELEPHONYD_SOCKET_FILE} && ! test -e {} && ! test -e /run/dbus/system_bus_socket && [ \"$(ls /run | wc -l)\" -le 2 ]",
        marker.display()
    );
    let mut running = launch_with(
        &["sh", "-c", &probe],
        private_run_dir(RunDirMode::PrivateRun, &[]),
        &sockets.context(&[]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(
        report.applied.run_dir.mode,
        RunDirMode::PrivateRun,
        "{report:?}"
    );
    assert_eq!(report.applied.run_dir.sockets, [APPD_SOCKET_FILE]);
    assert!(!report.degraded, "{report:?}");
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
    assert!(marker.exists(), "the host's /run is untouched");
}

/// Removes a file when dropped, pass or fail.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
