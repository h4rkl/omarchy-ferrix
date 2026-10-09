//! Stable, target-neutral hardware contracts.
//!
//! Product code (the mobile core, the shell, applications) depends only on this
//! crate. Target adapters implement these traits; the session/bootstrap layer
//! selects an adapter set at runtime. Nothing here knows about QEMU, kernels,
//! vendors, or boards.
//!
//! Traits are synchronous: state reads and commands are small and immediate.
//! Change notification uses a plain [`std::sync::mpsc`] subscription so that no
//! async runtime is imposed on consumers.
//!
//! Every capability has a `CapabilityStatus`. An adapter that does not provide
//! a capability must return [`HardwareError::Unsupported`] instead of imitating
//! a successful hardware operation.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

/// Current schema version of the serialized state and event types.
pub const SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Capabilities and errors
// ---------------------------------------------------------------------------

/// Every portable capability a target may provide.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Power,
    Display,
    Input,
    Network,
    Bluetooth,
    Audio,
    Telephony,
    Lifecycle,
    /// Deterministic test control of mocked state. Only mocked targets provide it.
    MockControl,
}

impl Capability {
    /// All capabilities, in matrix order.
    pub const ALL: [Capability; 9] = [
        Capability::Power,
        Capability::Display,
        Capability::Input,
        Capability::Network,
        Capability::Bluetooth,
        Capability::Audio,
        Capability::Telephony,
        Capability::Lifecycle,
        Capability::MockControl,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Capability::Power => "power",
            Capability::Display => "display",
            Capability::Input => "input",
            Capability::Network => "network",
            Capability::Bluetooth => "bluetooth",
            Capability::Audio => "audio",
            Capability::Telephony => "telephony",
            Capability::Lifecycle => "lifecycle",
            Capability::MockControl => "mock-control",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a target provides a capability. `Mocked` is emulator/test behavior and
/// must never be reported as `Supported`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    Mocked,
}

/// A target's capability matrix: one status per capability.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityMatrix(pub BTreeMap<Capability, CapabilityStatus>);

impl CapabilityMatrix {
    pub fn uniform(status: CapabilityStatus) -> Self {
        Self(Capability::ALL.iter().map(|c| (*c, status)).collect())
    }

    pub fn status(&self, capability: Capability) -> CapabilityStatus {
        self.0
            .get(&capability)
            .copied()
            .unwrap_or(CapabilityStatus::Unsupported)
    }

    pub fn with(mut self, capability: Capability, status: CapabilityStatus) -> Self {
        self.0.insert(capability, status);
        self
    }
}

/// Typed failures. Unsupported operations are errors, never silent successes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HardwareError {
    Unsupported {
        capability: Capability,
    },
    InvalidValue {
        capability: Capability,
        reason: String,
    },
    Failed {
        capability: Capability,
        reason: String,
    },
}

impl HardwareError {
    pub fn unsupported(capability: Capability) -> Self {
        HardwareError::Unsupported { capability }
    }

    pub fn invalid(capability: Capability, reason: impl Into<String>) -> Self {
        HardwareError::InvalidValue {
            capability,
            reason: reason.into(),
        }
    }

    pub fn failed(capability: Capability, reason: impl Into<String>) -> Self {
        HardwareError::Failed {
            capability,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for HardwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HardwareError::Unsupported { capability } => {
                write!(f, "{capability} is unsupported on this target")
            }
            HardwareError::InvalidValue { capability, reason } => {
                write!(f, "invalid {capability} value: {reason}")
            }
            HardwareError::Failed { capability, reason } => {
                write!(f, "{capability} operation failed: {reason}")
            }
        }
    }
}

impl std::error::Error for HardwareError {}

// ---------------------------------------------------------------------------
// Power
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PowerState {
    pub battery_percent: u8,
    pub charging: bool,
}

pub trait Power: Send + Sync {
    fn power_state(&self) -> PowerState;
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    Portrait,
    Landscape,
    PortraitFlipped,
    LandscapeFlipped,
}

