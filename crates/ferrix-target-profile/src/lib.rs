//! The single strict parser for a target's versioned profile
//! (`targets/<target-id>/profile.toml`).
//!
//! Every consumer (adapters, the session layer, reference surfaces, tests)
//! goes through this crate so that the schema is enforced in exactly one
//! place. Unknown keys, missing sections, an unexpected `schema_version`, and
//! out-of-range values are all hard errors. Values describe emulator-test
//! configuration, not physical-hardware facts.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use ferrix_hardware_api::{AudioRoute, ModemRegistration, Orientation, SimState, WifiState};
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
    /// Detailed mock telephony state (ADR-006). Absent in older profiles;
    /// [`Profile::telephony_section`] derives one from `[connectivity]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telephony: Option<TelephonySection>,
    /// Detailed mock network state (ADR-006). Absent in older profiles;
    /// [`Profile::network_section`] derives one from `[connectivity]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkSection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySection {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub orientation: Orientation,
    /// How the display controller reads 32-bit DRM fourccs. Optional; absent
    /// means the DRM standard.
    #[serde(default, skip_serializing_if = "ScanoutByteOrder::is_standard")]
    pub scanout_byte_order: ScanoutByteOrder,
    /// Panel edges unusable for application content (camera cutout, rounded
    /// corners, gesture margin), in panel pixels of the natural orientation.
    /// Optional; absent means the whole panel is usable.
    #[serde(default, skip_serializing_if = "SafeArea::is_empty")]
    pub safe_area: SafeArea,
}

/// Insets from each panel edge, in pixels of the panel's natural orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SafeArea {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl SafeArea {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Byte order the display controller applies to 32-bit DRM fourccs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanoutByteOrder {
    /// Buffers are read as the DRM fourcc defines (little-endian packing).
    #[default]
    Standard,
    /// The controller reads each 32-bit fourcc byte-reversed (AB24 as RA24),
    /// as the Pixel 9a exynos DPU does; the compositor registers the reversed
    /// code so the rendered bytes display correctly.
    Reversed,
}

impl ScanoutByteOrder {
    pub fn is_standard(&self) -> bool {
        *self == Self::Standard
    }
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
    /// How the production Wi-Fi adapter joins a WPA2/WPA3 transition-mode
    /// access point (one advertising both PSK and SAE): `sae` unless the
    /// target has evidence that its driver cannot complete SAE.
    #[serde(default)]
    pub wifi_transition_key_mgmt: ProfileTransitionKeyMgmt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSection {
    pub route: AudioRoute,
}

/// Mock modem lifecycle state at boot (`[telephony] modem`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileModemState {
    Absent,
    Disabled,
    Initializing,
    Ready,
    Failed,
}

/// Mock SIM state at boot (`[telephony] sim`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSimState {
    Absent,
    Locked,
    Blocked,
    Ready,
}

/// Radio access technology reported by the mock (`[telephony] access`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileAccessTechnology {
    None,
    Gsm,
    Umts,
    Lte,
    Nr,
}

/// Telephony features the mock advertises (`[telephony] features`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileTelephonyFeature {
    Voice,
    Sms,
    MultipartSms,
    DeliveryReports,
    Dtmf,
    Hold,
    Data,
    AudioRoute,
    SimPin,
    RadioPower,
}

impl ProfileTelephonyFeature {
    /// Every feature the mock adapter can emulate.
    pub const ALL: [ProfileTelephonyFeature; 10] = [
        ProfileTelephonyFeature::Voice,
        ProfileTelephonyFeature::Sms,
        ProfileTelephonyFeature::MultipartSms,
        ProfileTelephonyFeature::DeliveryReports,
        ProfileTelephonyFeature::Dtmf,
        ProfileTelephonyFeature::Hold,
        ProfileTelephonyFeature::Data,
        ProfileTelephonyFeature::AudioRoute,
        ProfileTelephonyFeature::SimPin,
        ProfileTelephonyFeature::RadioPower,
    ];
}

/// Optional `[telephony]` section: the mock modem's boot state. Values are
/// synthetic emulator configuration, never carrier or device facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TelephonySection {
    pub modem: ProfileModemState,
    pub sim: ProfileSimState,
    /// Synthetic PIN; only meaningful when `sim = "locked"`.
    pub sim_pin: String,
    pub operator: String,
    pub access: ProfileAccessTechnology,
    /// Signal bars, `0..=4`.
    pub signal_bars: u8,
    pub roaming: bool,
    pub features: BTreeSet<ProfileTelephonyFeature>,
}

