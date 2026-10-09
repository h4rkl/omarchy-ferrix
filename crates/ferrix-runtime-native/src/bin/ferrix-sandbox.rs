//! `ferrix-sandbox`: the namespace supervisor for native applications.
//!
//! ```text
//! ferrix-sandbox --policy JSON --app-id ID [--evidence LABEL] [--report NAME] -- PROGRAM [ARGS...]
//! ferrix-sandbox --probe [--policy JSON]
//! ```
//!
//! Stage 1 (this invocation) unshares the namespaces the policy asks for,
//! sets up the user-namespace mappings and hostname, and starts itself again
//! as `--init`, which becomes PID 1 of the new PID namespace. Stage 2 makes
//! the mount namespace private, mounts a fresh `/proc` and (optionally) a
//! private `/tmp`, sets `no_new_privs`, runs the application, reaps zombies,
//! forwards TERM/INT/HUP, and exits with the application's shell status. Stage
//! 1 forwards that status upward, so the launcher classifies the exit exactly.
//!
//! With `run_dir.mode = private` (or `private-run`) stage 2 also replaces the
//! platform socket directory with a private tmpfs holding only the listed
//! sockets, so an application can `connect()` exactly the daemons its grants
//! allow (ADR-004 §4).
//!
//! All records are single JSON lines on stdout with the evidence label first:
//! `sandbox-ready`, `sandbox-failed`, `sandbox-mount-failed`,
//! `sandbox-degraded`, `sandbox-exec-failed`, `sandbox-app-exited`,
//! `sandbox-probe`. Everything is safe Rust (`unsafe_code` is forbidden
//! workspace-wide).

use std::process::ExitCode;

use ferrix_runtime_native::{SandboxArgs, SandboxMode, USAGE_STATUS, emit, record};
use serde_json::json;

fn exit_code(status: i32) -> ExitCode {
    ExitCode::from(u8::try_from(status).unwrap_or(1))
}

fn evidence_from_argv() -> String {
    let args: Vec<String> = std::env::args().skip(1).collect();
    args.windows(2)
        .find(|pair| pair[0] == "--evidence")
        .map(|pair| pair[1].clone())
        .unwrap_or_else(|| "QEMU".to_owned())
}

