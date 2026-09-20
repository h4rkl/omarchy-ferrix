//! `ferrix-networkd` library: the networking service (ADR-002) with
//! per-request authorization (ADR-004) and evidence records (ADR-006).
//!
//! The library works with any `ferrix_network_adapter_api::NetworkBackend`
//! and any [`identity::Identity`]; only the binary composes a concrete
//! backend and the appd identity link. Everything here is host-testable
//! with the doubles in [`testing`].

pub mod cli;
pub mod control;
pub mod identity;
pub mod records;
pub mod server;
pub mod service;
pub mod testing;

pub use ferrix_network_api::{DEFAULT_NETWORKD_SOCKET, NETWORKD_SOCKET_ENV, SCHEMA_VERSION};

use std::path::Path;
use std::sync::Arc;

use ferrix_mock_control::{Observer, Resolved};

use crate::records::Recorder;

/// Reports the mock-control channel through the daemon's records.
pub struct ControlObserver {
    recorder: Recorder,
}

impl ControlObserver {
    pub fn new(recorder: Recorder) -> Arc<Self> {
        Arc::new(Self { recorder })
    }
}

impl Observer for ControlObserver {
    fn ready(&self, resolved: &Resolved) {
        self.recorder
            .mock_control("ready", Some(resolved.path()), Some(resolved.kind()));
    }

    fn failed(&self, stage: &str, error: &str) {
        self.recorder
            .mock_control("failed", None::<&Path>, Some(&format!("{stage}: {error}")));
    }
}
