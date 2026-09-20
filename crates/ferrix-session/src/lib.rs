//! Session/bootstrap wiring.
//!
//! This crate owns target selection: it loads the versioned target profile,
//! picks the adapter set for the selected target id, and hands product code a
//! [`HardwareServices`] bundle of `hardware-api` trait objects plus the
//! target's capability matrix. Product crates (`mobile-core`, shells, apps)
//! depend on this crate and `hardware-api` only; they never name an adapter.
//!
//! Today the only target is the generic QEMU virtual phone
//! (`qemu-aarch64`), served by `hardware-emulator`. A future target package
//! adds a new arm to [`select_adapters`], nowhere else.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use hardware_api::{
    Audio, Bluetooth, CapabilityMatrix, Display, EventSource, HardwareEvent, HardwareState, Input,
    Lifecycle, MockControl, Network, Power, Telephony,
};
use target_profile::{Profile, ProfileError};

/// Profile path inside the guest image.
pub const GUEST_PROFILE_PATH: &str = "/etc/ferrix/profile.toml";
/// Profile path when run from the repository root (host tests, nested shell).
pub const REPO_PROFILE_PATH: &str = "targets/qemu-aarch64/profile.toml";
/// Environment override for the profile path.
pub const PROFILE_ENV: &str = "FERRIX_QEMU_PROFILE";
/// Environment override for the target id.
pub const TARGET_ENV: &str = "FERRIX_TARGET";
/// The default and only supported target.
pub const DEFAULT_TARGET_ID: &str = hardware_emulator::TARGET_ID;

#[derive(Clone, Debug, Default)]
pub struct BootstrapOptions {
    pub profile_path: Option<PathBuf>,
    pub target_id: Option<String>,
}

impl BootstrapOptions {
    /// Reads `FERRIX_QEMU_PROFILE` and `FERRIX_TARGET`.
    pub fn from_env() -> Self {
        Self {
            profile_path: std::env::var_os(PROFILE_ENV).map(PathBuf::from),
            target_id: std::env::var(TARGET_ENV).ok().filter(|v| !v.is_empty()),
        }
    }

    pub fn with_profile_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.profile_path = Some(path.into());
        self
    }
}

/// The guest profile when it exists, otherwise the repository copy.
pub fn default_profile_path() -> PathBuf {
    if Path::new(GUEST_PROFILE_PATH).is_file() {
        PathBuf::from(GUEST_PROFILE_PATH)
    } else {
        PathBuf::from(REPO_PROFILE_PATH)
    }
}

/// Typed handles to every capability of the selected target.
#[derive(Clone)]
pub struct HardwareServices {
    pub power: Arc<dyn Power>,
    pub display: Arc<dyn Display>,
    pub input: Arc<dyn Input>,
    pub network: Arc<dyn Network>,
    pub bluetooth: Arc<dyn Bluetooth>,
    pub audio: Arc<dyn Audio>,
    pub telephony: Arc<dyn Telephony>,
    pub lifecycle: Arc<dyn Lifecycle>,
    pub events: Arc<dyn EventSource>,
    /// Present only on mocked targets.
    pub mock_control: Option<Arc<dyn MockControl>>,
}

impl HardwareServices {
    pub fn snapshot(&self) -> HardwareState {
        HardwareState {
            power: self.power.power_state(),
            display: self.display.display_state(),
            input: self.input.input_state(),
            network: self.network.network_state(),
            bluetooth: self.bluetooth.bluetooth_state(),
            audio: self.audio.audio_state(),
            telephony: self.telephony.telephony_state(),
            lifecycle: self.lifecycle.lifecycle_state(),
        }
    }

    /// Registers a new subscriber and returns its receiver.
    pub fn subscribe(&self) -> Receiver<HardwareEvent> {
        let (tx, rx) = mpsc::channel();
        self.events.subscribe(tx);
        rx
    }
}