fn main() -> ExitCode {
    let args = match SandboxArgs::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(reason) => {
            emit(&record(
                &evidence_from_argv(),
                "sandbox-failed",
                json!({ "reason": reason }),
            ));
            return exit_code(USAGE_STATUS);
        }
    };
    #[cfg(target_os = "linux")]
    {
        match args.mode {
            SandboxMode::Launch => linux::launch(&args),
            SandboxMode::Init => linux::init(&args),
            SandboxMode::Probe => linux::probe(&args),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = SandboxMode::Launch;
        emit(&record(
            &args.evidence,
            "sandbox-failed",
            json!({ "app": args.app_id, "reason": "not linux" }),
        ));
        exit_code(ferrix_runtime_native::ISOLATION_UNAVAILABLE_STATUS)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CStr;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, ExitStatus};
    use std::thread;
    use std::time::Duration;

    use ferrix_app_api::ExitReason;
    use ferrix_runtime_native::channel::{ReportListener, WaitOutcome, send_line};
    use ferrix_runtime_native::{
        EXEC_FAILED_STATUS, ISOLATION_UNAVAILABLE_STATUS, InitReport, IsolationMode,
        IsolationPolicy, NamespaceSet, ProbeReport, RUN_ROOT, RunDirMode, RunDirOutcome,
        RunDirPolicy, SANDBOX_INTERNAL_STATUS, SandboxArgs, SandboxReady, USER_RUN_DIR,
        UserNamespace, emit, is_valid_socket_name, record, run_dir, select_namespaces,
        shell_status_of,
    };
    use rustix::io::Errno;
    use rustix::mount::{
        MountFlags, MountPropagationFlags, UnmountFlags, mount, mount_bind, mount_bind_recursive,
        mount_change, mount_move, unmount,
    };
    use rustix::process::{
        Pid, Signal, WaitOptions, getegid, geteuid, kill_process, set_parent_process_death_signal,
        waitpid,
    };
    use rustix::thread::{
        Capability, CapabilitySet, UnshareFlags, capabilities, clear_ambient_capability_set,
        configure_capability_in_ambient_set, set_capabilities, set_no_new_privs,
    };
    use serde_json::json;
    use signal_hook::consts::{SIGCHLD, SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    /// How long stage 1 waits for stage 2's report (shorter than the
    /// runtime's wait so a `sandbox-ready` record always reaches the log).
    const INIT_REPORT_TIMEOUT: Duration = Duration::from_secs(8);

    fn exit_code(status: i32) -> ExitCode {
        ExitCode::from(u8::try_from(status).unwrap_or(1))
    }

    fn pdeathsig_kill(evidence: &str, app: &str) {
        if let Err(e) = set_parent_process_death_signal(Some(Signal::KILL)) {
            emit(&record(
                evidence,
                "sandbox-degraded",
                json!({ "app": app, "feature": "pdeathsig", "error": e.to_string() }),
            ));
        }
    }

    /// Namespace identities from `/proc/self/ns`; a namespace is proven new
    /// when its link target changes across `unshare`.
    #[derive(Default)]
    struct NsLinks {
        pid_for_children: Option<PathBuf>,
        mnt: Option<PathBuf>,
        ipc: Option<PathBuf>,
        uts: Option<PathBuf>,
        user: Option<PathBuf>,
    }

    impl NsLinks {
        fn read() -> Self {
            let link = |name: &str| std::fs::read_link(format!("/proc/self/ns/{name}")).ok();
            Self {
                pid_for_children: link("pid_for_children"),
                mnt: link("mnt"),
                ipc: link("ipc"),
                uts: link("uts"),
                user: link("user"),
            }
        }

        /// Namespaces whose identity changed (and was readable both times).
        fn changed_since(&self, before: &NsLinks) -> NamespaceSet {
            NamespaceSet {
                pid: pid_namespace_changed(&before.pid_for_children, &self.pid_for_children),
                mount: link_differs(&self.mnt, &before.mnt),
                ipc: link_differs(&self.ipc, &before.ipc),
                uts: link_differs(&self.uts, &before.uts),
                user: link_differs(&self.user, &before.user),
            }
        }
    }

    fn link_differs(a: &Option<PathBuf>, b: &Option<PathBuf>) -> bool {
        matches!((a, b), (Some(a), Some(b)) if a != b)
    }

    /// `pid_for_children` is special (namespaces(7)): after `unshare` it is
    /// unreadable (ENOENT) until the first child exists in the new namespace,
    /// so "readable before, gone after" is the new namespace too. Once a child
    /// exists the link resolves again and must differ from the old one.
    fn pid_namespace_changed(before: &Option<PathBuf>, after: &Option<PathBuf>) -> bool {
        match (before, after) {
            (Some(a), Some(b)) => a != b,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Launcher identity captured before `unshare`: inside a fresh user
    /// namespace `geteuid` reports the overflow id until a map is written.
    #[derive(Clone, Copy)]
    struct Identity {
        uid: u32,
        gid: u32,
    }

    impl Identity {
        fn current() -> Self {
            Self {
                uid: geteuid().as_raw(),
                gid: getegid().as_raw(),
            }
        }
    }

    /// Namespace-only `unshare`. rustix deprecates the safe `unshare` because
    /// `UnshareFlags::FILES` can strand file descriptors across threads; the
    /// sandbox never unshares the descriptor table (the workspace forbids the
    /// `unsafe` replacement), so the safe call is sound for these flags.
    #[allow(deprecated)]
    fn unshare_namespaces(flags: UnshareFlags) -> rustix::io::Result<()> {
        debug_assert!(!flags.contains(UnshareFlags::FILES));
        rustix::thread::unshare(flags)
    }

    fn unshare_flags(wanted: NamespaceSet) -> UnshareFlags {
        let mut flags = UnshareFlags::empty();
        if wanted.user {
            flags |= UnshareFlags::NEWUSER;
        }
        if wanted.pid {
            flags |= UnshareFlags::NEWPID;
        }
        if wanted.mount {
            flags |= UnshareFlags::NEWNS;
        }
        if wanted.ipc {
            flags |= UnshareFlags::NEWIPC;
        }
        if wanted.uts {
            flags |= UnshareFlags::NEWUTS;
        }
        flags
    }

    /// Outcome of the namespace step.
    struct Unshared {
        wanted: NamespaceSet,
        applied: NamespaceSet,
        notes: Vec<String>,
        identity: Identity,
        /// `pid_for_children` before `unshare`, to re-confirm the PID
        /// namespace once the first child exists.
        pid_link_before: Option<PathBuf>,
    }

    /// Creates the requested namespaces in one `unshare` call (so the user
    /// namespace, when present, owns the others). Under `Auto` a root
    /// launcher that still lacks the privilege (an unprivileged container)
    /// gets one retry with a user namespace.
    fn unshare_for(policy: &IsolationPolicy) -> Result<Unshared, (NamespaceSet, String)> {
        let identity = Identity::current();
        let mut wanted = select_namespaces(policy, identity.uid == 0);
        let mut notes = Vec::new();
        let before = NsLinks::read();
        // CONFIG_IPC_NS exists only with SysV IPC or POSIX mqueue; a kernel
        // without it (Android GKI) has no IPC objects to share, and asking for
        // CLONE_NEWIPC fails the whole unshare with EINVAL.
        if wanted.ipc && !Path::new("/proc/self/ns/ipc").exists() {
            wanted.ipc = false;
            notes.push(
                "kernel has no IPC namespace (no SysV IPC or POSIX mqueue to isolate)".to_owned(),
            );
        }
        if !wanted.any() {
            return Ok(Unshared {
                wanted,
                applied: NamespaceSet::default(),
                notes,
                identity,
                pid_link_before: before.pid_for_children,
            });
        }
        let mut result = unshare_namespaces(unshare_flags(wanted));
        if let Err(Errno::PERM) = result
            && !wanted.user
            && policy.user_namespace == UserNamespace::Auto
        {
            let retry = NamespaceSet {
                user: true,
                ..wanted
            };
            result = unshare_namespaces(unshare_flags(retry));
            if result.is_ok() {
                notes.push("unshare without a user namespace was denied; used one".to_owned());
                wanted = retry;
            }
        }
        match result {
            Ok(()) => {
                let applied = NsLinks::read().changed_since(&before);
                let missing = applied.missing_from(wanted);
                if !missing.is_empty() {
                    notes.push(format!(
                        "unshare succeeded but /proc/self/ns did not change for {}",
                        missing.join(",")
                    ));
                }
                Ok(Unshared {
                    wanted,
                    applied,
                    notes,
                    identity,
                    pid_link_before: before.pid_for_children,
                })
            }
            Err(e) => Err((wanted, format!("unshare({:?}): {e}", unshare_flags(wanted)))),
        }
    }

    /// Identity uid/gid mapping for a fresh user namespace: full capabilities
    /// inside it, the launcher's own identity outside.
    fn map_identity(identity: Identity) -> Vec<String> {
        let mut errors = Vec::new();
        let Identity { uid, gid } = identity;
        for (path, content) in [
            ("/proc/self/setgroups", "deny".to_owned()),
            ("/proc/self/uid_map", format!("{uid} {uid} 1")),
            ("/proc/self/gid_map", format!("{gid} {gid} 1")),
        ] {
            if let Err(e) = std::fs::write(path, content) {
                errors.push(format!("{path}: {e}"));
            }
        }
        errors
    }

    fn child_pid(child: &Child) -> Pid {
        Pid::from_child(child)
    }

    /// Forwards TERM/INT/HUP to `target` from a helper thread. Signals are
    /// registered before the target exists so none are lost in between.
    fn forward_signals(mut signals: Signals, target: Pid) {
        thread::Builder::new()
            .name("signal-forward".to_owned())
            .spawn(move || {
                for signal in signals.forever() {
                    if let Some(signal) = Signal::from_named_raw(signal) {
                        let _ = kill_process(target, signal);
                    }
                }
            })
            .ok();
    }

    fn wait_status(child: &mut Child) -> ExitStatus {
        loop {
            match child.wait() {
                Ok(status) => return status,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return ExitStatus::default(),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Stage 1
    // -----------------------------------------------------------------------

    pub fn launch(args: &SandboxArgs) -> ExitCode {
        let evidence = args.evidence.as_str();
        let app = args.app_id.as_str();
        pdeathsig_kill(evidence, app);
        // Resolve before unshare: the mount namespace is a copy, but do not
        // rely on `/proc/self/exe` after changing namespaces.
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                return fail(
                    evidence,
                    app,
                    format!("current_exe: {e}"),
                    SANDBOX_INTERNAL_STATUS,
                );
            }
        };

        let mut notes = Vec::new();
        let unshared = match unshare_for(&args.policy) {
            Ok(unshared) => unshared,
            Err((wanted, reason)) => {
                if args.policy.mode == IsolationMode::Required {
                    return fail(evidence, app, reason, ISOLATION_UNAVAILABLE_STATUS);
                }
                notes.push(reason);
                Unshared {
                    wanted,
                    applied: NamespaceSet::default(),
                    notes: Vec::new(),
                    identity: Identity::current(),
                    pid_link_before: None,
                }
            }
        };
        notes.extend(unshared.notes);
        let mut applied = unshared.applied;

        if applied.user {
            for error in map_identity(unshared.identity) {
                emit(&record(
                    evidence,
                    "sandbox-degraded",
                    json!({ "app": app, "feature": "uid_map", "error": error }),
                ));
                notes.push(format!("uid_map: {error}"));
            }
            if applied.mount
                && unshared.identity.uid != 0
                && let Err(e) = keep_mount_capability_across_exec()
            {
                emit(&record(
                    evidence,
                    "sandbox-degraded",
                    json!({ "app": app, "feature": "mount_capability", "error": e.to_string() }),
                ));
                notes.push(format!("mount capability: {e}"));
            }
        }
        if applied.uts
            && let Err(e) = rustix::system::sethostname(app.as_bytes())
        {
            emit(&record(
                evidence,
                "sandbox-degraded",
                json!({ "app": app, "feature": "hostname", "error": e.to_string() }),
            ));
            notes.push(format!("sethostname: {e}"));
        }

        // Stage 1 must stay single-threaded: after `unshare(NEWPID)` the
        // kernel refuses new threads (they cannot straddle the pending PID
        // namespace). Signals are therefore registered here, before the
        // child exists, and drained from the main thread; SIGCHLD wakes the
        // final wait when stage 2 ends.
        let mut signals = match Signals::new([SIGTERM, SIGINT, SIGHUP, SIGCHLD]) {
            Ok(signals) => Some(signals),
            Err(e) => {
                notes.push(format!("signal forwarding unavailable: {e}"));
                None
            }
        };
        let listener = ReportListener::bind("ferrix-init").ok();

        let mut command = Command::new(&exe);
        command
            .arg("--init")
            .arg("--policy")
            .arg(serde_json::to_string(&args.policy).unwrap_or_default())
            .arg("--app-id")
            .arg(app)
            .arg("--evidence")
            .arg(evidence)
            .arg("--applied")
            .arg(serde_json::to_string(&applied).unwrap_or_default());
        if let Some(listener) = &listener {
            command.arg("--report").arg(listener.name());
        }
        command.arg("--").args(&args.exec);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                return fail(
                    evidence,
                    app,
                    format!("cannot start init stage {}: {e}", exe.display()),
                    SANDBOX_INTERNAL_STATUS,
                );
            }
        };
        let init_pid = child.id();
        let init = child_pid(&child);
        // Definitive PID-namespace proof: the link resolves again now that
        // the namespace has a member, and must differ from the old one.
        let pid_link_after = NsLinks::read().pid_for_children;
        if unshared.wanted.pid {
            applied.pid = pid_namespace_changed(&unshared.pid_link_before, &pid_link_after);
        }
        let missing = applied.missing_from(unshared.wanted);
        if !missing.is_empty() {
            notes.push(format!("namespaces not established: {}", missing.join(",")));
        }
        let mut forward_pending = || {
            if let Some(signals) = &mut signals {
                for signal in signals.pending() {
                    forward_one(signal, init);
                }
            }
        };

        let mut private_tmp = false;
        let mut no_new_privs = false;
        let mut run_dir_outcome = RunDirOutcome::default();
        match listener
            .map(|l| l.wait_for_line(&mut child, INIT_REPORT_TIMEOUT, &mut forward_pending))
        {
            Some(Ok(line)) => match serde_json::from_str::<InitReport>(&line) {
                Ok(report) => {
                    private_tmp = report.private_tmp;
                    no_new_privs = report.no_new_privs;
                    run_dir_outcome = report.run_dir;
                    notes.extend(report.reasons);
                }
                Err(e) => notes.push(format!("unreadable init report: {e}")),
            },
            Some(Err(WaitOutcome::ChildExited(status))) => {
                // Stage 2 refuses to start the application when any Required
                // isolation control cannot be established; forward that as
                // the isolation failure it is (no readiness).
                if shell_status_of(status) == ISOLATION_UNAVAILABLE_STATUS {
                    return fail(
                        evidence,
                        app,
                        "init stage could not establish required isolation (see sandbox-mount-failed)"
                            .to_owned(),
                        ISOLATION_UNAVAILABLE_STATUS,
                    );
                }
                notes.push(format!(
                    "init stage exited with status {} before reporting",
                    shell_status_of(status)
                ));
            }
            Some(Err(WaitOutcome::TimedOut)) => {
                notes.push("init stage did not report in time".to_owned())
            }
            Some(Err(WaitOutcome::Io(e))) => notes.push(format!("init report channel: {e}")),
            None => notes.push("init report channel unavailable".to_owned()),
        }
        let degraded = !missing.is_empty()
            || (args.policy.private_tmp && !private_tmp)
            || (args.policy.no_new_privs && !no_new_privs)
            || (args.policy.run_dir.is_private()
                && run_dir_outcome.mode != args.policy.run_dir.mode)
            || !notes.is_empty();
        let ready = SandboxReady {
            app: app.to_owned(),
            init_pid,
            namespaces: applied,
            private_tmp,
            no_new_privs,
            run_dir: run_dir_outcome,
            degraded,
            reason: (!notes.is_empty()).then(|| notes.join("; ")),
        };
        emit(&record(
            evidence,
            "sandbox-ready",
            serde_json::to_value(&ready).unwrap_or_default(),
        ));
        if let Some(name) = &args.report {
            let _ = send_line(name, &serde_json::to_string(&ready).unwrap_or_default());
        }

        // Forward until stage 2 ends; SIGCHLD (registered before the spawn,
        // so never missed) wakes the blocking wait.
        let status = match signals {
            Some(mut signals) => loop {
                if let Ok(Some(status)) = child.try_wait() {
                    break status;
                }
                for signal in signals.wait() {
                    forward_one(signal, init);
                }
            },
            None => wait_status(&mut child),
        };
        exit_code(shell_status_of(status))
    }

    /// Forwards one received signal to `target`; SIGCHLD is ours, not theirs.
    fn forward_one(signal: i32, target: Pid) {
        if signal == SIGCHLD {
            return;
        }
        if let Some(signal) = Signal::from_named_raw(signal) {
            let _ = kill_process(target, signal);
        }
    }

    fn fail(evidence: &str, app: &str, reason: String, status: i32) -> ExitCode {
        emit(&record(
            evidence,
            "sandbox-failed",
            json!({ "app": app, "reason": reason }),
        ));
        exit_code(status)
    }

    // -----------------------------------------------------------------------
    // Stage 2: PID 1 of the new PID namespace
    // -----------------------------------------------------------------------

    /// Mounts for the private mount namespace. Failures are recorded here;
    /// the init stage fails closed before exec under a Required policy and
    /// reports degradation under BestEffort.
    fn setup_mounts(
        evidence: &str,
        app: &str,
        policy: &IsolationPolicy,
        reasons: &mut Vec<String>,
    ) -> bool {
        let mut mount_failed = |target: &str, error: String| {
            emit(&record(
                evidence,
                "sandbox-mount-failed",
                json!({ "app": app, "target": target, "error": error }),
            ));
            reasons.push(format!("mount {target}: {error}"));
        };
        if let Err(e) = mount_change(
            "/",
            MountPropagationFlags::PRIVATE | MountPropagationFlags::REC,
        ) {
            mount_failed("/", e.to_string());
        }
        if let Err(e) = mount(
            "proc",
            "/proc",
            "proc",
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            None::<&CStr>,
        ) {
            mount_failed("/proc", e.to_string());
        }
        if !policy.private_tmp {
            return false;
        }
        match mount(
            "tmpfs",
            "/tmp",
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV,
            c"mode=1777,size=64m",
        ) {
            Ok(()) => true,
            Err(e) => {
                mount_failed("/tmp", e.to_string());
                false
            }
        }
    }

    // -- socket-directory view (ADR-004 §4) ------------------------------

    /// Where the filtered view is assembled before it is moved into place.
    /// `/tmp` is preferred: with `private_tmp` it is this namespace's own
    /// tmpfs; otherwise the directory only exists between `mkdir` and the
    /// `rmdir` after the move. A sibling of the socket directory is the
    /// fallback for `private` (never for `private-run`, whose target `/run`
    /// would contain it).
    fn staging_dir(socket_dir: &Path, mode: RunDirMode) -> Result<PathBuf, String> {
        let name = format!(".ferrix-run-{}", std::process::id());
        let mut candidates = vec![PathBuf::from("/tmp").join(&name)];
        if mode == RunDirMode::Private {
            candidates.push(socket_dir.with_file_name(name));
        }
        let mut errors = Vec::new();
        for candidate in candidates {
            match std::fs::create_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(e) => errors.push(format!("{}: {e}", candidate.display())),
            }
        }
        Err(format!("no staging directory: {}", errors.join("; ")))
    }

    /// Builds the application's view of the platform socket directory.
    ///
    /// The originals must stay visible while the sockets are bind-mounted by
    /// path, so the view is assembled on a tmpfs at a staging directory (each
    /// listed socket file is created there and the original bind-mounted
    /// over it, `/run/user` bound back recursively under `private-run`) and
    /// then moved over the target in one `mount(MS_MOVE)`. Nothing is opened
    /// or held open. Propagation is already private (stage 2 starts with
    /// `mount --make-rprivate /`), which `MS_MOVE` requires.
    fn setup_run_dir(policy: &RunDirPolicy) -> Result<RunDirOutcome, String> {
        let socket_dir = run_dir();
        let target = match policy.mode {
            RunDirMode::Inherit => return Ok(RunDirOutcome::default()),
            RunDirMode::Private => socket_dir.clone(),
            RunDirMode::PrivateRun => PathBuf::from(RUN_ROOT),
        };
        if !target.is_dir() {
            return Err(format!("{} is not a directory", target.display()));
        }
        // Where the socket directory sits inside the new view.
        let relative = match policy.mode {
            RunDirMode::PrivateRun => socket_dir
                .strip_prefix(RUN_ROOT)
                .map_err(|_| {
                    format!(
                        "{} is not under {RUN_ROOT}; private-run needs it there",
                        socket_dir.display()
                    )
                })?
                .to_path_buf(),
            _ => PathBuf::new(),
        };
        let staging = staging_dir(&socket_dir, policy.mode)?;
        let result = assemble_run_dir(&staging, &relative, &socket_dir, &target, policy);
        let _ = std::fs::remove_dir(&staging);
        result
    }

    fn assemble_run_dir(
        staging: &Path,
        relative: &Path,
        socket_dir: &Path,
        target: &Path,
        policy: &RunDirPolicy,
    ) -> Result<RunDirOutcome, String> {
        mount(
            "tmpfs",
            staging,
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            c"mode=0755,size=1m",
        )
        .map_err(|e| format!("mount tmpfs on {}: {e}", staging.display()))?;
        let result = populate_run_dir(staging, relative, socket_dir, policy);
        match result {
            Ok(outcome) => match mount_move(staging, target) {
                Ok(()) => Ok(outcome),
                Err(e) => {
                    let _ = unmount(staging, UnmountFlags::DETACH);
                    Err(format!(
                        "move {} onto {}: {e}",
                        staging.display(),
                        target.display()
                    ))
                }
            },
            Err(error) => {
                let _ = unmount(staging, UnmountFlags::DETACH);
                Err(error)
            }
        }
    }

    fn populate_run_dir(
        staging: &Path,
        relative: &Path,
        socket_dir: &Path,
        policy: &RunDirPolicy,
    ) -> Result<RunDirOutcome, String> {
        let io =
            |what: &str, path: &Path, e: std::io::Error| format!("{what} {}: {e}", path.display());
        let view = staging.join(relative);
        std::fs::create_dir_all(&view).map_err(|e| io("create", &view, e))?;
        if policy.mode == RunDirMode::PrivateRun && Path::new(USER_RUN_DIR).is_dir() {
            let user = staging.join(
                Path::new(USER_RUN_DIR)
                    .strip_prefix(RUN_ROOT)
                    .unwrap_or(Path::new("user")),
            );
            std::fs::create_dir_all(&user).map_err(|e| io("create", &user, e))?;
            mount_bind_recursive(USER_RUN_DIR, &user)
                .map_err(|e| format!("bind {USER_RUN_DIR} into the view: {e}"))?;
        }
        let mut sockets = Vec::new();
        let mut missing = Vec::new();
        for name in &policy.sockets {
            if !is_valid_socket_name(name) {
                return Err(format!("invalid socket name {name:?}"));
            }
            let source = socket_dir.join(name);
            if !source.exists() {
                missing.push(name.clone());
                continue;
            }
            let dest = view.join(name);
            std::fs::File::create(&dest).map_err(|e| io("create", &dest, e))?;
            mount_bind(&source, &dest)
                .map_err(|e| format!("bind {} into the view: {e}", source.display()))?;
            sockets.push(name.clone());
        }
        Ok(RunDirOutcome {
            mode: policy.mode,
            sockets,
            missing,
        })
    }

    /// Applies the socket-directory policy in stage 2 or the probe. `Err`
    /// carries the reason; the caller decides whether that is fatal.
    fn apply_run_dir(
        evidence: &str,
        app: &str,
        policy: &IsolationPolicy,
        mount_namespace: bool,
        reasons: &mut Vec<String>,
    ) -> Result<RunDirOutcome, String> {
        if !policy.run_dir.is_private() {
            return Ok(RunDirOutcome::default());
        }
        if !mount_namespace {
            reasons.push("run_dir needs the mount namespace".to_owned());
            return Ok(RunDirOutcome::default());
        }
        setup_run_dir(&policy.run_dir).map_err(|error| {
            emit(&record(
                evidence,
                "sandbox-mount-failed",
                json!({
                    "app": app,
                    "target": match policy.run_dir.mode {
                        RunDirMode::PrivateRun => RUN_ROOT.to_owned(),
                        _ => run_dir().display().to_string(),
                    },
                    "error": error,
                }),
            ));
            error
        })
    }

    /// An unprivileged launcher is a non-root uid inside its user namespace,
    /// so its capabilities there would not survive the `execve` of stage 2.
    /// Raising `CAP_SYS_ADMIN` into the ambient set carries the one capability
    /// the mounts need across that exec; stage 2 drops it before the
    /// application starts ([`drop_inherited_capabilities`]).
    fn keep_mount_capability_across_exec() -> rustix::io::Result<()> {
        let mut sets = capabilities(None)?;
        sets.inheritable |= CapabilitySet::SYS_ADMIN;
        set_capabilities(None, sets)?;
        configure_capability_in_ambient_set(Capability::SystemAdmin, true)
    }

    /// Empties the ambient and inheritable sets so the application's `execve`
    /// leaves it no capabilities (unless the launcher itself was root).
    fn drop_inherited_capabilities(evidence: &str, app: &str, reasons: &mut Vec<String>) {
        let result = clear_ambient_capability_set().and_then(|()| {
            let mut sets = capabilities(None)?;
            if sets.inheritable.is_empty() {
                return Ok(());
            }
            sets.inheritable = CapabilitySet::empty();
            set_capabilities(None, sets)
        });
        if let Err(e) = result {
            emit(&record(
                evidence,
                "sandbox-degraded",
                json!({ "app": app, "feature": "capability_drop", "error": e.to_string() }),
            ));
            reasons.push(format!("capability drop: {e}"));
        }
    }

    fn apply_no_new_privs(evidence: &str, app: &str, reasons: &mut Vec<String>) -> bool {
        match set_no_new_privs(true) {
            Ok(()) => true,
            Err(e) => {
                emit(&record(
                    evidence,
                    "sandbox-degraded",
                    json!({ "app": app, "feature": "no_new_privs", "error": e.to_string() }),
                ));
                reasons.push(format!("no_new_privs: {e}"));
                false
            }
        }
    }

    pub fn init(args: &SandboxArgs) -> ExitCode {
        let evidence = args.evidence.as_str();
        let app = args.app_id.as_str();
        pdeathsig_kill(evidence, app);
        let applied = args.applied.unwrap_or_default();
        let mut reasons = Vec::new();

        let private_tmp = if applied.mount {
            setup_mounts(evidence, app, &args.policy, &mut reasons)
        } else {
            if args.policy.private_tmp {
                reasons.push("private_tmp needs the mount namespace".to_owned());
            }
            false
        };
        let run_dir_outcome =
            match apply_run_dir(evidence, app, &args.policy, applied.mount, &mut reasons) {
                Ok(outcome) => outcome,
                Err(error) if args.policy.mode == IsolationMode::Required => {
                    // Required means required: never start the application
                    // with a wider socket view than the policy allows.
                    emit(&record(
                        evidence,
                        "sandbox-failed",
                        json!({ "app": app, "reason": format!("required run_dir view: {error}") }),
                    ));
                    return exit_code(ISOLATION_UNAVAILABLE_STATUS);
                }
                Err(error) => {
                    reasons.push(format!("run_dir: {error}"));
                    RunDirOutcome::default()
                }
            };
        drop_inherited_capabilities(evidence, app, &mut reasons);
        let no_new_privs =
            args.policy.no_new_privs && apply_no_new_privs(evidence, app, &mut reasons);

        if args.policy.mode == IsolationMode::Required {
            let wanted = select_namespaces(&args.policy, geteuid().is_root());
            let mut unmet = applied.missing_from(wanted);
            if args.policy.pid_namespace && std::process::id() != 1 {
                unmet.push("pid");
            }
            if args.policy.private_tmp && !private_tmp {
                unmet.push("private_tmp");
            }
            if args.policy.no_new_privs && !no_new_privs {
                unmet.push("no_new_privs");
            }
            if args.policy.run_dir.is_private() && run_dir_outcome.mode != args.policy.run_dir.mode
            {
                unmet.push("run_dir");
            }
            if !unmet.is_empty() || !reasons.is_empty() {
                let mut detail = Vec::new();
                if !unmet.is_empty() {
                    unmet.sort_unstable();
                    unmet.dedup();
                    detail.push(format!("unmet: {}", unmet.join(",")));
                }
                detail.extend(reasons);
                return fail(
                    evidence,
                    app,
                    format!("required isolation unavailable: {}", detail.join("; ")),
                    ISOLATION_UNAVAILABLE_STATUS,
                );
            }
        }

        let signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).ok();
        if let Some(name) = &args.report {
            let report = InitReport {
                private_tmp,
                no_new_privs,
                run_dir: run_dir_outcome,
                reasons: reasons.clone(),
            };
            let _ = send_line(name, &serde_json::to_string(&report).unwrap_or_default());
        }

        let program = &args.exec[0];
        let child = match Command::new(program).args(&args.exec[1..]).spawn() {
            Ok(child) => child,
            Err(e) => {
                emit(&record(
                    evidence,
                    "sandbox-exec-failed",
                    json!({ "app": app, "program": program, "error": e.to_string() }),
                ));
                return exit_code(EXEC_FAILED_STATUS);
            }
        };
        let app_pid = child_pid(&child);
        if let Some(signals) = signals {
            forward_signals(signals, app_pid);
        }

        // Mini init: reap everything; stop when the application itself ends.
        // PID 1's exit then kills whatever else is left in the namespace.
        let exit = loop {
            match waitpid(None, WaitOptions::empty()) {
                Ok(Some((pid, status))) if pid == app_pid => {
                    break match (status.exit_status(), status.terminating_signal()) {
                        (Some(code), _) => ExitReason::Exited { code },
                        (None, Some(signal)) => ExitReason::Signaled { signal },
                        (None, None) => ExitReason::Exited { code: 1 },
                    };
                }
                Ok(_) => continue,
                Err(Errno::INTR) => continue,
                Err(e) => {
                    emit(&record(
                        evidence,
                        "sandbox-degraded",
                        json!({ "app": app, "feature": "reap", "error": e.to_string() }),
                    ));
                    break ExitReason::Exited { code: 1 };
                }
            }
        };
        emit(&record(
            evidence,
            "sandbox-app-exited",
            json!({ "app": app, "exit": exit }),
        ));
        exit_code(exit.shell_status())
    }

    // -----------------------------------------------------------------------
    // Probe
    // -----------------------------------------------------------------------

    /// Tries the policy in this throwaway process: unshare, mappings, and the
    /// init-stage mounts inside the new (private) mount namespace.
    pub fn probe(args: &SandboxArgs) -> ExitCode {
        let evidence = args.evidence.as_str();
        let policy = &args.policy;
        let mut reasons = Vec::new();
        let unshared = match unshare_for(policy) {
            Ok(unshared) => unshared,
            Err((_, reason)) => {
                let report = ProbeReport {
                    applied: Default::default(),
                    degraded: true,
                    reason: Some(reason),
                };
                emit(&record(
                    evidence,
                    "sandbox-probe",
                    serde_json::to_value(&report).unwrap_or_default(),
                ));
                return exit_code(if policy.mode == IsolationMode::Required {
                    ISOLATION_UNAVAILABLE_STATUS
                } else {
                    0
                });
            }
        };
        reasons.extend(unshared.notes);
        let applied = unshared.applied;
        let missing = applied.missing_from(unshared.wanted);
        if !missing.is_empty() {
            reasons.push(format!("namespaces not established: {}", missing.join(",")));
        }
        if applied.user {
            reasons.extend(
                map_identity(unshared.identity)
                    .into_iter()
                    .map(|e| format!("uid_map: {e}")),
            );
        }
        let private_tmp = if applied.mount {
            setup_mounts(evidence, "probe", policy, &mut reasons)
        } else {
            false
        };
        let run_dir_outcome = apply_run_dir(evidence, "probe", policy, applied.mount, &mut reasons)
            .unwrap_or_else(|error| {
                reasons.push(format!("run_dir: {error}"));
                RunDirOutcome::default()
            });
        let no_new_privs =
            policy.no_new_privs && apply_no_new_privs(evidence, "probe", &mut reasons);
        let report = ProbeReport {
            degraded: !reasons.is_empty()
                || (policy.private_tmp && !private_tmp)
                || (policy.no_new_privs && !no_new_privs)
                || (policy.run_dir.is_private() && run_dir_outcome.mode != policy.run_dir.mode),
            applied: applied.applied(private_tmp, no_new_privs, run_dir_outcome),
            reason: (!reasons.is_empty()).then(|| reasons.join("; ")),
        };
        emit(&record(
            evidence,
            "sandbox-probe",
            serde_json::to_value(&report).unwrap_or_default(),
        ));
        ExitCode::SUCCESS
    }
}
