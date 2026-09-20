//! The application service: single-instance launch, supervision, surface
//! association, foreground tracking, and notification fan-out.
//!
//! All mutable state sits behind one mutex. Runtimes are `app_runtime_api::Runtime`
//! trait objects composed by the binary; the library never names an
//! implementation. Each launched instance gets a waiter thread that polls the
//! runtime handle and posts the exit back, so a crash is observed, recorded,
//! and published without ever touching the compositor or the core.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use app_api::{
    APP_ID_ENV, APP_INSTANCE_ENV, APPD_SOCKET_ENV, AppError, AppEvent, AppId, AppInstance,
    DEFAULT_APPD_SOCKET, ExitReason, LifecycleState, RuntimeKind, SCHEMA_VERSION,
};
use app_protocol::{ClientRole, Notification, Reply};
use app_runtime_api::{LaunchContext, RunningApp, Runtime};
use serde::Serialize;

use crate::catalog::Catalog;
use crate::lifecycle::{HomeRestartConfig, HomeRestartTracker, RestartDecision};
use crate::process_tree::{self, ProcessTree};
use crate::records::Recorder;

/// Source of `Instant`s for the restart budget; tests inject a fake.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

type Handle = Arc<Mutex<Box<dyn RunningApp>>>;

#[derive(Clone, Debug)]
pub struct ServiceOptions {
    /// Socket path handed to launched apps as `FERRIX_APPD_SOCKET`.
    pub socket: PathBuf,
    /// Grace period between the polite and the forced stop.
    pub terminate_grace: Duration,
    /// How often waiter threads poll a running instance.
    pub poll_interval: Duration,
    pub restart: HomeRestartConfig,
}

impl Default for ServiceOptions {
    fn default() -> Self {
        Self {
            socket: PathBuf::from(DEFAULT_APPD_SOCKET),
            terminate_grace: Duration::from_secs(2),
            poll_interval: Duration::from_millis(20),
            restart: HomeRestartConfig::default(),
        }
    }
}

/// Result of `Launch`/`Home`: the instance and whether it already ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchOutcome {
    pub instance: AppInstance,
    pub activated_existing: bool,
}

/// A scheduled home restart that has not fired yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingRestart {
    pub attempt: u32,
    pub delay: Duration,
    token: u64,
}

/// A stream that asked for notifications. Compositor subscribers also
/// receive `Notification::Activate`.
pub struct Subscriber {
    pub role: ClientRole,
    pub sink: Box<dyn Write + Send>,
}

struct Tracked {
    instance: AppInstance,
    handle: Option<Handle>,
    terminate_requested: bool,
}

#[derive(Default)]
struct State {
    next_instance: u32,
    instances: BTreeMap<AppId, Tracked>,
    /// Kernel pids known to belong to an instance: the launched pid plus
    /// every descendant resolved through the process tree.
    pids: HashMap<u32, AppId>,
    foreground: Option<AppId>,
    /// Last pid the compositor reported focus on, resolved or not.
    focused_pid: Option<u32>,
    home_restart: HomeRestartTracker,
    pending_restart: Option<PendingRestart>,
    restart_tokens: u64,
}

pub struct Service {
    catalog: Catalog,
    runtimes: HashMap<RuntimeKind, Box<dyn Runtime>>,
    tree: Box<dyn ProcessTree>,
    recorder: Recorder,
    options: ServiceOptions,
    clock: Clock,
    state: Mutex<State>,
    subscribers: Mutex<Vec<Subscriber>>,
}

