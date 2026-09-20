//! The contacts service: request dispatch over the store, authorization per
//! request, event publication, and records. Transport-free so every rule is
//! tested on the host.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use ferrix_contacts_api::{
    CAP_READ, ChangeKind, Contact, ContactEvent, ContactId, ContactInput, ContactsError,
    SCHEMA_VERSION, lookup, search,
};
use ferrix_contacts_protocol::{Reply, Request};

use crate::auth::{Identified, Identity, required_capability};
use crate::records::Recorder;
use crate::store::{LoadReport, Store};
use crate::vcard;

/// Per-connection state owned by the transport.
#[derive(Debug, Default)]
pub struct Connection {
    /// Kernel-reported peer pid (`SO_PEERCRED`), `None` when unavailable.
    pub pid: Option<u32>,
    pub hello_done: bool,
    /// Last resolved identity, retained for diagnostics; authorization is
    /// resolved again for every request.
    pub identity: Option<Identified>,
}

impl Connection {
    pub fn with_pid(pid: Option<u32>) -> Self {
        Self {
            pid,
            ..Self::default()
        }
    }
}

/// What the transport does after a request.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Reply(Reply),
    /// Send the reply, then forward events from the receiver until the peer
    /// goes away.
    Subscribe(Reply),
}

pub struct Service {
    store: Mutex<Store>,
    identity: Box<dyn Identity>,
    recorder: Recorder,
    region: String,
    subscribers: Mutex<Vec<Subscriber>>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
}

