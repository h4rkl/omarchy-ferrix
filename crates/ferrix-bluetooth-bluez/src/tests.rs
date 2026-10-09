//! Mocked-bus tests: the reconciliation engine, discovery bounds, the
//! pairing agent's prompt flow, and address redaction. No D-Bus involved.

use super::*;
use std::sync::mpsc::Receiver;

type Gate = Mutex<Option<Receiver<Result<(), BusError>>>>;

struct MockBus {
    adapters: Mutex<Result<Vec<AdapterRecord>, String>>,
    devices: Mutex<Vec<DeviceRecord>>,
    watchers: Mutex<Vec<Sender<BusEvent>>>,
    writes: Mutex<Vec<(String, bool)>>,
    calls: Mutex<Vec<String>>,
    agent: Mutex<Option<Arc<dyn AgentCallbacks>>>,
    /// `pair` blocks on this until the test sends the result.
    pair_gate: Gate,
    connect_result: Mutex<Result<(), BusError>>,
}

impl Default for MockBus {
    fn default() -> Self {
        Self {
            adapters: Mutex::new(Ok(Vec::new())),
            devices: Mutex::new(Vec::new()),
            watchers: Mutex::new(Vec::new()),
            writes: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            agent: Mutex::new(None),
            pair_gate: Mutex::new(None),
            connect_result: Mutex::new(Ok(())),
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

    fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    fn agent(&self) -> Arc<dyn AgentCallbacks> {
        lock(&self.agent).clone().expect("agent registered")
    }

    fn gate(&self) -> Sender<Result<(), BusError>> {
        let (tx, rx) = mpsc::channel();
        *lock(&self.pair_gate) = Some(rx);
        tx
    }

    fn set_discovering(&self, discovering: bool) {
        if let Ok(adapters) = lock(&self.adapters).as_mut() {
            for adapter in adapters {
                adapter.discovering = discovering;
            }
        }
    }

    fn record(&self, call: String) {
        lock(&self.calls).push(call);
    }
}

impl BluezBus for MockBus {
    fn adapters(&self) -> Result<Vec<AdapterRecord>, String> {
        lock(&self.adapters).clone()
    }

    fn devices(&self) -> Result<Vec<DeviceRecord>, String> {
        Ok(lock(&self.devices).clone())
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

    fn register_agent(&self, agent: Arc<dyn AgentCallbacks>) -> Result<(), BusError> {
        self.record("register-agent".into());
        *lock(&self.agent) = Some(agent);
        Ok(())
    }

    fn start_discovery(&self, adapter: &str) -> Result<(), BusError> {
        self.record(format!("start-discovery {adapter}"));
        self.set_discovering(true);
        Ok(())
    }

    fn stop_discovery(&self, adapter: &str) -> Result<(), BusError> {
        self.record(format!("stop-discovery {adapter}"));
        self.set_discovering(false);
        Ok(())
    }

    fn pair(&self, device: &str) -> Result<(), BusError> {
        // Recorded after the gate is taken: a test that saw the call may
        // install the next attempt's gate.
        let gate = lock(&self.pair_gate).take();
        self.record(format!("pair {device}"));
        let result = match gate {
            Some(gate) => gate
                .recv()
                .unwrap_or_else(|_| Err(BusError::named("org.bluez.Error.Failed", ""))),
            None => Ok(()),
        };
        if result.is_ok() {
            for record in lock(&self.devices).iter_mut() {
                if record.path == device {
                    record.paired = true;
                }
            }
        }
        result
    }

    fn cancel_pairing(&self, device: &str) -> Result<(), BusError> {
        self.record(format!("cancel-pairing {device}"));
        Ok(())
    }

    fn set_trusted(&self, device: &str) -> Result<(), BusError> {
        self.record(format!("trust {device}"));
        Ok(())
    }

    fn connect_device(&self, device: &str) -> Result<(), BusError> {
        self.record(format!("connect {device}"));
        let result = lock(&self.connect_result).clone();
        if result.is_ok() {
            for record in lock(&self.devices).iter_mut() {
                if record.path == device {
                    record.connected = true;
                }
            }
        }
        result
    }

    fn disconnect_device(&self, device: &str) -> Result<(), BusError> {
        self.record(format!("disconnect {device}"));
        Ok(())
    }

    fn remove_device(&self, adapter: &str, device: &str) -> Result<(), BusError> {
        self.record(format!("remove {adapter} {device}"));
        lock(&self.devices).retain(|record| record.path != device);
        Ok(())
    }
}

const HCI0: &str = "/org/bluez/hci0";
const BUDS: &str = "/org/bluez/hci0/dev_66_77_88_99_AA_BB";
const NEW: &str = "/org/bluez/hci0/dev_00_11_22_33_44_55";

fn record(path: &str, powered: bool) -> AdapterRecord {
    AdapterRecord {
        path: path.to_owned(),
        powered,
        discovering: false,
    }
}

fn device(path: &str, name: Option<&str>, paired: bool, connected: bool) -> DeviceRecord {
    DeviceRecord {
        path: path.to_owned(),
        adapter: path
            .rsplit_once('/')
            .map(|(a, _)| a.to_owned())
            .unwrap_or_default(),
        name: name.map(str::to_owned),
        paired,
        connected,
        ..DeviceRecord::default()
    }
}

fn subscribed(adapter: &BluezBluetooth) -> Receiver<HardwareEvent> {
    let (sender, receiver) = mpsc::channel();
    adapter.subscribe(sender);
    receiver
}

fn fast() -> Timeouts {
    Timeouts {
        discovery: Duration::from_millis(80),
        prompt: Duration::from_millis(80),
    }
}

/// A powered hci0 with one bonded, connected device and one found device.
fn paired_setup() -> (Arc<MockBus>, Arc<BluezBluetooth>) {
    let bus = Arc::new(MockBus::with_adapters(vec![record(HCI0, true)]));
    *lock(&bus.devices) = vec![
        device(BUDS, Some("Buds"), true, true),
        DeviceRecord {
            rssi: Some(-55),
            icon: Some("audio-headphones".into()),
            ..device(NEW, Some("Headphones"), false, false)
        },
    ];
    let adapter = BluezBluetooth::from_bus_with(bus.clone(), fast()).unwrap();
    (bus, adapter)
}

fn id_of(adapter: &BluezBluetooth, name: &str) -> String {
    adapter
        .bluetooth_session()
        .unwrap()
        .devices
        .into_iter()
        .find(|d| d.name.as_deref() == Some(name))
        .unwrap()
        .id
}

fn phase(adapter: &BluezBluetooth) -> Option<PairingPhase> {
    adapter
        .bluetooth_session()
        .unwrap()
        .pairing
        .map(|p| p.phase)
}

/// Polls `check` for up to two seconds (worker threads finish async).
fn eventually(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Starts pairing and waits until the mock bus is inside `pair`.
fn start_pair(bus: &MockBus, adapter: &BluezBluetooth, id: &str) {
    let pairs = |bus: &MockBus| {
        bus.calls()
            .iter()
            .filter(|c| c.starts_with("pair "))
            .count()
    };
    let before = pairs(bus);
    adapter.pair_bluetooth_device(id).unwrap();
    assert!(eventually(|| pairs(bus) > before), "pair reached the bus");
}

fn responder() -> (Box<dyn FnOnce(bool) + Send>, Receiver<bool>) {
    let (tx, rx) = mpsc::channel();
    (
        Box::new(move |accept| {
            let _ = tx.send(accept);
        }),
        rx,
    )
}

#[test]
fn absent_bus_state_maps_to_absent_and_disabled() {
    let bus = Arc::new(MockBus::with_adapters(Vec::new()));
    let adapter = BluezBluetooth::from_bus(bus.clone()).unwrap();
    assert_eq!(
        adapter.bluetooth_state(),
        BluetoothState {
            present: false,
            enabled: false
        }
    );
    assert_eq!(
        adapter.bluetooth_session().unwrap(),
        BluetoothSession::default()
    );
    assert!(bus.calls().is_empty(), "no agent without a controller");
}

#[test]
fn deterministic_first_adapter_supplies_state_and_receives_commands() {
    let bus = Arc::new(MockBus::with_adapters(vec![
        record("/org/bluez/hci1", true),
        record(HCI0, false),
    ]));
    let adapter = BluezBluetooth::from_bus(bus.clone()).unwrap();
    assert_eq!(
        adapter.bluetooth_state(),
        BluetoothState {
            present: true,
            enabled: false
        }
    );
    assert_eq!(bus.calls(), ["register-agent"]);

    let state = adapter.set_bluetooth_enabled(true).unwrap();
    assert!(state.enabled);
    assert_eq!(*lock(&bus.writes), vec![(HCI0.to_owned(), true)]);
}

#[test]
fn mocked_bus_changes_publish_only_real_state_deltas() {
    let bus = Arc::new(MockBus::with_adapters(vec![record(HCI0, false)]));
    let adapter = BluezBluetooth::from_bus(bus.clone()).unwrap();
    let events = subscribed(&adapter);

    bus.replace(vec![record(HCI0, false)]);
    adapter.refresh().unwrap();
    assert!(events.recv_timeout(Duration::from_millis(50)).is_err());
    bus.replace(vec![record(HCI0, true)]);
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
    let bus = Arc::new(MockBus::with_adapters(vec![record(HCI0, true)]));
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
    // The agent is registered again once BlueZ is back.
    bus.signal(BusEvent::Changed);
    assert!(eventually(|| bus
        .calls()
        .iter()
        .filter(|c| *c == "register-agent")
        .count()
        == 2));
}

#[test]
fn devices_of_the_managed_controller_are_listed_without_addresses() {
    let bus = Arc::new(MockBus::with_adapters(vec![
        record(HCI0, true),
        record("/org/bluez/hci1", true),
    ]));
    *lock(&bus.devices) = vec![
        device(NEW, None, false, false),
        DeviceRecord {
            icon: Some("audio-headset".into()),
            ..device(BUDS, Some("Buds"), true, true)
        },
        device(
            "/org/bluez/hci1/dev_CC_DD_EE_FF_00_11",
            Some("Other"),
            true,
            false,
        ),
    ];
    let adapter = BluezBluetooth::from_bus(bus.clone()).unwrap();
    let devices = adapter.bluetooth_devices().unwrap();
    assert_eq!(devices.len(), 2, "only hci0 devices");
    assert_eq!(devices[0].name.as_deref(), Some("Buds"));
    assert_eq!(devices[0].kind, BluetoothDeviceKind::Audio);
    assert!(devices[0].paired && devices[0].connected);
    assert_eq!(devices[1].name, None);
    for d in &devices {
        assert!(
            d.id.starts_with("bt-") && !d.id.contains("dev_"),
            "{}",
            d.id
        );
        let json = format!("{d:?}");
        assert!(!json.contains("55") || !json.contains("44"), "{json}");
    }
    assert_eq!(devices, adapter.bluetooth_devices().unwrap(), "stable ids");
    assert_eq!(
        adapter.bluetooth_session().unwrap().devices,
        devices,
        "session lists the same devices"
    );
    bus.replace(Vec::new());
    assert!(
        adapter.bluetooth_devices().unwrap().is_empty(),
        "no controller"
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
    assert!(adapter.start_bluetooth_discovery().is_err());
    assert!(adapter.pair_bluetooth_device("bt-0").is_err());
}

#[test]
fn discovery_is_bounded_and_stopped_by_the_adapter() {
    let (bus, adapter) = paired_setup();
    let events = subscribed(&adapter);
    adapter.start_bluetooth_discovery().unwrap();
    let session = adapter.bluetooth_session().unwrap();
    assert!(session.discovery.active);
    let remaining = session.discovery.remaining_ms.unwrap();
    assert!(remaining <= 80, "{remaining}");
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(1)).unwrap(),
        HardwareEvent::BluetoothSessionChanged(summary) if summary.discovering
    ));
    assert!(eventually(|| bus
        .calls()
        .contains(&format!("stop-discovery {HCI0}"))));
    adapter.refresh().unwrap();
    assert!(!adapter.bluetooth_session().unwrap().discovery.active);
    // A found device carries signal bars and its kind.
    let found = adapter.bluetooth_session().unwrap();
    let headphones = found.device(&id_of(&adapter, "Headphones")).unwrap();
    assert_eq!(headphones.signal_bars, Some(4));
    assert_eq!(headphones.kind, BluetoothDeviceKind::Audio);
    // Stop is accepted even when nothing of ours runs.
    adapter.stop_bluetooth_discovery().unwrap();
}

#[test]
fn a_restart_supersedes_the_old_discovery_timer() {
    let bus = Arc::new(MockBus::with_adapters(vec![record(HCI0, true)]));
    let slow = Timeouts {
        discovery: Duration::from_millis(400),
        ..fast()
    };
    let adapter = BluezBluetooth::from_bus_with(bus.clone(), slow).unwrap();
    adapter.start_bluetooth_discovery().unwrap();
    thread::sleep(Duration::from_millis(200));
    adapter.start_bluetooth_discovery().unwrap();
    // Past the first deadline (400 ms), well before the second (600 ms).
    thread::sleep(Duration::from_millis(300));
    let stops = |bus: &MockBus| {
        bus.calls()
            .iter()
            .filter(|c| c.starts_with("stop-discovery"))
            .count()
    };
    assert_eq!(stops(&bus), 0, "the first timer no longer owns discovery");
    assert!(eventually(|| stops(&bus) == 1));
}

#[test]
fn numeric_comparison_prompt_reaches_the_session_and_success_connects() {
    let (bus, adapter) = paired_setup();
    let id = id_of(&adapter, "Headphones");
    let gate = bus.gate();
    start_pair(&bus, &adapter, &id);
    assert_eq!(phase(&adapter), Some(PairingPhase::Pairing));
    assert!(adapter.pair_bluetooth_device(&id).is_err(), "one at a time");
    assert!(eventually(|| bus.calls().contains(&format!("pair {NEW}"))));
    // BlueZ asks the agent; the prompt is in-memory only.
    let (respond, answer) = responder();
    bus.agent().prompt(
        NEW,
        PairingPhase::ConfirmPasskey { passkey: 482_913 },
        respond,
    );
    let pairing = adapter.bluetooth_session().unwrap().pairing.unwrap();
    assert_eq!(pairing.device, id);
    assert_eq!(
        pairing.phase,
        PairingPhase::ConfirmPasskey { passkey: 482_913 }
    );
    adapter.reply_bluetooth_pairing(true).unwrap();
    assert!(answer.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(phase(&adapter), Some(PairingPhase::Pairing));
    assert!(adapter.reply_bluetooth_pairing(true).is_err(), "answered");
    gate.send(Ok(())).unwrap();
    assert!(eventually(|| phase(&adapter) == Some(PairingPhase::Paired)));
    assert!(eventually(|| bus
        .calls()
        .contains(&format!("connect {NEW}"))));
    assert!(bus.calls().contains(&format!("trust {NEW}")));
    assert!(eventually(|| {
        let _ = adapter.refresh();
        adapter
            .bluetooth_session()
            .unwrap()
            .device(&id)
            .is_some_and(|d| d.paired && d.connected)
    }));
}

#[test]
fn declined_and_unanswered_prompts_fail_with_local_reasons() {
    let (bus, adapter) = paired_setup();
    let id = id_of(&adapter, "Headphones");
    // Declined: BlueZ then reports a rejection; the local reason wins.
    let gate = bus.gate();
    start_pair(&bus, &adapter, &id);
    let (respond, answer) = responder();
    bus.agent().prompt(NEW, PairingPhase::Authorize, respond);
    adapter.reply_bluetooth_pairing(false).unwrap();
    assert!(!answer.recv_timeout(Duration::from_secs(1)).unwrap());
    gate.send(Err(BusError::named(
        "org.bluez.Error.AuthenticationRejected",
        "",
    )))
    .unwrap();
    let declined = Some(PairingPhase::Failed {
        reason: PairingFailure::Declined,
    });
    assert!(eventually(|| phase(&adapter) == declined));
    // Unanswered: the adapter declines after the prompt timeout.
    let gate = bus.gate();
    start_pair(&bus, &adapter, &id);
    let (respond, answer) = responder();
    bus.agent()
        .prompt(NEW, PairingPhase::ConfirmPasskey { passkey: 1 }, respond);
    assert!(!answer.recv_timeout(Duration::from_secs(1)).unwrap());
    let timed_out = Some(PairingPhase::Failed {
        reason: PairingFailure::TimedOut,
    });
    assert_eq!(phase(&adapter), timed_out);
    gate.send(Err(BusError::named(
        "org.bluez.Error.AuthenticationCanceled",
        "",
    )))
    .unwrap();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(phase(&adapter), timed_out, "local reason kept");
}

#[test]
fn remote_failures_map_to_typed_reasons_and_pin_is_refused() {
    let (bus, adapter) = paired_setup();
    let id = id_of(&adapter, "Headphones");
    for (name, reason) in [
        (
            "org.bluez.Error.AuthenticationRejected",
            PairingFailure::Rejected,
        ),
        (
            "org.bluez.Error.AuthenticationTimeout",
            PairingFailure::TimedOut,
        ),
        (
            "org.bluez.Error.ConnectionAttemptFailed",
            PairingFailure::Unreachable,
        ),
        ("org.bluez.Error.Failed", PairingFailure::Failed),
    ] {
        let gate = bus.gate();
        start_pair(&bus, &adapter, &id);
        gate.send(Err(BusError::named(name, "Page Timeout")))
            .unwrap();
        let expected = Some(PairingPhase::Failed { reason });
        assert!(eventually(|| phase(&adapter) == expected), "{name}");
    }
    let gate = bus.gate();
    start_pair(&bus, &adapter, &id);
    bus.agent().refuse(NEW, PairingFailure::PinRequired);
    gate.send(Err(BusError::named(
        "org.bluez.Error.AuthenticationFailed",
        "",
    )))
    .unwrap();
    let pin = Some(PairingPhase::Failed {
        reason: PairingFailure::PinRequired,
    });
    assert!(eventually(|| phase(&adapter) == pin));
    // Cancel of a finished attempt dismisses it.
    adapter.cancel_bluetooth_pairing().unwrap();
    assert_eq!(phase(&adapter), None);
    // Cancel of a running one asks BlueZ to cancel.
    let gate = bus.gate();
    start_pair(&bus, &adapter, &id);
    adapter.cancel_bluetooth_pairing().unwrap();
    assert!(bus.calls().contains(&format!("cancel-pairing {NEW}")));
    gate.send(Err(BusError::named(
        "org.bluez.Error.AuthenticationCanceled",
        "",
    )))
    .unwrap();
    let canceled = Some(PairingPhase::Failed {
        reason: PairingFailure::Canceled,
    });
    assert!(eventually(|| phase(&adapter) == canceled));
}

#[test]
fn incoming_display_passkey_and_service_authorization() {
    let (bus, adapter) = paired_setup();
    let agent = bus.agent();
    agent.display(NEW, 205_718);
    let pairing = adapter.bluetooth_session().unwrap().pairing.unwrap();
    assert_eq!(pairing.device, id_of(&adapter, "Headphones"));
    assert_eq!(
        pairing.phase,
        PairingPhase::DisplayPasskey { passkey: 205_718 }
    );
    // The remote finished the bond: the refresh notices.
    lock(&bus.devices)[1].paired = true;
    adapter.refresh().unwrap();
    assert_eq!(phase(&adapter), Some(PairingPhase::Paired));
    assert!(agent.service_allowed(BUDS), "bonded");
    assert!(!agent.service_allowed("/org/bluez/hci0/dev_99_99_99_99_99_99"));
    // BlueZ cancelling a prompt answers it.
    let (respond, answer) = responder();
    agent.prompt(BUDS, PairingPhase::Authorize, respond);
    agent.cancel();
    assert!(!answer.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(
        phase(&adapter),
        Some(PairingPhase::Failed {
            reason: PairingFailure::Canceled
        })
    );
}

#[test]
fn connect_failures_are_redacted_and_forget_removes_the_bond() {
    let (bus, adapter) = paired_setup();
    let buds = id_of(&adapter, "Buds");
    *lock(&bus.connect_result) = Err(BusError::named(
        "org.bluez.Error.Failed",
        "br-connection-page-timeout for 66:77:88:99:AA:BB",
    ));
    adapter.connect_bluetooth_device(&buds).unwrap();
    assert!(eventually(|| {
        adapter
            .bluetooth_session()
            .unwrap()
            .operation
            .is_some_and(|o| o.failed.is_some())
    }));
    let operation = adapter.bluetooth_session().unwrap().operation.unwrap();
    assert_eq!(operation.action, BluetoothAction::Connect);
    let text = operation.failed.unwrap();
    assert_eq!(text, "br-connection-page-timeout for <device>");
    // Unpaired devices cannot be connected.
    let new = id_of(&adapter, "Headphones");
    assert!(adapter.connect_bluetooth_device(&new).is_err());
    adapter.forget_bluetooth_device(&buds).unwrap();
    assert!(bus.calls().contains(&format!("remove {HCI0} {BUDS}")));
    let session = adapter.bluetooth_session().unwrap();
    assert!(session.device(&buds).is_none());
    assert!(session.operation.is_none(), "forget clears the operation");
    assert!(adapter.forget_bluetooth_device("bt-unknown").is_err());
}

#[test]
fn redaction_removes_both_address_forms() {
    assert_eq!(
        redact("Device /org/bluez/hci0/dev_00_11_22_33_44_55 not found"),
        "Device /org/bluez/hci0/dev_<device> not found"
    );
    assert_eq!(redact("aa:bb:cc:dd:ee:ff left"), "<device> left");
    assert_eq!(redact("10:30 meeting · ok"), "10:30 meeting · ok");
    assert_eq!(redact("Page Timeout"), "Page Timeout");
}
