//! Host mock-control channel owned by the daemon (ADR-006, lead decision
//! 2026-09-21).
//!
//! The scenario runner reaches only this channel (virtio-serial port in the
//! guest, Unix socket nested), never the protocol socket. So besides
//! forwarding backend-mutation commands to the mock adapter's own handler,
//! this handler executes a small set of **test-only local client verbs**
//! through exactly the request path a protocol client uses
//! ([`crate::server::dispatch`]) under a synthetic host-tool identity that
//! holds every telephony capability. Typed rejections come back as
//! `Ack::Error`.
//!
//! This is not an application API: the channel exists only with the mock
//! backend (a production backend refuses `--mock-control`) and is never
//! visible inside an application sandbox.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferrix_mock_control::{Ack, Handler};
use ferrix_telephony_api::{AudioRoute, CallId, CallState, Dtmf, MessageId, RadioPower};
use ferrix_telephony_protocol::{Reply, Request};
use serde::Deserialize;
use serde_json::Value;

use crate::identity::{AllowAll, Identity, IdentityInfo, all_capabilities};
use crate::server::{Connection, Outcome, dispatch};
use crate::service::Service;

/// Application id recorded for host-tool actions.
pub const HOST_TOOL_APP: &str = "org.ferrix.host-tool";

/// Longest an ack waits for the daemon to publish what a command caused.
pub const SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

/// Command names handled locally; everything else is forwarded.
pub const LOCAL_COMMANDS: &[&str] = &[
    "dial",
    "answer",
    "hangup",
    "reject",
    "hold",
    "resume",
    "dtmf",
    "send-sms",
    "enter-pin",
    "set-audio-route",
    "set-radio-power",
    "mark-read",
];

/// Test-only client verbs. `call_id` defaults to the single call the verb
/// applies to (ringing for `answer`/`reject`, active for `hold`, held for
/// `resume`, any live call for `hangup`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LocalCommand {
    Dial {
        number: String,
    },
    Answer {
        #[serde(default)]
        call_id: Option<String>,
    },
    Hangup {
        #[serde(default)]
        call_id: Option<String>,
    },
    Reject {
        #[serde(default)]
        call_id: Option<String>,
    },
    Hold {
        #[serde(default)]
        call_id: Option<String>,
    },
    Resume {
        #[serde(default)]
        call_id: Option<String>,
    },
    Dtmf {
        #[serde(default)]
        call_id: Option<String>,
        digit: Dtmf,
    },
    SendSms {
        number: String,
        body: String,
    },
    EnterPin {
        pin: String,
    },
    SetAudioRoute {
        route: AudioRoute,
    },
    SetRadioPower {
        power: RadioPower,
    },
    MarkRead {
        message_id: String,
    },
}

pub fn host_tool_identity() -> IdentityInfo {
    IdentityInfo {
        app: Some(HOST_TOOL_APP.to_owned()),
        granted: all_capabilities(),
    }
}

/// [`Handler`] composed by the binary: local verbs run through the service,
/// everything else goes to `backend` (the mock adapter's handler).
pub struct LocalControl {
    service: Arc<Service>,
    backend: Arc<dyn Handler>,
    /// Commands are serialized so event capture stays unambiguous.
    serial: Mutex<()>,
}

impl LocalControl {
    pub fn new(service: Arc<Service>, backend: Arc<dyn Handler>) -> Self {
        Self {
            service,
            backend,
            serial: Mutex::new(()),
        }
    }

    fn resolve_call(
        &self,
        explicit: Option<String>,
        pred: impl Fn(CallState) -> bool,
        verb: &str,
    ) -> Result<CallId, String> {
        if let Some(id) = explicit {
            return Ok(CallId(id));
        }
        let state = self.service.state();
        let mut matching = state.calls.values().filter(|c| pred(c.state));
        let first = matching
            .next()
            .ok_or_else(|| format!("no call to {verb}; pass call_id"))?;
        if matching.next().is_some() {
            return Err(format!("more than one call to {verb}; pass call_id"));
        }
        Ok(first.id.clone())
    }

    fn to_request(&self, command: LocalCommand) -> Result<Request, String> {
        Ok(match command {
            LocalCommand::Dial { number } => Request::Dial { number },
            LocalCommand::Answer { call_id } => Request::Answer {
                call_id: self.resolve_call(call_id, |s| s == CallState::Ringing, "answer")?,
            },
            LocalCommand::Reject { call_id } => Request::Reject {
                call_id: self.resolve_call(call_id, |s| s == CallState::Ringing, "reject")?,
            },
            LocalCommand::Hangup { call_id } => Request::Hangup {
                call_id: self.resolve_call(call_id, CallState::is_live, "hangup")?,
            },
            LocalCommand::Hold { call_id } => Request::Hold {
                call_id: self.resolve_call(call_id, |s| s == CallState::Active, "hold")?,
            },
            LocalCommand::Resume { call_id } => Request::Resume {
                call_id: self.resolve_call(call_id, |s| s == CallState::Held, "resume")?,
            },
            LocalCommand::Dtmf { call_id, digit } => Request::Dtmf {
                call_id: self.resolve_call(call_id, |s| s == CallState::Active, "dtmf")?,
                digit,
            },
            LocalCommand::SendSms { number, body } => Request::SendSms { to: number, body },
            LocalCommand::EnterPin { pin } => Request::EnterPin { pin },
            LocalCommand::SetAudioRoute { route } => Request::SetAudioRoute { route },
            LocalCommand::SetRadioPower { power } => Request::SetRadioPower { power },
            LocalCommand::MarkRead { message_id } => Request::MarkRead {
                id: MessageId(message_id),
            },
        })
    }

