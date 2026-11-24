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
//! the mount namespace private, mounts a fresh `/proc` through a short lived
//! helper child, and optionally mounts a private `/tmp`. It sets
//! `no_new_privs`, runs the application, reaps zombies,
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
            SandboxMode::MountProc => linux::mount_proc(),
            SandboxMode::MapIds => linux::map_ids(&args),
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
    use ferrix_runtime_native::cgroup::{join_cgroup, remove_app_cgroup, setup_app_cgroup};
    use ferrix_runtime_native::channel::{ReportListener, ReportSender, WaitOutcome, send_line};
    use ferrix_runtime_native::fsview::{
        DEV_DIRS, DEV_NODES, SYSTEM_DIRS, app_data_dir, exposable_home, ferrix_config_dir,
        home_dir, mounts_under, under_root,
    };
    use ferrix_runtime_native::identity::per_app_map;
    use ferrix_runtime_native::seccomp::{
        denies_clone_namespaces, effective_denied, syscall_number,
    };
    use ferrix_runtime_native::{
        AppliedIsolation, EXEC_FAILED_STATUS, ISOLATION_UNAVAILABLE_STATUS, InitReport,
        IsolationMode, IsolationPolicy, NamespaceSet, NetworkOutcome, ProbeReport, RUN_ROOT,
        RunDirMode, RunDirOutcome, RunDirPolicy, SANDBOX_INTERNAL_STATUS, SandboxArgs,
        SandboxReady, USER_RUN_DIR, UserNamespace, emit, is_valid_socket_name, record, run_dir,
        select_namespaces, shell_status_of,
    };
    use ferrix_runtime_native::{CgroupError, CgroupOutcome, app_cgroup_name, cgroup_slice};
    use ferrix_runtime_native::{FsViewMode, FsViewOutcome};
    use ferrix_runtime_native::{IdentityOutcome, UidMode, app_subuid};
    use ferrix_runtime_native::{SeccompOutcome, SeccompPolicy, SeccompProfile};
    use rustix::io::Errno;
    use rustix::mount::{
        MountFlags, MountPropagationFlags, UnmountFlags, mount, mount_bind, mount_bind_recursive,
        mount_change, mount_move, mount_remount, unmount,
    };
    use rustix::process::{
        Pid, Signal, WaitOptions, getegid, geteuid, kill_process, set_parent_process_death_signal,
        waitpid,
    };
    use rustix::thread::{
        CapabilitySet, UnshareFlags, capabilities, clear_ambient_capability_set,
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
        net: Option<PathBuf>,
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
                net: link("net"),
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
                net: link_differs(&self.net, &before.net),
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
        if wanted.net {
            flags |= UnshareFlags::NEWNET;
        }
        flags
    }

    /// [`select_namespaces`] narrowed to what this kernel can create, with a
    /// note for anything dropped. `CONFIG_IPC_NS` exists only with SysV IPC or
    /// POSIX mqueue; a kernel without it (Android GKI) has no IPC objects to
    /// share, and asking for `CLONE_NEWIPC` fails the whole unshare (EINVAL).
    /// A kernel without `CONFIG_NET_NS` likewise cannot give the application
    /// a loopback-only view; that is reported, never hidden.
    fn kernel_namespaces(
        policy: &IsolationPolicy,
        is_root: bool,
    ) -> (NamespaceSet, Vec<&'static str>) {
        let mut wanted = select_namespaces(policy, is_root);
        let mut notes = Vec::new();
        if wanted.ipc && !Path::new("/proc/self/ns/ipc").exists() {
            wanted.ipc = false;
            notes.push("kernel has no IPC namespace (no SysV IPC or POSIX mqueue to isolate)");
        }
        if wanted.net && !Path::new("/proc/self/ns/net").exists() {
            wanted.net = false;
            notes.push("kernel has no network namespace (CONFIG_NET_NS): network not isolated");
        }
        (wanted, notes)
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
        let (mut wanted, kernel_notes) = kernel_namespaces(policy, identity.uid == 0);
        let mut notes: Vec<String> = kernel_notes.into_iter().map(str::to_owned).collect();
        let before = NsLinks::read();
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

    /// Brings `lo` up in the fresh network namespace (it starts down) with
    /// one rtnetlink `RTM_NEWLINK` request and checks the kernel's ack.
    ///
    /// Hand-rolled on purpose: the request is a fixed 32-byte message
    /// (`nlmsghdr` + `ifinfomsg`), the safe alternatives are spawning `ip`
    /// (a host tool the guest may lack) or a netlink crate stack for one
    /// constant message, and the `SIOCSIFFLAGS` ioctl needs `unsafe`. The
    /// kernel always registers loopback first in a new namespace, with the
    /// fixed index `LOOPBACK_IFINDEX` (1).
    fn bring_up_loopback() -> Result<(), String> {
        use rustix::net::netlink::SocketAddrNetlink;
        use rustix::net::{
            AddressFamily, RecvFlags, SendFlags, SocketFlags, SocketType, recv, sendto, socket_with,
        };
        const RTM_NEWLINK: u16 = 16;
        const NLM_F_REQUEST: u16 = 0x1;
        const NLM_F_ACK: u16 = 0x4;
        const NLMSG_ERROR: u16 = 2;
        const IFF_UP: u32 = 0x1;
        const LOOPBACK_IFINDEX: i32 = 1;
        const LEN: u32 = 32;

        let fd = socket_with(
            AddressFamily::NETLINK,
            SocketType::RAW,
            SocketFlags::CLOEXEC,
            None, // NETLINK_ROUTE
        )
        .map_err(|e| format!("netlink socket: {e}"))?;
        let mut msg = Vec::with_capacity(LEN as usize);
        // struct nlmsghdr
        msg.extend_from_slice(&LEN.to_ne_bytes());
        msg.extend_from_slice(&RTM_NEWLINK.to_ne_bytes());
        msg.extend_from_slice(&(NLM_F_REQUEST | NLM_F_ACK).to_ne_bytes());
        msg.extend_from_slice(&1u32.to_ne_bytes()); // seq
        msg.extend_from_slice(&0u32.to_ne_bytes()); // pid: the kernel
        // struct ifinfomsg
        msg.extend_from_slice(&[0, 0]); // AF_UNSPEC, padding
        msg.extend_from_slice(&0u16.to_ne_bytes()); // device type
        msg.extend_from_slice(&LOOPBACK_IFINDEX.to_ne_bytes());
        msg.extend_from_slice(&IFF_UP.to_ne_bytes()); // flags
        msg.extend_from_slice(&IFF_UP.to_ne_bytes()); // change mask
        sendto(&fd, &msg, SendFlags::empty(), &SocketAddrNetlink::new(0, 0))
            .map_err(|e| format!("netlink send: {e}"))?;
        let mut reply = [0u8; 256];
        let (len, _) = recv(&fd, &mut reply[..], RecvFlags::empty())
            .map_err(|e| format!("netlink recv: {e}"))?;
        if len < 20 {
            return Err(format!("netlink reply of {len} bytes"));
        }
        let kind = u16::from_ne_bytes([reply[4], reply[5]]);
        if kind != NLMSG_ERROR {
            return Err(format!("unexpected netlink reply type {kind}"));
        }
        match i32::from_ne_bytes([reply[16], reply[17], reply[18], reply[19]]) {
            0 => Ok(()),
            error => Err(format!(
                "RTM_NEWLINK lo up: {}",
                Errno::from_raw_os_error(-error)
            )),
        }
    }

    /// Interfaces of the caller's network namespace (`/proc/self/net/dev`
    /// is per task, so this is the view the application will have).
    fn network_interfaces() -> Vec<String> {
        std::fs::read_to_string("/proc/self/net/dev")
            .unwrap_or_default()
            .lines()
            .skip(2)
            .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim().to_owned()))
            .collect()
    }

    /// The loopback-only view after a private network namespace was made.
    fn setup_network(evidence: &str, app: &str, notes: &mut Vec<String>) -> NetworkOutcome {
        let loopback_up = match bring_up_loopback() {
            Ok(()) => true,
            Err(error) => {
                emit(&record(
                    evidence,
                    "sandbox-degraded",
                    json!({ "app": app, "feature": "loopback", "error": error }),
                ));
                notes.push(format!("loopback: {error}"));
                false
            }
        };
        let interfaces = network_interfaces();
        if interfaces.iter().any(|name| name != "lo") {
            notes.push(format!(
                "network namespace lists more than loopback: {}",
                interfaces.join(",")
            ));
        }
        NetworkOutcome {
            interfaces,
            loopback_up,
        }
    }

    // -- per-application uid (issue #10) -------------------------------------

    /// `--map-ids PID ID`: waits for one line on stdin (`go`, written by
    /// stage 1 once it has unshared its user namespace), then writes that
    /// process's `uid_map` and `gid_map` ([`per_app_map`]). It runs in the
    /// launcher's namespace with the launcher's `CAP_SETUID`/`CAP_SETGID`,
    /// which is what a map holding an id other than the writer's own needs.
    /// End of input without `go` (stage 1 gave up) writes nothing.
    pub fn map_ids(args: &SandboxArgs) -> ExitCode {
        let Some((pid, id)) = args.map_ids else {
            return exit_code(SANDBOX_INTERNAL_STATUS);
        };
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() || line.trim() != "go" {
            return exit_code(SANDBOX_INTERNAL_STATUS);
        }
        let identity = Identity::current();
        for (file, launcher) in [("uid_map", identity.uid), ("gid_map", identity.gid)] {
            let path = format!("/proc/{pid}/{file}");
            if let Err(e) = std::fs::write(&path, per_app_map(launcher, id)) {
                eprintln!("{path}: {e}");
                return exit_code(ISOLATION_UNAVAILABLE_STATUS);
            }
        }
        ExitCode::SUCCESS
    }

    /// Starts the map helper for this process before it unshares; `Err`
    /// when a per-app uid cannot be had here (an unprivileged launcher
    /// cannot map a foreign id).
    fn start_map_helper(policy: &IsolationPolicy, app: &str) -> Result<Option<Child>, String> {
        if policy.uid != UidMode::PerApp {
            return Ok(None);
        }
        if !geteuid().is_root() {
            return Err(
                "uid per-app needs a root launcher (CAP_SETUID to map the subuid)".to_owned(),
            );
        }
        let exe = std::env::current_exe().map_err(|e| format!("resolve sandbox binary: {e}"))?;
        Command::new(exe)
            .arg("--map-ids")
            .arg(std::process::id().to_string())
            .arg(app_subuid(app).to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map(Some)
            .map_err(|e| format!("start the id-map helper: {e}"))
    }

    /// Tells the helper to write the maps now that the namespace exists.
    fn finish_map_helper(mut helper: Child) -> Result<(), String> {
        use std::io::Write as _;
        if let Some(mut stdin) = helper.stdin.take() {
            stdin
                .write_all(b"go\n")
                .map_err(|e| format!("id-map helper: {e}"))?;
        }
        let output = helper
            .wait_with_output()
            .map_err(|e| format!("id-map helper: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "id-map helper exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    /// Writes this namespace's maps: the per-app map through the helper, or
    /// the identity map of the launcher's own ids.
    fn write_maps(identity: Identity, helper: Option<Child>) -> Vec<String> {
        match helper {
            Some(helper) => finish_map_helper(helper).err().into_iter().collect(),
            None => map_identity(identity),
        }
    }

    /// Whether this process's user namespace maps the application's subuid
    /// (read from its own `/proc` in stage 2).
    fn maps_subuid(app: &str) -> bool {
        let id = app_subuid(app).to_string();
        std::fs::read_to_string("/proc/self/uid_map").is_ok_and(|map| {
            map.lines().any(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                fields == [id.as_str(), id.as_str(), "1"]
            })
        })
    }

    /// Hands the application's data directory to its subuid (owner-only).
    fn own_for(path: &Path, id: u32) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt as _;
        std::os::unix::fs::chown(path, Some(id), Some(id))
            .map_err(|e| io_error("chown", path, e))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| io_error("chmod", path, e))
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
        // Connect the readiness report now: the runtime's abstract listener
        // lives in the launcher's network namespace, which a private
        // network namespace would otherwise put out of reach.
        let report = args
            .report
            .as_deref()
            .and_then(|name| match ReportSender::connect(name) {
                Ok(sender) => Some(sender),
                Err(e) => {
                    notes.push(format!("readiness report channel: {e}"));
                    None
                }
            });
        // The group is joined before anything is unshared: the launcher's
        // credentials can still write the hierarchy, and stage 2 plus every
        // application process inherit it from here on.
        let cgroup = match apply_cgroup(evidence, app, &args.policy, &mut notes) {
            Ok(outcome) => outcome,
            Err(reason) => return fail(evidence, app, reason, ISOLATION_UNAVAILABLE_STATUS),
        };
        // A per-app uid's map is written from outside the new namespace.
        let helper = match start_map_helper(&args.policy, app) {
            Ok(helper) => helper,
            Err(reason) if args.policy.mode == IsolationMode::Required => {
                return fail(evidence, app, reason, ISOLATION_UNAVAILABLE_STATUS);
            }
            Err(reason) => {
                notes.push(format!("uid: {reason}"));
                None
            }
        };
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
            let per_app = helper.is_some();
            for error in write_maps(unshared.identity, helper) {
                if per_app && args.policy.mode == IsolationMode::Required {
                    return fail(
                        evidence,
                        app,
                        format!("uid per-app: {error}"),
                        ISOLATION_UNAVAILABLE_STATUS,
                    );
                }
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
        let network = if applied.net {
            setup_network(evidence, app, &mut notes)
        } else {
            NetworkOutcome::default()
        };
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
        let mut fs_view = FsViewOutcome::default();
        let mut seccomp = SeccompOutcome::default();
        let mut identity = IdentityOutcome::default();
        match listener
            .map(|l| l.wait_for_line(&mut child, INIT_REPORT_TIMEOUT, &mut forward_pending))
        {
            Some(Ok(line)) => match serde_json::from_str::<InitReport>(&line) {
                Ok(report) => {
                    private_tmp = report.private_tmp;
                    no_new_privs = report.no_new_privs;
                    run_dir_outcome = report.run_dir;
                    fs_view = report.fs_view;
                    seccomp = report.seccomp;
                    identity = report.identity;
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
            || (args.policy.fs_view.is_minimal() && fs_view.mode != FsViewMode::Minimal)
            || (args.policy.seccomp.is_confined() && !seccomp.applied())
            || (args.policy.uid == UidMode::PerApp && identity.uid.is_none())
            || (args.policy.resources.cgroup && !cgroup.applied())
            || (applied.net && !network.loopback_up)
            || !notes.is_empty();
        let ready = SandboxReady {
            app: app.to_owned(),
            init_pid,
            namespaces: applied,
            private_tmp,
            no_new_privs,
            run_dir: run_dir_outcome,
            cgroup,
            network,
            fs_view,
            seccomp,
            identity,
            degraded,
            reason: (!notes.is_empty()).then(|| notes.join("; ")),
        };
        emit(&record(
            evidence,
            "sandbox-ready",
            serde_json::to_value(&ready).unwrap_or_default(),
        ));
        if let Some(sender) = report {
            let _ = sender.send(&serde_json::to_string(&ready).unwrap_or_default());
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

    /// Creates the launch's group under the Ferrix slice and moves this
    /// process into it. A hierarchy or controller the system lacks is a
    /// degraded launch with a `sandbox-degraded{feature:"cgroup"}` record (as
    /// with a kernel without IPC namespaces); any other failure is `Err`
    /// under `Required` (fail closed) and degraded under `BestEffort`.
    fn apply_cgroup(
        evidence: &str,
        app: &str,
        policy: &IsolationPolicy,
        notes: &mut Vec<String>,
    ) -> Result<CgroupOutcome, String> {
        if !policy.resources.cgroup {
            return Ok(CgroupOutcome::default());
        }
        let name = app_cgroup_name(app, std::process::id());
        let result =
            setup_app_cgroup(&cgroup_slice(), &name, &policy.resources).and_then(|outcome| {
                let group = PathBuf::from(outcome.path.clone().unwrap_or_default());
                join_cgroup(&group, std::process::id())
                    .map(|()| outcome)
                    .map_err(|error| {
                        let _ = std::fs::remove_dir(&group);
                        CgroupError::Failed(error)
                    })
            });
        let error = match result {
            Ok(outcome) => return Ok(outcome),
            Err(CgroupError::Failed(reason)) if policy.mode == IsolationMode::Required => {
                return Err(format!("cgroup: {reason}"));
            }
            Err(error) => error,
        };
        emit(&record(
            evidence,
            "sandbox-degraded",
            json!({ "app": app, "feature": "cgroup", "error": error.message() }),
        ));
        notes.push(format!("cgroup: {}", error.message()));
        Ok(CgroupOutcome::default())
    }

    /// The probe's cgroup check: create a throwaway group with the limits,
    /// never join it, remove it again.
    fn probe_cgroup(policy: &IsolationPolicy, reasons: &mut Vec<String>) -> CgroupOutcome {
        if !policy.resources.cgroup {
            return CgroupOutcome::default();
        }
        let name = app_cgroup_name("probe", std::process::id());
        match setup_app_cgroup(&cgroup_slice(), &name, &policy.resources) {
            Ok(outcome) => {
                let group = PathBuf::from(outcome.path.clone().unwrap_or_default());
                if let Err(error) = remove_app_cgroup(&group, Duration::from_secs(1)) {
                    reasons.push(format!("cgroup: {error}"));
                }
                outcome
            }
            Err(error) => {
                reasons.push(format!("cgroup: {}", error.message()));
                CgroupOutcome::default()
            }
        }
    }

    // -- filesystem view (issue #10) --------------------------------------

    fn env_var(key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    const MOUNTINFO: &str = "/proc/self/mountinfo";

    /// `/proc/self/mountinfo` opened by [`keep_mountinfo`].
    static KEPT_MOUNTINFO: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> =
        std::sync::OnceLock::new();

    /// The probe stays outside the PID namespace whose fresh procfs it
    /// mounts over `/proc`, so `/proc/self` stops resolving for it. It opens
    /// its mountinfo first; the file is bound to the caller's mount
    /// namespace and regenerated on every read from offset 0. Stage 2 (PID 1
    /// inside the namespace) reads the path directly.
    fn keep_mountinfo() {
        if let Ok(file) = std::fs::File::open(MOUNTINFO) {
            let _ = KEPT_MOUNTINFO.set(std::sync::Mutex::new(file));
        }
    }

    fn read_mountinfo() -> Result<String, String> {
        use std::io::{Read as _, Seek as _, SeekFrom};
        match std::fs::read_to_string(MOUNTINFO) {
            Ok(text) => Ok(text),
            Err(error) => {
                let kept = KEPT_MOUNTINFO
                    .get()
                    .ok_or_else(|| io_error("read", Path::new(MOUNTINFO), &error))?;
                let mut file = kept
                    .lock()
                    .map_err(|_| "mountinfo lock poisoned".to_owned())?;
                let mut text = String::new();
                file.seek(SeekFrom::Start(0))
                    .and_then(|_| file.read_to_string(&mut text))
                    .map_err(|e| io_error("read kept", Path::new(MOUNTINFO), e))?;
                Ok(text)
            }
        }
    }

    fn io_error(what: &str, path: &Path, e: impl std::fmt::Display) -> String {
        format!("{what} {}: {e}", path.display())
    }

    /// Remounts the bind at `target` and every mount below it read-only,
    /// keeping each mount's own `nosuid`/`nodev`/`noexec`/atime flags: the
    /// kernel refuses a remount that would clear a flag locked by a user
    /// namespace, and the submounts (`/etc/hosts` in a container,
    /// `/sys/fs/cgroup`) need it as much as the top.
    fn remount_read_only(target: &Path) -> Result<(), String> {
        let mountinfo = read_mountinfo()?;
        let entries = mounts_under(&mountinfo, target);
        if entries.is_empty() {
            return Err(format!("{} is not a mount point", target.display()));
        }
        for entry in entries {
            let mut flags = MountFlags::BIND | MountFlags::RDONLY;
            for (option, flag) in [
                ("nosuid", MountFlags::NOSUID),
                ("nodev", MountFlags::NODEV),
                ("noexec", MountFlags::NOEXEC),
                ("noatime", MountFlags::NOATIME),
                ("nodiratime", MountFlags::NODIRATIME),
                ("relatime", MountFlags::RELATIME),
            ] {
                if entry.has(option) {
                    flags |= flag;
                }
            }
            mount_remount(&entry.mount_point, flags, "")
                .map_err(|e| io_error("remount read-only", &entry.mount_point, e))?;
        }
        Ok(())
    }

    fn bind_dir(source: &Path, dest: &Path, read_only: bool) -> Result<(), String> {
        std::fs::create_dir_all(dest).map_err(|e| io_error("create", dest, e))?;
        mount_bind_recursive(source, dest).map_err(|e| io_error("bind", source, e))?;
        if read_only {
            remount_read_only(dest)?;
        }
        Ok(())
    }

    fn bind_file(source: &Path, dest: &Path) -> Result<(), String> {
        std::fs::File::create(dest).map_err(|e| io_error("create", dest, e))?;
        mount_bind(source, dest).map_err(|e| io_error("bind", source, e))
    }

    fn tmpfs(dest: &Path, flags: MountFlags, options: &std::ffi::CStr) -> Result<(), String> {
        std::fs::create_dir_all(dest).map_err(|e| io_error("create", dest, e))?;
        mount("tmpfs", dest, "tmpfs", flags, options)
            .map_err(|e| io_error("mount tmpfs on", dest, e))
    }

    /// A minimal `/dev`: the standard character devices bound from the host,
    /// a private devpts instance (so an application never sees another's
    /// ptys), a private `/dev/shm`, the GPU nodes when present, and the usual
    /// symlinks. Block devices, input devices and everything else stay out.
    fn build_dev(root: &Path) -> Result<(), String> {
        let dev = root.join("dev");
        tmpfs(
            &dev,
            MountFlags::NOSUID | MountFlags::NOEXEC,
            c"mode=0755,size=64k",
        )?;
        for node in DEV_NODES {
            let source = Path::new("/dev").join(node);
            if source.exists() {
                bind_file(&source, &dev.join(node))?;
            }
        }
        let pts = dev.join("pts");
        std::fs::create_dir(&pts).map_err(|e| io_error("create", &pts, e))?;
        if mount(
            "devpts",
            &pts,
            "devpts",
            MountFlags::NOSUID | MountFlags::NOEXEC,
            c"newinstance,ptmxmode=0666,mode=0620",
        )
        .is_err()
        {
            mount_bind_recursive("/dev/pts", &pts).map_err(|e| io_error("bind", &pts, e))?;
        }
        tmpfs(
            &dev.join("shm"),
            MountFlags::NOSUID | MountFlags::NODEV,
            c"mode=1777,size=64m",
        )?;
        for dir in DEV_DIRS {
            let source = Path::new("/dev").join(dir);
            if source.is_dir() {
                bind_dir(&source, &dev.join(dir), false)?;
            }
        }
        for (link, target) in [
            ("ptmx", "pts/ptmx"),
            ("fd", "/proc/self/fd"),
            ("stdin", "/proc/self/fd/0"),
            ("stdout", "/proc/self/fd/1"),
            ("stderr", "/proc/self/fd/2"),
        ] {
            std::os::unix::fs::symlink(target, dev.join(link))
                .map_err(|e| io_error("symlink", &dev.join(link), e))?;
        }
        Ok(())
    }

    /// The paths a minimal root is built from, resolved before anything is
    /// mounted.
    struct ViewPlan {
        data_dir: Option<PathBuf>,
        home: Option<PathBuf>,
        expose_home: bool,
        config: Option<PathBuf>,
        /// The per-app subuid that owns the stand-in home.
        owner: Option<u32>,
    }

    fn populate_root(root: &Path, plan: &ViewPlan) -> Result<FsViewOutcome, String> {
        let mut outcome = FsViewOutcome {
            mode: FsViewMode::Minimal,
            ..FsViewOutcome::default()
        };
        for dir in SYSTEM_DIRS {
            let source = Path::new(dir);
            let Ok(meta) = std::fs::symlink_metadata(source) else {
                continue;
            };
            let dest = under_root(root, source);
            if meta.file_type().is_symlink() {
                let target =
                    std::fs::read_link(source).map_err(|e| io_error("readlink", source, e))?;
                std::os::unix::fs::symlink(&target, &dest)
                    .map_err(|e| io_error("symlink", &dest, e))?;
            } else if meta.is_dir() {
                bind_dir(source, &dest, true)?;
                outcome.read_only.push((*dir).to_owned());
            }
        }
        build_dev(root)?;
        // The sandbox's own fresh /proc, its /run view, and /tmp (only the
        // /tmp mount itself: the staging root lives below it).
        bind_dir(Path::new("/proc"), &root.join("proc"), false)?;
        bind_dir(Path::new("/sys"), &root.join("sys"), true)?;
        outcome.read_only.push("/sys".to_owned());
        bind_dir(Path::new(RUN_ROOT), &root.join("run"), false)?;
        let tmp = root.join("tmp");
        std::fs::create_dir(&tmp).map_err(|e| io_error("create", &tmp, e))?;
        mount_bind("/tmp", &tmp).map_err(|e| io_error("bind", Path::new("/tmp"), e))?;
        // A socket directory outside /run (nested runs, tests) stays visible.
        let sockets = run_dir();
        if !sockets.starts_with(RUN_ROOT) && sockets.is_dir() {
            bind_dir(&sockets, &under_root(root, &sockets), false)?;
        }
        if let Some(home) = &plan.home {
            if plan.expose_home {
                bind_dir(home, &under_root(root, home), false)?;
                outcome.home = Some(home.display().to_string());
            } else {
                // An empty, writable stand-in so `$HOME` still exists.
                let dest = under_root(root, home);
                std::fs::create_dir_all(&dest).map_err(|e| io_error("create", &dest, e))?;
                if let Some(id) = plan.owner {
                    own_for(&dest, id)?;
                }
            }
        }
        if let Some(data) = &plan.data_dir
            && !(plan.expose_home && plan.home.as_ref().is_some_and(|h| data.starts_with(h)))
        {
            let dest = under_root(root, data);
            std::fs::create_dir_all(&dest).map_err(|e| io_error("create", &dest, e))?;
            mount_bind(data, &dest).map_err(|e| io_error("bind", data, e))?;
        }
        outcome.data_dir = plan.data_dir.as_ref().map(|d| d.display().to_string());
        if let Some(config) = &plan.config
            && config.is_dir()
            && !(plan.expose_home && plan.home.as_ref().is_some_and(|h| config.starts_with(h)))
        {
            bind_dir(config, &under_root(root, config), true)?;
            outcome.read_only.push(config.display().to_string());
        }
        Ok(outcome)
    }

    /// Enters `root`: `pivot_root(".", ".")` then detaching the old root
    /// (pivot_root(2)); an initramfs root cannot be pivoted away from
    /// (EINVAL), so there the new root is moved over `/` and entered with
    /// `chroot` (what container runtimes do on a ramfs root).
    fn enter_root(root: &Path) -> Result<&'static str, String> {
        rustix::process::chdir(root).map_err(|e| io_error("chdir", root, e))?;
        match rustix::process::pivot_root(".", ".") {
            Ok(()) => {
                unmount(".", UnmountFlags::DETACH)
                    .map_err(|e| format!("detach the old root: {e}"))?;
                rustix::process::chdir("/").map_err(|e| format!("chdir /: {e}"))?;
                Ok("pivot_root")
            }
            Err(Errno::INVAL) => {
                mount_move(".", "/").map_err(|e| format!("move the new root over /: {e}"))?;
                rustix::process::chroot(".").map_err(|e| format!("chroot: {e}"))?;
                rustix::process::chdir("/").map_err(|e| format!("chdir /: {e}"))?;
                Ok("move-chroot")
            }
            Err(e) => Err(format!("pivot_root: {e}")),
        }
    }

    /// Creates the application's data directory (owner-only) on the host
    /// before any view is built; `None` when no absolute data root exists.
    fn prepare_data_dir(app: &str, reasons: &mut Vec<String>) -> Option<PathBuf> {
        use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
        let dir = app_data_dir(env_var, app)?;
        if let Err(e) = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
        {
            reasons.push(io_error("data directory", &dir, e));
            return None;
        }
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        Some(dir)
    }

    /// Builds the minimal root on a tmpfs at a staging directory in `/tmp`
    /// (the application's own tmpfs with `private_tmp`) and enters it.
    fn build_minimal_root(
        policy: &IsolationPolicy,
        data_dir: Option<&Path>,
        owner: Option<u32>,
        reasons: &mut Vec<String>,
    ) -> Result<FsViewOutcome, String> {
        let home = home_dir(env_var);
        let expose_home = match (&home, policy.fs_view.home) {
            (_, false) => false,
            (None, true) => {
                reasons.push("files-home: HOME is not an absolute path".to_owned());
                false
            }
            (Some(home), true) => match exposable_home(home) {
                Ok(()) if home.is_dir() => true,
                Ok(()) => {
                    reasons.push(format!("files-home: {} is not a directory", home.display()));
                    false
                }
                Err(error) => {
                    reasons.push(format!("files-home: {error}"));
                    false
                }
            },
        };
        let plan = ViewPlan {
            data_dir: data_dir.map(Path::to_path_buf),
            home: home.filter(|h| exposable_home(h).is_ok()),
            expose_home,
            config: ferrix_config_dir(env_var),
            owner,
        };
        let staging = PathBuf::from("/tmp").join(format!(".ferrix-root-{}", std::process::id()));
        std::fs::create_dir(&staging).map_err(|e| io_error("create", &staging, e))?;
        let built = mount(
            "tmpfs",
            &staging,
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV,
            c"mode=0755,size=16m",
        )
        .map_err(|e| io_error("mount tmpfs on", &staging, e))
        .and_then(|()| populate_root(&staging, &plan));
        let mut outcome = match built {
            Ok(outcome) => outcome,
            Err(error) => {
                let _ = unmount(&staging, UnmountFlags::DETACH);
                let _ = std::fs::remove_dir(&staging);
                return Err(error);
            }
        };
        let method = enter_root(&staging)?;
        // The staging directory is now an empty directory inside the view's
        // /tmp (its mount belonged to the detached old tree).
        let _ = std::fs::remove_dir(&staging);
        outcome.root = Some(method.to_owned());
        Ok(outcome)
    }

    /// Applies `fs_view` in stage 2 (or the probe). `Err` carries the
    /// reason; the caller decides whether that is fatal.
    fn apply_fs_view(
        evidence: &str,
        app: &str,
        policy: &IsolationPolicy,
        mount_namespace: bool,
        data_dir: Option<&Path>,
        owner: Option<u32>,
        reasons: &mut Vec<String>,
    ) -> Result<FsViewOutcome, String> {
        let inherit = || FsViewOutcome {
            data_dir: data_dir.map(|d| d.display().to_string()),
            ..FsViewOutcome::default()
        };
        if !policy.fs_view.is_minimal() {
            return Ok(inherit());
        }
        if !mount_namespace {
            reasons.push("fs_view needs the mount namespace".to_owned());
            return Ok(inherit());
        }
        build_minimal_root(policy, data_dir, owner, reasons).map_err(|error| {
            emit(&record(
                evidence,
                "sandbox-mount-failed",
                json!({ "app": app, "target": "/", "error": error }),
            ));
            error
        })
    }

    // -- seccomp (issue #10) ----------------------------------------------

    /// Installs the launch's system-call filter on this thread; stage 2's
    /// application (and every descendant) inherits it across `fork` and
    /// `execve`. Two filters are stacked: the deny list (plus `clone` with
    /// any namespace flag) returns `EPERM`, and `clone3` returns `ENOSYS` so
    /// libc falls back to `clone`, whose flags the first filter can read.
    /// `seccompiler::apply_filter` also sets `no_new_privs`, which seccomp
    /// requires of an unprivileged caller.
    fn install_seccomp(policy: &SeccompPolicy) -> Result<SeccompOutcome, String> {
        use std::collections::BTreeMap;

        use seccompiler::{
            BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition,
            SeccompFilter, SeccompRule, TargetArch, apply_filter,
        };

        if !policy.is_confined() {
            return Ok(SeccompOutcome::default());
        }
        let arch =
            TargetArch::try_from(std::env::consts::ARCH).map_err(|e| format!("seccomp: {e}"))?;
        let compile = |rules: BTreeMap<i64, Vec<SeccompRule>>, errno: i32| {
            SeccompFilter::new(
                rules,
                SeccompAction::Allow,
                SeccompAction::Errno(errno.unsigned_abs()),
                arch,
            )
            .and_then(BpfProgram::try_from)
            .map_err(|e| format!("seccomp filter: {e}"))
        };

        let denied = effective_denied(policy);
        let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
        for name in &denied {
            let number =
                syscall_number(name).ok_or_else(|| format!("seccomp: no number for {name}"))?;
            rules.insert(number, Vec::new());
        }
        let mut count = denied.len();
        if denies_clone_namespaces(policy) {
            let namespace_flags = [
                libc::CLONE_NEWNS,
                libc::CLONE_NEWCGROUP,
                libc::CLONE_NEWUTS,
                libc::CLONE_NEWIPC,
                libc::CLONE_NEWUSER,
                libc::CLONE_NEWPID,
                libc::CLONE_NEWNET,
                libc::CLONE_NEWTIME,
            ];
            let mut clone_rules = Vec::new();
            for flag in namespace_flags {
                let flag = u64::from(flag.unsigned_abs());
                let condition = SeccompCondition::new(
                    0,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::MaskedEq(flag),
                    flag,
                )
                .and_then(|condition| SeccompRule::new(vec![condition]))
                .map_err(|e| format!("seccomp clone rule: {e}"))?;
                clone_rules.push(condition);
            }
            rules.insert(libc::SYS_clone, clone_rules);
            count += 1;
        }
        let deny = compile(rules, libc::EPERM)?;
        let clone3 = compile(
            BTreeMap::from([(libc::SYS_clone3, Vec::new())]),
            libc::ENOSYS,
        )?;
        apply_filter(&clone3).map_err(|e| format!("install seccomp (clone3): {e}"))?;
        apply_filter(&deny).map_err(|e| format!("install seccomp: {e}"))?;
        Ok(SeccompOutcome {
            profile: Some(SeccompProfile::Default),
            denied: count,
            allowed: policy.allow.clone(),
        })
    }

    fn apply_seccomp(
        evidence: &str,
        app: &str,
        policy: &IsolationPolicy,
        reasons: &mut Vec<String>,
    ) -> SeccompOutcome {
        match install_seccomp(&policy.seccomp) {
            Ok(outcome) => outcome,
            Err(error) => {
                emit(&record(
                    evidence,
                    "sandbox-degraded",
                    json!({ "app": app, "feature": "seccomp", "error": error }),
                ));
                reasons.push(error);
                SeccompOutcome::default()
            }
        }
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
        // The init process is PID 1 in the child PID namespace. On the
        // hosted and local privileged container kernels, its own procfs
        // mount is refused (EPERM), while a child in the same mount/PID
        // namespaces can mount it. The helper exits before any app starts.
        if let Err(e) = mount_proc_in_child() {
            mount_failed("/proc", e);
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

    fn mount_proc_in_child() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| format!("resolve sandbox binary: {e}"))?;
        let output = Command::new(exe)
            .arg("--mount-proc")
            .output()
            .map_err(|e| format!("start procfs mount helper: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "procfs mount helper exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    pub fn mount_proc() -> ExitCode {
        match mount(
            "proc",
            "/proc",
            "proc",
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            None::<&CStr>,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("mount /proc: {e}");
                exit_code(ISOLATION_UNAVAILABLE_STATUS)
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
        configure_capability_in_ambient_set(CapabilitySet::SYS_ADMIN, true)
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
        let data_dir = prepare_data_dir(app, &mut reasons);
        // The subuid exists only when stage 1's map really holds it.
        let subuid =
            (args.policy.uid == UidMode::PerApp && maps_subuid(app)).then(|| app_subuid(app));
        if args.policy.uid == UidMode::PerApp && subuid.is_none() {
            reasons.push("uid per-app: the user namespace does not map the subuid".to_owned());
        }
        if let (Some(id), Some(dir)) = (subuid, &data_dir)
            && let Err(error) = own_for(dir, id)
        {
            reasons.push(format!("uid per-app: {error}"));
        }
        let identity = IdentityOutcome {
            mode: if subuid.is_some() {
                UidMode::PerApp
            } else {
                UidMode::Launcher
            },
            uid: subuid,
            gid: subuid,
        };
        let fs_view = match apply_fs_view(
            evidence,
            app,
            &args.policy,
            applied.mount,
            data_dir.as_deref(),
            subuid,
            &mut reasons,
        ) {
            Ok(outcome) => outcome,
            Err(error) if args.policy.mode == IsolationMode::Required => {
                // Never start the application with the launcher's whole
                // filesystem when a minimal view was required.
                emit(&record(
                    evidence,
                    "sandbox-failed",
                    json!({ "app": app, "reason": format!("required fs_view: {error}") }),
                ));
                return exit_code(ISOLATION_UNAVAILABLE_STATUS);
            }
            Err(error) => {
                reasons.push(format!("fs_view: {error}"));
                FsViewOutcome {
                    data_dir: data_dir.as_ref().map(|d| d.display().to_string()),
                    ..FsViewOutcome::default()
                }
            }
        };
        drop_inherited_capabilities(evidence, app, &mut reasons);
        let no_new_privs =
            args.policy.no_new_privs && apply_no_new_privs(evidence, app, &mut reasons);
        // Last, after every mount: from here on this process and the
        // application cannot mount, pivot, unshare or ptrace.
        let seccomp = apply_seccomp(evidence, app, &args.policy, &mut reasons);

        if args.policy.mode == IsolationMode::Required {
            let (wanted, _) = kernel_namespaces(&args.policy, geteuid().is_root());
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
            if args.policy.fs_view.is_minimal() && fs_view.mode != FsViewMode::Minimal {
                unmet.push("fs_view");
            }
            if args.policy.seccomp.is_confined() && !seccomp.applied() {
                unmet.push("seccomp");
            }
            if args.policy.uid == UidMode::PerApp && identity.uid.is_none() {
                unmet.push("uid");
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
                fs_view: fs_view.clone(),
                seccomp,
                identity: identity.clone(),
                reasons: reasons.clone(),
            };
            let _ = send_line(name, &serde_json::to_string(&report).unwrap_or_default());
        }

        let program = &args.exec[0];
        let mut command = Command::new(program);
        command.args(&args.exec[1..]);
        if let Some(dir) = &fs_view.data_dir {
            command.env(ferrix_app_api::APP_DATA_DIR_ENV, dir);
        }
        if let Some(id) = identity.uid {
            // std drops the supplementary groups (setgroups) before setgid
            // and setuid in the child; nothing of root's survives the exec.
            use std::os::unix::process::CommandExt as _;
            command.uid(id).gid(id);
        }
        let child = match command.spawn() {
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
        let cgroup = probe_cgroup(policy, &mut reasons);
        let helper = start_map_helper(policy, "probe").unwrap_or_else(|reason| {
            reasons.push(format!("uid: {reason}"));
            None
        });
        let per_app = helper.is_some();
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
                write_maps(unshared.identity, helper)
                    .into_iter()
                    .map(|e| format!("uid_map: {e}")),
            );
        }
        // Before the fresh /proc covers the launcher's (see keep_mountinfo).
        keep_mountinfo();
        let network = if applied.net {
            setup_network(evidence, "probe", &mut reasons)
        } else {
            NetworkOutcome::default()
        };
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
        // No data directory: the probe must not leave one behind.
        let fs_view = apply_fs_view(
            evidence,
            "probe",
            policy,
            applied.mount,
            None,
            None,
            &mut reasons,
        )
        .unwrap_or_else(|error| {
            reasons.push(format!("fs_view: {error}"));
            FsViewOutcome::default()
        });
        let no_new_privs =
            policy.no_new_privs && apply_no_new_privs(evidence, "probe", &mut reasons);
        let seccomp = apply_seccomp(evidence, "probe", policy, &mut reasons);
        let report = ProbeReport {
            degraded: !reasons.is_empty()
                || (policy.private_tmp && !private_tmp)
                || (policy.no_new_privs && !no_new_privs)
                || (policy.run_dir.is_private() && run_dir_outcome.mode != policy.run_dir.mode)
                || (policy.resources.cgroup && !cgroup.applied())
                || (policy.fs_view.is_minimal() && fs_view.mode != FsViewMode::Minimal)
                || (policy.seccomp.is_confined() && !seccomp.applied())
                || (policy.uid == UidMode::PerApp && !per_app),
            applied: AppliedIsolation {
                identity: IdentityOutcome {
                    mode: if per_app {
                        UidMode::PerApp
                    } else {
                        UidMode::Launcher
                    },
                    uid: per_app.then(|| app_subuid("probe")),
                    gid: per_app.then(|| app_subuid("probe")),
                },
                cgroup,
                network,
                fs_view,
                seccomp,
                ..applied.applied(private_tmp, no_new_privs, run_dir_outcome)
            },
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
