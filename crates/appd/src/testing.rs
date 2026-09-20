//! Test doubles shared by unit and integration tests: a channel-driven fake
//! runtime, a fake process tree, and an in-memory subscriber sink. Compiled
//! into the library so `tests/*.rs` can use them; nothing here touches the
//! operating system.

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_api::{AppDescriptor, AppError, AppId, ExitReason, RuntimeKind};
use app_runtime_api::{LaunchContext, RunningApp, Runtime};

use crate::process_tree::ProcessTree;

/// One launch the fake runtime performed.
pub struct FakeLaunch {
    pub app: AppId,
    pub context: LaunchContext,
    pub pid: u32,
    exit: Sender<ExitReason>,
    terminated: Arc<AtomicBool>,
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
        }))
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
        _config: &serde_json::Value,
        context: &LaunchContext,
    ) -> Result<Box<dyn RunningApp>, AppError> {
        if let Some(error) = lock(&self.0.fail_next).take() {
            return Err(error);
        }
        let pid = self.0.next_pid.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        let terminated = Arc::new(AtomicBool::new(false));
        lock(&self.0.launches).push(FakeLaunch {
            app: app.id.clone(),
            context: context.clone(),
            pid,
            exit: tx,
            terminated: Arc::clone(&terminated),
        });
        Ok(Box::new(FakeRunning {
            pid,
            rx,
            done: None,
            terminated,
        }))
    }
}

struct FakeRunning {
    pid: u32,
    rx: Receiver<ExitReason>,
    done: Option<ExitReason>,
    terminated: Arc<AtomicBool>,
}

impl RunningApp for FakeRunning {
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
