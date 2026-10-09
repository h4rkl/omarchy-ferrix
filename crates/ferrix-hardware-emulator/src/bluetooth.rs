//! Deterministic mock Bluetooth controller: remembered devices from the
//! profile's `[[bluetooth.devices]]`, devices discovery finds from
//! `[[bluetooth.discoverable]]`, and a pairing state machine whose remote
//! steps, prompt timeouts, and discovery deadline advance only on the mock
//! clock (`bluetooth-tick`), never on the wall clock.

use ferrix_hardware_api::{
    BLUETOOTH_DISCOVERY_TIMEOUT_MS, BLUETOOTH_PROMPT_TIMEOUT_MS, BluetoothDevice,
    BluetoothDeviceKind, BluetoothDiscovery, BluetoothPairing, BluetoothPin, BluetoothSession,
    Capability, HardwareError, MockPairingMethod, MockPairingOutcome, PairingFailure, PairingPhase,
    signal_bars_from_rssi,
};
use ferrix_target_profile::Profile;

/// Mock milliseconds the remote takes to finish a bond once both sides agreed.
pub const MOCK_PAIR_STEP_MS: u64 = 1_000;
/// Mock milliseconds the remote takes to type a displayed passkey.
pub const MOCK_REMOTE_TYPING_MS: u64 = 3_000;
/// Mock milliseconds a silent remote takes to time the attempt out.
pub const MOCK_REMOTE_TIMEOUT_MS: u64 = 30_000;

#[derive(Clone, Debug)]
struct MockDevice {
    id: String,
    name: Option<String>,
    kind: BluetoothDeviceKind,
    paired: bool,
    connected: bool,
    /// Received signal strength once discovery has found the device.
    rssi: Option<i16>,
    method: MockPairingMethod,
    outcome: MockPairingOutcome,
    passkey: u32,
    /// The legacy PIN a `pin` remote expects.
    pin: String,
    /// Remembered devices are always listed; discoverable ones only after a
    /// discovery found them (like BlueZ's device cache).
    visible: bool,
}

/// What the mock clock does next to the running pairing attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// The remote finishes bonding with the attempt's outcome.
    Resolve,
    /// A silent remote: the attempt times out.
    RemoteTimeout,
    /// Nobody answered the prompt.
    PromptTimeout,
}

#[derive(Clone, Debug)]
struct Attempt {
    device: String,
    phase: PairingPhase,
    due: Option<(u64, Step)>,
    /// The remote started the attempt.
    incoming: bool,
    /// An outcome decided locally (a wrong PIN) that wins over the
    /// device's profile outcome and the override.
    forced: Option<MockPairingOutcome>,
}

/// Default legacy PIN of a mock `pin` remote.
const DEFAULT_MOCK_PIN: &str = "0000";

#[derive(Debug)]
pub struct MockBluetooth {
    devices: Vec<MockDevice>,
    clock_ms: u64,
    discovery_until: Option<u64>,
    attempt: Option<Attempt>,
    outcome_override: Option<MockPairingOutcome>,
}

fn invalid(reason: impl Into<String>) -> HardwareError {
    HardwareError::invalid(Capability::Bluetooth, reason)
}

impl MockBluetooth {
    pub fn from_profile(profile: &Profile) -> Self {
        let section = profile.bluetooth.clone().unwrap_or_default();
        let named = |name: &str| (!name.is_empty()).then(|| name.to_owned());
        let mut devices: Vec<MockDevice> = section
            .devices
            .iter()
            .enumerate()
            .map(|(index, device)| MockDevice {
                id: format!("mock-bt-{index}"),
                name: named(&device.name),
                kind: device.kind,
                paired: device.paired,
                connected: device.connected,
                rssi: None,
                method: MockPairingMethod::JustWorks,
                outcome: MockPairingOutcome::Success,
                passkey: 0,
                pin: DEFAULT_MOCK_PIN.to_owned(),
                visible: true,
            })
            .collect();
        devices.extend(
            section
                .discoverable
                .iter()
                .enumerate()
                .map(|(index, device)| MockDevice {
                    id: format!("mock-bt-found-{index}"),
                    name: named(&device.name),
                    kind: device.kind,
                    paired: false,
                    connected: false,
                    rssi: Some(device.rssi),
                    method: device.pairing,
                    outcome: device.outcome,
                    passkey: device.passkey,
                    pin: device
                        .pin
                        .clone()
                        .unwrap_or_else(|| DEFAULT_MOCK_PIN.to_owned()),
                    visible: false,
                }),
        );
        Self {
            devices,
            clock_ms: 0,
            discovery_until: None,
            attempt: None,
            outcome_override: None,
        }
    }

