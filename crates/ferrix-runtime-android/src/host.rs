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

/// One package the Android side reports (`waydroid app list` block).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub package: String,
    /// Android's label for the package (`Name:`), untrusted display text.
    pub name: String,
    /// Intent categories of its launch activity (`categories:`).
    pub categories: Vec<String>,
}

impl InstalledPackage {
    /// Whether the package has a launcher entry (`android.intent.category.LAUNCHER`).
    pub fn is_launchable(&self) -> bool {
        self.categories
            .iter()
            .any(|c| c == "android.intent.category.LAUNCHER")
    }
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

    /// Every package the Android side lists with a launch activity. An
    /// empty listing is reported as an error (a running Android always has
    /// launchable packages; the CLI prints nothing while the session is
    /// stopped), so callers never mistake "unavailable" for "uninstalled".
    fn installed_packages(&self) -> Result<Vec<InstalledPackage>, HostError>;

    /// Whether Android inside the running container finished booting
    /// (`sys.boot_completed`). `None` when the host cannot tell; callers
    /// then rely on [`AndroidHost::status`] and the listing alone. While
    /// Android boots, PackageManager may answer with a partial or empty
    /// launcher set, so a listing taken then is not authoritative.
    fn android_booted(&self) -> Option<bool> {
        None
    }

    /// The package's launcher icon, decoded and bounded
    /// ([`crate::icon`]), or `None` when the host has none or it is
    /// unusable. Bounded file I/O only; never starts anything.
    fn launcher_icon(&self, _package: &str) -> Option<ferrix_app_api::AppIconBitmap> {
        None
    }

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

    /// Every host pid that may hold a Wayland connection for the container
    /// (multi-window mode opens one connection per Android task, possibly
    /// from more than one process). These pids are the trust anchor for
    /// associating task windows; ascending order.
    fn surface_client_pids(&self) -> Vec<u32> {
        self.surface_client_pid().into_iter().collect()
    }

    /// The xdg `app_id` the runtime gives the task window(s) of `package`.
    /// Only meaningful together with [`AndroidHost::surface_client_pids`]:
    /// the compositor-side match requires both.
    fn window_app_id(&self, package: &str) -> String;
}