impl Orientation {
    pub fn name(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
            Orientation::PortraitFlipped => "portrait-flipped",
            Orientation::LandscapeFlipped => "landscape-flipped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayState {
    /// Native panel geometry in pixels, independent of orientation.
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub orientation: Orientation,
    pub brightness_percent: u8,
    /// Auto-rotation is off: the applied orientation stays at the natural
    /// portrait orientation whatever the rotation sensor reports. Runtime UI
    /// state, not a target fact; absent in older snapshots means unlocked.
    #[serde(default)]
    pub rotation_locked: bool,
}

pub trait Display: Send + Sync {
    fn display_state(&self) -> DisplayState;
    fn set_brightness(&self, percent: u8) -> Result<DisplayState, HardwareError>;
    /// Turns auto-rotation off (`locked`, pinning portrait) or back on
    /// (following the rotation sensor again). Targets without a rotation
    /// source report [`HardwareError::Unsupported`]; nothing is imitated.
    fn set_rotation_lock(&self, _locked: bool) -> Result<DisplayState, HardwareError> {
        Err(HardwareError::unsupported(Capability::Display))
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InputState {
    pub touch: bool,
    pub rotation: bool,
}

pub trait Input: Send + Sync {
    fn input_state(&self) -> InputState;
}

// ---------------------------------------------------------------------------
// Network (Wi-Fi)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WifiState {
    Connected,
    Disconnected,
    Disabled,
}

impl WifiState {
    pub fn name(self) -> &'static str {
        match self {
            WifiState::Connected => "connected",
            WifiState::Disconnected => "disconnected",
            WifiState::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkState {
    pub wifi: WifiState,
}

pub trait Network: Send + Sync {
    fn network_state(&self) -> NetworkState;
    fn set_wifi_enabled(&self, enabled: bool) -> Result<NetworkState, HardwareError>;
}

// ---------------------------------------------------------------------------
// Bluetooth
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BluetoothState {
    pub present: bool,
    pub enabled: bool,
}

/// How long one discovery session runs before the adapter stops it by
/// itself. Discovery costs radio time and battery; nothing scans unbounded.
pub const BLUETOOTH_DISCOVERY_TIMEOUT_MS: u32 = 30_000;

/// How long a pairing prompt waits for the user before the adapter answers
/// it as declined and the pairing fails with [`PairingFailure::TimedOut`].
pub const BLUETOOTH_PROMPT_TIMEOUT_MS: u32 = 30_000;

/// Coarse device class for a label or glyph. Derived from what the remote
/// advertises; never a fingerprint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BluetoothDeviceKind {
    /// Headphones, headsets, speakers.
    Audio,
    /// Keyboards, mice, game controllers.
    Input,
    Phone,
    Computer,
    /// Watches and other wearables.
    Wearable,
    #[default]
    Other,
}

impl BluetoothDeviceKind {
    pub fn name(self) -> &'static str {
        match self {
            BluetoothDeviceKind::Audio => "audio",
            BluetoothDeviceKind::Input => "input",
            BluetoothDeviceKind::Phone => "phone",
            BluetoothDeviceKind::Computer => "computer",
            BluetoothDeviceKind::Wearable => "wearable",
            BluetoothDeviceKind::Other => "other",
        }
    }

    pub fn is_other(&self) -> bool {
        *self == BluetoothDeviceKind::Other
    }
}

/// A device the managed controller knows (paired, remembered, or found by
/// discovery). Display data only: `id` is an adapter-assigned opaque token
/// that is never a hardware address, and evidence records carry counts,
/// never names.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BluetoothDevice {
    pub id: String,
    /// The remote's advertised name, if it reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub paired: bool,
    pub connected: bool,
    #[serde(default, skip_serializing_if = "BluetoothDeviceKind::is_other")]
    pub kind: BluetoothDeviceKind,
    /// Signal strength in bars (`0..=4`) while the device is in range of
    /// the current or last discovery; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_bars: Option<u8>,
}

/// Maps a received signal strength (dBm) to `0..=4` bars.
pub fn signal_bars_from_rssi(rssi_dbm: i16) -> u8 {
    match rssi_dbm {
        r if r >= -60 => 4,
        r if r >= -70 => 3,
        r if r >= -80 => 2,
        r if r >= -90 => 1,
        _ => 0,
    }
}

/// Why a pairing attempt ended without a bond.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairingFailure {
    /// The user answered the prompt with Cancel.
    Declined,
    /// The remote device refused (or the codes did not match on its side).
    Rejected,
    /// The prompt went unanswered, or the remote stopped responding.
    TimedOut,
    /// Pairing was cancelled (Bluetooth turned off, cancel requested).
    Canceled,
    /// The remote needs a legacy PIN typed in; PIN entry is not supported.
    PinRequired,
    /// The remote left range or could not be reached.
    Unreachable,
    /// Any other failure the Bluetooth service reported.
    Failed,
}