impl Default for TelephonySection {
    fn default() -> Self {
        TelephonySection {
            modem: ProfileModemState::Ready,
            sim: ProfileSimState::Ready,
            sim_pin: "0000".to_owned(),
            operator: "Ferrix Mock".to_owned(),
            access: ProfileAccessTechnology::Lte,
            signal_bars: 3,
            roaming: false,
            features: ProfileTelephonyFeature::ALL.into_iter().collect(),
        }
    }
}

impl TelephonySection {
    /// The section an older profile without `[telephony]` implies, derived
    /// from the coarse `[connectivity]` summary.
    pub fn derived_from(connectivity: &ConnectivitySection) -> Self {
        let sim = match connectivity.sim {
            SimState::Present => ProfileSimState::Ready,
            SimState::Absent => ProfileSimState::Absent,
            SimState::Locked => ProfileSimState::Locked,
        };
        let (signal_bars, access) = match connectivity.modem {
            ModemRegistration::Registered => (3, ProfileAccessTechnology::Lte),
            ModemRegistration::Searching | ModemRegistration::Denied => {
                (0, ProfileAccessTechnology::Lte)
            }
            ModemRegistration::Unregistered => (0, ProfileAccessTechnology::None),
        };
        TelephonySection {
            sim,
            signal_bars,
            access,
            ..TelephonySection::default()
        }
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.signal_bars > 4 {
            return Err(ProfileError::Invalid(format!(
                "telephony.signal_bars must be 0..=4 (got {})",
                self.signal_bars
            )));
        }
        if self.operator.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "telephony.operator must not be empty".to_owned(),
            ));
        }
        if self.sim == ProfileSimState::Locked
            && (self.sim_pin.len() < 4
                || self.sim_pin.len() > 8
                || !self.sim_pin.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(ProfileError::Invalid(
                "telephony.sim_pin must be 4..=8 digits when sim = \"locked\"".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Mock Wi-Fi adapter presence (`[network] adapter`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileAdapterPresence {
    Absent,
    Present,
}

/// Mock Wi-Fi radio state (`[network] radio`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileRadioState {
    Enabled,
    Disabled,
}

/// Security type of a synthetic access point.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSecurityType {
    Open,
    Wep,
    WpaPsk,
    Wpa2Psk,
    Wpa3Sae,
    Enterprise,
}

/// One synthetic access point the mock network adapter advertises.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileAccessPoint {
    pub ssid: String,
    pub security: ProfileSecurityType,
    /// Signal strength, `0..=100`.
    pub signal_percent: u8,
    /// Synthetic passphrase; mock only, never a real secret.
    #[serde(default)]
    pub passphrase: String,
}

/// Optional `[network]` section: the mock Wi-Fi adapter's boot state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct NetworkSection {
    pub adapter: ProfileAdapterPresence,
    pub radio: ProfileRadioState,
    /// SSID of the access point connected at boot, or `""` for none. Must
    /// name one of `access_points`.
    pub connected: String,
    pub access_points: Vec<ProfileAccessPoint>,
}

/// Key management for WPA2/WPA3 transition-mode access points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileTransitionKeyMgmt {
    #[default]
    Sae,
    Psk,
}

impl Default for NetworkSection {
    fn default() -> Self {
        NetworkSection {
            adapter: ProfileAdapterPresence::Present,
            radio: ProfileRadioState::Enabled,
            connected: String::new(),
            access_points: Vec::new(),
        }
    }
}

impl NetworkSection {
    /// The section an older profile without `[network]` implies, derived
    /// from the coarse `[connectivity] wifi` summary. A connected summary
    /// yields one synthetic open access point so `connected` stays valid.
    pub fn derived_from(connectivity: &ConnectivitySection) -> Self {
        match connectivity.wifi {
            WifiState::Connected => NetworkSection {
                connected: "ferrix-mock".to_owned(),
                access_points: vec![ProfileAccessPoint {
                    ssid: "ferrix-mock".to_owned(),
                    security: ProfileSecurityType::Open,
                    signal_percent: 70,
                    passphrase: String::new(),
                }],
                ..NetworkSection::default()
            },
            WifiState::Disconnected => NetworkSection::default(),
            WifiState::Disabled => NetworkSection {
                radio: ProfileRadioState::Disabled,
                ..NetworkSection::default()
            },
        }
    }

