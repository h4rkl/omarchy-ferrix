//! Guest-side home surface: the [`ClientHandler`] that folds ferrix-core
//! hardware events and `ferrix_appd` lifecycle notifications into the launcher,
//! sends hardware commands and launch requests, and shows crash notices.
//!
//! Every background source (the core event reader, the `ferrix_appd`
//! notification reader, and the telephony/network reconnectors) wakes the
//! UI through the runner's single [`Wake`] handle; `wake` drains whichever
//! channel has data. When `ferrix_appd` is not reachable the catalog comes
//! from the descriptor directory and a thread keeps retrying every second,
//! so the launcher is never blank and starts launching as soon as the
//! service appears. `CatalogChanged` re-fetches the catalog.
//!
//! Status bar and quick settings follow ADR-001/002: signal, registration,
//! SIM, and connectivity come from `ferrix-telephonyd` / `ferrix-networkd`
//! while they are reachable, the Wi-Fi toggle goes to `ferrix-networkd`
//! (`SetRadio`), and both fall back to the hardware-api summary and the
//! core's product command otherwise (`status.rs` holds the pure rule).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use ferrix_app_api::{AppEvent, AppId, LifecycleState};
use ferrix_app_protocol::{Client, ClientRole, Notification, Subscription};
use ferrix_hardware_api::{HardwareCommand, HardwareEvent, HardwareState, WifiState};
use ferrix_network_api::NetworkState;
use ferrix_network_protocol::{
    Client as NetClient, ClientKind as NetKind, Feed, Reconnector as NetReconnector,
};
use ferrix_target_profile::Profile;
use ferrix_telephony_api::RedactedTelephonyState;
use ferrix_telephony_protocol::{ClientKind as TeleKind, Reconnector as TeleReconnector, Update};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::clock::MinuteClock;
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape};
use ferrix_ui::keymap::Modifiers;

use crate::EVIDENCE;
use crate::catalog::{self, tiles_from_catalog};
use crate::interaction::{CrashNotice, Input, SystemAction, UiState};
use crate::screen;
use crate::status::{self, WifiRoute};

pub const APP_ID: &str = "org.ferrix.home";
const TITLE: &str = "ferrix \u{b7} home";
const APPD_TIMEOUT: Duration = Duration::from_secs(1);
const APPD_RETRY: Duration = Duration::from_secs(1);

/// State of the link to `ferrix_appd`.
enum AppdLink {
    /// A background thread is retrying; it delivers the subscription here.
    Connecting(Receiver<Subscription>),
    /// Subscribed: lifecycle notifications arrive here.
    Connected(Receiver<Notification>),
}

struct Home {
    ty: Type,
    clock: MinuteClock,
    hw: HardwareState,
    hw_events: Receiver<HardwareEvent>,
    core_socket: PathBuf,
    core_lost_reported: bool,
    ui: UiState,
    input: Input,
    wake: Wake,
    appd_socket: PathBuf,
    ferrix_appd: AppdLink,
    appd_unavailable_reported: bool,
    /// Request/reply connection for launches (Home role), opened lazily.
    launcher: Option<Client>,
    /// `home-ready` was emitted (first composed frame with a catalog).
    ready_reported: bool,
    /// `ferrix-telephonyd` snapshot while reachable (ADR-001 status source).
    telephony: Receiver<Update>,
    tele_state: Option<RedactedTelephonyState>,
    /// `ferrix-networkd` snapshot while reachable (ADR-002 status source).
    network: Receiver<Feed>,
    net_state: Option<NetworkState>,
    network_socket: PathBuf,
}

fn connect_subscription(socket: &Path) -> Result<Subscription, String> {
    Client::connect(socket, ClientRole::Home, APPD_TIMEOUT)?.subscribe()
}

