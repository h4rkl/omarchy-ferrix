//! Resilient link to the application service (`appd`).
//!
//! The compositor is a `ClientRole::Compositor` subscriber: it tells `appd`
//! which client pid mapped a surface, which pid holds focus, and when the
//! home gesture fired; `appd` answers with `Notification::Activate` directives
//! that raise a client's toplevel. `appd` may start after the compositor or
//! crash and restart, so a background thread connects with retries and
//! reconnects on loss, forever. Nothing here blocks the compositor loop: the
//! thread hands events over an `mpsc` channel and wakes the loop through the
//! caller's `wake` callback (a calloop ping), the same pattern the mobile-core
//! subscription uses.
//!
//! This module is plain std + `app-protocol`, so the connection logic is unit
//! tested on every host against a fake `appd`; the Smithay side lives in
//! `compositor::appd_link`.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use app_protocol::{Client, ClientRole, Notification, Report, Reporter};

/// How long one connection attempt waits for the socket before pausing.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Pause between failed connection attempts.
pub const RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// Something the link thread wants the compositor to know.
#[derive(Debug)]
pub enum LinkEvent {
    /// A subscription is established; reports flow from now on.
    Connected,
    /// The connection could not be (re)established; sent once per outage.
    Retrying { reason: String },
    /// The subscription ended (`appd` closed the stream).
    Lost,
    /// A pushed directive or lifecycle event.
    Notification(Notification),
}

impl LinkEvent {
    /// Value of the `status` field in `appd-link` records, when this event
    /// changes the link status.
    pub fn status(&self) -> Option<&'static str> {
        match self {
            LinkEvent::Connected => Some("connected"),
            LinkEvent::Retrying { .. } => Some("retrying"),
            LinkEvent::Lost => Some("lost"),
            LinkEvent::Notification(_) => None,
        }
    }
}

/// Compositor-side handle: drains events from the link thread and writes
/// reports while a subscription exists.
pub struct AppdLink {
    socket: PathBuf,
    events: Receiver<(LinkEvent, Option<Reporter>)>,
    reporter: Option<Reporter>,
    last_focus: Option<Option<u32>>,
}

impl AppdLink {
    /// Start the link thread towards `socket`. `wake` is called from the
    /// thread after every queued event so the compositor loop drains it.
    pub fn start(socket: &Path, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        let path = socket.to_path_buf();
        thread::Builder::new()
            .name("appd-link".to_owned())
            .spawn(move || link_thread(&path, tx, wake))
            .expect("spawn appd link thread");
        Self {
            socket: socket.to_path_buf(),
            events: rx,
            reporter: None,
            last_focus: None,
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Whether reports currently reach `appd`.
    pub fn connected(&self) -> bool {
        self.reporter.is_some()
    }

    /// Take every queued event, updating the connection state as it goes.
    pub fn drain(&mut self) -> Vec<LinkEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok((event, reporter)) => {
                    match &event {
                        LinkEvent::Connected => {
                            self.reporter = reporter;
                            // A fresh appd knows nothing; the next focus report
                            // must go out even if it repeats the last one.
                            self.last_focus = None;
                        }
                        LinkEvent::Lost | LinkEvent::Retrying { .. } => {
                            self.reporter = None;
                        }
                        LinkEvent::Notification(_) => {}
                    }
                    out.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The thread only exits when this handle is dropped, so
                    // this is unreachable in practice; stay quiet regardless.
                    self.reporter = None;
                    break;
                }
            }
        }
        out
    }

    /// Send a report if connected. Returns `true` when it was written; a
    /// write failure drops the reporter (the reader thread reports `Lost`
    /// on its own once the stream closes).
    pub fn report(&mut self, report: &Report) -> bool {
        let Some(reporter) = &self.reporter else {
            return false;
        };
        match reporter.report(report) {
            Ok(()) => true,
            Err(_) => {
                self.reporter = None;
                false
            }
        }
    }

    /// Report a focus change, suppressing repeats of the last reported pid.
    pub fn report_focus(&mut self, pid: Option<u32>) -> bool {
        if self.last_focus == Some(pid) {
            return false;
        }
        if self.report(&Report::FocusChanged { pid }) {
            self.last_focus = Some(pid);
            true
        } else {
            false
        }
    }
}

