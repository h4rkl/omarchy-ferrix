//! Smithay compositor core shared by the DRM (QEMU guest) and nested backends.
//!
//! Policy lives in the pure `layout` module; this module wires Smithay's
//! Wayland globals (compositor, xdg-shell, wlr-layer-shell, shm, seat, output,
//! data device) to that policy: the newest mapped toplevel is fullscreen and
//! focused, layer surfaces are reserved for status/launcher chrome, and every
//! input event is routed through one `wl_seat` (keyboard + pointer + touch).
//!
//! Each toplevel is tagged with its client's kernel-reported pid so the
//! application service (`ferrix_appd`, see `appd_link`) can associate surfaces with
//! the processes it supervises; the bottom-edge home gesture (`gesture`) is
//! recognised before input reaches any client.

pub mod appd_link;
pub mod input;
pub mod render;
#[cfg(feature = "udev")]
mod scanout;
#[cfg(feature = "udev")]
pub mod udev;
#[cfg(feature = "nested")]
pub mod winit;

use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, TryRecvError},
    },
    time::Duration,
};

use ferrix_hardware_api::{HardwareEvent, HardwareState};
use ferrix_protocol::Subscription;

use smithay::{
    backend::{
        allocator::{Buffer, dmabuf::Dmabuf, format::FormatSet},
        input::TouchSlot,
        renderer::utils::{on_commit_buffer_handler, with_renderer_surface_state},
    },
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_layer_shell,
    delegate_output, delegate_seat, delegate_shm, delegate_xdg_shell,
    desktop::{
        LayerSurface, PopupKind, PopupManager, Window, WindowSurfaceType, find_popup_root_surface,
        get_popup_toplevel_coords, layer_map_for_output, utils::send_frames_surface_tree,
    },
    input::{Seat, SeatHandler, SeatState, keyboard::XkbConfig, pointer::CursorImageStatus},
    output::Output,
    reexports::{
        calloop::{
            EventLoop, Interest, LoopHandle, Mode as CalloopMode, PostAction, generic::Generic,
            ping::make_ping,
        },
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Rectangle, SERIAL_COUNTER, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent,
            is_sync_subsurface, with_states,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        output::{OutputHandler, OutputManagerState},
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
            },
        },
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
    },
};

use crate::{
    Failure,
    appd_link::AppdLink,
    config::ShellConfig,
    evidence::{self, Evidence, Value},
    gesture::EdgeGesture,
    layout::{OutputLayout, Rotation, WindowStack},
};

/// Which contact currently owns the grabbed home gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureSource {
    /// Pointer left button (virtio-tablet in QEMU, mouse when nested).
    Pointer,
    /// One touch slot; other slots keep reaching clients.
    Touch(TouchSlot),
}

impl GestureSource {
    pub fn label(self) -> &'static str {
        match self {
            GestureSource::Pointer => "pointer",
            GestureSource::Touch(_) => "touch",
        }
    }
}

pub fn run_udev(
    config: ShellConfig,
    plan: OutputLayout,
    evidence: Evidence,
    core: Subscription,
) -> Result<(), Failure> {
    #[cfg(feature = "udev")]
    {
        udev::run(config, plan, evidence, core)
    }
    #[cfg(not(feature = "udev"))]
    {
        let _ = (config, plan, evidence, core);
        Err((
            "feature",
            "ferrix-shell was built without the `udev` feature".to_owned(),
        ))
    }
}

pub fn run_nested(
    config: ShellConfig,
    plan: OutputLayout,
    evidence: Evidence,
    core: Subscription,
) -> Result<(), Failure> {
    #[cfg(feature = "nested")]
    {
        winit::run(config, plan, evidence, core)
    }
    #[cfg(not(feature = "nested"))]
    {
        let _ = (config, plan, evidence, core);
        Err((
            "feature",
            "ferrix-shell was built without the `nested` feature".to_owned(),
        ))
    }
}