/// Retries the subscription every second until it succeeds or the receiver
/// is dropped, then wakes the UI.
fn start_connecting(socket: PathBuf, wake: Wake) -> Receiver<Subscription> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("ferrix-appd-connect".to_owned())
        .spawn(move || {
            loop {
                match connect_subscription(&socket) {
                    Ok(subscription) => {
                        if tx.send(subscription).is_ok() {
                            wake();
                        }
                        return;
                    }
                    Err(_) => thread::sleep(APPD_RETRY),
                }
            }
        })
        .expect("spawn ferrix-appd connect thread");
    rx
}

pub fn run(
    profile: &Profile,
    core: ferrix_protocol::Subscription,
    appd_socket: &Path,
) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: TITLE.to_owned(),
        initial_size: (profile.display.width, profile.display.height),
        evidence: EVIDENCE.to_string(),
        first_frame_event: "basic-screen-frame".to_owned(),
        first_frame_extra: vec![("profile".to_owned(), profile.name.clone())],
    };
    let appd_socket = appd_socket.to_path_buf();
    ferrix_ui::client::run(config, move |wake| {
        // Redraw the status-bar clock at each local minute boundary.
        let tick = Arc::clone(&wake);
        ferrix_ui::clock::spawn_minute_ticker(move || tick());
        let ty = Type::load()?;
        let core_socket = core.socket().to_owned();
        let target = core.target.clone();
        let capabilities = core.capabilities.clone();
        let hw = core.state;
        let hw_events = core.spawn(Arc::clone(&wake));
        let (wifi_on, bt_on) = crate::ui_toggles(&hw);

        // Catalog: ferrix_appd when reachable, else descriptors plus a retry loop.
        let mut appd_unavailable_reported = false;
        let (tiles, ferrix_appd) = match connect_subscription(&appd_socket) {
            Ok(subscription) => {
                let tiles = tiles_from_catalog(&subscription.apps, &subscription.instances);
                emit(appd_connected_record(&appd_socket, tiles.len()));
                let (notifications, _reporter) = subscription.spawn(Arc::clone(&wake));
                (tiles, AppdLink::Connected(notifications))
            }
            Err(error) => {
                appd_unavailable_reported = true;
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-unavailable\",\"socket\":\"{}\",\"reason\":\"{}\",\"fallback\":\"descriptors\"}}",
                    json_escape(&appd_socket.display().to_string()),
                    json_escape(&error)
                ));
                let tiles = match catalog::load_descriptor_tiles() {
                    Ok((dir, tiles)) => {
                        emit(format!(
                            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"catalog-loaded\",\"source\":\"descriptors\",\"dir\":\"{}\",\"apps\":{}}}",
                            json_escape(&dir.display().to_string()),
                            tiles.len()
                        ));
                        tiles
                    }
                    Err(error) => {
                        emit(format!(
                            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"catalog-empty\",\"reason\":\"{}\"}}",
                            json_escape(&error)
                        ));
                        Vec::new()
                    }
                };
                let rx = start_connecting(appd_socket.clone(), Arc::clone(&wake));
                (tiles, AppdLink::Connecting(rx))
            }
        };
        let mut ui = UiState::new(wifi_on, bt_on, tiles);
        ui.brightness = hw.display.brightness_percent;
        ui.set_session(&target, capabilities);
        ui.tele = status::telephony_summary(None, &hw);
        ui.net = status::network_summary(None, &hw);

        let telephony = TeleReconnector::new(
            ferrix_telephony_protocol::socket_path(),
            TeleKind::Home,
            APPD_TIMEOUT,
        )
        .spawn(Arc::clone(&wake));
        let network_socket = ferrix_network_protocol::socket_path();
        let network = NetReconnector::new(network_socket.clone(), NetKind::Application)
            .with_timeout(APPD_TIMEOUT)
            .spawn(Arc::clone(&wake));

        Ok(Home {
            ty,
            clock: MinuteClock::now(),
            hw,
            hw_events,
            core_socket,
            core_lost_reported: false,
            ui,
            input: Input::default(),
            wake,
            appd_socket,
            ferrix_appd,
            appd_unavailable_reported,
            launcher: None,
            ready_reported: false,
            telephony,
            tele_state: None,
            network,
            net_state: None,
            network_socket,
        })
    })
}

