//! Unix-socket front end: one thread per connection, line-delimited JSON,
//! `ferrix_app_protocol::Inbound` parsing, role gating, and subscription hand-off.
//!
//! Mirrors `ferrix-core`'s server. The dispatch step is a pure function over
//! a [`Connection`] so gating is unit-tested without sockets.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use ferrix_app_api::{IntentKind, SCHEMA_VERSION};
use ferrix_app_protocol::{ClientRole, Inbound, Reply, Report, Request};

use crate::service::Service;

/// Per-connection protocol state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    pub role: Option<ClientRole>,
    pub subscribed: bool,
    /// Kernel-reported peer pid when available (Linux/QEMU).
    pub peer_pid: Option<u32>,
    /// Pids this connection already identified; the `appd-identify` record
    /// is emitted once per pid per connection.
    pub identified: BTreeSet<u32>,
}

/// What the connection thread does with a parsed line.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Write this reply.
    Reply(Reply),
    /// Register the stream as a subscriber (the service writes the `Ok`).
    Subscribe,
    /// Reports never get a reply.
    Silent,
}

pub fn role_name(role: ClientRole) -> &'static str {
    match role {
        ClientRole::Compositor => "compositor",
        ClientRole::Home => "home",
        ClientRole::Application => "application",
        ClientRole::Tool => "tool",
        ClientRole::Service => "service",
    }
}

pub fn message_name(inbound: &Inbound) -> &'static str {
    match inbound {
        Inbound::Request(Request::Hello { .. }) => "hello",
        Inbound::Request(Request::Identify { .. }) => "identify",
        Inbound::Request(Request::Catalog) => "catalog",
        Inbound::Request(Request::Instances) => "instances",
        Inbound::Request(Request::Launch { .. }) => "launch",
        Inbound::Request(Request::Activate { .. }) => "activate",
        Inbound::Request(Request::Terminate { .. }) => "terminate",
        Inbound::Request(Request::Home) => "home",
        Inbound::Request(Request::Subscribe) => "subscribe",
        Inbound::Request(Request::Open { .. }) => "open",
        Inbound::Report(Report::SurfaceMapped { .. }) => "surface-mapped",
        Inbound::Report(Report::SurfaceUnmapped { .. }) => "surface-unmapped",
        Inbound::Report(Report::FocusChanged { .. }) => "focus-changed",
        Inbound::Report(Report::HomeGesture) => "home-gesture",
        Inbound::Report(Report::CloseGesture) => "close-gesture",
        Inbound::Report(Report::YieldForeground) => "yield-foreground",
    }
}

/// The trust-ring table: which role may send which message (after hello).
///
/// | message | roles |
/// |---|---|
/// | `Catalog`, `Instances`, `Subscribe` | any |
/// | `Home` | `compositor`, `home`, `application`, `tool` |
/// | `Identify` | `service`, `tool` |
/// | `Launch`, `Activate`, `Terminate` | `home`, `tool` |
/// | `Open` (`dial`, `compose`) | `application`, `home`, `tool` |
/// | `Open` (`show-call`) | `service`, `tool` |
/// | surface/focus/gesture reports | `compositor` |
/// | `YieldForeground` | `application`, `home` |
///
/// A `service` resolves identities and observes; the only thing it may
/// raise is the handler of a system intent (`show-call`), never a named app.
pub fn allowed(role: ClientRole, inbound: &Inbound) -> bool {
    use ClientRole::*;
    match inbound {
        Inbound::Request(Request::Hello { .. }) => true,
        Inbound::Request(Request::Catalog | Request::Instances | Request::Subscribe) => true,
        Inbound::Request(Request::Home) => role != Service,
        Inbound::Request(Request::Identify { .. }) => matches!(role, Service | Tool),
        Inbound::Request(
            Request::Launch { .. } | Request::Activate { .. } | Request::Terminate { .. },
        ) => {
            matches!(role, Home | Tool)
        }
        Inbound::Report(
            Report::SurfaceMapped { .. }
            | Report::SurfaceUnmapped { .. }
            | Report::FocusChanged { .. }
            | Report::HomeGesture
            | Report::CloseGesture,
        ) => role == Compositor,
        Inbound::Report(Report::YieldForeground) => matches!(role, Application | Home),
        Inbound::Request(Request::Open { intent }) => match intent.kind() {
            IntentKind::Dial | IntentKind::Compose => matches!(role, Application | Home | Tool),
            IntentKind::ShowCall => matches!(role, Service | Tool),
        },
    }
}

