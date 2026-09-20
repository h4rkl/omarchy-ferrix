//! The single strict parser for a target's versioned profile
//! (`targets/<target-id>/profile.toml`).
//!
//! Every consumer (adapters, the session layer, reference surfaces, tests)
//! goes through this crate so that the schema is enforced in exactly one
//! place. Unknown keys, missing sections, an unexpected `schema_version`, and
//! out-of-range values are all hard errors. Values describe emulator-test
//! configuration, not physical-hardware facts.

use std::fmt;
use std::path::Path;

use hardware_api::{AudioRoute, ModemRegistration, Orientation, SimState, WifiState};
use serde::{Deserialize, Serialize};

/// The only schema version this crate accepts.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    pub display: DisplaySection,
    pub input: InputSection,
    pub power: PowerSection,
    pub connectivity: ConnectivitySection,
    pub audio: AudioSection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySection {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub orientation: Orientation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputSection {
    pub touch: bool,
    pub rotation: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerSection {
    pub battery_percent: u8,
    pub charging: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectivitySection {
    pub wifi: WifiState,
    pub bluetooth_present: bool,
    pub modem: ModemRegistration,
    pub sim: SimState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSection {
    pub route: AudioRoute,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// The file could not be read.
    Read { path: String, reason: String },
    /// TOML syntax or schema violation (unknown key, missing field, wrong type).
    Schema(String),
    /// The document parsed but a value is out of range.
    Invalid(String),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::Read { path, reason } => {
                write!(f, "cannot read profile {path}: {reason}")
            }
            ProfileError::Schema(reason) => write!(f, "invalid profile: {reason}"),
            ProfileError::Invalid(reason) => write!(f, "invalid profile value: {reason}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// Parses and validates a profile document.
    pub fn parse(input: &str) -> Result<Self, ProfileError> {
        let profile: Profile = toml::from_str(input)
            .map_err(|e| ProfileError::Schema(e.message().trim().to_owned()))?;
        profile.validate()?;
        Ok(profile)
    }

    /// Reads, parses, and validates the profile at `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProfileError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| ProfileError::Read {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        Self::parse(&text)
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ProfileError::Invalid(format!(
                "unsupported schema_version {} (expected {SCHEMA_VERSION})",
                self.schema_version
            )));
        }
        if self.name.trim().is_empty() {
            return Err(ProfileError::Invalid("name must not be empty".to_owned()));
        }
        if self.display.width == 0 || self.display.height == 0 {
            return Err(ProfileError::Invalid(format!(
                "display width and height must be > 0 (got {}x{})",
                self.display.width, self.display.height
            )));
        }
        if !(self.display.scale.is_finite() && self.display.scale > 0.0) {
            return Err(ProfileError::Invalid(format!(
                "display scale must be a positive number (got {})",
                self.display.scale
            )));
        }
        if self.power.battery_percent > 100 {
            return Err(ProfileError::Invalid(format!(
                "power.battery_percent must be 0..=100 (got {})",
                self.power.battery_percent
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
schema_version = 1
name = "emulator-dev"

[display]
width = 1080
height = 2340
scale = 1.0
orientation = "portrait"

[input]
touch = true
rotation = true

[power]
battery_percent = 75
charging = false

[connectivity]
wifi = "connected"
bluetooth_present = true
modem = "registered"
sim = "present"

[audio]
route = "speaker"
"#;

    #[test]
    fn fixture_parses() {
        let p = Profile::parse(FIXTURE).expect("fixture parses");
        assert_eq!(p.schema_version, 1);
        assert_eq!(p.name, "emulator-dev");
        assert_eq!((p.display.width, p.display.height), (1080, 2340));
        assert_eq!(p.display.orientation, Orientation::Portrait);
        assert!(p.input.touch && p.input.rotation);
        assert_eq!(p.power.battery_percent, 75);
        assert!(!p.power.charging);
        assert_eq!(p.connectivity.wifi, WifiState::Connected);
        assert!(p.connectivity.bluetooth_present);
        assert_eq!(p.connectivity.modem, ModemRegistration::Registered);
        assert_eq!(p.connectivity.sim, SimState::Present);
        assert_eq!(p.audio.route, AudioRoute::Speaker);
    }

    #[test]
    fn checked_in_profile_parses() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        );
        let p = Profile::load(path).expect("checked-in profile parses");
        assert_eq!(p.name, "emulator-dev");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let text = FIXTURE.replace("charging = false", "charging = false\nvoltage = 3.7");
        let err = Profile::parse(&text).expect_err("unknown key must fail");
        assert!(matches!(err, ProfileError::Schema(_)), "{err}");
        assert!(err.to_string().contains("voltage"), "{err}");
    }

    #[test]
    fn unknown_schema_is_rejected() {
        let text = FIXTURE.replace("schema_version = 1", "schema_version = 2");
        let err = Profile::parse(&text).expect_err("schema 2 must fail");
        assert!(
            err.to_string().contains("unsupported schema_version 2"),
            "{err}"
        );
    }

    #[test]
    fn missing_section_is_rejected() {
        let text = FIXTURE.replace("[power]\nbattery_percent = 75\ncharging = false\n", "");
        let err = Profile::parse(&text).expect_err("missing power must fail");
        assert!(matches!(err, ProfileError::Schema(_)), "{err}");
        assert!(err.to_string().contains("power"), "{err}");
    }

    #[test]
    fn battery_out_of_range_is_rejected() {
        let text = FIXTURE.replace("battery_percent = 75", "battery_percent = 101");
        let err = Profile::parse(&text).expect_err("101% must fail");
        assert!(err.to_string().contains("0..=100"), "{err}");
    }

    #[test]
    fn unknown_enum_value_is_rejected() {
        let text = FIXTURE.replace("wifi = \"connected\"", "wifi = \"maybe\"");
        let err = Profile::parse(&text).expect_err("unknown wifi state must fail");
        assert!(matches!(err, ProfileError::Schema(_)), "{err}");
    }

    #[test]
    fn zero_geometry_is_rejected() {
        let text = FIXTURE.replace("width = 1080", "width = 0");
        let err = Profile::parse(&text).expect_err("zero width must fail");
        assert!(err.to_string().contains("> 0"), "{err}");
    }
}
