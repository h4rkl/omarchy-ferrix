//! BlueZ adapter for Ferrix's target-neutral Bluetooth hardware contract.
//!
//! BlueZ owns controller and protocol handling. This crate translates the
//! system D-Bus `Adapter1.Powered` property into the deliberately coarse
//! [`BluetoothState`] used by product controls. It does not implement the
//! Bluetooth protocol, pairing, profiles, or audio routing.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;

use ferrix_hardware_api::{
    Bluetooth, BluetoothState, Capability, EventSource, HardwareError, HardwareEvent,
};

#[cfg(feature = "production")]
mod backend;

#[derive(Clone, Debug, Eq, PartialEq)]
struct AdapterRecord {
    path: String,
    powered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Used by the production transport and mocked-bus tests.
enum BusEvent {
    Changed,
    ServiceLost,
}

#[allow(dead_code)] // `watch` is inactive in non-production library builds.
trait BluezBus: Send + Sync {
    fn adapters(&self) -> Result<Vec<AdapterRecord>, String>;
    fn set_powered(&self, path: &str, powered: bool) -> Result<(), String>;
    fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String>;
}

#[derive(Debug)]
struct State {
    bluetooth: BluetoothState,
    subscribers: Vec<Sender<HardwareEvent>>,
}

/// BlueZ-backed implementation of Ferrix's Bluetooth capability.
///
/// Use [`BluezBluetooth::connect`] with the `production` feature enabled. The
/// adapter is returned in an [`Arc`] because its signal watcher shares the
/// same authoritative state and event subscribers.
pub struct BluezBluetooth {
    bus: Arc<dyn BluezBus>,
    state: Mutex<State>,
    commands: Mutex<()>,
}

impl BluezBluetooth {
    #[cfg(feature = "production")]
    pub fn connect() -> Result<Arc<Self>, HardwareError> {
        Self::from_bus(Arc::new(backend::SystemBluezBus::connect()?))
    }