    fn validate(&self) -> Result<(), ProfileError> {
        let mut seen = BTreeSet::new();
        for ap in &self.access_points {
            if ap.ssid.trim().is_empty() {
                return Err(ProfileError::Invalid(
                    "network.access_points[].ssid must not be empty".to_owned(),
                ));
            }
            if ap.signal_percent > 100 {
                return Err(ProfileError::Invalid(format!(
                    "network.access_points[].signal_percent must be 0..=100 (got {} for ssid {:?})",
                    ap.signal_percent, ap.ssid
                )));
            }
            if !seen.insert(ap.ssid.as_str()) {
                return Err(ProfileError::Invalid(format!(
                    "network.access_points lists ssid {:?} twice",
                    ap.ssid
                )));
            }
        }
        if !self.connected.is_empty() && !seen.contains(self.connected.as_str()) {
            return Err(ProfileError::Invalid(format!(
                "network.connected names ssid {:?}, which is not in network.access_points",
                self.connected
            )));
        }
        Ok(())
    }
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
        if let Some(telephony) = &self.telephony {
            telephony.validate()?;
        }
        if let Some(network) = &self.network {
            network.validate()?;
        }
        Ok(())
    }

    /// The `[telephony]` section, or the one `[connectivity]` implies when
    /// the profile predates it. Mock adapters read this, never the field.
    pub fn telephony_section(&self) -> TelephonySection {
        self.telephony
            .clone()
            .unwrap_or_else(|| TelephonySection::derived_from(&self.connectivity))
    }

    /// The `[network]` section, or the one `[connectivity]` implies when the
    /// profile predates it.
    pub fn network_section(&self) -> NetworkSection {
        self.network
            .clone()
            .unwrap_or_else(|| NetworkSection::derived_from(&self.connectivity))
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

    const SERVICE_SECTIONS: &str = r#"
[telephony]
modem = "ready"
sim = "locked"
sim_pin = "0000"
operator = "Ferrix Mock"
access = "lte"
signal_bars = 3
roaming = false
features = ["voice", "sms", "sim-pin"]

[network]
adapter = "present"
radio = "enabled"
connected = "ferrix-lab"

[[network.access_points]]
ssid = "ferrix-lab"
security = "wpa2-psk"
signal_percent = 78
passphrase = "mock-passphrase"

[[network.access_points]]
ssid = "ferrix-guest"
security = "open"
signal_percent = 41
"#;

    fn with_services() -> String {
        format!("{FIXTURE}{SERVICE_SECTIONS}")
    }

    #[test]
    fn service_sections_absent_derive_from_connectivity() {
        let p = Profile::parse(FIXTURE).expect("fixture parses");
        assert!(p.telephony.is_none() && p.network.is_none());
        let telephony = p.telephony_section();
        assert_eq!(telephony.modem, ProfileModemState::Ready);
        assert_eq!(telephony.sim, ProfileSimState::Ready);
        assert_eq!(telephony.signal_bars, 3);
        assert_eq!(telephony.features.len(), ProfileTelephonyFeature::ALL.len());
        let network = p.network_section();
        assert_eq!(network.adapter, ProfileAdapterPresence::Present);
        assert_eq!(network.radio, ProfileRadioState::Enabled);
        assert_eq!(network.connected, "ferrix-mock");
        assert_eq!(network.access_points.len(), 1);
        network.validate().expect("derived section is valid");

        let text = FIXTURE
            .replace("wifi = \"connected\"", "wifi = \"disabled\"")
            .replace("modem = \"registered\"", "modem = \"unregistered\"")
            .replace("sim = \"present\"", "sim = \"absent\"");
        let p = Profile::parse(&text).expect("parses");
        assert_eq!(p.network_section().radio, ProfileRadioState::Disabled);
        assert_eq!(p.network_section().connected, "");
        assert_eq!(p.telephony_section().sim, ProfileSimState::Absent);
        assert_eq!(p.telephony_section().access, ProfileAccessTechnology::None);
        assert_eq!(p.telephony_section().signal_bars, 0);
    }

    #[test]
    fn service_sections_parse() {
        let p = Profile::parse(&with_services()).expect("service sections parse");
        let telephony = p.telephony.as_ref().expect("telephony present");
        assert_eq!(telephony.sim, ProfileSimState::Locked);
        assert_eq!(telephony.sim_pin, "0000");
        assert_eq!(telephony.operator, "Ferrix Mock");
        assert_eq!(telephony.access, ProfileAccessTechnology::Lte);
        assert_eq!(telephony.features.len(), 3);
        assert!(
            telephony
                .features
                .contains(&ProfileTelephonyFeature::SimPin)
        );
        let network = p.network.as_ref().expect("network present");
        assert_eq!(network.connected, "ferrix-lab");
        assert_eq!(
            network.access_points[0].security,
            ProfileSecurityType::Wpa2Psk
        );
        assert_eq!(network.access_points[1].passphrase, "");
        assert_eq!(p.telephony_section(), *telephony);
        assert_eq!(p.network_section(), *network);
    }

    #[test]
    fn partial_service_sections_fill_defaults() {
        let text = format!(
            "{FIXTURE}\n[telephony]\nsim = \"absent\"\n\n[network]\nradio = \"disabled\"\n"
        );
        let p = Profile::parse(&text).expect("partial sections parse");
        let telephony = p.telephony.expect("telephony present");
        assert_eq!(telephony.sim, ProfileSimState::Absent);
        assert_eq!(telephony.modem, ProfileModemState::Ready);
        assert_eq!(telephony.operator, "Ferrix Mock");
        let network = p.network.expect("network present");
        assert_eq!(network.radio, ProfileRadioState::Disabled);
        assert_eq!(network.adapter, ProfileAdapterPresence::Present);
        assert!(network.access_points.is_empty());
    }

    #[test]
    fn unknown_key_in_service_sections_is_rejected() {
        let text = with_services().replace("roaming = false", "roaming = false\nimsi = \"1\"");
        let err = Profile::parse(&text).expect_err("unknown telephony key must fail");
        assert!(matches!(err, ProfileError::Schema(_)), "{err}");
        assert!(err.to_string().contains("imsi"), "{err}");

        let text =
            with_services().replace("signal_percent = 41", "signal_percent = 41\nbssid = \"x\"");
        let err = Profile::parse(&text).expect_err("unknown access point key must fail");
        assert!(matches!(err, ProfileError::Schema(_)), "{err}");
        assert!(err.to_string().contains("bssid"), "{err}");
    }

    #[test]
    fn unknown_service_enum_values_are_rejected() {
        for (from, to) in [
            ("modem = \"ready\"", "modem = \"maybe\""),
            ("sim = \"locked\"", "sim = \"puk\""),
            ("access = \"lte\"", "access = \"6g\""),
            ("\"sim-pin\"", "\"teleport\""),
            ("adapter = \"present\"", "adapter = \"broken\""),
            ("security = \"open\"", "security = \"wpa9\""),
        ] {
            let text = with_services().replace(from, to);
            let err = Profile::parse(&text).expect_err(to);
            assert!(matches!(err, ProfileError::Schema(_)), "{to}: {err}");
        }
    }

    #[test]
    fn bad_signal_bars_are_rejected() {
        let text = with_services().replace("signal_bars = 3", "signal_bars = 5");
        let err = Profile::parse(&text).expect_err("5 bars must fail");
        assert!(
            err.to_string().contains("signal_bars must be 0..=4"),
            "{err}"
        );
        let text = with_services().replace("signal_bars = 3", "signal_bars = -1");
        assert!(Profile::parse(&text).is_err());
    }

    #[test]
    fn bad_sim_pin_is_rejected_only_when_locked() {
        let text = with_services().replace("sim_pin = \"0000\"", "sim_pin = \"12ab\"");
        let err = Profile::parse(&text).expect_err("non-digit pin on a locked sim must fail");
        assert!(err.to_string().contains("sim_pin"), "{err}");
        let text = text.replace("sim = \"locked\"", "sim = \"ready\"");
        Profile::parse(&text).expect("pin is irrelevant when the sim is ready");
    }

    #[test]
    fn connected_to_unknown_ssid_is_rejected() {
        let text =
            with_services().replace("connected = \"ferrix-lab\"", "connected = \"elsewhere\"");
        let err = Profile::parse(&text).expect_err("connected to an unlisted ssid must fail");
        assert!(matches!(err, ProfileError::Invalid(_)), "{err}");
        assert!(err.to_string().contains("elsewhere"), "{err}");
        let text = with_services().replace("connected = \"ferrix-lab\"", "connected = \"\"");
        Profile::parse(&text).expect("empty connected means none");
    }

    #[test]
    fn access_point_constraints_are_enforced() {
        let text = with_services().replace("ssid = \"ferrix-guest\"", "ssid = \" \"");
        let err = Profile::parse(&text).expect_err("blank ssid must fail");
        assert!(err.to_string().contains("ssid must not be empty"), "{err}");
        let text = with_services().replace("signal_percent = 41", "signal_percent = 101");
        let err = Profile::parse(&text).expect_err("101% must fail");
        assert!(err.to_string().contains("signal_percent"), "{err}");
        let text = with_services().replace("ssid = \"ferrix-guest\"", "ssid = \"ferrix-lab\"");
        let err = Profile::parse(&text).expect_err("duplicate ssid must fail");
        assert!(err.to_string().contains("twice"), "{err}");
    }

    #[test]
    fn checked_in_profile_has_service_sections() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        );
        let p = Profile::load(path).expect("checked-in profile parses");
        let telephony = p.telephony.expect("[telephony] present");
        assert_eq!(telephony.operator, "Ferrix Mock");
        assert_eq!(telephony.sim_pin, "0000");
        let network = p.network.expect("[network] present");
        assert_eq!(network.connected, "ferrix-lab");
        assert_eq!(network.access_points.len(), 2);
    }
}
