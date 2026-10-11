//! Settings state that comes from `ferrix-core` (Display, Sound, Bluetooth)
//! and the
//! SIM/mobile half of the telephony snapshot (Mobile network). Pure state,
//! like `model.rs`: the session owner feeds [`Model::apply_core`] and
//! [`Model::set_bluetooth_devices`] and drains `Model::pending`.
//!
//! Every control names why it cannot act (core unavailable, capability
//! unsupported on this target, no controller, no SIM, ...) instead of
//! imitating success; the daemons stay authoritative.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_hardware_api::{
    Capability, CapabilityMatrix, CapabilityStatus, HardwareEvent, HardwareState,
};
use ferrix_protocol::Feed as CoreFeed;
use ferrix_telephony_api::{
    DataBearer, ModemState, RadioPower, Registration, SimState, TelephonyFeature,
};

use crate::bluetooth::DeviceList;
use crate::model::{Action, Link, Model};

/// Why the Bluetooth power toggle cannot act, in the order checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BluetoothBlock {
    CoreUnavailable,
    Unsupported,
    NoController,
}

impl BluetoothBlock {
    pub fn reason(self) -> &'static str {
        match self {
            BluetoothBlock::CoreUnavailable => "core \u{b7} unavailable",
            BluetoothBlock::Unsupported => "bluetooth \u{b7} not supported on this target",
            BluetoothBlock::NoController => "bluetooth \u{b7} no controller attached",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BluetoothBlock::CoreUnavailable => "core-unavailable",
            BluetoothBlock::Unsupported => "unsupported",
            BluetoothBlock::NoController => "no-controller",
        }
    }
}

/// Why the mobile-data toggle cannot act, in the order checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataBlock {
    TelephonyUnavailable,
    NoModem,
    RadioOff,
    NoSim,
    SimFailed,
    SimLocked,
    Unsupported,
}

impl DataBlock {
    pub fn reason(self) -> &'static str {
        match self {
            DataBlock::TelephonyUnavailable => "telephony \u{b7} unavailable",
            DataBlock::NoModem => "modem \u{b7} absent",
            DataBlock::RadioOff => "cellular radio \u{b7} off",
            DataBlock::NoSim => "sim \u{b7} absent",
            DataBlock::SimFailed => "sim \u{b7} failed",
            DataBlock::SimLocked => "sim \u{b7} locked",
            DataBlock::Unsupported => "mobile data \u{b7} not supported by this modem backend",
        }
    }
}

fn status_word(status: CapabilityStatus) -> &'static str {
    match status {
        CapabilityStatus::Supported => "supported",
        CapabilityStatus::Mocked => "mock",
        CapabilityStatus::Unsupported => "unsupported",
    }
}

impl Model {
    // -- core feed -----------------------------------------------------------

    /// Folds one core reconnector update. A Bluetooth change marks the
    /// session stale (a fresh snapshot drops it) and, while the Bluetooth
    /// page is shown, queues a re-query.
    pub fn apply_core(&mut self, feed: CoreFeed) -> bool {
        match feed {
            CoreFeed::Snapshot {
                target,
                capabilities,
                state,
                ..
            } => {
                self.core_link = if self.core_lost_once {
                    Link::Reconnected
                } else {
                    Link::Connected
                };
                self.core_lost_once = false;
                self.hw = Some(state);
                self.hw_capabilities = capabilities;
                self.hw_target = Some(target);
                self.bt_devices = DeviceList::Unknown;
                self.invalidate_devices();
            }
            CoreFeed::Event(event) => {
                if let Some(hw) = self.hw.as_mut() {
                    event.apply_to(hw);
                }
                if matches!(
                    event,
                    HardwareEvent::BluetoothChanged(_) | HardwareEvent::BluetoothSessionChanged(_)
                ) {
                    self.invalidate_devices();
                }
            }
            CoreFeed::Disconnected { .. } => {
                self.core_link = Link::Unavailable;
                self.core_lost_once = true;
            }
        }
        true
    }

    fn invalidate_devices(&mut self) {
        self.bt_stale = true;
        self.request_devices_if_shown();
    }