/// Install a stderr tracing subscriber (`RUST_LOG` overrides, default `info`).
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

pub fn rotation_to_transform(rotation: Rotation) -> Transform {
    match rotation {
        Rotation::Normal => Transform::Normal,
        Rotation::Rotate90 => Transform::_90,
        Rotation::Rotate180 => Transform::_180,
        Rotation::Rotate270 => Transform::_270,
    }
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// Compositor state: one output, one seat, a fullscreen toplevel stack.
pub struct Shell {
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, Shell>,
    pub clock: Clock<Monotonic>,
    pub evidence: Evidence,
    pub config: ShellConfig,
    pub plan: OutputLayout,
    pub output: Output,
    pub socket_name: String,
    pub running: bool,
    hardware: HardwareState,
    hardware_events: Receiver<HardwareEvent>,
    core_lost_reported: bool,
    /// Link to the application service; `None` with `--no-ferrix_appd`.
    ferrix_appd: Option<AppdLink>,
    /// Bottom-edge home gesture recogniser and the contact that owns it.
    pub gesture: EdgeGesture,
    pub gesture_source: Option<GestureSource>,

    pub windows: WindowStack<Window>,
    /// Stable damage-tracking ids for the rows of the compositor's home
    /// indicator (`OutputLayout::system_pill_rows`).
    pub pill_ids: Vec<smithay::backend::renderer::element::Id>,
    pub popups: PopupManager,
    pub cursor_status: CursorImageStatus,
    pub pointer_location: Point<f64, Logical>,
    redraw_requested: bool,
    /// The focused fullscreen surface changed. DRM buffer-age damage alone
    /// cannot reliably describe that scene replacement, so the backend must
    /// invalidate its damage history before presenting the next frame.
    full_redraw_requested: bool,
    mapped_clients: usize,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    /// zwp_linux_dmabuf_v1, published by the backend once its renderer is up
    /// (`enable_dmabuf`); clients that render on a GPU (Waydroid's
    /// hwcomposer) hand over linear buffers instead of CPU-copying into SHM.
    dmabuf_state: DmabufState,
    dmabuf_global: Option<DmabufGlobal>,
    dmabuf_formats: FormatSet,
    /// Owns the wl_output/xdg_output globals; kept alive for the shell's lifetime.
    #[allow(dead_code)]
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Shell>,
    pub data_device_state: DataDeviceState,
    pub seat: Seat<Shell>,
}

impl Shell {
    /// Create the globals, the listening socket and the seat. The caller has
    /// already created the `Output` (the backend knows the connector) and is
    /// responsible for its mode; `Shell::new` publishes its global.
    #[allow(clippy::too_many_arguments)] // one call site per backend; a builder would only hide the wiring
    pub fn new(
        display: &mut Display<Shell>,
        event_loop: &EventLoop<'static, Shell>,
        config: ShellConfig,
        plan: OutputLayout,
        evidence: Evidence,
        output: Output,
        seat_name: &str,
        core: Subscription,
    ) -> Result<Self, Failure> {
        let dh = display.handle();
        let compositor_state = CompositorState::new::<Shell>(&dh);
        // Fullscreen is the only window-management capability this shell offers.
        let xdg_shell_state = XdgShellState::new_with_capabilities::<Shell>(
            &dh,
            [xdg_toplevel::WmCapabilities::Fullscreen],
        );
        let layer_shell_state = WlrLayerShellState::new::<Shell>(&dh);
        let shm_state = ShmState::new::<Shell>(&dh, Vec::new());
        let dmabuf_state = DmabufState::new();
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Shell>(&dh);
        let data_device_state = DataDeviceState::new::<Shell>(&dh);
        let mut seat_state = SeatState::new();
        let mut seat: Seat<Shell> = seat_state.new_wl_seat(&dh, seat_name);
        // The QEMU virt machine always carries virtio-keyboard and virtio-tablet,
        // and touch is the product's primary input, so all three capabilities
        // are advertised up front rather than on libinput hot-plug.
        seat.add_keyboard(XkbConfig::default(), 200, 25)
            .map_err(|e| ("keyboard", format!("xkb keymap: {e}")))?;
        seat.add_pointer();
        seat.add_touch();
        let _ = output.create_global::<Shell>(&dh);

        let loop_handle = event_loop.handle();
        let socket = ListeningSocketSource::with_name(&config.socket_name)
            .map_err(|e| ("socket", format!("cannot bind {}: {e}", config.socket_name)))?;
        let socket_name = socket.socket_name().to_string_lossy().into_owned();
        loop_handle
            .insert_source(socket, |stream, _, shell| {
                if let Err(e) = shell
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    tracing::warn!("failed to insert client: {e}");
                }
            })
            .map_err(|e| ("socket", format!("event loop: {e}")))?;
        // Wake the loop whenever clients wrote; the backend loop drains the
        // display right after every dispatch (`Shell::dispatch_clients`).
        let poll_fd = display
            .backend()
            .poll_fd()
            .try_clone_to_owned()
            .map_err(|e| ("display", format!("poll fd: {e}")))?;
        loop_handle
            .insert_source(
                Generic::new(poll_fd, Interest::READ, CalloopMode::Level),
                |_, _, _| Ok(PostAction::Continue),
            )
            .map_err(|e| ("display", format!("event loop: {e}")))?;

        let hardware = core.state;
        let (core_ping, core_source) = make_ping().map_err(|e| ("core", e.to_string()))?;
        loop_handle
            .insert_source(core_source, |_, _, shell| shell.hardware_wake())
            .map_err(|e| ("core", format!("event source: {e}")))?;
        let hardware_events = core.spawn(Arc::new(move || core_ping.ping()));

        // ferrix_appd may start later or restart; the link retries in the background
        // and wakes the loop through its own ping. Nothing here waits for it.
        let ferrix_appd = match &config.appd_socket {
            Some(path) => {
                let (appd_ping, appd_source) =
                    make_ping().map_err(|e| ("ferrix-appd", e.to_string()))?;
                loop_handle
                    .insert_source(appd_source, |_, _, shell| shell.appd_wake())
                    .map_err(|e| ("ferrix-appd", format!("event source: {e}")))?;
                Some(AppdLink::start(
                    Path::new(path),
                    Arc::new(move || appd_ping.ping()),
                ))
            }
            None => None,
        };

        Ok(Self {
            display_handle: dh,
            loop_handle,
            clock: Clock::new(),
            evidence,
            config,
            plan,
            output,
            socket_name,
            running: true,
            hardware,
            hardware_events,
            core_lost_reported: false,
            ferrix_appd,
            gesture: EdgeGesture::new(),
            gesture_source: None,
            windows: WindowStack::new(),
            // One id per merged pill row: 32 covers a 1080 px basis (about a
            // dozen rows) with room to spare; rows past the last id are not drawn.
            pill_ids: (0..32)
                .map(|_| smithay::backend::renderer::element::Id::new())
                .collect(),
            popups: PopupManager::default(),
            cursor_status: CursorImageStatus::default_named(),
            pointer_location: Point::from((0.0, 0.0)),
            redraw_requested: true,
            full_redraw_requested: false,
            mapped_clients: 0,
            compositor_state,
            xdg_shell_state,
            layer_shell_state,
            shm_state,
            dmabuf_state,
            dmabuf_global: None,
            dmabuf_formats: FormatSet::default(),
            output_manager_state,
            seat_state,
            data_device_state,
            seat,
        })
    }

    /// Drain client requests and flush replies; called by the backend loop
    /// after every `EventLoop::dispatch`.
    pub fn dispatch_clients(&mut self, display: &mut Display<Shell>) -> Result<(), Failure> {
        display
            .dispatch_clients(self)
            .map_err(|e| ("display", format!("dispatch: {e}")))?;
        display
            .flush_clients()
            .map_err(|e| ("display", format!("flush: {e}")))?;
        Ok(())
    }

    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
    }

    /// Take the pending redraw request.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw_requested)
    }

    /// Consume a request to invalidate backend damage history.
    pub fn take_full_redraw(&mut self) -> bool {
        std::mem::take(&mut self.full_redraw_requested)
    }

    pub fn emit(&self, event: &str, fields: &[(&str, Value)]) {
        evidence::emit(&evidence::record(self.evidence, event, fields));
    }

    /// Logical output size clients see (after rotation).
    pub fn logical_size(&self) -> Size<i32, Logical> {
        let (w, h) = self.plan.logical_size();
        Size::from((w as i32, h as i32))
    }

    pub fn output_rect(&self) -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), self.logical_size())
    }

    /// Where fullscreen clients live: the logical output minus the profile's
    /// safe area (camera cutout, rounded corners, gesture margin).
    pub fn client_area(&self) -> Rectangle<i32, Logical> {
        let (x, y, w, h) = self.plan.client_rect(self.config.safe_area);
        Rectangle::new((x, y).into(), (w as i32, h as i32).into())
    }

    /// Rows of the compositor home indicator, when the focused window does
    /// not draw Ferrix's pill itself (see `layout::draws_own_pill`).
    pub fn system_pill(&self) -> Vec<Rectangle<i32, Logical>> {
        let focused = self.windows.focused();
        let own = focused.is_some_and(|w| crate::layout::draws_own_pill(self.windows.app_id_of(w)));
        if focused.is_none() || own {
            return Vec::new();
        }
        self.plan
            .system_pill_rows(self.config.safe_area)
            .into_iter()
            .map(|(x, y, w, h)| Rectangle::new((x, y).into(), (w as i32, h as i32).into()))
            .collect()
    }

    /// Apply a (possibly new) output plan: mode is owned by the backend, this
    /// updates transform and re-configures every toplevel to the new logical
    /// size.
    pub fn apply_plan(&mut self, plan: OutputLayout) {
        self.plan = plan;
        self.output.change_current_state(
            None,
            Some(rotation_to_transform(plan.rotation)),
            None,
            Some((0, 0).into()),
        );
        let size = self.client_area().size;
        for window in self.windows.iter() {
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|state| state.size = Some(size));
                if toplevel.is_initial_configure_sent() {
                    toplevel.send_pending_configure();
                }
            }
        }
        layer_map_for_output(&self.output).arrange();
        self.request_redraw();
    }

    fn hardware_wake(&mut self) {
        loop {
            match self.hardware_events.try_recv() {
                Ok(event) => {
                    event.apply_to(&mut self.hardware);
                    if matches!(event, HardwareEvent::DisplayChanged(_)) {
                        self.config.orientation = self.hardware.display.orientation;
                        let plan = OutputLayout::plan(
                            self.plan.panel_width,
                            self.plan.panel_height,
                            self.config.orientation,
                        );
                        self.apply_plan(plan);
                        let (width, height) = self.plan.logical_size();
                        self.emit(
                            "output-rotated",
                            &[
                                ("source", "ferrix-core".into()),
                                ("orientation", self.config.orientation.name().into()),
                                ("rotation", self.plan.rotation.label().into()),
                                ("logical_width", width.into()),
                                ("logical_height", height.into()),
                            ],
                        );
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.core_lost_reported {
                        self.core_lost_reported = true;
                        self.emit("core-disconnected", &[("status", "stale-state".into())]);
                    }
                    break;
                }
            }
        }
    }

    /// Rotate the logical output a quarter turn clockwise (debug hook: F9;
    /// the product gesture/sensor trigger arrives through `ferrix-hardware-api`).
    pub fn rotate_clockwise(&mut self) {
        let plan = self.plan.with_rotation(self.plan.rotation.next_clockwise());
        self.apply_plan(plan);
        let (w, h) = self.plan.logical_size();
        self.emit(
            "output-rotated",
            &[
                ("rotation", self.plan.rotation.label().into()),
                ("logical_width", w.into()),
                ("logical_height", h.into()),
                ("profile", self.config.profile_name.as_str().into()),
            ],
        );
    }

    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.windows
            .iter()
            .find(|w| {
                w.toplevel()
                    .map(|t| t.wl_surface() == surface)
                    .unwrap_or(false)
            })
            .cloned()
    }

    /// Topmost surface under a logical point: overlay/top layers, then the
    /// fullscreen toplevel (with its popups), then bottom/background layers.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let map = layer_map_for_output(&self.output);
        let layer_hit = |layers: &[Layer]| {
            layers.iter().find_map(|layer| {
                map.layer_under(*layer, pos).and_then(|surface| {
                    let geometry = map.layer_geometry(surface)?;
                    surface
                        .surface_under(pos - geometry.loc.to_f64(), WindowSurfaceType::ALL)
                        .map(|(s, p)| (s, (p + geometry.loc).to_f64()))
                })
            })
        };
        if let Some(hit) = layer_hit(&[Layer::Overlay, Layer::Top]) {
            return Some(hit);
        }
        if let Some(window) = self.windows.focused() {
            let location = self.client_area().loc - window.geometry().loc;
            if let Some((s, p)) =
                window.surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
            {
                return Some((s, (p + location).to_f64()));
            }
        }
        layer_hit(&[Layer::Bottom, Layer::Background])
    }

    /// Point keyboard focus at the focused toplevel, activate it, and tell
    /// `ferrix_appd` when the focused pid changed.
    /// Safe to call when the previously focused client is already gone: the
    /// stack no longer contains it, so focus moves to the next mapped window.
    fn refocus(&mut self) {
        let focused = self.windows.focused().cloned();
        for window in self.windows.iter() {
            let active = focused.as_ref() == Some(window);
            if window.set_activated(active)
                && let Some(toplevel) = window.toplevel()
                && toplevel.is_initial_configure_sent()
            {
                toplevel.send_pending_configure();
            }
        }
        let target = focused.and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()));
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, target, SERIAL_COUNTER.next_serial());
        }
        // Pointer focus follows the next physical motion. Assigning it here
        // races a newly mapped client binding `wl_pointer`: Smithay then sees
        // later motion over the same surface as no focus change, so the client
        // never receives an enter or any subsequent taps.
        self.report_focus();
        self.full_redraw_requested = true;
        self.request_redraw();
    }

    fn on_window_mapped(&mut self, window: &Window) {
        self.mapped_clients += 1;
        let (app_id, title) = appd_link::toplevel_meta(window);
        self.windows.set_app_id(window, app_id.clone());
        let size = window.geometry().size;
        evidence::emit(&evidence::client_mapped(
            self.evidence,
            self.windows.pid_of(window),
            app_id.as_deref().unwrap_or_default(),
            title.as_deref().unwrap_or_default(),
            size.w,
            size.h,
            self.windows.mapped_count(),
        ));
        self.report_surface_mapped(window);
        self.refocus();
    }

    /// Kernel-reported pid of the client owning `surface`; `None` when the
    /// client is already gone or the credentials are unavailable.
    fn client_pid(&self, surface: &WlSurface) -> Option<u32> {
        let client = surface.client()?;
        let credentials = client.get_credentials(&self.display_handle).ok()?;
        u32::try_from(credentials.pid).ok().filter(|pid| *pid > 0)
    }

    fn handle_toplevel_commit(&mut self, surface: &WlSurface) {
        let Some(window) = self.window_for_surface(surface) else {
            return;
        };
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .map(|d| d.lock().unwrap().initial_configure_sent)
                .unwrap_or(true)
        });
        if !initial_configure_sent {
            // The pending state already carries fullscreen + the output size.
            toplevel.send_configure();
            return;
        }
        let has_buffer =
            with_renderer_surface_state(surface, |state| state.buffer().is_some()).unwrap_or(false);
        if has_buffer && self.windows.mark_mapped(&window) {
            self.on_window_mapped(&window);
        }
    }

    fn handle_layer_commit(&mut self, surface: &WlSurface) {
        let mut map = layer_map_for_output(&self.output);
        let Some(layer) = map
            .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .cloned()
        else {
            return;
        };
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<LayerSurfaceData>()
                .map(|d| d.lock().unwrap().initial_configure_sent)
                .unwrap_or(true)
        });
        map.arrange();
        if !initial_configure_sent {
            layer.layer_surface().send_configure();
        }
    }

    fn handle_popup_commit(&mut self, surface: &WlSurface) {
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface)
            && !popup.is_initial_configure_sent()
        {
            let _ = popup.send_configure();
        }
    }

    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_for_surface(&root) else {
            return;
        };
        let mut target = self.output_rect();
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= Point::from((0, 0)) - window.geometry().loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    /// Send `wl_surface.frame` callbacks after a presented frame.
    pub fn send_frames(&self, time: Duration) {
        let output = self.output.clone();
        for window in self.windows.iter() {
            window.send_frame(&output, time, None, |_, _| Some(output.clone()));
        }
        for layer in layer_map_for_output(&output).layers() {
            layer.send_frame(&output, time, None, |_, _| Some(output.clone()));
        }
        if let CursorImageStatus::Surface(surface) = &self.cursor_status {
            send_frames_surface_tree(surface, &output, time, None, |_, _| Some(output.clone()));
        }
    }

    /// Periodic housekeeping: dead popups and layers.
    pub fn cleanup(&mut self) {
        self.popups.cleanup();
        layer_map_for_output(&self.output).cleanup();
        if let CursorImageStatus::Surface(surface) = &self.cursor_status
            && !surface.is_alive()
        {
            self.cursor_status = CursorImageStatus::default_named();
        }
    }
}