impl Service {
    pub fn new(
        catalog: Catalog,
        runtimes: Vec<Box<dyn Runtime>>,
        tree: Box<dyn ProcessTree>,
        recorder: Recorder,
        options: ServiceOptions,
    ) -> Self {
        let runtimes = runtimes
            .into_iter()
            .map(|runtime| (runtime.kind(), runtime))
            .collect();
        Self {
            catalog,
            runtimes,
            tree,
            recorder,
            options,
            clock: Arc::new(Instant::now),
            state: Mutex::new(State {
                next_instance: 1,
                ..State::default()
            }),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Replaces the clock used by the home restart budget.
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    pub fn evidence(&self) -> &str {
        self.recorder.evidence()
    }

    pub fn options(&self) -> &ServiceOptions {
        &self.options
    }

    pub fn supports(&self, kind: RuntimeKind) -> bool {
        self.runtimes.contains_key(&kind)
    }

    pub fn hello_reply(&self) -> Reply {
        Reply::Hello {
            schema_version: SCHEMA_VERSION,
            evidence: self.recorder.evidence().to_owned(),
            home: self.catalog.home().map(|d| d.id.clone()),
        }
    }

    /// Every instance ever tracked (one per app), including finished ones.
    pub fn instances(&self) -> Vec<AppInstance> {
        self.lock_state()
            .instances
            .values()
            .map(|t| t.instance.clone())
            .collect()
    }

    pub fn instance(&self, id: &AppId) -> Option<AppInstance> {
        self.lock_state()
            .instances
            .get(id)
            .map(|t| t.instance.clone())
    }

    pub fn foreground(&self) -> Option<AppId> {
        self.lock_state().foreground.clone()
    }

    /// Resolve a socket peer that belongs to a launched process into its
    /// daemon-assigned protocol role. The peer cannot choose this identity.
    pub fn client_role_for_pid(&self, pid: u32) -> Option<ClientRole> {
        let mut state = self.lock_state();
        let app = self.resolve_pid(&mut state, pid)?;
        let role = self.catalog.get(&app)?.descriptor.role;
        Some(match role {
            app_api::AppRole::Home => ClientRole::Home,
            app_api::AppRole::Application => ClientRole::Application,
        })
    }

    /// The scheduled home restart, if one is waiting for its delay.
    pub fn pending_home_restart(&self) -> Option<PendingRestart> {
        self.lock_state().pending_restart
    }

    pub fn home_restart_abandoned(&self) -> bool {
        self.lock_state().home_restart.abandoned()
    }

    // -- requests -----------------------------------------------------------

    /// Starts `id`, or activates it when it already runs.
    pub fn launch(self: &Arc<Self>, id: &AppId) -> Result<LaunchOutcome, AppError> {
        let mut state = self.lock_state();
        if let Some(instance) = self.activate_locked(&mut state, id) {
            return Ok(LaunchOutcome {
                instance,
                activated_existing: true,
            });
        }
        if self.is_home(id) {
            state.home_restart.reset();
            state.pending_restart = None;
        }
        let instance = self.launch_locked(&mut state, id)?;
        Ok(LaunchOutcome {
            instance,
            activated_existing: false,
        })
    }

    /// Raises a running app; `NotRunning` otherwise.
    pub fn activate(&self, id: &AppId) -> Result<AppInstance, AppError> {
        let mut state = self.lock_state();
        if !self.catalog.contains(id) {
            return Err(AppError::UnknownApp { id: id.clone() });
        }
        self.activate_locked(&mut state, id)
            .ok_or_else(|| AppError::NotRunning { id: id.clone() })
    }

    /// Stops a running app: polite, then forced after the grace period. The
    /// final state is `Terminated` whatever the exit reason.
    pub fn terminate(self: &Arc<Self>, id: &AppId) -> Result<AppInstance, AppError> {
        let (handle, number) = {
            let mut state = self.lock_state();
            if !self.catalog.contains(id) {
                return Err(AppError::UnknownApp { id: id.clone() });
            }
            let tracked = state
                .instances
                .get_mut(id)
                .filter(|t| t.instance.state.is_running())
                .ok_or_else(|| AppError::NotRunning { id: id.clone() })?;
            tracked.terminate_requested = true;
            let handle = tracked
                .handle
                .clone()
                .ok_or_else(|| AppError::NotRunning { id: id.clone() })?;
            self.recorder.app_terminating(id, tracked.instance.instance);
            (handle, tracked.instance.instance)
        };
        let reason = lock(&handle).terminate(self.options.terminate_grace)?;
        self.post_exit(id, number, reason, true);
        self.instance(id)
            .ok_or_else(|| AppError::NotRunning { id: id.clone() })
    }

    /// Returns to the home surface: activates it, or (re)launches it when it
    /// is not running, which also resets the restart budget.
    pub fn home(self: &Arc<Self>) -> Result<LaunchOutcome, AppError> {
        let home = self
            .catalog
            .home()
            .map(|d| d.id.clone())
            .ok_or_else(|| AppError::Io {
                reason: "the catalog has no home surface".to_owned(),
            })?;
        self.launch(&home)
    }

    /// Starts the home surface, then every other `autostart` app. Failures
    /// are recorded (`app-launch-failed`) and returned, never fatal.
    pub fn autostart(self: &Arc<Self>) -> Vec<(AppId, Result<LaunchOutcome, AppError>)> {
        let mut ids: Vec<AppId> = self
            .catalog
            .home()
            .map(|d| d.id.clone())
            .into_iter()
            .collect();
        ids.extend(self.catalog.autostart().map(|d| d.id.clone()));
        ids.into_iter()
            .map(|id| {
                let result = self.launch(&id);
                (id, result)
            })
            .collect()
    }

    // -- compositor reports -------------------------------------------------

    /// A toplevel of `pid` committed its first buffer. Resolves the pid to an
    /// instance (directly or through parents) and returns the app, if any.
    pub fn surface_mapped(
        &self,
        pid: u32,
        app_id: Option<&str>,
        title: Option<&str>,
    ) -> Option<AppId> {
        let mut state = self.lock_state();
        let Some(app) = self.resolve_pid(&mut state, pid) else {
            self.recorder.app_surface_unknown(pid, app_id);
            return None;
        };
        let tracked = state.instances.get_mut(&app)?;
        if !tracked.instance.state.is_running() {
            return None;
        }
        tracked.instance.surface_mapped = true;
        self.recorder.app_surface(&app, pid, app_id, title);
        if let Some(reported) = app_id
            && reported != app.as_str()
        {
            self.recorder.app_id_mismatch(&app, pid, reported);
        }
        if tracked.instance.state == LifecycleState::Starting {
            let focused = state.focused_pid == Some(pid)
                || state.foreground.as_ref() == Some(&app)
                || state.foreground.is_none();
            if focused {
                self.set_foreground(&mut state, &app);
            } else {
                self.transition(&mut state, &app, LifecycleState::Background);
            }
        } else {
            self.publish_instance(&state, &app);
        }
        Some(app)
    }

    /// The last toplevel of `pid` went away.
    pub fn surface_unmapped(&self, pid: u32) -> Option<AppId> {
        let mut state = self.lock_state();
        let app = self.resolve_pid(&mut state, pid)?;
        let tracked = state.instances.get_mut(&app)?;
        tracked.instance.surface_mapped = false;
        if state.foreground.as_ref() == Some(&app) {
            state.foreground = None;
            self.transition(&mut state, &app, LifecycleState::Background);
        } else {
            self.publish_instance(&state, &app);
        }
        Some(app)
    }

    /// Focus moved to `pid` (or to nothing). Returns the app now in front.
    pub fn focus_changed(&self, pid: Option<u32>) -> Option<AppId> {
        let mut state = self.lock_state();
        state.focused_pid = pid;
        let target = pid
            .and_then(|pid| self.resolve_pid(&mut state, pid))
            .filter(|app| {
                state
                    .instances
                    .get(app)
                    .is_some_and(|t| t.instance.state.is_running())
            });
        match target {
            Some(app) => {
                self.set_foreground(&mut state, &app);
                Some(app)
            }
            None => {
                if let Some(previous) = state.foreground.take() {
                    self.transition(&mut state, &previous, LifecycleState::Background);
                }
                None
            }
        }
    }

    // -- subscribers --------------------------------------------------------

    /// Registers a subscriber and acknowledges on its own stream while
    /// holding the fan-out lock, so no notification precedes the `Ok`.
    pub fn subscribe(&self, role: ClientRole, mut sink: Box<dyn Write + Send>) -> io::Result<()> {
        let mut subscribers = lock(&self.subscribers);
        write_line(&mut *sink, &Reply::Ok)?;
        subscribers.push(Subscriber { role, sink });
        Ok(())
    }

    pub fn subscriber_count(&self) -> usize {
        lock(&self.subscribers).len()
    }

    /// Publishes `CatalogChanged` for the current catalog.
    pub fn announce_catalog(&self) {
        self.publish(&Notification::App {
            event: AppEvent::CatalogChanged {
                apps: self.catalog.len(),
            },
        });
    }

    /// Fans a notification out; `Activate` only reaches compositor
    /// subscribers. Dead streams are dropped.
    pub fn publish(&self, notification: &Notification) {
        let compositor_only = matches!(notification, Notification::Activate { .. });
        let mut subscribers = lock(&self.subscribers);
        subscribers.retain_mut(|subscriber| {
            if compositor_only && subscriber.role != ClientRole::Compositor {
                return true;
            }
            write_line(&mut *subscriber.sink, notification).is_ok()
        });
    }

    // -- exits --------------------------------------------------------------

    /// Records that instance `number` of `id` ended. Idempotent: a stale or
    /// duplicate report (waiter racing `terminate`) is ignored. Applies the
    /// home restart policy for unrequested home exits.
    pub fn post_exit(
        self: &Arc<Self>,
        id: &AppId,
        number: u32,
        reason: ExitReason,
        requested: bool,
    ) {
        let mut state = self.lock_state();
        let Some(tracked) = state.instances.get_mut(id) else {
            return;
        };
        if tracked.instance.instance != number || !tracked.instance.state.is_running() {
            return;
        }
        let requested = requested || tracked.terminate_requested;
        let final_state = if requested {
            LifecycleState::Terminated
        } else {
            reason.final_state()
        };
        tracked.instance.state = final_state;
        tracked.instance.exit = Some(reason);
        tracked.instance.surface_mapped = false;
        tracked.handle = None;
        state.pids.retain(|_, app| app != id);
        if state.foreground.as_ref() == Some(id) {
            state.foreground = None;
        }
        if final_state == LifecycleState::Crashed {
            self.recorder.app_crashed(id, number, reason);
        } else {
            self.recorder.app_exited(id, number, reason);
        }
        self.publish_instance(&state, id);

        if self.is_home(id) && !requested {
            let now = (self.clock)();
            let config = self.options.restart.clone();
            match state.home_restart.record_exit(&config, now) {
                RestartDecision::Abandon { exits } => {
                    self.recorder.home_restart_abandoned(exits, config.window);
                }
                RestartDecision::Restart { attempt, delay } => {
                    self.recorder.home_restarting(attempt, delay);
                    if delay.is_zero() {
                        let _ = self.launch_locked(&mut state, id);
                    } else {
                        self.schedule_home_restart(&mut state, attempt, delay);
                    }
                }
            }
        }
    }

    // -- internals ----------------------------------------------------------

    fn is_home(&self, id: &AppId) -> bool {
        self.catalog.home().is_some_and(|home| &home.id == id)
    }

    fn lock_state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn launch_locked(
        self: &Arc<Self>,
        state: &mut State,
        id: &AppId,
    ) -> Result<AppInstance, AppError> {
        let entry = self
            .catalog
            .get(id)
            .ok_or_else(|| AppError::UnknownApp { id: id.clone() })?;
        let descriptor = &entry.descriptor;
        if !descriptor.launchable {
            return Err(AppError::NotLaunchable { id: id.clone() });
        }
        let runtime =
            self.runtimes
                .get(&descriptor.runtime)
                .ok_or(AppError::RuntimeUnsupported {
                    runtime: descriptor.runtime,
                })?;
        let previous = state
            .instances
            .get(id)
            .map_or(LifecycleState::NotRunning, |t| t.instance.state);
        previous.transition(LifecycleState::Starting)?;

        let number = state.next_instance;
        state.next_instance += 1;
        self.recorder.app_launching(id, number);
        let context = LaunchContext {
            instance: number,
            env: vec![
                (APP_ID_ENV.to_owned(), id.to_string()),
                (APP_INSTANCE_ENV.to_owned(), number.to_string()),
                (
                    APPD_SOCKET_ENV.to_owned(),
                    self.options.socket.display().to_string(),
                ),
            ],
            evidence: self.recorder.evidence().to_owned(),
        };
        let running = runtime
            .launch(descriptor, &entry.launch, &context)
            .inspect_err(|error| self.recorder.app_launch_failed(id, error))?;
        let pid = running.pid();
        let diagnostics = running.diagnostics();
        let instance = AppInstance {
            app: id.clone(),
            runtime: descriptor.runtime,
            role: descriptor.role,
            instance: number,
            state: LifecycleState::Starting,
            pid,
            exit: None,
            surface_mapped: false,
        };
        if let Some(pid) = pid {
            state.pids.insert(pid, id.clone());
        }
        let handle: Handle = Arc::new(Mutex::new(running));
        state.instances.insert(
            id.clone(),
            Tracked {
                instance: instance.clone(),
                handle: Some(Arc::clone(&handle)),
                terminate_requested: false,
            },
        );
        self.recorder.app_started(id, number, pid, &diagnostics);
        self.publish_instance(state, id);
        self.spawn_waiter(id.clone(), number, handle);
        Ok(instance)
    }

    /// Pushes `Activate` for a running app; `None` when it is not running.
    fn activate_locked(&self, state: &mut State, id: &AppId) -> Option<AppInstance> {
        let tracked = state.instances.get(id)?;
        if !tracked.instance.state.is_running() {
            return None;
        }
        let instance = tracked.instance.clone();
        if let Some(pid) = instance.pid {
            self.publish(&Notification::Activate {
                pid,
                app_id: id.clone(),
            });
        }
        Some(instance)
    }

    /// Walks `pid` and its ancestors until one is a known instance pid; the
    /// resolved pid is cached for later reports.
    fn resolve_pid(&self, state: &mut State, pid: u32) -> Option<AppId> {
        if let Some(app) = state.pids.get(&pid) {
            return Some(app.clone());
        }
        let app = process_tree::ancestors(self.tree.as_ref(), pid)
            .find_map(|ancestor| state.pids.get(&ancestor).cloned())?;
        state.pids.insert(pid, app.clone());
        Some(app)
    }

    /// Moves `app` to the foreground and the previous foreground app back.
    fn set_foreground(&self, state: &mut State, app: &AppId) {
        if state.foreground.as_ref() == Some(app) {
            self.transition(state, app, LifecycleState::Foreground);
            return;
        }
        if let Some(previous) = state.foreground.take() {
            self.transition(state, &previous, LifecycleState::Background);
        }
        if self.transition(state, app, LifecycleState::Foreground) {
            state.foreground = Some(app.clone());
        }
    }

    /// Applies a legal transition, records it, and publishes. Illegal ones
    /// (a late report for a finished instance) are ignored. Returns whether
    /// the instance is now in `next`.
    fn transition(&self, state: &mut State, app: &AppId, next: LifecycleState) -> bool {
        let Some(tracked) = state.instances.get_mut(app) else {
            return false;
        };
        let current = tracked.instance.state;
        if current == next {
            return true;
        }
        let Ok(next) = current.transition(next) else {
            return false;
        };
        tracked.instance.state = next;
        match next {
            LifecycleState::Foreground => self.recorder.app_foreground(app),
            LifecycleState::Background => self.recorder.app_background(app),
            _ => {}
        }
        self.publish_instance(state, app);
        true
    }

    fn publish_instance(&self, state: &State, app: &AppId) {
        if let Some(tracked) = state.instances.get(app) {
            self.publish(&Notification::App {
                event: AppEvent::Lifecycle {
                    instance: tracked.instance.clone(),
                },
            });
        }
    }

    fn is_current(&self, id: &AppId, number: u32) -> bool {
        self.lock_state()
            .instances
            .get(id)
            .is_some_and(|t| t.instance.instance == number && t.instance.state.is_running())
    }

    /// Polls the runtime handle until the instance ends, then posts the exit.
    /// Polling (rather than a blocking `wait`) keeps the handle available to
    /// `terminate` in between.
    fn spawn_waiter(self: &Arc<Self>, id: AppId, number: u32, handle: Handle) {
        let service = Arc::clone(self);
        let poll = self.options.poll_interval;
        let name = format!("appd-wait-{id}");
        let waited = id.clone();
        let spawned = thread::Builder::new().name(name).spawn(move || {
            let id = waited;
            loop {
                let polled = lock(&handle).try_wait();
                match polled {
                    Ok(Some(reason)) => {
                        service.post_exit(&id, number, reason, false);
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        service.recorder.app_wait_failed(&id, number, &error);
                        service.post_exit(&id, number, ExitReason::Exited { code: 255 }, false);
                        return;
                    }
                }
                if !service.is_current(&id, number) {
                    return;
                }
                thread::sleep(poll);
            }
        });
        if let Err(error) = spawned {
            self.recorder.app_wait_failed(
                &id,
                number,
                &AppError::Io {
                    reason: format!("cannot spawn waiter thread: {error}"),
                },
            );
        }
    }

    fn schedule_home_restart(self: &Arc<Self>, state: &mut State, attempt: u32, delay: Duration) {
        state.restart_tokens += 1;
        let token = state.restart_tokens;
        state.pending_restart = Some(PendingRestart {
            attempt,
            delay,
            token,
        });
        let service = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("appd-home-restart".to_owned())
            .spawn(move || {
                thread::sleep(delay);
                service.run_pending_restart(token);
            });
    }

    /// Fires a scheduled restart unless it was superseded or cancelled.
    pub fn run_pending_restart(self: &Arc<Self>, token: u64) {
        let mut state = self.lock_state();
        let Some(pending) = state.pending_restart else {
            return;
        };
        if pending.token != token {
            return;
        }
        state.pending_restart = None;
        let Some(home) = self.catalog.home().map(|d| d.id.clone()) else {
            return;
        };
        if state
            .instances
            .get(&home)
            .is_some_and(|t| t.instance.state.is_running())
        {
            return;
        }
        let _ = self.launch_locked(&mut state, &home);
    }
}

impl PendingRestart {
    /// Token identifying this schedule, for `run_pending_restart`.
    pub fn token(&self) -> u64 {
        self.token
    }
}

fn write_line(sink: &mut dyn Write, value: &impl Serialize) -> io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(io::Error::other)?;
    text.push('\n');
    sink.write_all(text.as_bytes())?;
    sink.flush()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{Captured, captured_events};
    use crate::testing::{FakeProcessTree, FakeRuntime, channel_sink, wait_for};
    use app_api::{AppDescriptor, AppRole, IconRef, RuntimeKind, TrustLevel};
    use std::sync::mpsc::Receiver;

    const TIMEOUT: Duration = Duration::from_secs(5);

    fn id(text: &str) -> AppId {
        AppId::new(text).unwrap()
    }

    fn descriptor(
        text: &str,
        runtime: RuntimeKind,
        role: AppRole,
        launchable: bool,
    ) -> AppDescriptor {
        AppDescriptor {
            schema_version: SCHEMA_VERSION,
            id: id(text),
            runtime,
            name: text.to_owned(),
            icon: IconRef::default(),
            launchable,
            autostart: false,
            role,
            trust: TrustLevel::Untrusted,
            capabilities: Default::default(),
        }
    }

    fn catalog() -> Catalog {
        Catalog::from_descriptors([
            descriptor("org.ferrix.home", RuntimeKind::Native, AppRole::Home, true),
            descriptor(
                "org.ferrix.terminal",
                RuntimeKind::Native,
                AppRole::Application,
                true,
            ),
            descriptor(
                "org.ferrix.settings",
                RuntimeKind::Native,
                AppRole::Application,
                false,
            ),
            descriptor(
                "org.example.web",
                RuntimeKind::Web,
                AppRole::Application,
                true,
            ),
            descriptor(
                "org.example.android",
                RuntimeKind::Android,
                AppRole::Application,
                true,
            ),
        ])
        .unwrap()
    }

    struct Harness {
        service: Arc<Service>,
        runtime: FakeRuntime,
        tree: FakeProcessTree,
        records: Captured,
    }

    fn harness() -> Harness {
        harness_with_clock(Arc::new(Instant::now))
    }

    fn harness_with_clock(clock: Clock) -> Harness {
        let runtime = FakeRuntime::native();
        let tree = FakeProcessTree::default();
        let (recorder, records) = Recorder::capturing("mock");
        let options = ServiceOptions {
            socket: PathBuf::from("/tmp/appd-test.sock"),
            poll_interval: Duration::from_millis(1),
            ..ServiceOptions::default()
        };
        let service = Service::new(
            catalog(),
            vec![Box::new(runtime.clone())],
            Box::new(tree.clone()),
            recorder,
            options,
        )
        .with_clock(clock);
        Harness {
            service: Arc::new(service),
            runtime,
            tree,
            records,
        }
    }

    fn subscribe(service: &Service, role: ClientRole) -> Receiver<String> {
        let (sink, rx) = channel_sink();
        service.subscribe(role, Box::new(sink)).unwrap();
        assert_eq!(rx.recv().unwrap(), r#"{"reply":"ok"}"#);
        rx
    }

    fn notifications(rx: &Receiver<String>) -> Vec<Notification> {
        rx.try_iter()
            .map(|line| serde_json::from_str(&line).unwrap())
            .collect()
    }

    fn lifecycle_states(rx: &Receiver<String>, app: &AppId) -> Vec<LifecycleState> {
        notifications(rx)
            .into_iter()
            .filter_map(|n| match n {
                Notification::App {
                    event: AppEvent::Lifecycle { instance },
                } if &instance.app == app => Some(instance.state),
                _ => None,
            })
            .collect()
    }

    fn wait_state(service: &Service, app: &AppId, state: LifecycleState) -> AppInstance {
        assert!(
            wait_for(TIMEOUT, || service
                .instance(app)
                .is_some_and(|i| i.state == state)),
            "{app} never reached {state}: {:?}",
            service.instance(app)
        );
        service.instance(app).unwrap()
    }

    #[test]
    fn unknown_and_non_launchable_apps_are_typed_errors() {
        let h = harness();
        assert_eq!(
            h.service.launch(&id("org.ferrix.nope")),
            Err(AppError::UnknownApp {
                id: id("org.ferrix.nope")
            })
        );
        assert_eq!(
            h.service.launch(&id("org.ferrix.settings")),
            Err(AppError::NotLaunchable {
                id: id("org.ferrix.settings")
            })
        );
        assert_eq!(
            h.service.activate(&id("org.ferrix.terminal")),
            Err(AppError::NotRunning {
                id: id("org.ferrix.terminal")
            })
        );
        assert_eq!(h.runtime.launch_count(), 0);
    }

    #[test]
    fn web_and_android_descriptors_need_a_runtime() {
        let h = harness();
        assert_eq!(
            h.service.launch(&id("org.example.web")),
            Err(AppError::RuntimeUnsupported {
                runtime: RuntimeKind::Web
            })
        );
        assert_eq!(
            h.service.launch(&id("org.example.android")),
            Err(AppError::RuntimeUnsupported {
                runtime: RuntimeKind::Android
            })
        );
        assert!(!h.service.supports(RuntimeKind::Web));
        assert!(h.service.supports(RuntimeKind::Native));
    }

    #[test]
    fn launch_hands_the_app_its_environment_and_records_start() {
        let h = harness();
        let terminal = id("org.ferrix.terminal");
        let outcome = h.service.launch(&terminal).unwrap();
        assert!(!outcome.activated_existing);
        assert_eq!(outcome.instance.state, LifecycleState::Starting);
        assert_eq!(outcome.instance.instance, 1);
        assert_eq!(outcome.instance.pid, h.runtime.last_pid(&terminal));
        let env = h.runtime.last_env(&terminal).unwrap();
        assert!(env.contains(&(APP_ID_ENV.to_owned(), "org.ferrix.terminal".to_owned())));
        assert!(env.contains(&(APP_INSTANCE_ENV.to_owned(), "1".to_owned())));
        assert!(env.contains(&(APPD_SOCKET_ENV.to_owned(), "/tmp/appd-test.sock".to_owned())));
        assert_eq!(
            captured_events(&h.records),
            ["app-launching", "app-started"]
        );
    }

    #[test]
    fn second_launch_activates_the_existing_instance() {
        let h = harness();
        let compositor = subscribe(&h.service, ClientRole::Compositor);
        let home = subscribe(&h.service, ClientRole::Home);
        let terminal = id("org.ferrix.terminal");
        let first = h.service.launch(&terminal).unwrap();
        let second = h.service.launch(&terminal).unwrap();
        assert!(second.activated_existing);
        assert_eq!(second.instance.instance, first.instance.instance);
        assert_eq!(h.runtime.launch_count(), 1);

        let pid = first.instance.pid.unwrap();
        let compositor_notes = notifications(&compositor);
        assert!(compositor_notes.contains(&Notification::Activate {
            pid,
            app_id: terminal.clone()
        }));
        // The home surface sees lifecycle events but never compositor directives.
        assert!(
            notifications(&home)
                .iter()
                .all(|n| matches!(n, Notification::App { .. }))
        );
    }

    #[test]
    fn crash_is_recorded_published_and_clears_association() {
        let h = harness();
        let home = subscribe(&h.service, ClientRole::Home);
        let terminal = id("org.ferrix.terminal");
        let instance = h.service.launch(&terminal).unwrap().instance;
        let pid = instance.pid.unwrap();
        h.tree.set_parent(pid + 100, pid);
        assert_eq!(
            h.service.surface_mapped(pid + 100, None, None),
            Some(terminal.clone())
        );

        assert!(
            h.runtime
                .exit(&terminal, ExitReason::Signaled { signal: 11 })
        );
        let crashed = wait_state(&h.service, &terminal, LifecycleState::Crashed);
        assert_eq!(crashed.exit, Some(ExitReason::Signaled { signal: 11 }));
        assert!(!crashed.surface_mapped);
        assert_eq!(h.service.foreground(), None);

        let events = captured_events(&h.records);
        assert!(events.contains(&"app-crashed".to_owned()), "{events:?}");
        let crash = h
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|r| r["event"] == "app-crashed")
            .cloned()
            .unwrap();
        assert_eq!(crash["signal"], 11);
        assert_eq!(crash["app"], "org.ferrix.terminal");

        let states = lifecycle_states(&home, &terminal);
        assert_eq!(states.last(), Some(&LifecycleState::Crashed));

        // The association is gone: a late surface report resolves to nothing,
        // and the compositor's pid no longer maps to the app.
        assert_eq!(h.service.surface_mapped(pid + 100, None, None), None);
        assert_eq!(h.service.surface_mapped(pid, None, None), None);

        // A relaunch after a crash is legal and gets a new instance number.
        let relaunched = h.service.launch(&terminal).unwrap();
        assert!(!relaunched.activated_existing);
        assert_eq!(relaunched.instance.instance, 2);
    }

    #[test]
    fn clean_exit_is_terminated_with_a_zero_code() {
        let h = harness();
        let terminal = id("org.ferrix.terminal");
        h.service.launch(&terminal).unwrap();
        assert!(h.runtime.exit(&terminal, ExitReason::Exited { code: 0 }));
        let done = wait_state(&h.service, &terminal, LifecycleState::Terminated);
        assert_eq!(done.exit, Some(ExitReason::Exited { code: 0 }));
        let events = captured_events(&h.records);
        assert!(events.contains(&"app-exited".to_owned()));
        assert!(!events.contains(&"app-crashed".to_owned()));
    }

    #[test]
    fn terminate_requests_a_stop_and_ends_terminated() {
        let h = harness();
        let terminal = id("org.ferrix.terminal");
        h.service.launch(&terminal).unwrap();
        let done = h.service.terminate(&terminal).unwrap();
        assert_eq!(done.state, LifecycleState::Terminated);
        assert_eq!(done.exit, Some(ExitReason::Signaled { signal: 15 }));
        assert!(h.runtime.last_terminated(&terminal));
        assert_eq!(
            h.service.terminate(&terminal),
            Err(AppError::NotRunning {
                id: terminal.clone()
            })
        );
        let events = captured_events(&h.records);
        assert!(events.contains(&"app-terminating".to_owned()));
        assert!(events.contains(&"app-exited".to_owned()));
        assert!(!events.contains(&"app-crashed".to_owned()));
    }

    #[test]
    fn surface_mapped_resolves_grandchildren_and_moves_to_foreground() {
        let h = harness();
        let subscriber = subscribe(&h.service, ClientRole::Home);
        let terminal = id("org.ferrix.terminal");
        let instance = h.service.launch(&terminal).unwrap().instance;
        let sandbox = instance.pid.unwrap();
        // appd -> sandbox (stage 1) -> init (stage 2) -> app
        let init = sandbox + 1;
        let app = sandbox + 2;
        h.tree.set_parent(init, sandbox);
        h.tree.set_parent(app, init);

        let resolved = h
            .service
            .surface_mapped(app, Some("org.ferrix.terminal"), Some("Terminal"));
        assert_eq!(resolved, Some(terminal.clone()));
        let current = h.service.instance(&terminal).unwrap();
        assert!(current.surface_mapped);
        assert_eq!(current.state, LifecycleState::Foreground);
        assert_eq!(h.service.foreground(), Some(terminal.clone()));
        let events = captured_events(&h.records);
        assert!(events.contains(&"app-surface".to_owned()));
        assert!(events.contains(&"app-foreground".to_owned()));
        assert!(!events.contains(&"app-id-mismatch".to_owned()));
        assert_eq!(
            lifecycle_states(&subscriber, &terminal),
            [LifecycleState::Starting, LifecycleState::Foreground]
        );

        // An unknown pid is reported, not associated.
        assert_eq!(
            h.service
                .surface_mapped(4242, Some("weston-terminal"), None),
            None
        );
        assert!(captured_events(&h.records).contains(&"app-surface-unknown".to_owned()));
    }

    #[test]
    fn app_id_hint_is_untrusted_metadata() {
        let h = harness();
        let terminal = id("org.ferrix.terminal");
        let pid = h.service.launch(&terminal).unwrap().instance.pid.unwrap();
        // The client claims to be home; the kernel pid says otherwise.
        assert_eq!(
            h.service.surface_mapped(pid, Some("org.ferrix.home"), None),
            Some(terminal.clone())
        );
        let records = h.records.lock().unwrap();
        let mismatch = records
            .iter()
            .find(|r| r["event"] == "app-id-mismatch")
            .expect("mismatch record");
        assert_eq!(mismatch["app"], "org.ferrix.terminal");
        assert_eq!(mismatch["reported"], "org.ferrix.home");
    }

    #[test]
    fn focus_changes_move_foreground_and_background() {
        let h = harness();
        let subscriber = subscribe(&h.service, ClientRole::Home);
        let home = id("org.ferrix.home");
        let terminal = id("org.ferrix.terminal");
        let home_pid = h.service.launch(&home).unwrap().instance.pid.unwrap();
        let terminal_pid = h.service.launch(&terminal).unwrap().instance.pid.unwrap();

        h.service.surface_mapped(home_pid, None, None);
        assert_eq!(
            h.service.instance(&home).unwrap().state,
            LifecycleState::Foreground
        );
        // Home is in front, so the terminal maps into the background until focused.
        h.service.surface_mapped(terminal_pid, None, None);
        assert_eq!(
            h.service.instance(&terminal).unwrap().state,
            LifecycleState::Background
        );

        assert_eq!(
            h.service.focus_changed(Some(terminal_pid)),
            Some(terminal.clone())
        );
        assert_eq!(
            h.service.instance(&home).unwrap().state,
            LifecycleState::Background
        );
        assert_eq!(
            h.service.instance(&terminal).unwrap().state,
            LifecycleState::Foreground
        );
        assert_eq!(h.service.foreground(), Some(terminal.clone()));

        assert_eq!(h.service.focus_changed(Some(home_pid)), Some(home.clone()));
        assert_eq!(
            h.service.instance(&home).unwrap().state,
            LifecycleState::Foreground
        );
        assert_eq!(
            h.service.instance(&terminal).unwrap().state,
            LifecycleState::Background
        );

        // Focus to nothing (or an unknown client) sends the foreground app back.
        assert_eq!(h.service.focus_changed(None), None);
        assert_eq!(
            h.service.instance(&home).unwrap().state,
            LifecycleState::Background
        );
        assert_eq!(h.service.foreground(), None);

        // Unmapping the last surface of the foreground app also clears it.
        h.service.focus_changed(Some(terminal_pid));
        assert_eq!(
            h.service.surface_unmapped(terminal_pid),
            Some(terminal.clone())
        );
        assert!(!h.service.instance(&terminal).unwrap().surface_mapped);
        assert_eq!(h.service.foreground(), None);

        let states = lifecycle_states(&subscriber, &terminal);
        assert!(states.contains(&LifecycleState::Foreground));
        assert!(states.contains(&LifecycleState::Background));
        let events = captured_events(&h.records);
        assert!(events.iter().filter(|e| *e == "app-foreground").count() >= 3);
        assert!(events.iter().filter(|e| *e == "app-background").count() >= 3);
    }

    #[test]
    fn home_crash_restarts_immediately_then_with_backoff() {
        let now = Arc::new(Mutex::new(Instant::now()));
        let clock_now = Arc::clone(&now);
        let h = harness_with_clock(Arc::new(move || *clock_now.lock().unwrap()));
        let home = id("org.ferrix.home");
        let first = h.service.launch(&home).unwrap().instance;
        assert_eq!(first.instance, 1);

        // First crash: restarted at once with a fresh instance number.
        h.runtime.exit(&home, ExitReason::Signaled { signal: 11 });
        assert!(wait_for(TIMEOUT, || h.runtime.launch_count() == 2));
        let second = wait_state(&h.service, &home, LifecycleState::Starting);
        assert_eq!(second.instance, 2);
        assert_eq!(h.service.pending_home_restart(), None);
        let restarting: Vec<_> = h
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "home-restarting")
            .cloned()
            .collect();
        assert_eq!(restarting.len(), 1);
        assert_eq!(restarting[0]["attempt"], 1);
        assert_eq!(restarting[0]["delay_ms"], 0);

        // Second crash shortly after: scheduled with a one second delay.
        *now.lock().unwrap() += Duration::from_secs(1);
        h.runtime.exit(&home, ExitReason::Exited { code: 3 });
        wait_state(&h.service, &home, LifecycleState::Crashed);
        let pending = h.service.pending_home_restart().expect("scheduled restart");
        assert_eq!(pending.attempt, 2);
        assert_eq!(pending.delay, Duration::from_secs(1));
        assert_eq!(h.runtime.launch_count(), 2);

        // Firing the schedule (as the sleeper thread would) relaunches home.
        h.service.run_pending_restart(pending.token());
        assert_eq!(h.runtime.launch_count(), 3);
        assert_eq!(h.service.pending_home_restart(), None);
        assert_eq!(
            h.service.instance(&home).unwrap().state,
            LifecycleState::Starting
        );
        // A stale token is ignored.
        h.service.run_pending_restart(pending.token());
        assert_eq!(h.runtime.launch_count(), 3);
    }

