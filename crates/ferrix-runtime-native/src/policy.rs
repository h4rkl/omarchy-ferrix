//! Linux-native launch and containment configuration.
//!
//! These types stay behind the native runtime boundary and never appear in
//! the public application protocol.

use serde::{Deserialize, Serialize};

use crate::{CgroupOutcome, IdentityOutcome, SeccompOutcome, SeccompPolicy, UidMode};

/// The application's filesystem view ([`crate::fsview`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FsViewMode {
    /// The launcher's filesystem, with the private `/proc`, `/tmp` and
    /// socket-directory views on top.
    #[default]
    Inherit,
    /// A minimal root: read-only system directories, a minimal `/dev`, the
    /// application's data directory, and the home folder only with
    /// `files-home`.
    Minimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FsViewPolicy {
    pub mode: FsViewMode,
    /// Bind the user's home folder read-write. Under `ferrix_appd` the
    /// runtime derives it from the `files-home` grant (`crate::with_grants`);
    /// a manifest value is replaced.
    pub home: bool,
}

impl Default for FsViewPolicy {
    fn default() -> Self {
        Self {
            mode: FsViewMode::Minimal,
            home: false,
        }
    }
}

impl FsViewPolicy {
    pub fn inherit() -> Self {
        Self {
            mode: FsViewMode::Inherit,
            home: false,
        }
    }

    pub fn is_minimal(&self) -> bool {
        self.mode == FsViewMode::Minimal
    }
}

/// What the sandbox did to the filesystem view (`mode: inherit` and nothing
/// else when no root was built).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FsViewOutcome {
    pub mode: FsViewMode,
    /// How the root was entered: `pivot_root`, or `move-chroot` on an
    /// initramfs root, which the kernel cannot pivot away from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// The writable data directory ([`ferrix_app_api::APP_DATA_DIR_ENV`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,
    /// The home folder, when `files-home` exposed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    /// Host directories bound read-only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub read_only: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserNamespace {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationMode {
    #[default]
    Required,
    BestEffort,
}

/// How the application sees the platform socket directory (`/run/ferrix`,
/// or `FERRIX_RUN_DIR`). See ADR-004 §4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunDirMode {
    /// The shared directory stays visible: every platform socket is
    /// reachable (no socket-level enforcement in the sandbox).
    #[default]
    Inherit,
    /// A private tmpfs replaces the socket directory; only the sockets
    /// listed in [`RunDirPolicy::sockets`] are bind-mounted into it. The rest
    /// of `/run` (D-Bus, udev, `/run/user`) is unchanged.
    Private,
    /// A private tmpfs replaces all of `/run`: the filtered socket directory
    /// is placed back at its path and `/run/user` (Wayland) is bind-mounted
    /// back; everything else under `/run` (including the D-Bus system bus
    /// socket) disappears from the application's view.
    PrivateRun,
}

/// Socket visibility for one launch. Under `ferrix_appd` the `sockets` list
/// is **derived from the application's effective grants** by the runtime
/// ([`crate::sockets_for_grants`]); a list written into a manifest is
/// replaced, never merged, so a descriptor cannot widen its own access.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunDirPolicy {
    pub mode: RunDirMode,
    /// File names (no directory part) under the socket directory.
    pub sockets: Vec<String>,
}

impl RunDirPolicy {
    pub fn is_private(&self) -> bool {
        self.mode != RunDirMode::Inherit
    }
}

/// Default `memory.max` of an application group: 1 GiB.
pub const DEFAULT_MEMORY_MAX: u64 = 1 << 30;
/// Default `pids.max` of an application group.
pub const DEFAULT_PIDS_MAX: u64 = 1024;
/// Default `cpu.weight` (the kernel's own default, so applications share the
/// CPU evenly until a policy or a boost says otherwise).
pub const DEFAULT_CPU_WEIGHT: u16 = 100;

/// Per-application cgroup v2 limits ([`crate::cgroup`]). Each `None` limit
/// leaves that controller alone; `cgroup: false` creates no group at all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourcePolicy {
    pub cgroup: bool,
    /// `memory.max` in bytes (`memory.oom.group` is set with it).
    pub memory_max: Option<u64>,
    pub pids_max: Option<u64>,
    /// `cpu.weight`, 1..=10000.
    pub cpu_weight: Option<u16>,
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            cgroup: true,
            memory_max: Some(DEFAULT_MEMORY_MAX),
            pids_max: Some(DEFAULT_PIDS_MAX),
            cpu_weight: Some(DEFAULT_CPU_WEIGHT),
        }
    }
}