    /// Runs a local verb through `dispatch` exactly like a socket client.
    pub fn apply_local(&self, name: &str, line: &str) -> Ack {
        let command: LocalCommand = match serde_json::from_str(line) {
            Ok(command) => command,
            Err(error) => return Ack::error(Some(name), format!("invalid command: {error}")),
        };
        let request = match self.to_request(command) {
            Ok(request) => request,
            Err(reason) => return Ack::error(Some(name), reason),
        };
        let source = AllowAll;
        let mut connection =
            Connection::preauthenticated(host_tool_identity(), source.generation());
        // The backend answers synchronously but reports progress through the
        // event loop; wait for it so the ack follows the records it caused.
        let (outcome, events) = self.service.capture_events(|| {
            let outcome = dispatch(&self.service, &source, &mut connection, request);
            self.service.settle(SETTLE_TIMEOUT);
            outcome
        });
        match outcome {
            Outcome::Reply(reply) => match *reply {
                Reply::Ok | Reply::Call { .. } | Reply::Sms { .. } | Reply::SmsList { .. } => {
                    Ack::ok(name, events, self.service.control_state_json())
                }
                Reply::Error { error } => Ack::error(
                    Some(name),
                    format!(
                        "{}: {}",
                        error.kind(),
                        serde_json::to_string(&error).unwrap_or_default()
                    ),
                ),
                Reply::ProtocolError { reason } => Ack::error(Some(name), reason),
                Reply::Hello { .. } | Reply::State { .. } => {
                    Ack::error(Some(name), "unexpected reply")
                }
            },
            Outcome::Subscribe(_) => Ack::error(Some(name), "subscribe is not a control verb"),
        }
    }
}

