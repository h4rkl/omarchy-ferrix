//! Service and protocol tests with the scripted backend: hello ordering,
//! schema mismatch, malformed lines, every unauthorized path, state-grant
//! cannot control, subscription fan-out, and restart reconciliation.
//! Evidence label `mock`; no wall-clock assertions.

use std::sync::Arc;
use std::time::Duration;

use ferrix_network_adapter_api::{BackendError, BackendEvent};
use ferrix_network_api::{
    AccessPoint, AdapterKind, AdapterPresence, ApId, ConnectionState, Connectivity, NetworkError,
    NetworkEvent, NetworkState, Passphrase, ProfileId, RadioState, SCHEMA_VERSION, ScanState,
    SecurityType, Ssid,
};
use ferrix_network_protocol::{Client, ClientError, ClientKind, Reply, Request};
use ferrix_networkd::identity::{
    AllowAll, CAP_CONTROL, CAP_STATE, DenyAll, Identity, PeerIdentity, StaticIdentity,
};
use ferrix_networkd::records::{Captured, Recorder, captured_events};
use ferrix_networkd::server::{Connection, Outcome, Server, dispatch, dispatch_line};
use ferrix_networkd::service::Service;
use ferrix_networkd::testing::{ChannelSink, FakeBackend};

const SETTINGS: &str = "org.ferrix.settings";
const READER: &str = "org.ferrix.status";
const SETTINGS_PID: u32 = 4100;
const READER_PID: u32 = 4200;
const STRANGER_PID: u32 = 4300;

fn boot_state() -> NetworkState {
    NetworkState {
        adapter: AdapterPresence::Present {
            kind: AdapterKind::Wifi,
        },
        radio: RadioState::Enabled,
        access_points: vec![AccessPoint {
            id: ApId::new("ap-1"),
            ssid: Ssid::from("ferrix-lab"),
            bssid_hash: "0011223344556677".into(),
            signal_percent: 78,
            security: SecurityType::Wpa2Psk,
            frequency_mhz: Some(2437),
        }],
        ..NetworkState::default()
    }
}

struct Harness {
    service: Arc<Service>,
    backend: FakeBackend,
    identity: Arc<StaticIdentity>,
    captured: Captured,
}

fn harness() -> Harness {
    let backend = FakeBackend::new(boot_state());
    let identity = Arc::new(StaticIdentity::new());
    identity.grant(
        SETTINGS_PID,
        PeerIdentity::granted(Some(SETTINGS), &[CAP_STATE, CAP_CONTROL]),
    );
    identity.grant(
        READER_PID,
        PeerIdentity::granted(Some(READER), &[CAP_STATE]),
    );
    let (recorder, captured) = Recorder::capturing("mock");
    let service = Service::new(
        Arc::new(backend.clone()),
        identity.clone() as Arc<dyn Identity>,
        recorder,
    )
    .unwrap();
    service.start();
    Harness {
        service,
        backend,
        identity,
        captured,
    }
}

fn hello() -> Request {
    Request::Hello {
        schema_version: SCHEMA_VERSION,
        client_kind: ClientKind::Application,
    }
}

fn helloed(service: &Service, pid: u32) -> Connection {
    let mut connection = Connection::with_pid(Some(pid));
    assert!(matches!(
        dispatch(service, &mut connection, hello()),
        Outcome::Reply(Reply::Hello { .. })
    ));
    connection
}

fn unauthorized(capability: &str) -> Outcome {
    Outcome::Reply(Reply::Error {
        error: NetworkError::Unauthorized {
            capability: capability.to_owned(),
        },
    })
}

#[test]
fn hello_must_come_first_and_only_once() {
    let h = harness();
    let mut connection = Connection::with_pid(Some(SETTINGS_PID));
    let Outcome::Reply(Reply::ProtocolError { reason }) =
        dispatch(&h.service, &mut connection, Request::State {})
    else {
        panic!("state before hello must be a protocol error");
    };
    assert!(reason.contains("hello"));
    assert!(!connection.hello_done);
    let Outcome::Reply(Reply::Hello {
        schema_version,
        evidence,
        state,
    }) = dispatch(&h.service, &mut connection, hello())
    else {
        panic!("hello reply");
    };
    assert_eq!(schema_version, SCHEMA_VERSION);
    assert_eq!(evidence, "mock");
    assert_eq!(state, Some(boot_state()));
    assert!(matches!(
        dispatch(&h.service, &mut connection, hello()),
        Outcome::Reply(Reply::ProtocolError { .. })
    ));
    assert_eq!(
        captured_events(&h.captured),
        ["networkd-request-rejected", "networkd-request-rejected"]
    );
}

