//! Pure planning helpers shared by the runtime and the sandbox binary:
//! policy -> namespace selection, sandbox argv construction, exit-status
//! mapping, and sandbox argument parsing. Everything here is testable on any
//! host.

use std::process::ExitStatus;

use ferrix_app_api::{AppId, ExitReason};
use serde::{Deserialize, Serialize};

use crate::{AppliedIsolation, IsolationPolicy, RunDirOutcome, UserNamespace};

/// Name of the sandbox supervisor binary, resolved through `PATH`.
pub const SANDBOX_BIN: &str = "ferrix-sandbox";
/// Environment variable overriding the sandbox binary path.
pub const SANDBOX_BIN_ENV: &str = "FERRIX_SANDBOX_BIN";

/// Exit status of `ferrix-sandbox` when a `Required` policy cannot be met:
/// the namespaces could not be created. The runtime maps it to
/// [`ferrix_app_api::AppError::IsolationUnavailable`].
pub const ISOLATION_UNAVAILABLE_STATUS: i32 = 126;
/// Exit status when the application program itself could not be executed
/// (shell convention for "command not found").
pub const EXEC_FAILED_STATUS: i32 = 127;
/// Exit status when the sandbox failed internally (it could not start its own
/// init stage).
pub const SANDBOX_INTERNAL_STATUS: i32 = 125;
/// Exit status for a command-line usage error.
pub const USAGE_STATUS: i32 = 2;

/// Which Linux namespaces are (to be) created. Field names are the short
/// forms used in the `sandbox-ready` record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NamespaceSet {
    pub pid: bool,
    pub mount: bool,
    pub ipc: bool,
    pub uts: bool,
    pub user: bool,
}

impl NamespaceSet {
    pub fn any(self) -> bool {
        self.pid || self.mount || self.ipc || self.uts || self.user
    }

    /// Names of the namespaces in `wanted` that are missing here.
    pub fn missing_from(self, wanted: NamespaceSet) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if wanted.pid && !self.pid {
            missing.push("pid");
        }
        if wanted.mount && !self.mount {
            missing.push("mount");
        }
        if wanted.ipc && !self.ipc {
            missing.push("ipc");
        }
        if wanted.uts && !self.uts {
            missing.push("uts");
        }
        if wanted.user && !self.user {
            missing.push("user");
        }
        missing
    }

    /// Combines the namespace outcome with the init-stage outcome.
    pub fn applied(
        self,
        private_tmp: bool,
        no_new_privs: bool,
        run_dir: RunDirOutcome,
    ) -> AppliedIsolation {
        AppliedIsolation {
            pid_namespace: self.pid,
            mount_namespace: self.mount,
            ipc_namespace: self.ipc,
            uts_namespace: self.uts,
            user_namespace: self.user,
            private_tmp,
            no_new_privs,
            run_dir,
        }
    }
}

/// The namespaces stage 1 asks the kernel for. `UserNamespace::Auto` means
/// "only when the launcher is not root": root already holds the capabilities
/// the other namespaces need, and an unprivileged launcher can only get them
/// by owning a user namespace.
pub fn select_namespaces(policy: &IsolationPolicy, is_root: bool) -> NamespaceSet {
    let user = match policy.user_namespace {
        UserNamespace::Always => true,
        UserNamespace::Never => false,
        UserNamespace::Auto => !is_root,
    };
    NamespaceSet {
        pid: policy.pid_namespace,
        mount: policy.mount_namespace,
        ipc: policy.ipc_namespace,
        uts: policy.uts_namespace,
        // A user namespace on its own buys nothing for the application; it
        // is only the key to the others (or explicitly demanded).
        user: user
            && (policy.requests_any_namespace() || policy.user_namespace == UserNamespace::Always),
    }
}

/// Whether the policy needs the sandbox at all. Without it the application
/// is still a separate supervised process, just spawned directly.
pub fn needs_sandbox(policy: &IsolationPolicy) -> bool {
    policy.requests_any_namespace()
        || policy.private_tmp
        || policy.no_new_privs
        || policy.run_dir.is_private()
}

/// Serializes a policy for the `--policy` argument.
pub fn policy_json(policy: &IsolationPolicy) -> String {
    serde_json::to_string(policy).unwrap_or_default()
}

