//! End-to-end mock-control through the daemon with the real mock adapter:
//! the scenario runner's view. Only the channel is used; records are read
//! from the capturing recorder. Evidence label `mock`; deterministic clock.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ferrix_mock_control::{Ack, Handler};
use ferrix_network_mock::{MockNetwork, MockNetworkConfig};
use ferrix_networkd::control::MockControl;
use ferrix_networkd::identity::{DenyAll, Identity};
use ferrix_networkd::records::{Captured, Recorder};
use ferrix_networkd::service::Service;
use serde_json::Value;

fn harness(config: MockNetworkConfig) -> (Arc<MockControl>, Captured) {
    let mock = MockNetwork::new(config);
    let (recorder, captured) = Recorder::capturing("mock");
    let service = Service::new(
        mock.clone(),
        Arc::new(DenyAll) as Arc<dyn Identity>,
        recorder,
    )
    .unwrap();
    service.start();
    (MockControl::new(service, mock), captured)
}

fn ok(control: &MockControl, line: &str) -> Value {
    match control.handle(line) {
        Ack::Ok { state, .. } => state,
        Ack::Error { reason, .. } => panic!("{line}: {reason}"),
    }
}

/// Waits (bounded) until `count` network-event records exist, then returns
/// them. Ordering, not timing, is asserted.
fn network_events(captured: &Captured, count: usize) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events: Vec<Value> = captured
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "network-event")
            .cloned()
            .collect();
        if events.len() >= count || Instant::now() > deadline {
            return events;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn network_events_until(captured: &Captured, predicate: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events: Vec<Value> = captured
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "network-event")
            .cloned()
            .collect();
        if predicate(&events) || Instant::now() > deadline {
            return events;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn kinds(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn wifi_auth_failed_scenario_through_the_channel() {
    let (control, captured) = harness(MockNetworkConfig {
        connected: None,
        ..MockNetworkConfig::default()
    });
    ok(
        &control,
        r#"{"cmd":"set-connect-result","result":"auth-failed"}"#,
    );
    let state = ok(
        &control,
        r#"{"cmd":"connect","ssid":"ferrix-lab","passphrase":"mock-passphrase"}"#,
    );
    assert_eq!(state["connection"], "associating");
    ok(&control, r#"{"cmd":"tick","ms":1000}"#);
    let events = network_events(&captured, 3);
    assert_eq!(
        kinds(&events),
        [
            "connection-changed",
            "connection-changed",
            "connection-changed"
        ]
    );
    assert_eq!(events[0]["connection"]["state"], "associating");
    assert_eq!(events[1]["connection"]["state"], "authenticating");
    assert_eq!(
        events[2]["connection"],
        serde_json::json!({"state": "failed", "error": {"kind": "authentication-failed"}})
    );
    for record in captured.lock().unwrap().iter() {
        let text = record.to_string();
        assert!(!text.contains("mock-passphrase"), "{text}");
        assert!(!text.contains("ferrix-lab"), "{text}");
    }
}

#[test]
fn radios_disabled_scan_connect_and_captive_portal() {
    let (control, captured) = harness(MockNetworkConfig {
        connected: None,
        ..MockNetworkConfig::default()
    });
    ok(&control, r#"{"cmd":"set-radio-enabled","enabled":false}"#);
    let Ack::Error { reason, .. } = control.handle(r#"{"cmd":"scan"}"#) else {
        panic!("scan with the radio off must be a typed error");
    };
    assert!(reason.starts_with("radio-disabled"), "{reason}");
    ok(&control, r#"{"cmd":"set-radio-enabled","enabled":true}"#);
    ok(&control, r#"{"cmd":"scan"}"#);
    ok(&control, r#"{"cmd":"tick","ms":1000}"#);
    ok(&control, r#"{"cmd":"connect","ssid":"ferrix-guest"}"#);
    ok(&control, r#"{"cmd":"tick","ms":1500}"#);
    ok(
        &control,
        r#"{"cmd":"set-connectivity","connectivity":"portal","url":"http://portal.example/login"}"#,
    );
    let events = network_events_until(&captured, |events| {
        events.last().is_some_and(|event| {
            event["kind"] == "connectivity-changed" && event["connectivity"]["state"] == "portal"
        })
    });
    let kinds = kinds(&events);
    assert_eq!(
        kinds[..2],
        [
            "radio-changed".to_owned(),
            "access-points-changed".to_owned()
        ]
    );
    assert_eq!(events[0]["radio"], serde_json::json!({"state": "disabled"}));
    assert!(kinds.contains(&"scan-changed".to_owned()));
    let completed = events
        .iter()
        .find(|e| e["scan"]["state"] == "completed")
        .expect("scan completes on tick");
    assert_eq!(completed["scan"]["count"], 2);
    let connected = events
        .iter()
        .find(|e| e["connection"]["state"] == "connected")
        .expect("open network joins");
    assert!(
        connected["connection"]["ap"]
            .as_str()
            .unwrap()
            .starts_with("ap-")
    );
    let portal = events.last().expect("portal is the last delta");
    assert_eq!(portal["kind"], "connectivity-changed");
    assert_eq!(portal["connectivity"]["state"], "portal");
}

#[test]
fn crash_backend_ends_in_service_restarted_after_tick() {
    let (control, captured) = harness(MockNetworkConfig::default());
    ok(&control, r#"{"cmd":"crash-backend"}"#);
    let Ack::Error { .. } = control.handle(r#"{"cmd":"scan"}"#) else {
        panic!("a crashed backend refuses client verbs");
    };
    ok(&control, r#"{"cmd":"tick","ms":10}"#);
    let events = network_events(&captured, 2);
    assert_eq!(kinds(&events), ["radio-changed", "service-restarted"]);
    assert_eq!(events[0]["radio"]["state"], "unavailable");
    assert_eq!(events[1]["snapshot"]["connection"], "connected");
    let all: Vec<String> = captured
        .lock()
        .unwrap()
        .iter()
        .map(|r| r["event"].as_str().unwrap().to_owned())
        .collect();
    assert!(all.contains(&"network-backend-lost".to_owned()));
    // The service is usable again.
    ok(&control, r#"{"cmd":"disconnect-request"}"#);
    ok(&control, r#"{"cmd":"forget","ssid":"ferrix-lab"}"#);
    let state = ok(&control, r#"{"cmd":"state"}"#);
    assert_eq!(state["connection"], "disconnected");
    assert_eq!(state["profiles"].as_array().unwrap().len(), 0);
}