pub struct Session {
    pub target_id: String,
    pub profile_path: PathBuf,
    pub profile: Profile,
    pub capabilities: CapabilityMatrix,
    pub services: HardwareServices,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    Profile(ProfileError),
    UnknownTarget(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Profile(e) => write!(f, "{e}"),
            SessionError::UnknownTarget(id) => write!(
                f,
                "unknown target id {id:?}; the only supported target is {DEFAULT_TARGET_ID}"
            ),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ProfileError> for SessionError {
    fn from(e: ProfileError) -> Self {
        SessionError::Profile(e)
    }
}

/// Loads the profile and constructs the adapter set for the target.
pub fn bootstrap(options: BootstrapOptions) -> Result<Session, SessionError> {
    let profile_path = options.profile_path.unwrap_or_else(default_profile_path);
    let target_id = options
        .target_id
        .unwrap_or_else(|| DEFAULT_TARGET_ID.to_owned());
    let profile = Profile::load(&profile_path)?;
    let (capabilities, services) = select_adapters(&target_id, &profile)?;
    Ok(Session {
        target_id,
        profile_path,
        profile,
        capabilities,
        services,
    })
}

/// Constructs a session from an already-parsed profile (host tests).
pub fn bootstrap_with_profile(target_id: &str, profile: Profile) -> Result<Session, SessionError> {
    let (capabilities, services) = select_adapters(target_id, &profile)?;
    Ok(Session {
        target_id: target_id.to_owned(),
        profile_path: PathBuf::new(),
        profile,
        capabilities,
        services,
    })
}

/// The one place that maps a target id to an adapter implementation.
fn select_adapters(
    target_id: &str,
    profile: &Profile,
) -> Result<(CapabilityMatrix, HardwareServices), SessionError> {
    match target_id {
        hardware_emulator::TARGET_ID => {
            let emulator = Arc::new(hardware_emulator::Emulator::from_profile(profile));
            let services = HardwareServices {
                power: emulator.clone(),
                display: emulator.clone(),
                input: emulator.clone(),
                network: emulator.clone(),
                bluetooth: emulator.clone(),
                audio: emulator.clone(),
                telephony: emulator.clone(),
                lifecycle: emulator.clone(),
                events: emulator.clone(),
                mock_control: Some(emulator),
            };
            Ok((hardware_emulator::Emulator::capability_matrix(), services))
        }
        other => Err(SessionError::UnknownTarget(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hardware_api::{Capability, CapabilityStatus, MockCommand};

    fn repo_profile() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        ))
    }

    #[test]
    fn bootstraps_the_qemu_target_from_the_checked_in_profile() {
        let session = bootstrap(BootstrapOptions::default().with_profile_path(repo_profile()))
            .expect("bootstrap");
        assert_eq!(session.target_id, "qemu-aarch64");
        assert_eq!(
            session.capabilities.status(Capability::Power),
            CapabilityStatus::Mocked
        );
        let state = session.services.snapshot();
        assert_eq!(state.power.battery_percent, 75);
        assert!(!state.power.charging);
        assert!(session.services.mock_control.is_some());
    }

    #[test]
    fn unknown_target_is_rejected() {
        let err = bootstrap(BootstrapOptions {
            profile_path: Some(repo_profile()),
            target_id: Some("solana-seeker".to_owned()),
        })
        .err()
        .expect("must fail");
        assert_eq!(err, SessionError::UnknownTarget("solana-seeker".to_owned()));
    }

    #[test]
    fn events_flow_through_the_session_bundle() {
        let session = bootstrap(BootstrapOptions::default().with_profile_path(repo_profile()))
            .expect("bootstrap");
        let rx = session.services.subscribe();
        let control = session.services.mock_control.clone().unwrap();
        control
            .apply(MockCommand::SetCharging { charging: true })
            .unwrap();
        let event = rx.try_recv().expect("event delivered");
        assert_eq!(event.name(), "power-changed");
        assert!(session.services.power.power_state().charging);
    }
}
