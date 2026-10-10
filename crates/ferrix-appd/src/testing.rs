//! Test doubles shared by unit and integration tests: a channel-driven fake
//! runtime, a fake process tree, and an in-memory subscriber sink. Compiled
//! into the library so `tests/*.rs` can use them; nothing here touches the
//! operating system.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ferrix_app_api::{AppDescriptor, AppError, AppId, ExitReason, RuntimeKind};
use ferrix_app_runtime_api::{LaunchContext, ProvidedApp, RunningApp, Runtime, SurfaceMatch};

use crate::process_tree::ProcessTree;

/// One launch the fake runtime performed.
pub struct FakeLaunch {
    pub app: AppId,
    pub context: LaunchContext,
    pub pid: u32,
    /// Started through `Runtime::attach` (adopted), not `launch`.
    pub attached: bool,
    exit: Sender<ExitReason>,
    terminated: Arc<AtomicBool>,
    matcher: Arc<Mutex<SurfaceMatch>>,
    reopens: Arc<AtomicU32>,
}

impl FakeLaunch {
    /// Whether `terminate` was called on the running instance.
    pub fn terminated(&self) -> bool {
        self.terminated.load(Ordering::SeqCst)
    }
}

struct FakeInner {
    kind: RuntimeKind,
    next_pid: AtomicU32,
    launches: Mutex<Vec<FakeLaunch>>,
    fail_next: Mutex<Option<AppError>>,
    /// `(client pids, app_id prefix)` for a shared-host runtime: every
    /// instance's windows are `prefix + app id` from those pids.
    shared_host: Mutex<Option<(BTreeSet<u32>, String)>>,
    /// What `installed_apps` returns (`None`: the runtime has no notion).
    installed: Mutex<Option<Result<Vec<ProvidedApp>, AppError>>>,
    listings: AtomicU32,
    /// Pids `shared_host_pids` reports whether or not an instance runs.
    host_pids: Mutex<BTreeSet<u32>>,
}

/// A [`Runtime`] whose instances exit when the test says so. Clone handles
/// share state, so a test keeps one and hands another to the service.
#[derive(Clone)]
pub struct FakeRuntime(Arc<FakeInner>);

impl FakeRuntime {
    pub fn new(kind: RuntimeKind) -> Self {
        Self(Arc::new(FakeInner {
            kind,
            next_pid: AtomicU32::new(1000),
            launches: Mutex::new(Vec::new()),
            fail_next: Mutex::new(None),
            shared_host: Mutex::new(None),
            installed: Mutex::new(None),
            listings: AtomicU32::new(0),
            host_pids: Mutex::new(BTreeSet::new()),
        }))
    }

    /// Sets what `installed_apps` reports from now on.
    pub fn set_installed(&self, installed: Option<Result<Vec<ProvidedApp>, AppError>>) {
        *lock(&self.0.installed) = installed;
    }

    /// How often `installed_apps` was called.
    pub fn listings(&self) -> u32 {
        self.0.listings.load(Ordering::SeqCst)
    }

    /// The pids `shared_host_pids` reports (the runtime's composer).
    pub fn set_host_pids(&self, pids: &[u32]) {
        *lock(&self.0.host_pids) = pids.iter().copied().collect();
    }

    /// Whether the most recent instance of `app` was adopted (`attach`).
    pub fn last_attached(&self, app: &AppId) -> Option<bool> {
        lock(&self.0.launches)
            .iter()
            .rev()
            .find(|l| &l.app == app)
            .map(|l| l.attached)
    }

    /// A runtime whose windows come from shared client `pids` (like the
    /// Android composer): each instance claims toplevels whose app_id is
    /// `prefix` followed by its app id.
    pub fn shared_host(kind: RuntimeKind, pids: &[u32], prefix: &str) -> Self {
        let runtime = Self::new(kind);
        *lock(&runtime.0.shared_host) = Some((pids.iter().copied().collect(), prefix.to_owned()));
        runtime
    }

    /// Replaces the shared client pids of the most recent instance of `app`
    /// (the runtime learned or lost composer connections).
    pub fn set_client_pids(&self, app: &AppId, pids: &[u32]) {
        if let Some(launch) = lock(&self.0.launches).iter().rev().find(|l| &l.app == app) {
            let mut matcher = lock(&launch.matcher);
            if let SurfaceMatch::SharedHost { client_pids, .. } = &mut *matcher {
                *client_pids = pids.iter().copied().collect();
            }
        }
    }

