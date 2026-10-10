//! End-to-end over a real Unix socket with the `ferrix_app_protocol::Client`: hello,
//! catalog from the repository `apps/` directory, typed errors, role gating,
//! and pushed notifications. Runs on the host; the runtime is the fake.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use ferrix_app_api::{
    AppCapability, AppError, AppEvent, AppId, AppIdentity, ExitReason, LifecycleState, TrustLevel,
};
use ferrix_app_protocol::{Client, ClientRole, Notification, Reply, Report, Request};
use ferrix_appd::records::Recorder;
use ferrix_appd::testing::{FakeProcessTree, FakeRuntime};
use ferrix_appd::{Catalog, Server, Service, ServiceOptions, load_grants};

const TIMEOUT: Duration = Duration::from_secs(5);

struct Fixture {
    socket: PathBuf,
    runtime: FakeRuntime,
    tree: FakeProcessTree,
    service: Arc<Service>,
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("ferrix-appd-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("ferrix-appd.sock");
    let apps = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
    let runtime = FakeRuntime::native();
    let tree = FakeProcessTree::default();
    let (recorder, _records) = Recorder::capturing("mock");
    // The repository grants policy beside the descriptors.
    let (grants, _source) = load_grants(None, std::slice::from_ref(&apps)).unwrap();
    let service = Arc::new(Service::new(
        Catalog::load_dir(&apps).unwrap(),
        vec![Box::new(runtime.clone())],
        Box::new(tree.clone()),
        recorder,
        ServiceOptions {
            socket: socket.clone(),
            poll_interval: Duration::from_millis(1),
            grants,
            ..ServiceOptions::default()
        },
    ));
    let listener = Server::bind(&socket).unwrap();
    let server = Arc::new(Server::new(Arc::clone(&service)));
    thread::spawn(move || server.serve(listener));
    Fixture {
        socket,
        runtime,
        tree,
        service,
    }
}

fn next_app_event(rx: &Receiver<Notification>) -> AppEvent {
    loop {
        match rx.recv_timeout(TIMEOUT).expect("notification") {
            Notification::App { event } => return event,
            Notification::Activate { .. }
            | Notification::Withhold { .. }
            | Notification::Close { .. }
            | Notification::Intent { .. } => continue,
        }
    }
}

fn id(text: &str) -> AppId {
    AppId::new(text).unwrap()
}

#[test]
fn hello_catalog_and_typed_errors_over_the_socket() {
    let f = fixture("catalog");
    let mut home = Client::connect(&f.socket, ClientRole::Home, TIMEOUT).unwrap();
    let apps = home.catalog().unwrap();
    assert!(apps.len() >= 2);
    assert!(apps.iter().any(|d| d.id.as_str() == "org.ferrix.home"));
    assert!(apps.iter().any(|d| d.id.as_str() == "org.ferrix.terminal"));
    assert!(home.instances().unwrap().is_empty());
    // First-party apps never carry a bitmap icon; the request is public.
    assert_eq!(home.app_icon(&id("org.ferrix.terminal")), Ok(None));
    assert_eq!(home.app_icon(&id("apk.org.example.absent")), Ok(None));

    // A reference entry is not launchable: typed error, not a protocol error.
    // Which entries are illustrative changes as apps land, so pick one from
    // the catalog instead of naming it.
    let reference = apps
        .iter()
        .find(|d| !d.launchable)
        .map(|d| d.id.clone())
        .expect("a non-launchable reference entry in apps/");
    assert_eq!(
        home.launch(&reference),
        Err(Ok(AppError::NotLaunchable {
            id: reference.clone()
        }))
    );
    assert_eq!(
        home.launch(&id("org.ferrix.nope")),
        Err(Ok(AppError::UnknownApp {
            id: id("org.ferrix.nope")
        }))
    );

    // Malformed lines get a protocol error and the connection survives.
    assert!(matches!(
        raw_reply(&f.socket, &serde_json::json!({"bogus": 1})),
        Reply::ProtocolError { .. }
    ));
    assert!(home.catalog().is_ok());
}

/// Sends one arbitrary JSON line on a fresh connection and reads the reply.
fn raw_reply(socket: &Path, value: &serde_json::Value) -> Reply {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    let mut text = serde_json::to_string(value).unwrap();
    text.push('\n');
    stream.write_all(text.as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

#[test]
fn compositor_role_is_gated_and_associates_surfaces() {
    let f = fixture("gating");
    let mut compositor = Client::connect(&f.socket, ClientRole::Compositor, TIMEOUT).unwrap();
    let reply = compositor
        .request(&Request::Launch {
            id: id("org.ferrix.terminal"),
        })
        .unwrap();
    assert!(matches!(reply, Reply::ProtocolError { reason } if reason.contains("compositor")));
    assert_eq!(f.runtime.launch_count(), 0);

    let mut tool = Client::connect(&f.socket, ClientRole::Tool, TIMEOUT).unwrap();
    let (instance, existing) = tool.launch(&id("org.ferrix.terminal")).unwrap();
    assert!(!existing);
    let sandbox_pid = instance.pid.unwrap();
    f.tree.set_parent(sandbox_pid + 2, sandbox_pid + 1);
    f.tree.set_parent(sandbox_pid + 1, sandbox_pid);

    // A home-role client may not report surfaces; nothing changes.
    let mut home = Client::connect(&f.socket, ClientRole::Home, TIMEOUT).unwrap();
    home.report(&Report::SurfaceMapped {
        pid: sandbox_pid + 2,
        app_id: Some("org.ferrix.terminal".to_owned()),
        title: None,
        surface: None,
    })
    .unwrap();
    // Reports are fire-and-forget; a request afterwards proves the stream is fine.
    assert!(home.instances().unwrap()[0].state == LifecycleState::Starting);
    assert!(
        !f.service
            .instance(&id("org.ferrix.terminal"))
            .unwrap()
            .surface_mapped
    );

    compositor
        .report(&Report::SurfaceMapped {
            pid: sandbox_pid + 2,
            app_id: Some("org.ferrix.terminal".to_owned()),
            title: Some("Terminal".to_owned()),
            surface: None,
        })
        .unwrap();
    compositor
        .report(&Report::FocusChanged {
            pid: Some(sandbox_pid + 2),
            surface: None,
        })
        .unwrap();
    assert!(ferrix_appd::testing::wait_for(TIMEOUT, || {
        f.service
            .instance(&id("org.ferrix.terminal"))
            .is_some_and(|i| i.surface_mapped && i.state == LifecycleState::Foreground)
    }));
}

/// A `service`-role client resolves the pid of a launched application's
/// grandchild (what a daemon reads from `SO_PEERCRED`) to its identity and
/// effective grants; unknown pids and privileged verbs are refused or empty.
#[test]
fn service_role_identifies_launched_pids_over_the_socket() {
    let f = fixture("identify");
    let mut tool = Client::connect(&f.socket, ClientRole::Tool, TIMEOUT).unwrap();
    let terminal = id("org.ferrix.terminal");
    let (instance, _) = tool.launch(&terminal).unwrap();
    let sandbox_pid = instance.pid.unwrap();
    // ferrix-appd -> sandbox -> init -> app: the daemon sees the app pid.
    let app_pid = sandbox_pid + 2;
    f.tree.set_parent(app_pid, sandbox_pid + 1);
    f.tree.set_parent(sandbox_pid + 1, sandbox_pid);

    let mut service = Client::connect(&f.socket, ClientRole::Service, TIMEOUT).unwrap();
    let identity = service.identify(app_pid).unwrap();
    assert_eq!(identity.app, Some(terminal.clone()));
    assert_eq!(identity.trust, Some(TrustLevel::Untrusted));
    assert_eq!(
        identity.granted,
        [AppCapability::Shell, AppCapability::Background]
            .into_iter()
            .collect()
    );
    assert!(!identity.has(AppCapability::TelephonyCall));

    // The literal wire shape daemons are coded against.
    let raw = raw_line(
        &f.socket,
        &[
            serde_json::json!({"request": "hello", "role": "service"}),
            serde_json::json!({"request": "identify", "pid": app_pid}),
        ],
    );
    assert_eq!(
        raw,
        r#"{"reply":"identity","schema_version":1,"app":"org.ferrix.terminal","trust":"untrusted","granted":["shell","background"]}"#
    );

    // Unknown pid: `app: null`, no trust, no grants.
    assert_eq!(service.identify(999_999).unwrap(), AppIdentity::unknown());
    let raw = raw_line(
        &f.socket,
        &[
            serde_json::json!({"request": "hello", "role": "service"}),
            serde_json::json!({"request": "identify", "pid": 999_999}),
        ],
    );
    assert_eq!(
        raw,
        r#"{"reply":"identity","schema_version":1,"app":null,"trust":null,"granted":[]}"#
    );

    // A service may not launch; an application may not identify.
    assert!(matches!(
        service.request(&Request::Launch { id: terminal.clone() }).unwrap(),
        Reply::ProtocolError { reason } if reason.contains("service")
    ));
    assert_eq!(f.runtime.launch_count(), 1);
    let mut app = Client::connect(&f.socket, ClientRole::Application, TIMEOUT).unwrap();
    assert!(app.identify(app_pid).is_err());

    // After a crash the same pid resolves to nothing.
    assert!(
        f.runtime
            .exit(&terminal, ExitReason::Signaled { signal: 11 })
    );
    assert!(ferrix_appd::testing::wait_for(TIMEOUT, || {
        f.service
            .instance(&terminal)
            .is_some_and(|i| i.state == LifecycleState::Crashed)
    }));
    assert_eq!(service.identify(app_pid).unwrap(), AppIdentity::unknown());
    assert_eq!(
        service.identify(sandbox_pid).unwrap(),
        AppIdentity::unknown()
    );
}

/// Sends `lines` on a fresh connection and returns the last reply verbatim.
fn raw_line(socket: &Path, lines: &[serde_json::Value]) -> String {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut last = String::new();
    for value in lines {
        let mut text = serde_json::to_string(value).unwrap();
        text.push('\n');
        stream.write_all(text.as_bytes()).unwrap();
        last.clear();
        reader.read_line(&mut last).unwrap();
    }
    last.trim().to_owned()
}

#[test]
fn subscribers_receive_catalog_and_lifecycle_notifications() {
    let f = fixture("subscribe");
    let subscription = Client::connect(&f.socket, ClientRole::Home, TIMEOUT)
        .unwrap()
        .subscribe()
        .unwrap();
    assert!(subscription.apps.len() >= 2);
    let (rx, _reporter) = subscription.spawn(Arc::new(|| {}));

    f.service.announce_catalog();
    assert_eq!(
        next_app_event(&rx),
        AppEvent::CatalogChanged {
            apps: f.service.catalog().len()
        }
    );

    let mut tool = Client::connect(&f.socket, ClientRole::Tool, TIMEOUT).unwrap();
    let terminal = id("org.ferrix.terminal");
    let (instance, _) = tool.launch(&terminal).unwrap();
    match next_app_event(&rx) {
        AppEvent::Lifecycle { instance: seen } => {
            assert_eq!(seen.app, terminal);
            assert_eq!(seen.instance, instance.instance);
            assert_eq!(seen.state, LifecycleState::Starting);
        }
        other => panic!("unexpected {other:?}"),
    }

    // A compositor subscriber gets the mapped Wayland client pid, not the
    // sandbox supervisor pid, in the Activate directive on a second launch.
    let client_pid = instance.pid.unwrap() + 2;
    f.tree.set_parent(client_pid, instance.pid.unwrap() + 1);
    f.tree
        .set_parent(instance.pid.unwrap() + 1, instance.pid.unwrap());
    let mut compositor = Client::connect(&f.socket, ClientRole::Compositor, TIMEOUT).unwrap();
    compositor
        .report(&Report::SurfaceMapped {
            pid: client_pid,
            app_id: Some("org.ferrix.terminal".to_owned()),
            title: Some("Terminal".to_owned()),
            surface: None,
        })
        .unwrap();
    let compositor = compositor.subscribe().unwrap();
    let (compositor_rx, _reporter) = compositor.spawn(Arc::new(|| {}));
    let (_, existing) = tool.launch(&terminal).unwrap();
    assert!(existing);
    assert_eq!(
        compositor_rx.recv_timeout(TIMEOUT).unwrap(),
        Notification::Activate {
            pid: client_pid,
            app_id: terminal.clone(),
            surface: None,
        }
    );

    // Crash: the home subscriber learns the final state and exit reason.
    assert!(
        f.runtime
            .exit(&terminal, ExitReason::Signaled { signal: 11 })
    );
    let crashed = loop {
        match next_app_event(&rx) {
            AppEvent::Lifecycle { instance } if instance.state == LifecycleState::Crashed => {
                break instance;
            }
            _ => continue,
        }
    };
    assert_eq!(crashed.exit, Some(ExitReason::Signaled { signal: 11 }));
    assert_eq!(crashed.instance, instance.instance);
}
