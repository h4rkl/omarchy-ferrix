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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedIsolation {
    pub pid_namespace: bool,
    pub mount_namespace: bool,
    pub ipc_namespace: bool,
    pub uts_namespace: bool,
    pub user_namespace: bool,
    pub private_tmp: bool,
    pub no_new_privs: bool,
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
        let degraded =
            requested.requests_any_namespace() || requested.private_tmp || requested.no_new_privs;
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