    /// Queues one session query when the Bluetooth page needs it.
    pub fn request_devices_if_shown(&mut self) {
        if self.page == crate::model::Page::Bluetooth
            && (self.bt_devices == DeviceList::Unknown || self.bt_stale)
            && self.bluetooth_block().is_ok()
            && !self.pending.contains(&Action::RefreshBluetoothDevices)
        {
            self.pending.push(Action::RefreshBluetoothDevices);
        }
    }

    pub fn core_label(&self) -> String {
        format!("core \u{b7} {}", self.core_link.name())
    }

    fn core_ready(&self) -> Option<&HardwareState> {
        self.hw
            .as_ref()
            .filter(|_| self.core_link != Link::Unavailable)
    }

    pub fn capability_status(&self, capability: Capability) -> CapabilityStatus {
        self.hw_capabilities.status(capability)
    }

    /// `display · mock · qemu-aarch64`: where a value comes from.
    pub fn source_caption(&self, name: &str, capability: Capability) -> String {
        match (&self.hw_target, self.core_ready()) {
            (Some(target), Some(_)) => format!(
                "{name} \u{b7} {} \u{b7} {target}",
                status_word(self.capability_status(capability))
            ),
            _ => format!("{name} \u{b7} {}", self.core_label()),
        }
    }

    // -- display -------------------------------------------------------------

