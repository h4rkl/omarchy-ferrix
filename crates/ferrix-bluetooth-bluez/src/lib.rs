//! BlueZ adapter for Ferrix's target-neutral Bluetooth hardware contract.
//!
//! BlueZ owns controller and protocol handling. This crate translates the
//! system D-Bus `org.bluez` objects into the portable [`Bluetooth`]
//! capability: controller power (`Adapter1.Powered`), the managed
//! controller's `Device1` objects, bounded discovery
//! (`Adapter1.StartDiscovery`/`StopDiscovery`, stopped by this adapter after
//! [`BLUETOOTH_DISCOVERY_TIMEOUT_MS`]), pairing (`Device1.Pair` with an
//! exported `org.bluez.Agent1` registered as the default agent with the
//! `DisplayYesNo` capability), connect/disconnect, and forget
//! (`Adapter1.RemoveDevice`). It does not implement the Bluetooth protocol,
//! profiles, or audio routing.
//!
//! Privacy: device object paths embed the hardware address and never leave
//! this crate. Product code sees a per-process keyed hash, and every error
//! text that crosses the boundary is passed through [`redact`].
//!
//! Pairing prompts: the agent hands numeric-comparison and Just Works
//! authorization requests to the session as [`PairingPhase`] prompts that
//! Settings answers through [`Bluetooth::reply_bluetooth_pairing`]; an
//! unanswered prompt is declined after [`BLUETOOTH_PROMPT_TIMEOUT_MS`].
//! Legacy PIN entry is not offered: `RequestPinCode` is refused and the
//! attempt fails with [`PairingFailure::PinRequired`] (documented choice; a
//! typed PIN prompt can be added without changing the contract).

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_hardware_api::{
    BLUETOOTH_DISCOVERY_TIMEOUT_MS, BLUETOOTH_PROMPT_TIMEOUT_MS, Bluetooth, BluetoothAction,
    BluetoothDevice, BluetoothDeviceKind, BluetoothDiscovery, BluetoothOperation, BluetoothPairing,
    BluetoothSession, BluetoothState, Capability, EventSource, HardwareError, HardwareEvent,
    PairingFailure, PairingPhase, signal_bars_from_rssi,
};

#[cfg(feature = "production")]
mod backend;

#[derive(Clone, Debug, Eq, PartialEq)]
struct AdapterRecord {
    path: String,
    powered: bool,
    discovering: bool,
}

/// One `org.bluez.Device1` object. `path` embeds the hardware address and
/// never leaves this crate; product code sees only an opaque hash.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DeviceRecord {
    path: String,
    adapter: String,
    name: Option<String>,
    paired: bool,
    connected: bool,
    /// Present while the device is in range of a discovery.
    rssi: Option<i16>,
    icon: Option<String>,
    appearance: Option<u16>,
}

/// A failed BlueZ call: the D-Bus error name when the service answered,
/// and a [`redact`]ed description.
#[derive(Clone, Debug, Eq, PartialEq)]
struct BusError {
    name: Option<String>,
    text: String,
}

impl BusError {
    #[allow(dead_code)] // Built by the production transport and the tests.
    fn named(name: &str, text: &str) -> Self {
        Self {
            name: Some(name.to_owned()),
            text: redact(text),
        }
    }

    fn is(&self, short: &str) -> bool {
        self.name
            .as_deref()
            .is_some_and(|name| name.strip_prefix("org.bluez.Error.") == Some(short))
    }

    /// What Settings shows: BlueZ's own short reason (for example
    /// `br-connection-page-timeout`), else the error name's last part.
    fn describe(&self) -> String {
        if !self.text.trim().is_empty() {
            return self.text.trim().to_owned();
        }
        self.name
            .as_deref()
            .and_then(|name| name.rsplit('.').next())
            .unwrap_or("failed")
            .to_owned()
    }

