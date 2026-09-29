//! Machine-readable records: one JSON line per record on stdout, `evidence`
//! first, mirroring `ferrix-appd` and `ferrix-core`.
//!
//! Records carry ids and states only. No number, body, PIN, or radio
//! identifier is ever placed in a record; the architecture guard scans for
//! those keys.

use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use ferrix_telephony_api::{Capabilities, TelephonyEvent};
use serde_json::{Map, Value, json};

/// Evidence label for the QEMU guest.
pub const EVIDENCE_QEMU: &str = "QEMU";
/// Evidence label for nested host runs from the mock profile.
pub const EVIDENCE_MOCK: &str = "mock";

/// Resolve the process evidence label without encoding a physical board in
/// this service crate.
pub fn runtime_evidence(nested: bool) -> String {
    if nested {
        EVIDENCE_MOCK.to_owned()
    } else {
        std::env::var("FERRIX_EVIDENCE")
            .ok()
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| EVIDENCE_QEMU.to_owned())
    }
}

/// Prints one JSON record per line and flushes, so serial captures are exact.
pub fn emit(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, value);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

/// Builds a record with `evidence` first, then `event`, then `fields`.
pub fn record(
    evidence: &str,
    event: &str,
    fields: impl IntoIterator<Item = (&'static str, Value)>,
) -> Value {
    let mut map = Map::new();
    map.insert("evidence".to_owned(), Value::String(evidence.to_owned()));
    map.insert("event".to_owned(), Value::String(event.to_owned()));
    for (key, value) in fields {
        map.insert(key.to_owned(), value);
    }
    Value::Object(map)
}

/// Records captured by [`Recorder::capturing`].
pub type Captured = Arc<Mutex<Vec<Value>>>;

type Sink = Arc<dyn Fn(&Value) + Send + Sync>;

/// Emits records with a fixed evidence label to a sink (stdout by default).
#[derive(Clone)]
pub struct Recorder {
    evidence: Arc<str>,
    sink: Sink,
}

impl Recorder {
    pub fn stdout(evidence: &str) -> Self {
        Self {
            evidence: Arc::from(evidence),
            sink: Arc::new(emit),
        }
    }

    /// Records are collected in memory; for tests.
    pub fn capturing(evidence: &str) -> (Self, Captured) {
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let recorder = Self {
            evidence: Arc::from(evidence),
            sink: Arc::new(move |value: &Value| {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(value.clone());
            }),
        };
        (recorder, captured)
    }

    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    pub fn emit(&self, event: &str, fields: impl IntoIterator<Item = (&'static str, Value)>) {
        (self.sink)(&record(&self.evidence, event, fields));
    }

    pub fn ready(&self, socket: &Path, backend: &str, capabilities: &Capabilities) {
        self.emit(
            "telephonyd-ready",
            [
                ("socket", Value::from(socket.display().to_string())),
                ("backend", Value::from(backend)),
                (
                    "capabilities",
                    Value::Array(capabilities.iter().map(|c| Value::from(c.name())).collect()),
                ),
            ],
        );
    }

    pub fn failed(&self, reason: &str) {
        self.emit(
            "telephonyd-failed",
            [
                ("status", Value::from("failed")),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn socket_error(&self, error: &str) {
        self.emit("telephonyd-socket-error", [("error", Value::from(error))]);
    }

    /// Mock-control channel status: `ready`, `none`, `unavailable`, or `failed`.
    pub fn mock_control(&self, status: &str, path: Option<&Path>, detail: Option<&str>) {
        self.emit(
            "telephony-mock-control",
            [
                ("status", Value::from(status)),
                (
                    "path",
                    path.map_or(Value::Null, |p| Value::from(p.display().to_string())),
                ),
                ("detail", detail.map_or(Value::Null, Value::from)),
            ],
        );
    }

    /// Nested development only: the daemon accepts every peer as fully
    /// granted because no application service is reachable.
    pub fn unauthenticated_warning(&self) {
        self.emit(
            "telephonyd-unauthenticated",
            [("status", Value::from("warning"))],
        );
    }

    /// A published event as a record: `kind` is the event name and the
    /// payload mirrors the event's wire form with parties and bodies
    /// removed. Calls carry `{id, direction, state, end_reason}`, messages
    /// `{id, direction, state, failure, parts}`; `service-restarted` carries
    /// a summary, never the snapshot.
    pub fn telephony_event(&self, event: &TelephonyEvent) {
        let mut fields: Vec<(&'static str, Value)> = vec![("kind", Value::from(event.name()))];
        match event {
            TelephonyEvent::ModemChanged { modem } => fields.push(("modem", to_value(modem))),
            TelephonyEvent::SimChanged { sim } => fields.push(("sim", to_value(sim))),
            TelephonyEvent::RegistrationChanged {
                registration,
                operator,
                access,
            } => {
                fields.push(("registration", to_value(registration)));
                fields.push(("operator", to_value(operator)));
                fields.push(("access", to_value(access)));
            }
            TelephonyEvent::SignalChanged { signal } => fields.push(("signal", to_value(signal))),
            TelephonyEvent::CallChanged { call } => {
                fields.push(("call_id", Value::from(call.id.as_str())));
                fields.push((
                    "call",
                    json!({
                        "id": call.id,
                        "direction": call.direction,
                        "state": call.state,
                        "end_reason": call.end_reason,
                        "started_at_ms": call.started_at_ms,
                    }),
                ));
            }
            TelephonyEvent::SmsChanged { sms } => {
                fields.push(("message_id", Value::from(sms.id.as_str())));
                fields.push((
                    "sms",
                    json!({
                        "id": sms.id,
                        "direction": sms.direction,
                        "state": sms.state,
                        "failure": sms.failure,
                        "parts": sms.parts,
                        "timestamp_ms": sms.timestamp_ms,
                    }),
                ));
            }
            TelephonyEvent::DataChanged { data } => fields.push(("data", to_value(data))),
            TelephonyEvent::AudioRouteChanged { audio } => fields.push(("audio", to_value(audio))),
            TelephonyEvent::RadioPowerChanged { radio } => fields.push(("radio", to_value(radio))),
            TelephonyEvent::CapabilitiesChanged { capabilities } => {
                fields.push(("capabilities", to_value(capabilities)));
            }
            TelephonyEvent::ServiceRestarted { state } => fields.push((
                "state",
                json!({
                    "modem": state.modem,
                    "sim": state.sim,
                    "registration": state.registration,
                    "radio": state.radio,
                    "calls": state.calls.len(),
                    "sms": state.sms.len(),
                }),
            )),
        }
        self.emit("telephony-event", fields);
    }

    /// A request refused by capability gating (ADR-004).
    pub fn request_denied(&self, app: Option<&str>, capability: &str, request: &str) {
        self.emit(
            "telephonyd-request-denied",
            [
                ("app", app.map_or(Value::Null, Value::from)),
                ("capability", Value::from(capability)),
                ("request", Value::from(request)),
            ],
        );
    }

    /// A message refused by ordering or parsing rules.
    pub fn request_rejected(&self, request: Option<&str>, reason: &str) {
        self.emit(
            "telephonyd-request-rejected",
            [
                ("request", request.map_or(Value::Null, Value::from)),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn store_error(&self, path: &Path, reason: &str) {
        self.emit(
            "telephony-store-error",
            [
                ("path", Value::from(path.display().to_string())),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn backend_lost(&self, reason: &str) {
        self.emit("telephony-backend-lost", [("reason", Value::from(reason))]);
    }
}

fn to_value(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Event names of captured records, in order; for tests.
pub fn captured_events(captured: &Captured) -> Vec<String> {
    captured
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter_map(|v| v.get("event").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_telephony_api::{
        Call, CallId, CallState, Direction, E164, MessageId, PartyRef, Sms, SmsState,
    };

    #[test]
    fn records_lead_with_evidence_then_event() {
        let value = record(
            EVIDENCE_MOCK,
            "telephonyd-ready",
            [("backend", Value::from("mock"))],
        );
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"evidence":"mock","event":"telephonyd-ready","backend":"mock"}"#
        );
    }

    #[test]
    fn event_records_carry_ids_and_states_only() {
        let (recorder, captured) = Recorder::capturing(EVIDENCE_QEMU);
        let party = PartyRef::new(E164::new("+15550100042").unwrap());
        recorder.telephony_event(&TelephonyEvent::CallChanged {
            call: Call {
                id: CallId::from("c1"),
                direction: Direction::Incoming,
                state: CallState::Ringing,
                party: party.clone(),
                started_at_ms: 0,
                end_reason: None,
            },
        });
        recorder.telephony_event(&TelephonyEvent::SmsChanged {
            sms: Sms {
                id: MessageId::from("m1"),
                direction: Direction::Incoming,
                party,
                body: "secret".into(),
                parts: Vec::new(),
                state: SmsState::Received,
                timestamp_ms: 0,
                failure: None,
            },
        });
        let text = serde_json::to_string(&*captured.lock().unwrap()).unwrap();
        assert!(!text.contains("0100042"), "{text}");
        assert!(!text.contains("secret"), "{text}");
        assert!(text.contains(r#""call_id":"c1""#));
        assert!(text.contains(r#""message_id":"m1""#));
        let records = captured.lock().unwrap();
        assert_eq!(records[0]["kind"], "call-changed");
        assert_eq!(records[0]["call"]["direction"], "incoming");
        assert_eq!(records[0]["call"]["state"], "ringing");
        assert!(records[0]["call"].get("party").is_none());
        assert_eq!(records[1]["kind"], "sms-changed");
        assert_eq!(records[1]["sms"]["state"], "received");
        assert!(records[1]["sms"].get("body").is_none());
        drop(records);
        assert_eq!(
            captured_events(&captured),
            ["telephony-event", "telephony-event"]
        );
    }
}
