//! Host tests (`mock` evidence) for the telephony service: dispatch as a pure
//! function, hello ordering and schema, every unauthorized case (ADR-004),
//! restart reconciliation, multipart assembly, and the socket server.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ferrix_telephony_adapter_api::{
    BackendCall, BackendError, BackendEvent, CallHandle, SmsHandle,
};
use ferrix_telephony_api::{
    AudioRoute, CallEndReason, CallId, CallState, DataProfile, Direction, Dtmf, E164, MessageId,
    ModemState, MultipartPart, RadioPower, SCHEMA_VERSION, SmsFailure, SmsState, TelephonyError,
    TelephonyEvent, TelephonyFeature,
};
use ferrix_telephony_protocol::{
    Client, ClientKind, Event, Reply, Request, SmsFilter, StateView, write_line,
};
use ferrix_telephonyd::records::captured_events;
use ferrix_telephonyd::server::parse_line;
use ferrix_telephonyd::testing::{FakeBackend, MemorySink, temp_data_dir};
use ferrix_telephonyd::{
    AllowAll, CAP_CALL, CAP_DATA, CAP_RADIO, CAP_SMS, CAP_STATE, Connection, DenyAll, Identity,
    IdentityInfo, Outcome, Recorder, Server, Service, ServiceOptions, SmsStore, StaticIdentity,
    dispatch,
};

const NUMBER: &str = "+15550100042";
const PHONE_PID: u32 = 100;
const MESSAGES_PID: u32 = 200;
const STATUS_PID: u32 = 300;
const DECLARED_NOT_GRANTED_PID: u32 = 400;
const DATA_PID: u32 = 500;

fn number() -> E164 {
    E164::new(NUMBER).unwrap()
}

struct Harness {
    service: Arc<Service>,
    backend: FakeBackend,
    identity: Arc<StaticIdentity>,
    records: ferrix_telephonyd::records::Captured,
    dir: PathBuf,
}

fn harness(tag: &str) -> Harness {
    harness_with(tag, FakeBackend::ready(), ServiceOptions::default())
}

fn harness_with(tag: &str, backend: FakeBackend, options: ServiceOptions) -> Harness {
    let dir = temp_data_dir(tag);
    let (recorder, records) = Recorder::capturing("mock");
    let service = Service::new(
        Arc::new(backend.clone()),
        SmsStore::new(&dir),
        recorder,
        options,
    )
    .unwrap();
    let identity = Arc::new(
        StaticIdentity::new()
            .with(
                PHONE_PID,
                IdentityInfo::granted(Some("org.ferrix.phone"), &[CAP_STATE, CAP_CALL]),
            )
            .with(
                MESSAGES_PID,
                IdentityInfo::granted(Some("org.ferrix.messages"), &[CAP_STATE, CAP_SMS]),
            )
            .with(
                STATUS_PID,
                IdentityInfo::granted(Some("org.ferrix.home"), &[CAP_STATE]),
            )
            .with(
                DATA_PID,
                IdentityInfo::granted(
                    Some("org.ferrix.settings"),
                    &[CAP_STATE, CAP_DATA, CAP_RADIO],
                ),
            )
            // Declared telephony-call in its descriptor, but the grants file
            // does not grant it: the effective set is empty.
            .with(
                DECLARED_NOT_GRANTED_PID,
                IdentityInfo::granted(Some("org.ferrix.terminal"), &[]),
            ),
    );
    Harness {
        service,
        backend,
        identity,
        records,
        dir,
    }
}

#[test]
fn cellular_data_has_a_separate_capability_and_backend_operation() {
    let backend = FakeBackend::ready();
    backend.set_capabilities([TelephonyFeature::Data].into_iter().collect());
    let h = harness_with("data-control", backend, ServiceOptions::default());
    let mut conn = h.conn(DATA_PID);

    assert_eq!(
        h.send(
            &mut conn,
            Request::ConnectData {
                profile: DataProfile {
                    apn: Some("internet".into()),
                },
            },
        ),
        Reply::Ok
    );
    assert_eq!(h.backend.ops(), ["connect-data internet"]);
    assert_eq!(h.send(&mut conn, Request::DisconnectData {}), Reply::Ok);
    assert_eq!(
        h.backend.ops(),
        ["connect-data internet", "disconnect-data"]
    );

    let state_only = h.conn(STATUS_PID);
    let mut state_only = state_only;
    h.denied(
        &mut state_only,
        Request::ConnectData {
            profile: DataProfile::default(),
        },
        CAP_DATA,
    );
}

#[test]
fn settings_radio_grant_does_not_grant_voice_calls() {
    let h = harness("radio-control");
    let mut settings = h.conn(DATA_PID);
    assert_eq!(
        h.send(
            &mut settings,
            Request::SetRadioPower {
                power: RadioPower::Off,
            },
        ),
        Reply::Ok
    );
    assert_eq!(h.backend.ops(), ["set-radio-power off"]);
    h.denied(
        &mut settings,
        Request::Dial {
            number: NUMBER.into(),
        },
        CAP_CALL,
    );
}

