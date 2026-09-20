//! Linux-native launch and containment configuration.
//!
//! These types stay behind the native runtime boundary and never appear in
//! the public application protocol.

use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IsolationPolicy {
    pub pid_namespace: bool,
    pub mount_namespace: bool,
    pub ipc_namespace: bool,
    pub uts_namespace: bool,
    pub user_namespace: UserNamespace,
    pub private_tmp: bool,
    pub no_new_privs: bool,
    pub mode: IsolationMode,
    /// Socket-directory visibility; `inherit` unless the manifest opts in.
    pub run_dir: RunDirPolicy,
}

impl Default for IsolationPolicy {
    fn default() -> Self {
        Self {
            pid_namespace: true,
            mount_namespace: true,
            ipc_namespace: true,
            uts_namespace: true,
            user_namespace: UserNamespace::Auto,
            private_tmp: true,
            no_new_privs: true,
            mode: IsolationMode::Required,
            run_dir: RunDirPolicy::default(),
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
            user_namespace: UserNamespace::Never,
            private_tmp: false,
            no_new_privs: false,
            mode: IsolationMode::BestEffort,
            run_dir: RunDirPolicy::default(),
        }
    }

    pub fn requests_any_namespace(&self) -> bool {
        self.pid_namespace
            || self.mount_namespace
            || self.ipc_namespace
            || self.uts_namespace
            || self.user_namespace == UserNamespace::Always
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppliedIsolation {
    pub pid_namespace: bool,
    pub mount_namespace: bool,
    pub ipc_namespace: bool,
    pub uts_namespace: bool,
    pub user_namespace: bool,
    pub private_tmp: bool,
    pub no_new_privs: bool,
    pub run_dir: RunDirOutcome,
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
            || requested.run_dir.is_private();
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