fn appd_connected_record(socket: &Path, apps: usize) -> String {
    format!(
        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-connected\",\"socket\":\"{}\",\"apps\":{apps}}}",
        json_escape(&socket.display().to_string())
    )
}

impl Home {
    fn sync_controls(&mut self) {
        // The Wi-Fi toggle mirrors the network service when it is reachable.
        self.ui.wifi_on = match &self.net_state {
            Some(net) => net.radio.is_enabled(),
            None => self.hw.network.wifi != WifiState::Disabled,
        };
        self.ui.bluetooth_on = self.hw.bluetooth.enabled;
        self.ui.brightness = self.hw.display.brightness_percent;
        self.ui.tele = status::telephony_summary(self.tele_state.as_ref(), &self.hw);
        self.ui.net = status::network_summary(self.net_state.as_ref(), &self.hw);
    }

    /// Wi-Fi through `ferrix-networkd` (`SetRadio`); the authoritative state
    /// comes back on the subscription.
    fn set_wifi_via_networkd(&mut self, enabled: bool) {
        let outcome = NetClient::connect(&self.network_socket, NetKind::Application, APPD_TIMEOUT)
            .and_then(|mut client| client.set_radio(enabled));
        match outcome {
            Ok(()) => emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-command\",\"target\":\"networkd\",\"command\":\"set-radio\",\"enabled\":{enabled},\"status\":\"ok\"}}"
            )),
            Err(error) => {
                self.sync_controls();
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-command-failed\",\"target\":\"networkd\",\"error\":\"{}\"}}",
                    json_escape(&error.to_string())
                ));
            }
        }
    }

    /// Runs after every UI change: forwards queued hardware requests to the
    /// network service or the core, and launch requests to `ferrix_appd`.
    fn after_input(&mut self) {
        for action in std::mem::take(&mut self.ui.pending_system) {
            let command = match action {
                SystemAction::WifiEnabled(enabled) => {
                    match status::wifi_route(self.net_state.is_some()) {
                        WifiRoute::Networkd => {
                            self.set_wifi_via_networkd(enabled);
                            continue;
                        }
                        WifiRoute::Core => HardwareCommand::SetWifiEnabled { enabled },
                    }
                }
                SystemAction::BluetoothEnabled(enabled) => {
                    HardwareCommand::SetBluetoothEnabled { enabled }
                }
                SystemAction::Brightness(percent) => HardwareCommand::SetBrightness { percent },
            };
            match ferrix_protocol::command(&self.core_socket, command, Duration::from_secs(2)) {
                Ok(state) => {
                    self.hw = state;
                    self.sync_controls();
                }
                Err(error) => {
                    self.sync_controls();
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-command-failed\",\"error\":\"{}\"}}",
                        json_escape(&error)
                    ));
                }
            }
        }
        for id in std::mem::take(&mut self.ui.pending_launch) {
            self.launch(&id);
        }
    }

    fn launcher(&mut self) -> Option<&mut Client> {
        if self.launcher.is_none() {
            match Client::connect(&self.appd_socket, ClientRole::Home, APPD_TIMEOUT) {
                Ok(client) => self.launcher = Some(client),
                Err(error) => {
                    self.report_appd_unavailable(&error);
                    return None;
                }
            }
        }
        self.launcher.as_mut()
    }

    fn report_appd_unavailable(&mut self, reason: &str) {
        if self.appd_unavailable_reported {
            return;
        }
        self.appd_unavailable_reported = true;
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-unavailable\",\"socket\":\"{}\",\"reason\":\"{}\"}}",
            json_escape(&self.appd_socket.display().to_string()),
            json_escape(reason)
        ));
    }

    fn set_starting(&mut self, id: &AppId, starting: bool) {
        if let Some(index) = self.ui.tile_index(id) {
            self.ui.tiles[index].starting = starting;
        }
    }

    /// Sends `Request::Launch`; the tile stays in its starting state until a
    /// lifecycle notification reports the instance, or is cleared on failure.
    fn launch(&mut self, id: &AppId) {
        let outcome = match self.launcher() {
            Some(client) => client.launch(id),
            None => Err(Err("ferrix-appd unavailable".to_owned())),
        };
        match outcome {
            Ok((instance, activated_existing)) => {
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"home-launch\",\"app\":\"{id}\",\"status\":\"ok\",\"activated_existing\":{activated_existing},\"instance\":{},\"state\":\"{}\"}}",
                    instance.instance, instance.state
                ));
                if let Some(index) = self.ui.tile_index(id) {
                    let tile = &mut self.ui.tiles[index];
                    tile.running = instance.state.is_running();
                    tile.starting = instance.state == LifecycleState::Starting;
                }
                self.ui.crash_notice.take_if(|notice| notice.app == *id);
            }
            Err(Ok(error)) => {
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"home-launch\",\"app\":\"{id}\",\"status\":\"error\",\"activated_existing\":false,\"error\":\"{}\"}}",
                    json_escape(&error.to_string())
                ));
                self.set_starting(id, false);
                self.launch_notice(id, error.to_string());
            }
            Err(Err(transport)) => {
                // The connection is gone; the next launch reconnects.
                self.launcher = None;
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"home-launch\",\"app\":\"{id}\",\"status\":\"ferrix-appd-unavailable\",\"activated_existing\":false,\"error\":\"{}\"}}",
                    json_escape(&transport)
                ));
                self.set_starting(id, false);
                self.launch_notice(id, "ferrix-appd \u{b7} unavailable".to_owned());
            }
        }
    }

    /// Shows the low-noise "could not start" banner for a refused launch and
    /// records it. Reasons are typed `AppError` text, never process output.
    fn launch_notice(&mut self, id: &AppId, reason: String) {
        let name = self
            .ui
            .tile_index(id)
            .map(|index| self.ui.tiles[index].name.clone())
            .unwrap_or_else(|| id.to_string());
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"home-launch-notice\",\"app\":\"{id}\",\"reason\":\"{}\"}}",
            json_escape(&reason)
        ));
        self.ui.crash_notice = Some(CrashNotice::launch_failed(id.clone(), name, reason));
    }

    /// Re-fetches the catalog and instances over the request connection
    /// after `CatalogChanged`.
    fn refresh_catalog(&mut self) -> bool {
        let fetched = match self.launcher() {
            Some(client) => client
                .catalog()
                .and_then(|apps| client.instances().map(|i| (apps, i))),
            None => Err("ferrix-appd unavailable".to_owned()),
        };
        match fetched {
            Ok((apps, instances)) => {
                let tiles = tiles_from_catalog(&apps, &instances);
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"catalog-refreshed\",\"apps\":{}}}",
                    tiles.len()
                ));
                self.ui.set_tiles(tiles);
                true
            }
            Err(error) => {
                self.launcher = None;
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"catalog-refresh-failed\",\"error\":\"{}\"}}",
                    json_escape(&error)
                ));
                false
            }
        }
    }

    /// Folds one `ferrix_appd` notification into the tiles. Returns whether the
    /// launcher changed.
    fn apply_notification(&mut self, notification: Notification) -> bool {
        let instance = match notification {
            Notification::App {
                event: AppEvent::Lifecycle { instance },
            } => instance,
            Notification::App {
                event: AppEvent::CatalogChanged { .. },
            } => return self.refresh_catalog(),
            // `Activate` is a compositor directive.
            Notification::Activate { .. } => return false,
        };
        let Some(index) = self.ui.tile_index(&instance.app) else {
            return false;
        };
        let tile = &mut self.ui.tiles[index];
        tile.running = instance.state.is_running();
        tile.starting = instance.state == LifecycleState::Starting;
        let name = tile.name.clone();
        match instance.state {
            LifecycleState::Crashed => {
                let reason = instance
                    .exit
                    .map_or_else(|| "unknown reason".to_owned(), |exit| exit.to_string());
                emit(format!(
                    "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"home-crash-notice\",\"app\":\"{}\",\"reason\":\"{}\"}}",
                    instance.app,
                    json_escape(&reason)
                ));
                self.ui.crash_notice = Some(CrashNotice::crashed(instance.app, name, reason));
            }
            LifecycleState::Starting | LifecycleState::Foreground | LifecycleState::Background => {
                // A relaunch of the crashed app retires its notice.
                self.ui
                    .crash_notice
                    .take_if(|notice| notice.app == instance.app);
            }
            _ => {}
        }
        true
    }

    /// Drains the `ferrix_appd` link: a fresh subscription from the retry thread,
    /// or notifications (and the loss of the stream). The channel is read
    /// first and the launcher mutated afterwards.
    fn appd_wake(&mut self) -> bool {
        enum Step {
            Idle,
            Subscribed(Subscription),
            RetryThreadGone,
            Notifications(Vec<Notification>, bool),
        }
        let step = match &self.ferrix_appd {
            AppdLink::Connecting(incoming) => match incoming.try_recv() {
                Ok(subscription) => Step::Subscribed(subscription),
                Err(TryRecvError::Empty) => Step::Idle,
                Err(TryRecvError::Disconnected) => Step::RetryThreadGone,
            },
            AppdLink::Connected(notifications) => {
                let mut batch = Vec::new();
                let mut lost = false;
                loop {
                    match notifications.try_recv() {
                        Ok(notification) => batch.push(notification),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            lost = true;
                            break;
                        }
                    }
                }
                Step::Notifications(batch, lost)
            }
        };
        match step {
            Step::Idle => false,
            Step::Subscribed(subscription) => {
                let tiles = tiles_from_catalog(&subscription.apps, &subscription.instances);
                emit(appd_connected_record(&self.appd_socket, tiles.len()));
                self.ui.set_tiles(tiles);
                let (notifications, _reporter) = subscription.spawn(Arc::clone(&self.wake));
                self.ferrix_appd = AppdLink::Connected(notifications);
                self.appd_unavailable_reported = false;
                true
            }
            Step::RetryThreadGone => {
                // The retry thread cannot end without delivering; restart it
                // defensively.
                let rx = start_connecting(self.appd_socket.clone(), Arc::clone(&self.wake));
                self.ferrix_appd = AppdLink::Connecting(rx);
                false
            }
            Step::Notifications(batch, lost) => {
                let mut changed = false;
                for notification in batch {
                    changed |= self.apply_notification(notification);
                }
                if lost {
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-disconnected\",\"status\":\"stale-catalog\"}}"
                    ));
                    self.launcher = None;
                    for tile in &mut self.ui.tiles {
                        tile.running = false;
                        tile.starting = false;
                    }
                    let rx = start_connecting(self.appd_socket.clone(), Arc::clone(&self.wake));
                    self.ferrix_appd = AppdLink::Connecting(rx);
                    changed = true;
                }
                changed
            }
        }
    }

    /// Drains ferrix-core hardware events (or notices it went away). Every
    /// event is folded into the live state and reported.
    fn hardware_wake(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.hw_events.try_recv() {
                Ok(event) => {
                    event.apply_to(&mut self.hw);
                    self.sync_controls();
                    changed = true;
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-state\",\"source\":\"{}\",\"battery_percent\":{},\"charging\":{},\"wifi\":\"{}\",\"modem\":\"{}\",\"sim\":\"{}\",\"orientation\":\"{}\",\"brightness_percent\":{}}}",
                        event.name(),
                        self.hw.power.battery_percent,
                        self.hw.power.charging,
                        self.hw.network.wifi.name(),
                        self.hw.telephony.registration.name(),
                        self.hw.telephony.sim.name(),
                        self.hw.display.orientation.name(),
                        self.hw.display.brightness_percent,
                    ));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.core_lost_reported {
                        self.core_lost_reported = true;
                        emit(format!(
                            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"core-disconnected\",\"status\":\"stale-state\"}}"
                        ));
                    }
                    break;
                }
            }
        }
        changed
    }
}

