//! Linux composition for the admitted Google Pixel 9a target.
//!
//! This crate is the physical-target seam, not a product-facing hardware API.
//! It owns the target-local composition of standard Linux services and maps
//! their richer service contracts onto [`ferrix_hardware_api`]. Product code
//! sees only the returned trait-object bundle; it never sees a board name,
//! service backend, D-Bus object, or kernel interface.

use std::fmt;
use std::sync::Arc;

use ferrix_hardware_api::{
    Audio, Bluetooth, Capability, CapabilityMatrix, CapabilityStatus, Display, EventSource, Input,
    Lifecycle, Network, Power, Telephony,
};

/// Stable id of the exact admitted physical target.
pub const TARGET_ID: &str = "google-pixel-9a-tegu-gxq96-mp1-0";
/// Exact root compatible/model strings emitted by the pinned `zumapro-tegu-mp`
/// device-tree source.  The vendor strings are intentionally case-sensitive;
/// a generic `tegu` substring is not sufficient to admit another board or a
/// QEMU guest into this target composition.
#[cfg(target_os = "linux")]
const DEVICE_TREE_COMPATIBLE: &[u8] = b"google,ZUMA PRO TEGU";
#[cfg(target_os = "linux")]
const DEVICE_TREE_MODEL: &[u8] = b"ZUMA PRO TEGU MP on ZUMA PRO";

/// Errors raised while composing a target adapter set.
///
/// `Unavailable` is intentionally available on every host so callers can
/// report a typed result when a Linux-only target is selected elsewhere.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectError {
    Unavailable {
        target: &'static str,
        reason: String,
    },
    Failed {
        capability: Capability,
        reason: String,
    },
}

impl ConnectError {
    #[cfg(target_os = "linux")]
    fn failed(capability: Capability, reason: impl Into<String>) -> Self {
        Self::Failed {
            capability,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { target, reason } => {
                write!(formatter, "target {target} is unavailable: {reason}")
            }
            Self::Failed { capability, reason } => {
                write!(formatter, "failed to connect {capability}: {reason}")
            }
        }
    }
}

impl std::error::Error for ConnectError {}

/// Target-neutral trait objects used by the session/bootstrap layer.
#[derive(Clone)]
pub struct HardwareBundle {
    pub power: Arc<dyn Power>,
    pub display: Arc<dyn Display>,
    pub input: Arc<dyn Input>,
    pub network: Arc<dyn Network>,
    pub bluetooth: Arc<dyn Bluetooth>,
    pub audio: Arc<dyn Audio>,
    pub telephony: Arc<dyn Telephony>,
    pub lifecycle: Arc<dyn Lifecycle>,
    pub events: Arc<dyn EventSource>,
}

/// Returns the conservative admission matrix for this composition seam.
///
/// Building the composition is not physical-device evidence. The checked-in
/// board matrix therefore remains unsupported until the exact target has
/// target-specific Linux evidence for each capability.
pub fn capability_matrix() -> CapabilityMatrix {
    CapabilityMatrix::uniform(CapabilityStatus::Unsupported)
}