    /// How often the most recent instance of `app` was asked to reopen.
    pub fn reopen_count(&self, app: &AppId) -> u32 {
        lock(&self.0.launches)
            .iter()
            .rev()
            .find(|l| &l.app == app)
            .map_or(0, |l| l.reopens.load(Ordering::SeqCst))
    }

    pub fn native() -> Self {
        Self::new(RuntimeKind::Native)
    }

    /// The next `launch` fails with `error`.
    pub fn fail_next(&self, error: AppError) {
        *lock(&self.0.fail_next) = Some(error);
    }

    pub fn launch_count(&self) -> usize {
        lock(&self.0.launches).len()
    }

    /// Forces the pid assigned by the next launch. Used to exercise the
    /// service's duplicate surface/runtime pid guard.
    pub fn set_next_pid(&self, pid: u32) {
        self.0.next_pid.store(pid, Ordering::SeqCst);
    }

    /// Snapshot of `(app, instance, pid)` for every launch, in order.
    pub fn launches(&self) -> Vec<(AppId, u32, u32)> {
        lock(&self.0.launches)
            .iter()
            .map(|l| (l.app.clone(), l.context.instance, l.pid))
            .collect()
    }

    /// Environment handed to the most recent launch of `app`.
    pub fn last_env(&self, app: &AppId) -> Option<Vec<(String, String)>> {
        lock(&self.0.launches)
            .iter()
            .rev()
            .find(|l| &l.app == app)
            .map(|l| l.context.env.clone())
    }

    /// Effective capabilities handed to the most recent launch of `app`.
    pub fn last_granted(
        &self,
        app: &AppId,
    ) -> Option<std::collections::BTreeSet<ferrix_app_api::AppCapability>> {
        lock(&self.0.launches)
            .iter()
            .rev()
            .find(|l| &l.app == app)
            .map(|l| l.context.granted.clone())
    }

    /// Pid of the most recent launch of `app`.
    pub fn last_pid(&self, app: &AppId) -> Option<u32> {
        lock(&self.0.launches)
            .iter()
            .rev()
            .find(|l| &l.app == app)
            .map(|l| l.pid)
    }

    /// Whether the most recent launch of `app` was asked to terminate.
    pub fn last_terminated(&self, app: &AppId) -> bool {
        lock(&self.0.launches)
            .iter()
            .rev()
            .find(|l| &l.app == app)
            .is_some_and(FakeLaunch::terminated)
    }

    /// Makes the most recent instance of `app` exit with `reason`.
    pub fn exit(&self, app: &AppId, reason: ExitReason) -> bool {
        lock(&self.0.launches)
            .iter()
            .rev()
            .find(|l| &l.app == app)
            .is_some_and(|l| l.exit.send(reason).is_ok())
    }
}

impl Runtime for FakeRuntime {
    fn kind(&self) -> RuntimeKind {
        self.0.kind
    }

    fn launch(
        &self,
        app: &AppDescriptor,
        config: &serde_json::Value,
        context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        self.start(app, config, context, false)
    }

    fn attach(
        &self,
        app: &AppDescriptor,
        config: &serde_json::Value,
        context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        if lock(&self.0.shared_host).is_none() {
            return Err(AppError::RuntimeUnsupported {
                runtime: app.runtime,
            });
        }
        self.start(app, config, context, true)
    }

    fn installed_apps(&self) -> Option<Result<Vec<ProvidedApp>, AppError>> {
        self.0.listings.fetch_add(1, Ordering::SeqCst);
        lock(&self.0.installed).clone()
    }

    /// `config.key`, standing in for the Android package.
    fn app_key(&self, config: &serde_json::Value) -> Option<String> {
        config.get("key")?.as_str().map(str::to_owned)
    }

    /// `config.window`, standing in for `waydroid.<package>`.
    fn window_app_id(&self, config: &serde_json::Value) -> Option<String> {
        config.get("window")?.as_str().map(str::to_owned)
    }