    /// Why brightness cannot be changed, if it cannot.
    pub fn display_block(&self) -> Result<(), &'static str> {
        if self.core_ready().is_none() {
            return Err("core \u{b7} unavailable");
        }
        if self.capability_status(Capability::Display) == CapabilityStatus::Unsupported {
            return Err("display \u{b7} not supported on this target");
        }
        Ok(())
    }

    /// The brightness shown: the drag preview, else the core's value.
    pub fn brightness(&self) -> Option<u8> {
        self.brightness_drag
            .or_else(|| self.hw.as_ref().map(|hw| hw.display.brightness_percent))
    }

    pub fn drag_brightness(&mut self, percent: u8) {
        if self.display_block().is_ok() {
            self.brightness_drag = Some(percent.min(100));
        }
    }

    /// Commits the dragged value: the core reconciles through
    /// `display-changed`, so the preview is dropped once it is sent.
    pub fn commit_brightness(&mut self) {
        let dragged = self.brightness_drag.take();
        if let Err(reason) = self.display_block() {
            self.last_error = Some(reason.to_owned());
            return;
        }
        if let Some(percent) = dragged {
            self.last_error = None;
            self.pending.push(Action::SetBrightness(percent));
        }
    }

    pub fn rotation_sensor(&self) -> bool {
        self.hw.as_ref().is_some_and(|hw| hw.input.rotation)
    }

    pub fn auto_rotate(&self) -> bool {
        self.hw
            .as_ref()
            .is_some_and(|hw| !hw.display.rotation_locked)
    }

    pub fn rotation_label(&self) -> String {
        match self.hw.as_ref() {
            None => self.core_label(),
            Some(_) if !self.rotation_sensor() => "no rotation sensor on this target".to_owned(),
            Some(hw) if hw.display.rotation_locked => "locked to portrait".to_owned(),
            Some(hw) => format!(
                "follows the rotation sensor \u{b7} {} now",
                hw.display.orientation.name()
            ),
        }
    }

    pub fn toggle_auto_rotate(&mut self) {
        if let Err(reason) = self.display_block() {
            self.last_error = Some(reason.to_owned());
            return;
        }
        if !self.rotation_sensor() {
            self.last_error = Some("rotation \u{b7} no sensor on this target".to_owned());
            return;
        }
        // Auto-rotate on means unlocked.
        self.pending
            .push(Action::SetRotationLock(self.auto_rotate()));
    }

    pub fn display_state_name(&self) -> String {
        match (self.display_block(), self.hw.as_ref()) {
            (Ok(()), Some(hw)) => format!(
                "brightness-{}-rotation-{}",
                hw.display.brightness_percent,
                if hw.display.rotation_locked {
                    "locked"
                } else {
                    "auto"
                }
            ),
            (Err(_), _) | (_, None) => "unavailable".to_owned(),
        }
    }

    // -- sound ---------------------------------------------------------------

    /// Why the output volume and mute cannot be changed, if they cannot. A
    /// target whose audio service reports a level is controllable even while
    /// its matrix still says `unsupported`; the source caption keeps that
    /// status visible instead of upgrading it.
    pub fn sound_block(&self) -> Result<(), &'static str> {
        let Some(hw) = self.core_ready() else {
            return Err("core \u{b7} unavailable");
        };
        if hw.audio.output.is_some() {
            return Ok(());
        }
        if self.capability_status(Capability::Audio) == CapabilityStatus::Unsupported {
            return Err("audio \u{b7} not supported on this target");
        }
        Err("volume \u{b7} not exposed by this target")
    }

    /// The volume shown: the drag preview, else the core's value.
    pub fn volume(&self) -> Option<u8> {
        self.volume_drag.or_else(|| {
            self.hw
                .as_ref()
                .and_then(|hw| hw.audio.output)
                .map(|level| level.volume_percent)
        })
    }

    pub fn muted(&self) -> bool {
        self.hw
            .as_ref()
            .and_then(|hw| hw.audio.output)
            .is_some_and(|level| level.muted)
    }

    pub fn drag_volume(&mut self, percent: u8) {
        if self.sound_block().is_ok() {
            self.volume_drag = Some(percent.min(100));
        }
    }

    /// Commits the dragged value; the core reconciles through
    /// `audio-changed`, so the preview is dropped once it is sent.
    pub fn commit_volume(&mut self) {
        let dragged = self.volume_drag.take();
        if let Err(reason) = self.sound_block() {
            self.last_error = Some(reason.to_owned());
            return;
        }
        if let Some(percent) = dragged {
            self.last_error = None;
            self.pending.push(Action::SetVolume(percent));
        }
    }

    pub fn toggle_mute(&mut self) {
        match self.sound_block() {
            Ok(()) => {
                self.last_error = None;
                self.pending.push(Action::SetMuted(!self.muted()));
            }
            Err(reason) => self.last_error = Some(reason.to_owned()),
        }
    }

    /// The active output route, as the audio service reports it.
    pub fn output_route_label(&self) -> String {
        match self.core_ready() {
            None => self.core_label(),
            Some(hw) => format!("{} \u{b7} set by the audio service", hw.audio.route.name()),
        }
    }

    pub fn sound_summary(&self) -> String {
        match (self.sound_block(), self.hw.as_ref()) {
            (Ok(()), Some(hw)) => match hw.audio.output {
                Some(level) if level.muted => format!("muted \u{b7} {}", hw.audio.route.name()),
                Some(level) => {
                    format!("{}% \u{b7} {}", level.volume_percent, hw.audio.route.name())
                }
                None => self.core_label(),
            },
            (Err(reason), _) => reason.to_owned(),
            (_, None) => self.core_label(),
        }
    }

    pub fn sound_state_name(&self) -> String {
        match self.core_ready().map(|hw| hw.audio.output) {
            None => "unavailable".to_owned(),
            Some(None) => "no-output-level".to_owned(),
            Some(Some(level)) => format!(
                "volume-{}-{}",
                level.volume_percent,
                if level.muted { "muted" } else { "unmuted" }
            ),
        }
    }

    // -- bluetooth -----------------------------------------------------------

    pub fn bluetooth_block(&self) -> Result<(), BluetoothBlock> {
        let Some(hw) = self.core_ready() else {
            return Err(BluetoothBlock::CoreUnavailable);
        };
        if self.capability_status(Capability::Bluetooth) == CapabilityStatus::Unsupported
            && !hw.bluetooth.present
        {
            return Err(BluetoothBlock::Unsupported);
        }
        if !hw.bluetooth.present {
            return Err(BluetoothBlock::NoController);
        }
        Ok(())
    }

    pub fn bluetooth_on(&self) -> bool {
        self.bluetooth_block().is_ok() && self.hw.as_ref().is_some_and(|hw| hw.bluetooth.enabled)
    }

    pub fn toggle_bluetooth(&mut self) {
        match self.bluetooth_block() {
            Ok(()) => self
                .pending
                .push(Action::SetBluetooth(!self.bluetooth_on())),
            Err(block) => self.last_error = Some(block.reason().to_owned()),
        }
    }

    pub fn connected_devices(&self) -> usize {
        self.devices().iter().filter(|d| d.connected).count()
    }

    pub fn bluetooth_summary(&self) -> String {
        match self.bluetooth_block() {
            Err(BluetoothBlock::NoController) => "unavailable \u{b7} no controller".to_owned(),
            Err(block) => block.reason().to_owned(),
            Ok(()) if !self.bluetooth_on() => "off".to_owned(),
            Ok(()) => match &self.bt_devices {
                DeviceList::Loaded { .. } => {
                    format!("on \u{b7} {} connected", self.connected_devices())
                }
                _ => "on".to_owned(),
            },
        }
    }

    pub fn bluetooth_state_name(&self) -> String {
        match self.bluetooth_block() {
            Err(block) => block.name().to_owned(),
            Ok(()) => {
                let power = if self.bluetooth_on() { "on" } else { "off" };
                match &self.bt_devices {
                    DeviceList::Loaded { .. } => {
                        format!("{power}-{}-saved", self.saved_devices().len())
                    }
                    DeviceList::Failed(_) => format!("{power}-devices-failed"),
                    DeviceList::Unknown => power.to_owned(),
                }
            }
        }
    }

    // -- mobile network ------------------------------------------------------

    pub fn sim_label(&self) -> (&'static str, &'static str) {
        match self.tele.as_ref().map(|t| &t.sim) {
            None => ("unknown", "telephony is not connected"),
            Some(SimState::Ready) => ("ready", "SIM card ready"),
            Some(SimState::Absent) => ("absent", "No SIM card inserted"),
            Some(SimState::Locked { .. }) => ("locked", "SIM is locked; unlock it from Phone"),
            Some(SimState::Blocked) => ("blocked", "SIM is blocked; contact the carrier"),
            Some(SimState::Failed { .. }) => ("failed", "The modem reported a SIM failure"),
        }
    }

    pub fn sim_present(&self) -> bool {
        self.tele
            .as_ref()
            .is_some_and(|t| !matches!(t.sim, SimState::Absent | SimState::Failed { .. }))
    }

    pub fn operator_label(&self) -> String {
        let Some(t) = &self.tele else {
            return "telephony \u{b7} not connected".to_owned();
        };
        if !self.sim_present() {
            return "none \u{b7} no SIM".to_owned();
        }
        match (&t.operator, t.registration) {
            (Some(op), Registration::Registered { roaming }) => format!(
                "{} \u{b7} {}{}",
                op.name,
                t.access.name(),
                if roaming { " \u{b7} roaming" } else { "" }
            ),
            (_, registration) => format!("none \u{b7} {}", registration.name()),
        }
    }

    pub fn signal_label(&self) -> String {
        match &self.tele {
            None => "unknown".to_owned(),
            Some(_) if !self.sim_present() => "no service \u{b7} no SIM".to_owned(),
            Some(t) if !t.registration.is_registered() => "no service".to_owned(),
            Some(t) => match t.signal.rssi_dbm {
                Some(dbm) => format!("{} of 4 bars \u{b7} {dbm} dBm", t.signal.bars),
                None => format!("{} of 4 bars", t.signal.bars),
            },
        }
    }

    pub fn signal_bars(&self) -> Option<u8> {
        self.tele
            .as_ref()
            .filter(|t| t.registration.is_registered())
            .map(|t| t.signal.bars)
    }

    pub fn data_block(&self) -> Result<(), DataBlock> {
        let Some(t) = &self.tele else {
            return Err(DataBlock::TelephonyUnavailable);
        };
        if self.tele_link == Link::Unavailable {
            return Err(DataBlock::TelephonyUnavailable);
        }
        if t.modem == ModemState::Absent {
            return Err(DataBlock::NoModem);
        }
        if t.radio == RadioPower::Off {
            return Err(DataBlock::RadioOff);
        }
        match t.sim {
            SimState::Absent => return Err(DataBlock::NoSim),
            SimState::Failed { .. } => return Err(DataBlock::SimFailed),
            SimState::Locked { .. } | SimState::Blocked => return Err(DataBlock::SimLocked),
            SimState::Ready => {}
        }
        if !t.capabilities.contains(&TelephonyFeature::Data)
            || matches!(t.data, DataBearer::Unsupported)
        {
            return Err(DataBlock::Unsupported);
        }
        Ok(())
    }

    /// The mobile-data toggle: refused with the typed reason, otherwise the
    /// bearer request (`ferrix-telephonyd` stays authoritative).
    pub fn toggle_cellular_data(&mut self) {
        // Turning data off is always allowed while a bearer is up.
        if self.cellular_data_on() {
            self.pending.push(Action::SetCellularData(false));
            return;
        }
        match self.data_block() {
            Ok(()) => self.pending.push(Action::SetCellularData(true)),
            Err(block) => self.last_error = Some(block.reason().to_owned()),
        }
    }

    pub fn mobile_summary(&self) -> String {
        match &self.tele {
            None => format!("telephony \u{b7} {}", self.tele_link.name()),
            Some(_) if !self.sim_present() => "no SIM".to_owned(),
            Some(t) => match (&t.operator, t.registration.is_registered()) {
                (Some(op), true) => format!("{} \u{b7} {} bars", op.name, t.signal.bars),
                _ => format!("sim {} \u{b7} no service", t.sim.name()),
            },
        }
    }

    pub fn mobile_state_name(&self) -> String {
        match (self.data_block(), &self.tele) {
            (_, None) => "unavailable".to_owned(),
            (Err(DataBlock::NoSim), _) => "no-sim".to_owned(),
            (Err(DataBlock::SimLocked), _) => "sim-locked".to_owned(),
            (_, Some(_)) => format!(
                "data-{}",
                if self.cellular_data_on() { "on" } else { "off" }
            ),
        }
    }

    // -- root ----------------------------------------------------------------

    pub fn display_summary(&self) -> String {
        match (self.display_block(), self.hw.as_ref()) {
            (Ok(()), Some(hw)) => format!(
                "{}% \u{b7} auto-rotate {}",
                hw.display.brightness_percent,
                if hw.display.rotation_locked {
                    "off"
                } else {
                    "on"
                }
            ),
            (Err(reason), _) => reason.to_owned(),
            (_, None) => self.core_label(),
        }
    }
}