// ---------------------------------------------------------------------------
// wl_compositor / wl_shm

impl CompositorHandler for Shell {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<ClientState>()
            .expect("client data is always ClientState")
            .compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for_surface(&root) {
                window.on_commit();
            }
        }
        self.handle_toplevel_commit(surface);
        self.handle_layer_commit(surface);
        self.handle_popup_commit(surface);
        self.request_redraw();
    }
}

impl BufferHandler for Shell {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl Shell {
    /// Publish zwp_linux_dmabuf_v1 with the formats the backend's renderer
    /// can import (pixman: linear single-plane only). Idempotent.
    pub fn enable_dmabuf(&mut self, formats: FormatSet) {
        if self.dmabuf_global.is_some() || formats.iter().next().is_none() {
            return;
        }
        let global = self
            .dmabuf_state
            .create_global::<Shell>(&self.display_handle, formats.iter().copied());
        tracing::info!(
            formats = formats.iter().count(),
            "zwp_linux_dmabuf_v1 published"
        );
        self.dmabuf_formats = formats;
        self.dmabuf_global = Some(global);
    }
}

impl DmabufHandler for Shell {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        // The renderer maps the buffer on first use; refuse up front what it
        // could never import so the client falls back instead of drawing nothing.
        if dmabuf.num_planes() == 1 && self.dmabuf_formats.contains(&dmabuf.format()) {
            let _ = notifier.successful::<Shell>();
        } else {
            notifier.failed();
        }
    }
}

