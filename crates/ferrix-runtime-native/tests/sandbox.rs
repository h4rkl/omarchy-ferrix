//! Linux-only end-to-end tests: the built `ferrix-sandbox` through
//! `NativeRuntime`. Run from the repository root with
//! `FERRIX_SANDBOX_TESTS=required just linux-check test -p ferrix-runtime-native`
//! (privileged container; CI's `sandbox-check` job). When the policy cannot be
//! met (EPERM, or a degraded probe on a hosted runner that denies `uid_map` or
//! mounts) each test prints a `sandbox-test-skipped` record and passes
//! vacuously, unless `FERRIX_SANDBOX_TESTS=required`, which turns every skip
//! into a failure so the gate cannot pass without exercising the sandbox.
#![cfg(target_os = "linux")]

use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ferrix_app_api::{
    AppCapability, AppDescriptor, AppId, AppRole, ExitReason, IconRef, RuntimeKind, SCHEMA_VERSION,
    TrustLevel,
};
use ferrix_app_runtime_api::{LaunchContext, RunningApp, Runtime};
use ferrix_runtime_native::{
    APPD_SOCKET_FILE, FsViewMode, FsViewPolicy, ISOLATION_UNAVAILABLE_STATUS, IdentityOutcome,
    IsolationMode, IsolationPolicy, NativeLaunch, NativeRuntime, NativeRuntimeOptions, RUN_DIR_ENV,
    ResourcePolicy, RunDirMode, RunDirPolicy, SandboxReady, SeccompPolicy, SeccompProfile,
    TELEPHONYD_SOCKET_FILE, UidMode, UserNamespace, app_subuid, emit, find_record,
    probe_sandbox_with, sandbox_plan, skipped_record,
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
        origin: Default::default(),
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
    assert!(report.applied.cgroup.applied(), "{report:?}");
    assert_eq!(
        running.cgroup_path(),
        report.applied.cgroup.path.map(PathBuf::from)
    );
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
        ready.namespaces.net && ready.network.loopback_up,
        "{ready:?}"
    );
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

// ---------------------------------------------------------------------------
// cgroups v2 (issue #10, increment 1)
// ---------------------------------------------------------------------------

/// Waits until `path` exists (the application signals a phase with a file).
fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    path.exists()
}

fn cgroup_value(group: &Path, file: &str) -> String {
    std::fs::read_to_string(group.join(file))
        .unwrap_or_else(|e| panic!("read {}/{file}: {e}", group.display()))
        .trim()
        .to_owned()
}

/// The `key value` counter from a cgroup `*.events` file.
fn cgroup_event(group: &Path, file: &str, key: &str) -> u64 {
    cgroup_value(group, file)
        .lines()
        .find_map(|line| line.strip_prefix(key)?.trim().parse().ok())
        .unwrap_or(0)
}

/// These tests signal phases through files in the host scratch directory,
/// so the application keeps the launcher's filesystem.
fn limited(memory_max: Option<u64>, pids_max: Option<u64>) -> IsolationPolicy {
    IsolationPolicy {
        resources: ResourcePolicy {
            cgroup: true,
            memory_max,
            pids_max,
            cpu_weight: Some(50),
        },
        fs_view: FsViewPolicy::inherit(),
        ..IsolationPolicy::default()
    }
}

#[test]
fn cgroup_limits_are_written_and_the_group_is_removed_after_exit() {
    let policy = limited(Some(64 << 20), Some(64));
    if !available(&policy) {
        return;
    }
    let ready = scratch("ferrix-cgroup-ready");
    let mut running = launch(
        &[
            "sh",
            "-c",
            "touch \"$1\"; sleep 30",
            "sh",
            ready.to_str().unwrap(),
        ],
        policy,
    );
    let group = running
        .cgroup_path()
        .expect("the sandbox reports its cgroup");
    assert!(
        group.starts_with(ferrix_runtime_native::cgroup_slice()),
        "{}",
        group.display()
    );
    assert!(wait_for_file(&ready, Duration::from_secs(10)));
    assert_eq!(
        cgroup_value(&group, "memory.max"),
        (64u64 << 20).to_string()
    );
    assert_eq!(cgroup_value(&group, "pids.max"), "64");
    assert_eq!(cgroup_value(&group, "cpu.weight"), "50");
    // Stage 1, stage 2 and the application are all in the group.
    let procs = cgroup_value(&group, "cgroup.procs");
    assert!(procs.lines().count() >= 3, "{procs}");
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(
        report.applied.cgroup.path.as_deref(),
        group.to_str(),
        "{report:?}"
    );
    assert!(!report.degraded, "{report:?}");
    running.terminate(Duration::from_secs(5)).unwrap();
    assert!(!group.exists(), "{} survived the exit", group.display());
    assert!(running.cgroup_path().is_none());
    let _ = std::fs::remove_file(ready);
}