/// Canned core snapshot for the host preview (a mock target, never a device).
pub fn preview_hardware() -> HardwareState {
    use ferrix_hardware_api::{
        AudioRoute, AudioRouteAvailability, AudioState, BluetoothState, DisplayState, InputState,
        LifecycleState, ModemRegistration, NetworkState, Orientation, PowerState, SimState,
        TelephonyState, WifiState,
    };
    HardwareState {
        power: PowerState {
            battery_percent: 75,
            charging: false,
        },
        display: DisplayState {
            width: 1080,
            height: 2340,
            scale: 1.0,
            orientation: Orientation::Portrait,
            brightness_percent: 72,
            rotation_locked: false,
        },
        input: InputState {
            touch: true,
            rotation: true,
        },
        network: NetworkState {
            wifi: WifiState::Connected,
        },
        bluetooth: BluetoothState {
            present: true,
            enabled: true,
        },
        audio: AudioState {
            route: AudioRoute::Speaker,
            available: AudioRouteAvailability::ALL,
            output: Some(ferrix_hardware_api::AudioOutputLevel {
                volume_percent: 40,
                muted: false,
            }),
        },
        telephony: TelephonyState {
            registration: ModemRegistration::Registered,
            sim: SimState::Present,
        },
        lifecycle: LifecycleState::default(),
    }
}

