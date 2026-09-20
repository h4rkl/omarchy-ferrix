//! End-to-end mock-control through the daemon with the real mock adapter:
//! the scenario runner's view. Only the channel is used (local client verbs
//! and forwarded backend mutations); records are read from the capturing
//! recorder. Evidence label `mock`; the clock advances only on `tick`.

use std::sync::Arc;

use ferrix_mock_control::{Ack, Handler};
use ferrix_telephony_mock::{
    MockControl, MockModem, MockTelephonyConfig, OUTGOING_PROGRESS_MS, RINGING_TIMEOUT_MS,
    SMS_DELAYED_DELIVERY_MS, SMS_SEND_MS,
};
use ferrix_telephonyd::records::{Captured, Recorder};
use ferrix_telephonyd::testing::temp_data_dir;
use ferrix_telephonyd::{LocalControl, Service, ServiceOptions, SmsStore};
use serde_json::Value;

const NUMBER: &str = "+15550100042";
const BODY: &str = "synthetic body text";

fn harness(tag: &str, config: MockTelephonyConfig) -> (LocalControl, Captured) {
    let mock = MockModem::new(config);
    let (recorder, captured) = Recorder::capturing("mock");
    let service = Service::new(
        Arc::new(mock.clone()),
        SmsStore::new(temp_data_dir(&format!("e2e-{tag}"))),
        recorder,
        ServiceOptions::default(),
    )
    .unwrap();
    service.spawn_event_loop();
    (
        LocalControl::new(service, Arc::new(MockControl::new(mock))),
        captured,
    )
}

fn ok(control: &LocalControl, line: &str) -> (Vec<String>, Value) {
    match control.handle(line) {
        Ack::Ok { events, state, .. } => (events, state),
        Ack::Error { reason, .. } => panic!("{line}: {reason}"),
    }
}

fn err(control: &LocalControl, line: &str) -> String {
    match control.handle(line) {
        Ack::Error { ack, reason } => {
            assert!(ack.is_some(), "error acks name the command: {reason}");
            reason
        }
        Ack::Ok { .. } => panic!("{line} unexpectedly succeeded"),
    }
}

/// Acks are written after the records they cause, so no waiting is needed.
fn records(captured: &Captured) -> Vec<Value> {
    captured
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r["event"] == "telephony-event")
        .cloned()
        .collect()
}