impl Harness {
    fn conn(&self, pid: u32) -> Connection {
        let mut conn = Connection::with_peer(Some(pid));
        let hello = dispatch(
            &self.service,
            self.identity.as_ref(),
            &mut conn,
            Request::Hello {
                schema_version: SCHEMA_VERSION,
                client_kind: ClientKind::Application,
            },
        );
        assert!(
            matches!(hello, Outcome::Reply(ref r) if matches!(**r, Reply::Hello { .. })),
            "{hello:?}"
        );
        conn
    }

    fn send(&self, conn: &mut Connection, request: Request) -> Reply {
        match dispatch(&self.service, self.identity.as_ref(), conn, request) {
            Outcome::Reply(reply) => *reply,
            Outcome::Subscribe(_) => Reply::Ok,
        }
    }

    fn denied(&self, conn: &mut Connection, request: Request, capability: &str) {
        let name = request.name();
        let reply = self.send(conn, request);
        assert_eq!(
            reply,
            Reply::Error {
                error: TelephonyError::Unauthorized {
                    capability: capability.to_owned()
                }
            },
            "{name}"
        );
    }

    fn denials(&self) -> Vec<(String, String, String)> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "telephonyd-request-denied")
            .map(|r| {
                (
                    r["app"].as_str().unwrap_or("").to_owned(),
                    r["capability"].as_str().unwrap().to_owned(),
                    r["request"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }
}

fn hello(schema_version: u32) -> Request {
    Request::Hello {
        schema_version,
        client_kind: ClientKind::Application,
    }
}

// ---------------------------------------------------------------------------
// Hello ordering, schema, malformed lines
// ---------------------------------------------------------------------------

#[test]
fn hello_must_come_first_carry_the_schema_and_happen_once() {
    let h = harness("hello");
    let mut conn = Connection::with_peer(Some(PHONE_PID));
    assert_eq!(
        h.send(&mut conn, Request::State {}),
        Reply::ProtocolError {
            reason: "hello must come first".into()
        }
    );
    assert!(matches!(
        h.send(&mut conn, hello(SCHEMA_VERSION + 1)),
        Reply::ProtocolError { reason } if reason.contains("schema 2 is not 1")
    ));
    assert!(!conn.hello_done);
    match h.send(&mut conn, hello(SCHEMA_VERSION)) {
        Reply::Hello {
            schema_version,
            evidence,
            state: StateView::Full(state),
        } => {
            assert_eq!(schema_version, SCHEMA_VERSION);
            assert_eq!(evidence, "mock");
            assert_eq!(state.modem, ModemState::Ready);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        h.send(&mut conn, hello(SCHEMA_VERSION)),
        Reply::ProtocolError { reason } if reason.contains("already")
    ));
    let events = captured_events(&h.records);
    assert!(events.contains(&"telephonyd-request-rejected".to_owned()));
    assert_eq!(
        events[0], "telephony-event",
        "service-restarted is published at start"
    );
}

#[test]
fn malformed_and_unknown_field_lines_never_reach_the_service() {
    assert!(parse_line("{").is_err());
    assert!(parse_line(r#"{"request":"reboot"}"#).is_err());
    assert!(parse_line(r#"{"request":"state","extra":1}"#).is_err());
    // A forged application id in the payload is an unknown field: rejected
    // before any capability logic runs.
    assert!(
        parse_line(r#"{"request":"dial","number":"+15550100042","app":"org.ferrix.phone"}"#)
            .is_err()
    );
    assert!(parse_line(r#"{"request":"dial","number":"+15550100042"}"#).is_ok());
    let h = harness("malformed");
    assert_eq!(h.backend.ops().len(), 0);
}

// ---------------------------------------------------------------------------
// Authorization (ADR-004)
// ---------------------------------------------------------------------------

#[test]
fn unknown_pid_is_denied_at_hello() {
    let h = harness("unknown-pid");
    let mut conn = Connection::with_peer(Some(999));
    assert_eq!(
        h.send(&mut conn, hello(SCHEMA_VERSION)),
        Reply::Error {
            error: TelephonyError::Unauthorized {
                capability: CAP_STATE.into()
            }
        }
    );
    assert!(!conn.hello_done);
    let mut no_peer = Connection::with_peer(None);
    assert!(matches!(
        h.send(&mut no_peer, hello(SCHEMA_VERSION)),
        Reply::Error { .. }
    ));
    assert_eq!(
        h.denials()[0],
        ("".into(), CAP_STATE.into(), "hello".into())
    );
}

#[test]
fn declared_but_not_granted_is_denied() {
    let h = harness("not-granted");
    let mut conn = Connection::with_peer(Some(DECLARED_NOT_GRANTED_PID));
    assert!(matches!(
        h.send(&mut conn, hello(SCHEMA_VERSION)),
        Reply::Error {
            error: TelephonyError::Unauthorized { .. }
        }
    ));
    assert_eq!(
        h.denials()[0],
        (
            "org.ferrix.terminal".into(),
            CAP_STATE.into(),
            "hello".into()
        )
    );
}

#[test]
fn state_grant_cannot_reach_any_control_verb() {
    let h = harness("state-only");
    let mut conn = h.conn(STATUS_PID);
    match h.send(&mut conn, Request::State {}) {
        Reply::State {
            state: StateView::Redacted(_),
        } => {}
        other => panic!("{other:?}"),
    }
    let c1 = CallId::from("c1");
    for (request, capability) in [
        (
            Request::Dial {
                number: NUMBER.into(),
            },
            CAP_CALL,
        ),
        (
            Request::Answer {
                call_id: c1.clone(),
            },
            CAP_CALL,
        ),
        (
            Request::Reject {
                call_id: c1.clone(),
            },
            CAP_CALL,
        ),
        (
            Request::Hangup {
                call_id: c1.clone(),
            },
            CAP_CALL,
        ),
        (
            Request::Hold {
                call_id: c1.clone(),
            },
            CAP_CALL,
        ),
        (
            Request::Resume {
                call_id: c1.clone(),
            },
            CAP_CALL,
        ),
        (
            Request::Dtmf {
                call_id: c1.clone(),
                digit: Dtmf::new('1').unwrap(),
            },
            CAP_CALL,
        ),
        (
            Request::SetAudioRoute {
                route: AudioRoute::Speaker,
            },
            CAP_CALL,
        ),
        (
            Request::SetRadioPower {
                power: RadioPower::Off,
            },
            CAP_RADIO,
        ),
        (Request::EnterPin { pin: "0000".into() }, CAP_CALL),
        (
            Request::SendSms {
                to: NUMBER.into(),
                body: "x".into(),
            },
            CAP_SMS,
        ),
        (
            Request::ListSms {
                filter: SmsFilter::default(),
            },
            CAP_SMS,
        ),
        (
            Request::MarkRead {
                id: MessageId::from("m1"),
            },
            CAP_SMS,
        ),
        (
            Request::DeleteSms {
                id: MessageId::from("m1"),
            },
            CAP_SMS,
        ),
        (
            Request::ConnectData {
                profile: DataProfile::default(),
            },
            CAP_DATA,
        ),
        (Request::DisconnectData {}, CAP_DATA),
    ] {
        h.denied(&mut conn, request, capability);
    }
    assert_eq!(
        h.backend.ops().len(),
        0,
        "no denied request reached the backend"
    );
    let denials = h.denials();
    assert_eq!(denials.len(), 16);
    assert!(denials.iter().all(|(app, _, _)| app == "org.ferrix.home"));
    assert!(
        denials
            .iter()
            .any(|(_, cap, req)| cap == CAP_CALL && req == "dial")
    );
    assert!(
        denials
            .iter()
            .any(|(_, cap, req)| cap == CAP_SMS && req == "send-sms")
    );
}

#[test]
fn call_and_sms_grants_are_distinct() {
    let h = harness("distinct");
    let mut phone = h.conn(PHONE_PID);
    h.denied(
        &mut phone,
        Request::ListSms {
            filter: SmsFilter::default(),
        },
        CAP_SMS,
    );
    assert!(matches!(
        h.send(
            &mut phone,
            Request::Dial {
                number: NUMBER.into()
            }
        ),
        Reply::Call { .. }
    ));
    let mut messages = h.conn(MESSAGES_PID);
    h.denied(
        &mut messages,
        Request::Dial {
            number: NUMBER.into(),
        },
        CAP_CALL,
    );
    assert!(matches!(
        h.send(
            &mut messages,
            Request::ListSms {
                filter: SmsFilter::default()
            }
        ),
        Reply::SmsList { .. }
    ));
}

#[test]
fn identity_is_re_resolved_on_every_request_and_after_appd_restart() {
    let h = harness("regen");
    let mut conn = h.conn(STATUS_PID);
    h.denied(
        &mut conn,
        Request::Dial {
            number: NUMBER.into(),
        },
        CAP_CALL,
    );
    // Grant changes apply immediately to an existing connection.
    h.identity.set(
        STATUS_PID,
        IdentityInfo::granted(Some("org.ferrix.home"), &[CAP_STATE, CAP_CALL]),
    );
    assert!(matches!(
        h.send(
            &mut conn,
            Request::Dial {
                number: NUMBER.into()
            }
        ),
        Reply::Call { .. }
    ));
    // Revocation is immediate too; a later authority restart remains safe.
    h.identity.remove(STATUS_PID);
    h.denied(&mut conn, Request::State {}, CAP_STATE);
    h.identity.bump_generation();
    h.denied(&mut conn, Request::State {}, CAP_STATE);
}

#[test]
fn deny_all_and_allow_all_sources() {
    let h = harness("sources");
    let mut conn = Connection::with_peer(None);
    assert!(matches!(
        dispatch(&h.service, &DenyAll, &mut conn, hello(SCHEMA_VERSION)),
        Outcome::Reply(r) if matches!(*r, Reply::Error { .. })
    ));
    let mut conn = Connection::with_peer(None);
    assert!(matches!(
        dispatch(&h.service, &AllowAll, &mut conn, hello(SCHEMA_VERSION)),
        Outcome::Reply(r) if matches!(*r, Reply::Hello { state: StateView::Full(_), .. })
    ));
    assert_eq!(DenyAll.name(), "deny-all");
}

// ---------------------------------------------------------------------------
// Redaction per capability
// ---------------------------------------------------------------------------

#[test]
fn state_clients_see_redacted_parties_and_no_bodies() {
    let h = harness("redact");
    h.backend.emit(BackendEvent::Tick { now_ms: 0 });
    h.service.handle_backend_event(BackendEvent::CallAdded {
        handle: CallHandle::from("h1"),
        direction: Direction::Incoming,
        number: number(),
        state: CallState::Ringing,
    });
    h.service.handle_backend_event(BackendEvent::SmsReceived {
        from: number(),
        body: "secret".into(),
    });
    let mut status = h.conn(STATUS_PID);
    let reply = serde_json::to_string(&h.send(&mut status, Request::State {})).unwrap();
    assert!(!reply.contains("0100042"), "{reply}");
    assert!(!reply.contains("secret"), "{reply}");
    assert!(reply.contains(r#""party_redacted":"+***0042""#), "{reply}");

    let mut phone = h.conn(PHONE_PID);
    let reply = serde_json::to_string(&h.send(&mut phone, Request::State {})).unwrap();
    assert!(reply.contains(NUMBER), "{reply}");
    assert!(
        !reply.contains("secret"),
        "call-only peers get no messages: {reply}"
    );

    let mut messages = h.conn(MESSAGES_PID);
    let reply = serde_json::to_string(&h.send(&mut messages, Request::State {})).unwrap();
    assert!(reply.contains("secret"), "{reply}");
    assert!(
        !reply.contains(r#""calls":{"c1""#),
        "sms-only peers get no calls: {reply}"
    );

    // Subscribers are projected the same way.
    let status_sink = MemorySink::new();
    let phone_sink = MemorySink::new();
    h.service
        .subscribe(
            h.identity.identify(Some(STATUS_PID)),
            Box::new(status_sink.clone()),
        )
        .unwrap();
    h.service
        .subscribe(
            h.identity.identify(Some(PHONE_PID)),
            Box::new(phone_sink.clone()),
        )
        .unwrap();
    h.service.handle_backend_event(BackendEvent::CallChanged {
        handle: CallHandle::from("h1"),
        state: CallState::Active,
        end_reason: None,
    });
    assert_eq!(status_sink.values()[0]["reply"], "ok");
    let status_event = &status_sink.values()[1];
    assert_eq!(status_event["event"], "call-changed");
    assert_eq!(status_event["call"]["party"]["party_redacted"], "+***0042");
    assert!(status_event["call"]["party"].get("number").is_none());
    let phone_event = &phone_sink.values()[1];
    assert_eq!(phone_event["call"]["party"]["number"], NUMBER);
    // A state client can fold the pushed event into its snapshot.
    let event: Event = serde_json::from_value(status_event.clone()).unwrap();
    assert!(matches!(event, Event::Redacted(_)));

    // Records never carry numbers or bodies.
    let text = serde_json::to_string(&*h.records.lock().unwrap()).unwrap();
    assert!(!text.contains("0100042"), "{text}");
    assert!(!text.contains("secret"), "{text}");
}

// ---------------------------------------------------------------------------
// Calls
// ---------------------------------------------------------------------------

#[test]
fn outgoing_call_lifecycle_and_typed_errors() {
    let h = harness("outgoing");
    let mut phone = h.conn(PHONE_PID);
    let call = match h.send(
        &mut phone,
        Request::Dial {
            number: NUMBER.into(),
        },
    ) {
        Reply::Call { call } => call,
        other => panic!("{other:?}"),
    };
    assert_eq!(call.id, CallId::from("c1"));
    assert_eq!(call.state, CallState::Dialing);
    assert_eq!(call.direction, Direction::Outgoing);
    assert_eq!(h.backend.ops(), ["dial +***0042"]);

    // The backend reports the same call: no duplicate id.
    h.service.handle_backend_event(BackendEvent::CallAdded {
        handle: CallHandle::from("h1"),
        direction: Direction::Outgoing,
        number: number(),
        state: CallState::Dialing,
    });
    assert_eq!(h.service.state().calls.len(), 1);
    h.service.handle_backend_event(BackendEvent::CallChanged {
        handle: CallHandle::from("h1"),
        state: CallState::Active,
        end_reason: None,
    });
    assert!(matches!(
        h.send(&mut phone, Request::Hold { call_id: call.id.clone() }),
        Reply::Call { call } if call.state == CallState::Held
    ));
    assert!(matches!(
        h.send(&mut phone, Request::Dtmf { call_id: call.id.clone(), digit: Dtmf::new('1').unwrap() }),
        Reply::Error { error: TelephonyError::InvalidState { from, op } } if from == "held" && op == "dtmf"
    ));
    assert!(matches!(
        h.send(&mut phone, Request::Resume { call_id: call.id.clone() }),
        Reply::Call { call } if call.state == CallState::Active
    ));
    assert_eq!(
        h.send(
            &mut phone,
            Request::Dtmf {
                call_id: call.id.clone(),
                digit: Dtmf::new('#').unwrap()
            }
        ),
        Reply::Ok
    );
    assert!(matches!(
        h.send(&mut phone, Request::Hangup { call_id: call.id.clone() }),
        Reply::Call { call } if call.state == CallState::Ended && call.end_reason == Some(CallEndReason::LocalHangup)
    ));
    assert!(matches!(
        h.send(
            &mut phone,
            Request::Answer {
                call_id: call.id.clone()
            }
        ),
        Reply::Error {
            error: TelephonyError::InvalidState { .. }
        }
    ));
    assert_eq!(
        h.send(
            &mut phone,
            Request::Hangup {
                call_id: CallId::from("c9")
            }
        ),
        Reply::Error {
            error: TelephonyError::NotFound { id: "c9".into() }
        }
    );
    assert_eq!(
        h.backend.ops(),
        [
            "dial +***0042",
            "hold h1",
            "resume h1",
            "dtmf h1 #",
            "hangup h1"
        ]
    );
}

#[test]
fn emergency_and_invalid_numbers_are_rejected_before_the_backend() {
    let h = harness("emergency");
    let mut phone = h.conn(PHONE_PID);
    for n in ["112", "911", "+911", "000"] {
        assert_eq!(
            h.send(
                &mut phone,
                Request::Dial {
                    number: n.to_owned()
                }
            ),
            Reply::Error {
                error: TelephonyError::Unsupported {
                    feature: TelephonyFeature::Emergency
                }
            },
            "{n}"
        );
    }
    assert!(matches!(
        h.send(
            &mut phone,
            Request::Dial {
                number: "5550100042".into()
            }
        ),
        Reply::Error {
            error: TelephonyError::InvalidNumber { .. }
        }
    ));
    assert!(h.backend.ops().is_empty());
}

#[test]
fn backend_errors_map_to_typed_replies() {
    let h = harness("backend-errors");
    let mut phone = h.conn(PHONE_PID);
    h.backend.fail_next(BackendError::NoService);
    assert_eq!(
        h.send(
            &mut phone,
            Request::Dial {
                number: NUMBER.into()
            }
        ),
        Reply::Error {
            error: TelephonyError::NoService
        }
    );
    assert!(h.service.state().calls.is_empty());
    h.backend
        .fail_next(BackendError::Unsupported(TelephonyFeature::AudioRoute));
    assert_eq!(
        h.send(
            &mut phone,
            Request::SetAudioRoute {
                route: AudioRoute::Speaker
            }
        ),
        Reply::Error {
            error: TelephonyError::Unsupported {
                feature: TelephonyFeature::AudioRoute
            }
        }
    );
    // A feature the backend does not advertise is refused by the service.
    h.backend
        .set_capabilities([TelephonyFeature::Voice].into_iter().collect());
    h.service.handle_backend_event(BackendEvent::Capabilities(
        [TelephonyFeature::Voice].into_iter().collect(),
    ));
    let mut settings = h.conn(DATA_PID);
    assert_eq!(
        h.send(
            &mut settings,
            Request::SetRadioPower {
                power: RadioPower::Off
            }
        ),
        Reply::Error {
            error: TelephonyError::Unsupported {
                feature: TelephonyFeature::RadioPower
            }
        }
    );
    assert!(matches!(
        h.send(&mut phone, Request::EnterPin { pin: "abc".into() }),
        Reply::Error {
            error: TelephonyError::Unsupported { .. }
        }
    ));
}

#[test]
fn incoming_call_answer_and_backend_loss() {
    let h = harness("incoming");
    h.service
        .handle_backend_event(BackendEvent::Tick { now_ms: 500 });
    h.service.handle_backend_event(BackendEvent::CallAdded {
        handle: CallHandle::from("h1"),
        direction: Direction::Incoming,
        number: number(),
        state: CallState::Ringing,
    });
    let call = h.service.state().calls[&CallId::from("c1")].clone();
    assert_eq!(call.started_at_ms, 500);
    let mut phone = h.conn(PHONE_PID);
    assert!(matches!(
        h.send(&mut phone, Request::Answer { call_id: call.id.clone() }),
        Reply::Call { call } if call.state == CallState::Active
    ));
    h.service.handle_backend_event(BackendEvent::BackendLost {
        reason: "crash".into(),
    });
    let state = h.service.state();
    assert_eq!(state.modem, ModemState::Recovering);
    let call = &state.calls[&CallId::from("c1")];
    assert_eq!(call.state, CallState::Ended);
    assert_eq!(call.end_reason, Some(CallEndReason::ModemLost));
    let events = captured_events(&h.records);
    assert!(events.contains(&"telephony-backend-lost".to_owned()));

    // Backend back: snapshot re-read, service-restarted published.
    let sink = MemorySink::new();
    h.service
        .subscribe(h.identity.identify(Some(PHONE_PID)), Box::new(sink.clone()))
        .unwrap();
    h.service
        .handle_backend_event(BackendEvent::BackendRestarted);
    assert_eq!(h.service.state().modem, ModemState::Ready);
    let values = sink.values();
    assert_eq!(values[1]["event"], "service-restarted");
    assert_eq!(values[1]["state"]["modem"]["state"], "ready");
}

#[test]
fn subscriber_is_dropped_when_state_grant_is_revoked() {
    let h = harness("subscription-revocation");
    let sink = MemorySink::new();
    h.service
        .subscribe_authorized(
            Some(PHONE_PID),
            h.identity.clone() as Arc<dyn Identity>,
            h.identity.identify(Some(PHONE_PID)),
            Box::new(sink.clone()),
        )
        .unwrap();
    assert_eq!(sink.values().len(), 1, "initial ok reply");
    h.identity.remove(PHONE_PID);
    h.service.handle_backend_event(BackendEvent::BackendLost {
        reason: "revocation probe".into(),
    });
    assert_eq!(sink.values().len(), 1, "revoked peer receives no event");
}

// ---------------------------------------------------------------------------
// SMS
// ---------------------------------------------------------------------------

#[test]
fn outgoing_sms_lifecycle_list_mark_read_delete() {
    let h = harness("sms-out");
    let mut messages = h.conn(MESSAGES_PID);
    let sms = match h.send(
        &mut messages,
        Request::SendSms {
            to: NUMBER.into(),
            body: "hello".into(),
        },
    ) {
        Reply::Sms { sms } => sms,
        other => panic!("{other:?}"),
    };
    assert_eq!(sms.id, MessageId::from("m1"));
    assert_eq!(sms.state, SmsState::Sending);
    h.service.handle_backend_event(BackendEvent::SmsChanged {
        handle: SmsHandle::from("s1"),
        state: SmsState::Sent,
        failure: None,
    });
    h.service.handle_backend_event(BackendEvent::SmsChanged {
        handle: SmsHandle::from("s1"),
        state: SmsState::Delivered,
        failure: None,
    });
    assert_eq!(h.service.state().sms[&sms.id].state, SmsState::Delivered);

    h.service
        .handle_backend_event(BackendEvent::Tick { now_ms: 10 });
    h.service.handle_backend_event(BackendEvent::SmsReceived {
        from: number(),
        body: "reply".into(),
    });
    let listed = match h.send(
        &mut messages,
        Request::ListSms {
            filter: SmsFilter::default(),
        },
    ) {
        Reply::SmsList { messages, total } => {
            assert_eq!(total, 2);
            messages
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(listed[0].id, MessageId::from("m2"), "newest first");
    let unread = match h.send(
        &mut messages,
        Request::ListSms {
            filter: SmsFilter {
                party: Some(number()),
                unread_only: true,
                limit: 1,
            },
        },
    ) {
        Reply::SmsList { messages, total } => {
            assert_eq!(total, 1);
            messages
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(unread[0].state, SmsState::Received);
    assert!(matches!(
        h.send(&mut messages, Request::MarkRead { id: MessageId::from("m2") }),
        Reply::Sms { sms } if sms.state == SmsState::Read
    ));
    assert!(matches!(
        h.send(
            &mut messages,
            Request::MarkRead {
                id: MessageId::from("m1")
            }
        ),
        Reply::Error {
            error: TelephonyError::InvalidState { .. }
        }
    ));
    assert!(matches!(
        h.send(&mut messages, Request::DeleteSms { id: MessageId::from("m1") }),
        Reply::Sms { sms } if sms.state == SmsState::Deleted
    ));
    assert!(!h.service.state().sms.contains_key(&MessageId::from("m1")));
    assert_eq!(
        h.send(
            &mut messages,
            Request::DeleteSms {
                id: MessageId::from("m1")
            }
        ),
        Reply::Error {
            error: TelephonyError::NotFound { id: "m1".into() }
        }
    );
    assert!(matches!(
        h.send(
            &mut messages,
            Request::SendSms {
                to: NUMBER.into(),
                body: String::new()
            }
        ),
        Reply::Error {
            error: TelephonyError::Protocol { .. }
        }
    ));
    assert!(matches!(
        h.send(
            &mut messages,
            Request::SendSms {
                to: "911".into(),
                body: "x".into()
            }
        ),
        Reply::Error {
            error: TelephonyError::InvalidNumber { .. }
        }
    ));
}

#[test]
fn outgoing_sms_failure_carries_typed_reason() {
    let h = harness("sms-fail");
    let mut messages = h.conn(MESSAGES_PID);
    h.send(
        &mut messages,
        Request::SendSms {
            to: NUMBER.into(),
            body: "x".into(),
        },
    );
    h.service.handle_backend_event(BackendEvent::SmsChanged {
        handle: SmsHandle::from("s1"),
        state: SmsState::Failed,
        failure: Some(SmsFailure::NoService),
    });
    let sms = &h.service.state().sms[&MessageId::from("m1")];
    assert_eq!(sms.state, SmsState::Failed);
    assert_eq!(sms.failure, Some(SmsFailure::NoService));
    // A late "delivered" for a failed message is ignored.
    h.service.handle_backend_event(BackendEvent::SmsChanged {
        handle: SmsHandle::from("s1"),
        state: SmsState::Delivered,
        failure: None,
    });
    assert_eq!(
        h.service.state().sms[&MessageId::from("m1")].state,
        SmsState::Failed
    );
}

fn part(ref_id: u16, index: u8, count: u8, text: &str) -> BackendEvent {
    BackendEvent::SmsPartReceived {
        from: number(),
        part: MultipartPart {
            ref_id,
            index,
            count,
            text: text.to_owned(),
        },
    }
}

#[test]
fn multipart_assembles_out_of_order_and_publishes_once() {
    let h = harness("multipart");
    let sink = MemorySink::new();
    h.service
        .subscribe(
            h.identity.identify(Some(MESSAGES_PID)),
            Box::new(sink.clone()),
        )
        .unwrap();
    h.service.handle_backend_event(part(7, 3, 3, "C"));
    h.service.handle_backend_event(part(7, 1, 3, "A"));
    assert!(h.service.state().sms.is_empty());
    // A different concatenation interleaves without mixing.
    h.service.handle_backend_event(part(8, 1, 2, "x"));
    h.service.handle_backend_event(part(7, 2, 3, "B"));
    let state = h.service.state();
    assert_eq!(state.sms.len(), 1);
    let sms = &state.sms[&MessageId::from("m1")];
    assert_eq!(sms.body, "ABC");
    assert_eq!(sms.state, SmsState::Received);
    assert_eq!(sms.parts.len(), 3);
    assert_eq!(sms.parts[0].index, 1);
    assert_eq!(sms.parts[2].ref_id, 7);
    let events: Vec<_> = sink
        .values()
        .into_iter()
        .filter(|v| v["event"] == "sms-changed")
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["sms"]["body"], "ABC");
    // Duplicate segment is harmless.
    h.service.handle_backend_event(part(8, 1, 2, "x"));
    h.service.handle_backend_event(part(8, 2, 2, "y"));
    assert_eq!(h.service.state().sms[&MessageId::from("m2")].body, "xy");
    // Out-of-range segments are dropped.
    h.service.handle_backend_event(part(9, 0, 2, "?"));
    h.service.handle_backend_event(part(9, 3, 2, "?"));
    assert_eq!(h.service.state().sms.len(), 2);
}

#[test]
fn incomplete_multipart_times_out_to_a_typed_failure() {
    let h = harness_with(
        "multipart-timeout",
        FakeBackend::ready(),
        ServiceOptions {
            multipart_window_ms: 1_000,
        },
    );
    h.service
        .handle_backend_event(BackendEvent::Tick { now_ms: 100 });
    h.service.handle_backend_event(part(5, 2, 3, "B"));
    h.service
        .handle_backend_event(BackendEvent::Tick { now_ms: 1_099 });
    assert!(h.service.state().sms.is_empty());
    h.service.handle_backend_event(part(5, 3, 3, "C"));
    h.service
        .handle_backend_event(BackendEvent::Tick { now_ms: 1_100 });
    let state = h.service.state();
    assert_eq!(state.sms.len(), 1);
    let sms = &state.sms[&MessageId::from("m1")];
    assert_eq!(sms.state, SmsState::Failed);
    assert_eq!(sms.failure, Some(SmsFailure::incomplete_multipart()));
    assert_eq!(sms.body, "BC", "partial text is kept");
    assert_eq!(sms.parts.len(), 2);
    // A late part starts a new concatenation instead of resurrecting the old.
    h.service.handle_backend_event(part(5, 1, 3, "A"));
    assert_eq!(h.service.state().sms.len(), 1);
    let record = h
        .records
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["event"] == "telephony-event" && r["message_id"] == "m1")
        .cloned()
        .unwrap();
    assert_eq!(record["sms"]["failure"]["kind"], "failed");
    assert_eq!(record["sms"]["failure"]["reason"], "incomplete-multipart");
    assert_eq!(record["sms"]["state"], "failed");
    assert_eq!(record["sms"]["parts"].as_array().unwrap().len(), 2);
    assert!(record["sms"].get("body").is_none());
    assert!(record["sms"].get("party").is_none());
}

// ---------------------------------------------------------------------------
// Persistence and restart reconciliation
// ---------------------------------------------------------------------------

#[test]
fn restart_over_the_same_data_dir_reproduces_the_snapshot() {
    let first = harness("restart");
    let mut messages = first.conn(MESSAGES_PID);
    first.send(
        &mut messages,
        Request::SendSms {
            to: NUMBER.into(),
            body: "out".into(),
        },
    );
    first
        .service
        .handle_backend_event(BackendEvent::SmsChanged {
            handle: SmsHandle::from("s1"),
            state: SmsState::Sent,
            failure: None,
        });
    first
        .service
        .handle_backend_event(BackendEvent::SmsReceived {
            from: number(),
            body: "in".into(),
        });
    first.send(
        &mut messages,
        Request::MarkRead {
            id: MessageId::from("m2"),
        },
    );
    let before = first.service.state();
    drop(first.service);

    // Same directory, fresh process: the store is the only carrier.
    let (recorder, records) = Recorder::capturing("mock");
    let backend = FakeBackend::ready();
    backend.push_call(BackendCall {
        handle: CallHandle::from("h1"),
        direction: Direction::Incoming,
        number: number(),
        state: CallState::Ringing,
        end_reason: None,
    });
    let second = Service::new(
        Arc::new(backend),
        SmsStore::new(&first.dir),
        recorder,
        ServiceOptions::default(),
    )
    .unwrap();
    let after = second.state();
    assert_eq!(after.sms, before.sms);
    assert_eq!(after.calls.len(), 1, "live backend calls are adopted");
    assert_eq!(after.calls[&CallId::from("c1")].state, CallState::Ringing);
    assert_eq!(captured_events(&records)[0], "telephony-event");
    let restarted = records.lock().unwrap()[0].clone();
    assert_eq!(restarted["kind"], "service-restarted");
    assert_eq!(restarted["state"]["sms"], 2);
    assert_eq!(restarted["state"]["calls"], 1);

    // Ids continue after the persisted counter.
    second.handle_backend_event(BackendEvent::SmsReceived {
        from: number(),
        body: "third".into(),
    });
    assert!(second.state().sms.contains_key(&MessageId::from("m3")));
    let _ = std::fs::remove_dir_all(&first.dir);
}

#[test]
fn corrupt_store_fails_closed_and_preserves_authoritative_file() {
    let dir = temp_data_dir("corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sms.json"), "nope").unwrap();
    let (recorder, _records) = Recorder::capturing("mock");
    let error = Service::new(
        Arc::new(FakeBackend::ready()),
        SmsStore::new(&dir),
        recorder,
        ServiceOptions::default(),
    )
    .err()
    .expect("corrupt stores must prevent startup");
    assert!(error.contains("is corrupt"));
    assert_eq!(
        std::fs::read_to_string(dir.join("sms.json")).unwrap(),
        "nope"
    );
    assert_eq!(
        std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("sms.json.corrupt-"))
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn service_restarted_event_folds_into_a_client_snapshot() {
    let h = harness("fold");
    let mut local = ferrix_telephony_api::TelephonyState::new();
    let event = TelephonyEvent::ServiceRestarted {
        state: h.service.state(),
    };
    event.apply_to(&mut local);
    assert_eq!(local, h.service.state());
}

// ---------------------------------------------------------------------------
// Socket server end to end (nested host: no peer credentials, allow-all)
// ---------------------------------------------------------------------------

#[test]
fn socket_server_serves_hello_state_subscribe_and_rejects_garbage() {
    let h = harness("socket");
    let socket = h.dir.join("telephonyd.sock");
    let listener = Server::bind(&socket).unwrap();
    let server = Arc::new(Server::new(Arc::clone(&h.service), Arc::new(AllowAll)));
    std::thread::spawn(move || server.serve(listener));

    let mut client = Client::connect(&socket, ClientKind::Tool, Duration::from_secs(2)).unwrap();
    assert_eq!(client.evidence(), "mock");
    let call = client.dial(NUMBER).unwrap();
    assert_eq!(call.id, CallId::from("c1"));
    let (messages, total) = client.list_sms(SmsFilter::default()).unwrap();
    assert!(messages.is_empty());
    assert_eq!(total, 0);
    assert!(matches!(
        client.expect_ok(&Request::EnterPin { pin: "x".into() }),
        Err(Ok(TelephonyError::Protocol { .. }))
    ));

    // Garbage on the wire gets a protocol error and nothing else changes.
    let mut raw = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    write_line(&mut raw, &serde_json::json!({"request":"state","extra":1})).unwrap();
    let mut reader = std::io::BufReader::new(raw.try_clone().unwrap());
    let mut line = String::new();
    std::io::BufRead::read_line(&mut reader, &mut line).unwrap();
    let reply: Reply = serde_json::from_str(&line).unwrap();
    assert!(matches!(reply, Reply::ProtocolError { .. }));

    // Subscribe: snapshot first, then deltas.
    let subscription = Client::connect(&socket, ClientKind::Tool, Duration::from_secs(2))
        .unwrap()
        .subscribe()
        .unwrap();
    assert!(matches!(subscription.state, StateView::Full(_)));
    let events = subscription.spawn(Arc::new(|| {}));
    h.service.handle_backend_event(BackendEvent::CallChanged {
        handle: CallHandle::from("h1"),
        state: CallState::Active,
        end_reason: None,
    });
    let event = events.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(event.name(), "call-changed");
    let _ = std::fs::remove_dir_all(&h.dir);
}

// ---------------------------------------------------------------------------
// Incoming-call attention
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CountingAttention(std::sync::atomic::AtomicUsize);

impl ferrix_telephonyd::Attention for CountingAttention {
    fn incoming_call(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn only_a_new_ringing_incoming_call_raises_attention() {
    let h = harness("attention");
    let attention = Arc::new(CountingAttention::default());
    h.service.set_attention(attention.clone());
    let count = || attention.0.load(std::sync::atomic::Ordering::SeqCst);
    // Outgoing calls never raise anything.
    h.service.handle_backend_event(BackendEvent::CallAdded {
        handle: CallHandle::from("out"),
        direction: Direction::Outgoing,
        number: number(),
        state: CallState::Dialing,
    });
    assert_eq!(count(), 0);
    h.service.handle_backend_event(BackendEvent::CallAdded {
        handle: CallHandle::from("in"),
        direction: Direction::Incoming,
        number: number(),
        state: CallState::Ringing,
    });
    assert_eq!(count(), 1);
    // A duplicate add and later state changes of the same call do not.
    h.service.handle_backend_event(BackendEvent::CallAdded {
        handle: CallHandle::from("in"),
        direction: Direction::Incoming,
        number: number(),
        state: CallState::Ringing,
    });
    h.service.handle_backend_event(BackendEvent::CallChanged {
        handle: CallHandle::from("in"),
        state: CallState::Active,
        end_reason: None,
    });
    assert_eq!(count(), 1);
    // A second incoming call rings again.
    h.service.handle_backend_event(BackendEvent::CallAdded {
        handle: CallHandle::from("in2"),
        direction: Direction::Incoming,
        number: number(),
        state: CallState::Ringing,
    });
    assert_eq!(count(), 2);
}