impl ShmHandler for Shell {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

delegate_compositor!(Shell);
delegate_shm!(Shell);
delegate_dmabuf!(Shell);

// ---------------------------------------------------------------------------
// xdg-shell: fullscreen-only toplevels

impl XdgShellHandler for Shell {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let size = self.client_area().size;
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Fullscreen);
            state.size = Some(size);
        });
        let pid = self.client_pid(surface.wl_surface());
        let window = Window::new_wayland_window(surface);
        self.windows.push_with_pid(window, pid);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Popup grabs are not offered on the phone shell yet; popups stay
        // mapped without an explicit grab.
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<wl_output::WlOutput>,
    ) {
        let size = self.client_area().size;
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Fullscreen);
            state.size = Some(size);
        });
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        // Every toplevel stays fullscreen; the protocol still demands a reply.
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        self.fullscreen_request(surface, None);
    }

    /// Called for a graceful `xdg_toplevel.destroy` and, through resource
    /// cleanup, when a client disconnects or crashes. Either way the window
    /// leaves the stack and focus moves to the next mapped toplevel, so a
    /// crashing foreground app reveals the window below it (home).
    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            let was_mapped = self.windows.is_mapped(&window);
            let pid = self.windows.pid_of(&window);
            let app_id = self
                .windows
                .app_id_of(&window)
                .unwrap_or_default()
                .to_owned();
            self.windows.remove(&window);
            if was_mapped {
                let toplevels = self.windows.mapped_count();
                self.emit(
                    "client-unmapped",
                    &[("toplevels", Value::Int(toplevels as i64))],
                );
                let client_gone = pid.is_none_or(|pid| self.windows.mapped_count_for_pid(pid) == 0);
                if client_gone {
                    evidence::emit(&evidence::client_gone(
                        self.evidence,
                        pid,
                        &app_id,
                        toplevels,
                    ));
                    if let Some(pid) = pid {
                        self.report_surface_unmapped(pid);
                    }
                }
            }
        }
        self.refocus();
    }
}

