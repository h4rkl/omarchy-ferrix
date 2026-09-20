//! Host-only mock-control channel owned by the daemon (ADR-006).
//!
//! The scenario runner reaches only this channel, never the protocol socket,
//! so besides forwarding backend mutations to the mock adapter's own
//! handler, the daemon executes a small set of **test-only client verbs**
//! through the same [`crate::server::dispatch`] path a protocol client
//! would take, under a synthetic host-tool identity that holds both network
//! capabilities. Typed rejections come back as `Ack::Error`.
//!
//! This channel is not an application API: it exists only with the mock
//! backend (`--mock-control` is refused with the production backend) and
//! nothing here bypasses the daemon's request path.

use std::sync::Arc;
use std::time::Duration;

use ferrix_mock_control::{Ack, Handler};
use ferrix_network_api::{ApId, Passphrase, ProfileId, Ssid};
use ferrix_network_protocol::{Reply, Request};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::identity::{CAP_CONTROL, CAP_STATE, PeerIdentity};
use crate::server::{Connection, Outcome, dispatch};
use crate::service::Service;

/// The synthetic caller for client verbs; shows up as `app` in records.
pub const HOST_TOOL_APP: &str = "host.tool.mock-control";

/// Test-only client verbs. Everything else on the channel is a backend
/// mutation forwarded verbatim to the mock adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ClientCommand {
    /// `Request::Scan`.
    Scan {},
    /// `Request::Connect` after resolving `ssid` among visible access points.
    Connect {
        ssid: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<Passphrase>,
    },
    /// `Request::Disconnect` (named apart from the backend `disconnect`
    /// mutation, which simulates the network dropping the station).
    DisconnectRequest {},
    /// `Request::Forget` after resolving `ssid` among saved profiles.
    Forget { ssid: String },
    /// `Request::SetRadio` (the backend mutation `set-radio` flips the mock
    /// directly without going through the request path).
    SetRadioEnabled { enabled: bool },
}

impl ClientCommand {
    pub const NAMES: [&'static str; 5] = [
        "scan",
        "connect",
        "disconnect-request",
        "forget",
        "set-radio-enabled",
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ClientCommand::Scan {} => "scan",
            ClientCommand::Connect { .. } => "connect",
            ClientCommand::DisconnectRequest {} => "disconnect-request",
            ClientCommand::Forget { .. } => "forget",
            ClientCommand::SetRadioEnabled { .. } => "set-radio-enabled",
        }
    }

    pub fn is_client_verb(name: &str) -> bool {
        Self::NAMES.contains(&name)
    }
}

/// The daemon's mock-control handler.
pub struct MockControl {
    service: Arc<Service>,
    backend: Arc<dyn Handler>,
}

impl MockControl {
    pub fn new(service: Arc<Service>, backend: Arc<dyn Handler>) -> Arc<Self> {
        Arc::new(Self { service, backend })
    }

    fn host_tool_connection(&self) -> Connection {
        Connection {
            peer_pid: None,
            hello_done: true,
            subscribed: false,
            preauthenticated: true,
            identity: Some((
                self.service.identity().generation(),
                PeerIdentity::granted(Some(HOST_TOOL_APP), &[CAP_STATE, CAP_CONTROL]),
            )),
        }
    }

    /// Resolves a client verb to a protocol request; `Err` names why not.
    fn to_request(&self, command: &ClientCommand) -> Result<Request, String> {
        Ok(match command {
            ClientCommand::Scan {} => Request::Scan {},
            ClientCommand::Connect { ssid, passphrase } => {
                let wanted = Ssid::from(ssid.as_str());
                let state = self.service.state();
                let ap: &ApId = state
                    .access_points
                    .iter()
                    .find(|ap| ap.ssid == wanted)
                    .map(|ap| &ap.id)
                    .ok_or_else(|| format!("no visible access point named {ssid}"))?;
                Request::Connect {
                    ap: ap.clone(),
                    passphrase: passphrase.clone(),
                }
            }
            ClientCommand::DisconnectRequest {} => Request::Disconnect {},
            ClientCommand::Forget { ssid } => {
                let wanted = Ssid::from(ssid.as_str());
                let state = self.service.state();
                let profile: &ProfileId = state
                    .profiles
                    .iter()
                    .find(|p| p.ssid == wanted)
                    .map(|p| &p.id)
                    .ok_or_else(|| format!("no saved profile named {ssid}"))?;
                Request::Forget {
                    profile: profile.clone(),
                }
            }
            ClientCommand::SetRadioEnabled { enabled } => Request::SetRadio { enabled: *enabled },
        })
    }

