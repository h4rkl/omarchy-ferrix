//! Deterministic implementation of the portable hardware contracts for the
//! generic AArch64 QEMU virtual phone and for host tests.
//!
//! Every capability is `Mocked`: initial state comes from the versioned
//! target profile, and tests drive transitions through the typed mutation
//! methods (or [`ferrix_hardware_api::MockControl`]). Each mutation emits exactly one
//! [`HardwareEvent`] to every subscriber. Nothing here reads real hardware.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use ferrix_hardware_api::{
    Audio, AudioState, Bluetooth, BluetoothState, Capability, CapabilityMatrix, CapabilityStatus,
    Display, DisplayState, EventSource, HardwareError, HardwareEvent, HardwareState, Input,
    InputState, Lifecycle, LifecycleState, MockCommand, MockControl, ModemRegistration, Network,
    NetworkState, Orientation, Power, PowerState, SimState, SuspendOutcome, Telephony,
    TelephonyState, WifiState,
};
use ferrix_target_profile::Profile;

/// Target id of the generic QEMU virtual phone this adapter mocks.
pub const TARGET_ID: &str = "qemu-aarch64";

/// Default brightness the mock display starts at; the profile has no
/// brightness field because it is runtime UI state, not a target fact.
const DEFAULT_BRIGHTNESS: u8 = 72;

#[derive(Debug)]
struct Inner {
    state: HardwareState,
    next_suspend: SuspendOutcome,
    subscribers: Vec<Sender<HardwareEvent>>,
}

/// The mock adapter. Cheap to clone; clones share state and subscribers so a
/// session can hand out one handle per capability trait.
#[derive(Clone, Debug)]
pub struct Emulator {
    inner: Arc<Mutex<Inner>>,
}

impl Emulator {
    pub fn from_profile(profile: &Profile) -> Self {
        let state = HardwareState {
            power: PowerState {
                battery_percent: profile.power.battery_percent,
                charging: profile.power.charging,
            },
            display: DisplayState {
                width: profile.display.width,
                height: profile.display.height,
                scale: profile.display.scale,
                orientation: profile.display.orientation,
                brightness_percent: DEFAULT_BRIGHTNESS,
            },
            input: InputState {
                touch: profile.input.touch,
                rotation: profile.input.rotation,
            },
            network: NetworkState {
                wifi: profile.connectivity.wifi,
            },
            bluetooth: BluetoothState {
                present: profile.connectivity.bluetooth_present,
                enabled: profile.connectivity.bluetooth_present,
            },
            audio: AudioState {
                route: profile.audio.route,
            },
            telephony: TelephonyState {
                registration: profile.connectivity.modem,
                sim: profile.connectivity.sim,
            },
            lifecycle: LifecycleState::default(),
        };
        Self {
            inner: Arc::new(Mutex::new(Inner {
                state,
                next_suspend: SuspendOutcome::Resumed,
                subscribers: Vec::new(),
            })),
        }
    }

    /// Every capability is mocked on this target; none is `Supported`.
    pub fn capability_matrix() -> CapabilityMatrix {
        CapabilityMatrix::uniform(CapabilityStatus::Mocked)
    }

    pub fn snapshot(&self) -> HardwareState {
        self.lock().state
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock only means another thread panicked mid-update; the
        // plain-old-data state is still consistent enough to keep serving.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn publish(inner: &mut Inner, event: HardwareEvent) -> HardwareEvent {
        event.apply_to(&mut inner.state);
        inner
            .subscribers
            .retain(|sender| sender.send(event).is_ok());
        event
    }

    // -- deterministic mutations ------------------------------------------

    pub fn set_charging(&self, charging: bool) -> HardwareEvent {
        let mut inner = self.lock();
        let mut power = inner.state.power;
        power.charging = charging;
        Self::publish(&mut inner, HardwareEvent::PowerChanged(power))
    }

