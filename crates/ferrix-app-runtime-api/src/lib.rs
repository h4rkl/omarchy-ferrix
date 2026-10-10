//! Internal contract between `ferrix_appd` and replaceable runtime adapters.
//!
//! This is deliberately separate from `ferrix-app-api`: applications and shell
//! clients need identities, descriptors, and lifecycle state, but must not
//! inherit process-launch or runtime-implementation details.

use std::collections::BTreeSet;
use std::time::Duration;

use ferrix_app_api::{AppCapability, AppDescriptor, AppError, ExitReason, RuntimeKind};
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
}
