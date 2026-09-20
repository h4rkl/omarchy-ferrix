//! Internal boundary between [`crate::AndroidRuntime`] and the Android-on-Linux
//! runtime that actually hosts the container. Implemented by
//! [`crate::WaydroidCli`] (production) and [`crate::testing::FakeAndroidHost`]
//! (tests). Nothing outside this crate sees the trait.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Coarse state of the compatibility runtime as a whole (not of one app).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum RuntimeStatus {
    /// The runtime is not installed on this host (binary missing).
    Absent { reason: String },
    /// Installed and initialised, but no session/container is running.
    Stopped,
    /// A session start was requested and is not yet reported running.
    Starting,
    /// Session and container are running. `session_pid` is the host pid of
    /// the session supervisor when this adapter started it.
    Running {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_pid: Option<u32>,
    },
    /// Installed but unusable (not initialised, images missing, container
    /// failed, or the status command itself failed).
    Failed { reason: String },
}

impl RuntimeStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, RuntimeStatus::Running { .. })
    }
}

/// Result of asking the host to start an Android application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchedApp {
    /// Host-namespace pid of the Android application process, when the host
    /// can see it (Linux `/proc` scan). `None` when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Host-namespace pid of the container process that holds the Wayland
    /// connection to the compositor. This, not `pid`, is what the compositor
    /// reports in `SurfaceMapped`, so surface association must use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_client_pid: Option<u32>,
}

/// Typed failures of the host; the runtime maps them onto `AppError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostError {
    /// The runtime is not installed here.
    Absent(String),
    /// The runtime exists but the session/container could not be used.
    SessionFailed(String),
    /// The package is not installed in the Android side.
    NotInstalled,
    /// The command ran but reported an error.
    CommandFailed(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Absent(reason) => write!(f, "runtime absent: {reason}"),
            HostError::SessionFailed(reason) => write!(f, "session-failed: {reason}"),
            HostError::NotInstalled => f.write_str("not-installed"),
            HostError::CommandFailed(reason) => write!(f, "command failed: {reason}"),
        }
    }
}

impl std::error::Error for HostError {}

/// What the adapter needs from an Android-on-Linux runtime. Every method is
/// synchronous and bounded; implementations shell out to the runtime's
/// maintained CLI and never speak Binder themselves.
pub trait AndroidHost: Send + Sync {
    /// Stable host name for diagnostics (`waydroid`, `fake`).
    fn name(&self) -> &'static str;

    /// Current runtime state. Must not start anything.
    fn status(&self) -> RuntimeStatus;

    /// Makes sure a session/container is running, starting one when needed
    /// with `env` (Wayland display and runtime directory of the launching
    /// session) and waiting up to `timeout`. Returns the resulting status,
    /// which is `Running` on success.
    fn ensure_session(
        &self,
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<RuntimeStatus, HostError>;

    /// Installs an already verified APK. Never downloads.
    fn install(&self, apk: &Path) -> Result<(), HostError>;

    /// Whether `package` is installed in the Android side.
    fn is_installed(&self, package: &str) -> Result<bool, HostError>;

    /// Starts `package` (its default activity, or `activity` when given).
    fn launch(&self, package: &str, activity: Option<&str>) -> Result<LaunchedApp, HostError>;

    /// Whether a process of `package` exists right now.
    fn is_running(&self, package: &str) -> Result<bool, HostError>;

    /// Force-stops `package`.
    fn stop(&self, package: &str) -> Result<(), HostError>;

    /// Host pid of the package's main process, when visible.
    fn app_pid(&self, package: &str) -> Option<u32>;

    /// Host pid of the container's Wayland client, when visible.
    fn surface_client_pid(&self) -> Option<u32> {
        None
    }
}