    pub fn set_battery_percent(&self, percent: u8) -> Result<HardwareEvent, HardwareError> {
        if percent > 100 {
            return Err(HardwareError::invalid(
                Capability::Power,
                format!("battery_percent must be 0..=100 (got {percent})"),
            ));
        }
        let mut inner = self.lock();
        let mut power = inner.state.power;
        power.battery_percent = percent;
        Ok(Self::publish(
            &mut inner,
            HardwareEvent::PowerChanged(power),
        ))
    }

    pub fn set_wifi(&self, wifi: WifiState) -> HardwareEvent {
        let mut inner = self.lock();
        Self::publish(
            &mut inner,
            HardwareEvent::NetworkChanged(NetworkState { wifi }),
        )
    }

    pub fn set_bluetooth_present(&self, present: bool) -> HardwareEvent {
        let mut inner = self.lock();
        let enabled = present && inner.state.bluetooth.enabled;
        Self::publish(
            &mut inner,
            HardwareEvent::BluetoothChanged(BluetoothState { present, enabled }),
        )
    }

    pub fn set_modem_registration(&self, registration: ModemRegistration) -> HardwareEvent {
        let mut inner = self.lock();
        let mut telephony = inner.state.telephony;
        telephony.registration = registration;
        Self::publish(&mut inner, HardwareEvent::TelephonyChanged(telephony))
    }

    pub fn set_sim(&self, sim: SimState) -> HardwareEvent {
        let mut inner = self.lock();
        let mut telephony = inner.state.telephony;
        telephony.sim = sim;
        Self::publish(&mut inner, HardwareEvent::TelephonyChanged(telephony))
    }

    pub fn set_orientation(&self, orientation: Orientation) -> HardwareEvent {
        let mut inner = self.lock();
        let mut display = inner.state.display;
        display.orientation = orientation;
        Self::publish(&mut inner, HardwareEvent::DisplayChanged(display))
    }

    /// Chooses what the next [`Lifecycle::suspend`] call reports. Emits no
    /// event by itself.
    pub fn set_suspend_outcome(&self, outcome: SuspendOutcome) {
        self.lock().next_suspend = outcome;
    }
}

impl Power for Emulator {
    fn power_state(&self) -> PowerState {
        self.lock().state.power
    }
}

impl Display for Emulator {
    fn display_state(&self) -> DisplayState {
        self.lock().state.display
    }

    fn set_brightness(&self, percent: u8) -> Result<DisplayState, HardwareError> {
        if percent > 100 {
            return Err(HardwareError::invalid(
                Capability::Display,
                format!("brightness must be 0..=100 (got {percent})"),
            ));
        }
        let mut inner = self.lock();
        let mut display = inner.state.display;
        display.brightness_percent = percent;
        Self::publish(&mut inner, HardwareEvent::DisplayChanged(display));
        Ok(display)
    }
}

impl Input for Emulator {
    fn input_state(&self) -> InputState {
        self.lock().state.input
    }
}

impl Network for Emulator {
    fn network_state(&self) -> NetworkState {
        self.lock().state.network
    }

    fn set_wifi_enabled(&self, enabled: bool) -> Result<NetworkState, HardwareError> {
        let mut inner = self.lock();
        let wifi = match (enabled, inner.state.network.wifi) {
            (false, _) => WifiState::Disabled,
            (true, WifiState::Disabled) => WifiState::Disconnected,
            (true, current) => current,
        };
        let network = NetworkState { wifi };
        Self::publish(&mut inner, HardwareEvent::NetworkChanged(network));
        Ok(network)
    }
}

impl Bluetooth for Emulator {
    fn bluetooth_state(&self) -> BluetoothState {
        self.lock().state.bluetooth
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<BluetoothState, HardwareError> {
        let mut inner = self.lock();
        if !inner.state.bluetooth.present {
            return Err(HardwareError::unsupported(Capability::Bluetooth));
        }
        let bluetooth = BluetoothState {
            present: true,
            enabled,
        };
        Self::publish(&mut inner, HardwareEvent::BluetoothChanged(bluetooth));
        Ok(bluetooth)
    }
}

impl Audio for Emulator {
    fn audio_state(&self) -> AudioState {
        self.lock().state.audio
    }