    pub fn clock_ms(&self) -> u64 {
        self.clock_ms
    }

    /// The listed devices; nothing is connected while the controller is
    /// off. Signal bars only for devices the discovery found.
    pub fn devices(&self, enabled: bool) -> Vec<BluetoothDevice> {
        let mut devices: Vec<BluetoothDevice> = self
            .devices
            .iter()
            .filter(|d| d.visible)
            .map(|d| BluetoothDevice {
                id: d.id.clone(),
                name: d.name.clone(),
                paired: d.paired,
                connected: d.connected && enabled,
                kind: d.kind,
                signal_bars: d.rssi.map(signal_bars_from_rssi),
            })
            .collect();
        ferrix_hardware_api::sort_bluetooth_devices(&mut devices);
        devices
    }

    pub fn session(&self, enabled: bool) -> BluetoothSession {
        BluetoothSession {
            discovery: BluetoothDiscovery {
                active: self.discovery_until.is_some(),
                remaining_ms: self
                    .discovery_until
                    .map(|until| u32::try_from(until - self.clock_ms).unwrap_or(u32::MAX)),
            },
            devices: self.devices(enabled),
            pairing: self.attempt.as_ref().map(|a| BluetoothPairing {
                device: a.device.clone(),
                phase: a.phase,
                incoming: a.incoming,
            }),
            operation: None,
        }
    }

    fn device_mut(&mut self, id: &str) -> Result<&mut MockDevice, HardwareError> {
        self.devices
            .iter_mut()
            .find(|d| d.visible && d.id == id)
            .ok_or_else(|| invalid("no such bluetooth device"))
    }

    fn running(&self) -> Option<&Attempt> {
        self.attempt.as_ref().filter(|a| !a.phase.is_finished())
    }

    // -- product operations ------------------------------------------------
    //
    // Each returns whether the session changed (the caller publishes one
    // event when it did).

    pub fn start_discovery(&mut self) -> bool {
        self.discovery_until = Some(self.clock_ms + u64::from(BLUETOOTH_DISCOVERY_TIMEOUT_MS));
        for device in &mut self.devices {
            if device.rssi.is_some() {
                device.visible = true;
            }
        }
        true
    }

    pub fn stop_discovery(&mut self) -> bool {
        self.discovery_until.take().is_some()
    }

    pub fn pair(&mut self, id: &str) -> Result<bool, HardwareError> {
        if self.running().is_some() {
            return Err(invalid("a pairing attempt is already in progress"));
        }
        let now = self.clock_ms;
        let device = self.device_mut(id)?;
        if device.paired {
            return Err(invalid("the device is already paired"));
        }
        let (phase, due) = Self::first_phase(device.method, device.passkey, now);
        self.attempt = Some(Attempt {
            device: id.to_owned(),
            phase,
            due,
            incoming: false,
            forced: None,
        });
        // As the BlueZ adapter does: discovery competes with paging the
        // remote, so pairing stops it.
        self.discovery_until = None;
        Ok(true)
    }