struct Subscriber {
    pid: Option<u32>,
    sender: Sender<ContactEvent>,
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Service {
    /// Opens the store in `dir`, records what loading did, and loads
    /// `fixtures` (a v1 document) when the store is empty.
    pub fn open(
        dir: &Path,
        identity: Box<dyn Identity>,
        recorder: Recorder,
        region: &str,
        fixtures: Option<&Path>,
    ) -> Result<Arc<Self>, ContactsError> {
        let (store, report) = Store::open(dir)?;
        let service = Arc::new(Self {
            store: Mutex::new(store),
            identity,
            recorder,
            region: region.to_owned(),
            subscribers: Mutex::new(Vec::new()),
            clock: Box::new(wall_ms),
        });
        service.report_load(&report);
        if let Some(fixtures) = fixtures {
            service.load_fixtures(fixtures)?;
        }
        service.recorder.event(&ContactEvent::ServiceRestarted);
        Ok(service)
    }

    /// A service over a store in `dir` with a fixed clock; for tests.
    pub fn for_tests(
        dir: &Path,
        identity: Box<dyn Identity>,
        recorder: Recorder,
        clock: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Arc<Self>, ContactsError> {
        let (store, report) = Store::open(dir)?;
        let service = Arc::new(Self {
            store: Mutex::new(store),
            identity,
            recorder,
            region: "US".to_owned(),
            subscribers: Mutex::new(Vec::new()),
            clock: Box::new(clock),
        });
        service.report_load(&report);
        Ok(service)
    }

    fn report_load(&self, report: &LoadReport) {
        for (from, to) in &report.migrations {
            self.recorder.migrated(*from, *to);
        }
    }

    fn load_fixtures(&self, path: &Path) -> Result<(), ContactsError> {
        let mut store = self.store.lock().unwrap_or_else(|p| p.into_inner());
        if !store.contacts().is_empty() {
            return Ok(());
        }
        let text = std::fs::read_to_string(path).map_err(|e| ContactsError::Store {
            reason: format!("fixtures {}: {e}", path.display()),
        })?;
        let (document, _) = crate::store::parse_document(&text)?;
        let count = document.contacts.len();
        *store.contacts_mut() = document.contacts;
        store.save()?;
        self.recorder.emit(
            "contacts-fixtures-loaded",
            [
                ("path", serde_json::Value::from(path.display().to_string())),
                ("contacts", serde_json::Value::from(count)),
            ],
        );
        Ok(())
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    pub fn contact_count(&self) -> usize {
        self.store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contacts()
            .len()
    }

    pub fn snapshot(&self) -> Vec<Contact> {
        self.store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contacts()
            .to_vec()
    }

    fn publish(&self, event: ContactEvent) {
        self.recorder.event(&event);
        let mut subs = self.subscribers.lock().unwrap_or_else(|p| p.into_inner());
        subs.retain(|subscriber| {
            self.identity.identify(subscriber.pid).has(CAP_READ)
                && subscriber.sender.send(event.clone()).is_ok()
        });
    }

    fn subscribe_channel(&self, pid: Option<u32>) -> Receiver<ContactEvent> {
        let (tx, rx) = mpsc::channel();
        self.subscribers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Subscriber { pid, sender: tx });
        rx
    }

    /// Handles one request for `connection`. Hello must come first and
    /// only once; everything else is authorized against the verified peer.
    pub fn dispatch(
        &self,
        connection: &mut Connection,
        request: Request,
    ) -> (Outcome, Option<Receiver<ContactEvent>>) {
        if let Request::Hello {
            schema_version,
            client_kind: _,
        } = &request
        {
            if connection.hello_done {
                return (protocol_error("hello already sent"), None);
            }
            if *schema_version != SCHEMA_VERSION {
                return (
                    protocol_error(&format!("schema {schema_version} is not {SCHEMA_VERSION}")),
                    None,
                );
            }
            connection.hello_done = true;
            return (
                Outcome::Reply(Reply::Hello {
                    schema_version: SCHEMA_VERSION,
                    evidence: self.recorder.evidence().to_owned(),
                }),
                None,
            );
        }
        if !connection.hello_done {
            return (protocol_error("hello must come first"), None);
        }
        // Resolve every request. This also probes the persistent appd link,
        // so a restart cannot leave stale grants cached on an idle socket.
        let identity = self.identity.identify(connection.pid);
        connection.identity = Some(identity.clone());
        if let Some(capability) = required_capability(&request)
            && !identity.has(capability)
        {
            self.recorder
                .request_denied(identity.app.as_deref(), capability, request.name());
            return (
                Outcome::Reply(Reply::Error {
                    error: ContactsError::Unauthorized {
                        capability: capability.to_owned(),
                    },
                }),
                None,
            );
        }
        if matches!(request, Request::Subscribe {}) {
            let rx = self.subscribe_channel(connection.pid);
            return (Outcome::Subscribe(Reply::Ok), Some(rx));
        }
        let reply = match self.handle(request) {
            Ok(reply) => reply,
            Err(error) => {
                if matches!(error, ContactsError::Store { .. }) {
                    self.recorder.store_error(&error);
                }
                Reply::Error { error }
            }
        };
        (Outcome::Reply(reply), None)
    }

    fn handle(&self, request: Request) -> Result<Reply, ContactsError> {
        let mut store = self.store.lock().unwrap_or_else(|p| p.into_inner());
        let now = (self.clock)();
        match request {
            Request::Hello { .. } | Request::Subscribe {} => unreachable!("handled by dispatch"),
            Request::Get { id } => store
                .contacts()
                .iter()
                .find(|c| c.id == id)
                .cloned()
                .map(|contact| Reply::Contact { contact })
                .ok_or(ContactsError::NotFound { id }),
            Request::List {} => Ok(Reply::Contacts {
                contacts: store.contacts().to_vec(),
            }),
            Request::Search { query } => Ok(Reply::Contacts {
                contacts: search(store.contacts(), &query)
                    .into_iter()
                    .cloned()
                    .collect(),
            }),
            Request::Lookup { number } => {
                Ok(match lookup(store.contacts(), &number, &self.region) {
                    Some(contact) => Reply::Contact {
                        contact: contact.clone(),
                    },
                    None => Reply::Ok,
                })
            }
            Request::Create { contact_input } => {
                let contact = self.create_locked(&mut store, contact_input, now)?;
                store.save()?;
                drop(store);
                self.publish(ContactEvent::ContactChanged {
                    id: contact.id.clone(),
                    kind: ChangeKind::Created,
                });
                Ok(Reply::Contact { contact })
            }
            Request::Update { id, contact_input } => {
                let index = store
                    .contacts()
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or_else(|| ContactsError::NotFound { id: id.clone() })?;
                let existing = store.contacts()[index].clone();
                let contact =
                    contact_input.into_contact(id.clone(), now, &self.region, Some(&existing))?;
                store.contacts_mut()[index] = contact.clone();
                store.save()?;
                drop(store);
                self.publish(ContactEvent::ContactChanged {
                    id,
                    kind: ChangeKind::Updated,
                });
                Ok(Reply::Contact { contact })
            }
            Request::Delete { id } => {
                let before = store.contacts().len();
                store.contacts_mut().retain(|c| c.id != id);
                if store.contacts().len() == before {
                    return Err(ContactsError::NotFound { id });
                }
                store.save()?;
                drop(store);
                self.publish(ContactEvent::ContactChanged {
                    id,
                    kind: ChangeKind::Deleted,
                });
                Ok(Reply::Ok)
            }
            Request::ExportVcard {} => Ok(Reply::Vcard {
                text: vcard::export(store.contacts()),
            }),
            Request::ImportVcard { text } => {
                let inputs = vcard::import(&text)?;
                for input in &inputs {
                    input.validate()?;
                }
                let mut created = Vec::with_capacity(inputs.len());
                for input in inputs {
                    created.push(self.create_locked(&mut store, input, now)?);
                }
                store.save()?;
                drop(store);
                for contact in &created {
                    self.publish(ContactEvent::ContactChanged {
                        id: contact.id.clone(),
                        kind: ChangeKind::Created,
                    });
                }
                Ok(Reply::Contacts { contacts: created })
            }
        }
    }

    fn create_locked(
        &self,
        store: &mut Store,
        input: ContactInput,
        now: u64,
    ) -> Result<Contact, ContactsError> {
        let used: BTreeSet<&ContactId> = store.contacts().iter().map(|c| &c.id).collect();
        let mut id = ContactId::random()?;
        while used.contains(&id) {
            id = ContactId::random()?;
        }
        let contact = input.into_contact(id, now, &self.region, None)?;
        store.contacts_mut().push(contact.clone());
        Ok(contact)
    }
}

fn protocol_error(reason: &str) -> Outcome {
    Outcome::Reply(Reply::ProtocolError {
        reason: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StaticIdentity;
    use crate::records::Captured;
    use crate::store::temp_dir;
    use ferrix_contacts_api::{CAP_READ, CAP_WRITE, NumberInput, NumberLabel};
    use ferrix_contacts_protocol::ClientKind;
    use serde_json::Value;

    const READER: u32 = 100;
    const WRITER: u32 = 200;
    const UNKNOWN: u32 = 999;

    fn service(dir: &Path) -> (Arc<Service>, Captured) {
        let (recorder, captured) = Recorder::capturing("mock");
        let identity = StaticIdentity::default()
            .with(READER, "org.ferrix.phone", &[CAP_READ])
            .with(WRITER, "org.ferrix.contacts", &[CAP_READ, CAP_WRITE]);
        let service = Service::for_tests(dir, Box::new(identity), recorder, || 1_000).unwrap();
        (service, captured)
    }

    fn hello(service: &Service, pid: u32) -> Connection {
        let mut conn = Connection::with_pid(Some(pid));
        let (outcome, _) = service.dispatch(
            &mut conn,
            Request::Hello {
                schema_version: SCHEMA_VERSION,
                client_kind: ClientKind::Application,
            },
        );
        assert!(
            matches!(outcome, Outcome::Reply(Reply::Hello { .. })),
            "{outcome:?}"
        );
        conn
    }

    fn reply(service: &Service, conn: &mut Connection, request: Request) -> Reply {
        match service.dispatch(conn, request).0 {
            Outcome::Reply(reply) | Outcome::Subscribe(reply) => reply,
        }
    }

    fn input(name: &str, number: &str) -> ContactInput {
        ContactInput {
            display_name: name.to_owned(),
            given: None,
            family: None,
            numbers: vec![NumberInput {
                label: NumberLabel::Mobile,
                raw: number.to_owned(),
            }],
        }
    }

    fn events(captured: &Captured, name: &str) -> Vec<Value> {
        captured
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == name)
            .cloned()
            .collect()
    }

    #[test]
    fn hello_must_come_first_and_only_once_with_the_right_schema() {
        let dir = temp_dir("hello");
        let (service, _) = service(&dir);
        let mut conn = Connection::with_pid(Some(WRITER));
        assert!(matches!(
            reply(&service, &mut conn, Request::List {}),
            Reply::ProtocolError { .. }
        ));
        assert!(matches!(
            reply(
                &service,
                &mut conn,
                Request::Hello {
                    schema_version: 7,
                    client_kind: ClientKind::Tool
                }
            ),
            Reply::ProtocolError { .. }
        ));
        assert!(!conn.hello_done);
        let mut conn = hello(&service, WRITER);
        assert!(matches!(
            reply(
                &service,
                &mut conn,
                Request::Hello {
                    schema_version: SCHEMA_VERSION,
                    client_kind: ClientKind::Tool
                }
            ),
            Reply::ProtocolError { .. }
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn crud_search_lookup_and_events() {
        let dir = temp_dir("crud");
        let (service, captured) = service(&dir);
        let mut conn = hello(&service, WRITER);
        let rx = match service.dispatch(&mut hello(&service, READER), Request::Subscribe {}) {
            (Outcome::Subscribe(Reply::Ok), Some(rx)) => rx,
            other => panic!("{other:?}"),
        };
        let created = match reply(
            &service,
            &mut conn,
            Request::Create {
                contact_input: input("Test Contact 01", "(555) 010-0001"),
            },
        ) {
            Reply::Contact { contact } => contact,
            other => panic!("{other:?}"),
        };
        assert_eq!(created.numbers[0].e164.as_deref(), Some("+15550100001"));
        assert_eq!(created.created_ms, 1_000);
        assert_eq!(
            rx.try_recv().unwrap(),
            ContactEvent::ContactChanged {
                id: created.id.clone(),
                kind: ChangeKind::Created
            }
        );
        let record = &events(&captured, "contacts-event")[0];
        assert_eq!(record["contact_id"], created.id.as_str());
        assert_eq!(record["kind"], "created");
        assert!(record.get("number").is_none() && record.get("display_name").is_none());

        assert!(matches!(
            reply(&service, &mut conn, Request::Search { query: "contact".into() }),
            Reply::Contacts { contacts } if contacts.len() == 1
        ));
        assert!(matches!(
            reply(
                &service,
                &mut conn,
                Request::Lookup {
                    number: "+1 555 010 0001".into()
                }
            ),
            Reply::Contact { .. }
        ));
        assert_eq!(
            reply(
                &service,
                &mut conn,
                Request::Lookup {
                    number: "+1 555 010 0099".into()
                }
            ),
            Reply::Ok
        );
        let updated = reply(
            &service,
            &mut conn,
            Request::Update {
                id: created.id.clone(),
                contact_input: input("Renamed", "+15550100001"),
            },
        );
        assert!(matches!(updated, Reply::Contact { contact } if contact.display_name == "Renamed"));
        assert!(matches!(
            rx.try_recv().unwrap(),
            ContactEvent::ContactChanged {
                kind: ChangeKind::Updated,
                ..
            }
        ));
        assert_eq!(
            reply(
                &service,
                &mut conn,
                Request::Delete {
                    id: created.id.clone()
                }
            ),
            Reply::Ok
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            ContactEvent::ContactChanged {
                kind: ChangeKind::Deleted,
                ..
            }
        ));
        assert_eq!(
            reply(
                &service,
                &mut conn,
                Request::Delete {
                    id: created.id.clone()
                }
            ),
            Reply::Error {
                error: ContactsError::NotFound { id: created.id }
            }
        );
        assert!(matches!(
            reply(
                &service,
                &mut conn,
                Request::Create {
                    contact_input: ContactInput::default()
                }
            ),
            Reply::Error {
                error: ContactsError::InvalidInput { .. }
            }
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_grant_cannot_write_and_unknown_pid_is_denied() {
        let dir = temp_dir("authz");
        let (service, captured) = service(&dir);
        let mut reader = hello(&service, READER);
        assert!(matches!(
            reply(&service, &mut reader, Request::List {}),
            Reply::Contacts { .. }
        ));
        assert_eq!(
            reply(
                &service,
                &mut reader,
                Request::Create {
                    contact_input: input("x", "+15550100001")
                }
            ),
            Reply::Error {
                error: ContactsError::Unauthorized {
                    capability: CAP_WRITE.to_owned()
                }
            }
        );
        assert_eq!(service.contact_count(), 0, "denied request never mutates");
        let denied = events(&captured, "contactsd-request-denied");
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0]["app"], "org.ferrix.phone");
        assert_eq!(denied[0]["capability"], "contacts-write");
        assert_eq!(denied[0]["request"], "create");

        let mut unknown = hello(&service, UNKNOWN);
        assert!(matches!(
            reply(&service, &mut unknown, Request::List {}),
            Reply::Error {
                error: ContactsError::Unauthorized { .. }
            }
        ));
        let mut no_pid = hello(&service, 0);
        no_pid.pid = None;
        no_pid.identity = None;
        assert!(matches!(
            reply(&service, &mut no_pid, Request::Subscribe {}),
            Reply::Error {
                error: ContactsError::Unauthorized { .. }
            }
        ));
        assert_eq!(events(&captured, "contactsd-request-denied").len(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vcard_round_trip_and_restart_reconciliation() {
        let dir = temp_dir("vcard");
        let (service, _) = service(&dir);
        let mut conn = hello(&service, WRITER);
        for n in 1..=3 {
            reply(
                &service,
                &mut conn,
                Request::Create {
                    contact_input: input(
                        &format!("Test Contact {n:02}"),
                        &format!("+1555010000{n}"),
                    ),
                },
            );
        }
        let text = match reply(&service, &mut conn, Request::ExportVcard {}) {
            Reply::Vcard { text } => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(text.matches("BEGIN:VCARD").count(), 3);
        let imported = match reply(&service, &mut conn, Request::ImportVcard { text }) {
            Reply::Contacts { contacts } => contacts,
            other => panic!("{other:?}"),
        };
        assert_eq!(imported.len(), 3);
        assert_eq!(service.contact_count(), 6);
        assert!(matches!(
            reply(
                &service,
                &mut conn,
                Request::ImportVcard {
                    text: "BEGIN:VCARD\r\n".into()
                }
            ),
            Reply::Error {
                error: ContactsError::Vcard { .. }
            }
        ));
        assert_eq!(service.contact_count(), 6, "a bad import changes nothing");

        // Restart: a fresh service over the same directory sees the saved set.
        let saved = service.snapshot();
        drop(service);
        let (restarted, _) = super::tests::service(&dir);
        assert_eq!(restarted.snapshot(), saved);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fixtures_load_only_into_an_empty_store() {
        let dir = temp_dir("fixtures");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/mock-contacts.json");
        let (recorder, captured) = Recorder::capturing("mock");
        let service = Service::open(
            &dir,
            Box::new(StaticIdentity::default()),
            recorder,
            "US",
            Some(&fixtures),
        )
        .unwrap();
        assert_eq!(service.contact_count(), 8);
        assert!(
            service
                .snapshot()
                .iter()
                .all(|c| c.display_name.starts_with("Test Contact "))
        );
        assert!(service.snapshot().iter().all(|c| {
            c.numbers[0]
                .e164
                .as_deref()
                .unwrap()
                .starts_with("+1555010")
        }));
        assert_eq!(events(&captured, "contacts-fixtures-loaded").len(), 1);
        assert_eq!(
            events(&captured, "contacts-event")[0]["event_kind"],
            "service-restarted"
        );
        drop(service);
        let (recorder, captured) = Recorder::capturing("mock");
        let again = Service::open(
            &dir,
            Box::new(StaticIdentity::default()),
            recorder,
            "US",
            Some(&fixtures),
        )
        .unwrap();
        assert_eq!(again.contact_count(), 8);
        assert!(
            events(&captured, "contacts-fixtures-loaded").is_empty(),
            "not reloaded"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_store_refuses_open_and_migrations_are_recorded() {
        let dir = temp_dir("open-records");
        std::fs::write(Store::path_in(&dir), "nope").unwrap();
        let (recorder, _) = Recorder::capturing("mock");
        let error = Service::open(
            &dir,
            Box::new(StaticIdentity::default()),
            recorder,
            "US",
            None,
        )
        .err()
        .expect("corrupt data must fail closed");
        assert!(error.to_string().contains("preserved in place"));
        let v0 = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0-prerelease.json"),
        )
        .unwrap();
        std::fs::write(Store::path_in(&dir), v0).unwrap();
        let (recorder, captured) = Recorder::capturing("mock");
        let service = Service::open(
            &dir,
            Box::new(StaticIdentity::default()),
            recorder,
            "US",
            None,
        )
        .unwrap();
        let migrated = events(&captured, "contacts-migrated");
        assert_eq!(migrated.len(), 1);
        assert_eq!(
            (migrated[0]["from"].as_u64(), migrated[0]["to"].as_u64()),
            (Some(0), Some(1))
        );
        assert_eq!(service.contact_count(), 2);
        // The migrated document was written back at schema 1.
        let text = std::fs::read_to_string(Store::path_in(&dir)).unwrap();
        assert!(text.contains("\"schema_version\": 1"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