#[test]
fn schema_mismatch_is_rejected_at_hello() {
    let h = harness();
    let mut connection = Connection::with_pid(Some(SETTINGS_PID));
    let Outcome::Reply(Reply::ProtocolError { reason }) = dispatch(
        &h.service,
        &mut connection,
        Request::Hello {
            schema_version: 2,
            client_kind: ClientKind::Tool,
        },
    ) else {
        panic!("schema 2 must be rejected");
    };
    assert!(reason.contains("schema 2"), "{reason}");
    assert!(!connection.hello_done, "a rejected hello does not count");
    assert!(matches!(
        dispatch(&h.service, &mut connection, Request::Scan {}),
        Outcome::Reply(Reply::ProtocolError { .. })
    ));
    assert!(h.backend.calls().is_empty());
}

#[test]
fn malformed_and_unknown_field_lines_never_touch_state() {
    let h = harness();
    let mut connection = helloed(&h.service, SETTINGS_PID);
    for line in [
        "garbage",
        r#"{"request":"scan","extra":1}"#,
        r#"{"request":"connect","ap":"ap-1","bssid":"aa:bb"}"#,
        r#"{"request":"warp"}"#,
        r#"{"reply":"ok"}"#,
        r#"{"request":"set-radio"}"#,
    ] {
        let Outcome::Reply(Reply::ProtocolError { reason }) =
            dispatch_line(&h.service, &mut connection, line)
        else {
            panic!("{line} must be a protocol error");
        };
        assert!(reason.contains("invalid message"), "{reason}");
    }
    assert!(h.backend.calls().is_empty());
    assert!(connection.hello_done && !connection.subscribed);
    // A valid line on the same connection still works.
    assert_eq!(
        dispatch_line(&h.service, &mut connection, r#"{"request":"scan"}"#),
        Outcome::Reply(Reply::Ok)
    );
    assert_eq!(h.backend.calls().len(), 1);
}

#[test]
fn unknown_pid_and_missing_pid_are_denied_everything() {
    let h = harness();
    for pid in [Some(STRANGER_PID), None] {
        let mut connection = Connection::with_pid(pid);
        let Outcome::Reply(Reply::Hello { state, .. }) =
            dispatch(&h.service, &mut connection, hello())
        else {
            panic!("hello is a handshake, not a grant");
        };
        assert!(state.is_none(), "no snapshot without network-state");
        assert_eq!(
            dispatch(&h.service, &mut connection, Request::State {}),
            unauthorized(CAP_STATE)
        );
        assert_eq!(
            dispatch(&h.service, &mut connection, Request::Subscribe {}),
            unauthorized(CAP_STATE)
        );
        assert!(!connection.subscribed);
        assert_eq!(
            dispatch(&h.service, &mut connection, Request::Scan {}),
            unauthorized(CAP_CONTROL)
        );
    }
    assert!(h.backend.calls().is_empty());
    let records = h.captured.lock().unwrap();
    let denied: Vec<&serde_json::Value> = records
        .iter()
        .filter(|r| r["event"] == "networkd-request-denied")
        .collect();
    assert_eq!(denied.len(), 6);
    assert!(denied[0]["app"].is_null());
    assert_eq!(denied[0]["capability"], "network-state");
    assert_eq!(denied[0]["request"], "state");
    assert_eq!(denied[2]["capability"], "network-control");
    assert_eq!(denied[2]["request"], "scan");
}

#[test]
fn state_grant_cannot_reach_any_control_verb() {
    let h = harness();
    let mut connection = helloed(&h.service, READER_PID);
    assert!(matches!(
        dispatch(&h.service, &mut connection, Request::State {}),
        Outcome::Reply(Reply::State { .. })
    ));
    let controls = [
        Request::SetRadio { enabled: false },
        Request::Scan {},
        Request::Connect {
            ap: ApId::new("ap-1"),
            passphrase: Some(Passphrase::new("mock-passphrase")),
        },
        Request::ConnectProfile {
            profile: ProfileId::new("profile-1"),
        },
        Request::Disconnect {},
        Request::Forget {
            profile: ProfileId::new("profile-1"),
        },
    ];
    for request in controls {
        let name = request.name();
        assert_eq!(
            dispatch(&h.service, &mut connection, request),
            unauthorized(CAP_CONTROL),
            "{name}"
        );
    }
    assert!(
        h.backend.calls().is_empty(),
        "denied requests never reach the backend"
    );
    let records = h.captured.lock().unwrap();
    let denied: Vec<String> = records
        .iter()
        .filter(|r| r["event"] == "networkd-request-denied")
        .map(|r| r["request"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        denied,
        [
            "set-radio",
            "scan",
            "connect",
            "connect-profile",
            "disconnect",
            "forget"
        ]
    );
    for record in records.iter() {
        assert_eq!(record["app"], READER);
        let text = record.to_string();
        assert!(!text.contains("mock-passphrase"), "{text}");
    }
}

#[test]
fn control_grant_reaches_the_backend_and_forwards_typed_errors() {
    let h = harness();
    let mut connection = helloed(&h.service, SETTINGS_PID);
    assert_eq!(
        dispatch(
            &h.service,
            &mut connection,
            Request::SetRadio { enabled: false }
        ),
        Outcome::Reply(Reply::Ok)
    );
    assert_eq!(
        dispatch(
            &h.service,
            &mut connection,
            Request::Connect {
                ap: ApId::new("ap-1"),
                passphrase: Some(Passphrase::new("mock-passphrase")),
            }
        ),
        Outcome::Reply(Reply::Ok)
    );
    h.backend
        .fail_next(BackendError::Network(NetworkError::RadioDisabled));
    assert_eq!(
        dispatch(&h.service, &mut connection, Request::Scan {}),
        Outcome::Reply(Reply::Error {
            error: NetworkError::RadioDisabled
        })
    );
    h.backend.fail_next(BackendError::unsupported("forget"));
    assert_eq!(
        dispatch(
            &h.service,
            &mut connection,
            Request::Forget {
                profile: ProfileId::new("profile-1")
            }
        ),
        Outcome::Reply(Reply::Error {
            error: NetworkError::Unsupported {
                feature: "forget".into()
            }
        })
    );
    let calls = h.backend.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].op, "set-radio:off");
    assert_eq!(calls[1].op, "connect");
    assert_eq!(calls[1].id.as_deref(), Some("ap-1"));
    assert!(calls[1].passphrase_present);
    assert_eq!(calls[3].op, "forget");
}