impl PairingFailure {
    pub fn name(self) -> &'static str {
        match self {
            PairingFailure::Declined => "declined",
            PairingFailure::Rejected => "rejected",
            PairingFailure::TimedOut => "timed-out",
            PairingFailure::Canceled => "canceled",
            PairingFailure::PinRequired => "pin-required",
            PairingFailure::Unreachable => "unreachable",
            PairingFailure::Failed => "failed",
        }
    }
}

/// Where one pairing attempt stands. Prompt phases carry the six-digit code
/// for the Settings prompt; [`PairingPhase::stage`] drops it for records.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum PairingPhase {
    /// Bonding is in progress; nothing is asked of the user.
    Pairing,
    /// Numeric comparison: the user confirms the remote shows `passkey`.
    ConfirmPasskey {
        passkey: u32,
    },
    /// Just Works: the user confirms pairing with this device at all.
    Authorize,
    /// The user types `passkey` on the remote (keyboards).
    DisplayPasskey {
        passkey: u32,
    },
    /// Bonded.
    Paired,
    Failed {
        reason: PairingFailure,
    },
}

impl PairingPhase {
    /// Whether the phase waits for an accept/reject answer.
    pub fn awaits_reply(self) -> bool {
        matches!(
            self,
            PairingPhase::ConfirmPasskey { .. } | PairingPhase::Authorize
        )
    }

    /// Whether the attempt is over.
    pub fn is_finished(self) -> bool {
        matches!(self, PairingPhase::Paired | PairingPhase::Failed { .. })
    }

    pub fn stage(self) -> PairingStage {
        match self {
            PairingPhase::Pairing => PairingStage::Pairing,
            PairingPhase::ConfirmPasskey { .. } => PairingStage::ConfirmPasskey,
            PairingPhase::Authorize => PairingStage::Authorize,
            PairingPhase::DisplayPasskey { .. } => PairingStage::DisplayPasskey,
            PairingPhase::Paired => PairingStage::Paired,
            PairingPhase::Failed { reason } => PairingStage::Failed { reason },
        }
    }
}

/// [`PairingPhase`] without the passkey: safe for events and evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "kebab-case")]
pub enum PairingStage {
    Pairing,
    ConfirmPasskey,
    Authorize,
    DisplayPasskey,
    Paired,
    Failed { reason: PairingFailure },
}

impl PairingStage {
    pub fn name(self) -> &'static str {
        match self {
            PairingStage::Pairing => "pairing",
            PairingStage::ConfirmPasskey => "confirm-passkey",
            PairingStage::Authorize => "authorize",
            PairingStage::DisplayPasskey => "display-passkey",
            PairingStage::Paired => "paired",
            PairingStage::Failed { .. } => "failed",
        }
    }
}

/// The current (or last finished) pairing attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BluetoothPairing {
    /// Opaque id of the device being paired.
    pub device: String,
    pub phase: PairingPhase,
}

/// A connect or disconnect request still running or recently failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BluetoothAction {
    Connect,
    Disconnect,
}

impl BluetoothAction {
    pub fn name(self) -> &'static str {
        match self {
            BluetoothAction::Connect => "connect",
            BluetoothAction::Disconnect => "disconnect",
        }
    }
}

/// A device operation in flight, or the typed reason the last one failed.
/// A successful operation simply disappears: the device list is the truth.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BluetoothOperation {
    pub device: String,
    pub action: BluetoothAction,
    /// `None` while running; the failure text (never an address) after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
}

/// Discovery state of the managed controller.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BluetoothDiscovery {
    pub active: bool,
    /// Milliseconds until the adapter stops the session by itself, when
    /// it started it (another client's discovery has no deadline here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_ms: Option<u32>,
}

/// Everything the Pair-new-device and device-detail flows need, in one
/// reply. Not part of the copyable [`HardwareState`]: clients re-query on
/// [`HardwareEvent::BluetoothSessionChanged`] and `bluetooth-changed`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BluetoothSession {
    pub discovery: BluetoothDiscovery,
    /// Every known device, in [`sort_bluetooth_devices`] order.
    pub devices: Vec<BluetoothDevice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing: Option<BluetoothPairing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<BluetoothOperation>,
}

impl BluetoothSession {
    pub fn device(&self, id: &str) -> Option<&BluetoothDevice> {
        self.devices.iter().find(|device| device.id == id)
    }

