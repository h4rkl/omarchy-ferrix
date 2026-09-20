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

pub trait RunningApp: Send {
    fn pid(&self) -> Option<u32>;
    /// Runtime-owned evidence, deliberately absent from the public protocol.
    fn diagnostics(&self) -> Value;
    fn try_wait(&mut self) -> Result<Option<ExitReason>, AppError>;
    fn wait(&mut self) -> Result<ExitReason, AppError>;
    fn terminate(&mut self, grace: Duration) -> Result<ExitReason, AppError>;
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