    /// The mock's own evidence-safe state, so acks reflect the mutation
    /// synchronously (the daemon's copy trails by one pump step).
    fn backend_state(&self) -> Value {
        match self.backend.handle(r#"{"cmd":"state"}"#) {
            Ack::Ok { state, .. } => state,
            Ack::Error { .. } => Value::Null,
        }
    }

    fn client(&self, name: &str, line: &str) -> Ack {
        let command: ClientCommand = match serde_json::from_str(line) {
            Ok(command) => command,
            Err(error) => {
                return Ack::error(Some(name), format!("invalid client verb: {error}"));
            }
        };
        let request = match self.to_request(&command) {
            Ok(request) => request,
            Err(reason) => return Ack::error(Some(name), reason),
        };
        let mut connection = self.host_tool_connection();
        match dispatch(&self.service, &mut connection, request) {
            Outcome::Reply(Reply::Ok) => Ack::ok(name, Vec::new(), self.backend_state()),
            Outcome::Reply(Reply::Error { error }) => {
                Ack::error(Some(name), format!("{} ({error})", error.name()))
            }
            Outcome::Reply(Reply::ProtocolError { reason }) => Ack::error(Some(name), reason),
            Outcome::Reply(other) => Ack::error(Some(name), format!("unexpected reply {other:?}")),
            Outcome::Subscribe => Ack::error(Some(name), "subscribe is not a client verb"),
        }
    }
}

impl Handler for MockControl {
    fn handle(&self, line: &str) -> Ack {
        let name = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("cmd").and_then(Value::as_str).map(str::to_owned));
        let ack = match name.as_deref() {
            Some(name) if ClientCommand::is_client_verb(name) => self.client(name, line),
            // Backend mutations and unknown commands: the adapter's handler
            // parses strictly and rejects what it does not know.
            _ => self.backend.handle(line),
        };
        if matches!(ack, Ack::Ok { .. }) && !self.service.settle(Duration::from_secs(2)) {
            return Ack::error(name.as_deref(), "network event barrier timed out");
        }
        ack
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{DenyAll, Identity};
    use crate::records::{Recorder, captured_events};
    use crate::testing::FakeBackend;
    use ferrix_network_api::{
        AccessPoint, AdapterKind, AdapterPresence, NetworkState, RadioState, SavedProfile,
        SecurityType,
    };
    use std::sync::Mutex;

    /// Stands in for the adapter's handler: records lines, acks everything
    /// whose `cmd` is known to it.
    struct Forwarded(Mutex<Vec<String>>);