#[test]
fn forged_app_ids_in_payloads_are_not_a_thing() {
    // The wire has no field for a caller to name itself; any attempt is an
    // unknown field and therefore a protocol error, never a grant.
    let h = harness();
    let mut connection = helloed(&h.service, STRANGER_PID);
    let Outcome::Reply(Reply::ProtocolError { .. }) = dispatch_line(
        &h.service,
        &mut connection,
        r#"{"request":"scan","app":"org.ferrix.settings"}"#,
    ) else {
        panic!("forged identity must be a protocol error");
    };
    assert_eq!(
        dispatch_line(&h.service, &mut connection, r#"{"request":"scan"}"#),
        unauthorized(CAP_CONTROL)
    );
    assert!(h.backend.calls().is_empty());
}

#[test]
fn identity_is_re_resolved_on_every_request_and_after_appd_restart() {
    let h = harness();
    let mut connection = helloed(&h.service, SETTINGS_PID);
    assert_eq!(
        dispatch(&h.service, &mut connection, Request::Scan {}),
        Outcome::Reply(Reply::Ok)
    );
    // Revocation applies immediately to a live connection.
    h.identity.revoke(SETTINGS_PID);
    assert_eq!(
        dispatch(&h.service, &mut connection, Request::Scan {}),
        unauthorized(CAP_CONTROL)
    );
    // Grants restored are also visible immediately; a later authority
    // restart remains conservative and still re-resolves.
    h.identity.grant(
        SETTINGS_PID,
        PeerIdentity::granted(Some(SETTINGS), &[CAP_STATE, CAP_CONTROL]),
    );
    assert_eq!(
        dispatch(&h.service, &mut connection, Request::Scan {}),
        Outcome::Reply(Reply::Ok)
    );
    h.identity.bump();
    assert_eq!(
        dispatch(&h.service, &mut connection, Request::Scan {}),
        Outcome::Reply(Reply::Ok)
    );
    assert_eq!(h.backend.calls().len(), 3);
}

#[test]
fn subscribers_get_the_snapshot_first_then_deltas_in_order() {
    let h = harness();
    let (sink, rx) = ChannelSink::new();
    h.service.subscribe(Box::new(sink)).unwrap();
    assert_eq!(h.service.subscriber_count(), 1);
    let first: Reply =
        serde_json::from_str(&rx.recv_timeout(Duration::from_secs(2)).unwrap()).unwrap();
    assert_eq!(
        first,
        Reply::State {
            state: boot_state()
        }
    );
    h.backend
        .push(BackendEvent::Changed(NetworkEvent::ScanChanged {
            scan: ScanState::Scanning,
        }));
    h.backend
        .push(BackendEvent::Changed(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Connected {
                ap: ApId::new("ap-1"),
            },
        }));
    let mut local = boot_state();
    for expected in ["scan-changed", "connection-changed"] {
        let line = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let event: NetworkEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(event.name(), expected);
        event.apply_to(&mut local);
    }
    // Folding the deltas reproduces the service's state exactly.
    assert_eq!(local, h.service.state());
    assert!(local.connection.is_connected());
    // Subscribed connections accept no further requests.
    let mut connection = helloed(&h.service, READER_PID);
    assert_eq!(
        dispatch(&h.service, &mut connection, Request::Subscribe {}),
        Outcome::Subscribe
    );
    assert!(matches!(
        dispatch(&h.service, &mut connection, Request::State {}),
        Outcome::Reply(Reply::ProtocolError { .. })
    ));
}