impl Handler for LocalControl {
    fn handle(&self, line: &str) -> Ack {
        let _serial = self
            .serial
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let name = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("cmd").and_then(Value::as_str).map(str::to_owned));
        match name {
            Some(name) if LOCAL_COMMANDS.contains(&name.as_str()) => self.apply_local(&name, line),
            _ => {
                let ack = self.backend.handle(line);
                // Records for the mutation are on stdout before the ack.
                self.service.settle(SETTLE_TIMEOUT);
                ack
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::Recorder;
    use crate::service::ServiceOptions;
    use crate::store::SmsStore;
    use crate::testing::{FakeBackend, temp_data_dir};
    use ferrix_telephony_adapter_api::{BackendEvent, CallHandle};
    use ferrix_telephony_api::{Direction, E164};

    struct Forwarded;
    impl Handler for Forwarded {
        fn handle(&self, line: &str) -> Ack {
            Ack::ok("forwarded", vec![line.to_owned()], Value::Null)
        }
    }

    fn control() -> (LocalControl, Arc<Service>) {
        let (recorder, _) = Recorder::capturing("mock");
        let service = Service::new(
            Arc::new(FakeBackend::ready()),
            SmsStore::new(temp_data_dir("control")),
            recorder,
            ServiceOptions::default(),
        )
        .unwrap();
        (
            LocalControl::new(Arc::clone(&service), Arc::new(Forwarded)),
            service,
        )
    }

    fn ok(ack: &Ack) -> (&Vec<String>, &Value) {
        match ack {
            Ack::Ok { events, state, .. } => (events, state),
            Ack::Error { reason, .. } => panic!("{reason}"),
        }
    }

    fn err(ack: &Ack) -> &str {
        match ack {
            Ack::Error { reason, .. } => reason,
            Ack::Ok { .. } => panic!("unexpected ok"),
        }
    }

    #[test]
    fn local_verbs_run_through_dispatch_and_ack_with_events() {
        let (control, service) = control();
        let ack = control.handle(r#"{"cmd":"dial","number":"+15550100042"}"#);
        let (events, state) = ok(&ack);
        assert_eq!(events, &["call-changed"]);
        assert_eq!(state["calls"][0]["call_id"], "c1");
        assert_eq!(state["calls"][0]["state"], "dialing");
        assert!(!state.to_string().contains("0100042"));
        assert!(err(&control.handle(r#"{"cmd":"hold"}"#)).contains("no call to hold"));
        let ack = control.handle(r#"{"cmd":"hangup"}"#);
        assert_eq!(ok(&ack).0, &["call-changed"]);
        assert_eq!(
            service.state().calls[&CallId::from("c1")].state,
            CallState::Ended
        );
    }

    #[test]
    fn typed_rejections_are_ack_errors() {
        let (control, service) = control();
        let reason = err(&control.handle(r#"{"cmd":"dial","number":"911"}"#)).to_owned();
        assert!(reason.starts_with("unsupported:"), "{reason}");
        assert!(reason.contains("emergency"), "{reason}");
        assert!(err(&control.handle(r#"{"cmd":"answer"}"#)).contains("no call to answer"));
        assert!(
            err(&control.handle(r#"{"cmd":"answer","call_id":"c9"}"#)).starts_with("not-found:")
        );
        assert!(
            err(&control.handle(r#"{"cmd":"mark-read","message_id":"m9"}"#))
                .starts_with("not-found:")
        );
        assert!(
            err(&control.handle(r#"{"cmd":"dial","number":"+15550100042","app":"x"}"#))
                .contains("invalid command")
        );
        service.handle_backend_event(BackendEvent::CallAdded {
            handle: CallHandle::from("h1"),
            direction: Direction::Incoming,
            number: E164::new("+15550100042").unwrap(),
            state: CallState::Ringing,
        });
        let ack = control.handle(r#"{"cmd":"answer"}"#);
        assert_eq!(ok(&ack).1["calls"][0]["state"], "active");
        let ack = control.handle(r#"{"cmd":"set-audio-route","route":"speaker"}"#);
        ok(&ack);
        let ack = control.handle(r#"{"cmd":"send-sms","number":"+15550100042","body":"hi"}"#);
        let (events, state) = ok(&ack);
        assert_eq!(events, &["sms-changed"]);
        assert_eq!(state["sms"][0]["message_id"], "m1");
        assert!(!state.to_string().contains("hi\""));
    }

    #[test]
    fn acks_follow_the_records_the_command_caused() {
        struct Emitting(FakeBackend);
        impl Handler for Emitting {
            fn handle(&self, _line: &str) -> Ack {
                self.0.emit(BackendEvent::CallAdded {
                    handle: CallHandle::from("h7"),
                    direction: Direction::Incoming,
                    number: E164::new("+15550100042").unwrap(),
                    state: CallState::Ringing,
                });
                Ack::ok("incoming-call", vec!["call-added".into()], Value::Null)
            }
        }
        let (recorder, records) = Recorder::capturing("mock");
        let backend = FakeBackend::ready();
        let service = Service::new(
            Arc::new(backend.clone()),
            SmsStore::new(temp_data_dir("control-settle")),
            recorder,
            ServiceOptions::default(),
        )
        .unwrap();
        service.spawn_event_loop();
        let control = LocalControl::new(Arc::clone(&service), Arc::new(Emitting(backend)));
        assert!(
            control
                .handle(r#"{"cmd":"incoming-call","number":"+15550100042"}"#)
                .is_ok()
        );
        // Without waiting this would race the event loop.
        assert_eq!(
            service.state().calls[&CallId::from("c1")].state,
            CallState::Ringing
        );
        let kinds: Vec<String> = records
            .lock()
            .unwrap()
            .iter()
            .filter_map(|r| r["kind"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(kinds, ["service-restarted", "call-changed"]);
        let ack = control.handle(r#"{"cmd":"answer"}"#);
        assert_eq!(ok(&ack).1["calls"][0]["state"], "active");
    }

    #[test]
    fn local_verb_acks_include_events_the_backend_reports_asynchronously() {
        let (recorder, _) = Recorder::capturing("mock");
        let backend = FakeBackend::ready();
        backend.emit_audio_changes();
        let service = Service::new(
            Arc::new(backend.clone()),
            SmsStore::new(temp_data_dir("control-async")),
            recorder,
            ServiceOptions::default(),
        )
        .unwrap();
        service.spawn_event_loop();
        let control = LocalControl::new(Arc::clone(&service), Arc::new(Forwarded));
        for _ in 0..20 {
            let ack = control.handle(r#"{"cmd":"set-audio-route","route":"speaker"}"#);
            let (events, state) = ok(&ack);
            assert_eq!(events, &["audio-route-changed"]);
            assert_eq!(state["audio"], "speaker");
            let ack = control.handle(r#"{"cmd":"set-audio-route","route":"earpiece"}"#);
            assert_eq!(ok(&ack).0, &["audio-route-changed"]);
        }
    }

    #[test]
    fn other_commands_are_forwarded_verbatim() {
        let (control, _) = control();
        let ack = control.handle(r#"{"cmd":"tick","ms":5}"#);
        match ack {
            Ack::Ok { ack, events, .. } => {
                assert_eq!(ack, "forwarded");
                assert_eq!(events, [r#"{"cmd":"tick","ms":5}"#]);
            }
            other => panic!("{other:?}"),
        }
        // Malformed lines reach the backend handler too (it rejects them).
        assert!(control.handle("{").is_ok());
        assert!(LOCAL_COMMANDS.contains(&"set-radio-power"));
        assert!(LOCAL_COMMANDS.contains(&"dtmf"));
        assert!(!LOCAL_COMMANDS.contains(&"set-radio"));
    }
}