impl Home {
    /// Drains the telephony and network reconnectors. A lost daemon drops
    /// its snapshot so the summaries fall back to the hardware-api matrix.
    fn services_wake(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.telephony.try_recv() {
                Ok(Update::Snapshot(view)) => {
                    self.tele_state = Some(view.redacted());
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"status-source\",\"service\":\"telephony\",\"status\":\"connected\"}}"
                    ));
                    changed = true;
                }
                Ok(Update::Event(event)) => {
                    if let Some(state) = self.tele_state.as_mut() {
                        event.apply_redacted(state);
                    }
                    changed = true;
                }
                Ok(Update::Disconnected) => {
                    self.tele_state = None;
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"status-source\",\"service\":\"telephony\",\"status\":\"fallback-hardware-api\"}}"
                    ));
                    changed = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        loop {
            match self.network.try_recv() {
                Ok(Feed::Snapshot { state, .. }) => {
                    self.net_state = Some(state);
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"status-source\",\"service\":\"network\",\"status\":\"connected\"}}"
                    ));
                    changed = true;
                }
                Ok(Feed::Event(event)) => {
                    if let Some(state) = self.net_state.as_mut() {
                        event.apply_to(state);
                    }
                    changed = true;
                }
                Ok(Feed::Disconnected { .. }) => {
                    if self.net_state.take().is_some() {
                        emit(format!(
                            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"status-source\",\"service\":\"network\",\"status\":\"fallback-hardware-api\"}}"
                        ));
                    }
                    changed = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if changed {
            self.sync_controls();
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-state\",\"source\":\"services\",\"signal_bars\":{},\"registration\":\"{}\",\"sim\":\"{}\",\"telephony_source\":\"{}\",\"reach\":\"{}\",\"network_source\":\"{}\"}}",
                self.ui.tele.bars,
                self.ui.tele.registration,
                self.ui.tele.sim,
                self.ui.tele.source.name(),
                self.ui.net.reach.name(),
                self.ui.net.source.name()
            ));
        }
        changed
    }
}