/// Applies one parsed line. Hello must come first; every other message is
/// gated by [`allowed`]. Rejected requests get a `ProtocolError`; rejected
/// reports are dropped with an `ferrix_appd-request-rejected` record.
pub fn dispatch(service: &Arc<Service>, connection: &mut Connection, inbound: Inbound) -> Outcome {
    let name = message_name(&inbound);
    if let Inbound::Request(Request::Hello { role }) = inbound {
        if connection.role.is_some() {
            return protocol_error("hello was already sent on this connection");
        }
        if let Some(pid) = connection.peer_pid
            && let Some(actual) = service.client_role_for_pid(pid)
            && actual != role
        {
            let reason = format!(
                "peer pid {pid} is {}, not {}",
                role_name(actual),
                role_name(role)
            );
            service
                .recorder()
                .request_rejected(Some(role_name(role)), name, &reason);
            return protocol_error(&reason);
        }
        connection.role = Some(role);
        return Outcome::Reply(service.hello_reply());
    }
    let Some(role) = connection.role else {
        service
            .recorder()
            .request_rejected(None, name, "hello must come first");
        return match inbound {
            Inbound::Request(_) => protocol_error("hello must come first"),
            Inbound::Report(_) => Outcome::Silent,
        };
    };
    if !allowed(role, &inbound) {
        let reason = format!("role {} may not send {name}", role_name(role));
        service
            .recorder()
            .request_rejected(Some(role_name(role)), name, &reason);
        return match inbound {
            Inbound::Request(_) => protocol_error(&reason),
            Inbound::Report(_) => Outcome::Silent,
        };
    }
    match inbound {
        Inbound::Request(request) => handle_request(service, connection, request),
        Inbound::Report(report) => {
            handle_report(service, report);
            Outcome::Silent
        }
    }
}

fn handle_request(
    service: &Arc<Service>,
    connection: &mut Connection,
    request: Request,
) -> Outcome {
    let reply = match request {
        Request::Hello { .. } => unreachable!("handled by dispatch"),
        Request::Identify { pid } => {
            let identity = service.identify(pid);
            if connection.identified.insert(pid) {
                service.recorder().appd_identify(
                    pid,
                    identity.app.as_ref(),
                    identity.granted.len(),
                );
            }
            Reply::Identity {
                schema_version: SCHEMA_VERSION,
                identity,
            }
        }
        Request::Catalog => Reply::Catalog {
            schema_version: SCHEMA_VERSION,
            apps: service.catalog().descriptors(),
        },
        Request::Instances => Reply::Instances {
            schema_version: SCHEMA_VERSION,
            instances: service.instances(),
        },
        Request::Launch { id } => match service.launch(&id) {
            Ok(outcome) => Reply::Instance {
                schema_version: SCHEMA_VERSION,
                instance: outcome.instance,
                activated_existing: outcome.activated_existing,
            },
            Err(error) => Reply::Error { error },
        },
        Request::Activate { id } => match service.activate(&id) {
            Ok(instance) => Reply::Instance {
                schema_version: SCHEMA_VERSION,
                instance,
                activated_existing: true,
            },
            Err(error) => Reply::Error { error },
        },
        Request::Terminate { id } => match service.terminate(&id) {
            Ok(instance) => Reply::Instance {
                schema_version: SCHEMA_VERSION,
                instance,
                activated_existing: false,
            },
            Err(error) => Reply::Error { error },
        },
        Request::Home => match service.home() {
            Ok(outcome) => Reply::Instance {
                schema_version: SCHEMA_VERSION,
                instance: outcome.instance,
                activated_existing: outcome.activated_existing,
            },
            Err(error) => Reply::Error { error },
        },
        Request::Open { intent } => match service.open(&intent) {
            Ok(outcome) => Reply::Instance {
                schema_version: SCHEMA_VERSION,
                instance: outcome.instance,
                activated_existing: outcome.activated_existing,
            },
            Err(error) => Reply::Error { error },
        },
        Request::Subscribe => {
            if connection.subscribed {
                return protocol_error("already subscribed");
            }
            connection.subscribed = true;
            return Outcome::Subscribe;
        }
    };
    Outcome::Reply(reply)
}