    fn pairing_failure(&self) -> PairingFailure {
        match self
            .name
            .as_deref()
            .and_then(|n| n.strip_prefix("org.bluez.Error."))
        {
            Some("AuthenticationRejected") => PairingFailure::Rejected,
            Some("AuthenticationCanceled") => PairingFailure::Canceled,
            Some("AuthenticationTimeout") => PairingFailure::TimedOut,
            Some("ConnectionAttemptFailed" | "DoesNotExist") => PairingFailure::Unreachable,
            _ => PairingFailure::Failed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Used by the production transport and mocked-bus tests.
enum BusEvent {
    Changed,
    ServiceLost,
}

/// What the exported `org.bluez.Agent1` object asks of the adapter. Every
/// method is called on the D-Bus connection's executor and therefore only
/// touches in-memory state: no D-Bus call, no blocking wait.
#[allow(dead_code)] // Called by the production agent object and the tests.
trait AgentCallbacks: Send + Sync {
    /// A prompt the user answers; `respond` is called exactly once, by the
    /// user's reply, the prompt timeout, a cancellation, or a newer prompt.
    fn prompt(&self, device_path: &str, phase: PairingPhase, respond: Box<dyn FnOnce(bool) + Send>);
    /// The remote types `passkey` (keyboards).
    fn display(&self, device_path: &str, passkey: u32);
    /// The agent refused a request it does not support.
    fn refuse(&self, device_path: &str, reason: PairingFailure);
    /// BlueZ cancelled the outstanding request.
    fn cancel(&self);
    /// Whether a remote may use a service: only bonded devices may.
    fn service_allowed(&self, device_path: &str) -> bool;
}

#[allow(dead_code)] // `watch` is inactive in non-production library builds.
trait BluezBus: Send + Sync {
    fn adapters(&self) -> Result<Vec<AdapterRecord>, String>;
    fn devices(&self) -> Result<Vec<DeviceRecord>, String>;
    fn set_powered(&self, path: &str, powered: bool) -> Result<(), String>;
    fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String>;
    /// Exports the agent object (once) and registers it as BlueZ's default
    /// `DisplayYesNo` agent (again after a BlueZ restart).
    fn register_agent(&self, agent: Arc<dyn AgentCallbacks>) -> Result<(), BusError>;
    fn start_discovery(&self, adapter: &str) -> Result<(), BusError>;
    fn stop_discovery(&self, adapter: &str) -> Result<(), BusError>;
    /// Blocks until BlueZ finishes the bond (or fails it).
    fn pair(&self, device: &str) -> Result<(), BusError>;
    fn cancel_pairing(&self, device: &str) -> Result<(), BusError>;
    fn set_trusted(&self, device: &str) -> Result<(), BusError>;
    /// Blocks until the connection is up (or fails).
    fn connect_device(&self, device: &str) -> Result<(), BusError>;
    fn disconnect_device(&self, device: &str) -> Result<(), BusError>;
    fn remove_device(&self, adapter: &str, device: &str) -> Result<(), BusError>;
}

/// Bounds the adapter enforces itself. Tests shorten them.
#[derive(Clone, Copy, Debug)]
struct Timeouts {
    discovery: Duration,
    prompt: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            discovery: Duration::from_millis(u64::from(BLUETOOTH_DISCOVERY_TIMEOUT_MS)),
            prompt: Duration::from_millis(u64::from(BLUETOOTH_PROMPT_TIMEOUT_MS)),
        }
    }
}

/// The pairing attempt this adapter tracks (ours, or an incoming one the
/// agent was asked about).
struct Attempt {
    /// Distinguishes attempts on the same device: a late Pair() reply of
    /// an older attempt never touches a newer one.
    serial: u64,
    device: String,
    path: String,
    phase: PairingPhase,
    /// A reason decided locally (declined, prompt timeout, PIN refused)
    /// that wins over whatever error BlueZ reports afterwards.
    local_failure: Option<PairingFailure>,
}

struct Prompt {
    ticket: u64,
    respond: Box<dyn FnOnce(bool) + Send>,
}

struct State {
    bluetooth: BluetoothState,
    subscribers: Vec<Sender<HardwareEvent>>,
    /// The managed controller's devices as of the last refresh.
    devices: Vec<DeviceRecord>,
    managed: Option<AdapterRecord>,
    discovery_deadline: Option<Instant>,
    discovery_generation: u64,
    attempt: Option<Attempt>,
    operation: Option<BluetoothOperation>,
    prompt: Option<Prompt>,
    next_ticket: u64,
    next_attempt: u64,
    agent_registered: bool,
    /// The last session published (without the moving deadline), so only
    /// real changes produce `bluetooth-session-changed`.
    last_session: Option<BluetoothSession>,
}

/// BlueZ-backed implementation of Ferrix's Bluetooth capability.
///
/// Use [`BluezBluetooth::connect`] with the `production` feature enabled. The
/// adapter is returned in an [`Arc`] because its signal watcher, timers, and
/// agent share the same authoritative state and event subscribers.
pub struct BluezBluetooth {
    bus: Arc<dyn BluezBus>,
    state: Mutex<State>,
    commands: Mutex<()>,
    timeouts: Timeouts,
    /// Keys the opaque device ids: stable for this process, unguessable
    /// from the address.
    id_key: RandomState,
    /// Weak self-reference for worker threads, timers, and the agent, so
    /// none of them keeps the adapter alive.
    me: Weak<Self>,
}

impl BluezBluetooth {
    #[cfg(feature = "production")]
    pub fn connect() -> Result<Arc<Self>, HardwareError> {
        Self::from_bus(Arc::new(backend::SystemBluezBus::connect()?))
    }