#[test]
fn subscriber_is_dropped_when_state_grant_is_revoked() {
    let h = harness();
    let (sink, rx) = ChannelSink::new();
    h.service
        .subscribe_authorized(Some(READER_PID), Box::new(sink))
        .unwrap();
    let _: Reply = serde_json::from_str(&rx.recv().unwrap()).unwrap();
    h.identity.revoke(READER_PID);
    h.service
        .handle_backend_event(BackendEvent::Changed(NetworkEvent::ScanChanged {
            scan: ScanState::Scanning,
        }));
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    assert_eq!(h.service.subscriber_count(), 0);
}

#[test]
fn backend_loss_and_restart_reconcile_through_a_snapshot() {
    let h = harness();
    let (sink, rx) = ChannelSink::new();
    h.service.subscribe(Box::new(sink)).unwrap();
    let _snapshot = rx.recv_timeout(Duration::from_secs(2)).unwrap();

    h.backend.push(BackendEvent::Lost {
        reason: "backend exited".into(),
    });
    let line = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let event: NetworkEvent = serde_json::from_str(&line).unwrap();
    let NetworkEvent::RadioChanged {
        radio: RadioState::Unavailable { reason },
    } = &event
    else {
        panic!("loss is published as an unavailable radio, got {event:?}");
    };
    assert!(reason.contains("backend lost"));

    let mut fresh = boot_state();
    fresh.connection = ConnectionState::Connected {
        ap: ApId::new("ap-1"),
    };
    fresh.connectivity = Connectivity::Full;
    h.backend.push(BackendEvent::Restarted {
        state: fresh.clone(),
    });
    let line = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let event: NetworkEvent = serde_json::from_str(&line).unwrap();
    assert_eq!(
        event,
        NetworkEvent::ServiceRestarted {
            state: fresh.clone()
        }
    );
    let mut local = NetworkState::default();
    event.apply_to(&mut local);
    assert_eq!(local, fresh);
    assert_eq!(h.service.state(), fresh);

    // Records: loss, the unavailable-radio delta, the restart snapshot.
    let events = captured_events(&h.captured);
    assert_eq!(
        events,
        ["network-backend-lost", "network-event", "network-event"]
    );
    let records = h.captured.lock().unwrap();
    assert_eq!(records[1]["kind"], "radio-changed");
    assert_eq!(records[1]["radio"]["state"], "unavailable");
    assert_eq!(records[2]["kind"], "service-restarted");
    assert_eq!(records[2]["snapshot"]["connection"], "connected");
    for record in records.iter() {
        let text = record.to_string();
        assert!(!text.contains("ferrix-lab"), "ssid text in record: {text}");
    }
}

