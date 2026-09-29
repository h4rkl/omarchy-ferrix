//! Machine-readable records: one JSON line per record on stdout, `evidence`
//! first, then `event`, mirroring `ferrix-appd` and `ferrix-core`.
//!
//! Records carry ids, digests, and state names only. No SSID text, no
//! hardware address, no passphrase ever reaches a record: the event summary
//! comes from `ferrix_network_api`, which has none of those.

use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use ferrix_network_api::NetworkEvent;
use serde_json::{Map, Value};

/// Evidence label for the QEMU guest.
pub const EVIDENCE_QEMU: &str = "QEMU";
/// Evidence label for nested host runs.
pub const EVIDENCE_MOCK: &str = "mock";

/// Resolve the process evidence label without making the daemon know a board
/// name. Physical target bootstrap supplies the exact redacted label.
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

    /// The socket is bound and the backend answered its first snapshot.
    pub fn ready(&self, socket: &Path, backend: &str) {
        self.emit(
            "networkd-ready",
            [
                ("socket", Value::from(socket.display().to_string())),
                ("backend", Value::from(backend)),
            ],
        );
    }

    pub fn failed(&self, reason: &str) {
        self.emit(
            "networkd-failed",
            [
                ("status", Value::from("failed")),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn socket_error(&self, error: &str) {
        self.emit("networkd-socket-error", [("error", Value::from(error))]);
    }

    /// Mock-control channel state: `ready`, `none`, or `failed`.
    pub fn mock_control(&self, status: &str, path: Option<&Path>, detail: Option<&str>) {
        self.emit(
            "network-mock-control",
            [
                ("status", Value::from(status)),
                (
                    "path",
                    opt_str(path.map(|p| p.display().to_string()).as_deref()),
                ),
                ("detail", opt_str(detail)),
            ],
        );
    }

    /// A published delta: `kind` names it and the changed section follows
    /// as a typed object. Lists of access points and profiles, and the
    /// restart snapshot, are reduced to ids and digests.
    pub fn network_event(&self, event: &NetworkEvent) {
        let typed = |v: &dyn erased::Ser| v.to_json();
        let mut fields = vec![("kind", Value::from(event.name()))];
        match event {
            NetworkEvent::AdapterChanged { adapter } => fields.push(("adapter", typed(adapter))),
            NetworkEvent::RadioChanged { radio } => fields.push(("radio", typed(radio))),
            NetworkEvent::ScanChanged { scan } => fields.push(("scan", typed(scan))),
            NetworkEvent::AccessPointsChanged { access_points } => {
                fields.push(("count", Value::from(access_points.len())));
                fields.push((
                    "access_points",
                    Value::Array(access_points.iter().map(|ap| ap.summary()).collect()),
                ));
            }
            NetworkEvent::ConnectionChanged { connection } => {
                fields.push(("connection", typed(connection)));
            }
            NetworkEvent::IpChanged { ip } => fields.push((
                "ip",
                ip.as_ref().map_or(Value::Null, |ip| {
                    serde_json::json!({
                        "addresses": ip.addresses.len(),
                        "gateway": ip.gateway.is_some(),
                        "dns": ip.dns.len(),
                        "dhcp": ip.dhcp,
                    })
                }),
            )),
            NetworkEvent::ConnectivityChanged { connectivity } => {
                fields.push(("connectivity", typed(connectivity)));
            }
            NetworkEvent::ProfilesChanged { profiles } => {
                fields.push(("count", Value::from(profiles.len())));
                fields.push((
                    "profiles",
                    Value::Array(profiles.iter().map(|p| p.summary()).collect()),
                ));
            }
            NetworkEvent::ServiceRestarted { state } => fields.push(("snapshot", state.summary())),
        }
        self.emit("network-event", fields);
    }

    /// The backend lost its underlying stack.
    pub fn backend_lost(&self, reason: &str) {
        self.emit("network-backend-lost", [("reason", Value::from(reason))]);
    }

    /// Which identity source authorizes callers.
    pub fn identity_source(&self, source: &str) {
        self.emit("networkd-identity", [("source", Value::from(source))]);
    }

    /// Nested development only: every caller is treated as fully granted.
    pub fn unauthenticated_allowed(&self) {
        self.emit(
            "networkd-unauthenticated",
            [
                ("status", Value::from("allowed")),
                (
                    "warning",
                    Value::from("all callers are granted every network capability"),
                ),
            ],
        );
    }

    /// A request refused by authorization (ADR-004).
    pub fn request_denied(&self, app: Option<&str>, capability: &str, request: &str) {
        self.emit(
            "networkd-request-denied",
            [
                ("app", opt_str(app)),
                ("capability", Value::from(capability)),
                ("request", Value::from(request)),
            ],
        );
    }

    /// A message refused by ordering or schema rules.
    pub fn request_rejected(&self, request: Option<&str>, reason: &str) {
        self.emit(
            "networkd-request-rejected",
            [
                ("request", opt_str(request)),
                ("reason", Value::from(reason)),
            ],
        );
    }
}

mod erased {
    //! Serialize any typed state into a `Value` without a generic bound on
    //! the caller.
    use serde_json::Value;

    pub trait Ser {
        fn to_json(&self) -> Value;
    }

    impl<T: serde::Serialize> Ser for T {
        fn to_json(&self) -> Value {
            serde_json::to_value(self).unwrap_or(Value::Null)
        }
    }
}

fn opt_str(value: Option<&str>) -> Value {
    value.map_or(Value::Null, Value::from)
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
    use ferrix_network_api::{AccessPoint, ApId, ConnectionState, SecurityType, Ssid};

    #[test]
    fn records_lead_with_evidence_then_event() {
        let value = record(
            EVIDENCE_MOCK,
            "networkd-ready",
            [("socket", Value::from("x"))],
        );
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"evidence":"mock","event":"networkd-ready","socket":"x"}"#
        );
    }

    #[test]
    fn network_event_records_carry_digests_only() {
        let (recorder, captured) = Recorder::capturing(EVIDENCE_QEMU);
        recorder.network_event(&NetworkEvent::AccessPointsChanged {
            access_points: vec![AccessPoint {
                id: ApId::new("ap-1"),
                ssid: Ssid::from("ferrix-lab"),
                bssid_hash: "0123456789abcdef".into(),
                signal_percent: 70,
                security: SecurityType::Wpa2Psk,
                frequency_mhz: None,
            }],
        });
        recorder.network_event(&NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Connected {
                ap: ApId::new("ap-1"),
            },
        });
        recorder.request_denied(Some("org.ferrix.settings"), "network-control", "scan");
        let records = captured.lock().unwrap();
        assert_eq!(records[0]["event"], "network-event");
        assert_eq!(records[0]["kind"], "access-points-changed");
        assert_eq!(records[0]["count"], 1);
        let text = records[0].to_string();
        assert!(!text.contains("ferrix-lab"), "{text}");
        assert!(text.contains("ssid_hash"));
        assert_eq!(records[1]["kind"], "connection-changed");
        assert_eq!(records[1]["connection"]["state"], "connected");
        assert_eq!(records[1]["connection"]["ap"], "ap-1");
        assert_eq!(records[2]["event"], "networkd-request-denied");
        assert_eq!(records[2]["capability"], "network-control");
        drop(records);
        assert_eq!(
            captured_events(&captured),
            ["network-event", "network-event", "networkd-request-denied"]
        );
    }
}