    #[allow(dead_code)] // Production constructor seam, also used by tests.
    fn from_bus(bus: Arc<dyn BluezBus>) -> Result<Arc<Self>, HardwareError> {
        Self::from_bus_with(bus, Timeouts::default())
    }

    fn from_bus_with(
        bus: Arc<dyn BluezBus>,
        timeouts: Timeouts,
    ) -> Result<Arc<Self>, HardwareError> {
        let adapter = Arc::new_cyclic(|me| Self {
            me: me.clone(),
            bus,
            state: Mutex::new(State {
                bluetooth: BluetoothState {
                    present: false,
                    enabled: false,
                },
                subscribers: Vec::new(),
                devices: Vec::new(),
                managed: None,
                discovery_deadline: None,
                discovery_generation: 0,
                attempt: None,
                operation: None,
                prompt: None,
                next_ticket: 0,
                next_attempt: 0,
                agent_registered: false,
                last_session: None,
            }),
            commands: Mutex::new(()),
            timeouts,
            id_key: RandomState::new(),
        });
        adapter.refresh()?;
        adapter.ensure_agent();
        adapter.start_watcher()?;
        Ok(adapter)
    }

    #[allow(dead_code)]
    fn start_watcher(self: &Arc<Self>) -> Result<(), HardwareError> {
        let (sender, receiver) = mpsc::channel();
        self.bus.watch(sender).map_err(failed)?;
        let weak = Arc::downgrade(self);
        thread::Builder::new()
            .name("ferrix-bluetooth-bluez-state".to_owned())
            .spawn(move || watch_loop(weak, receiver))
            .map_err(|error| failed(error.to_string()))?;
        Ok(())
    }

    /// Registers the agent when it is not (first start, BlueZ restart).
    /// Without an agent, pairing still works for Just Works devices but
    /// prompts cannot reach Settings; the next refresh retries.
    fn ensure_agent(self: &Arc<Self>) {
        let wanted = {
            let state = lock(&self.state);
            !state.agent_registered && state.bluetooth.present
        };
        if !wanted {
            return;
        }
        let bridge: Arc<dyn AgentCallbacks> = Arc::new(AgentBridge(self.self_ref()));
        if self.bus.register_agent(bridge).is_ok() {
            lock(&self.state).agent_registered = true;
        }
    }

    /// Opaque, per-process device id: a keyed hash of the object path.
    fn opaque_id(&self, path: &str) -> String {
        format!("bt-{:016x}", self.id_key.hash_one(path))
    }

    /// Re-reads adapters and devices and publishes the deltas.
    fn refresh(&self) -> Result<BluetoothState, HardwareError> {
        let adapters = self.bus.adapters().map_err(failed)?;
        let managed = managed_adapter(&adapters).cloned();
        let devices = match &managed {
            Some(adapter) => self
                .bus
                .devices()
                .map_err(failed)?
                .into_iter()
                .filter(|device| device.adapter == adapter.path)
                .collect(),
            None => Vec::new(),
        };
        let bluetooth = state_from_adapters(&adapters);
        let mut state = lock(&self.state);
        if state.bluetooth != bluetooth {
            state.bluetooth = bluetooth;
            publish(&mut state, HardwareEvent::BluetoothChanged(bluetooth));
        }
        if !managed.as_ref().is_some_and(|a| a.powered) {
            state.discovery_deadline = None;
        }
        state.managed = managed;
        state.devices = devices;
        // An incoming pairing finishes without a Pair() reply of ours.
        let bonded: Vec<String> = state
            .devices
            .iter()
            .filter(|d| d.paired)
            .map(|d| d.path.clone())
            .collect();
        if let Some(attempt) = state.attempt.as_mut()
            && !attempt.phase.is_finished()
            && attempt.local_failure.is_none()
            && bonded.contains(&attempt.path)
        {
            attempt.phase = PairingPhase::Paired;
        }
        self.publish_session(&mut state);
        Ok(bluetooth)
    }