    fn set_audio_route(
        &self,
        route: ferrix_hardware_api::AudioRoute,
    ) -> Result<AudioState, HardwareError> {
        let mut inner = self.lock();
        let audio = AudioState { route };
        Self::publish(&mut inner, HardwareEvent::AudioChanged(audio));
        Ok(audio)
    }
}

impl Telephony for Emulator {
    fn telephony_state(&self) -> TelephonyState {
        self.lock().state.telephony
    }
}

impl Lifecycle for Emulator {
    fn lifecycle_state(&self) -> LifecycleState {
        self.lock().state.lifecycle
    }

    fn suspend(&self) -> Result<SuspendOutcome, HardwareError> {
        let mut inner = self.lock();
        let outcome = std::mem::replace(&mut inner.next_suspend, SuspendOutcome::Resumed);
        let mut lifecycle = inner.state.lifecycle;
        lifecycle.suspend_count += 1;
        lifecycle.last_suspend = Some(outcome);
        Self::publish(&mut inner, HardwareEvent::Suspended { outcome, lifecycle });
        match outcome {
            SuspendOutcome::Failed => Err(HardwareError::failed(
                Capability::Lifecycle,
                "mock suspend outcome is failed",
            )),
            other => Ok(other),
        }
    }
}

impl EventSource for Emulator {
    fn subscribe(&self, sender: Sender<HardwareEvent>) {
        self.lock().subscribers.push(sender);
    }
}

impl MockControl for Emulator {
    fn apply(&self, command: MockCommand) -> Result<Option<HardwareEvent>, HardwareError> {
        Ok(match command {
            MockCommand::SetCharging { charging } => Some(self.set_charging(charging)),
            MockCommand::SetBatteryPercent { battery_percent } => {
                Some(self.set_battery_percent(battery_percent)?)
            }
            MockCommand::SetWifi { state } => Some(self.set_wifi(state)),
            MockCommand::SetBluetoothPresent { present } => {
                Some(self.set_bluetooth_present(present))
            }
            MockCommand::SetAudioRoute { route } => {
                self.set_audio_route(route)?;
                Some(HardwareEvent::AudioChanged(self.audio_state()))
            }
            MockCommand::SetModemRegistration { registration } => {
                Some(self.set_modem_registration(registration))
            }
            MockCommand::SetSim { state } => Some(self.set_sim(state)),
            MockCommand::SetOrientation { orientation } => Some(self.set_orientation(orientation)),
            MockCommand::SetSuspendOutcome { outcome } => {
                self.set_suspend_outcome(outcome);
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const FIXTURE: &str = r#"
schema_version = 1
name = "test"
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

    fn emulator() -> (Emulator, mpsc::Receiver<HardwareEvent>) {
        let e = Emulator::from_profile(&Profile::parse(FIXTURE).unwrap());
        let (tx, rx) = mpsc::channel();
        e.subscribe(tx);
        (e, rx)
    }

    #[test]
    fn charging_transition_is_deterministic() {
        let (e, rx) = emulator();
        assert!(!e.power_state().charging);
        let event = e.set_charging(true);
        let expected = HardwareEvent::PowerChanged(PowerState {
            battery_percent: 75,
            charging: true,
        });
        assert_eq!(event, expected);
        assert_eq!(rx.try_recv().unwrap(), expected);
        assert!(e.power_state().charging);
    }

    #[test]
    fn battery_percent_is_range_checked() {
        let (e, rx) = emulator();
        assert!(e.set_battery_percent(101).is_err());
        assert!(rx.try_recv().is_err(), "no event on rejected value");
        e.set_battery_percent(40).unwrap();
        assert_eq!(e.power_state().battery_percent, 40);
    }

    #[test]
    fn display_orientation_and_brightness() {
        let (e, rx) = emulator();
        e.set_orientation(Orientation::Landscape);
        assert_eq!(e.display_state().orientation, Orientation::Landscape);
        assert!(matches!(
            rx.try_recv().unwrap(),
            HardwareEvent::DisplayChanged(_)
        ));
        assert_eq!(e.set_brightness(30).unwrap().brightness_percent, 30);
        assert!(e.set_brightness(101).is_err());
    }

    #[test]
    fn input_comes_from_profile() {
        let (e, _rx) = emulator();
        assert_eq!(
            e.input_state(),
            InputState {
                touch: true,
                rotation: true
            }
        );
    }

    #[test]
    fn wifi_transitions() {
        let (e, rx) = emulator();
        e.set_wifi(WifiState::Disconnected);
        assert_eq!(e.network_state().wifi, WifiState::Disconnected);
        assert_eq!(
            rx.try_recv().unwrap(),
            HardwareEvent::NetworkChanged(NetworkState {
                wifi: WifiState::Disconnected
            })
        );
        assert_eq!(e.set_wifi_enabled(false).unwrap().wifi, WifiState::Disabled);
        assert_eq!(
            e.set_wifi_enabled(true).unwrap().wifi,
            WifiState::Disconnected
        );
    }

    #[test]
    fn bluetooth_requires_presence() {
        let (e, _rx) = emulator();
        assert!(e.set_bluetooth_enabled(false).unwrap().present);
        e.set_bluetooth_present(false);
        assert_eq!(
            e.set_bluetooth_enabled(true),
            Err(HardwareError::unsupported(Capability::Bluetooth))
        );
    }

    #[test]
    fn telephony_transitions() {
        let (e, rx) = emulator();
        e.set_modem_registration(ModemRegistration::Searching);
        e.set_sim(SimState::Absent);
        assert_eq!(
            e.telephony_state(),
            TelephonyState {
                registration: ModemRegistration::Searching,
                sim: SimState::Absent
            }
        );
        assert_eq!(rx.try_iter().count(), 2);
    }

    #[test]
    fn suspend_outcome_is_controlled() {
        let (e, rx) = emulator();
        assert_eq!(e.suspend().unwrap(), SuspendOutcome::Resumed);
        e.set_suspend_outcome(SuspendOutcome::Failed);
        assert!(e.suspend().is_err());
        assert_eq!(e.lifecycle_state().suspend_count, 2);
        assert_eq!(
            e.lifecycle_state().last_suspend,
            Some(SuspendOutcome::Failed)
        );
        assert_eq!(rx.try_iter().count(), 2);
        assert_eq!(e.suspend().unwrap(), SuspendOutcome::Resumed);
    }

    #[test]
    fn audio_route_transitions() {
        let (e, rx) = emulator();
        e.set_audio_route(ferrix_hardware_api::AudioRoute::Headphones)
            .unwrap();
        assert_eq!(
            e.audio_state().route,
            ferrix_hardware_api::AudioRoute::Headphones
        );
        assert!(matches!(rx.try_recv(), Ok(HardwareEvent::AudioChanged(_))));
    }

    #[test]
    fn mock_control_routes_every_command() {
        let (e, rx) = emulator();
        let event = e
            .apply(MockCommand::SetCharging { charging: true })
            .unwrap()
            .unwrap();
        assert_eq!(event.name(), "power-changed");
        assert!(
            e.apply(MockCommand::SetSuspendOutcome {
                outcome: SuspendOutcome::Rejected
            })
            .unwrap()
            .is_none()
        );
        assert!(
            e.apply(MockCommand::SetBatteryPercent {
                battery_percent: 200
            })
            .is_err()
        );
        assert_eq!(rx.try_iter().count(), 1);
    }

    #[test]
    fn every_capability_is_mocked() {
        let m = Emulator::capability_matrix();
        for c in Capability::ALL {
            assert_eq!(m.status(c), CapabilityStatus::Mocked, "{c}");
        }
    }

    #[test]
    fn dropped_subscribers_are_pruned() {
        let (e, rx) = emulator();
        drop(rx);
        e.set_charging(true);
        assert!(e.lock().subscribers.is_empty());
    }
}