    #[allow(dead_code)] // Production constructor seam, also used by tests.
    fn from_bus(bus: Arc<dyn BluezBus>) -> Result<Arc<Self>, HardwareError> {
        let bluetooth = state_from_adapters(bus.adapters().map_err(failed)?);
        let adapter = Arc::new(Self {
            bus,
            state: Mutex::new(State {
                bluetooth,
                subscribers: Vec::new(),
            }),
            commands: Mutex::new(()),
        });
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

    fn refresh(&self) -> Result<BluetoothState, HardwareError> {
        let bluetooth = state_from_adapters(self.bus.adapters().map_err(failed)?);
        self.replace_state(bluetooth);
        Ok(bluetooth)
    }

    fn replace_state(&self, bluetooth: BluetoothState) {
        let mut state = lock(&self.state);
        if state.bluetooth == bluetooth {
            return;
        }
        state.bluetooth = bluetooth;
        let event = HardwareEvent::BluetoothChanged(bluetooth);
        state
            .subscribers
            .retain(|subscriber| subscriber.send(event).is_ok());
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
}

impl EventSource for BluezBluetooth {
    fn subscribe(&self, sender: Sender<HardwareEvent>) {
        lock(&self.state).subscribers.push(sender);
    }
}

#[allow(dead_code)]
fn watch_loop(adapter: Weak<BluezBluetooth>, receiver: mpsc::Receiver<BusEvent>) {
    while let Ok(event) = receiver.recv() {
        let Some(adapter) = adapter.upgrade() else {
            break;
        };
        match event {
            BusEvent::Changed => {
                // A subsequent owner-change or object signal retries a failed
                // enumeration. The coarse API has no bus-error state.
                let _ = adapter.refresh();
            }
            BusEvent::ServiceLost => adapter.replace_state(BluetoothState {
                present: false,
                enabled: false,
            }),
        }
    }
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

fn state_from_adapters(adapters: Vec<AdapterRecord>) -> BluetoothState {
    managed_adapter(&adapters).map_or(
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

fn failed(reason: impl Into<String>) -> HardwareError {
    HardwareError::failed(Capability::Bluetooth, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    struct MockBus {
        adapters: Mutex<Result<Vec<AdapterRecord>, String>>,
        watchers: Mutex<Vec<Sender<BusEvent>>>,
        writes: Mutex<Vec<(String, bool)>>,
    }

    impl Default for MockBus {
        fn default() -> Self {
            Self {
                adapters: Mutex::new(Ok(Vec::new())),
                watchers: Mutex::new(Vec::new()),
                writes: Mutex::new(Vec::new()),
            }
        }
    }

    impl MockBus {
        fn with_adapters(adapters: Vec<AdapterRecord>) -> Self {
            Self {
                adapters: Mutex::new(Ok(adapters)),
                ..Self::default()
            }
        }

        fn replace(&self, adapters: Vec<AdapterRecord>) {
            *lock(&self.adapters) = Ok(adapters);
        }

        fn signal(&self, event: BusEvent) {
            lock(&self.watchers).retain(|watcher| watcher.send(event).is_ok());
        }
    }

    impl BluezBus for MockBus {
        fn adapters(&self) -> Result<Vec<AdapterRecord>, String> {
            lock(&self.adapters).clone()
        }

        fn set_powered(&self, path: &str, powered: bool) -> Result<(), String> {
            lock(&self.writes).push((path.to_owned(), powered));
            let mut adapters = lock(&self.adapters);
            let entries = adapters.as_mut().map_err(|error| error.clone())?;
            let adapter = entries
                .iter_mut()
                .find(|adapter| adapter.path == path)
                .ok_or_else(|| "adapter disappeared".to_owned())?;
            adapter.powered = powered;
            Ok(())
        }

        fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String> {
            lock(&self.watchers).push(sender);
            Ok(())
        }
    }

    fn record(path: &str, powered: bool) -> AdapterRecord {
        AdapterRecord {
            path: path.to_owned(),
            powered,
        }
    }

    fn subscribed(adapter: &BluezBluetooth) -> Receiver<HardwareEvent> {
        let (sender, receiver) = mpsc::channel();
        adapter.subscribe(sender);
        receiver
    }

    #[test]
    fn absent_bus_state_maps_to_absent_and_disabled() {
        let bus = Arc::new(MockBus::with_adapters(Vec::new()));
        let adapter = BluezBluetooth::from_bus(bus).unwrap();
        assert_eq!(
            adapter.bluetooth_state(),
            BluetoothState {
                present: false,
                enabled: false
            }
        );
    }

    #[test]
    fn deterministic_first_adapter_supplies_state_and_receives_commands() {
        let bus = Arc::new(MockBus::with_adapters(vec![
            record("/org/bluez/hci1", true),
            record("/org/bluez/hci0", false),
        ]));
        let adapter = BluezBluetooth::from_bus(bus.clone()).unwrap();
        assert_eq!(
            adapter.bluetooth_state(),
            BluetoothState {
                present: true,
                enabled: false
            }
        );

        let state = adapter.set_bluetooth_enabled(true).unwrap();
        assert!(state.enabled);
        assert_eq!(
            *lock(&bus.writes),
            vec![("/org/bluez/hci0".to_owned(), true)]
        );
    }

    #[test]
    fn mocked_bus_changes_publish_only_real_state_deltas() {
        let bus = Arc::new(MockBus::with_adapters(vec![record(
            "/org/bluez/hci0",
            false,
        )]));
        let adapter = BluezBluetooth::from_bus(bus.clone()).unwrap();
        let events = subscribed(&adapter);

        bus.replace(vec![record("/org/bluez/hci0", false)]);
        adapter.refresh().unwrap();
        assert!(events.recv_timeout(Duration::from_millis(50)).is_err());
        bus.replace(vec![record("/org/bluez/hci0", true)]);
        adapter.refresh().unwrap();
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HardwareEvent::BluetoothChanged(BluetoothState {
                present: true,
                enabled: true
            })
        );
    }

    #[test]
    fn service_loss_clears_state_and_emits_a_delta() {
        let bus = Arc::new(MockBus::with_adapters(vec![record(
            "/org/bluez/hci0",
            true,
        )]));
        let adapter = BluezBluetooth::from_bus(bus.clone()).unwrap();
        let events = subscribed(&adapter);

        bus.signal(BusEvent::ServiceLost);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HardwareEvent::BluetoothChanged(BluetoothState {
                present: false,
                enabled: false
            })
        );
    }

    #[test]
    fn command_without_controller_is_a_typed_bluetooth_failure() {
        let bus = Arc::new(MockBus::with_adapters(Vec::new()));
        let adapter = BluezBluetooth::from_bus(bus).unwrap();
        let error = adapter.set_bluetooth_enabled(true).unwrap_err();
        assert!(matches!(
            error,
            HardwareError::Failed {
                capability: Capability::Bluetooth,
                ..
            }
        ));
    }
}