    fn service_lost(&self) {
        let mut state = lock(&self.state);
        state.agent_registered = false;
        state.discovery_deadline = None;
        state.managed = None;
        state.devices.clear();
        if let Some(prompt) = state.prompt.take() {
            (prompt.respond)(false);
        }
        if let Some(attempt) = state.attempt.as_mut()
            && !attempt.phase.is_finished()
        {
            attempt.phase = PairingPhase::Failed {
                reason: PairingFailure::Canceled,
            };
        }
        let bluetooth = BluetoothState {
            present: false,
            enabled: false,
        };
        if state.bluetooth != bluetooth {
            state.bluetooth = bluetooth;
            publish(&mut state, HardwareEvent::BluetoothChanged(bluetooth));
        }
        self.publish_session(&mut state);
    }

    fn session_from(&self, state: &State) -> BluetoothSession {
        if !state.bluetooth.present {
            return BluetoothSession::default();
        }
        let active = state.managed.as_ref().is_some_and(|a| a.discovering);
        let remaining_ms = state.discovery_deadline.filter(|_| active).map(|deadline| {
            let left = deadline.saturating_duration_since(Instant::now());
            u32::try_from(left.as_millis()).unwrap_or(u32::MAX)
        });
        BluetoothSession {
            discovery: BluetoothDiscovery {
                active,
                remaining_ms,
            },
            devices: self.product_devices(&state.devices, state.bluetooth.enabled),
            pairing: state.attempt.as_ref().map(|attempt| BluetoothPairing {
                device: attempt.device.clone(),
                phase: attempt.phase,
            }),
            operation: state.operation.clone(),
        }
    }

    fn product_devices(&self, records: &[DeviceRecord], enabled: bool) -> Vec<BluetoothDevice> {
        let mut devices: Vec<BluetoothDevice> = records
            .iter()
            .map(|device| BluetoothDevice {
                id: self.opaque_id(&device.path),
                name: device.name.clone().filter(|name| !name.trim().is_empty()),
                paired: device.paired,
                connected: device.connected && enabled,
                kind: kind_of(device),
                signal_bars: device.rssi.map(signal_bars_from_rssi),
            })
            .collect();
        ferrix_hardware_api::sort_bluetooth_devices(&mut devices);
        devices
    }

    /// Publishes one `bluetooth-session-changed` when the session differs
    /// from the last one published (the moving deadline aside).
    fn publish_session(&self, state: &mut State) {
        let mut session = self.session_from(state);
        let summary = session.summary();
        session.discovery.remaining_ms = None;
        if state.last_session.as_ref() == Some(&session) {
            return;
        }
        state.last_session = Some(session);
        publish(state, HardwareEvent::BluetoothSessionChanged(summary));
    }

    /// Resolves an opaque id to its device record on the managed adapter.
    fn resolve(&self, id: &str) -> Result<(DeviceRecord, String), HardwareError> {
        let state = lock(&self.state);
        let adapter = state
            .managed
            .as_ref()
            .filter(|adapter| adapter.powered)
            .ok_or_else(|| failed("bluetooth is off"))?
            .path
            .clone();
        let device = state
            .devices
            .iter()
            .find(|device| self.opaque_id(&device.path) == id)
            .cloned()
            .ok_or_else(|| {
                HardwareError::invalid(Capability::Bluetooth, "no such bluetooth device")
            })?;
        Ok((device, adapter))
    }

    fn powered_adapter(&self) -> Result<String, HardwareError> {
        let state = lock(&self.state);
        match &state.managed {
            None => Err(failed("BlueZ exposes no org.bluez.Adapter1 controller")),
            Some(adapter) if !adapter.powered => Err(failed("bluetooth is off")),
            Some(adapter) => Ok(adapter.path.clone()),
        }
    }

    fn spawn(name: &str, run: impl FnOnce() + Send + 'static) -> Result<(), HardwareError> {
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(run)
            .map(|_| ())
            .map_err(|error| failed(error.to_string()))
    }

    /// Ends attempt `serial` (if it is still the current one).
    fn finish_attempt(&self, serial: u64, result: Result<(), BusError>) {
        let mut state = lock(&self.state);
        if !state.attempt.as_ref().is_some_and(|a| a.serial == serial) {
            return;
        }
        Self::answer_prompt(&mut state, false);
        if let Some(attempt) = state.attempt.as_mut() {
            attempt.phase = match (&result, attempt.local_failure) {
                (_, Some(reason)) => PairingPhase::Failed { reason },
                (Ok(()), None) => PairingPhase::Paired,
                (Err(error), None) => PairingPhase::Failed {
                    reason: error.pairing_failure(),
                },
            };
        }
        self.publish_session(&mut state);
    }