fn call_states(captured: &Captured) -> Vec<(String, Option<String>)> {
    records(captured)
        .iter()
        .filter(|e| e["kind"] == "call-changed")
        .map(|e| {
            (
                e["call"]["state"].as_str().unwrap().to_owned(),
                e["call"]["end_reason"]["kind"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

fn sms_states(captured: &Captured) -> Vec<(String, Option<String>)> {
    records(captured)
        .iter()
        .filter(|e| e["kind"] == "sms-changed")
        .map(|e| {
            (
                e["sms"]["state"].as_str().unwrap().to_owned(),
                e["sms"]["failure"]["kind"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

fn assert_no_leak(captured: &Captured) {
    let text = serde_json::to_string(&*captured.lock().unwrap()).unwrap();
    assert!(!text.contains("0100042"), "number in records: {text}");
    assert!(!text.contains(BODY), "body in records: {text}");
    assert!(!text.contains("\"number\":"), "{text}");
    assert!(!text.contains("\"body\":"), "{text}");
}

#[test]
fn outgoing_call_connected_dtmf_hangup() {
    let (control, captured) = harness("connected", MockTelephonyConfig::default());
    let (events, state) = ok(
        &control,
        &format!(r#"{{"cmd":"dial","number":"{NUMBER}"}}"#),
    );
    assert_eq!(events, ["call-changed"]);
    assert_eq!(state["calls"][0]["call_id"], "c1");
    assert_eq!(state["calls"][0]["state"], "dialing");
    ok(
        &control,
        &format!(r#"{{"cmd":"tick","ms":{OUTGOING_PROGRESS_MS}}}"#),
    );
    assert_eq!(call_states(&captured).last().unwrap().0, "active");
    ok(&control, r#"{"cmd":"dtmf","digit":"5"}"#);
    ok(&control, r#"{"cmd":"hold"}"#);
    ok(&control, r#"{"cmd":"resume"}"#);
    let (events, _) = ok(&control, r#"{"cmd":"hangup"}"#);
    assert_eq!(events, ["call-changed"]);
    let all = call_states(&captured);
    let states: Vec<&str> = all.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(states, ["dialing", "active", "held", "active", "ended"]);
    assert_eq!(
        call_states(&captured).last().unwrap().1.as_deref(),
        Some("local-hangup")
    );
    assert_no_leak(&captured);
}

#[test]
fn outgoing_call_busy_rejected_timeout_policies() {
    for (result, reason, delay) in [
        ("busy", "busy", OUTGOING_PROGRESS_MS),
        ("rejected", "rejected", OUTGOING_PROGRESS_MS),
        ("timeout", "timed-out", RINGING_TIMEOUT_MS),
    ] {
        let (control, captured) = harness(result, MockTelephonyConfig::default());
        ok(
            &control,
            &format!(r#"{{"cmd":"set-outgoing-result","result":"{result}"}}"#),
        );
        ok(
            &control,
            &format!(r#"{{"cmd":"dial","number":"{NUMBER}"}}"#),
        );
        ok(&control, &format!(r#"{{"cmd":"tick","ms":{}}}"#, delay - 1));
        assert_eq!(call_states(&captured).len(), 1, "{result}");
        ok(&control, r#"{"cmd":"tick","ms":1}"#);
        assert_eq!(
            call_states(&captured).last().unwrap(),
            &("ended".to_owned(), Some(reason.to_owned())),
            "{result}"
        );
        assert_no_leak(&captured);
    }
}

#[test]
fn outgoing_call_no_service_and_emergency_are_ack_errors() {
    let (control, captured) = harness("no-service", MockTelephonyConfig::default());
    ok(
        &control,
        r#"{"cmd":"set-registration","registration":"searching"}"#,
    );
    let reason = err(
        &control,
        &format!(r#"{{"cmd":"dial","number":"{NUMBER}"}}"#),
    );
    assert!(reason.starts_with("no-service"), "{reason}");
    let reason = err(&control, r#"{"cmd":"dial","number":"112"}"#);
    assert!(reason.contains("emergency"), "{reason}");
    let reason = err(&control, r#"{"cmd":"dial","number":"5550100042"}"#);
    assert!(reason.starts_with("invalid-number"), "{reason}");
    ok(&control, r#"{"cmd":"set-radio","power":"off"}"#);
    let reason = err(
        &control,
        &format!(r#"{{"cmd":"dial","number":"{NUMBER}"}}"#),
    );
    assert!(reason.starts_with("no-service"), "{reason}");
    ok(&control, r#"{"cmd":"set-modem","state":"absent"}"#);
    let reason = err(
        &control,
        &format!(r#"{{"cmd":"dial","number":"{NUMBER}"}}"#),
    );
    assert!(reason.starts_with("no-modem"), "{reason}");
    assert!(call_states(&captured).is_empty());
    assert_no_leak(&captured);
}

#[test]
fn incoming_call_answer_route_remote_hangup() {
    let (control, captured) = harness("incoming", MockTelephonyConfig::default());
    let (events, _) = ok(
        &control,
        &format!(r#"{{"cmd":"incoming-call","number":"{NUMBER}"}}"#),
    );
    assert_eq!(events, ["call-added"]);
    assert_eq!(call_states(&captured), [("ringing".to_owned(), None)]);
    let (events, state) = ok(&control, r#"{"cmd":"answer"}"#);
    assert_eq!(events, ["call-changed"]);
    assert_eq!(state["calls"][0]["state"], "active");
    let (events, state) = ok(&control, r#"{"cmd":"set-audio-route","route":"speaker"}"#);
    assert_eq!(events, ["audio-route-changed"]);
    assert_eq!(state["audio"], "speaker");
    ok(&control, r#"{"cmd":"remote-hangup"}"#);
    assert_eq!(
        call_states(&captured).last().unwrap(),
        &("ended".to_owned(), Some("remote-hangup".to_owned()))
    );
    let audio = records(&captured)
        .into_iter()
        .find(|e| e["kind"] == "audio-route-changed")
        .unwrap();
    assert_eq!(audio["audio"]["active"], "speaker");
    assert_no_leak(&captured);
}

#[test]
fn sim_pin_locked_unlock() {
    let (control, captured) = harness("pin", MockTelephonyConfig::default());
    ok(
        &control,
        r#"{"cmd":"set-sim","state":"locked","attempts_left":3}"#,
    );
    let reason = err(
        &control,
        &format!(r#"{{"cmd":"dial","number":"{NUMBER}"}}"#),
    );
    assert!(reason.starts_with("sim-locked"), "{reason}");
    let reason = err(&control, r#"{"cmd":"enter-pin","pin":"1234"}"#);
    assert!(reason.starts_with("sim-locked"), "{reason}");
    let (events, state) = ok(&control, r#"{"cmd":"enter-pin","pin":"0000"}"#);
    assert_eq!(events, ["sim-changed", "registration-changed"]);
    assert_eq!(state["sim"], "ready");
    assert_eq!(state["registration"], "searching");
    ok(&control, r#"{"cmd":"tick","ms":1000}"#);
    let sims: Vec<Value> = records(&captured)
        .into_iter()
        .filter(|e| e["kind"] == "sim-changed")
        .collect();
    assert_eq!(sims[0]["sim"]["state"], "locked");
    assert_eq!(sims[0]["sim"]["attempts_left"], 3);
    assert_eq!(sims[1]["sim"]["attempts_left"], 2);
    assert_eq!(sims[2]["sim"]["state"], "ready");
    let all = records(&captured);
    let registration = all
        .iter()
        .rev()
        .find(|e| e["kind"] == "registration-changed")
        .unwrap();
    assert_eq!(registration["registration"]["state"], "registered");
    assert_eq!(registration["registration"]["roaming"], false);
    assert_eq!(registration["access"], "lte");
    assert_eq!(all.last().unwrap()["kind"], "signal-changed");
    assert_eq!(all.last().unwrap()["signal"]["bars"], 3);
    assert_no_leak(&captured);
}

#[test]
fn sms_outgoing_success_failure_delayed_delivery() {
    let (control, captured) = harness("sms", MockTelephonyConfig::default());
    let (events, state) = ok(
        &control,
        &format!(r#"{{"cmd":"send-sms","number":"{NUMBER}","body":"{BODY}"}}"#),
    );
    assert_eq!(events, ["sms-changed"]);
    assert_eq!(state["sms"][0]["message_id"], "m1");
    ok(&control, &format!(r#"{{"cmd":"tick","ms":{SMS_SEND_MS}}}"#));
    ok(&control, r#"{"cmd":"deliver-sms"}"#);
    assert_eq!(
        sms_states(&captured),
        [
            ("sending".to_owned(), None),
            ("sent".to_owned(), None),
            ("delivered".to_owned(), None)
        ]
    );

    ok(&control, r#"{"cmd":"set-sms-result","result":"failed"}"#);
    ok(
        &control,
        &format!(r#"{{"cmd":"send-sms","number":"{NUMBER}","body":"{BODY}"}}"#),
    );
    ok(&control, &format!(r#"{{"cmd":"tick","ms":{SMS_SEND_MS}}}"#));
    assert_eq!(
        sms_states(&captured).last().unwrap(),
        &("failed".to_owned(), Some("rejected".to_owned()))
    );

    ok(
        &control,
        r#"{"cmd":"set-sms-result","result":"delayed-delivery"}"#,
    );
    ok(
        &control,
        &format!(r#"{{"cmd":"send-sms","number":"{NUMBER}","body":"{BODY}"}}"#),
    );
    ok(&control, &format!(r#"{{"cmd":"tick","ms":{SMS_SEND_MS}}}"#));
    assert_eq!(sms_states(&captured).last().unwrap().0, "sent");
    ok(
        &control,
        &format!(r#"{{"cmd":"tick","ms":{}}}"#, SMS_DELAYED_DELIVERY_MS - 1),
    );
    assert_eq!(sms_states(&captured).last().unwrap().0, "sent");
    ok(&control, r#"{"cmd":"tick","ms":1}"#);
    assert_eq!(sms_states(&captured).last().unwrap().0, "delivered");

    let (events, _) = ok(
        &control,
        &format!(r#"{{"cmd":"incoming-sms","number":"{NUMBER}","body":"{BODY}"}}"#),
    );
    assert_eq!(events, ["sms-received"]);
    let (events, state) = ok(&control, r#"{"cmd":"mark-read","message_id":"m4"}"#);
    assert_eq!(events, ["sms-changed"]);
    assert_eq!(state["sms"][3]["state"], "read");
    let reason = err(
        &control,
        &format!(r#"{{"cmd":"send-sms","number":"{NUMBER}","body":""}}"#),
    );
    assert!(reason.starts_with("protocol"), "{reason}");
    assert_no_leak(&captured);
}

#[test]
fn unknown_and_malformed_commands_are_ack_errors() {
    let (control, _) = harness("bad", MockTelephonyConfig::default());
    assert!(err(&control, r#"{"cmd":"explode"}"#).contains("unknown variant"));
    assert!(
        err(
            &control,
            &format!(r#"{{"cmd":"dial","number":"{NUMBER}","forged":1}}"#)
        )
        .contains("unknown field")
    );
    assert!(!control.handle("{").is_ok());
    assert!(
        err(
            &control,
            r#"{"cmd":"set-modem","state":"failed","reason":"init-timeout","x":1}"#
        )
        .contains("unknown field")
    );
    let (events, state) = ok(
        &control,
        r#"{"cmd":"set-modem","state":"failed","reason":"init-timeout"}"#,
    );
    assert_eq!(events[0], "modem");
    assert_eq!(state["modem"], "failed");
}