impl ClientHandler for Home {
    fn draw(&mut self, size: (u32, u32)) -> Canvas {
        let frame = screen::compose(
            &self.hw,
            self.clock.minute(),
            &self.ty,
            size.0,
            size.1,
            &self.ui,
        );
        // The launcher is hit-testable once a frame with the catalog is
        // composed; the runner's `basic-screen-frame` follows the commit.
        if !self.ready_reported && !self.ui.tiles.is_empty() {
            self.ready_reported = true;
            let catalog = match self.ferrix_appd {
                AppdLink::Connected(_) => "appd",
                AppdLink::Connecting(_) => "descriptors",
            };
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"home-ready\",\"tiles\":{},\"catalog\":\"{catalog}\",\"width\":{},\"height\":{}}}",
                self.ui.tiles.len(),
                size.0,
                size.1
            ));
        }
        frame
    }

    fn pointer_down(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.down(0, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn pointer_motion(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.motion(0, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn pointer_up(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let mut redraw = self.input.motion(0, position, &mut self.ui, size);
        redraw |= self.input.up(0, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn pointer_leave(&mut self) -> bool {
        self.input.cancel(&mut self.ui)
    }

    fn touch_down(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.down(id, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_motion(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.motion(id, position, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_up(&mut self, id: i32, size: (u32, u32)) -> bool {
        let redraw = self.input.up(id, &mut self.ui, size);
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_cancel(&mut self) -> bool {
        self.input.cancel(&mut self.ui)
    }

    fn key(&mut self, _code: u32, _mods: Modifiers) -> bool {
        // The launcher has no text input; hardware keys go to the focused app.
        false
    }

    fn wake(&mut self) -> bool {
        let hardware = self.hardware_wake();
        let ferrix_appd = self.appd_wake();
        let services = self.services_wake();
        let clock = self.clock.refresh();
        hardware || ferrix_appd || services || clock
    }

    fn page_name(&self) -> String {
        self.ui.page.name()
    }
}
