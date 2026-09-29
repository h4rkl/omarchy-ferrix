//! UPower adapter for Ferrix's target-neutral power hardware contract.
//!
//! UPower owns power-supply discovery and kernel integration. This crate reads
//! its aggregate display device and translates `Percentage` and `State` into
//! the deliberately small [`PowerState`] consumed by product code. It never
//! reads sysfs or implements a battery protocol itself.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;

use ferrix_hardware_api::{
    Capability, EventSource, HardwareError, HardwareEvent, Power, PowerState,
};

#[cfg(feature = "production")]
mod backend;

/// UPower's `UpDeviceState` values used by the portable mapping.
mod device_state {
    pub const CHARGING: u32 = 1;
    pub const PENDING_CHARGE: u32 = 5;
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PowerReading {
    percentage: f64,
    state: u32,
    present: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Used by the production transport and fake-bus tests.
enum BusEvent {
    Changed,
    ServiceLost,
}

#[allow(dead_code)] // `watch` is inactive in non-production library builds.
trait UpowerBus: Send + Sync {
    fn display_device(&self) -> Result<PowerReading, String>;
    fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String>;
}

#[derive(Debug)]
struct State {
    power: PowerState,
    subscribers: Vec<Sender<HardwareEvent>>,
}

/// UPower-backed implementation of Ferrix's power capability.
///
/// Use [`UpowerPower::connect`] with the `production` feature enabled. UPower's
/// aggregate display device is authoritative, including on multi-battery
/// systems. The adapter is returned in an [`Arc`] because the signal watcher
/// shares the current state and event subscribers.
pub struct UpowerPower {
    bus: Arc<dyn UpowerBus>,
    state: Mutex<State>,
}

impl UpowerPower {
    /// Connects to UPower on the system bus and reads its display device.
    #[cfg(feature = "production")]
    pub fn connect() -> Result<Arc<Self>, HardwareError> {
        Self::from_bus(Arc::new(backend::SystemUpowerBus::connect()?))
    }

    #[allow(dead_code)] // Production constructor seam, also used by tests.
    fn from_bus(bus: Arc<dyn UpowerBus>) -> Result<Arc<Self>, HardwareError> {
        let power = map_reading(bus.display_device().map_err(failed)?)?;
        let adapter = Arc::new(Self {
            bus,
            state: Mutex::new(State {
                power,
                subscribers: Vec::new(),
            }),
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
            .name("ferrix-power-upower-state".to_owned())
            .spawn(move || watch_loop(weak, receiver))
            .map_err(|error| failed(error.to_string()))?;
        Ok(())
    }

    fn refresh(&self) -> Result<PowerState, HardwareError> {
        let power = map_reading(self.bus.display_device().map_err(failed)?)?;
        self.replace_state(power);
        Ok(power)
    }

    fn replace_state(&self, power: PowerState) {
        let mut state = lock(&self.state);
        if state.power == power {
            return;
        }
        state.power = power;
        let event = HardwareEvent::PowerChanged(power);
        state
            .subscribers
            .retain(|subscriber| subscriber.send(event).is_ok());
    }
}

impl Power for UpowerPower {
    fn power_state(&self) -> PowerState {
        lock(&self.state).power
    }
}

impl EventSource for UpowerPower {
    fn subscribe(&self, sender: Sender<HardwareEvent>) {
        lock(&self.state).subscribers.push(sender);
    }
}

#[allow(dead_code)]
fn watch_loop(adapter: Weak<UpowerPower>, receiver: mpsc::Receiver<BusEvent>) {
    while let Ok(event) = receiver.recv() {
        let Some(adapter) = adapter.upgrade() else {
            break;
        };
        match event {
            BusEvent::Changed => {
                // A later property or owner signal retries a transient failed
                // read. The coarse API deliberately has no bus-error state.
                let _ = adapter.refresh();
            }
            BusEvent::ServiceLost => {
                // `PowerState` has no unavailable/unknown representation. Keep
                // the last authoritative reading rather than manufacture zero.
            }
        }
    }
}

fn map_reading(reading: PowerReading) -> Result<PowerState, HardwareError> {
    if !reading.present {
        return Err(failed("UPower display device is not present"));
    }
    if !reading.percentage.is_finite() {
        return Err(failed("UPower returned a non-finite battery percentage"));
    }

    Ok(PowerState {
        battery_percent: reading.percentage.clamp(0.0, 100.0).round() as u8,
        charging: matches!(
            reading.state,
            device_state::CHARGING | device_state::PENDING_CHARGE
        ),
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn failed(reason: impl Into<String>) -> HardwareError {
    HardwareError::failed(Capability::Power, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    struct FakeBus {
        reading: Mutex<Result<PowerReading, String>>,
        watchers: Mutex<Vec<Sender<BusEvent>>>,
    }

    impl FakeBus {
        fn with_reading(reading: PowerReading) -> Self {
            Self {
                reading: Mutex::new(Ok(reading)),
                watchers: Mutex::new(Vec::new()),
            }
        }

        fn replace(&self, reading: PowerReading) {
            *lock(&self.reading) = Ok(reading);
            self.signal(BusEvent::Changed);
        }

        fn fail(&self, reason: &str) {
            *lock(&self.reading) = Err(reason.to_owned());
            self.signal(BusEvent::Changed);
        }

        fn signal(&self, event: BusEvent) {
            lock(&self.watchers).retain(|watcher| watcher.send(event).is_ok());
        }
    }

    impl UpowerBus for FakeBus {
        fn display_device(&self) -> Result<PowerReading, String> {
            lock(&self.reading).clone()
        }

        fn watch(&self, sender: Sender<BusEvent>) -> Result<(), String> {
            lock(&self.watchers).push(sender);
            Ok(())
        }
    }

    fn reading(percentage: f64, state: u32) -> PowerReading {
        PowerReading {
            percentage,
            state,
            present: true,
        }
    }

    fn subscribed(adapter: &UpowerPower) -> Receiver<HardwareEvent> {
        let (sender, receiver) = mpsc::channel();
        adapter.subscribe(sender);
        receiver
    }

    #[test]
    fn percentage_is_rounded_and_defensively_bounded() {
        assert_eq!(
            map_reading(reading(74.6, 2)).unwrap(),
            PowerState {
                battery_percent: 75,
                charging: false
            }
        );
        assert_eq!(map_reading(reading(-3.0, 2)).unwrap().battery_percent, 0);
        assert_eq!(map_reading(reading(103.0, 2)).unwrap().battery_percent, 100);
    }

    #[test]
    fn charging_and_pending_charge_are_charging() {
        assert!(map_reading(reading(50.0, 1)).unwrap().charging);
        assert!(map_reading(reading(50.0, 5)).unwrap().charging);
        assert!(!map_reading(reading(100.0, 4)).unwrap().charging);
        assert!(!map_reading(reading(50.0, 6)).unwrap().charging);
    }

    #[test]
    fn absent_or_non_finite_display_device_is_a_typed_power_failure() {
        let absent = PowerReading {
            present: false,
            ..reading(50.0, 2)
        };
        for error in [
            map_reading(absent).unwrap_err(),
            map_reading(reading(f64::NAN, 2)).unwrap_err(),
        ] {
            assert!(matches!(
                error,
                HardwareError::Failed {
                    capability: Capability::Power,
                    ..
                }
            ));
        }
    }

    #[test]
    fn fake_bus_changes_publish_only_real_state_deltas() {
        let bus = Arc::new(FakeBus::with_reading(reading(75.0, 2)));
        let adapter = UpowerPower::from_bus(bus.clone()).unwrap();
        let events = subscribed(&adapter);

        bus.replace(reading(75.4, 2));
        assert!(events.recv_timeout(Duration::from_millis(50)).is_err());
        bus.replace(reading(75.5, 1));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HardwareEvent::PowerChanged(PowerState {
                battery_percent: 76,
                charging: true
            })
        );
    }

    #[test]
    fn transient_read_failure_and_service_loss_preserve_last_state() {
        let bus = Arc::new(FakeBus::with_reading(reading(60.0, 2)));
        let adapter = UpowerPower::from_bus(bus.clone()).unwrap();
        let events = subscribed(&adapter);

        bus.fail("temporary read failure");
        bus.signal(BusEvent::ServiceLost);
        assert!(events.recv_timeout(Duration::from_millis(50)).is_err());
        assert_eq!(
            adapter.power_state(),
            PowerState {
                battery_percent: 60,
                charging: false
            }
        );
    }

    #[test]
    fn service_return_refreshes_and_emits_authoritative_state() {
        let bus = Arc::new(FakeBus::with_reading(reading(20.0, 2)));
        let adapter = UpowerPower::from_bus(bus.clone()).unwrap();
        let events = subscribed(&adapter);

        bus.signal(BusEvent::ServiceLost);
        bus.replace(reading(21.0, 1));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HardwareEvent::PowerChanged(PowerState {
                battery_percent: 21,
                charging: true
            })
        );
    }
}