    fn run_operation(
        self: &Arc<Self>,
        device: String,
        path: String,
        action: BluetoothAction,
    ) -> Result<(), HardwareError> {
        {
            let mut state = lock(&self.state);
            state.operation = Some(BluetoothOperation {
                device: device.clone(),
                action,
                failed: None,
            });
            self.publish_session(&mut state);
        }
        let weak = Arc::downgrade(self);
        Self::spawn("ferrix-bluetooth-bluez-connect", move || {
            let Some(adapter) = weak.upgrade() else {
                return;
            };
            let result = match action {
                BluetoothAction::Connect => adapter.bus.connect_device(&path),
                BluetoothAction::Disconnect => adapter.bus.disconnect_device(&path),
            };
            {
                let mut state = lock(&adapter.state);
                if let Some(operation) = state
                    .operation
                    .as_mut()
                    .filter(|o| o.device == device && o.action == action)
                {
                    match &result {
                        Ok(()) => state.operation = None,
                        Err(error) => operation.failed = Some(error.describe()),
                    }
                }
                adapter.publish_session(&mut state);
            }
            let _ = adapter.refresh();
        })
    }

    /// Answers and forgets the pending prompt, if any.
    fn answer_prompt(state: &mut State, accept: bool) -> bool {
        match state.prompt.take() {
            Some(prompt) => {
                (prompt.respond)(accept);
                true
            }
            None => false,
        }
    }

    /// The attempt the agent is asking about: the current one for this
    /// device, or a new (incoming) attempt.
    fn attempt_for<'s>(&self, state: &'s mut State, path: &str) -> &'s mut Attempt {
        let device = self.opaque_id(path);
        let reuse = state
            .attempt
            .as_ref()
            .is_some_and(|a| a.device == device && !a.phase.is_finished());
        if !reuse {
            state.next_attempt += 1;
            state.attempt = Some(Attempt {
                serial: state.next_attempt,
                device,
                path: path.to_owned(),
                phase: PairingPhase::Pairing,
                local_failure: None,
            });
        }
        state.attempt.as_mut().expect("attempt was just set")
    }
}