fn link_thread(
    path: &Path,
    tx: Sender<(LinkEvent, Option<Reporter>)>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let send = |event: LinkEvent, reporter: Option<Reporter>| -> bool {
        let ok = tx.send((event, reporter)).is_ok();
        if ok {
            wake();
        }
        ok
    };
    let mut retrying_reported = false;
    loop {
        let subscription = Client::connect(path, ClientRole::Compositor, CONNECT_TIMEOUT)
            .and_then(Client::subscribe);
        let subscription = match subscription {
            Ok(subscription) => subscription,
            Err(reason) => {
                if !retrying_reported {
                    retrying_reported = true;
                    if !send(LinkEvent::Retrying { reason }, None) {
                        return;
                    }
                }
                thread::sleep(RETRY_INTERVAL);
                continue;
            }
        };
        retrying_reported = false;
        let (notifications, reporter) = subscription.spawn(wake.clone());
        if !send(LinkEvent::Connected, Some(reporter)) {
            return;
        }
        // Blocks until appd closes the stream (the reader thread drops its
        // sender); the reader already woke the loop for each notification.
        for notification in notifications {
            if tx
                .send((LinkEvent::Notification(notification), None))
                .is_err()
            {
                return;
            }
        }
        if !send(LinkEvent::Lost, None) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use app_api::{AppId, SCHEMA_VERSION};
    use app_protocol::{Inbound, Reply, Request};

    struct FakeAppd {
        writer: UnixStream,
        reader: BufReader<UnixStream>,
    }

    impl FakeAppd {
        fn accept(listener: &UnixListener) -> Self {
            let (stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let writer = stream.try_clone().unwrap();
            Self {
                writer,
                reader: BufReader::new(stream),
            }
        }

        fn read(&mut self) -> Inbound {
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line).expect("read");
            assert!(bytes > 0, "link closed the stream");
            serde_json::from_str(line.trim()).expect("inbound json")
        }

        fn reply(&mut self, reply: &Reply) {
            let mut text = serde_json::to_string(reply).unwrap();
            text.push('\n');
            self.writer.write_all(text.as_bytes()).unwrap();
        }

        /// Walk the link through Hello, Catalog, Instances, Subscribe.
        fn handshake(&mut self) {
            assert_eq!(
                self.read(),
                Inbound::Request(Request::Hello {
                    role: ClientRole::Compositor
                })
            );
            self.reply(&Reply::Hello {
                schema_version: SCHEMA_VERSION,
                evidence: "mock".to_owned(),
                home: None,
            });
            assert_eq!(self.read(), Inbound::Request(Request::Catalog));
            self.reply(&Reply::Catalog {
                schema_version: SCHEMA_VERSION,
                apps: Vec::new(),
            });
            assert_eq!(self.read(), Inbound::Request(Request::Instances));
            self.reply(&Reply::Instances {
                schema_version: SCHEMA_VERSION,
                instances: Vec::new(),
            });
            assert_eq!(self.read(), Inbound::Request(Request::Subscribe));
            self.reply(&Reply::Ok);
        }

        fn push(&mut self, notification: &Notification) {
            let mut text = serde_json::to_string(notification).unwrap();
            text.push('\n');
            self.writer.write_all(text.as_bytes()).unwrap();
        }
    }

    fn socket_path(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mobile-shell-appd-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("appd.sock")
    }

    /// Wait (bounded) until `link.drain()` yields something matching `pred`.
    fn wait_for(
        link: &mut AppdLink,
        wakes: &Receiver<()>,
        pred: impl Fn(&LinkEvent) -> bool,
    ) -> LinkEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut pending: Vec<LinkEvent> = Vec::new();
        loop {
            pending.extend(link.drain());
            if let Some(index) = pending.iter().position(&pred) {
                return pending.remove(index);
            }
            assert!(Instant::now() < deadline, "timed out; saw {pending:?}");
            let _ = wakes.recv_timeout(Duration::from_millis(200));
        }
    }

    #[test]
    fn subscribes_reports_receives_and_reconnects() {
        let path = socket_path("roundtrip");
        let listener = UnixListener::bind(&path).unwrap();
        let (wake_tx, wakes) = mpsc::channel();
        let wake_count = Arc::new(AtomicUsize::new(0));
        let counter = wake_count.clone();
        let mut link = AppdLink::start(
            &path,
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = wake_tx.send(());
            }),
        );
        assert!(!link.connected());
        assert!(!link.report(&Report::HomeGesture), "no stream yet");

        let mut appd = FakeAppd::accept(&listener);
        appd.handshake();
        let event = wait_for(&mut link, &wakes, |e| matches!(e, LinkEvent::Connected));
        assert_eq!(event.status(), Some("connected"));
        assert!(link.connected());
        assert!(
            wake_count.load(Ordering::SeqCst) >= 1,
            "connect woke the loop"
        );

        // Compositor -> appd reports, with focus de-duplication.
        assert!(link.report(&Report::SurfaceMapped {
            pid: 42,
            app_id: Some("org.ferrix.terminal".to_owned()),
            title: Some("Terminal".to_owned()),
        }));
        assert!(link.report_focus(Some(42)));
        assert!(
            !link.report_focus(Some(42)),
            "identical focus is not repeated"
        );
        assert!(link.report_focus(None));
        assert!(link.report(&Report::HomeGesture));
        assert_eq!(
            appd.read(),
            Inbound::Report(Report::SurfaceMapped {
                pid: 42,
                app_id: Some("org.ferrix.terminal".to_owned()),
                title: Some("Terminal".to_owned()),
            })
        );
        assert_eq!(
            appd.read(),
            Inbound::Report(Report::FocusChanged { pid: Some(42) })
        );
        assert_eq!(
            appd.read(),
            Inbound::Report(Report::FocusChanged { pid: None })
        );
        assert_eq!(appd.read(), Inbound::Report(Report::HomeGesture));

        // appd -> compositor directive.
        let activate = Notification::Activate {
            pid: 7,
            app_id: AppId::new("org.ferrix.home").unwrap(),
        };
        appd.push(&activate);
        let event = wait_for(&mut link, &wakes, |e| {
            matches!(e, LinkEvent::Notification(_))
        });
        match event {
            LinkEvent::Notification(n) => assert_eq!(n, activate),
            other => panic!("unexpected {other:?}"),
        }

        // appd goes away: Lost, then exactly one Retrying while it is down.
        drop(appd);
        let event = wait_for(&mut link, &wakes, |e| matches!(e, LinkEvent::Lost));
        assert_eq!(event.status(), Some("lost"));
        assert!(!link.connected());
        assert!(!link.report(&Report::HomeGesture));
        // Take the listener down so connection attempts fail outright.
        drop(listener);
        std::fs::remove_file(&path).unwrap();
        let event = wait_for(&mut link, &wakes, |e| {
            matches!(e, LinkEvent::Retrying { .. })
        });
        assert_eq!(event.status(), Some("retrying"));

        // Bring appd back; the link reconnects, re-handshakes, and focus
        // de-duplication is reset so a fresh appd learns the current focus.
        let listener = UnixListener::bind(&path).unwrap();
        let mut appd = FakeAppd::accept(&listener);
        appd.handshake();
        let events_before = wait_for(&mut link, &wakes, |e| matches!(e, LinkEvent::Connected));
        assert_eq!(events_before.status(), Some("connected"));
        let extra_retrying = link
            .drain()
            .into_iter()
            .filter(|e| matches!(e, LinkEvent::Retrying { .. }))
            .count();
        assert_eq!(extra_retrying, 0, "retrying is recorded once per outage");
        assert!(link.report_focus(None), "focus repeats after a reconnect");
        assert_eq!(
            appd.read(),
            Inbound::Report(Report::FocusChanged { pid: None })
        );

        drop(link);
        drop(appd);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_socket_reports_retrying_once_and_keeps_trying() {
        let path = socket_path("missing");
        let (wake_tx, wakes) = mpsc::channel();
        let mut link = AppdLink::start(
            &path,
            Arc::new(move || {
                let _ = wake_tx.send(());
            }),
        );
        let event = wait_for(&mut link, &wakes, |e| {
            matches!(e, LinkEvent::Retrying { .. })
        });
        match event {
            LinkEvent::Retrying { reason } => {
                assert!(reason.contains("unavailable"), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(!link.connected());
        assert!(link.drain().is_empty());
        assert!(!link.report_focus(Some(1)));
        drop(link);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