    /// A remote starts pairing with this phone. It becomes visible (BlueZ
    /// creates the device object on the incoming connection); a Just Works
    /// remote is asked about explicitly, as BlueZ's agent is for incoming
    /// requests.
    pub fn incoming(&mut self, id: &str) -> Result<bool, HardwareError> {
        if self.running().is_some() {
            return Err(invalid("a pairing attempt is already in progress"));
        }
        let now = self.clock_ms;
        let device = self
            .devices
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or_else(|| invalid("no such bluetooth device"))?;
        if device.paired {
            return Err(invalid("the device is already paired"));
        }
        device.visible = true;
        let method = match device.method {
            MockPairingMethod::JustWorks => MockPairingMethod::Authorize,
            other => other,
        };
        let (phase, due) = Self::first_phase(method, device.passkey, now);
        self.attempt = Some(Attempt {
            device: id.to_owned(),
            phase,
            due,
            incoming: true,
            forced: None,
        });
        Ok(true)
    }

    fn first_phase(
        method: MockPairingMethod,
        passkey: u32,
        now: u64,
    ) -> (PairingPhase, Option<(u64, Step)>) {
        let prompt = |phase| {
            (
                phase,
                Some((
                    now + u64::from(BLUETOOTH_PROMPT_TIMEOUT_MS),
                    Step::PromptTimeout,
                )),
            )
        };
        match method {
            MockPairingMethod::JustWorks => (
                PairingPhase::Pairing,
                Some((now + MOCK_PAIR_STEP_MS, Step::Resolve)),
            ),
            MockPairingMethod::Confirm => prompt(PairingPhase::ConfirmPasskey { passkey }),
            MockPairingMethod::Authorize => prompt(PairingPhase::Authorize),
            MockPairingMethod::DisplayPasskey => (
                PairingPhase::DisplayPasskey { passkey },
                Some((now + MOCK_REMOTE_TYPING_MS, Step::Resolve)),
            ),
            MockPairingMethod::Pin => prompt(PairingPhase::RequestPin),
        }
    }

    pub fn reply(&mut self, accept: bool) -> Result<bool, HardwareError> {
        let now = self.clock_ms;
        let attempt = self
            .attempt
            .as_mut()
            .filter(|a| a.phase.awaits_reply())
            .ok_or_else(|| invalid("no pairing prompt is waiting for an answer"))?;
        if accept && attempt.phase == PairingPhase::RequestPin {
            return Err(invalid("this pairing needs a PIN"));
        }
        if accept {
            attempt.phase = PairingPhase::Pairing;
            attempt.due = Some((now + MOCK_PAIR_STEP_MS, Step::Resolve));
        } else {
            attempt.phase = PairingPhase::Failed {
                reason: PairingFailure::Declined,
            };
            attempt.due = None;
        }
        Ok(true)
    }

    /// Answers a PIN request. A PIN the remote does not expect still
    /// bonds for a moment, then the remote rejects it (as a real legacy
    /// device does). The PIN is compared and dropped; nothing records it.
    pub fn reply_pin(&mut self, pin: &BluetoothPin) -> Result<bool, HardwareError> {
        let now = self.clock_ms;
        let device = self
            .attempt
            .as_ref()
            .filter(|a| a.phase == PairingPhase::RequestPin)
            .map(|a| a.device.clone())
            .ok_or_else(|| invalid("no PIN request is waiting for an answer"))?;
        let expected = self
            .devices
            .iter()
            .find(|d| d.id == device)
            .is_some_and(|d| d.pin == pin.expose());
        if let Some(attempt) = self.attempt.as_mut() {
            attempt.phase = PairingPhase::Pairing;
            attempt.due = Some((now + MOCK_PAIR_STEP_MS, Step::Resolve));
            attempt.forced = (!expected).then_some(MockPairingOutcome::Rejected);
        }
        Ok(true)
    }

    /// Cancels a running attempt. With nothing running it only clears a
    /// finished attempt's outcome (the flow was dismissed).
    pub fn cancel(&mut self) -> bool {
        match self.attempt.as_mut() {
            Some(attempt) if !attempt.phase.is_finished() => {
                attempt.phase = PairingPhase::Failed {
                    reason: PairingFailure::Canceled,
                };
                attempt.due = None;
                true
            }
            Some(_) => {
                self.attempt = None;
                true
            }
            None => false,
        }
    }