delegate_xdg_shell!(Shell);

// ---------------------------------------------------------------------------
// wlr-layer-shell: reserved for status / launcher surfaces

impl WlrLayerShellHandler for Shell {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        _output: Option<wl_output::WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        self.emit(
            "layer-surface",
            &[
                ("namespace", namespace.as_str().into()),
                ("layer", format!("{layer:?}").to_lowercase().into()),
            ],
        );
        let mut map = layer_map_for_output(&self.output);
        if let Err(e) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            tracing::warn!("layer surface rejected: {e}");
        }
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        let _ = self.popups.track_popup(PopupKind::Xdg(popup));
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let mut map = layer_map_for_output(&self.output);
        let layer = map
            .layers()
            .find(|layer| layer.layer_surface() == &surface)
            .cloned();
        if let Some(layer) = layer {
            map.unmap_layer(&layer);
        }
        drop(map);
        self.request_redraw();
    }
}

delegate_layer_shell!(Shell);

// ---------------------------------------------------------------------------
// wl_seat / data device / wl_output

impl SeatHandler for Shell {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Shell> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
        self.request_redraw();
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

delegate_seat!(Shell);

impl SelectionHandler for Shell {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Shell {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Shell {}
impl ServerDndGrabHandler for Shell {}

delegate_data_device!(Shell);

impl OutputHandler for Shell {}
delegate_output!(Shell);

/// Shared helper for backends: the wl_output mode for a physical size.
pub fn output_mode(width: i32, height: i32, refresh_mhz: i32) -> smithay::output::Mode {
    smithay::output::Mode {
        size: Size::from((width, height)),
        refresh: refresh_mhz,
    }
}

/// Hotspot of a client cursor surface, if it set one.
pub fn cursor_hotspot(surface: &WlSurface) -> Point<i32, Logical> {
    with_states(surface, |states| {
        states
            .data_map
            .get::<Mutex<smithay::input::pointer::CursorImageAttributes>>()
            .map(|attrs| attrs.lock().unwrap().hotspot)
            .unwrap_or_default()
    })
}