/// Connects the target's Linux services and returns a target-neutral bundle.
#[cfg(not(target_os = "linux"))]
pub fn connect() -> Result<HardwareBundle, ConnectError> {
    Err(ConnectError::Unavailable {
        target: TARGET_ID,
        reason: "the physical composition requires a Linux host".to_owned(),
    })
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread;

    use ferrix_audio_wireplumber::WirePlumberAudio;
    use ferrix_bluetooth_bluez::BluezBluetooth;
    use ferrix_hardware_api::{
        Audio, AudioRoute, AudioRouteAvailability, AudioState, Bluetooth, BluetoothState,
        Capability, Display, DisplayState, EventSource, HardwareError, HardwareEvent, Input,
        InputState, Lifecycle, LifecycleState, ModemRegistration, Network,
        NetworkState as HardwareNetworkState, Orientation, Power, SimState, SuspendOutcome,
        Telephony, TelephonyState, WifiState,
    };
    use ferrix_network_adapter_api::NetworkBackend;
    use ferrix_network_api::{
        AdapterKind, AdapterPresence, NetworkState as RichNetworkState, RadioState,
    };
    use ferrix_network_nm::NetworkManagerBackend;
    use ferrix_power_upower::UpowerPower;
    use ferrix_telephony_adapter_api::{BackendSnapshot, ModemBackend};
    use ferrix_telephony_api::{
        ModemState as RichModemState, Registration, SimState as RichSimState,
    };
    use ferrix_telephony_mm::ModemManagerBackend;

    use super::{ConnectError, HardwareBundle};

    #[derive(Default)]
    struct EventHub {
        subscribers: Mutex<Vec<Sender<HardwareEvent>>>,
    }

    impl EventHub {
        fn publish(&self, event: HardwareEvent) {
            lock(&self.subscribers).retain(|subscriber| subscriber.send(event).is_ok());
        }
    }

    impl EventSource for EventHub {
        fn subscribe(&self, sender: Sender<HardwareEvent>) {
            lock(&self.subscribers).push(sender);
        }
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn spawn_named<F>(
        name: &'static str,
        capability: Capability,
        task: F,
    ) -> Result<(), ConnectError>
    where
        F: FnOnce() + Send + 'static,
    {
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(task)
            .map(|_| ())
            .map_err(|error| ConnectError::failed(capability, error.to_string()))
    }

    fn bridge_hardware_events(
        source: Arc<dyn EventSource>,
        events: Arc<EventHub>,
        name: &'static str,
        capability: Capability,
    ) -> Result<(), ConnectError> {
        let (sender, receiver) = mpsc::channel();
        source.subscribe(sender);
        spawn_named(name, capability, move || forward_events(receiver, events))
    }

    fn forward_events(receiver: Receiver<HardwareEvent>, events: Arc<EventHub>) {
        while let Ok(event) = receiver.recv() {
            events.publish(event);
        }
    }

    /// Fallback implementation used when an optional Linux service is not
    /// present yet.  A physical bring-up must be able to start the Rust OS
    /// with, for example, ModemManager absent while the Wi-Fi and Bluetooth
    /// services are being debugged.  These values are deliberately boring and
    /// every mutating operation returns `Unsupported`; the target capability
    /// matrix remains unsupported until evidence promotes it.
    #[derive(Default)]
    struct UnavailableHardware;

    impl Power for UnavailableHardware {
        fn power_state(&self) -> ferrix_hardware_api::PowerState {
            ferrix_hardware_api::PowerState {
                battery_percent: 0,
                charging: false,
            }
        }
    }

    impl Display for UnavailableHardware {
        fn display_state(&self) -> DisplayState {
            DisplayState {
                width: 0,
                height: 0,
                scale: 1.0,
                orientation: Orientation::Portrait,
                brightness_percent: 0,
                rotation_locked: false,
            }
        }

        fn set_brightness(&self, _percent: u8) -> Result<DisplayState, HardwareError> {
            Err(HardwareError::unsupported(Capability::Display))
        }
    }

    impl Input for UnavailableHardware {
        fn input_state(&self) -> InputState {
            InputState {
                touch: false,
                rotation: false,
            }
        }
    }

    impl Network for UnavailableHardware {
        fn network_state(&self) -> HardwareNetworkState {
            HardwareNetworkState {
                wifi: WifiState::Disabled,
            }
        }

        fn set_wifi_enabled(&self, _enabled: bool) -> Result<HardwareNetworkState, HardwareError> {
            Err(HardwareError::unsupported(Capability::Network))
        }
    }

    impl Bluetooth for UnavailableHardware {
        fn bluetooth_state(&self) -> BluetoothState {
            BluetoothState {
                present: false,
                enabled: false,
            }
        }

        fn set_bluetooth_enabled(&self, _enabled: bool) -> Result<BluetoothState, HardwareError> {
            Err(HardwareError::unsupported(Capability::Bluetooth))
        }
    }

    impl Audio for UnavailableHardware {
        fn audio_state(&self) -> AudioState {
            AudioState {
                route: AudioRoute::Speaker,
                available: AudioRouteAvailability::default(),
            }
        }

        fn set_audio_route(&self, _route: AudioRoute) -> Result<AudioState, HardwareError> {
            Err(HardwareError::unsupported(Capability::Audio))
        }
    }

    impl Telephony for UnavailableHardware {
        fn telephony_state(&self) -> TelephonyState {
            TelephonyState {
                registration: ModemRegistration::Unregistered,
                sim: SimState::Absent,
            }
        }
    }

    impl Lifecycle for UnavailableHardware {
        fn lifecycle_state(&self) -> LifecycleState {
            LifecycleState::default()
        }

        fn suspend(&self) -> Result<SuspendOutcome, HardwareError> {
            Err(HardwareError::unsupported(Capability::Lifecycle))
        }
    }

    impl EventSource for UnavailableHardware {
        fn subscribe(&self, _sender: Sender<HardwareEvent>) {}
    }

    struct NetworkAdapter {
        backend: Arc<dyn NetworkBackend>,
        state: Mutex<HardwareNetworkState>,
        events: Arc<EventHub>,
    }

    impl NetworkAdapter {
        fn connect(
            backend: Arc<dyn NetworkBackend>,
            events: Arc<EventHub>,
        ) -> Result<Arc<Self>, ConnectError> {
            let initial = backend
                .snapshot()
                .map_err(|error| ConnectError::failed(Capability::Network, error.to_string()))?;
            let adapter = Arc::new(Self {
                backend,
                state: Mutex::new(map_network_snapshot(&initial)),
                events,
            });
            adapter.start_watcher()?;
            Ok(adapter)
        }

        fn start_watcher(self: &Arc<Self>) -> Result<(), ConnectError> {
            let (sender, receiver) = mpsc::channel();
            self.backend.subscribe(sender);
            let weak = Arc::downgrade(self);
            spawn_named(
                "ferrix-target-pixel9a-network",
                Capability::Network,
                move || {
                    while receiver.recv().is_ok() {
                        let Some(adapter) = weak.upgrade() else {
                            break;
                        };
                        let _ = adapter.refresh();
                    }
                },
            )
        }

        fn refresh(&self) -> Result<HardwareNetworkState, HardwareError> {
            let snapshot = self
                .backend
                .snapshot()
                .map_err(|error| network_error(error.to_string()))?;
            let mapped = map_network_snapshot(&snapshot);
            let changed = {
                let mut state = lock(&self.state);
                if *state == mapped {
                    false
                } else {
                    *state = mapped;
                    true
                }
            };
            if changed {
                self.events.publish(HardwareEvent::NetworkChanged(mapped));
            }
            Ok(mapped)
        }
    }

    impl Network for NetworkAdapter {
        fn network_state(&self) -> HardwareNetworkState {
            *lock(&self.state)
        }

        fn set_wifi_enabled(&self, enabled: bool) -> Result<HardwareNetworkState, HardwareError> {
            self.backend
                .set_radio(enabled)
                .map_err(|error| network_error(error.to_string()))?;
            self.refresh()
        }
    }

    fn map_network_snapshot(snapshot: &RichNetworkState) -> HardwareNetworkState {
        let wifi = match (&snapshot.adapter, &snapshot.radio) {
            (
                AdapterPresence::Present {
                    kind: AdapterKind::Wifi,
                },
                RadioState::Enabled,
            ) if snapshot.connection.is_connected() => WifiState::Connected,
            (
                AdapterPresence::Present {
                    kind: AdapterKind::Wifi,
                },
                RadioState::Enabled,
            ) => WifiState::Disconnected,
            _ => WifiState::Disabled,
        };
        HardwareNetworkState { wifi }
    }

    fn network_error(reason: impl Into<String>) -> HardwareError {
        HardwareError::failed(Capability::Network, reason)
    }

    struct TelephonyAdapter {
        backend: Arc<dyn ModemBackend>,
        state: Mutex<TelephonyState>,
        events: Arc<EventHub>,
    }

    impl TelephonyAdapter {
        fn connect(
            backend: Arc<dyn ModemBackend>,
            events: Arc<EventHub>,
        ) -> Result<Arc<Self>, ConnectError> {
            let initial = map_telephony_snapshot(&backend.snapshot());
            let adapter = Arc::new(Self {
                backend,
                state: Mutex::new(initial),
                events,
            });
            adapter.start_watcher()?;
            Ok(adapter)
        }

        fn start_watcher(self: &Arc<Self>) -> Result<(), ConnectError> {
            let (sender, receiver) = mpsc::channel();
            self.backend.subscribe(sender);
            let weak = Arc::downgrade(self);
            spawn_named(
                "ferrix-target-pixel9a-telephony",
                Capability::Telephony,
                move || {
                    while receiver.recv().is_ok() {
                        let Some(adapter) = weak.upgrade() else {
                            break;
                        };
                        adapter.refresh();
                    }
                },
            )
        }

        fn refresh(&self) {
            let mapped = map_telephony_snapshot(&self.backend.snapshot());
            let changed = {
                let mut state = lock(&self.state);
                if *state == mapped {
                    false
                } else {
                    *state = mapped;
                    true
                }
            };
            if changed {
                self.events.publish(HardwareEvent::TelephonyChanged(mapped));
            }
        }
    }

    impl Telephony for TelephonyAdapter {
        fn telephony_state(&self) -> TelephonyState {
            *lock(&self.state)
        }
    }

    fn map_telephony_snapshot(snapshot: &BackendSnapshot) -> TelephonyState {
        let modem_present = !matches!(
            snapshot.modem,
            RichModemState::Absent | RichModemState::Disabled | RichModemState::Failed { .. }
        );
        let registration = if !modem_present {
            ModemRegistration::Unregistered
        } else {
            match snapshot.registration {
                Registration::Registered { .. } => ModemRegistration::Registered,
                Registration::Searching => ModemRegistration::Searching,
                Registration::Denied | Registration::EmergencyOnly => ModemRegistration::Denied,
                Registration::Unknown => ModemRegistration::Unregistered,
            }
        };
        let sim = if !modem_present {
            SimState::Absent
        } else {
            match snapshot.sim {
                RichSimState::Ready => SimState::Present,
                RichSimState::Locked { .. } | RichSimState::Blocked => SimState::Locked,
                RichSimState::Absent | RichSimState::Failed { .. } => SimState::Absent,
            }
        };
        TelephonyState { registration, sim }
    }

    struct Backlight {
        brightness: PathBuf,
        max: u32,
    }

    struct LinuxDisplay {
        state: Mutex<DisplayState>,
        backlight: Option<Backlight>,
        events: Arc<EventHub>,
    }

    impl LinuxDisplay {
        fn new(events: Arc<EventHub>) -> Self {
            let (width, height) = display_geometry();
            let backlight = discover_backlight();
            let brightness_percent = backlight
                .as_ref()
                .and_then(read_brightness_percent)
                .unwrap_or(100);
            let orientation = if height >= width {
                Orientation::Portrait
            } else {
                Orientation::Landscape
            };
            Self {
                state: Mutex::new(DisplayState {
                    width,
                    height,
                    scale: 1.0,
                    orientation,
                    brightness_percent,
                    rotation_locked: false,
                }),
                backlight,
                events,
            }
        }
    }

    impl Display for LinuxDisplay {
        fn display_state(&self) -> DisplayState {
            *lock(&self.state)
        }

        fn set_brightness(&self, percent: u8) -> Result<DisplayState, HardwareError> {
            if percent > 100 {
                return Err(HardwareError::invalid(
                    Capability::Display,
                    format!("brightness must be 0..=100 (got {percent})"),
                ));
            }
            let Some(backlight) = self.backlight.as_ref() else {
                return Err(HardwareError::unsupported(Capability::Display));
            };
            let value =
                u32::try_from(u64::from(percent) * u64::from(backlight.max) / u64::from(100_u8))
                    .unwrap_or(backlight.max);
            fs::write(&backlight.brightness, value.to_string()).map_err(|error| {
                HardwareError::failed(
                    Capability::Display,
                    format!("could not write backlight brightness: {error}"),
                )
            })?;
            let display = {
                let mut state = lock(&self.state);
                state.brightness_percent = percent;
                *state
            };
            self.events.publish(HardwareEvent::DisplayChanged(display));
            Ok(display)
        }
    }

    /// Geometry of the built-in panel: the largest mode of a connected DSI/eDP
    /// connector, else of any connected connector. Disconnected outputs are
    /// ignored; the tegu DisplayPort connector advertises 4096x2160 modes even
    /// with nothing attached, which made the panel look like a landscape screen.
    fn display_geometry() -> (u32, u32) {
        let Ok(entries) = fs::read_dir("/sys/class/drm") else {
            return (0, 0);
        };
        let mut connected = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let status = fs::read_to_string(path.join("status")).unwrap_or_default();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Writeback connectors report "connected" with 4096x2160 modes but
            // are capture sinks, not displays.
            if status.trim() != "connected" || name.contains("-Writeback-") {
                continue;
            }
            let Ok(contents) = fs::read_to_string(path.join("modes")) else {
                continue;
            };
            connected.push((
                name,
                contents.lines().filter_map(parse_mode).collect::<Vec<_>>(),
            ));
        }
        select_panel_geometry(&connected)
    }

    fn select_panel_geometry(connectors: &[(String, Vec<(u32, u32)>)]) -> (u32, u32) {
        let internal = |name: &str| name.contains("-DSI-") || name.contains("-eDP-");
        let largest = |modes: &mut dyn Iterator<Item = &(u32, u32)>| {
            modes
                .copied()
                .max_by_key(|mode| u64::from(mode.0) * u64::from(mode.1))
        };
        largest(
            &mut connectors
                .iter()
                .filter(|(name, _)| internal(name))
                .flat_map(|(_, modes)| modes.iter()),
        )
        .or_else(|| largest(&mut connectors.iter().flat_map(|(_, modes)| modes.iter())))
        .unwrap_or((0, 0))
    }

    /// `WIDTHxHEIGHT`, optionally followed by `@refresh...` (the exynos DRM
    /// driver lists `1080x2424@60:60`).
    fn parse_mode(mode: &str) -> Option<(u32, u32)> {
        let mode = mode.trim();
        let mode = mode.split_once('@').map_or(mode, |(size, _)| size);
        let (width, height) = mode.split_once('x')?;
        let width = width.parse().ok()?;
        let height = height.parse().ok()?;
        (width > 0 && height > 0).then_some((width, height))
    }

    fn discover_backlight() -> Option<Backlight> {
        let entries = fs::read_dir("/sys/class/backlight").ok()?;
        let mut paths = entries
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        paths.into_iter().find_map(|path| {
            let brightness = path.join("brightness");
            let maximum = path.join("max_brightness");
            let max = fs::read_to_string(maximum).ok()?.trim().parse().ok()?;
            (max > 0 && brightness.is_file()).then_some(Backlight { brightness, max })
        })
    }

    fn read_brightness_percent(backlight: &Backlight) -> Option<u8> {
        let current: u32 = fs::read_to_string(&backlight.brightness)
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let percent =
            u64::from(current.min(backlight.max)) * u64::from(100_u8) / u64::from(backlight.max);
        u8::try_from(percent).ok()
    }

    struct LinuxInput {
        state: InputState,
    }

    impl LinuxInput {
        fn new() -> Self {
            Self {
                state: InputState {
                    touch: has_touch_device(),
                    rotation: has_rotation_sensor(),
                },
            }
        }
    }

    impl Input for LinuxInput {
        fn input_state(&self) -> InputState {
            self.state
        }
    }

    fn has_touch_device() -> bool {
        let Ok(entries) = fs::read_dir("/sys/class/input") else {
            return false;
        };
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("event") {
                return false;
            }
            let device_name = entry.path().join("device/name");
            let Ok(name) = fs::read_to_string(device_name) else {
                return false;
            };
            let name = name.to_ascii_lowercase();
            name.contains("touch") || name.contains("digitizer")
        })
    }

    fn has_rotation_sensor() -> bool {
        let Ok(entries) = fs::read_dir("/sys/bus/iio/devices") else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            path.join("in_accel_x_raw").is_file() || path.join("in_anglvel_x_raw").is_file()
        })
    }

    fn verify_device_tree() -> Result<(), ConnectError> {
        let path = "/proc/device-tree/compatible";
        let compatible = fs::read(path).map_err(|error| ConnectError::Unavailable {
            target: super::TARGET_ID,
            reason: format!("cannot read {path}: {error}"),
        })?;
        let tegu = compatible
            .split(|byte| *byte == 0)
            .any(|entry| entry == super::DEVICE_TREE_COMPATIBLE);
        if !tegu {
            return Err(ConnectError::Unavailable {
                target: super::TARGET_ID,
                reason: format!(
                    "{path} does not identify {}",
                    String::from_utf8_lossy(super::DEVICE_TREE_COMPATIBLE)
                ),
            });
        }
        let model_path = "/proc/device-tree/model";
        let model = fs::read(model_path).map_err(|error| ConnectError::Unavailable {
            target: super::TARGET_ID,
            reason: format!("cannot read {model_path}: {error}"),
        })?;
        if model
            .split(|byte| *byte == 0)
            .any(|entry| entry == super::DEVICE_TREE_MODEL)
        {
            Ok(())
        } else {
            Err(ConnectError::Unavailable {
                target: super::TARGET_ID,
                reason: format!(
                    "{model_path} does not identify {}",
                    String::from_utf8_lossy(super::DEVICE_TREE_MODEL)
                ),
            })
        }
    }

    struct LinuxLifecycle {
        state: Mutex<LifecycleState>,
        events: Arc<EventHub>,
    }

    impl LinuxLifecycle {
        fn new(events: Arc<EventHub>) -> Self {
            Self {
                state: Mutex::new(LifecycleState::default()),
                events,
            }
        }
    }

    impl Lifecycle for LinuxLifecycle {
        fn lifecycle_state(&self) -> LifecycleState {
            *lock(&self.state)
        }

        fn suspend(&self) -> Result<SuspendOutcome, HardwareError> {
            // The target image uses OpenRC, so systemctl is neither required
            // nor a valid lifecycle dependency. The kernel power-state ABI is
            // the shared Linux interface and returns only after resume.
            let states = fs::read_to_string("/sys/power/state").map_err(|error| {
                HardwareError::failed(
                    Capability::Lifecycle,
                    format!("could not read /sys/power/state: {error}"),
                )
            })?;
            let mode = states
                .split_whitespace()
                .find(|state| matches!(*state, "mem" | "freeze"))
                .ok_or_else(|| {
                    HardwareError::failed(
                        Capability::Lifecycle,
                        format!("/sys/power/state exposes no supported suspend mode: {states:?}"),
                    )
                })?;
            fs::write("/sys/power/state", format!("{mode}\n")).map_err(|error| {
                HardwareError::failed(
                    Capability::Lifecycle,
                    format!("could not enter Linux suspend mode {mode}: {error}"),
                )
            })?;
            let outcome = SuspendOutcome::Resumed;
            let lifecycle = {
                let mut state = lock(&self.state);
                state.suspend_count = state.suspend_count.saturating_add(1);
                state.last_suspend = Some(outcome);
                *state
            };
            self.events
                .publish(HardwareEvent::Suspended { outcome, lifecycle });
            Ok(outcome)
        }
    }

    pub(super) fn connect() -> Result<HardwareBundle, ConnectError> {
        verify_device_tree()?;
        let events = Arc::new(EventHub::default());
        let unavailable = Arc::new(UnavailableHardware);

        // Optional daemons must not make the target-neutral core unbootable.
        // A missing service gets an explicit unsupported adapter and can be
        // brought up independently during physical Linux debugging.
        let power_result = UpowerPower::connect();
        let power_events = power_result.as_ref().ok().map(Arc::clone);
        let power: Arc<dyn Power> = power_result
            .map(|power| power as Arc<dyn Power>)
            .unwrap_or_else(|_| unavailable.clone());

        let bluetooth_result = BluezBluetooth::connect();
        let bluetooth_events = bluetooth_result.as_ref().ok().map(Arc::clone);
        let bluetooth: Arc<dyn Bluetooth> = bluetooth_result
            .map(|bluetooth| bluetooth as Arc<dyn Bluetooth>)
            .unwrap_or_else(|_| unavailable.clone());

        let audio_result = WirePlumberAudio::connect();
        let audio_events = audio_result.as_ref().ok().map(Arc::clone);
        let audio: Arc<dyn Audio> = audio_result
            .map(|audio| audio as Arc<dyn Audio>)
            .unwrap_or_else(|_| unavailable.clone());

        let network: Arc<dyn Network> = match NetworkManagerBackend::connect() {
            Ok(network_backend) => {
                let network_backend: Arc<dyn NetworkBackend> = network_backend;
                match NetworkAdapter::connect(network_backend, Arc::clone(&events)) {
                    Ok(network) => network,
                    Err(_) => unavailable.clone(),
                }
            }
            Err(_) => unavailable.clone(),
        };

        let telephony: Arc<dyn Telephony> = match ModemManagerBackend::connect() {
            Ok(modem_backend) => {
                let modem_backend: Arc<dyn ModemBackend> = Arc::new(modem_backend);
                match TelephonyAdapter::connect(modem_backend, Arc::clone(&events)) {
                    Ok(telephony) => telephony,
                    Err(_) => unavailable.clone(),
                }
            }
            Err(_) => unavailable.clone(),
        };

        let display = Arc::new(LinuxDisplay::new(Arc::clone(&events)));
        let input = Arc::new(LinuxInput::new());
        let lifecycle = Arc::new(LinuxLifecycle::new(Arc::clone(&events)));

        if let Some(power_events) = power_events {
            let power_events: Arc<dyn EventSource> = power_events;
            bridge_hardware_events(
                power_events,
                Arc::clone(&events),
                "ferrix-target-pixel9a-power-events",
                Capability::Power,
            )?;
        }
        if let Some(bluetooth_events) = bluetooth_events {
            let bluetooth_events: Arc<dyn EventSource> = bluetooth_events;
            bridge_hardware_events(
                bluetooth_events,
                Arc::clone(&events),
                "ferrix-target-pixel9a-bluetooth-events",
                Capability::Bluetooth,
            )?;
        }
        if let Some(audio_events) = audio_events {
            let audio_events: Arc<dyn EventSource> = audio_events;
            bridge_hardware_events(
                audio_events,
                Arc::clone(&events),
                "ferrix-target-pixel9a-audio-events",
                Capability::Audio,
            )?;
        }
        Ok(HardwareBundle {
            power,
            display,
            input,
            network,
            bluetooth,
            audio,
            telephony,
            lifecycle,
            events,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ferrix_hardware_api::{ModemRegistration, SimState, WifiState};
        use ferrix_network_api::{AdapterKind, AdapterPresence, ApId, ConnectionState, RadioState};
        use ferrix_telephony_api::{ModemState, Registration, SimState as RichSimState};

        #[test]
        fn panel_geometry_prefers_the_connected_internal_panel() {
            let connectors = vec![
                ("card0-DP-1".to_owned(), vec![(4096, 2160), (1920, 1080)]),
                ("card0-DSI-1".to_owned(), vec![(1080, 2424)]),
            ];
            assert_eq!(select_panel_geometry(&connectors), (1080, 2424));
            let external_only = vec![("card0-DP-1".to_owned(), vec![(1920, 1080)])];
            assert_eq!(select_panel_geometry(&external_only), (1920, 1080));
            assert_eq!(select_panel_geometry(&[]), (0, 0));
        }

        #[test]
        fn modes_with_refresh_suffixes_parse() {
            assert_eq!(parse_mode("1080x2424@60:60"), Some((1080, 2424)));
            assert_eq!(parse_mode("4096x2160"), Some((4096, 2160)));
            assert_eq!(parse_mode("bogus"), None);
        }

        #[test]
        fn rich_network_states_fold_into_coarse_wifi_states() {
            let mut snapshot = RichNetworkState {
                adapter: AdapterPresence::Present {
                    kind: AdapterKind::Wifi,
                },
                radio: RadioState::Enabled,
                connection: ConnectionState::Connected {
                    ap: ApId::new("ap-test"),
                },
                ..RichNetworkState::default()
            };
            assert_eq!(map_network_snapshot(&snapshot).wifi, WifiState::Connected);

            snapshot.connection = ConnectionState::Disconnected;
            assert_eq!(
                map_network_snapshot(&snapshot).wifi,
                WifiState::Disconnected
            );

            snapshot.radio = RadioState::Disabled;
            assert_eq!(map_network_snapshot(&snapshot).wifi, WifiState::Disabled);
        }

        #[test]
        fn rich_modem_states_fold_into_coarse_telephony_states() {
            let snapshot = BackendSnapshot {
                modem: ModemState::Ready,
                sim: RichSimState::Ready,
                registration: Registration::Registered { roaming: false },
                ..BackendSnapshot::default()
            };
            assert_eq!(
                map_telephony_snapshot(&snapshot),
                TelephonyState {
                    registration: ModemRegistration::Registered,
                    sim: SimState::Present,
                }
            );

            let locked = BackendSnapshot {
                modem: ModemState::Ready,
                sim: RichSimState::Locked {
                    lock: ferrix_telephony_api::SimLock::Pin,
                    attempts_left: 2,
                },
                registration: Registration::Searching,
                ..BackendSnapshot::default()
            };
            assert_eq!(
                map_telephony_snapshot(&locked),
                TelephonyState {
                    registration: ModemRegistration::Searching,
                    sim: SimState::Locked,
                }
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub fn connect() -> Result<HardwareBundle, ConnectError> {
    linux::connect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_id_is_the_admitted_exact_target() {
        assert_eq!(TARGET_ID, "google-pixel-9a-tegu-gxq96-mp1-0");
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_hosts_get_a_typed_unavailable_error() {
        assert!(matches!(
            connect(),
            Err(ConnectError::Unavailable { target, .. }) if target == TARGET_ID
        ));
    }
}