/// The `ferrix-sandbox` argument vector (without the program) for one launch:
/// `--policy <json> --app-id <id> --evidence <label> -- <exec...>`.
pub fn sandbox_plan(
    policy: &IsolationPolicy,
    app_id: &AppId,
    evidence: &str,
    exec: &[String],
) -> Vec<String> {
    sandbox_args(policy, app_id, evidence, None, exec)
}

/// [`sandbox_plan`] plus the optional readiness report channel name the
/// runtime listens on.
pub fn sandbox_args(
    policy: &IsolationPolicy,
    app_id: &AppId,
    evidence: &str,
    report: Option<&str>,
    exec: &[String],
) -> Vec<String> {
    let mut args = vec![
        "--policy".to_owned(),
        policy_json(policy),
        "--app-id".to_owned(),
        app_id.to_string(),
        "--evidence".to_owned(),
        evidence.to_owned(),
    ];
    if let Some(report) = report {
        args.push("--report".to_owned());
        args.push(report.to_owned());
    }
    args.push("--".to_owned());
    args.extend(exec.iter().cloned());
    args
}

/// Maps a supervised child's `ExitStatus` to an [`ExitReason`]. A real
/// signal death of the child is reported as such; an exit code is decoded
/// with the shell convention (`128 + signal`) because the sandbox forwards
/// the application's death that way.
pub fn exit_reason_from_status(status: ExitStatus) -> ExitReason {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return ExitReason::Signaled { signal };
        }
    }
    ExitReason::from_shell_status(status.code().unwrap_or(1))
}

/// The shell status a supervisor exits with to forward `status` exactly.
pub fn shell_status_of(status: ExitStatus) -> i32 {
    exit_reason_from_status(status).shell_status()
}

/// What `ferrix-sandbox` was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxMode {
    /// Stage 1: unshare, then start the init stage.
    Launch,
    /// Stage 2 (`--init`): PID 1 of the new PID namespace.
    Init,
    /// `--probe`: try the policy and report, never start anything.
    Probe,
    /// Internal one-shot procfs mount helper for stage 2.
    MountProc,
}

/// Parsed `ferrix-sandbox` command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxArgs {
    pub mode: SandboxMode,
    pub policy: IsolationPolicy,
    pub app_id: String,
    pub evidence: String,
    /// Abstract socket name to send the readiness report to.
    pub report: Option<String>,
    /// Stage 2 only: the namespaces stage 1 established.
    pub applied: Option<NamespaceSet>,
    pub exec: Vec<String>,
}

impl SandboxArgs {
    pub const USAGE: &'static str = "usage: ferrix-sandbox [--policy JSON] [--app-id ID] [--evidence LABEL] [--report NAME] -- PROGRAM [ARGS...]\n       ferrix-sandbox --probe [--policy JSON] [--evidence LABEL]";