#[test]
fn records_never_carry_passphrases_or_raw_addresses() {
    let h = harness();
    let mut connection = helloed(&h.service, SETTINGS_PID);
    dispatch(
        &h.service,
        &mut connection,
        Request::Connect {
            ap: ApId::new("ap-1"),
            passphrase: Some(Passphrase::new("mock-passphrase")),
        },
    );
    h.backend
        .push(BackendEvent::Changed(NetworkEvent::AccessPointsChanged {
            access_points: boot_state().access_points,
        }));
    // Wait for the pump to fold the event.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while captured_events(&h.captured).is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let records = h.captured.lock().unwrap();
    assert!(!records.is_empty());
    for record in records.iter() {
        let text = record.to_string();
        assert!(!text.contains("mock-passphrase"), "{text}");
        assert!(!text.contains("\"bssid\":"), "{text}");
        assert!(!text.contains("ferrix-lab"), "{text}");
    }
}

// ---------------------------------------------------------------------------
// Socket round trips
// ---------------------------------------------------------------------------

fn socket_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ferrix-networkd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn serve(identity: Arc<dyn Identity>, tag: &str) -> (std::path::PathBuf, FakeBackend, Captured) {
    let backend = FakeBackend::new(boot_state());
    let (recorder, captured) = Recorder::capturing("mock");
    let service = Service::new(Arc::new(backend.clone()), identity, recorder).unwrap();
    service.start();
    let path = socket_dir(tag).join("networkd.sock");
    let listener = Server::bind(&path).unwrap();
    let server = Arc::new(Server::new(service));
    std::thread::spawn(move || server.serve(listener));
    (path, backend, captured)
}

#[test]
fn socket_client_round_trip_with_full_grants() {
    // macOS has no SO_PEERCRED pid; AllowAll stands in for a granted app so
    // the socket layer itself is exercised. Authorization is covered above.
    let (path, backend, _captured) = serve(Arc::new(AllowAll), "allow");
    let mut client = Client::connect(&path, ClientKind::Tool, Duration::from_secs(5)).unwrap();
    assert_eq!(client.evidence(), "mock");
    assert_eq!(client.hello_state(), Some(&boot_state()));
    assert_eq!(client.state().unwrap(), boot_state());
    client.scan().unwrap();
    client
        .connect_ap(&ApId::new("ap-1"), Some("mock-passphrase"))
        .unwrap();
    backend.fail_next(BackendError::Network(NetworkError::NotFound {
        id: "profile-9".into(),
    }));
    assert_eq!(
        client.forget(&ProfileId::new("profile-9")),
        Err(ClientError::Network(NetworkError::NotFound {
            id: "profile-9".into()
        }))
    );
    let calls = backend.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls[1].passphrase_present);

    // Subscribe: snapshot first, then the pushed delta.
    let subscription = client.subscribe().unwrap();
    assert_eq!(subscription.state, boot_state());
    // The reader sends the event before calling `wake`, so `recv` can return
    // first; wait (bounded) for the wake signal instead of sampling a counter.
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    let (state, rx) = subscription.spawn(Arc::new(move || {
        let _ = wake_tx.send(());
    }));
    assert_eq!(state, boot_state());
    backend.push(BackendEvent::Changed(NetworkEvent::RadioChanged {
        radio: RadioState::Disabled,
    }));
    let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        event,
        NetworkEvent::RadioChanged {
            radio: RadioState::Disabled
        }
    );
    wake_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("wake called after event delivery");
}

#[test]
fn socket_client_is_denied_without_an_identity() {
    let (path, backend, captured) = serve(Arc::new(DenyAll), "deny");
    let mut client =
        Client::connect(&path, ClientKind::Application, Duration::from_secs(5)).unwrap();
    assert!(client.hello_state().is_none());
    assert_eq!(
        client.state(),
        Err(ClientError::Network(NetworkError::Unauthorized {
            capability: CAP_STATE.into()
        }))
    );
    assert_eq!(
        client.set_radio(true),
        Err(ClientError::Network(NetworkError::Unauthorized {
            capability: CAP_CONTROL.into()
        }))
    );
    assert!(matches!(
        client.subscribe(),
        Err(ClientError::Network(NetworkError::Unauthorized { .. }))
    ));
    assert!(backend.calls().is_empty());
    let events = captured_events(&captured);
    assert_eq!(
        events
            .iter()
            .filter(|e| *e == "networkd-request-denied")
            .count(),
        3
    );
}