    /// The evidence-safe summary carried by
    /// [`HardwareEvent::BluetoothSessionChanged`]: counts and stages only.
    pub fn summary(&self) -> BluetoothSessionSummary {
        let count = |f: fn(&BluetoothDevice) -> bool| {
            u16::try_from(self.devices.iter().filter(|d| f(d)).count()).unwrap_or(u16::MAX)
        };
        BluetoothSessionSummary {
            discovering: self.discovery.active,
            available: count(|d| !d.paired),
            paired: count(|d| d.paired),
            connected: count(|d| d.connected),
            pairing: self.pairing.as_ref().map(|p| p.phase.stage()),
            operation: self.operation.as_ref().map(|o| OperationStage {
                action: o.action,
                failed: o.failed.is_some(),
            }),
        }
    }
}

/// An operation's action and whether it failed, without the device or text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationStage {
    pub action: BluetoothAction,
    pub failed: bool,
}

/// Copyable, evidence-safe digest of a [`BluetoothSession`]: no names, no
/// ids, no passkeys.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BluetoothSessionSummary {
    pub discovering: bool,
    pub available: u16,
    pub paired: u16,
    pub connected: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing: Option<PairingStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<OperationStage>,
}

/// Product Bluetooth requests over the core socket. Device ids are the
/// adapter's opaque tokens from [`BluetoothSession::devices`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum BluetoothCommand {
    StartDiscovery,
    StopDiscovery,
    Pair {
        device: String,
    },
    /// Abandon the running pairing attempt.
    CancelPairing,
    /// Answer the pending [`PairingPhase::awaits_reply`] prompt.
    ReplyPairing {
        accept: bool,
    },
    Connect {
        device: String,
    },
    Disconnect {
        device: String,
    },
    /// Remove the bond and the device record.
    Forget {
        device: String,
    },
}

impl BluetoothCommand {
    pub fn name(&self) -> &'static str {
        match self {
            BluetoothCommand::StartDiscovery => "start-discovery",
            BluetoothCommand::StopDiscovery => "stop-discovery",
            BluetoothCommand::Pair { .. } => "pair",
            BluetoothCommand::CancelPairing => "cancel-pairing",
            BluetoothCommand::ReplyPairing { .. } => "reply-pairing",
            BluetoothCommand::Connect { .. } => "connect",
            BluetoothCommand::Disconnect { .. } => "disconnect",
            BluetoothCommand::Forget { .. } => "forget",
        }
    }

    /// Routes the command to the matching [`Bluetooth`] method.
    pub fn apply(&self, bluetooth: &dyn Bluetooth) -> Result<(), HardwareError> {
        match self {
            BluetoothCommand::StartDiscovery => bluetooth.start_bluetooth_discovery(),
            BluetoothCommand::StopDiscovery => bluetooth.stop_bluetooth_discovery(),
            BluetoothCommand::Pair { device } => bluetooth.pair_bluetooth_device(device),
            BluetoothCommand::CancelPairing => bluetooth.cancel_bluetooth_pairing(),
            BluetoothCommand::ReplyPairing { accept } => bluetooth.reply_bluetooth_pairing(*accept),
            BluetoothCommand::Connect { device } => bluetooth.connect_bluetooth_device(device),
            BluetoothCommand::Disconnect { device } => {
                bluetooth.disconnect_bluetooth_device(device)
            }
            BluetoothCommand::Forget { device } => bluetooth.forget_bluetooth_device(device),
        }
    }
}

/// Bluetooth controller power, devices, discovery, and pairing.
///
/// Discovery, pairing, and connections are asynchronous: each command
/// returns once accepted, and progress arrives as
/// [`HardwareEvent::BluetoothSessionChanged`] with the full state in
/// [`Bluetooth::bluetooth_session`]. Every optional method defaults to
/// [`HardwareError::Unsupported`]; nothing is imitated.
pub trait Bluetooth: Send + Sync {
    fn bluetooth_state(&self) -> BluetoothState;
    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<BluetoothState, HardwareError>;
    /// Devices of the managed controller in [`sort_bluetooth_devices`]
    /// order. An absent controller has none. Adapters that cannot enumerate
    /// devices report [`HardwareError::Unsupported`]; nothing is imitated.
    fn bluetooth_devices(&self) -> Result<Vec<BluetoothDevice>, HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    /// Devices plus discovery, pairing, and operation state.
    fn bluetooth_session(&self) -> Result<BluetoothSession, HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    /// Starts a bounded discovery that stops by itself after
    /// [`BLUETOOTH_DISCOVERY_TIMEOUT_MS`]. Restarting extends the deadline.
    fn start_bluetooth_discovery(&self) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    fn stop_bluetooth_discovery(&self) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    /// Starts pairing an unpaired device. A pairing that needs the user's
    /// answer moves to a prompt phase; answer with
    /// [`Bluetooth::reply_bluetooth_pairing`] within
    /// [`BLUETOOTH_PROMPT_TIMEOUT_MS`].
    fn pair_bluetooth_device(&self, _device: &str) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    fn cancel_bluetooth_pairing(&self) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    fn reply_bluetooth_pairing(&self, _accept: bool) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    fn connect_bluetooth_device(&self, _device: &str) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    fn disconnect_bluetooth_device(&self, _device: &str) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
    /// Removes the bond and the device record.
    fn forget_bluetooth_device(&self, _device: &str) -> Result<(), HardwareError> {
        Err(HardwareError::unsupported(Capability::Bluetooth))
    }
}