    #[test]
    fn home_restarts_are_abandoned_after_five_exits_in_a_minute() {
        let now = Arc::new(Mutex::new(Instant::now()));
        let clock_now = Arc::clone(&now);
        let h = harness_with_clock(Arc::new(move || *clock_now.lock().unwrap()));
        let home = id("org.ferrix.home");
        h.service.launch(&home).unwrap();

        for crash in 1..=5u32 {
            *now.lock().unwrap() += Duration::from_secs(1);
            let before = h.runtime.launch_count();
            h.runtime.exit(&home, ExitReason::Signaled { signal: 6 });
            if crash == 1 {
                assert!(wait_for(TIMEOUT, || h.runtime.launch_count() == before + 1));
            } else if crash < 5 {
                wait_state(&h.service, &home, LifecycleState::Crashed);
                let pending = h.service.pending_home_restart().expect("pending");
                assert_eq!(pending.attempt, crash);
                h.service.run_pending_restart(pending.token());
                assert_eq!(h.runtime.launch_count(), before + 1);
            } else {
                wait_state(&h.service, &home, LifecycleState::Crashed);
                assert_eq!(h.service.pending_home_restart(), None);
                assert_eq!(h.runtime.launch_count(), before);
            }
        }
        assert!(h.service.home_restart_abandoned());
        let events = captured_events(&h.records);
        assert!(events.contains(&"home-restart-abandoned".to_owned()));

        // An explicit Home request recovers and resets the budget.
        let outcome = h.service.home().unwrap();
        assert!(!outcome.activated_existing);
        assert!(!h.service.home_restart_abandoned());
        assert_eq!(
            h.service.instance(&home).unwrap().state,
            LifecycleState::Starting
        );
    }