/// A preview capability matrix: every capability mocked.
pub fn preview_capabilities() -> CapabilityMatrix {
    CapabilityMatrix::uniform(CapabilityStatus::Mocked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Page;
    use ferrix_hardware_api::{BluetoothState, DisplayState, Orientation};
    use ferrix_telephony_api::TelephonyState as Tele;
    use ferrix_telephony_protocol::{StateView, Update};

    fn snapshot(state: HardwareState, capabilities: CapabilityMatrix) -> CoreFeed {
        CoreFeed::Snapshot {
            target: "qemu-aarch64".into(),
            profile: "emulator-dev".into(),
            capabilities,
            state,
        }
    }

    fn with_core() -> Model {
        let mut m = Model::new();
        m.apply_core(snapshot(preview_hardware(), preview_capabilities()));
        m
    }

    fn tele(f: impl FnOnce(&mut Tele)) -> Update {
        let mut t = Tele::new();
        t.modem = ModemState::Ready;
        t.sim = SimState::Ready;
        t.registration = Registration::Registered { roaming: false };
        t.operator = Some(ferrix_telephony_api::Operator {
            name: "Ferrix Mock".into(),
            mcc_mnc: None,
        });
        t.access = ferrix_telephony_api::AccessTechnology::Lte;
        t.signal = ferrix_telephony_api::Signal::bars(3);
        t.radio = RadioPower::On;
        t.data = DataBearer::Disconnected;
        t.capabilities = [TelephonyFeature::Data].into_iter().collect();
        f(&mut t);
        Update::Snapshot(StateView::Redacted(t.redacted()))
    }

    fn audio_level(volume_percent: u8, muted: bool) -> HardwareEvent {
        let mut audio = preview_hardware().audio;
        audio.output = Some(ferrix_hardware_api::AudioOutputLevel {
            volume_percent,
            muted,
        });
        HardwareEvent::AudioChanged(audio)
    }

    #[test]
    fn volume_drag_previews_commits_once_and_reconciles_from_core() {
        let mut m = with_core();
        m.show(Page::Sound);
        assert_eq!(m.volume(), Some(40));
        assert_eq!(m.sound_state_name(), "volume-40-unmuted");
        m.drag_volume(65);
        assert_eq!(m.volume(), Some(65), "preview while dragging");
        m.commit_volume();
        assert_eq!(m.pending, vec![Action::SetVolume(65)]);
        assert_eq!(m.volume(), Some(40), "core stays authoritative");
        m.commit_volume();
        assert_eq!(m.pending.len(), 1, "a release without a drag sends nothing");
        m.apply_core(CoreFeed::Event(audio_level(65, false)));
        assert_eq!(m.volume(), Some(65));
        assert_eq!(m.sound_summary(), "65% \u{b7} speaker");
    }

    #[test]
    fn mute_toggles_from_the_reported_state() {
        let mut m = with_core();
        m.toggle_mute();
        assert_eq!(m.pending.pop(), Some(Action::SetMuted(true)));
        assert!(!m.muted(), "nothing changes until the core reports it");
        m.apply_core(CoreFeed::Event(audio_level(40, true)));
        assert!(m.muted());
        assert_eq!(m.sound_state_name(), "volume-40-muted");
        assert_eq!(m.sound_summary(), "muted \u{b7} speaker");
        m.toggle_mute();
        assert_eq!(m.pending.pop(), Some(Action::SetMuted(false)));
    }

    #[test]
    fn sound_controls_name_why_they_cannot_act() {
        // No output level and the matrix says unsupported (the Pixel today
        // until WirePlumber exposes a sink).
        let mut hw = preview_hardware();
        hw.audio.output = None;
        let unsupported =
            preview_capabilities().with(Capability::Audio, CapabilityStatus::Unsupported);
        let mut m = Model::new();
        m.apply_core(snapshot(hw, unsupported.clone()));
        m.drag_volume(10);
        assert_eq!(m.volume_drag, None, "no preview without a level");
        m.toggle_mute();
        m.commit_volume();
        assert!(m.pending.is_empty());
        assert_eq!(
            m.last_error.as_deref(),
            Some("audio \u{b7} not supported on this target")
        );
        assert_eq!(m.sound_state_name(), "no-output-level");

        // Mocked audio without a level (the QEMU profile today).
        let mut m = Model::new();
        m.apply_core(snapshot(hw, preview_capabilities()));
        assert_eq!(
            m.sound_block(),
            Err("volume \u{b7} not exposed by this target")
        );

        // A reported level stays controllable while the matrix still says
        // unsupported, and the caption keeps that status visible.
        let mut m = Model::new();
        m.apply_core(snapshot(preview_hardware(), unsupported));
        m.toggle_mute();
        assert_eq!(m.pending, vec![Action::SetMuted(true)]);
        assert_eq!(
            m.source_caption("audio", Capability::Audio),
            "audio \u{b7} unsupported \u{b7} qemu-aarch64"
        );

        // Losing the core blocks everything.
        m.apply_core(CoreFeed::Disconnected {
            reason: "gone".into(),
        });
        assert_eq!(m.sound_block(), Err("core \u{b7} unavailable"));
        assert_eq!(m.sound_state_name(), "unavailable");
    }

    #[test]
    fn brightness_drag_previews_then_commits_once() {
        let mut m = with_core();
        assert_eq!(m.brightness(), Some(72));
        m.drag_brightness(30);
        assert_eq!(m.brightness(), Some(30), "preview while dragging");
        m.commit_brightness();
        assert_eq!(m.pending, vec![Action::SetBrightness(30)]);
        assert_eq!(m.brightness(), Some(72), "core stays authoritative");
        m.commit_brightness();
        assert_eq!(m.pending.len(), 1, "nothing to commit twice");
        m.apply_core(CoreFeed::Event(HardwareEvent::DisplayChanged(
            DisplayState {
                brightness_percent: 30,
                ..preview_hardware().display
            },
        )));
        assert_eq!(m.brightness(), Some(30));
        assert_eq!(m.display_state_name(), "brightness-30-rotation-auto");
        m.apply_core(CoreFeed::Disconnected {
            reason: "gone".into(),
        });
        m.drag_brightness(10);
        assert_eq!(m.brightness_drag, None, "no preview without the core");
        assert_eq!(m.display_state_name(), "unavailable");
    }

    #[test]
    fn auto_rotate_maps_to_the_lock_and_needs_a_sensor() {
        let mut m = with_core();
        assert!(m.auto_rotate());
        assert!(
            m.rotation_label()
                .starts_with("follows the rotation sensor")
        );
        m.toggle_auto_rotate();
        assert_eq!(m.pending.pop(), Some(Action::SetRotationLock(true)));
        m.apply_core(CoreFeed::Event(HardwareEvent::DisplayChanged(
            DisplayState {
                rotation_locked: true,
                orientation: Orientation::Portrait,
                ..preview_hardware().display
            },
        )));
        assert!(!m.auto_rotate());
        assert_eq!(m.rotation_label(), "locked to portrait");
        m.toggle_auto_rotate();
        assert_eq!(m.pending.pop(), Some(Action::SetRotationLock(false)));
        let mut fixed = preview_hardware();
        fixed.input.rotation = false;
        m.apply_core(snapshot(fixed, preview_capabilities()));
        m.toggle_auto_rotate();
        assert!(m.pending.is_empty());
        assert_eq!(
            m.last_error.as_deref(),
            Some("rotation \u{b7} no sensor on this target")
        );
    }

    #[test]
    fn bluetooth_absent_controller_is_a_clean_unavailable_state() {
        let mut hw = preview_hardware();
        hw.bluetooth = BluetoothState {
            present: false,
            enabled: false,
        };
        // The physical-target shape: capability unsupported, no controller.
        let caps = CapabilityMatrix::uniform(CapabilityStatus::Supported)
            .with(Capability::Bluetooth, CapabilityStatus::Unsupported);
        let mut m = Model::new();
        m.apply_core(snapshot(hw, caps));
        m.show(Page::Bluetooth);
        assert!(m.pending.is_empty(), "no device query without a controller");
        assert_eq!(m.bluetooth_block(), Err(BluetoothBlock::Unsupported));
        assert_eq!(m.bluetooth_state_name(), "unsupported");
        m.toggle_bluetooth();
        assert!(m.pending.is_empty());
        assert_eq!(
            m.last_error.as_deref(),
            Some("bluetooth \u{b7} not supported on this target")
        );
        // A mocked/supported target whose controller is simply not attached.
        let mut hw = preview_hardware();
        hw.bluetooth.present = false;
        m.apply_core(snapshot(hw, preview_capabilities()));
        assert_eq!(m.bluetooth_block(), Err(BluetoothBlock::NoController));
        assert_eq!(m.bluetooth_summary(), "unavailable \u{b7} no controller");
        assert!(!m.bluetooth_on());
    }

    #[test]
    fn bluetooth_devices_are_queried_on_show_and_after_changes() {
        let mut m = with_core();
        m.show(Page::Bluetooth);
        assert_eq!(m.pending, vec![Action::RefreshBluetoothDevices]);
        m.pending.clear();
        m.set_bluetooth_session(Ok(crate::bluetooth::preview_session(None)));
        assert_eq!(m.bluetooth_state_name(), "on-2-saved");
        assert_eq!(m.bluetooth_summary(), "on \u{b7} 1 connected");
        m.toggle_bluetooth();
        assert_eq!(m.pending.pop(), Some(Action::SetBluetooth(false)));
        m.apply_core(CoreFeed::Event(HardwareEvent::BluetoothChanged(
            BluetoothState {
                present: true,
                enabled: false,
            },
        )));
        assert!(m.bt_stale, "the shown session is kept, marked stale");
        assert_eq!(m.pending, vec![Action::RefreshBluetoothDevices]);
        m.request_devices_if_shown();
        assert_eq!(m.pending.len(), 1, "one query in flight");
        m.pending.clear();
        m.set_bluetooth_session(Err("bluetooth is unsupported on this target".into()));
        assert_eq!(m.bluetooth_state_name(), "off-devices-failed");
        m.show(Page::Root);
        m.apply_core(CoreFeed::Event(HardwareEvent::BluetoothChanged(
            BluetoothState {
                present: true,
                enabled: true,
            },
        )));
        assert!(m.pending.is_empty(), "no query while the page is hidden");
    }

    #[test]
    fn mobile_page_names_sim_operator_signal_and_data_blocks() {
        let mut m = Model::new();
        assert_eq!(m.data_block(), Err(DataBlock::TelephonyUnavailable));
        m.apply_telephony(tele(|_| {}));
        assert_eq!(m.sim_label().0, "ready");
        assert_eq!(m.operator_label(), "Ferrix Mock \u{b7} lte");
        assert_eq!(m.signal_label(), "3 of 4 bars");
        assert_eq!(m.mobile_summary(), "Ferrix Mock \u{b7} 3 bars");
        assert_eq!(m.mobile_state_name(), "data-off");
        m.toggle_cellular_data();
        assert_eq!(m.pending.pop(), Some(Action::SetCellularData(true)));

        m.apply_telephony(tele(|t| {
            t.sim = SimState::Absent;
            t.registration = Registration::Unknown;
            t.operator = None;
        }));
        assert_eq!(m.sim_label(), ("absent", "No SIM card inserted"));
        assert_eq!(m.operator_label(), "none \u{b7} no SIM");
        assert_eq!(m.signal_label(), "no service \u{b7} no SIM");
        assert_eq!(m.signal_bars(), None);
        assert_eq!(m.mobile_summary(), "no SIM");
        assert_eq!(m.mobile_state_name(), "no-sim");
        m.toggle_cellular_data();
        assert!(m.pending.is_empty());
        assert_eq!(m.last_error.as_deref(), Some("sim \u{b7} absent"));

        m.apply_telephony(tele(|t| t.capabilities.clear()));
        assert_eq!(m.data_block(), Err(DataBlock::Unsupported));
        m.apply_telephony(tele(|t| t.radio = RadioPower::Off));
        assert_eq!(m.data_block(), Err(DataBlock::RadioOff));
        m.apply_telephony(tele(|t| {
            t.data = DataBearer::Connected { interface: None };
            t.capabilities.clear();
        }));
        m.toggle_cellular_data();
        assert_eq!(
            m.pending.pop(),
            Some(Action::SetCellularData(false)),
            "an active bearer can always be turned off"
        );
        m.apply_telephony(tele(|t| {
            t.registration = Registration::Registered { roaming: true };
            t.signal = ferrix_telephony_api::Signal::new(2, Some(-97));
        }));
        assert_eq!(m.operator_label(), "Ferrix Mock \u{b7} lte \u{b7} roaming");
        assert_eq!(m.signal_label(), "2 of 4 bars \u{b7} -97 dBm");
    }

    #[test]
    fn captions_name_the_capability_status_and_target() {
        let mut m = with_core();
        assert_eq!(
            m.source_caption("display", Capability::Display),
            "display \u{b7} mock \u{b7} qemu-aarch64"
        );
        assert_eq!(m.display_summary(), "72% \u{b7} auto-rotate on");
        m.apply_core(CoreFeed::Disconnected {
            reason: "gone".into(),
        });
        assert_eq!(
            m.source_caption("display", Capability::Display),
            "display \u{b7} core \u{b7} unavailable"
        );
        m.apply_core(snapshot(preview_hardware(), preview_capabilities()));
        assert_eq!(m.core_link, Link::Reconnected);
    }
}