#[test]
fn pids_limit_stops_a_fork_bomb() {
    let policy = limited(None, Some(16));
    if !available(&policy) {
        return;
    }
    let ready = scratch("ferrix-pids-ready");
    // A child shell tries to start 64 background sleeps and dies at the
    // first refused fork (ash treats that as fatal); the parent then signals
    // and parks without forking, since no pid is left.
    let script = r#"sh -c 'i=0; while [ $i -lt 64 ]; do sleep 30 & i=$((i+1)); done' 2>/dev/null; : >"$1"; exec sleep 30"#;
    let mut running = launch(&["sh", "-c", script, "sh", ready.to_str().unwrap()], policy);
    let group = running.cgroup_path().expect("cgroup");
    assert!(wait_for_file(&ready, Duration::from_secs(20)));
    let current: u64 = cgroup_value(&group, "pids.current").parse().unwrap();
    assert!(current <= 16, "pids.current {current} above pids.max 16");
    assert!(
        cgroup_event(&group, "pids.events", "max") > 0,
        "no fork was refused: {}",
        cgroup_value(&group, "pids.events")
    );
    running.terminate(Duration::from_secs(5)).unwrap();
    let _ = std::fs::remove_file(ready);
}

#[test]
fn memory_limit_kills_the_whole_application_group() {
    let policy = limited(Some(24 << 20), None);
    if !available(&policy) {
        return;
    }
    let go = scratch("ferrix-memory-go");
    // tmpfs pages are charged to the writer's group and cannot be reclaimed
    // without swap; 48 MiB in a 64 MiB private /tmp passes the 24 MiB limit.
    let script = r#"while [ ! -e "$1" ]; do sleep 0.05; done; exec dd if=/dev/zero of=/tmp/fill bs=1M count=48"#;
    let mut running = launch(&["sh", "-c", script, "sh", go.to_str().unwrap()], policy);
    let group = running.cgroup_path().expect("cgroup");
    // A host with swap would page the tmpfs out instead of killing; the
    // test is about the limit, so take swap away from this group only.
    let swap = group.join("memory.swap.max");
    if swap.exists() {
        std::fs::write(&swap, "0").unwrap();
    }
    std::fs::write(&go, "").unwrap();
    let reason = running.wait().unwrap();
    assert_eq!(
        reason,
        ExitReason::Signaled { signal: 9 },
        "OOM kill expected"
    );
    assert!(!group.exists());
    let _ = std::fs::remove_file(go);
}

#[test]
fn cgroup_disabled_creates_no_group() {
    let policy = IsolationPolicy {
        resources: ResourcePolicy::none(),
        ..IsolationPolicy::default()
    };
    if !available(&policy) {
        return;
    }
    let mut running = launch(&["sh", "-c", "exit 0"], policy);
    assert!(running.cgroup_path().is_none());
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(
        !report.applied.cgroup.applied() && !report.degraded,
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

// ---------------------------------------------------------------------------
// Network namespace (issue #10, increment 2)
// ---------------------------------------------------------------------------

fn granted(capabilities: &[AppCapability]) -> LaunchContext {
    let mut context = context();
    context.granted = capabilities.iter().copied().collect();
    context
}

/// A TCP service on 127.0.0.1 in the test's own (launcher) network
/// namespace that counts the connections it accepts.
struct HostService {
    url: String,
    accepted: Arc<AtomicU32>,
}

fn host_service() -> HostService {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind host service");
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&accepted);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            counter.fetch_add(1, Ordering::SeqCst);
            let _ = stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok");
        }
    });
    HostService { url, accepted }
}

/// The app's own view: exactly `lo` in its network namespace, and that
/// loopback is up (ICMP to 127.0.0.1 answers).
const ONLY_LOOPBACK: &str = r#"[ "$(tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d ' ')" = lo ] || exit 10; ping -c 1 -W 2 127.0.0.1 >/dev/null 2>&1 || exit 11"#;