impl ResourcePolicy {
    pub fn none() -> Self {
        Self {
            cgroup: false,
            memory_max: None,
            pids_max: None,
            cpu_weight: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IsolationPolicy {
    pub pid_namespace: bool,
    pub mount_namespace: bool,
    pub ipc_namespace: bool,
    pub uts_namespace: bool,
    /// A private network namespace holding only `lo`. Under `ferrix_appd`
    /// the runtime derives it from the effective grants: on unless the
    /// application holds `network` (`crate::with_grants`); a manifest
    /// value is replaced, so a descriptor cannot widen its own access.
    pub net_namespace: bool,
    pub user_namespace: UserNamespace,
    pub private_tmp: bool,
    pub no_new_privs: bool,
    pub mode: IsolationMode,
    /// Socket-directory visibility; `inherit` unless the manifest opts in.
    pub run_dir: RunDirPolicy,
    /// Per-application cgroup and its limits.
    pub resources: ResourcePolicy,
    /// Filesystem view; a minimal root unless the manifest asks to inherit.
    pub fs_view: FsViewPolicy,
    /// System-call filter ([`crate::seccomp`]); the default profile unless
    /// the launch configuration relaxes it.
    pub seccomp: SeccompPolicy,
    /// Identity of the application process ([`crate::identity`]).
    pub uid: UidMode,
}

impl Default for IsolationPolicy {
    fn default() -> Self {
        Self {
            pid_namespace: true,
            mount_namespace: true,
            ipc_namespace: true,
            uts_namespace: true,
            net_namespace: true,
            user_namespace: UserNamespace::Auto,
            private_tmp: true,
            no_new_privs: true,
            mode: IsolationMode::Required,
            run_dir: RunDirPolicy::default(),
            resources: ResourcePolicy::default(),
            fs_view: FsViewPolicy::default(),
            seccomp: SeccompPolicy::default(),
            uid: UidMode::Launcher,
        }
    }
}

impl IsolationPolicy {
    pub fn none() -> Self {
        Self {
            pid_namespace: false,
            mount_namespace: false,
            ipc_namespace: false,
            uts_namespace: false,
            net_namespace: false,
            user_namespace: UserNamespace::Never,
            private_tmp: false,
            no_new_privs: false,
            mode: IsolationMode::BestEffort,
            run_dir: RunDirPolicy::default(),
            resources: ResourcePolicy::none(),
            fs_view: FsViewPolicy::inherit(),
            seccomp: SeccompPolicy::unconfined(),
            uid: UidMode::Launcher,
        }
    }

    pub fn requests_any_namespace(&self) -> bool {
        self.pid_namespace
            || self.mount_namespace
            || self.ipc_namespace
            || self.uts_namespace
            || self.net_namespace
            || self.user_namespace == UserNamespace::Always
            || self.uid == UidMode::PerApp
    }
}

/// What the sandbox did to the socket directory: the mode actually applied
/// (`inherit` when nothing was mounted), the sockets bound into the private
/// view, and listed sockets that did not exist at launch (a daemon not yet
/// running; the application simply cannot reach it).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunDirOutcome {
    pub mode: RunDirMode,
    pub sockets: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

/// The network view inside a private network namespace: the interfaces
/// the kernel lists there (`["lo"]`) and whether loopback was brought up.
/// Empty when the application shares the launcher's network.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkOutcome {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    pub loopback_up: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppliedIsolation {
    pub pid_namespace: bool,
    pub mount_namespace: bool,
    pub ipc_namespace: bool,
    pub uts_namespace: bool,
    pub net_namespace: bool,
    pub user_namespace: bool,
    pub private_tmp: bool,
    pub no_new_privs: bool,
    pub run_dir: RunDirOutcome,
    /// Loopback-only network view (`{}` unless `net_namespace`).
    pub network: NetworkOutcome,
    /// Filesystem view (`{mode: inherit}` unless a minimal root was built).
    pub fs_view: FsViewOutcome,
    /// The installed system-call filter.
    pub seccomp: SeccompOutcome,
    /// The application's identity (`uid`/`gid` with `per-app`).
    pub identity: IdentityOutcome,
    /// The application's cgroup (`{}` when none was created).
    pub cgroup: CgroupOutcome,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationReport {
    pub requested: IsolationPolicy,
    pub applied: AppliedIsolation,
    pub degraded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl IsolationReport {
    pub fn unisolated(requested: IsolationPolicy, reason: impl Into<String>) -> Self {
        let degraded = requested.requests_any_namespace()
            || requested.private_tmp
            || requested.no_new_privs
            || requested.run_dir.is_private()
            || requested.resources.cgroup
            || requested.fs_view.is_minimal()
            || requested.seccomp.is_confined();
        Self {
            requested,
            applied: AppliedIsolation::default(),
            degraded,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NativeLaunch {
    pub exec: Vec<String>,
    pub isolation: IsolationPolicy,
}
