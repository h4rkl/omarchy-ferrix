//! Machine-readable records: one JSON line per record on stdout, `evidence`
//! first, then `event`, mirroring `ferrix-appd`. Records carry ids, counts,
//! and state names only: never names, numbers, or vCard text.

use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use ferrix_contacts_api::{ContactEvent, ContactsError};
use serde_json::{Map, Value};

pub const EVIDENCE_QEMU: &str = "QEMU";
pub const EVIDENCE_MOCK: &str = "mock";

/// The process evidence label, resolved like the other Ferrix daemons: `mock`
/// when nested, else the bootstrap's `FERRIX_EVIDENCE` (the exact physical
/// target's label), else `QEMU`.
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

/// Prints one JSON record per line and flushes.
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

pub type Captured = Arc<Mutex<Vec<Value>>>;
type Sink = Arc<dyn Fn(&Value) + Send + Sync>;

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
                    .unwrap_or_else(|p| p.into_inner())
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

    pub fn ready(&self, socket: &Path, contacts: usize) {
        self.emit(
            "contactsd-ready",
            [
                ("socket", Value::from(socket.display().to_string())),
                ("contacts", Value::from(contacts)),
            ],
        );
    }

    pub fn failed(&self, reason: &str) {
        self.emit(
            "contactsd-failed",
            [
                ("status", Value::from("failed")),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn event(&self, event: &ContactEvent) {
        let contact_id = match event {
            ContactEvent::ContactChanged { id, .. } => Value::from(id.as_str()),
            ContactEvent::ServiceRestarted => Value::Null,
        };
        let kind = match event {
            ContactEvent::ContactChanged { kind, .. } => Value::from(kind.name()),
            ContactEvent::ServiceRestarted => Value::Null,
        };
        self.emit(
            "contacts-event",
            [
                ("event_kind", Value::from(event.name())),
                ("contact_id", contact_id),
                ("kind", kind),
            ],
        );
    }

    pub fn request_denied(&self, app: Option<&str>, capability: &str, request: &str) {
        self.emit(
            "contactsd-request-denied",
            [
                ("app", app.map_or(Value::Null, Value::from)),
                ("capability", Value::from(capability)),
                ("request", Value::from(request)),
            ],
        );
    }

    pub fn store_corrupt(&self, moved_to: &Path, reason: &str) {
        self.emit(
            "contacts-store-corrupt",
            [
                ("moved_to", Value::from(moved_to.display().to_string())),
                ("reason", Value::from(reason)),
            ],
        );
    }

    pub fn migrated(&self, from: u32, to: u32) {
        self.emit(
            "contacts-migrated",
            [("from", Value::from(from)), ("to", Value::from(to))],
        );
    }

    pub fn store_error(&self, error: &ContactsError) {
        self.emit(
            "contacts-store-error",
            [("error", Value::from(error.to_string()))],
        );
    }
}