#[test]
fn without_the_network_grant_the_application_has_only_loopback() {
    let policy = IsolationPolicy::default();
    if !available(&policy) {
        return;
    }
    let service = host_service();
    // Reaching a service on the launcher's loopback must fail.
    let script = format!(r#"{ONLY_LOOPBACK}; wget -q -T 3 -O /dev/null "$1" && exit 12; exit 0"#);
    let mut running = launch_with(
        &["sh", "-c", &script, "sh", &service.url],
        policy,
        &granted(&[AppCapability::NetworkState]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(report.applied.net_namespace, "{report:?}");
    assert_eq!(report.applied.network.interfaces, ["lo"], "{report:?}");
    assert!(
        report.applied.network.loopback_up && !report.degraded,
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
    assert_eq!(service.accepted.load(Ordering::SeqCst), 0);
}

#[test]
fn the_network_grant_keeps_the_launcher_network() {
    let policy = IsolationPolicy::default();
    if !available(&policy) {
        return;
    }
    let service = host_service();
    let mut running = launch_with(
        &[
            "sh",
            "-c",
            r#"wget -q -T 5 -O /dev/null "$1""#,
            "sh",
            &service.url,
        ],
        policy,
        &granted(&[AppCapability::Network]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(
        !report.applied.net_namespace && !report.degraded,
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
    assert_eq!(service.accepted.load(Ordering::SeqCst), 1);
}

#[test]
fn a_manifest_cannot_opt_out_of_the_network_namespace() {
    let policy = IsolationPolicy {
        net_namespace: false,
        ..IsolationPolicy::default()
    };
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let mut running = launch(&["sh", "-c", ONLY_LOOPBACK], policy);
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

#[test]
fn readiness_still_arrives_from_inside_a_network_namespace() {
    // The runtime's listener is an abstract socket in the launcher's network
    // namespace; stage 1 connects before it unshares. Without that the
    // report would be lost and the launch reported `unverified`.
    if !available(&IsolationPolicy::default()) {
        return;
    }
    let mut running = launch(&["sh", "-c", "exit 0"], IsolationPolicy::default());
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(
        report
            .reason
            .as_deref()
            .is_none_or(|r| !r.contains("unverified")),
        "{report:?}"
    );
    assert!(
        report.applied.net_namespace && !report.degraded,
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

// ---------------------------------------------------------------------------
// Filesystem view (issue #10, increment 3)
// ---------------------------------------------------------------------------

/// A scratch home folder holding a private document; removed on drop.
struct ScratchHome(PathBuf);

impl ScratchHome {
    fn new() -> Self {
        Self::at(scratch("ferrix-home"))
    }

    /// Under `/home` (the container's own filesystem, and not a reserved
    /// directory a home may never be in), for tests that check ownership on
    /// the host side.
    fn local() -> Self {
        Self::at(Path::new("/home").join(unique("ferrix-home")))
    }

    fn at(home: PathBuf) -> Self {
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        std::fs::write(home.join("Documents/secret.txt"), "secret").unwrap();
        Self(home)
    }

    fn document(&self) -> PathBuf {
        self.0.join("Documents/secret.txt")
    }

    fn data_dir(&self) -> PathBuf {
        self.0.join(".local/share/ferrix/org.ferrix.test")
    }

    fn context(&self, capabilities: &[AppCapability]) -> LaunchContext {
        let mut context = granted(capabilities);
        context
            .env
            .push(("HOME".to_owned(), self.0.display().to_string()));
        context
    }
}

impl Drop for ScratchHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn minimal_root_hides_the_host_and_keeps_the_system_read_only() {
    let policy = IsolationPolicy::default();
    if !available(&policy) {
        return;
    }
    let home = ScratchHome::new();
    let host_file = scratch("ferrix-host-file");
    std::fs::write(&host_file, "host").unwrap();
    let _host_file = RemoveOnDrop(host_file.clone());
    // Each forbidden action that succeeds is its own exit code.
    let script = r#"
        [ -e "$1" ] && exit 10                                   # host scratch file
        [ -e "$2" ] && exit 11                                   # home document without files-home
        [ -x /bin/sh ] && [ -r /etc/passwd ] || exit 12          # system directories present
        touch /usr/ferrix-probe 2>/dev/null && exit 13           # /usr read-only
        touch /etc/ferrix-probe 2>/dev/null && exit 14           # /etc read-only
        echo $$ > /sys/fs/cgroup/cgroup.procs 2>/dev/null && exit 15   # no cgroup escape
        [ -c /dev/null ] && [ -c /dev/pts/ptmx ] || exit 16      # minimal /dev
        [ -e /dev/kmsg ] || [ -e /dev/mem ] && exit 17           # nothing else from the host /dev
        [ "$HOME" = "$3" ] && [ -d "$HOME" ] || exit 18          # an empty stand-in home
        echo kept > "$FERRIX_APP_DATA_DIR/note" || exit 19       # writable data directory
        exit 0
    "#;
    let mut running = launch_with(
        &[
            "sh",
            "-c",
            script,
            "sh",
            host_file.to_str().unwrap(),
            home.document().to_str().unwrap(),
            home.0.to_str().unwrap(),
        ],
        policy,
        &home.context(&[]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    let view = &report.applied.fs_view;
    assert_eq!(view.mode, FsViewMode::Minimal, "{report:?}");
    assert_eq!(view.root.as_deref(), Some("pivot_root"), "{report:?}");
    assert!(view.read_only.iter().any(|d| d == "/usr"), "{report:?}");
    assert!(view.home.is_none() && !report.degraded, "{report:?}");
    assert_eq!(
        view.data_dir.as_deref(),
        home.data_dir().to_str(),
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
    // The data directory is the host's and survives the launch.
    assert_eq!(
        std::fs::read_to_string(home.data_dir().join("note")).unwrap(),
        "kept\n"
    );
}

#[test]
fn files_home_grant_shows_the_home_folder_read_write() {
    let policy = IsolationPolicy::default();
    if !available(&policy) {
        return;
    }
    let home = ScratchHome::new();
    let script = r#"[ "$(cat "$1")" = secret ] && echo made > "$HOME/Documents/new.txt""#;
    let mut running = launch_with(
        &["sh", "-c", script, "sh", home.document().to_str().unwrap()],
        policy,
        &home.context(&[AppCapability::FilesHome]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(
        report.applied.fs_view.home.as_deref(),
        home.0.to_str(),
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
    assert_eq!(
        std::fs::read_to_string(home.0.join("Documents/new.txt")).unwrap(),
        "made\n"
    );
}

#[test]
fn a_reserved_directory_is_never_exposed_as_the_home_folder() {
    let policy = IsolationPolicy::default();
    if !available(&policy) {
        return;
    }
    let mut context = granted(&[AppCapability::FilesHome]);
    context.env.push(("HOME".to_owned(), "/etc".to_owned()));
    let config = serde_json::to_value(NativeLaunch {
        exec: vec!["true".to_owned()],
        isolation: policy,
    })
    .unwrap();
    // Required: an unmet grant is a failed launch, not a silent widening.
    assert!(matches!(
        runtime().launch(&descriptor(), &config, &context),
        Err(ferrix_app_api::AppError::IsolationUnavailable { .. })
    ));
}

#[test]
fn inherit_keeps_the_launcher_filesystem_and_still_sets_the_data_dir() {
    let policy = IsolationPolicy {
        fs_view: FsViewPolicy::inherit(),
        ..IsolationPolicy::default()
    };
    if !available(&policy) {
        return;
    }
    let home = ScratchHome::new();
    let script = r#"[ -e "$1" ] && [ -d "$FERRIX_APP_DATA_DIR" ]"#;
    let mut running = launch_with(
        &["sh", "-c", script, "sh", home.document().to_str().unwrap()],
        policy,
        &home.context(&[]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(report.applied.fs_view.mode, FsViewMode::Inherit);
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

// ---------------------------------------------------------------------------
// seccomp (issue #10, increment 4)
// ---------------------------------------------------------------------------

/// Actions a root application inside its own namespaces could take without
/// the filter; each that succeeds is its own exit code.
const FORBIDDEN_SYSCALLS: &str = r#"
    mkdir -p /tmp/m
    mount -t tmpfs none /tmp/m 2>/dev/null && exit 20        # mount
    mount -o remount,rw /usr 2>/dev/null && exit 21          # undo the read-only view
    unshare -n true 2>/dev/null && exit 22                   # new namespaces
    unshare -U true 2>/dev/null && exit 23
    chroot / true 2>/dev/null && exit 24                     # chroot (escape primitive)
    grep -q '^Seccomp:[[:space:]]*2' /proc/self/status || exit 25
    exit 0
"#;

fn seccomp(policy: SeccompPolicy) -> IsolationPolicy {
    IsolationPolicy {
        seccomp: policy,
        ..IsolationPolicy::default()
    }
}

#[test]
fn default_seccomp_profile_refuses_dangerous_system_calls() {
    let policy = IsolationPolicy::default();
    if !available(&policy) {
        return;
    }
    let mut running = launch(&["sh", "-c", FORBIDDEN_SYSCALLS], policy);
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    let filter = &report.applied.seccomp;
    assert_eq!(filter.profile, Some(SeccompProfile::Default), "{report:?}");
    // The deny list plus the clone-with-namespace-flags rule.
    assert_eq!(
        filter.denied,
        ferrix_runtime_native::seccomp::denied_for_target().len() + 1
    );
    assert!(!report.degraded, "{report:?}");
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

#[test]
fn without_the_filter_the_same_mount_succeeds() {
    // The control for the test above: the refusal comes from seccomp, not
    // from missing privilege.
    let policy = seccomp(SeccompPolicy::unconfined());
    if !available(&policy) {
        return;
    }
    let mut running = launch(&["sh", "-c", FORBIDDEN_SYSCALLS], policy);
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert!(!report.applied.seccomp.applied(), "{report:?}");
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 20 });
}

#[test]
fn the_escape_hatch_relaxes_only_the_named_calls() {
    let policy = seccomp(SeccompPolicy {
        allow: vec!["mount".to_owned(), "umount2".to_owned()],
        ..SeccompPolicy::default()
    });
    if !available(&policy) {
        return;
    }
    let script = r#"mkdir -p /tmp/m && mount -t tmpfs none /tmp/m && umount /tmp/m || exit 30
        unshare -n true 2>/dev/null && exit 31
        exit 0"#;
    let mut running = launch(&["sh", "-c", script], policy);
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(report.applied.seccomp.allowed, ["mount", "umount2"]);
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}

// ---------------------------------------------------------------------------
// Per-application uid (issue #10, increment 5)
// ---------------------------------------------------------------------------

#[test]
fn per_app_uid_runs_the_application_as_its_own_subuid() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let policy = IsolationPolicy {
        uid: UidMode::PerApp,
        ..IsolationPolicy::default()
    };
    if !available(&policy) {
        return;
    }
    // A container-local directory: the cargo target directory may be a host
    // share (macOS through OrbStack) that does not keep foreign owners.
    let home = ScratchHome::local();
    let uid = app_subuid("org.ferrix.test");
    // /etc/shadow is root-only (0640 root:shadow): the launcher can read it,
    // the application must not, although it is "root-like" in nothing.
    let script = r#"
        [ "$(id -u)" = "$1" ] && [ "$(id -g)" = "$1" ] || exit 40
        [ "$(id -G)" = "$1" ] || exit 41                     # no supplementary groups
        [ -e /etc/shadow ] || exit 42
        cat /etc/shadow >/dev/null 2>&1 && exit 43           # forbidden path
        grep -q '^CapEff:[[:space:]]*0*$' /proc/self/status || exit 44   # no capabilities at all
        echo mine > "$FERRIX_APP_DATA_DIR/f" || exit 45      # its data directory
        [ "$(stat -c %u:%g "$FERRIX_APP_DATA_DIR/f")" = "$1:$1" ] || exit 46
        touch "$HOME/x" || exit 47                           # its stand-in home
        exit 0
    "#;
    let mut running = launch_with(
        &["sh", "-c", script, "sh", &uid.to_string()],
        policy,
        &home.context(&[]),
    );
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(
        report.applied.identity,
        IdentityOutcome {
            mode: UidMode::PerApp,
            uid: Some(uid),
            gid: Some(uid),
        },
        "{report:?}"
    );
    assert!(
        report.applied.user_namespace && !report.degraded,
        "{report:?}"
    );
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
    // On the host the files belong to the subuid, and the directory is
    // owner-only.
    let data = std::fs::metadata(home.data_dir()).unwrap();
    assert_eq!((data.uid(), data.gid()), (uid, uid));
    assert_eq!(data.permissions().mode() & 0o777, 0o700);
    let file = std::fs::metadata(home.data_dir().join("f")).unwrap();
    assert_eq!(file.uid(), uid);
}

#[test]
fn launcher_identity_is_the_default() {
    let policy = IsolationPolicy::default();
    if !available(&policy) {
        return;
    }
    let mut running = launch(&["sh", "-c", "exit 0"], policy);
    let report: ferrix_runtime_native::IsolationReport =
        serde_json::from_value(running.diagnostics()).unwrap();
    assert_eq!(report.applied.identity, IdentityOutcome::default());
    assert_eq!(running.wait().unwrap(), ExitReason::Exited { code: 0 });
}