    pub fn connect(&mut self, id: &str, connected: bool) -> Result<bool, HardwareError> {
        let device = self.device_mut(id)?;
        if !device.paired {
            return Err(invalid("pair the device before connecting it"));
        }
        let changed = device.connected != connected;
        device.connected = connected;
        Ok(changed)
    }

    pub fn forget(&mut self, id: &str) -> Result<bool, HardwareError> {
        let index = self
            .devices
            .iter()
            .position(|d| d.visible && d.id == id)
            .ok_or_else(|| invalid("no such bluetooth device"))?;
        let device = &mut self.devices[index];
        if !device.paired {
            return Err(invalid("the device is not paired"));
        }
        if device.rssi.is_some() {
            // A discoverable device stays in the cache, unpaired.
            device.paired = false;
            device.connected = false;
        } else {
            self.devices.remove(index);
        }
        if self.attempt.as_ref().is_some_and(|a| a.device == id) {
            self.attempt = None;
        }
        Ok(true)
    }

    /// The controller turned off or went away: discovery stops and a
    /// running pairing attempt is cancelled.
    pub fn power_lost(&mut self) -> bool {
        let stopped = self.stop_discovery();
        let canceled = self.running().is_some() && self.cancel();
        stopped || canceled
    }

    pub fn set_outcome_override(&mut self, outcome: Option<MockPairingOutcome>) {
        self.outcome_override = outcome;
    }

    /// Advances the mock clock; returns whether the session changed.
    pub fn tick(&mut self, ms: u32) -> bool {
        self.clock_ms = self.clock_ms.saturating_add(u64::from(ms));
        // A running discovery's remaining time moved: clients re-query.
        let mut changed = self.discovery_until.is_some();
        if self
            .discovery_until
            .is_some_and(|until| until <= self.clock_ms)
        {
            self.discovery_until = None;
        }
        // A resolve can schedule a remote timeout, which may also be due.
        while let Some((due, step)) = self.due_step() {
            self.run_step(step, due);
            changed = true;
        }
        changed
    }

    fn due_step(&mut self) -> Option<(u64, Step)> {
        let attempt = self.attempt.as_mut()?;
        match attempt.due {
            Some((due, step)) if due <= self.clock_ms => {
                attempt.due = None;
                Some((due, step))
            }
            _ => None,
        }
    }

    /// Runs `step`, which fell due at mock time `now`; a follow-up step is
    /// scheduled from then, so one long tick walks every step that is due.
    fn run_step(&mut self, step: Step, now: u64) {
        let Some(attempt) = self.attempt.clone() else {
            return;
        };
        let outcome = attempt.forced.or(self.outcome_override).unwrap_or_else(|| {
            self.devices
                .iter()
                .find(|d| d.id == attempt.device)
                .map_or(MockPairingOutcome::Success, |d| d.outcome)
        });
        let (phase, due) = match step {
            Step::PromptTimeout | Step::RemoteTimeout => (
                PairingPhase::Failed {
                    reason: PairingFailure::TimedOut,
                },
                None,
            ),
            Step::Resolve => match outcome {
                MockPairingOutcome::Success => {
                    if let Some(device) = self.devices.iter_mut().find(|d| d.id == attempt.device) {
                        device.paired = true;
                        // Like Settings on a phone: a new bond connects.
                        device.connected = true;
                    }
                    (PairingPhase::Paired, None)
                }
                MockPairingOutcome::Rejected => (
                    PairingPhase::Failed {
                        reason: PairingFailure::Rejected,
                    },
                    None,
                ),
                MockPairingOutcome::Timeout => (
                    PairingPhase::Pairing,
                    Some((now + MOCK_REMOTE_TIMEOUT_MS, Step::RemoteTimeout)),
                ),
            },
        };
        if let Some(current) = self.attempt.as_mut() {
            current.phase = phase;
            current.due = due;
        }
    }
}