    impl Handler for Forwarded {
        fn handle(&self, line: &str) -> Ack {
            self.0.lock().unwrap().push(line.to_owned());
            let cmd = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|v| v["cmd"].as_str().map(str::to_owned));
            match cmd.as_deref() {
                Some("state") => Ack::ok("state", vec![], serde_json::json!({"clock_ms": 7})),
                Some("tick") => Ack::ok("tick", vec!["scan-changed".into()], Value::Null),
                Some(other) => Ack::error(Some(other), "unknown backend command"),
                None => Ack::error(None, "missing cmd"),
            }
        }
    }

    fn state() -> NetworkState {
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
                frequency_mhz: None,
            }],
            profiles: vec![SavedProfile {
                id: ProfileId::new("profile-1"),
                ssid: Ssid::from("ferrix-lab"),
                security: SecurityType::Wpa2Psk,
                autoconnect: true,
            }],
            ..NetworkState::default()
        }
    }

    fn harness() -> (
        Arc<MockControl>,
        FakeBackend,
        Arc<Forwarded>,
        crate::records::Captured,
    ) {
        let backend = FakeBackend::new(state());
        let (recorder, captured) = Recorder::capturing("mock");
        // DenyAll: proves the client verbs carry their own identity.
        let service = Service::new(
            Arc::new(backend.clone()),
            Arc::new(DenyAll) as Arc<dyn Identity>,
            recorder,
        )
        .unwrap();
        service.start();
        let forwarded = Arc::new(Forwarded(Mutex::new(Vec::new())));
        let control = MockControl::new(service, forwarded.clone());
        (control, backend, forwarded, captured)
    }

    #[test]
    fn client_verbs_go_through_the_request_path_with_a_host_identity() {
        let (control, backend, forwarded, captured) = harness();
        let ack = control.handle(r#"{"cmd":"scan"}"#);
        let Ack::Ok { ack, state, .. } = ack else {
            panic!("scan must ack ok: {ack:?}");
        };
        assert_eq!(ack, "scan");
        assert_eq!(state["clock_ms"], 7, "ack state comes from the adapter");
        assert!(
            control
                .handle(r#"{"cmd":"connect","ssid":"ferrix-lab","passphrase":"mock-passphrase"}"#)
                .is_ok()
        );
        assert!(
            control
                .handle(r#"{"cmd":"set-radio-enabled","enabled":false}"#)
                .is_ok()
        );
        assert!(control.handle(r#"{"cmd":"disconnect-request"}"#).is_ok());
        assert!(
            control
                .handle(r#"{"cmd":"forget","ssid":"ferrix-lab"}"#)
                .is_ok()
        );
        let calls: Vec<&str> = backend.calls().iter().map(|c| c.op).collect::<Vec<_>>();
        assert_eq!(
            calls,
            ["scan", "connect", "set-radio:off", "disconnect", "forget"]
        );
        assert_eq!(backend.calls()[1].id.as_deref(), Some("ap-1"));
        assert!(backend.calls()[1].passphrase_present);
        assert_eq!(backend.calls()[4].id.as_deref(), Some("profile-1"));
        // Only the `state` lookups reached the adapter's handler.
        assert!(
            forwarded
                .0
                .lock()
                .unwrap()
                .iter()
                .all(|l| l == r#"{"cmd":"state"}"#)
        );
        assert!(
            !captured_events(&captured)
                .iter()
                .any(|e| e == "networkd-request-denied")
        );
    }

    #[test]
    fn backend_mutations_and_unknown_commands_are_forwarded() {
        let (control, backend, forwarded, _) = harness();
        let ack = control.handle(r#"{"cmd":"tick","ms":500}"#);
        let Ack::Ok { ack, events, .. } = ack else {
            panic!("tick must be forwarded");
        };
        assert_eq!(ack, "tick");
        assert_eq!(events, ["scan-changed"]);
        assert!(!control.handle(r#"{"cmd":"reboot"}"#).is_ok());
        assert!(!control.handle("garbage").is_ok());
        assert!(!control.handle(r#"{"nope":1}"#).is_ok());
        assert_eq!(forwarded.0.lock().unwrap().len(), 4);
        assert!(backend.calls().is_empty());
    }

    #[test]
    fn client_verbs_reject_unknown_fields_missing_targets_and_typed_errors() {
        let (control, backend, _, _) = harness();
        for line in [
            r#"{"cmd":"scan","x":1}"#,
            r#"{"cmd":"connect"}"#,
            r#"{"cmd":"connect","ssid":"ferrix-lab","ap":"ap-1"}"#,
            r#"{"cmd":"connect","ssid":"ferrix-nowhere"}"#,
            r#"{"cmd":"forget","ssid":"ferrix-guest"}"#,
            r#"{"cmd":"set-radio-enabled"}"#,
        ] {
            let ack = control.handle(line);
            assert!(!ack.is_ok(), "{line}");
        }
        assert!(
            backend.calls().is_empty(),
            "rejected verbs never reach the backend"
        );
        backend.fail_next(ferrix_network_adapter_api::BackendError::Network(
            ferrix_network_api::NetworkError::RadioDisabled,
        ));
        let Ack::Error { ack, reason } = control.handle(r#"{"cmd":"scan"}"#) else {
            panic!("typed backend error must be an error ack");
        };
        assert_eq!(ack.as_deref(), Some("scan"));
        assert!(reason.starts_with("radio-disabled"), "{reason}");
    }

    #[test]
    fn verb_names_are_exact() {
        for name in ClientCommand::NAMES {
            assert!(ClientCommand::is_client_verb(name));
        }
        assert!(!ClientCommand::is_client_verb("set-radio"));
        assert!(!ClientCommand::is_client_verb("disconnect"));
        let debug = format!(
            "{:?}",
            ClientCommand::Connect {
                ssid: "ferrix-lab".into(),
                passphrase: Some(Passphrase::new("mock-passphrase")),
            }
        );
        assert!(!debug.contains("mock-passphrase"));
    }
}