fn handle_report(service: &Arc<Service>, report: Report) {
    match report {
        Report::SurfaceMapped { pid, app_id, title } => {
            service.surface_mapped(pid, app_id.as_deref(), title.as_deref());
        }
        Report::SurfaceUnmapped { pid } => {
            service.surface_unmapped(pid);
        }
        Report::FocusChanged { pid } => {
            service.focus_changed(pid);
        }
        Report::HomeGesture | Report::YieldForeground => {
            // Activation is best effort: a failure here (no home in the
            // catalog) is already visible through launch records.
            let _ = service.home();
        }
        Report::CloseGesture => {
            let _ = service.terminate_foreground();
        }
    }
}

fn protocol_error(reason: &str) -> Outcome {
    Outcome::Reply(Reply::ProtocolError {
        reason: reason.to_owned(),
    })
}

pub struct Server {
    service: Arc<Service>,
}

impl Server {
    pub fn new(service: Arc<Service>) -> Self {
        Self { service }
    }

    /// Binds the socket. Nothing is reported as ready before this succeeds.
    pub fn bind(path: &Path) -> Result<UnixListener, String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("cannot remove stale {}: {e}", path.display()))?;
        }
        UnixListener::bind(path).map_err(|e| format!("cannot bind {}: {e}", path.display()))
    }

    /// Accept loop; one thread per connection. Returns only if the listener
    /// breaks.
    pub fn serve(self: Arc<Self>, listener: UnixListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let server = Arc::clone(&self);
                    let spawned = thread::Builder::new()
                        .name("ferrix-appd-conn".to_owned())
                        .spawn(move || server.handle(stream));
                    if let Err(error) = spawned {
                        self.service
                            .recorder()
                            .appd_socket_error(&format!("cannot spawn connection thread: {error}"));
                    }
                }
                Err(error) => self
                    .service
                    .recorder()
                    .appd_socket_error(&error.to_string()),
            }
        }
    }

    fn handle(&self, stream: UnixStream) {
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut connection = Connection {
            peer_pid: peer_pid(&stream),
            ..Connection::default()
        };
        let mut writer = stream;
        let mut lines = BufReader::new(read_half).lines();
        while let Some(Ok(line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let outcome = match serde_json::from_str::<Inbound>(line) {
                Ok(inbound) => dispatch(&self.service, &mut connection, inbound),
                Err(error) => protocol_error(&format!("invalid message: {error}")),
            };
            match outcome {
                Outcome::Reply(reply) => {
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                }
                Outcome::Subscribe => {
                    let role = connection.role.unwrap_or(ClientRole::Application);
                    let Ok(sink) = writer.try_clone() else {
                        return;
                    };
                    if self
                        .service
                        .subscribe_peer(role, connection.peer_pid, Box::new(sink))
                        .is_err()
                    {
                        return;
                    }
                }
                Outcome::Silent => {}
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    rustix::net::sockopt::socket_peercred(stream)
        .ok()
        .and_then(|credentials| u32::try_from(credentials.pid.as_raw_pid()).ok())
}

#[cfg(not(target_os = "linux"))]
fn peer_pid(_stream: &UnixStream) -> Option<u32> {
    None
}

fn write_json(stream: &mut UnixStream, value: &impl serde::Serialize) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(std::io::Error::other)?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::records::{Recorder, captured_events};
    use crate::service::ServiceOptions;
    use crate::testing::{FakeProcessTree, FakeRuntime};
    use ferrix_app_api::{AppId, LifecycleState};

    /// The repository catalog and the repository grants policy.
    fn service() -> (Arc<Service>, FakeRuntime, crate::records::Captured) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let runtime = FakeRuntime::native();
        let (recorder, records) = Recorder::capturing("mock");
        let (grants, _) = crate::grants::load_grants(None, std::slice::from_ref(&dir)).unwrap();
        let service = Service::new(
            Catalog::load_dir(&dir).unwrap(),
            vec![Box::new(runtime.clone())],
            Box::new(FakeProcessTree::default()),
            recorder,
            ServiceOptions {
                poll_interval: std::time::Duration::from_millis(1),
                grants,
                ..ServiceOptions::default()
            },
        );
        (Arc::new(service), runtime, records)
    }

    fn terminal() -> AppId {
        AppId::new("org.ferrix.terminal").unwrap()
    }

    #[test]
    fn open_is_gated_by_intent_kind() {
        use ferrix_app_api::AppIntent;
        let dial = Inbound::Request(Request::Open {
            intent: AppIntent::Dial {
                number: "5550100".to_owned(),
            },
        });
        let show = Inbound::Request(Request::Open {
            intent: AppIntent::ShowCall,
        });
        for (role, dial_ok, show_ok) in [
            (ClientRole::Application, true, false),
            (ClientRole::Home, true, false),
            (ClientRole::Tool, true, true),
            (ClientRole::Service, false, true),
            (ClientRole::Compositor, false, false),
        ] {
            assert_eq!(allowed(role, &dial), dial_ok, "{role:?} dial");
            assert_eq!(allowed(role, &show), show_ok, "{role:?} show-call");
        }
        assert_eq!(message_name(&show), "open");
    }

    #[test]
    fn service_show_call_launches_the_repository_phone() {
        let (service, runtime, records) = service();
        let mut conn = Connection::default();
        dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Hello {
                role: ClientRole::Service,
            }),
        );
        let reply = dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Open {
                intent: ferrix_app_api::AppIntent::ShowCall,
            }),
        );
        let phone = AppId::new("org.ferrix.phone").unwrap();
        match reply {
            Outcome::Reply(Reply::Instance { instance, .. }) => assert_eq!(instance.app, phone),
            other => panic!("{other:?}"),
        }
        assert!(runtime.last_pid(&phone).is_some());
        assert!(captured_events(&records).contains(&"app-intent".to_owned()));
        // A service still may not launch by name.
        let refused = dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Launch { id: terminal() }),
        );
        assert!(matches!(
            refused,
            Outcome::Reply(Reply::ProtocolError { .. })
        ));
    }

    #[test]
    fn hello_must_come_first_and_only_once() {
        let (service, _, _) = service();
        let mut conn = Connection::default();
        assert_eq!(
            dispatch(&service, &mut conn, Inbound::Request(Request::Catalog)),
            Outcome::Reply(Reply::ProtocolError {
                reason: "hello must come first".to_owned()
            })
        );
        assert_eq!(
            dispatch(&service, &mut conn, Inbound::Report(Report::HomeGesture)),
            Outcome::Silent
        );
        let hello = dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Hello {
                role: ClientRole::Tool,
            }),
        );
        assert!(matches!(
            hello,
            Outcome::Reply(Reply::Hello {
                schema_version: SCHEMA_VERSION,
                ..
            })
        ));
        assert_eq!(conn.role, Some(ClientRole::Tool));
        assert!(matches!(
            dispatch(
                &service,
                &mut conn,
                Inbound::Request(Request::Hello {
                    role: ClientRole::Compositor
                })
            ),
            Outcome::Reply(Reply::ProtocolError { .. })
        ));
        assert_eq!(conn.role, Some(ClientRole::Tool));
    }

    #[test]
    fn home_role_cannot_report_surfaces() {
        let (service, _, records) = service();
        let mut conn = Connection::default();
        dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Hello {
                role: ClientRole::Home,
            }),
        );
        let pid = match dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Launch { id: terminal() }),
        ) {
            Outcome::Reply(Reply::Instance { instance, .. }) => instance.pid.unwrap(),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(
            dispatch(
                &service,
                &mut conn,
                Inbound::Report(Report::SurfaceMapped {
                    pid,
                    app_id: None,
                    title: None
                })
            ),
            Outcome::Silent
        );
        let instance = service.instance(&terminal()).unwrap();
        assert!(!instance.surface_mapped);
        assert_eq!(instance.state, LifecycleState::Starting);
        assert!(captured_events(&records).contains(&"ferrix-appd-request-rejected".to_owned()));
        assert!(!captured_events(&records).contains(&"app-surface".to_owned()));
    }

    #[test]
    fn compositor_cannot_launch_but_can_report() {
        let (service, runtime, _) = service();
        let mut conn = Connection::default();
        dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Hello {
                role: ClientRole::Compositor,
            }),
        );
        assert!(matches!(
            dispatch(
                &service,
                &mut conn,
                Inbound::Request(Request::Launch { id: terminal() })
            ),
            Outcome::Reply(Reply::ProtocolError { reason }) if reason.contains("compositor")
        ));
        assert_eq!(runtime.launch_count(), 0);
        assert_eq!(
            dispatch(
                &service,
                &mut conn,
                Inbound::Report(Report::FocusChanged { pid: None })
            ),
            Outcome::Silent
        );
        // A home gesture from the compositor launches home when it is down.
        assert_eq!(
            dispatch(&service, &mut conn, Inbound::Report(Report::HomeGesture)),
            Outcome::Silent
        );
        assert_eq!(runtime.launch_count(), 1);
        assert_eq!(runtime.launches()[0].0.as_str(), "org.ferrix.home");
    }

    #[test]
    fn applications_may_only_yield_and_read() {
        let (service, runtime, _) = service();
        let mut conn = Connection::default();
        dispatch(
            &service,
            &mut conn,
            Inbound::Request(Request::Hello {
                role: ClientRole::Application,
            }),
        );
        assert!(matches!(
            dispatch(&service, &mut conn, Inbound::Request(Request::Catalog)),
            Outcome::Reply(Reply::Catalog { .. })
        ));
        assert!(matches!(
            dispatch(
                &service,
                &mut conn,
                Inbound::Request(Request::Terminate { id: terminal() })
            ),
            Outcome::Reply(Reply::ProtocolError { .. })
        ));
        assert_eq!(
            dispatch(
                &service,
                &mut conn,
                Inbound::Report(Report::YieldForeground)
            ),
            Outcome::Silent
        );
        assert_eq!(runtime.launch_count(), 1);
        assert_eq!(
            dispatch(&service, &mut conn, Inbound::Request(Request::Subscribe)),
            Outcome::Subscribe
        );
        assert!(conn.subscribed);
        assert!(matches!(
            dispatch(&service, &mut conn, Inbound::Request(Request::Subscribe)),
            Outcome::Reply(Reply::ProtocolError { .. })
        ));
    }

    #[test]
    fn launched_app_cannot_self_assign_a_privileged_role() {
        let (service, _, records) = service();
        let instance = service.launch(&terminal()).unwrap().instance;
        let pid = instance.pid.unwrap();

        let mut forged = Connection {
            peer_pid: Some(pid),
            ..Connection::default()
        };
        assert!(matches!(
            dispatch(
                &service,
                &mut forged,
                Inbound::Request(Request::Hello {
                    role: ClientRole::Tool
                })
            ),
            Outcome::Reply(Reply::ProtocolError { reason }) if reason.contains("application, not tool")
        ));
        assert_eq!(forged.role, None);

        let mut honest = Connection {
            peer_pid: Some(pid),
            ..Connection::default()
        };
        assert!(matches!(
            dispatch(
                &service,
                &mut honest,
                Inbound::Request(Request::Hello {
                    role: ClientRole::Application
                })
            ),
            Outcome::Reply(Reply::Hello { .. })
        ));
        assert!(captured_events(&records).contains(&"ferrix-appd-request-rejected".to_owned()));
    }

    fn hello(service: &Arc<Service>, conn: &mut Connection, role: ClientRole) {
        assert!(matches!(
            dispatch(service, conn, Inbound::Request(Request::Hello { role })),
            Outcome::Reply(Reply::Hello { .. })
        ));
    }

    fn identify(service: &Arc<Service>, conn: &mut Connection, pid: u32) -> Outcome {
        dispatch(service, conn, Inbound::Request(Request::Identify { pid }))
    }

    #[test]
    fn service_role_identifies_but_never_launches_or_steers() {
        let (service, runtime, records) = service();
        let mut conn = Connection::default();
        hello(&service, &mut conn, ClientRole::Service);

        // Unknown pid: a well-formed identity with nothing in it.
        assert_eq!(
            identify(&service, &mut conn, 4242),
            Outcome::Reply(Reply::Identity {
                schema_version: SCHEMA_VERSION,
                identity: ferrix_app_api::AppIdentity::unknown(),
            })
        );

        for refused in [
            Inbound::Request(Request::Launch { id: terminal() }),
            Inbound::Request(Request::Activate { id: terminal() }),
            Inbound::Request(Request::Terminate { id: terminal() }),
            Inbound::Request(Request::Home),
        ] {
            let name = message_name(&refused);
            assert!(
                matches!(
                    dispatch(&service, &mut conn, refused),
                    Outcome::Reply(Reply::ProtocolError { reason }) if reason.contains("service")
                ),
                "{name} must be refused for a service"
            );
        }
        assert_eq!(
            dispatch(&service, &mut conn, Inbound::Report(Report::HomeGesture)),
            Outcome::Silent
        );
        assert_eq!(
            dispatch(
                &service,
                &mut conn,
                Inbound::Report(Report::YieldForeground)
            ),
            Outcome::Silent
        );
        assert_eq!(runtime.launch_count(), 0, "nothing was launched");

        // Reading and subscribing stay available to services.
        assert!(matches!(
            dispatch(&service, &mut conn, Inbound::Request(Request::Catalog)),
            Outcome::Reply(Reply::Catalog { .. })
        ));
        assert!(matches!(
            dispatch(&service, &mut conn, Inbound::Request(Request::Instances)),
            Outcome::Reply(Reply::Instances { .. })
        ));
        assert_eq!(
            dispatch(&service, &mut conn, Inbound::Request(Request::Subscribe)),
            Outcome::Subscribe
        );
        let rejected = captured_events(&records)
            .iter()
            .filter(|e| *e == "ferrix-appd-request-rejected")
            .count();
        assert_eq!(rejected, 6);
    }

    #[test]
    fn identify_resolves_running_apps_once_per_connection() {
        let (service, runtime, records) = service();
        let instance = service.launch(&terminal()).unwrap().instance;
        let sandbox_pid = instance.pid.unwrap();
        let mut conn = Connection::default();
        hello(&service, &mut conn, ClientRole::Service);

        // The repository grants give the Terminal `shell` and `background`,
        // which its descriptor also declares.
        let Outcome::Reply(Reply::Identity { identity, .. }) =
            identify(&service, &mut conn, sandbox_pid)
        else {
            panic!("expected an identity reply");
        };
        assert_eq!(identity.app, Some(terminal()));
        assert_eq!(identity.trust, Some(ferrix_app_api::TrustLevel::Untrusted));
        assert_eq!(
            identity.granted,
            [
                ferrix_app_api::AppCapability::Shell,
                ferrix_app_api::AppCapability::Background
            ]
            .into_iter()
            .collect()
        );

        // Repeats do not spam the log; a second pid is recorded once too.
        identify(&service, &mut conn, sandbox_pid);
        identify(&service, &mut conn, 4242);
        identify(&service, &mut conn, 4242);
        let identify_records: Vec<_> = records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "appd-identify")
            .cloned()
            .collect();
        assert_eq!(identify_records.len(), 2, "{identify_records:?}");
        assert_eq!(identify_records[0]["pid"], sandbox_pid);
        assert_eq!(identify_records[0]["app"], "org.ferrix.terminal");
        assert_eq!(identify_records[0]["granted_count"], 2);
        assert_eq!(identify_records[1]["pid"], 4242);
        assert!(identify_records[1]["app"].is_null());
        assert_eq!(identify_records[1]["granted_count"], 0);

        // Once the instance has crashed the identity is gone.
        assert!(runtime.exit(
            &terminal(),
            ferrix_app_api::ExitReason::Signaled { signal: 9 }
        ));
        assert!(crate::testing::wait_for(
            std::time::Duration::from_secs(5),
            || service
                .instance(&terminal())
                .is_some_and(|i| i.state == LifecycleState::Crashed)
        ));
        assert_eq!(
            identify(&service, &mut conn, sandbox_pid),
            Outcome::Reply(Reply::Identity {
                schema_version: SCHEMA_VERSION,
                identity: ferrix_app_api::AppIdentity::unknown(),
            })
        );
    }

    #[test]
    fn application_and_home_roles_cannot_identify() {
        let (service, _, records) = service();
        for role in [
            ClientRole::Application,
            ClientRole::Home,
            ClientRole::Compositor,
        ] {
            let mut conn = Connection::default();
            hello(&service, &mut conn, role);
            assert!(
                matches!(
                    identify(&service, &mut conn, 1),
                    Outcome::Reply(Reply::ProtocolError { reason }) if reason.contains(role_name(role))
                ),
                "{} must not identify",
                role_name(role)
            );
        }
        let mut tool = Connection::default();
        hello(&service, &mut tool, ClientRole::Tool);
        assert!(matches!(
            identify(&service, &mut tool, 1),
            Outcome::Reply(Reply::Identity { .. })
        ));
        assert_eq!(
            captured_events(&records)
                .iter()
                .filter(|e| *e == "ferrix-appd-request-rejected")
                .count(),
            3
        );
    }

    #[test]
    fn launched_app_cannot_hello_as_a_service() {
        let (service, _, records) = service();
        let pid = service.launch(&terminal()).unwrap().instance.pid.unwrap();
        let mut forged = Connection {
            peer_pid: Some(pid),
            ..Connection::default()
        };
        assert!(matches!(
            dispatch(
                &service,
                &mut forged,
                Inbound::Request(Request::Hello {
                    role: ClientRole::Service
                })
            ),
            Outcome::Reply(Reply::ProtocolError { reason }) if reason.contains("application, not service")
        ));
        assert_eq!(forged.role, None);
        // Nothing after a refused hello works either.
        assert!(matches!(
            identify(&service, &mut forged, pid),
            Outcome::Reply(Reply::ProtocolError { .. })
        ));
        assert!(captured_events(&records).contains(&"ferrix-appd-request-rejected".to_owned()));
    }

    #[test]
    fn gating_table_is_explicit() {
        let identify = Inbound::Request(Request::Identify { pid: 1 });
        assert!(allowed(ClientRole::Service, &identify));
        assert!(allowed(ClientRole::Tool, &identify));
        assert!(!allowed(ClientRole::Home, &identify));
        assert!(!allowed(ClientRole::Application, &identify));
        assert!(!allowed(ClientRole::Compositor, &identify));
        assert!(!allowed(
            ClientRole::Service,
            &Inbound::Request(Request::Home)
        ));
        assert!(!allowed(
            ClientRole::Service,
            &Inbound::Report(Report::YieldForeground)
        ));
        assert!(!allowed(
            ClientRole::Service,
            &Inbound::Report(Report::HomeGesture)
        ));
        assert!(allowed(
            ClientRole::Compositor,
            &Inbound::Report(Report::CloseGesture)
        ));
        assert!(!allowed(
            ClientRole::Application,
            &Inbound::Report(Report::CloseGesture)
        ));
        assert!(allowed(
            ClientRole::Service,
            &Inbound::Request(Request::Subscribe)
        ));

        let launch = Inbound::Request(Request::Launch { id: terminal() });
        let mapped = Inbound::Report(Report::SurfaceMapped {
            pid: 1,
            app_id: None,
            title: None,
        });
        let yield_fg = Inbound::Report(Report::YieldForeground);
        assert!(allowed(ClientRole::Home, &launch));
        assert!(allowed(ClientRole::Tool, &launch));
        assert!(!allowed(ClientRole::Compositor, &launch));
        assert!(!allowed(ClientRole::Application, &launch));
        assert!(allowed(ClientRole::Compositor, &mapped));
        assert!(!allowed(ClientRole::Home, &mapped));
        assert!(allowed(ClientRole::Application, &yield_fg));
        assert!(!allowed(ClientRole::Compositor, &yield_fg));
        assert!(allowed(
            ClientRole::Application,
            &Inbound::Request(Request::Home)
        ));
    }
}