impl Bluetooth for BluezBluetooth {
    fn bluetooth_state(&self) -> BluetoothState {
        lock(&self.state).bluetooth
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<BluetoothState, HardwareError> {
        let _command = lock(&self.commands);
        let adapters = self.bus.adapters().map_err(failed)?;
        let managed = managed_adapter(&adapters)
            .ok_or_else(|| failed("BlueZ exposes no org.bluez.Adapter1 controller"))?;
        self.bus
            .set_powered(&managed.path, enabled)
            .map_err(failed)?;
        self.refresh()
    }

    fn bluetooth_devices(&self) -> Result<Vec<BluetoothDevice>, HardwareError> {
        let adapters = self.bus.adapters().map_err(failed)?;
        let Some(managed) = managed_adapter(&adapters) else {
            return Ok(Vec::new());
        };
        let records: Vec<DeviceRecord> = self
            .bus
            .devices()
            .map_err(failed)?
            .into_iter()
            .filter(|device| device.adapter == managed.path)
            .collect();
        Ok(self.product_devices(&records, managed.powered))
    }

    fn bluetooth_session(&self) -> Result<BluetoothSession, HardwareError> {
        Ok(self.session_from(&lock(&self.state)))
    }

    fn start_bluetooth_discovery(&self) -> Result<(), HardwareError> {
        let _command = lock(&self.commands);
        let adapter = self.powered_adapter()?;
        match self.bus.start_discovery(&adapter) {
            Ok(()) => {}
            // Already discovering for this client: extend the deadline.
            Err(error) if error.is("InProgress") => {}
            Err(error) => return Err(failed(format!("discovery \u{b7} {}", error.describe()))),
        }
        let generation = {
            let mut state = lock(&self.state);
            state.discovery_generation += 1;
            state.discovery_deadline = Some(Instant::now() + self.timeouts.discovery);
            if let Some(managed) = state.managed.as_mut() {
                managed.discovering = true;
            }
            self.publish_session(&mut state);
            state.discovery_generation
        };
        let bus = Arc::clone(&self.bus);
        let wait = self.timeouts.discovery;
        // The timer holds no reference to the adapter: it only stops the
        // discovery this start began; the watcher reconciles the state.
        let state_check = self.discovery_owner(generation);
        Self::spawn("ferrix-bluetooth-bluez-discovery", move || {
            thread::sleep(wait);
            if state_check() {
                let _ = bus.stop_discovery(&adapter);
            }
        })
    }

    fn stop_bluetooth_discovery(&self) -> Result<(), HardwareError> {
        let _command = lock(&self.commands);
        let adapter = self.powered_adapter()?;
        // BlueZ refuses to stop a discovery this client did not start;
        // there is then nothing of ours to stop.
        let _ = self.bus.stop_discovery(&adapter);
        let mut state = lock(&self.state);
        state.discovery_deadline = None;
        state.discovery_generation += 1;
        self.publish_session(&mut state);
        Ok(())
    }

    fn pair_bluetooth_device(&self, device: &str) -> Result<(), HardwareError> {
        let (record, adapter) = self.resolve(device)?;
        if record.paired {
            return Err(HardwareError::invalid(
                Capability::Bluetooth,
                "the device is already paired",
            ));
        }
        let serial = {
            let mut state = lock(&self.state);
            if state
                .attempt
                .as_ref()
                .is_some_and(|a| !a.phase.is_finished())
            {
                return Err(HardwareError::invalid(
                    Capability::Bluetooth,
                    "a pairing attempt is already in progress",
                ));
            }
            state.next_attempt += 1;
            state.attempt = Some(Attempt {
                serial: state.next_attempt,
                device: device.to_owned(),
                path: record.path.clone(),
                phase: PairingPhase::Pairing,
                local_failure: None,
            });
            // Discovery competes with paging the remote; stop ours first.
            state.discovery_deadline = None;
            state.discovery_generation += 1;
            self.publish_session(&mut state);
            state.next_attempt
        };
        let _ = self.bus.stop_discovery(&adapter);
        let bus = Arc::clone(&self.bus);
        let path = record.path;
        let id = device.to_owned();
        let owner = self.self_ref();
        Self::spawn("ferrix-bluetooth-bluez-pair", move || {
            let result = match bus.pair(&path) {
                // Already bonded counts as success.
                Err(error) if error.is("AlreadyExists") => Ok(()),
                other => other,
            };
            let paired = result.is_ok();
            if paired {
                // Trusted bonds reconnect and authorize services without
                // asking again.
                let _ = bus.set_trusted(&path);
            }
            let Some(adapter) = owner.upgrade() else {
                return;
            };
            adapter.finish_attempt(serial, result);
            let _ = adapter.refresh();
            let bonded = lock(&adapter.state)
                .attempt
                .as_ref()
                .is_some_and(|a| a.serial == serial && a.phase == PairingPhase::Paired);
            if paired && bonded {
                // Like Settings on a phone: a new bond connects.
                let _ = adapter.run_operation(id, path, BluetoothAction::Connect);
            }
        })
    }

    fn cancel_bluetooth_pairing(&self) -> Result<(), HardwareError> {
        let path = {
            let mut state = lock(&self.state);
            let running = state
                .attempt
                .as_ref()
                .is_some_and(|a| !a.phase.is_finished());
            if !running {
                // Dismissing a finished attempt's outcome.
                let had = state.attempt.take().is_some();
                if had {
                    self.publish_session(&mut state);
                }
                return Ok(());
            }
            Self::answer_prompt(&mut state, false);
            let attempt = state.attempt.as_mut().expect("running attempt");
            attempt.local_failure = Some(PairingFailure::Canceled);
            attempt.phase = PairingPhase::Failed {
                reason: PairingFailure::Canceled,
            };
            let path = attempt.path.clone();
            self.publish_session(&mut state);
            path
        };
        let _ = self.bus.cancel_pairing(&path);
        Ok(())
    }

    fn reply_bluetooth_pairing(&self, accept: bool) -> Result<(), HardwareError> {
        let mut state = lock(&self.state);
        if !Self::answer_prompt(&mut state, accept) {
            return Err(HardwareError::invalid(
                Capability::Bluetooth,
                "no pairing prompt is waiting for an answer",
            ));
        }
        if let Some(attempt) = state.attempt.as_mut() {
            if accept {
                attempt.phase = PairingPhase::Pairing;
            } else {
                attempt.local_failure = Some(PairingFailure::Declined);
                attempt.phase = PairingPhase::Failed {
                    reason: PairingFailure::Declined,
                };
            }
        }
        self.publish_session(&mut state);
        Ok(())
    }

    fn connect_bluetooth_device(&self, device: &str) -> Result<(), HardwareError> {
        let (record, _) = self.resolve(device)?;
        if !record.paired {
            return Err(HardwareError::invalid(
                Capability::Bluetooth,
                "pair the device before connecting it",
            ));
        }
        self.strong()?
            .run_operation(device.to_owned(), record.path, BluetoothAction::Connect)
    }

    fn disconnect_bluetooth_device(&self, device: &str) -> Result<(), HardwareError> {
        let (record, _) = self.resolve(device)?;
        self.strong()?
            .run_operation(device.to_owned(), record.path, BluetoothAction::Disconnect)
    }

    fn forget_bluetooth_device(&self, device: &str) -> Result<(), HardwareError> {
        let (record, adapter) = self.resolve(device)?;
        self.bus
            .remove_device(&adapter, &record.path)
            .map_err(|error| failed(format!("forget \u{b7} {}", error.describe())))?;
        {
            let mut state = lock(&self.state);
            if state.attempt.as_ref().is_some_and(|a| a.device == device) {
                state.attempt = None;
            }
            if state.operation.as_ref().is_some_and(|o| o.device == device) {
                state.operation = None;
            }
        }
        self.refresh().map(|_| ())
    }
}

impl BluezBluetooth {
    fn self_ref(&self) -> Weak<Self> {
        self.me.clone()
    }