/// Stable display order for a device list: paired first, connected first,
/// then by name (unnamed last), then id.
pub fn sort_bluetooth_devices(devices: &mut [BluetoothDevice]) {
    devices.sort_by(|a, b| {
        b.paired
            .cmp(&a.paired)
            .then_with(|| b.connected.cmp(&a.connected))
            .then_with(|| match (&a.name, &b.name) {
                (Some(x), Some(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.id.cmp(&b.id))
    });
}

// ---------------------------------------------------------------------------
// Audio route
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioRoute {
    Speaker,
    Earpiece,
    Headphones,
    Bluetooth,
}

impl AudioRoute {
    pub fn name(self) -> &'static str {
        match self {
            AudioRoute::Speaker => "speaker",
            AudioRoute::Earpiece => "earpiece",
            AudioRoute::Headphones => "headphones",
            AudioRoute::Bluetooth => "bluetooth",
        }
    }
}

/// Routes currently exposed by the system audio policy service.
///
/// A fixed field set keeps the aggregate hardware snapshot copyable while
/// still allowing controls to disable routes that are not currently usable.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AudioRouteAvailability {
    pub speaker: bool,
    pub earpiece: bool,
    pub headphones: bool,
    pub bluetooth: bool,
}

impl AudioRouteAvailability {
    pub const ALL: Self = Self {
        speaker: true,
        earpiece: true,
        headphones: true,
        bluetooth: true,
    };

    pub fn contains(self, route: AudioRoute) -> bool {
        match route {
            AudioRoute::Speaker => self.speaker,
            AudioRoute::Earpiece => self.earpiece,
            AudioRoute::Headphones => self.headphones,
            AudioRoute::Bluetooth => self.bluetooth,
        }
    }

    pub fn insert(&mut self, route: AudioRoute) {
        match route {
            AudioRoute::Speaker => self.speaker = true,
            AudioRoute::Earpiece => self.earpiece = true,
            AudioRoute::Headphones => self.headphones = true,
            AudioRoute::Bluetooth => self.bluetooth = true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AudioState {
    pub route: AudioRoute,
    pub available: AudioRouteAvailability,
}

pub trait Audio: Send + Sync {
    fn audio_state(&self) -> AudioState;
    fn set_audio_route(&self, route: AudioRoute) -> Result<AudioState, HardwareError>;
}

// ---------------------------------------------------------------------------
// Telephony (modem + SIM)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModemRegistration {
    Registered,
    Searching,
    Denied,
    Unregistered,
}

impl ModemRegistration {
    pub fn name(self) -> &'static str {
        match self {
            ModemRegistration::Registered => "registered",
            ModemRegistration::Searching => "searching",
            ModemRegistration::Denied => "denied",
            ModemRegistration::Unregistered => "unregistered",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SimState {
    Present,
    Absent,
    Locked,
}

impl SimState {
    pub fn name(self) -> &'static str {
        match self {
            SimState::Present => "present",
            SimState::Absent => "absent",
            SimState::Locked => "locked",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TelephonyState {
    pub registration: ModemRegistration,
    pub sim: SimState,
}

pub trait Telephony: Send + Sync {
    fn telephony_state(&self) -> TelephonyState;
}

// ---------------------------------------------------------------------------
// Lifecycle (suspend)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuspendOutcome {
    /// The system suspended and resumed normally.
    Resumed,
    /// Suspend was requested but the target refused (for example an inhibitor).
    Rejected,
    /// Suspend was attempted and failed.
    Failed,
}

impl SuspendOutcome {
    pub fn name(self) -> &'static str {
        match self {
            SuspendOutcome::Resumed => "resumed",
            SuspendOutcome::Rejected => "rejected",
            SuspendOutcome::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LifecycleState {
    pub suspend_count: u32,
    pub last_suspend: Option<SuspendOutcome>,
}

pub trait Lifecycle: Send + Sync {
    fn lifecycle_state(&self) -> LifecycleState;
    fn suspend(&self) -> Result<SuspendOutcome, HardwareError>;
}

// ---------------------------------------------------------------------------
// Aggregate state and events
// ---------------------------------------------------------------------------

/// A full typed snapshot of every capability's state.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HardwareState {
    pub power: PowerState,
    pub display: DisplayState,
    pub input: InputState,
    pub network: NetworkState,
    pub bluetooth: BluetoothState,
    pub audio: AudioState,
    pub telephony: TelephonyState,
    pub lifecycle: LifecycleState,
}

/// Every change notification an adapter may emit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum HardwareEvent {
    PowerChanged(PowerState),
    DisplayChanged(DisplayState),
    NetworkChanged(NetworkState),
    BluetoothChanged(BluetoothState),
    /// Discovery, the device set, a pairing attempt, or a device operation
    /// changed. Not folded into [`HardwareState`]; clients re-query
    /// [`Bluetooth::bluetooth_session`].
    BluetoothSessionChanged(BluetoothSessionSummary),
    AudioChanged(AudioState),
    TelephonyChanged(TelephonyState),
    /// A suspend cycle completed with the given outcome.
    Suspended {
        outcome: SuspendOutcome,
        lifecycle: LifecycleState,
    },
}

impl HardwareEvent {
    pub fn name(&self) -> &'static str {
        match self {
            HardwareEvent::PowerChanged(_) => "power-changed",
            HardwareEvent::DisplayChanged(_) => "display-changed",
            HardwareEvent::NetworkChanged(_) => "network-changed",
            HardwareEvent::BluetoothChanged(_) => "bluetooth-changed",
            HardwareEvent::BluetoothSessionChanged(_) => "bluetooth-session-changed",
            HardwareEvent::AudioChanged(_) => "audio-changed",
            HardwareEvent::TelephonyChanged(_) => "telephony-changed",
            HardwareEvent::Suspended { .. } => "suspended",
        }
    }

    pub fn capability(&self) -> Capability {
        match self {
            HardwareEvent::PowerChanged(_) => Capability::Power,
            HardwareEvent::DisplayChanged(_) => Capability::Display,
            HardwareEvent::NetworkChanged(_) => Capability::Network,
            HardwareEvent::BluetoothChanged(_) | HardwareEvent::BluetoothSessionChanged(_) => {
                Capability::Bluetooth
            }
            HardwareEvent::AudioChanged(_) => Capability::Audio,
            HardwareEvent::TelephonyChanged(_) => Capability::Telephony,
            HardwareEvent::Suspended { .. } => Capability::Lifecycle,
        }
    }

    /// Fold this event into a snapshot.
    pub fn apply_to(&self, state: &mut HardwareState) {
        match *self {
            HardwareEvent::PowerChanged(v) => state.power = v,
            HardwareEvent::DisplayChanged(v) => state.display = v,
            HardwareEvent::NetworkChanged(v) => state.network = v,
            HardwareEvent::BluetoothChanged(v) => state.bluetooth = v,
            HardwareEvent::BluetoothSessionChanged(_) => {}
            HardwareEvent::AudioChanged(v) => state.audio = v,
            HardwareEvent::TelephonyChanged(v) => state.telephony = v,
            HardwareEvent::Suspended { lifecycle, .. } => state.lifecycle = lifecycle,
        }
    }
}

/// Change-notification subscription. Adapters hold the registered senders and
/// drop any whose receiver has gone away.
pub trait EventSource: Send + Sync {
    fn subscribe(&self, sender: Sender<HardwareEvent>);
}

// ---------------------------------------------------------------------------
// Mock control (mocked targets only)
// ---------------------------------------------------------------------------

/// Deterministic state mutations a mocked target accepts for tests. A real
/// target does not provide this capability.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mock", rename_all = "kebab-case")]
pub enum MockCommand {
    SetCharging {
        charging: bool,
    },
    SetBatteryPercent {
        battery_percent: u8,
    },
    SetWifi {
        state: WifiState,
    },
    SetBluetoothPresent {
        present: bool,
    },
    SetAudioRoute {
        route: AudioRoute,
    },
    SetModemRegistration {
        registration: ModemRegistration,
    },
    SetSim {
        state: SimState,
    },
    SetOrientation {
        orientation: Orientation,
    },
    /// Outcome the next `Lifecycle::suspend` reports.
    SetSuspendOutcome {
        outcome: SuspendOutcome,
    },
    /// Overrides how every following mock pairing ends (`None` restores
    /// each device's profile outcome).
    SetBluetoothPairingOutcome {
        #[serde(default)]
        outcome: Option<MockPairingOutcome>,
    },
    /// Advances the mock Bluetooth clock: discovery deadlines, remote
    /// pairing steps, and prompt timeouts move only on this, never on the
    /// wall clock.
    BluetoothTick {
        ms: u32,
    },
}

/// How a mock remote ends a pairing attempt once the local side agreed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MockPairingOutcome {
    #[default]
    Success,
    /// The remote refuses: [`PairingFailure::Rejected`].
    Rejected,
    /// The remote stops responding: [`PairingFailure::TimedOut`] once the
    /// mock clock passes the timeout.
    Timeout,
}

/// Which pairing method a mock remote negotiates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MockPairingMethod {
    /// Just Works with no prompt (BlueZ bonds silently when we initiate).
    #[default]
    JustWorks,
    /// Numeric comparison: [`PairingPhase::ConfirmPasskey`].
    Confirm,
    /// Just Works with an explicit authorization prompt.
    Authorize,
    /// The user types the code on the remote: [`PairingPhase::DisplayPasskey`].
    DisplayPasskey,
    /// A legacy PIN device: [`PairingFailure::PinRequired`].
    Pin,
}

pub trait MockControl: Send + Sync {
    /// Applies the mutation and returns the event it produced (also delivered
    /// to every subscriber).
    fn apply(&self, command: MockCommand) -> Result<Option<HardwareEvent>, HardwareError>;
}

/// Commands available to product clients over the core socket. These are
/// deliberately separate from [`MockCommand`]: applications can request real
/// user-facing operations but cannot manufacture hardware facts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum HardwareCommand {
    SetWifiEnabled {
        enabled: bool,
    },
    SetBluetoothEnabled {
        enabled: bool,
    },
    SetBrightness {
        percent: u8,
    },
    /// Auto-rotation off (`locked`, portrait) or on.
    SetRotationLock {
        locked: bool,
    },
    SetAudioRoute {
        route: AudioRoute,
    },
    Suspend,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_with_kebab_case_tags() {
        let event = HardwareEvent::PowerChanged(PowerState {
            battery_percent: 75,
            charging: true,
        });
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"event":"power-changed","battery_percent":75,"charging":true}"#
        );
        let back: HardwareEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
        assert_eq!(event.name(), "power-changed");
    }

    #[test]
    fn unsupported_is_a_typed_error() {
        let err = HardwareError::unsupported(Capability::Telephony);
        assert_eq!(err.to_string(), "telephony is unsupported on this target");
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, r#"{"kind":"unsupported","capability":"telephony"}"#);
    }

    #[test]
    fn matrix_defaults_to_unsupported() {
        let m = CapabilityMatrix::default();
        assert_eq!(m.status(Capability::Power), CapabilityStatus::Unsupported);
        let m = CapabilityMatrix::uniform(CapabilityStatus::Mocked);
        assert!(
            Capability::ALL
                .iter()
                .all(|c| m.status(*c) == CapabilityStatus::Mocked)
        );
    }

    #[test]
    fn events_fold_into_state() {
        let mut state = HardwareState {
            power: PowerState {
                battery_percent: 10,
                charging: false,
            },
            display: DisplayState {
                width: 1,
                height: 2,
                scale: 1.0,
                orientation: Orientation::Portrait,
                brightness_percent: 50,
                rotation_locked: false,
            },
            input: InputState {
                touch: true,
                rotation: true,
            },
            network: NetworkState {
                wifi: WifiState::Disconnected,
            },
            bluetooth: BluetoothState {
                present: true,
                enabled: false,
            },
            audio: AudioState {
                route: AudioRoute::Speaker,
                available: AudioRouteAvailability::ALL,
            },
            telephony: TelephonyState {
                registration: ModemRegistration::Searching,
                sim: SimState::Absent,
            },
            lifecycle: LifecycleState::default(),
        };
        HardwareEvent::NetworkChanged(NetworkState {
            wifi: WifiState::Connected,
        })
        .apply_to(&mut state);
        assert_eq!(state.network.wifi, WifiState::Connected);
    }

    #[test]
    fn rotation_lock_defaults_off_in_older_snapshots_and_has_a_command() {
        let old =
            r#"{"width":1,"height":2,"scale":1.0,"orientation":"portrait","brightness_percent":5}"#;
        let display: DisplayState = serde_json::from_str(old).unwrap();
        assert!(!display.rotation_locked);
        assert_eq!(
            serde_json::to_string(&HardwareCommand::SetRotationLock { locked: true }).unwrap(),
            r#"{"command":"set-rotation-lock","locked":true}"#
        );
    }

    #[test]
    fn device_lists_sort_paired_connected_then_name() {
        let device = |id: &str, name: Option<&str>, paired, connected| BluetoothDevice {
            id: id.to_owned(),
            name: name.map(str::to_owned),
            paired,
            connected,
            kind: BluetoothDeviceKind::Other,
            signal_bars: None,
        };
        let mut devices = vec![
            device("d", None, false, false),
            device("c", Some("zeta"), false, false),
            device("b", Some("Beta"), true, false),
            device("a", Some("alpha"), true, true),
        ];
        sort_bluetooth_devices(&mut devices);
        let ids: Vec<&str> = devices.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "d"]);
        let json = serde_json::to_string(&devices[3]).unwrap();
        assert_eq!(json, r#"{"id":"d","paired":false,"connected":false}"#);
    }

    #[test]
    fn session_summary_events_carry_no_names_ids_or_passkeys() {
        let session = BluetoothSession {
            discovery: BluetoothDiscovery {
                active: true,
                remaining_ms: Some(1000),
            },
            devices: vec![
                BluetoothDevice {
                    id: "bt-1".into(),
                    name: Some("Secret Buds".into()),
                    paired: false,
                    connected: false,
                    kind: BluetoothDeviceKind::Audio,
                    signal_bars: Some(3),
                },
                BluetoothDevice {
                    id: "bt-2".into(),
                    name: None,
                    paired: true,
                    connected: true,
                    kind: BluetoothDeviceKind::Other,
                    signal_bars: None,
                },
            ],
            pairing: Some(BluetoothPairing {
                device: "bt-1".into(),
                phase: PairingPhase::ConfirmPasskey { passkey: 123_456 },
            }),
            operation: None,
        };
        let event = HardwareEvent::BluetoothSessionChanged(session.summary());
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"event":"bluetooth-session-changed","discovering":true,"available":1,"paired":1,"connected":1,"pairing":{"stage":"confirm-passkey"}}"#
        );
        assert!(!json.contains("bt-1") && !json.contains("Secret") && !json.contains("123"));
        assert_eq!(event.capability(), Capability::Bluetooth);
        let failed = PairingPhase::Failed {
            reason: PairingFailure::TimedOut,
        };
        assert_eq!(
            serde_json::to_string(&failed.stage()).unwrap(),
            r#"{"stage":"failed","reason":"timed-out"}"#
        );
        assert!(failed.is_finished() && !failed.awaits_reply());
        assert!(PairingPhase::Authorize.awaits_reply());
        // The full session (for the prompt) does carry the passkey.
        let full = serde_json::to_string(&session).unwrap();
        assert!(full.contains(r#""phase":{"phase":"confirm-passkey","passkey":123456}"#));
        let back: BluetoothSession = serde_json::from_str(&full).unwrap();
        assert_eq!(back, session);
    }

    #[test]
    fn bluetooth_commands_have_a_kebab_wire_form_and_default_to_unsupported() {
        let command = BluetoothCommand::Pair {
            device: "bt-1".into(),
        };
        assert_eq!(
            serde_json::to_string(&command).unwrap(),
            r#"{"command":"pair","device":"bt-1"}"#
        );
        struct Bare;
        impl Bluetooth for Bare {
            fn bluetooth_state(&self) -> BluetoothState {
                BluetoothState {
                    present: false,
                    enabled: false,
                }
            }
            fn set_bluetooth_enabled(&self, _: bool) -> Result<BluetoothState, HardwareError> {
                Err(HardwareError::unsupported(Capability::Bluetooth))
            }
        }
        for command in [
            BluetoothCommand::StartDiscovery,
            BluetoothCommand::ReplyPairing { accept: true },
            command,
        ] {
            assert_eq!(
                command.apply(&Bare),
                Err(HardwareError::unsupported(Capability::Bluetooth)),
                "{}",
                command.name()
            );
        }
        assert_eq!(signal_bars_from_rssi(-50), 4);
        assert_eq!(signal_bars_from_rssi(-75), 2);
        assert_eq!(signal_bars_from_rssi(-100), 0);
    }
}
