//! The application service: single-instance launch, supervision, surface
//! association, foreground tracking, and notification fan-out.
//!
//! All mutable state sits behind one mutex. Runtimes are `ferrix_app_runtime_api::Runtime`
//! trait objects composed by the binary; the library never names an
//! implementation. Each launched instance gets a waiter thread that polls the
//! runtime handle and posts the exit back, so a crash is observed, recorded,
//! and published without ever touching the compositor or the core.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_app_api::{
    APP_ID_ENV, APP_INSTANCE_ENV, APP_INTENT_ENV, APPD_SOCKET_ENV, AppCapability, AppError,
    AppEvent, AppId, AppIdentity, AppInstance, AppIntent, DEFAULT_APPD_SOCKET, ExitReason,
    GrantsPolicy, IntentKind, LifecycleState, RuntimeKind, SCHEMA_VERSION, TELEPHONY_APN_ENV,
};
use ferrix_app_protocol::{ClientRole, Notification, Reply};
use ferrix_app_runtime_api::{LaunchContext, RunningApp, Runtime, SurfaceMatch};
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
    /// Optional target-runtime APN for applications granted
    /// [`AppCapability::TelephonyData`]. Runtime launch boundaries remove the
    /// inherited process value and re-add it only for an authorized app.
    pub telephony_apn: Option<String>,
    /// Grace period between the polite and the forced stop.
    pub terminate_grace: Duration,
    /// How often waiter threads poll a running instance.
    pub poll_interval: Duration,
    pub restart: HomeRestartConfig,
    /// The authorization policy. The default is the **empty** policy: no
    /// application holds any effective capability until a row grants it.
    pub grants: GrantsPolicy,
    /// How long a shared-host instance (an Android task window) may be
    /// without any window before the instance ends as closed. A task that is
    /// recreated within this time (configuration change) keeps the instance;
    /// a process that dies within it is still classified as a crash.
    pub task_close_settle: Duration,
    /// How long the close gesture waits for a shared-host task window to go
    /// away after `xdg_toplevel.close` before force-stopping the app.
    pub task_close_grace: Duration,
}

impl Default for ServiceOptions {
    fn default() -> Self {
        Self {
            socket: PathBuf::from(DEFAULT_APPD_SOCKET),
            telephony_apn: None,
            terminate_grace: Duration::from_secs(2),
            poll_interval: Duration::from_millis(20),
            restart: HomeRestartConfig::default(),
            grants: GrantsPolicy::empty(),
            task_close_settle: Duration::from_secs(1),
            task_close_grace: Duration::from_secs(5),
        }
    }
}

/// One compositor toplevel: its client's kernel pid and the compositor's
/// per-toplevel id (absent from older compositors, which report per pid).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceKey {
    pub pid: u32,
    pub surface: Option<u64>,
}

/// What appd knows about a mapped toplevel.
#[derive(Clone, Debug, Default)]
struct SurfaceRecord {
    /// Untrusted xdg `app_id`; only meaningful together with a shared-host
    /// pid (see [`SurfaceMatch::SharedHost`]).
    app_id: Option<String>,
    title: Option<String>,
    owner: Option<AppId>,
    /// A `Withhold` directive was sent for it.
    withheld: bool,
}

/// Upper bound for remembered toplevels (unowned ones are dropped first).
const MAX_SURFACES: usize = 256;

/// Who a reported toplevel belongs to.
enum SurfaceOwner {
    App(AppId),
    /// Drawn by a shared runtime host but claimed by no running instance:
    /// another Android package, the setup wizard, the launcher, or the
    /// runtime's full UI. Never shown as, or attached to, an application.
    SharedUnclaimed,
    Unknown,
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
/// receive `Notification::Activate`; a stream resolved to an application
/// (`app`) also receives that application's `Notification::Intent`s.
pub struct Subscriber {
    pub role: ClientRole,
    pub app: Option<AppId>,
    pub sink: Box<dyn Write + Send>,
}

/// How an opened intent reached its handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentDelivery {
    /// The handler was launched with the intent in its environment.
    Launch,
    /// The running handler was activated and the intent pushed to it.
    Running,
    /// The running handler was activated but holds no subscription.
    ActivatedOnly,
}

impl IntentDelivery {
    pub fn name(self) -> &'static str {
        match self {
            IntentDelivery::Launch => "launch",
            IntentDelivery::Running => "running",
            IntentDelivery::ActivatedOnly => "activated-only",
        }
    }
}

struct Tracked {
    instance: AppInstance,
    handle: Option<Handle>,
    terminate_requested: bool,
    /// Mapped toplevels of this instance, oldest first; the last one is
    /// what an activation raises. The client pid of a toplevel is the
    /// Wayland client's, never the sandbox supervisor's.
    surfaces: Vec<SurfaceKey>,
    /// How compositor surfaces are matched to this instance (refreshed by
    /// the waiter thread from the runtime handle).
    matcher: SurfaceMatch,
    /// When the last window of a shared-host instance went away.
    window_closed_at: Option<Instant>,
    /// A launch/activation request that must be delivered once a surface pid
    /// is known. This closes the race where activation arrives before map.
    activation_pending: bool,
    /// Effective capabilities fixed at launch (`declared ∩ granted`); what
    /// `Identify` reports for every pid resolved to this instance.
    granted: BTreeSet<AppCapability>,
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
    /// Last toplevel the compositor reported focus on.
    focused_surface: Option<SurfaceKey>,
    /// Every mapped toplevel the compositor reported, owned or not.
    surfaces: BTreeMap<SurfaceKey, SurfaceRecord>,
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

    pub fn grants(&self) -> &GrantsPolicy {
        &self.options.grants
    }

    /// Resolves a kernel pid to the identity services enforce against: the
    /// running instance it belongs to (directly or through its ancestors),
    /// that descriptor's trust ring, and the effective capabilities fixed at
    /// launch. A pid that is unknown, or whose instance has exited, is
    /// [`AppIdentity::unknown`]; nothing a client claims about itself is
    /// consulted.
    pub fn identify(&self, pid: u32) -> AppIdentity {
        let mut state = self.lock_state();
        let Some(app) = self.resolve_pid(&mut state, pid) else {
            return AppIdentity::unknown();
        };
        let Some(tracked) = state
            .instances
            .get(&app)
            .filter(|t| t.instance.state.is_running())
        else {
            return AppIdentity::unknown();
        };
        let Some(entry) = self.catalog.get(&app) else {
            return AppIdentity::unknown();
        };
        AppIdentity {
            app: Some(app.clone()),
            trust: Some(entry.descriptor.trust),
            granted: tracked.granted.clone(),
        }
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
            ferrix_app_api::AppRole::Home => ClientRole::Home,
            ferrix_app_api::AppRole::Application => ClientRole::Application,
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
        let instance = self.launch_locked(&mut state, id, None)?;
        Ok(LaunchOutcome {
            instance,
            activated_existing: false,
        })
    }

    /// The handler of an intent kind: the first launchable catalog entry, in
    /// id order, that declares it.
    pub fn intent_handler(&self, kind: IntentKind) -> Option<AppId> {
        self.catalog
            .launchable()
            .find(|d| d.intents.contains(&kind))
            .map(|d| d.id.clone())
    }

