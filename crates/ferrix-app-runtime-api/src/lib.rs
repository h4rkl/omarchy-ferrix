//! Internal contract between `ferrix_appd` and replaceable runtime adapters.
//!
//! This is deliberately separate from `ferrix-app-api`: applications and shell
//! clients need identities, descriptors, and lifecycle state, but must not
//! inherit process-launch or runtime-implementation details.

use std::collections::BTreeSet;
use std::time::Duration;

use ferrix_app_api::{
    AppCapability, AppDescriptor, AppError, AppIconBitmap, ExitReason, RuntimeKind,
};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchContext {
    pub instance: u32,
    pub env: Vec<(String, String)>,
    pub evidence: String,
    /// The application's effective capabilities (`declared ∩ granted`), as
    /// computed by `ferrix_appd` from the grants policy. A runtime derives
    /// what the application may reach (for the native runtime: which service
    /// sockets stay visible) from this set, never from the descriptor.
    pub granted: BTreeSet<AppCapability>,
}

/// How `ferrix_appd` decides which compositor toplevels belong to a running
/// instance.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SurfaceMatch {
    /// The instance's own process tree: the reported pid or any descendant.
    /// The kernel-reported client pid is the whole proof.
    #[default]
    ProcessTree,
    /// A shared runtime host draws the windows of several applications from
    /// the same client process(es) (one toplevel per Android task). A
    /// toplevel belongs to this instance only when its kernel-reported client
    /// pid is one of `client_pids` (reported by the runtime adapter, the
    /// trust anchor) **and** its xdg `app_id` equals `app_id` exactly. The
    /// `app_id` alone is never trusted, and these pids never resolve to the
    /// instance by themselves (no identity, no ancestry match).
    SharedHost {
        client_pids: BTreeSet<u32>,
        app_id: String,
    },
}

impl SurfaceMatch {
    /// Whether this is a shared-host binding.
    pub fn is_shared_host(&self) -> bool {
        matches!(self, SurfaceMatch::SharedHost { .. })
    }

    /// Whether a toplevel of `pid` claiming `app_id` belongs to the instance
    /// under a shared-host binding. Always `false` for `ProcessTree`, which
    /// is resolved by ancestry instead.
    pub fn claims(&self, pid: u32, app_id: Option<&str>) -> bool {
        match self {
            SurfaceMatch::ProcessTree => false,
            SurfaceMatch::SharedHost {
                client_pids,
                app_id: expected,
            } => client_pids.contains(&pid) && app_id == Some(expected.as_str()),
        }
    }

    /// Whether `pid` is one of the shared host's client pids.
    pub fn hosts(&self, pid: u32) -> bool {
        match self {
            SurfaceMatch::ProcessTree => false,
            SurfaceMatch::SharedHost { client_pids, .. } => client_pids.contains(&pid),
        }
    }
}

pub trait RunningApp: Send {
    fn pid(&self) -> Option<u32>;
    /// Runtime-owned evidence, deliberately absent from the public protocol.
    fn diagnostics(&self) -> Value;
    fn try_wait(&mut self) -> Result<Option<ExitReason>, AppError>;
    fn wait(&mut self) -> Result<ExitReason, AppError>;
    fn terminate(&mut self, grace: Duration) -> Result<ExitReason, AppError>;
    /// How compositor surfaces map to this instance. Cheap: implementations
    /// return cached data (refreshed while polling in `try_wait`).
    fn surface_match(&self) -> SurfaceMatch {
        SurfaceMatch::ProcessTree
    }
    /// Asks the runtime to show the application again when it is running but
    /// has no window (an Android task that was closed or never appeared).
    /// Returns `Ok(true)` when the runtime re-issued a launch, `Ok(false)`
    /// when the runtime has no such notion (native processes own their
    /// windows).
    fn reopen(&mut self) -> Result<bool, AppError> {
        Ok(false)
    }
}

/// An application a runtime reports by itself: a package the user installed
/// into it (an APK). `descriptor` is public catalog data
/// ([`ferrix_app_api::AppOrigin::Installed`], untrusted, no capabilities);
/// `launch` is the same opaque configuration a checked-in descriptor of this
/// runtime carries.
#[derive(Clone, Debug, PartialEq)]
pub struct ProvidedApp {
    pub descriptor: AppDescriptor,
    pub launch: Value,
}

/// Adapter for one runtime kind. `config` is private manifest data interpreted
/// only by that adapter, keeping `ferrix_appd` and public clients runtime-neutral.
pub trait Runtime: Send + Sync {
    fn kind(&self) -> RuntimeKind;
    fn launch(
        &self,
        app: &AppDescriptor,
        config: &Value,
        context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError>;

    /// Launchable applications installed in this runtime, already filtered
    /// of the runtime's own system surfaces. `None` when the runtime has no
    /// such notion (native processes are described by checked-in files
    /// only). An `Err` means the listing is unavailable right now (runtime
    /// stopped or failed); the caller keeps what it had. May block (bounded);
    /// never call it while holding a lock other callers need.
    fn installed_apps(&self) -> Option<Result<Vec<ProvidedApp>, AppError>> {
        None
    }

    /// Runtime-native identity of a launch configuration (for Android: the
    /// package), used to avoid listing an installed package twice when a
    /// checked-in descriptor already covers it. `None` when not applicable.
    fn app_key(&self, _config: &Value) -> Option<String> {
        None
    }

    /// The xdg `app_id` the runtime gives this configuration's windows when
    /// they come from a shared host ([`SurfaceMatch::SharedHost`]); only
    /// meaningful together with [`Runtime::shared_host_pids`].
    fn window_app_id(&self, _config: &Value) -> Option<String> {
        None
    }

    /// The launcher icon of an app this runtime reports by itself
    /// ([`Runtime::installed_apps`]), decoded and bounded by the adapter
    /// (the runtime's own image files are untrusted input). `None` when
    /// there is none or it is unusable; the home then draws the letter
    /// glyph. May block on bounded file I/O; never call it while holding a
    /// lock other callers need.
    fn app_icon(&self, _config: &Value) -> Option<AppIconBitmap> {
        None
    }

    /// Host client pids the runtime draws shared-host windows from right
    /// now, whether or not any instance runs (the trust anchor for adopting
    /// a window the runtime opened by itself). May scan `/proc`.
    fn shared_host_pids(&self) -> BTreeSet<u32> {
        BTreeSet::new()
    }

    /// Supervises an application the runtime already started on its own
    /// (an Android task opened by an intent from another app) without
    /// starting it again. Runtimes that cannot do this refuse.
    fn attach(
        &self,
        app: &AppDescriptor,
        _config: &Value,
        _context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        Err(AppError::RuntimeUnsupported {
            runtime: app.runtime,
        })
    }
}
