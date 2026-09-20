//! Linux-only end-to-end tests: the built `ferrix-sandbox` through
//! `NativeRuntime`. Run from the repository root with
//! `just linux-check test -p runtime-native` (privileged container). When the
//! default policy cannot be met (EPERM in a locked-down container) each test
//! prints a `sandbox-test-skipped` record and passes vacuously.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use app_api::{
    AppDescriptor, AppId, AppRole, ExitReason, IconRef, RuntimeKind, SCHEMA_VERSION, TrustLevel,
};
use app_runtime_api::{LaunchContext, RunningApp, Runtime};
use runtime_native::{
    IsolationPolicy, NativeLaunch, NativeRuntime, NativeRuntimeOptions, SandboxReady,
    UserNamespace, emit, find_record, probe_sandbox_with, sandbox_plan, skipped_record,
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
    }
}

fn context() -> LaunchContext {
    LaunchContext {
        instance: 1,
        env: vec![("FERRIX_APP_ID".to_owned(), "org.ferrix.test".to_owned())],
        evidence: EVIDENCE.to_owned(),
    }
}

fn runtime() -> NativeRuntime {
    NativeRuntime::new(NativeRuntimeOptions {
        sandbox_bin: Some(PathBuf::from(SANDBOX)),
        evidence: EVIDENCE.to_owned(),
    })
}

/// `false` (after printing the skip record) when `policy` cannot be met here.
fn available(policy: &IsolationPolicy) -> bool {
    match probe_sandbox_with(Some(Path::new(SANDBOX)), policy) {
        Ok(_) => true,
        Err(reason) => {
            emit(&skipped_record(EVIDENCE, &reason));
            false
        }
    }
}

fn launch(exec: &[&str], policy: IsolationPolicy) -> Box<dyn RunningApp> {
    let config = serde_json::to_value(NativeLaunch {
        exec: exec.iter().map(|s| (*s).to_owned()).collect(),
        isolation: policy,
    })
    .unwrap();
    runtime()
        .launch(&descriptor(), &config, &context())
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
    let report: runtime_native::IsolationReport =
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
    let report: runtime_native::IsolationReport =
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