    /// Opens the handler of `intent`: launches it with the intent in its
    /// environment, or activates the running instance and pushes the intent
    /// to that application's own subscriptions only. The number an intent
    /// carries is never recorded.
    pub fn open(self: &Arc<Self>, intent: &AppIntent) -> Result<LaunchOutcome, AppError> {
        intent.validate()?;
        let kind = intent.kind();
        let handler = self
            .intent_handler(kind)
            .ok_or(AppError::NoHandler { intent: kind })?;
        let mut state = self.lock_state();
        let (outcome, delivery) = match self.activate_locked(&mut state, &handler) {
            Some(instance) => {
                let reached = self.publish_intent(&handler, intent);
                (
                    LaunchOutcome {
                        instance,
                        activated_existing: true,
                    },
                    if reached {
                        IntentDelivery::Running
                    } else {
                        IntentDelivery::ActivatedOnly
                    },
                )
            }
            None => {
                let instance = self.launch_locked(&mut state, &handler, Some(intent))?;
                (
                    LaunchOutcome {
                        instance,
                        activated_existing: false,
                    },
                    IntentDelivery::Launch,
                )
            }
        };
        self.recorder.app_intent(kind, &handler, delivery);
        Ok(outcome)
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

    /// Terminates the foreground application after a trusted compositor
    /// dismiss gesture. Home is never dismissible and an empty foreground is
    /// a harmless no-op.
    ///
    /// A shared-host app (an Android task) is closed the way its runtime
    /// expects: every task window gets `xdg_toplevel.close` (Android removes
    /// the task) and the instance ends when the windows are gone; only a
    /// task that ignores the close is force-stopped after
    /// [`ServiceOptions::task_close_grace`].
    pub fn terminate_foreground(self: &Arc<Self>) -> Result<Option<AppInstance>, AppError> {
        let foreground = self.lock_state().foreground.clone();
        let Some(id) = foreground.filter(|id| !self.is_home(id)) else {
            return Ok(None);
        };
        if let Some(instance) = self.close_windows(&id) {
            return Ok(Some(instance));
        }
        self.terminate(&id).map(Some)
    }

    /// Asks the compositor to close every window of a running shared-host
    /// instance. `None` when the app is not such an instance (or has no
    /// window), so the caller terminates it instead.
    fn close_windows(self: &Arc<Self>, id: &AppId) -> Option<AppInstance> {
        let (instance, surfaces) = {
            let mut state = self.lock_state();
            let tracked = state
                .instances
                .get_mut(id)
                .filter(|t| t.instance.state.is_running() && t.matcher.is_shared_host())?;
            if tracked.surfaces.iter().all(|key| key.surface.is_none()) {
                return None;
            }
            tracked.terminate_requested = true;
            (tracked.instance.clone(), tracked.surfaces.clone())
        };
        self.recorder.emit(
            "app-closing",
            [
                ("app", id.as_str().into()),
                ("instance", instance.instance.into()),
                ("windows", surfaces.len().into()),
            ],
        );
        for key in &surfaces {
            if let Some(surface) = key.surface {
                self.publish(&Notification::Close {
                    pid: key.pid,
                    surface,
                });
            }
        }
        let service = Arc::clone(self);
        let id = id.clone();
        let number = instance.instance;
        let grace = self.options.task_close_grace;
        let _ = thread::Builder::new()
            .name("ferrix-appd-close".to_owned())
            .spawn(move || {
                thread::sleep(grace);
                let stuck = service.lock_state().instances.get(&id).is_some_and(|t| {
                    t.instance.instance == number
                        && t.instance.state.is_running()
                        && !t.surfaces.is_empty()
                });
                if stuck {
                    let _ = service.terminate(&id);
                }
            });
        Some(instance)
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

    /// A toplevel of `pid` committed its first buffer: the pid-only form of
    /// [`Service::map_surface`] (older compositors, tests).
    pub fn surface_mapped(
        &self,
        pid: u32,
        app_id: Option<&str>,
        title: Option<&str>,
    ) -> Option<AppId> {
        self.map_surface(pid, None, app_id, title)
    }

    /// A toplevel committed its first buffer. Returns the app it now
    /// belongs to, if any.
    ///
    /// Association policy:
    ///
    /// - A client pid that a running shared-host instance reports (the
    ///   Android runtime's composer connections, [`SurfaceMatch::SharedHost`])
    ///   is the trust anchor: the toplevel belongs to the running instance
    ///   whose expected `app_id` it carries, and to nobody else. It is never
    ///   resolved by pid or ancestry. A toplevel no running instance claims
    ///   (another Android package, the setup wizard, the launcher, the
    ///   runtime's full UI) is withheld from presentation (`Withhold`) and
    ///   remembered, so a matching instance that starts later adopts it.
    /// - Any other pid resolves through the launched process tree; its
    ///   `app_id` is untrusted metadata and only recorded.
    pub fn map_surface(
        &self,
        pid: u32,
        surface: Option<u64>,
        app_id: Option<&str>,
        title: Option<&str>,
    ) -> Option<AppId> {
        let mut state = self.lock_state();
        let key = SurfaceKey { pid, surface };
        remember_surface(&mut state, key, app_id, title);
        match self.surface_owner(&mut state, pid, app_id) {
            SurfaceOwner::App(app) => self.attach_surface(&mut state, &app, key).then_some(app),
            SurfaceOwner::SharedUnclaimed => {
                self.withhold(&mut state, key);
                None
            }
            SurfaceOwner::Unknown => {
                self.recorder.app_surface_unknown(pid, app_id);
                None
            }
        }
    }

    /// Every toplevel of `pid` went away: the pid-only form of
    /// [`Service::unmap_surface`].
    pub fn surface_unmapped(&self, pid: u32) -> Option<AppId> {
        self.unmap_surface(pid, None)
    }

    /// A mapped toplevel went away (`surface`), or every toplevel of `pid`
    /// (`None`). Returns the app that lost it. A shared-host instance whose
    /// last window is gone ends as closed once
    /// [`ServiceOptions::task_close_settle`] passes without a new window
    /// (see [`Service::task_window_closed`]); it is not a crash.
    pub fn unmap_surface(&self, pid: u32, surface: Option<u64>) -> Option<AppId> {
        let mut state = self.lock_state();
        let keys: Vec<SurfaceKey> = match surface {
            Some(_) => vec![SurfaceKey { pid, surface }],
            None => state
                .surfaces
                .keys()
                .filter(|key| key.pid == pid)
                .copied()
                .collect(),
        };
        let mut known = false;
        let mut owners: Vec<AppId> = Vec::new();
        for key in &keys {
            if let Some(record) = state.surfaces.remove(key) {
                known = true;
                if let Some(owner) = record.owner
                    && !owners.contains(&owner)
                {
                    owners.push(owner);
                }
            }
        }
        if !known {
            // Mapped before this appd started (no record): resolve the pid
            // as before, but never a shared-host pid.
            if self.is_shared_host_pid(&state, pid) {
                return None;
            }
            owners.extend(self.resolve_pid(&mut state, pid));
        }
        let gone = |key: &SurfaceKey| {
            if known {
                keys.contains(key)
            } else {
                key.pid == pid
            }
        };
        if state.focused_surface.is_some_and(|key| gone(&key)) {
            state.focused_surface = None;
        }
        let mut result = None;
        for app in owners {
            let Some(tracked) = state.instances.get_mut(&app) else {
                continue;
            };
            tracked.surfaces.retain(|key| !gone(key));
            result = Some(app.clone());
            if !tracked.surfaces.is_empty() {
                continue;
            }
            tracked.instance.surface_mapped = false;
            if tracked.matcher.is_shared_host() && tracked.instance.state.is_running() {
                tracked.window_closed_at = Some(Instant::now());
            }
            if state.foreground.as_ref() == Some(&app) {
                state.foreground = None;
                self.transition(&mut state, &app, LifecycleState::Background);
            } else {
                self.publish_instance(&state, &app);
            }
        }
        result
    }

    /// Focus moved to `pid` (or to nothing): the pid-only form of
    /// [`Service::focus_surface`].
    pub fn focus_changed(&self, pid: Option<u32>) -> Option<AppId> {
        self.focus_surface(pid, None)
    }

    /// Focus moved to toplevel `surface` of `pid` (or to nothing). Returns
    /// the app now in front. A shared-host pid only ever resolves through
    /// the toplevel it names.
    pub fn focus_surface(&self, pid: Option<u32>, surface: Option<u64>) -> Option<AppId> {
        let mut state = self.lock_state();
        state.focused_pid = pid;
        let key = pid.map(|pid| SurfaceKey { pid, surface });
        state.focused_surface = key;
        let target = key
            .and_then(|key| self.surface_app(&mut state, key))
            .filter(|app| {
                state
                    .instances
                    .get(app)
                    .is_some_and(|t| t.instance.state.is_running())
            });
        match target {
            Some(app) => {
                if let (Some(key), Some(tracked)) = (key, state.instances.get_mut(&app))
                    && let Some(index) = tracked.surfaces.iter().position(|k| *k == key)
                {
                    // The focused window is what the next activation raises.
                    let key = tracked.surfaces.remove(index);
                    tracked.surfaces.push(key);
                }
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

    /// A client asked (xdg_activation_v1) to raise its toplevel `surface`.
    /// Granted when the toplevel belongs to a running instance: appd raises
    /// that window (and the compositor reports the focus). A withheld,
    /// unclaimed toplevel is never raised. Returns the app raised.
    pub fn activation_requested(&self, pid: u32, surface: u64) -> Option<AppId> {
        let mut state = self.lock_state();
        let key = SurfaceKey {
            pid,
            surface: Some(surface),
        };
        let app = self.surface_app(&mut state, key).filter(|app| {
            state
                .instances
                .get(app)
                .is_some_and(|t| t.instance.state.is_running())
        });
        self.recorder.emit(
            "app-activation-requested",
            [
                ("pid", pid.into()),
                ("surface", surface.into()),
                (
                    "app",
                    app.as_ref()
                        .map_or(serde_json::Value::Null, |app| app.as_str().into()),
                ),
                ("granted", app.is_some().into()),
            ],
        );
        let app = app?;
        if let Some(tracked) = state.instances.get_mut(&app) {
            tracked.surfaces.retain(|k| *k != key);
            tracked.surfaces.push(key);
        }
        self.activate_locked(&mut state, &app);
        Some(app)
    }

    /// Records that the waiter saw the runtime's surface matcher change
    /// (the Android composer pids became known or changed) and re-runs the
    /// association of every unowned toplevel.
    pub fn update_matcher(&self, id: &AppId, number: u32, matcher: SurfaceMatch) {
        let mut state = self.lock_state();
        let Some(tracked) = state
            .instances
            .get_mut(id)
            .filter(|t| t.instance.instance == number && t.instance.state.is_running())
        else {
            return;
        };
        tracked.matcher = matcher;
        self.reconcile_shared(&mut state);
    }

    /// Whether the shared-host instance `number` of `id` has been without
    /// any window for at least [`ServiceOptions::task_close_settle`]. The
    /// waiter then ends it as closed.
    pub fn task_window_closed(&self, id: &AppId, number: u32) -> bool {
        let state = self.lock_state();
        state.instances.get(id).is_some_and(|t| {
            t.instance.instance == number
                && t.instance.state.is_running()
                && t.surfaces.is_empty()
                && t.window_closed_at
                    .is_some_and(|at| at.elapsed() >= self.options.task_close_settle)
        })
    }

    /// The owner of a known toplevel; for one appd has no record of, the
    /// process tree (never for a shared-host pid).
    fn surface_app(&self, state: &mut State, key: SurfaceKey) -> Option<AppId> {
        if let Some(owner) = state.surfaces.get(&key).and_then(|r| r.owner.clone()) {
            return Some(owner);
        }
        if self.is_shared_host_pid(state, key.pid) {
            return None;
        }
        self.resolve_pid(state, key.pid)
    }

    /// Whether a running instance reports `pid` as a shared-host client.
    fn is_shared_host_pid(&self, state: &State, pid: u32) -> bool {
        state
            .instances
            .values()
            .any(|t| t.instance.state.is_running() && t.matcher.hosts(pid))
    }

    fn surface_owner(&self, state: &mut State, pid: u32, app_id: Option<&str>) -> SurfaceOwner {
        let mut hosted = false;
        for (app, tracked) in &state.instances {
            if !tracked.instance.state.is_running() {
                continue;
            }
            if tracked.matcher.claims(pid, app_id) {
                return SurfaceOwner::App(app.clone());
            }
            hosted |= tracked.matcher.hosts(pid);
        }
        if hosted {
            return SurfaceOwner::SharedUnclaimed;
        }
        match self.resolve_pid(state, pid) {
            Some(app) => SurfaceOwner::App(app),
            None => SurfaceOwner::Unknown,
        }
    }

    /// Attaches toplevel `key` to the running instance `app`, moves a
    /// starting instance out of `Starting`, and delivers a pending
    /// activation. Returns `false` when `app` is not running.
    fn attach_surface(&self, state: &mut State, app: &AppId, key: SurfaceKey) -> bool {
        let (was_starting, activation_pending, in_front, shared) = {
            let Some(tracked) = state.instances.get_mut(app) else {
                return false;
            };
            if !tracked.instance.state.is_running() {
                return false;
            }
            tracked.surfaces.retain(|k| *k != key);
            tracked.surfaces.push(key);
            tracked.instance.surface_mapped = true;
            tracked.window_closed_at = None;
            (
                tracked.instance.state == LifecycleState::Starting,
                std::mem::take(&mut tracked.activation_pending),
                tracked.instance.state == LifecycleState::Foreground,
                tracked.matcher.is_shared_host(),
            )
        };
        let (app_id, title) = match state.surfaces.get_mut(&key) {
            Some(record) => {
                record.owner = Some(app.clone());
                record.withheld = false;
                (record.app_id.clone(), record.title.clone())
            }
            None => (None, None),
        };
        self.recorder
            .app_surface(app, key.pid, app_id.as_deref(), title.as_deref());
        if !shared
            && let Some(reported) = app_id.as_deref()
            && reported != app.as_str()
        {
            self.recorder.app_id_mismatch(app, key.pid, reported);
        }
        if was_starting {
            let focused = state.focused_surface == Some(key)
                || (!shared && state.focused_pid == Some(key.pid))
                || state.foreground.as_ref() == Some(app)
                || state.foreground.is_none();
            if focused {
                self.set_foreground(state, app);
            } else {
                self.transition(state, app, LifecycleState::Background);
            }
        } else {
            self.publish_instance(state, app);
        }
        // A shared-host window stays withheld by the compositor until an
        // activation names it, so a new window of the app in front (a task
        // Android recreated) is raised too.
        if activation_pending || (shared && in_front) {
            self.publish(&Notification::Activate {
                pid: key.pid,
                app_id: app.clone(),
                surface: key.surface,
            });
        }
        true
    }

    /// Tells the compositor not to present an unclaimed shared-host
    /// toplevel. Once per toplevel.
    fn withhold(&self, state: &mut State, key: SurfaceKey) {
        let Some(record) = state.surfaces.get_mut(&key) else {
            return;
        };
        if record.withheld {
            return;
        }
        record.withheld = true;
        self.recorder.emit(
            "app-surface-withheld",
            [
                ("pid", key.pid.into()),
                (
                    "surface",
                    key.surface.map_or(serde_json::Value::Null, Into::into),
                ),
                (
                    "app_id",
                    record
                        .app_id
                        .as_deref()
                        .map_or(serde_json::Value::Null, Into::into),
                ),
            ],
        );
        if let Some(surface) = key.surface {
            self.publish(&Notification::Withhold {
                pid: key.pid,
                surface,
            });
        }
    }

    /// Re-associates every unowned toplevel after a shared-host instance
    /// started or learned its client pids: claimed ones attach, hosted but
    /// unclaimed ones are withheld.
    fn reconcile_shared(&self, state: &mut State) {
        let unowned: Vec<(SurfaceKey, Option<String>)> = state
            .surfaces
            .iter()
            .filter(|(_, record)| record.owner.is_none())
            .map(|(key, record)| (*key, record.app_id.clone()))
            .collect();
        for (key, app_id) in unowned {
            let mut owner = None;
            let mut hosted = false;
            for (app, tracked) in &state.instances {
                if !tracked.instance.state.is_running() {
                    continue;
                }
                if tracked.matcher.claims(key.pid, app_id.as_deref()) {
                    owner = Some(app.clone());
                    break;
                }
                hosted |= tracked.matcher.hosts(key.pid);
            }
            match owner {
                Some(app) => {
                    self.attach_surface(state, &app, key);
                }
                None if hosted => self.withhold(state, key),
                None => {}
            }
        }
    }

    // -- subscribers --------------------------------------------------------

    /// Registers a subscriber and acknowledges on its own stream while
    /// holding the fan-out lock, so no notification precedes the `Ok`.
    pub fn subscribe(&self, role: ClientRole, sink: Box<dyn Write + Send>) -> io::Result<()> {
        self.subscribe_peer(role, None, sink)
    }

    /// [`Service::subscribe`] for a socket peer: the kernel pid is resolved
    /// to the application it belongs to, so intents reach only that app.
    pub fn subscribe_peer(
        &self,
        role: ClientRole,
        peer_pid: Option<u32>,
        mut sink: Box<dyn Write + Send>,
    ) -> io::Result<()> {
        let app = peer_pid.and_then(|pid| {
            let mut state = self.lock_state();
            self.resolve_pid(&mut state, pid)
        });
        let mut subscribers = lock(&self.subscribers);
        write_line(&mut *sink, &Reply::Ok)?;
        subscribers.push(Subscriber { role, app, sink });
        Ok(())
    }

    /// Pushes `intent` to the subscriptions of `app` only. Returns whether
    /// at least one stream took it.
    fn publish_intent(&self, app: &AppId, intent: &AppIntent) -> bool {
        let notification = Notification::Intent {
            app_id: app.clone(),
            intent: intent.clone(),
        };
        let mut reached = false;
        let mut subscribers = lock(&self.subscribers);
        subscribers.retain_mut(|subscriber| {
            if subscriber.app.as_ref() != Some(app) {
                return true;
            }
            let alive = write_line(&mut *subscriber.sink, &notification).is_ok();
            reached |= alive;
            alive
        });
        reached
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
    /// subscribers and `Intent` only its handler's streams. Dead streams are
    /// dropped.
    pub fn publish(&self, notification: &Notification) {
        if let Notification::Intent { app_id, intent } = notification {
            self.publish_intent(app_id, intent);
            return;
        }
        let compositor_only = matches!(
            notification,
            Notification::Activate { .. }
                | Notification::Withhold { .. }
                | Notification::Close { .. }
        );
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
        let surfaces = std::mem::take(&mut tracked.surfaces);
        let shared = tracked.matcher.is_shared_host();
        tracked.window_closed_at = None;
        tracked.activation_pending = false;
        tracked.handle = None;
        state.pids.retain(|_, app| app != id);
        if state.foreground.as_ref() == Some(id) {
            state.foreground = None;
        }
        // A shared-host window that outlives its instance belongs to nobody
        // now: it must not stay presented as this app.
        for key in surfaces {
            if let Some(record) = state.surfaces.get_mut(&key) {
                record.owner = None;
            }
            if shared {
                self.withhold(&mut state, key);
            }
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
                        let _ = self.launch_locked(&mut state, id, None);
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
        intent: Option<&AppIntent>,
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
        // Authorization is decided here, once, from the checked-in policy:
        // the runtime and every service see only the effective set.
        let granted = self.options.grants.effective(descriptor);
        let denied = self.options.grants.denied(descriptor);
        let mut env = vec![
            (APP_ID_ENV.to_owned(), id.to_string()),
            (APP_INSTANCE_ENV.to_owned(), number.to_string()),
            (
                APPD_SOCKET_ENV.to_owned(),
                self.options.socket.display().to_string(),
            ),
        ];
        // APN is a target-runtime setting, not application metadata. Expose
        // it only to callers with the explicit data capability, and never
        // record it in appd evidence or lifecycle records.
        if granted.contains(&AppCapability::TelephonyData)
            && let Some(apn) = self.options.telephony_apn.as_deref()
            && !apn.is_empty()
        {
            env.push((TELEPHONY_APN_ENV.to_owned(), apn.to_owned()));
        }
        // An intent may carry a phone number: it is handed to the handler
        // only and never appears in appd records.
        if let Some(intent) = intent {
            env.push((APP_INTENT_ENV.to_owned(), intent.to_json()));
        }
        let context = LaunchContext {
            instance: number,
            env,
            evidence: self.recorder.evidence().to_owned(),
            granted: granted.clone(),
        };
        let mut running = runtime
            .launch(descriptor, &entry.launch, &context)
            .inspect_err(|error| self.recorder.app_launch_failed(id, error))?;
        let pid = running.pid();
        let matcher = running.surface_match();
        let shared = matcher.is_shared_host();
        if !shared
            && let Some(pid) = pid
            && let Some(owner) = state.pids.get(&pid)
            && owner != id
        {
            let error = AppError::LaunchFailed {
                id: id.clone(),
                reason: format!(
                    "runtime pid {pid} is already owned by {owner}; shared runtime surfaces are unsupported"
                ),
            };
            // A process-tree runtime that reports another app's pid would
            // silently reassign that app's surfaces and authorization
            // identity; refuse the second owner. Runtimes that really share
            // a compositor client declare `SurfaceMatch::SharedHost`.
            let _ = running.terminate(self.options.terminate_grace);
            self.recorder.app_launch_failed(id, &error);
            return Err(error);
        }
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
        // A shared-host instance is never resolved by pid: its windows are
        // matched by (runtime-reported client pid, app_id) only, and its
        // processes get no identity of their own.
        if !shared && let Some(pid) = pid {
            state.pids.insert(pid, id.clone());
        }
        let handle: Handle = Arc::new(Mutex::new(running));
        state.instances.insert(
            id.clone(),
            Tracked {
                instance: instance.clone(),
                handle: Some(Arc::clone(&handle)),
                terminate_requested: false,
                surfaces: Vec::new(),
                matcher: matcher.clone(),
                window_closed_at: None,
                activation_pending: true,
                granted: granted.clone(),
            },
        );
        self.recorder
            .app_started(id, number, pid, &granted, &denied, &diagnostics);
        self.publish_instance(state, id);
        if shared {
            // A task window may already exist (Android kept the task while
            // no instance was running): adopt it now.
            self.reconcile_shared(state);
        }
        self.spawn_waiter(id.clone(), number, handle, matcher);
        Ok(instance)
    }

    /// Pushes `Activate` for a running app's most recent window; `None` when
    /// it is not running. Without a live window the activation is retained
    /// until one maps, and a shared-host app (an Android task that was
    /// closed or never appeared) is relaunched through its runtime so a
    /// window actually comes back.
    fn activate_locked(&self, state: &mut State, id: &AppId) -> Option<AppInstance> {
        let tracked = state.instances.get_mut(id)?;
        if !tracked.instance.state.is_running() {
            return None;
        }
        let instance = tracked.instance.clone();
        if let Some(key) = tracked.surfaces.last().copied() {
            self.publish(&Notification::Activate {
                pid: key.pid,
                app_id: id.clone(),
                surface: key.surface,
            });
            return Some(instance);
        }
        tracked.activation_pending = true;
        if tracked.matcher.is_shared_host()
            && let Some(handle) = tracked.handle.clone()
        {
            let result = lock(&handle).reopen();
            let (status, reason) = match &result {
                Ok(true) => ("relaunched", None),
                Ok(false) => ("unsupported", None),
                Err(error) => ("failed", Some(error.to_string())),
            };
            self.recorder.emit(
                "app-reopen",
                [
                    ("app", id.as_str().into()),
                    ("instance", instance.instance.into()),
                    ("status", status.into()),
                    (
                        "reason",
                        reason.map_or(serde_json::Value::Null, serde_json::Value::from),
                    ),
                ],
            );
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
    fn spawn_waiter(
        self: &Arc<Self>,
        id: AppId,
        number: u32,
        handle: Handle,
        mut matcher: SurfaceMatch,
    ) {
        let service = Arc::clone(self);
        let poll = self.options.poll_interval;
        let name = format!("ferrix-appd-wait-{id}");
        let waited = id.clone();
        let spawned = thread::Builder::new().name(name).spawn(move || {
            let id = waited;
            loop {
                let (polled, current) = {
                    let mut running = lock(&handle);
                    let polled = running.try_wait();
                    (polled, running.surface_match())
                };
                match polled {
                    Ok(Some(reason)) => {
                        service.post_exit(&id, number, reason, false);
                        return;
                    }
                    Ok(None) => {
                        if current != matcher {
                            matcher = current;
                            service.update_matcher(&id, number, matcher.clone());
                        }
                        if service.task_window_closed(&id, number) {
                            // The task window went away and stayed away while
                            // the process lived on: the user (or Android)
                            // closed the app. A clean end, not a crash.
                            service.recorder.emit(
                                "app-task-closed",
                                [("app", id.as_str().into()), ("instance", number.into())],
                            );
                            service.post_exit(&id, number, ExitReason::Exited { code: 0 }, true);
                            return;
                        }
                    }
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
            .name("ferrix-appd-home-restart".to_owned())
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
        let _ = self.launch_locked(&mut state, &home, None);
    }
}

impl PendingRestart {
    /// Token identifying this schedule, for `run_pending_restart`.
    pub fn token(&self) -> u64 {
        self.token
    }
}

/// Records (or refreshes) a mapped toplevel, keeping its owner. The registry
/// is bounded: past [`MAX_SURFACES`], unowned entries are dropped first.
fn remember_surface(state: &mut State, key: SurfaceKey, app_id: Option<&str>, title: Option<&str>) {
    let record = state.surfaces.entry(key).or_default();
    record.app_id = app_id.map(str::to_owned);
    record.title = title.map(str::to_owned);
    while state.surfaces.len() > MAX_SURFACES {
        let Some(victim) = state
            .surfaces
            .iter()
            .find(|(k, r)| r.owner.is_none() && **k != key)
            .map(|(k, _)| *k)
        else {
            break;
        };
        state.surfaces.remove(&victim);
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
    use ferrix_app_api::{AppDescriptor, AppRole, IconRef, RuntimeKind, TrustLevel};
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
            intents: Default::default(),
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
            descriptor(
                "org.example.android-two",
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
        harness_with(catalog(), GrantsPolicy::empty(), clock)
    }

    fn harness_with(catalog: Catalog, grants: GrantsPolicy, clock: Clock) -> Harness {
        harness_with_apn(catalog, grants, clock, None)
    }

    fn harness_with_apn(
        catalog: Catalog,
        grants: GrantsPolicy,
        clock: Clock,
        telephony_apn: Option<String>,
    ) -> Harness {
        let runtime = FakeRuntime::native();
        let tree = FakeProcessTree::default();
        let (recorder, records) = Recorder::capturing("mock");
        let options = ServiceOptions {
            socket: PathBuf::from("/tmp/ferrix-appd-test.sock"),
            poll_interval: Duration::from_millis(1),
            telephony_apn,
            grants,
            ..ServiceOptions::default()
        };
        let service = Service::new(
            catalog,
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
    fn shared_runtime_pid_cannot_be_reassigned_to_another_app() {
        let runtime = FakeRuntime::new(RuntimeKind::Android);
        let (recorder, records) = Recorder::capturing("mock");
        let service = Arc::new(Service::new(
            catalog(),
            vec![Box::new(runtime.clone())],
            Box::new(FakeProcessTree::default()),
            recorder,
            ServiceOptions::default(),
        ));
        let first = id("org.example.android");
        let second = id("org.example.android-two");
        let first_pid = service.launch(&first).unwrap().instance.pid.unwrap();
        runtime.set_next_pid(first_pid);

        let error = service.launch(&second).unwrap_err();
        assert!(matches!(
            error,
            AppError::LaunchFailed { reason, .. }
                if reason.contains("shared runtime surfaces are unsupported")
        ));
        assert!(runtime.last_terminated(&second));
        assert_eq!(service.identify(first_pid).app, Some(first));
        assert!(service.instance(&second).is_none());
        assert_eq!(
            captured_events(&records),
            [
                "app-launching",
                "app-started",
                "app-launching",
                "app-launch-failed"
            ]
        );
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
        assert!(env.contains(&(
            APPD_SOCKET_ENV.to_owned(),
            "/tmp/ferrix-appd-test.sock".to_owned()
        )));
        assert_eq!(
            captured_events(&h.records),
            ["app-launching", "app-started"]
        );
    }

    #[test]
    fn target_apn_reaches_only_a_data_granted_application() {
        let home = descriptor("org.example.home", RuntimeKind::Native, AppRole::Home, true);
        let mut data = descriptor(
            "org.example.data",
            RuntimeKind::Native,
            AppRole::Application,
            true,
        );
        data.capabilities = [AppCapability::TelephonyData].into_iter().collect();
        let ordinary = descriptor(
            "org.example.ordinary",
            RuntimeKind::Native,
            AppRole::Application,
            true,
        );
        let catalog = Catalog::from_descriptors([home, data, ordinary]).unwrap();
        let grants = GrantsPolicy::from_json(
            r#"{"schema_version":1,"grants":{"org.example.data":["telephony-data"],"org.example.ordinary":["telephony-data"]}}"#,
        )
        .unwrap();
        let h = harness_with_apn(
            catalog,
            grants,
            Arc::new(Instant::now),
            Some("internet.example".to_owned()),
        );

        h.service.launch(&id("org.example.data")).unwrap();
        h.service.launch(&id("org.example.ordinary")).unwrap();

        let data_env = h.runtime.last_env(&id("org.example.data")).unwrap();
        assert!(data_env.contains(&(TELEPHONY_APN_ENV.to_owned(), "internet.example".to_owned())));
        let ordinary_env = h.runtime.last_env(&id("org.example.ordinary")).unwrap();
        assert!(!ordinary_env.iter().any(|(key, _)| key == TELEPHONY_APN_ENV));
        assert!(
            h.records
                .lock()
                .unwrap()
                .iter()
                .all(|record| !record.to_string().contains("internet.example"))
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

        let supervisor_pid = first.instance.pid.unwrap();
        assert!(
            notifications(&compositor)
                .iter()
                .all(|notification| !matches!(notification, Notification::Activate { .. })),
            "activation waits for a compositor-visible surface pid"
        );
        let client_pid = supervisor_pid + 100;
        h.tree.set_parent(client_pid, supervisor_pid);
        assert_eq!(
            h.service
                .surface_mapped(client_pid, Some("org.ferrix.terminal"), Some("Terminal")),
            Some(terminal.clone())
        );
        let compositor_notes = notifications(&compositor);
        assert!(compositor_notes.contains(&Notification::Activate {
            pid: client_pid,
            app_id: terminal.clone(),
            surface: None,
        }));

        let third = h.service.launch(&terminal).unwrap();
        assert!(third.activated_existing);
        assert!(
            notifications(&compositor).contains(&Notification::Activate {
                pid: client_pid,
                app_id: terminal.clone(),
                surface: None,
            })
        );
        // The home surface sees lifecycle events but never compositor directives.
        assert!(
            notifications(&home)
                .iter()
                .all(|n| matches!(n, Notification::App { .. }))
        );
    }

    fn intent_harness() -> Harness {
        let home = descriptor("org.example.home", RuntimeKind::Native, AppRole::Home, true);
        let mut phone = descriptor(
            "org.example.phone",
            RuntimeKind::Native,
            AppRole::Application,
            true,
        );
        phone.intents = [IntentKind::Dial, IntentKind::ShowCall]
            .into_iter()
            .collect();
        // A later id that also declares dial: the first in id order wins.
        let mut second = descriptor(
            "org.example.zdialer",
            RuntimeKind::Native,
            AppRole::Application,
            true,
        );
        second.intents = [IntentKind::Dial].into_iter().collect();
        // A non-launchable entry never handles anything.
        let mut parked = descriptor(
            "org.example.amessages",
            RuntimeKind::Native,
            AppRole::Application,
            false,
        );
        parked.intents = [IntentKind::Compose].into_iter().collect();
        let catalog = Catalog::from_descriptors([home, phone, second, parked]).unwrap();
        harness_with(catalog, GrantsPolicy::empty(), Arc::new(Instant::now))
    }

    fn dial() -> AppIntent {
        AppIntent::Dial {
            number: "+15550100042".to_owned(),
        }
    }

    #[test]
    fn open_launches_the_handler_with_the_intent_and_never_records_the_number() {
        let h = intent_harness();
        let phone = id("org.example.phone");
        assert_eq!(
            h.service.intent_handler(IntentKind::Dial),
            Some(phone.clone())
        );
        assert_eq!(
            h.service.open(&AppIntent::Compose {
                number: "5550100".to_owned()
            }),
            Err(AppError::NoHandler {
                intent: IntentKind::Compose
            })
        );
        assert!(matches!(
            h.service.open(&AppIntent::Dial {
                number: "not a number".to_owned()
            }),
            Err(AppError::InvalidIntent { .. })
        ));
        let outcome = h.service.open(&dial()).unwrap();
        assert!(!outcome.activated_existing);
        assert_eq!(outcome.instance.app, phone);
        let env = h.runtime.last_env(&phone).unwrap();
        assert!(env.contains(&(APP_INTENT_ENV.to_owned(), dial().to_json())));
        let records = h.records.lock().unwrap().clone();
        let intent = records
            .iter()
            .find(|r| r["event"] == "app-intent")
            .expect("app-intent record");
        assert_eq!(intent["kind"], "dial");
        assert_eq!(intent["handler"], "org.example.phone");
        assert_eq!(intent["delivery"], "launch");
        assert!(
            records
                .iter()
                .all(|r| !r.to_string().contains("5550100042")),
            "the number never reaches appd records"
        );
        // An ordinary launch carries no intent.
        let home = id("org.example.home");
        h.service.launch(&home).unwrap();
        let env = h.runtime.last_env(&home).unwrap();
        assert!(!env.iter().any(|(key, _)| key == APP_INTENT_ENV));
    }

    #[test]
    fn open_on_a_running_handler_activates_it_and_pushes_only_to_its_streams() {
        let h = intent_harness();
        let compositor = subscribe(&h.service, ClientRole::Compositor);
        let home = id("org.example.home");
        let phone = id("org.example.phone");
        let home_pid = h.service.launch(&home).unwrap().instance.pid.unwrap();
        h.tree.set_parent(home_pid + 100, home_pid);
        h.service.surface_mapped(home_pid + 100, None, None);
        let phone_pid = h.service.launch(&phone).unwrap().instance.pid.unwrap();
        let phone_client = phone_pid + 100;
        h.tree.set_parent(phone_client, phone_pid);
        h.service.surface_mapped(phone_client, None, None);
        // Phone goes to the background behind Home.
        h.service.focus_changed(Some(home_pid + 100));
        assert_eq!(h.service.foreground(), Some(home.clone()));

        // Without a subscription the handler is only activated.
        let _ = notifications(&compositor);
        let outcome = h.service.open(&AppIntent::ShowCall).unwrap();
        assert!(outcome.activated_existing);
        assert!(
            notifications(&compositor).contains(&Notification::Activate {
                pid: phone_client,
                app_id: phone.clone(),
                surface: None,
            })
        );

        let (phone_sink, phone_rx) = channel_sink();
        h.service
            .subscribe_peer(
                ClientRole::Application,
                Some(phone_client),
                Box::new(phone_sink),
            )
            .unwrap();
        assert_eq!(phone_rx.recv().unwrap(), r#"{"reply":"ok"}"#);
        let (home_sink, home_rx) = channel_sink();
        h.service
            .subscribe_peer(ClientRole::Home, Some(home_pid + 100), Box::new(home_sink))
            .unwrap();
        assert_eq!(home_rx.recv().unwrap(), r#"{"reply":"ok"}"#);

        h.service.open(&dial()).unwrap();
        let pushed = Notification::Intent {
            app_id: phone.clone(),
            intent: dial(),
        };
        assert!(notifications(&phone_rx).contains(&pushed));
        assert!(
            !notifications(&home_rx).contains(&pushed),
            "only the handler"
        );
        assert!(!notifications(&compositor).contains(&pushed));
        // The compositor raises Phone over Home; focus makes it foreground.
        h.service.focus_changed(Some(phone_client));
        assert_eq!(h.service.foreground(), Some(phone));
        let deliveries: Vec<String> = h
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "app-intent")
            .map(|r| r["delivery"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(deliveries, ["activated-only", "running"]);
    }

    #[test]
    fn activation_uses_each_apps_own_mapped_client_pid() {
        let h = harness();
        let compositor = subscribe(&h.service, ClientRole::Compositor);
        let home = id("org.ferrix.home");
        let terminal = id("org.ferrix.terminal");

        let home_supervisor = h.service.launch(&home).unwrap().instance.pid.unwrap();
        let home_client = home_supervisor + 100;
        h.tree.set_parent(home_client, home_supervisor);
        h.service
            .surface_mapped(home_client, Some(home.as_str()), Some("Home"));

        let terminal_supervisor = h.service.launch(&terminal).unwrap().instance.pid.unwrap();
        let terminal_client = terminal_supervisor + 100;
        h.tree.set_parent(terminal_client, terminal_supervisor);
        h.service
            .surface_mapped(terminal_client, Some(terminal.as_str()), Some("Terminal"));
        let _ = notifications(&compositor);

        h.service.activate(&home).unwrap();
        h.service.activate(&terminal).unwrap();
        let activations = notifications(&compositor);
        assert!(activations.contains(&Notification::Activate {
            pid: home_client,
            app_id: home,
            surface: None,
        }));
        assert!(activations.contains(&Notification::Activate {
            pid: terminal_client,
            app_id: terminal,
            surface: None,
        }));
        assert!(activations.iter().all(|notification| match notification {
            Notification::Activate { pid, .. } => {
                *pid != home_supervisor && *pid != terminal_supervisor
            }
            Notification::App { .. }
            | Notification::Intent { .. }
            | Notification::Withhold { .. }
            | Notification::Close { .. } => true,
        }));
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
    fn dismiss_terminates_the_foreground_app_but_never_home() {
        let h = harness();
        let home = id("org.ferrix.home");
        let terminal = id("org.ferrix.terminal");

        let home_pid = h.service.launch(&home).unwrap().instance.pid.unwrap();
        h.service
            .surface_mapped(home_pid, Some(home.as_str()), None);
        h.service.focus_changed(Some(home_pid));
        assert_eq!(h.service.terminate_foreground().unwrap(), None);
        assert!(!h.runtime.last_terminated(&home));

        let terminal_pid = h.service.launch(&terminal).unwrap().instance.pid.unwrap();
        h.service
            .surface_mapped(terminal_pid, Some(terminal.as_str()), None);
        h.service.focus_changed(Some(terminal_pid));
        let closed = h
            .service
            .terminate_foreground()
            .unwrap()
            .expect("foreground app is dismissible");
        assert_eq!(closed.app, terminal);
        assert_eq!(closed.state, LifecycleState::Terminated);
        assert!(h.runtime.last_terminated(&closed.app));
        assert!(!h.runtime.last_terminated(&home));
    }

    #[test]
    fn surface_mapped_resolves_grandchildren_and_moves_to_foreground() {
        let h = harness();
        let subscriber = subscribe(&h.service, ClientRole::Home);
        let terminal = id("org.ferrix.terminal");
        let instance = h.service.launch(&terminal).unwrap().instance;
        let sandbox = instance.pid.unwrap();
        // ferrix_appd -> sandbox (stage 1) -> init (stage 2) -> app
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
                    event: AppEvent::CatalogChanged { apps: 6 }
                }]
            );
        }
        assert_eq!(h.service.subscriber_count(), 2);
        drop(a);
        h.service.announce_catalog();
        assert_eq!(h.service.subscriber_count(), 1);
    }

    // -- grants and identity ------------------------------------------------

    /// Terminal declares shell, background, and telephony-sms; the policy
    /// grants shell, background, and network-control (never declared).
    fn grants_harness() -> Harness {
        let mut home = descriptor("org.ferrix.home", RuntimeKind::Native, AppRole::Home, true);
        home.trust = TrustLevel::Privileged;
        home.capabilities = [AppCapability::CoreState, AppCapability::CoreControl]
            .into_iter()
            .collect();
        let mut terminal = descriptor(
            "org.ferrix.terminal",
            RuntimeKind::Native,
            AppRole::Application,
            true,
        );
        terminal.capabilities = [
            AppCapability::Shell,
            AppCapability::Background,
            AppCapability::TelephonySms,
        ]
        .into_iter()
        .collect();
        let catalog = Catalog::from_descriptors([home, terminal]).unwrap();
        let grants = GrantsPolicy::from_json(
            r#"{"schema_version":1,"grants":{"org.ferrix.terminal":["shell","background","network-control"]}}"#,
        )
        .unwrap();
        harness_with(catalog, grants, Arc::new(Instant::now))
    }

    fn started_record(records: &Captured, app: &str) -> serde_json::Value {
        records
            .lock()
            .unwrap()
            .iter()
            .find(|r| r["event"] == "app-started" && r["app"] == app)
            .cloned()
            .expect("app-started record")
    }

    #[test]
    fn effective_grants_are_declared_intersect_granted_and_reach_the_runtime() {
        let h = grants_harness();
        let terminal = id("org.ferrix.terminal");
        let sandbox_pid = h.service.launch(&terminal).unwrap().instance.pid.unwrap();

        let expected: BTreeSet<AppCapability> = [AppCapability::Shell, AppCapability::Background]
            .into_iter()
            .collect();
        // The runtime receives exactly the effective set.
        assert_eq!(h.runtime.last_granted(&terminal), Some(expected.clone()));
        // The record names both halves; the undeclared grant is nowhere.
        let started = started_record(&h.records, "org.ferrix.terminal");
        assert_eq!(
            started["granted"],
            serde_json::json!(["shell", "background"])
        );
        assert_eq!(
            started["denied_capabilities"],
            serde_json::json!(["telephony-sms"])
        );

        // Identify: the supervisor pid and a grandchild resolve the same way.
        let identity = h.service.identify(sandbox_pid);
        assert_eq!(identity.app, Some(terminal.clone()));
        assert_eq!(identity.trust, Some(TrustLevel::Untrusted));
        assert_eq!(identity.granted, expected);
        assert!(!identity.has(AppCapability::TelephonySms));
        assert!(!identity.has(AppCapability::NetworkControl));
        let init = sandbox_pid + 1;
        let app = sandbox_pid + 2;
        h.tree.set_parent(init, sandbox_pid);
        h.tree.set_parent(app, init);
        assert_eq!(h.service.identify(app), identity);
    }

    #[test]
    fn unknown_and_exited_pids_have_no_identity() {
        let h = grants_harness();
        assert_eq!(h.service.identify(4242), AppIdentity::unknown());

        let terminal = id("org.ferrix.terminal");
        let sandbox_pid = h.service.launch(&terminal).unwrap().instance.pid.unwrap();
        let app = sandbox_pid + 2;
        h.tree.set_parent(app, sandbox_pid);
        assert!(h.service.identify(app).is_known());

        assert!(
            h.runtime
                .exit(&terminal, ExitReason::Signaled { signal: 11 })
        );
        wait_state(&h.service, &terminal, LifecycleState::Crashed);
        assert_eq!(h.service.identify(sandbox_pid), AppIdentity::unknown());
        assert_eq!(h.service.identify(app), AppIdentity::unknown());

        // A relaunch is a fresh instance with a fresh identity.
        let relaunched = h.service.launch(&terminal).unwrap().instance.pid.unwrap();
        assert_ne!(relaunched, sandbox_pid);
        assert!(h.service.identify(relaunched).is_known());
        assert_eq!(h.service.identify(sandbox_pid), AppIdentity::unknown());
    }

    #[test]
    fn without_a_grants_row_even_a_privileged_app_gets_nothing() {
        let h = grants_harness();
        let home = id("org.ferrix.home");
        let pid = h.service.launch(&home).unwrap().instance.pid.unwrap();
        let identity = h.service.identify(pid);
        assert_eq!(identity.app, Some(home.clone()));
        assert_eq!(identity.trust, Some(TrustLevel::Privileged));
        assert!(identity.granted.is_empty());
        assert_eq!(h.runtime.last_granted(&home), Some(BTreeSet::new()));
        let started = started_record(&h.records, "org.ferrix.home");
        assert_eq!(started["granted"], serde_json::json!([]));
        assert_eq!(
            started["denied_capabilities"],
            serde_json::json!(["core-state", "core-control"])
        );
    }

    // -- shared-host (Android task window) association ---------------------

    /// Host pid of the container's Wayland client in these tests.
    const COMPOSER: u32 = 4242;

    struct AndroidHarness {
        service: Arc<Service>,
        native: FakeRuntime,
        android: FakeRuntime,
        tree: FakeProcessTree,
        records: Captured,
        compositor: Receiver<String>,
    }

    fn android_harness_with(pids: &[u32], settle: Duration) -> AndroidHarness {
        let native = FakeRuntime::native();
        let android = FakeRuntime::shared_host(RuntimeKind::Android, pids, "waydroid.");
        let tree = FakeProcessTree::default();
        let (recorder, records) = Recorder::capturing("mock");
        let options = ServiceOptions {
            poll_interval: Duration::from_millis(1),
            task_close_settle: settle,
            task_close_grace: Duration::from_millis(50),
            ..ServiceOptions::default()
        };
        let service = Arc::new(Service::new(
            catalog(),
            vec![Box::new(native.clone()), Box::new(android.clone())],
            Box::new(tree.clone()),
            recorder,
            options,
        ));
        let compositor = subscribe(&service, ClientRole::Compositor);
        AndroidHarness {
            service,
            native,
            android,
            tree,
            records,
            compositor,
        }
    }

    fn android_harness() -> AndroidHarness {
        android_harness_with(&[COMPOSER], Duration::from_millis(20))
    }

    fn activate_note(pid: u32, app: &AppId, surface: u64) -> Notification {
        Notification::Activate {
            pid,
            app_id: app.clone(),
            surface: Some(surface),
        }
    }

    #[test]
    fn task_windows_of_one_runtime_pid_are_matched_by_app_id() {
        let h = android_harness();
        let first = id("org.example.android");
        let second = id("org.example.android-two");
        h.service.launch(&first).unwrap();
        h.service.launch(&second).unwrap();
        // Several toplevels (and Wayland connections) from the same pid.
        assert_eq!(
            h.service.map_surface(
                COMPOSER,
                Some(1),
                Some("waydroid.org.example.android"),
                Some("First")
            ),
            Some(first.clone())
        );
        assert_eq!(
            h.service.map_surface(
                COMPOSER,
                Some(2),
                Some("waydroid.org.example.android-two"),
                Some("Second")
            ),
            Some(second.clone())
        );
        // Android's own windows belong to no Ferrix app and are withheld.
        for (surface, app_id) in [
            (3, Some("waydroid.org.lineageos.setupwizard")),
            (4, Some("Waydroid")),
            (5, None),
        ] {
            assert_eq!(
                h.service.map_surface(COMPOSER, Some(surface), app_id, None),
                None
            );
        }
        let notes = notifications(&h.compositor);
        assert!(notes.contains(&activate_note(COMPOSER, &first, 1)));
        assert!(notes.contains(&activate_note(COMPOSER, &second, 2)));
        for surface in [3, 4, 5] {
            assert!(notes.contains(&Notification::Withhold {
                pid: COMPOSER,
                surface
            }));
            assert!(!notes.iter().any(|n| matches!(
                n,
                Notification::Activate { surface: Some(s), .. } if *s == surface
            )));
        }

        // Activation raises each app's own task window.
        h.service.activate(&first).unwrap();
        h.service.activate(&second).unwrap();
        let notes = notifications(&h.compositor);
        assert_eq!(
            notes
                .iter()
                .filter(|n| matches!(n, Notification::Activate { .. }))
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                activate_note(COMPOSER, &first, 1),
                activate_note(COMPOSER, &second, 2)
            ]
        );

        // Keyboard focus on a task window brings exactly that app forward;
        // focus on an unclaimed window brings nobody forward.
        assert_eq!(
            h.service.focus_surface(Some(COMPOSER), Some(2)),
            Some(second.clone())
        );
        assert_eq!(h.service.foreground(), Some(second.clone()));
        assert_eq!(h.service.focus_surface(Some(COMPOSER), Some(3)), None);
        assert_eq!(h.service.foreground(), None);
        // A pid-only focus report for the shared pid is ambiguous: nobody.
        assert_eq!(h.service.focus_changed(Some(COMPOSER)), None);
        // The shared pid never identifies an application.
        assert_eq!(h.service.identify(COMPOSER), AppIdentity::unknown());
        assert!(captured_events(&h.records).contains(&"app-surface-withheld".to_owned()));
        assert!(!captured_events(&h.records).contains(&"app-id-mismatch".to_owned()));
    }

    #[test]
    fn an_android_app_id_from_a_non_runtime_pid_is_never_trusted() {
        let h = android_harness();
        let android = id("org.example.android");
        let terminal = id("org.ferrix.terminal");
        h.service.launch(&android).unwrap();
        // Some other client claims the Android app's window app_id.
        assert_eq!(
            h.service
                .map_surface(777, Some(9), Some("waydroid.org.example.android"), None),
            None
        );
        assert!(!h.service.instance(&android).unwrap().surface_mapped);
        // A native app's own window keeps belonging to the native app,
        // whatever app_id it claims.
        let supervisor = h.service.launch(&terminal).unwrap().instance.pid.unwrap();
        h.tree.set_parent(supervisor + 1, supervisor);
        assert_eq!(
            h.service.map_surface(
                supervisor + 1,
                Some(10),
                Some("waydroid.org.example.android"),
                None
            ),
            Some(terminal.clone())
        );
        assert!(!h.service.instance(&android).unwrap().surface_mapped);
        let notes = notifications(&h.compositor);
        assert!(!notes.iter().any(|n| matches!(
            n,
            Notification::Activate { app_id, .. } if *app_id == android
        )));
        assert!(
            !notes
                .iter()
                .any(|n| matches!(n, Notification::Withhold { .. })),
            "non-runtime windows are never withheld: {notes:?}"
        );
        assert!(captured_events(&h.records).contains(&"app-surface-unknown".to_owned()));
        assert_eq!(h.native.launch_count(), 1);
    }

    #[test]
    fn a_window_seen_before_the_runtime_reports_its_pid_is_adopted_later() {
        let h = android_harness_with(&[], Duration::from_millis(20));
        let android = id("org.example.android");
        h.service.launch(&android).unwrap();
        assert_eq!(
            h.service.map_surface(
                COMPOSER,
                Some(1),
                Some("waydroid.org.example.android"),
                None
            ),
            None
        );
        h.android.set_client_pids(&android, &[COMPOSER]);
        assert!(wait_for(TIMEOUT, || h
            .service
            .instance(&android)
            .is_some_and(|i| i.surface_mapped)));
        assert!(notifications(&h.compositor).contains(&activate_note(COMPOSER, &android, 1)));
    }

    #[test]
    fn a_withheld_task_window_is_adopted_when_its_app_launches() {
        let h = android_harness();
        let first = id("org.example.android");
        let second = id("org.example.android-two");
        h.service.launch(&second).unwrap();
        // Android shows a task of `first` while only `second` runs.
        assert_eq!(
            h.service.map_surface(
                COMPOSER,
                Some(7),
                Some("waydroid.org.example.android"),
                None
            ),
            None
        );
        assert!(
            notifications(&h.compositor).contains(&Notification::Withhold {
                pid: COMPOSER,
                surface: 7
            })
        );
        let launched = h.service.launch(&first).unwrap();
        assert!(!launched.activated_existing);
        assert!(h.service.instance(&first).unwrap().surface_mapped);
        assert!(notifications(&h.compositor).contains(&activate_note(COMPOSER, &first, 7)));
    }

    #[test]
    fn closing_the_task_ends_the_instance_cleanly_and_the_next_launch_relaunches() {
        let h = android_harness();
        let android = id("org.example.android");
        h.service.launch(&android).unwrap();
        h.service.map_surface(
            COMPOSER,
            Some(1),
            Some("waydroid.org.example.android"),
            None,
        );
        h.service.focus_surface(Some(COMPOSER), Some(1));
        assert_eq!(h.service.foreground(), Some(android.clone()));

        assert_eq!(
            h.service.unmap_surface(COMPOSER, Some(1)),
            Some(android.clone())
        );
        let ended = wait_state(&h.service, &android, LifecycleState::Terminated);
        assert_eq!(ended.exit, Some(ExitReason::Exited { code: 0 }));
        let events = captured_events(&h.records);
        assert!(events.contains(&"app-task-closed".to_owned()));
        assert!(!events.contains(&"app-crashed".to_owned()));
        assert!(!h.android.last_terminated(&android), "no force-stop");

        // Tapping the app again is a fresh runtime launch, not an
        // activation of a window that no longer exists.
        let again = h.service.launch(&android).unwrap();
        assert!(!again.activated_existing);
        assert_eq!(h.android.launch_count(), 2);
    }

    #[test]
    fn a_task_recreated_within_the_settle_time_keeps_its_instance() {
        let h = android_harness_with(&[COMPOSER], Duration::from_millis(300));
        let android = id("org.example.android");
        let number = h.service.launch(&android).unwrap().instance.instance;
        h.service.map_surface(
            COMPOSER,
            Some(1),
            Some("waydroid.org.example.android"),
            None,
        );
        h.service.unmap_surface(COMPOSER, Some(1));
        h.service.map_surface(
            COMPOSER,
            Some(2),
            Some("waydroid.org.example.android"),
            None,
        );
        std::thread::sleep(Duration::from_millis(400));
        let instance = h.service.instance(&android).unwrap();
        assert!(instance.state.is_running(), "{instance:?}");
        assert_eq!(instance.instance, number);
        assert!(instance.surface_mapped);
    }

    #[test]
    fn a_crash_while_the_task_window_goes_away_is_still_a_crash() {
        let h = android_harness_with(&[COMPOSER], Duration::from_millis(300));
        let android = id("org.example.android");
        h.service.launch(&android).unwrap();
        h.service.map_surface(
            COMPOSER,
            Some(1),
            Some("waydroid.org.example.android"),
            None,
        );
        h.service.unmap_surface(COMPOSER, Some(1));
        assert!(h.android.exit(&android, ExitReason::Exited { code: 1 }));
        wait_state(&h.service, &android, LifecycleState::Crashed);
    }

    #[test]
    fn activating_an_android_app_without_a_window_relaunches_it() {
        let h = android_harness();
        let android = id("org.example.android");
        let terminal = id("org.ferrix.terminal");
        h.service.launch(&android).unwrap();
        // Running, but no task window (it never appeared or was lost).
        let outcome = h.service.launch(&android).unwrap();
        assert!(outcome.activated_existing);
        assert_eq!(h.android.reopen_count(&android), 1);
        h.service.activate(&android).unwrap();
        assert_eq!(h.android.reopen_count(&android), 2);
        assert!(captured_events(&h.records).contains(&"app-reopen".to_owned()));
        // The window that then maps receives the retained activation.
        h.service.map_surface(
            COMPOSER,
            Some(3),
            Some("waydroid.org.example.android"),
            None,
        );
        assert!(notifications(&h.compositor).contains(&activate_note(COMPOSER, &android, 3)));
        // With a live window, activation raises it and does not relaunch.
        h.service.activate(&android).unwrap();
        assert_eq!(h.android.reopen_count(&android), 2);

        // Native apps own their windows: no relaunch while one is pending.
        h.service.launch(&terminal).unwrap();
        h.service.activate(&terminal).unwrap();
        assert_eq!(h.native.reopen_count(&terminal), 0);
    }

    #[test]
    fn the_close_gesture_closes_task_windows_instead_of_force_stopping() {
        let h = android_harness();
        let android = id("org.example.android");
        h.service.launch(&android).unwrap();
        h.service.map_surface(
            COMPOSER,
            Some(1),
            Some("waydroid.org.example.android"),
            None,
        );
        h.service.focus_surface(Some(COMPOSER), Some(1));
        let _ = notifications(&h.compositor);

        let closing = h.service.terminate_foreground().unwrap().unwrap();
        assert!(closing.state.is_running());
        assert!(notifications(&h.compositor).contains(&Notification::Close {
            pid: COMPOSER,
            surface: 1
        }));
        assert!(!h.android.last_terminated(&android));
        // Android removes the task: the window goes away, the instance ends.
        h.service.unmap_surface(COMPOSER, Some(1));
        let ended = wait_state(&h.service, &android, LifecycleState::Terminated);
        assert_eq!(ended.exit, Some(ExitReason::Exited { code: 0 }));
        assert!(!h.android.last_terminated(&android));
    }

    #[test]
    fn a_task_that_ignores_close_is_force_stopped_after_the_grace() {
        let h = android_harness();
        let android = id("org.example.android");
        h.service.launch(&android).unwrap();
        h.service.map_surface(
            COMPOSER,
            Some(1),
            Some("waydroid.org.example.android"),
            None,
        );
        h.service.focus_surface(Some(COMPOSER), Some(1));
        h.service.terminate_foreground().unwrap();
        wait_state(&h.service, &android, LifecycleState::Terminated);
        assert!(h.android.last_terminated(&android));
    }

    #[test]
    fn activation_requests_raise_only_claimed_task_windows() {
        let h = android_harness();
        let android = id("org.example.android");
        h.service.launch(&android).unwrap();
        h.service.map_surface(
            COMPOSER,
            Some(1),
            Some("waydroid.org.example.android"),
            None,
        );
        h.service.map_surface(
            COMPOSER,
            Some(2),
            Some("waydroid.org.lineageos.setupwizard"),
            None,
        );
        let _ = notifications(&h.compositor);
        assert_eq!(
            h.service.activation_requested(COMPOSER, 1),
            Some(android.clone())
        );
        assert_eq!(
            notifications(&h.compositor),
            vec![activate_note(COMPOSER, &android, 1)]
        );
        assert_eq!(h.service.activation_requested(COMPOSER, 2), None);
        assert_eq!(h.service.activation_requested(COMPOSER, 99), None);
        assert!(notifications(&h.compositor).is_empty());
    }

    #[test]
    fn a_window_outliving_its_android_instance_is_withheld() {
        let h = android_harness();
        let android = id("org.example.android");
        h.service.launch(&android).unwrap();
        h.service.map_surface(
            COMPOSER,
            Some(1),
            Some("waydroid.org.example.android"),
            None,
        );
        let _ = notifications(&h.compositor);
        assert!(h.android.exit(&android, ExitReason::Exited { code: 1 }));
        wait_state(&h.service, &android, LifecycleState::Crashed);
        assert!(
            notifications(&h.compositor).contains(&Notification::Withhold {
                pid: COMPOSER,
                surface: 1
            })
        );
    }
}