    #[test]
    fn requested_home_stop_is_not_restarted_and_other_apps_never_are() {
        let h = harness();
        let home = id("org.ferrix.home");
        let terminal = id("org.ferrix.terminal");
        h.service.launch(&home).unwrap();
        h.service.launch(&terminal).unwrap();
        h.service.terminate(&home).unwrap();
        assert_eq!(
            h.service.instance(&home).unwrap().state,
            LifecycleState::Terminated
        );
        h.runtime
            .exit(&terminal, ExitReason::Signaled { signal: 9 });
        wait_state(&h.service, &terminal, LifecycleState::Crashed);
        assert_eq!(h.runtime.launch_count(), 2);
        assert_eq!(h.service.pending_home_restart(), None);
        assert!(!captured_events(&h.records).contains(&"home-restarting".to_owned()));

        // Home returns through the Home request, activating when it runs.
        let relaunch = h.service.home().unwrap();
        assert!(!relaunch.activated_existing);
        let again = h.service.home().unwrap();
        assert!(again.activated_existing);
    }

    #[test]
    fn autostart_launches_home_first_and_reports_failures() {
        let h = harness();
        h.runtime.fail_next(AppError::LaunchFailed {
            id: id("org.ferrix.home"),
            reason: "no such binary".to_owned(),
        });
        let results = h.service.autostart();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id("org.ferrix.home"));
        assert!(matches!(results[0].1, Err(AppError::LaunchFailed { .. })));
        assert!(captured_events(&h.records).contains(&"app-launch-failed".to_owned()));
        assert!(h.service.instance(&id("org.ferrix.home")).is_none());

        let results = h.service.autostart();
        assert!(results[0].1.is_ok());
    }

    #[test]
    fn catalog_announcements_reach_every_subscriber() {
        let h = harness();
        let a = subscribe(&h.service, ClientRole::Home);
        let b = subscribe(&h.service, ClientRole::Compositor);
        h.service.announce_catalog();
        for rx in [&a, &b] {
            assert_eq!(
                notifications(rx),
                [Notification::App {
                    event: AppEvent::CatalogChanged { apps: 5 }
                }]
            );
        }
        assert_eq!(h.service.subscriber_count(), 2);
        drop(a);
        h.service.announce_catalog();
        assert_eq!(h.service.subscriber_count(), 1);
    }
}