    fn strong(&self) -> Result<Arc<Self>, HardwareError> {
        self.self_ref()
            .upgrade()
            .ok_or_else(|| failed("bluetooth adapter is shutting down"))
    }

    /// A check the discovery timer runs: true (and the deadline cleared)
    /// when the discovery started under `generation` is still ours.
    fn discovery_owner(&self, generation: u64) -> impl Fn() -> bool + Send + 'static {
        let owner = self.self_ref();
        move || {
            let Some(adapter) = owner.upgrade() else {
                return false;
            };
            let mut state = lock(&adapter.state);
            if state.discovery_generation != generation {
                return false;
            }
            state.discovery_deadline = None;
            true
        }
    }
}

impl EventSource for BluezBluetooth {
    fn subscribe(&self, sender: Sender<HardwareEvent>) {
        lock(&self.state).subscribers.push(sender);
    }
}

/// The agent's view of the adapter. Holds a weak reference so the bus (which
/// owns the exported agent object) never keeps the adapter alive.
struct AgentBridge(Weak<BluezBluetooth>);

impl AgentCallbacks for AgentBridge {
    fn prompt(
        &self,
        device_path: &str,
        phase: PairingPhase,
        respond: Box<dyn FnOnce(bool) + Send>,
    ) {
        let Some(adapter) = self.0.upgrade() else {
            respond(false);
            return;
        };
        let ticket = {
            let mut state = lock(&adapter.state);
            // A newer request supersedes an unanswered one.
            BluezBluetooth::answer_prompt(&mut state, false);
            state.next_ticket += 1;
            let ticket = state.next_ticket;
            adapter.attempt_for(&mut state, device_path).phase = phase;
            state.prompt = Some(Prompt { ticket, respond });
            adapter.publish_session(&mut state);
            ticket
        };
        let weak = Arc::downgrade(&adapter);
        let wait = adapter.timeouts.prompt;
        let _ = BluezBluetooth::spawn("ferrix-bluetooth-bluez-prompt", move || {
            thread::sleep(wait);
            let Some(adapter) = weak.upgrade() else {
                return;
            };
            let mut state = lock(&adapter.state);
            if state.prompt.as_ref().is_none_or(|p| p.ticket != ticket) {
                return;
            }
            BluezBluetooth::answer_prompt(&mut state, false);
            if let Some(attempt) = state.attempt.as_mut() {
                attempt.local_failure = Some(PairingFailure::TimedOut);
                attempt.phase = PairingPhase::Failed {
                    reason: PairingFailure::TimedOut,
                };
            }
            adapter.publish_session(&mut state);
        });
    }

    fn display(&self, device_path: &str, passkey: u32) {
        let Some(adapter) = self.0.upgrade() else {
            return;
        };
        let mut state = lock(&adapter.state);
        adapter.attempt_for(&mut state, device_path).phase =
            PairingPhase::DisplayPasskey { passkey };
        adapter.publish_session(&mut state);
    }