    /// Parses the arguments after the program name.
    pub fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut parsed = SandboxArgs {
            mode: SandboxMode::Launch,
            policy: IsolationPolicy::default(),
            app_id: "unknown.app".to_owned(),
            evidence: "QEMU".to_owned(),
            report: None,
            applied: None,
            exec: Vec::new(),
        };
        let mut it = args.into_iter().map(Into::into);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--init" => parsed.mode = SandboxMode::Init,
                "--probe" => parsed.mode = SandboxMode::Probe,
                "--mount-proc" => parsed.mode = SandboxMode::MountProc,
                "--policy" => {
                    let text = it.next().ok_or("--policy needs JSON")?;
                    parsed.policy =
                        serde_json::from_str(&text).map_err(|e| format!("bad --policy: {e}"))?;
                }
                "--app-id" => parsed.app_id = it.next().ok_or("--app-id needs an id")?,
                "--evidence" => parsed.evidence = it.next().ok_or("--evidence needs a label")?,
                "--report" => parsed.report = Some(it.next().ok_or("--report needs a name")?),
                "--applied" => {
                    let text = it.next().ok_or("--applied needs JSON")?;
                    parsed.applied = Some(
                        serde_json::from_str(&text).map_err(|e| format!("bad --applied: {e}"))?,
                    );
                }
                "--" => {
                    parsed.exec = it.collect();
                    break;
                }
                "--help" | "-h" => return Err(Self::USAGE.to_owned()),
                other => return Err(format!("unknown argument {other:?}\n{}", Self::USAGE)),
            }
        }
        if (parsed.mode == SandboxMode::Launch || parsed.mode == SandboxMode::Init)
            && parsed.exec.is_empty()
        {
            return Err(format!("missing program after --\n{}", Self::USAGE));
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IsolationMode;

    fn id() -> AppId {
        AppId::new("org.ferrix.terminal").unwrap()
    }

    #[test]
    fn plan_places_policy_id_evidence_then_exec_after_separator() {
        let policy = IsolationPolicy::default();
        let exec = vec!["ferrix-terminal".to_owned(), "--flag".to_owned()];
        let plan = sandbox_plan(&policy, &id(), "QEMU", &exec);
        assert_eq!(
            plan,
            vec![
                "--policy",
                &policy_json(&policy),
                "--app-id",
                "org.ferrix.terminal",
                "--evidence",
                "QEMU",
                "--",
                "ferrix-terminal",
                "--flag",
            ]
        );
        let parsed: IsolationPolicy = serde_json::from_str(&plan[1]).unwrap();
        assert_eq!(parsed, policy);
    }

    #[test]
    fn plan_round_trips_through_the_argument_parser() {
        let policy = IsolationPolicy {
            mode: IsolationMode::BestEffort,
            ..IsolationPolicy::default()
        };
        let exec = vec!["sh".to_owned(), "-c".to_owned(), "exit 3".to_owned()];
        let args = sandbox_args(&policy, &id(), "mock", Some("chan"), &exec);
        let parsed = SandboxArgs::parse(args).unwrap();
        assert_eq!(parsed.mode, SandboxMode::Launch);
        assert_eq!(parsed.policy, policy);
        assert_eq!(parsed.app_id, "org.ferrix.terminal");
        assert_eq!(parsed.evidence, "mock");
        assert_eq!(parsed.report.as_deref(), Some("chan"));
        assert_eq!(parsed.exec, exec);
    }

    #[test]
    fn argument_parser_handles_modes_and_errors() {
        let probe = SandboxArgs::parse(["--probe"]).unwrap();
        assert_eq!(probe.mode, SandboxMode::Probe);
        assert_eq!(probe.evidence, "QEMU");
        assert_eq!(probe.policy, IsolationPolicy::default());
        assert_eq!(
            SandboxArgs::parse(["--mount-proc"]).unwrap().mode,
            SandboxMode::MountProc
        );

        let init = SandboxArgs::parse([
            "--init",
            "--applied",
            r#"{"pid":true,"mount":true}"#,
            "--",
            "true",
        ])
        .unwrap();
        assert_eq!(init.mode, SandboxMode::Init);
        assert_eq!(
            init.applied,
            Some(NamespaceSet {
                pid: true,
                mount: true,
                ..NamespaceSet::default()
            })
        );

        assert!(SandboxArgs::parse(["--policy", "{"]).is_err());
        assert!(SandboxArgs::parse(["--app-id", "x.y"]).is_err(), "no exec");
        assert!(SandboxArgs::parse(["--bogus", "--", "true"]).is_err());
    }

    #[test]
    fn user_namespace_follows_privilege_under_auto() {
        let policy = IsolationPolicy::default();
        let as_root = select_namespaces(&policy, true);
        assert_eq!(
            as_root,
            NamespaceSet {
                pid: true,
                mount: true,
                ipc: true,
                uts: true,
                user: false
            }
        );
        let as_user = select_namespaces(&policy, false);
        assert!(as_user.user && as_user.pid && as_user.mount);

        let always = IsolationPolicy {
            user_namespace: UserNamespace::Always,
            ..IsolationPolicy::default()
        };
        assert!(select_namespaces(&always, true).user);

        let never = IsolationPolicy {
            user_namespace: UserNamespace::Never,
            ..IsolationPolicy::default()
        };
        assert!(!select_namespaces(&never, false).user);

        // No other namespace requested: Auto does not add a lone user namespace.
        let mut only_tmp = IsolationPolicy::none();
        only_tmp.user_namespace = UserNamespace::Auto;
        only_tmp.no_new_privs = true;
        assert!(!select_namespaces(&only_tmp, false).any());
        assert!(needs_sandbox(&only_tmp));
        assert!(!needs_sandbox(&IsolationPolicy::none()));
        assert!(needs_sandbox(&IsolationPolicy::default()));
    }

    #[test]
    fn missing_namespaces_are_named() {
        let wanted = select_namespaces(&IsolationPolicy::default(), false);
        let got = NamespaceSet {
            pid: true,
            mount: false,
            ipc: true,
            uts: true,
            user: false,
        };
        assert_eq!(got.missing_from(wanted), vec!["mount", "user"]);
        assert!(wanted.missing_from(wanted).is_empty());
        let applied = got.applied(false, true, RunDirOutcome::default());
        assert!(applied.pid_namespace && !applied.mount_namespace);
        assert!(!applied.private_tmp && applied.no_new_privs);
        assert_eq!(applied.run_dir.mode, crate::RunDirMode::Inherit);
    }

    #[test]
    fn run_dir_policy_parses_with_defaults_and_rejects_unknown_fields() {
        use crate::{RunDirMode, RunDirPolicy};
        // Older manifests without `run_dir` keep working.
        let parsed: IsolationPolicy = serde_json::from_str(r#"{"private_tmp":false}"#).unwrap();
        assert_eq!(parsed.run_dir, RunDirPolicy::default());
        assert_eq!(parsed.run_dir.mode, RunDirMode::Inherit);
        assert!(!parsed.run_dir.is_private());

        let parsed: IsolationPolicy = serde_json::from_str(
            r#"{"run_dir":{"mode":"private","sockets":["ferrix-appd.sock","core.sock"]}}"#,
        )
        .unwrap();
        assert_eq!(parsed.run_dir.mode, RunDirMode::Private);
        assert_eq!(parsed.run_dir.sockets, ["ferrix-appd.sock", "core.sock"]);
        assert!(parsed.run_dir.is_private());
        assert!(needs_sandbox(&IsolationPolicy {
            run_dir: parsed.run_dir.clone(),
            ..IsolationPolicy::none()
        }));

        let parsed: IsolationPolicy =
            serde_json::from_str(r#"{"run_dir":{"mode":"private-run"}}"#).unwrap();
        assert_eq!(parsed.run_dir.mode, RunDirMode::PrivateRun);
        assert!(parsed.run_dir.sockets.is_empty());

        assert!(
            serde_json::from_str::<IsolationPolicy>(r#"{"run_dir":{"mode":"shared"}}"#).is_err()
        );
        assert!(
            serde_json::from_str::<IsolationPolicy>(r#"{"run_dir":{"paths":["/run"]}}"#).is_err()
        );

        // The default policy round-trips with the new field present.
        let text = policy_json(&IsolationPolicy::default());
        assert!(
            text.contains(r#""run_dir":{"mode":"inherit","sockets":[]}"#),
            "{text}"
        );
        assert_eq!(
            serde_json::from_str::<IsolationPolicy>(&text).unwrap(),
            IsolationPolicy::default()
        );
    }

    #[cfg(unix)]
    #[test]
    fn exit_statuses_map_to_exit_reasons() {
        use std::os::unix::process::ExitStatusExt as _;
        // wait(2) encoding: exit code in the high byte, signal in the low 7 bits.
        let exited = |code: i32| ExitStatus::from_raw(code << 8);
        let signaled = |signal: i32| ExitStatus::from_raw(signal);
        assert_eq!(
            exit_reason_from_status(exited(0)),
            ExitReason::Exited { code: 0 }
        );
        assert_eq!(
            exit_reason_from_status(exited(3)),
            ExitReason::Exited { code: 3 }
        );
        // The sandbox forwards a signal death as 128 + signal.
        assert_eq!(
            exit_reason_from_status(exited(139)),
            ExitReason::Signaled { signal: 11 }
        );
        assert_eq!(shell_status_of(exited(139)), 139);
        // The sandbox itself dying of a signal is a signal death too.
        assert_eq!(
            exit_reason_from_status(signaled(9)),
            ExitReason::Signaled { signal: 9 }
        );
        assert_eq!(shell_status_of(signaled(15)), 143);
        assert_eq!(
            exit_reason_from_status(exited(ISOLATION_UNAVAILABLE_STATUS)),
            ExitReason::Exited { code: 126 }
        );
    }
}
