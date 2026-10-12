//! Internal contract between the networking service and its backends.
//!
//! A backend owns the truth about one network stack (a deterministic mock,
//! or a production link-manager adapter) and exposes it through synchronous
//! operations plus an event channel. The service folds [`BackendEvent`]s
//! into the public [`ferrix_network_api::NetworkState`] and does the
//! authorization; a backend never sees callers. Applications never link this
//! crate.
//!
//! Hardware addresses are hashed inside the backend; every type crossing
//! this boundary is already one from `ferrix-network-api`.

use std::fmt;
use std::sync::mpsc::Sender;

use ferrix_network_api::{
    ApId, NetworkError, NetworkEvent, NetworkState, ProfileId, SecurityType, Ssid,
};

/// What a backend pushes to the service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendEvent {
    /// A delta to fold into the current state and forward to subscribers.
    Changed(NetworkEvent),
    /// The underlying stack went away (crash, service stop). Control is
    /// unavailable until [`BackendEvent::Restarted`].
    Lost { reason: String },
    /// The underlying stack is back and was re-enumerated: `state` replaces
    /// the service's state wholesale and is published as
    /// `service-restarted`.
    Restarted { state: NetworkState },
    /// Ordering barrier used by deterministic control/evidence workflows.
    /// It carries no product state and is never forwarded to applications.
    Barrier { token: u64 },
}

impl BackendEvent {
    pub fn name(&self) -> &'static str {
        match self {
            BackendEvent::Changed(event) => event.name(),
            BackendEvent::Lost { .. } => "backend-lost",
            BackendEvent::Restarted { .. } => "service-restarted",
            BackendEvent::Barrier { .. } => "barrier",
        }
    }
}

/// Typed backend failures. Converted to [`NetworkError`] at the service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    /// A failure the public contract already names.
    Network(NetworkError),
    /// The backend cannot do this at all; nothing imitates success.
    Unsupported { feature: String },
    /// The underlying stack is not reachable right now.
    Unavailable { reason: String },
}

impl BackendError {
    pub fn unsupported(feature: impl Into<String>) -> Self {
        BackendError::Unsupported {
            feature: feature.into(),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        BackendError::Unavailable {
            reason: reason.into(),
        }
    }
}

impl From<NetworkError> for BackendError {
    fn from(error: NetworkError) -> Self {
        BackendError::Network(error)
    }
}

impl From<BackendError> for NetworkError {
    fn from(error: BackendError) -> Self {
        match error {
            BackendError::Network(error) => error,
            BackendError::Unsupported { feature } => NetworkError::Unsupported { feature },
            BackendError::Unavailable { reason } => NetworkError::Backend { reason },
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Network(error) => write!(f, "{error}"),
            BackendError::Unsupported { feature } => write!(f, "{feature} is unsupported"),
            BackendError::Unavailable { reason } => write!(f, "backend unavailable: {reason}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// A network backend. All operations are synchronous requests; progress
/// (association, addressing, scan results) arrives through the subscribed
/// channel as [`BackendEvent`]s.
pub trait NetworkBackend: Send + Sync {
    /// Short name for the ready record (`mock`, `networkmanager`).
    fn name(&self) -> &'static str;

    /// Current full state.
    fn snapshot(&self) -> Result<NetworkState, BackendError>;

    fn set_radio(&self, enabled: bool) -> Result<(), BackendError>;

    /// Starts a scan; completion arrives as `scan-changed` events.
    fn scan(&self) -> Result<(), BackendError>;

    /// Joins a visible access point. The passphrase is used for this attempt
    /// (and the resulting saved profile) and must never be logged.
    fn connect(&self, ap: &ApId, passphrase: Option<&str>) -> Result<(), BackendError>;

    /// Joins the network named `ssid`, which need not be visible (a hidden
    /// network broadcasts no name). `security` is the caller's choice and
    /// the service has already checked the request with
    /// `ferrix_network_api::validate_hidden_join`. A successful join saves
    /// a profile like [`connect`](Self::connect); the passphrase must never
    /// be logged.
    fn join_hidden(
        &self,
        ssid: &Ssid,
        security: SecurityType,
        passphrase: Option<&str>,
    ) -> Result<(), BackendError>;

    fn connect_profile(&self, profile: &ProfileId) -> Result<(), BackendError>;

    fn disconnect(&self) -> Result<(), BackendError>;

    fn forget(&self, profile: &ProfileId) -> Result<(), BackendError>;

    /// Registers a channel that receives every event from now on.
    fn subscribe(&self, sink: Sender<BackendEvent>);

    /// Enqueues a token after every event emitted before this call. Backends
    /// that cannot provide this test/evidence facility may return unsupported.
    fn barrier(&self, _token: u64) -> Result<(), BackendError> {
        Err(BackendError::unsupported("barrier"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_errors_map_to_public_errors() {
        assert_eq!(
            NetworkError::from(BackendError::unsupported("scan")),
            NetworkError::Unsupported {
                feature: "scan".into()
            }
        );
        assert_eq!(
            NetworkError::from(BackendError::unavailable("bus down")),
            NetworkError::Backend {
                reason: "bus down".into()
            }
        );
        assert_eq!(
            NetworkError::from(BackendError::from(NetworkError::NoAdapter)),
            NetworkError::NoAdapter
        );
        assert_eq!(
            BackendEvent::Lost { reason: "x".into() }.name(),
            "backend-lost"
        );
        assert_eq!(
            BackendEvent::Restarted {
                state: NetworkState::default()
            }
            .name(),
            "service-restarted"
        );
    }
}