    fn shared_host_pids(&self) -> BTreeSet<u32> {
        lock(&self.0.host_pids).clone()
    }
}

impl FakeRuntime {
    fn start(
        &self,
        app: &AppDescriptor,
        config: &serde_json::Value,
        context: &LaunchContext,
        attached: bool,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        if let Some(error) = lock(&self.0.fail_next).take() {
            return Err(error);
        }
        let pid = self.0.next_pid.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        let terminated = Arc::new(AtomicBool::new(false));
        let matcher = match &*lock(&self.0.shared_host) {
            Some((pids, prefix)) => SurfaceMatch::SharedHost {
                client_pids: pids.clone(),
                app_id: self
                    .window_app_id(config)
                    .unwrap_or_else(|| format!("{prefix}{}", app.id)),
            },
            None => SurfaceMatch::ProcessTree,
        };
        let matcher = Arc::new(Mutex::new(matcher));
        let reopens = Arc::new(AtomicU32::new(0));
        lock(&self.0.launches).push(FakeLaunch {
            app: app.id.clone(),
            context: context.clone(),
            pid,
            attached,
            exit: tx,
            terminated: Arc::clone(&terminated),
            matcher: Arc::clone(&matcher),
            reopens: Arc::clone(&reopens),
        });
        Ok(Box::new(FakeRunning {
            pid,
            rx,
            done: None,
            terminated,
            matcher,
            reopens,
        }))
    }
}

struct FakeRunning {
    pid: u32,
    rx: Receiver<ExitReason>,
    done: Option<ExitReason>,
    terminated: Arc<AtomicBool>,
    matcher: Arc<Mutex<SurfaceMatch>>,
    reopens: Arc<AtomicU32>,
}

impl RunningApp for FakeRunning {
    fn surface_match(&self) -> SurfaceMatch {
        lock(&self.matcher).clone()
    }

    fn reopen(&mut self) -> Result<bool, AppError> {
        if self.done.is_some() || !lock(&self.matcher).is_shared_host() {
            return Ok(false);
        }
        self.reopens.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({"kind": "fake"})
    }

    fn try_wait(&mut self) -> Result<Option<ExitReason>, AppError> {
        if let Some(done) = self.done {
            return Ok(Some(done));
        }
        match self.rx.try_recv() {
            Ok(reason) => {
                self.done = Some(reason);
                Ok(Some(reason))
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(None),
        }
    }

    fn wait(&mut self) -> Result<ExitReason, AppError> {
        if let Some(done) = self.done {
            return Ok(done);
        }
        let reason = self.rx.recv().unwrap_or(ExitReason::Exited { code: 0 });
        self.done = Some(reason);
        Ok(reason)
    }

    fn terminate(&mut self, _grace: Duration) -> Result<ExitReason, AppError> {
        self.terminated.store(true, Ordering::SeqCst);
        if let Some(done) = self.done {
            return Ok(done);
        }
        let reason = ExitReason::Signaled { signal: 15 };
        self.done = Some(reason);
        Ok(reason)
    }
}

/// A [`ProcessTree`] with explicit parent links.
#[derive(Clone, Default)]
pub struct FakeProcessTree(Arc<Mutex<HashMap<u32, u32>>>);

impl FakeProcessTree {
    pub fn set_parent(&self, child: u32, parent: u32) {
        lock(&self.0).insert(child, parent);
    }
}

impl ProcessTree for FakeProcessTree {
    fn parent(&self, pid: u32) -> Option<u32> {
        lock(&self.0).get(&pid).copied()
    }
}

/// A `Write` that delivers each completed line to a channel; stands in for a
/// subscriber's socket.
pub struct ChannelSink {
    tx: Sender<String>,
    buffer: Vec<u8>,
}

/// Creates a sink and the receiver of its lines.
pub fn channel_sink() -> (ChannelSink, Receiver<String>) {
    let (tx, rx) = mpsc::channel();
    (
        ChannelSink {
            tx,
            buffer: Vec::new(),
        },
        rx,
    )
}

impl Write for ChannelSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        while let Some(newline) = self.buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            self.tx
                .send(text)
                .map_err(|_| io::Error::other("subscriber receiver dropped"))?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Polls `condition` every millisecond until it holds or `timeout` passes.
pub fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
