//! Private Android compatibility runtime adapter for `ferrix-appd`.
//!
//! [`AndroidRuntime`] implements [`ferrix_app_runtime_api::Runtime`] for
//! [`ferrix_app_api::RuntimeKind::Android`]. It never speaks Binder and never
//! exposes the underlying runtime's CLI to applications: everything the
//! Android side needs is reached through the small internal [`AndroidHost`]
//! trait, implemented once for the maintained Waydroid CLI
//! ([`WaydroidCli`]) and once as an in-memory double for tests
//! ([`testing::FakeAndroidHost`]).
//!
//! Process picture of one launch (Waydroid host):
//!
//! ```text
//! ferrix_appd -> AndroidRuntime::launch
//!                  |- waydroid status            (absent? stopped? running?)
//!                  |- waydroid session start     (detached, once per boot)
//!                  |- waydroid app list          (is the package installed?)
//!                  |- waydroid app launch <pkg>  (or `am start -n pkg/activity`)
//!                  '- /proc/*/cmdline scan       (host pid of the Android process)
//! ```
//!
//! The Android package and activity live only in the descriptor's
//! daemon-private `launch` object and in the runtime's diagnostics; they are
//! never returned over `ferrix-app-protocol`. See `docs/android-runtime.md`
//! for the evaluation, the guest integration plan, and the known limitations
//! (task-window association in particular).

mod config;
mod host;
pub mod icon;
mod installed;
mod runtime;
pub mod testing;
mod waydroid;

pub use config::{AndroidLaunch, ApkSource, validate_activity, validate_package};
pub use host::{AndroidHost, HostError, InstalledPackage, LaunchedApp, RuntimeStatus};
pub use installed::{
    HIDDEN_PACKAGES, display_label, installed_app_id, installed_descriptor, is_hidden_package,
};
pub use runtime::{AndroidRuntime, AndroidRuntimeOptions, DEFAULT_SESSION_TIMEOUT};
pub use waydroid::{
    WaydroidCli, WaydroidOptions, WaydroidStatus, parse_app_entries, parse_app_list, parse_status,
    pid_from_cmdlines,
};