    fn refuse(&self, device_path: &str, reason: PairingFailure) {
        let Some(adapter) = self.0.upgrade() else {
            return;
        };
        let mut state = lock(&adapter.state);
        let attempt = adapter.attempt_for(&mut state, device_path);
        attempt.local_failure = Some(reason);
        attempt.phase = PairingPhase::Failed { reason };
        adapter.publish_session(&mut state);
    }

    fn cancel(&self) {
        let Some(adapter) = self.0.upgrade() else {
            return;
        };
        let mut state = lock(&adapter.state);
        BluezBluetooth::answer_prompt(&mut state, false);
        if let Some(attempt) = state.attempt.as_mut()
            && !attempt.phase.is_finished()
            && attempt.local_failure.is_none()
        {
            attempt.local_failure = Some(PairingFailure::Canceled);
            attempt.phase = PairingPhase::Failed {
                reason: PairingFailure::Canceled,
            };
        }
        adapter.publish_session(&mut state);
    }

    fn service_allowed(&self, device_path: &str) -> bool {
        self.0.upgrade().is_some_and(|adapter| {
            lock(&adapter.state)
                .devices
                .iter()
                .any(|device| device.path == device_path && device.paired)
        })
    }
}

#[allow(dead_code)]
fn watch_loop(adapter: Weak<BluezBluetooth>, receiver: mpsc::Receiver<BusEvent>) {
    while let Ok(first) = receiver.recv() {
        // Coalesce bursts: discovery emits a property change per device per
        // advertisement. Only the last event's meaning matters.
        thread::sleep(Duration::from_millis(100));
        let mut lost = first == BusEvent::ServiceLost;
        let mut changed = first == BusEvent::Changed;
        while let Ok(event) = receiver.try_recv() {
            match event {
                BusEvent::ServiceLost => {
                    lost = true;
                    changed = false;
                }
                BusEvent::Changed => changed = true,
            }
        }
        let Some(adapter) = adapter.upgrade() else {
            break;
        };
        if lost {
            adapter.service_lost();
        }
        if changed {
            // A subsequent owner-change or object signal retries a failed
            // enumeration. The coarse API has no bus-error state.
            let _ = adapter.refresh();
            adapter.ensure_agent();
        }
    }
}

fn publish(state: &mut State, event: HardwareEvent) {
    state
        .subscribers
        .retain(|subscriber| subscriber.send(event).is_ok());
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn managed_adapter(adapters: &[AdapterRecord]) -> Option<&AdapterRecord> {
    adapters
        .iter()
        .min_by(|left, right| left.path.cmp(&right.path))
}

fn state_from_adapters(adapters: &[AdapterRecord]) -> BluetoothState {
    managed_adapter(adapters).map_or(
        BluetoothState {
            present: false,
            enabled: false,
        },
        |adapter| BluetoothState {
            present: true,
            enabled: adapter.powered,
        },
    )
}

/// Coarse kind from BlueZ's freedesktop icon name, else the GAP appearance
/// category (watch: 0x00C0..=0x00FF).
fn kind_of(device: &DeviceRecord) -> BluetoothDeviceKind {
    match device.icon.as_deref() {
        Some(icon) if icon.starts_with("audio-") => return BluetoothDeviceKind::Audio,
        Some(icon) if icon.starts_with("input-") => return BluetoothDeviceKind::Input,
        Some("phone") => return BluetoothDeviceKind::Phone,
        Some("computer") => return BluetoothDeviceKind::Computer,
        _ => {}
    }
    match device.appearance {
        Some(appearance) if appearance >> 6 == 3 => BluetoothDeviceKind::Wearable,
        _ => BluetoothDeviceKind::Other,
    }
}

/// Replaces anything shaped like a Bluetooth address (`AA:BB:CC:DD:EE:FF`,
/// or BlueZ's `AA_BB_CC_DD_EE_FF` object-path form) with `<device>`, so
/// error texts never carry one.
pub fn redact(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if is_address_at(bytes, i) {
            out.push_str("<device>");
            i += 17;
            continue;
        }
        let ch = text[i..]
            .chars()
            .next()
            .expect("index is on a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_address_at(bytes: &[u8], at: usize) -> bool {
    let Some(window) = bytes.get(at..at + 17) else {
        return false;
    };
    let separator = window[2];
    if separator != b':' && separator != b'_' {
        return false;
    }
    window.iter().enumerate().all(|(index, byte)| {
        if index % 3 == 2 {
            *byte == separator
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn failed(reason: impl Into<String>) -> HardwareError {
    HardwareError::failed(Capability::Bluetooth, redact(&reason.into()))
}

#[cfg(test)]
mod tests;
