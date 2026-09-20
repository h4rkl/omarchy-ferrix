//! Guest-side Wayland client built on smithay-client-toolkit: a fullscreen
//! xdg-toplevel with a software-rendered wl_shm buffer. Emits a
//! machine-readable record only after the first frame is actually attached and
//! committed, per the evidence rules.
//!
//! The client runs on calloop (Smithay's event loop) so the Terminal app's PTY
//! reader thread can wake the UI through a ping source instead of polling.
//! Keyboard input arrives as raw `wl_keyboard` evdev codes; see `keymap` for
//! why libxkbcommon is not linked into this static guest binary.

use std::io::Write as _;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::ping::make_ping;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface, wl_touch,
};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle, WEnum};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_touch, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        touch::TouchHandler,
    },
    shell::WaylandSurface,
    shell::xdg::XdgShell,
    shell::xdg::window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
    shm::slot::{Buffer, SlotPool},
    shm::{Shm, ShmHandler},
};

use hardware_api::{HardwareCommand, HardwareEvent, HardwareState, WifiState};
use mobile_protocol::Subscription;
use target_profile::Profile;

use crate::draw::Type;
use crate::interaction::{AppId, Input, Page, SystemAction, TermAction, UiState};
use crate::keymap;
use crate::screen;
use crate::terminal::{Geometry, Session, Snapshot};

/// The guest shell. BusyBox `sh` in the Alpine smoke-test guest; `HOME`,
/// `USER`, and `SHELL` come from the init environment.
const SHELL: &str = "/bin/sh";

struct App {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    profile: Profile,
    /// Live hardware state as reported by mobile-core (adapter -> core -> UI).
    hw: HardwareState,
    hw_events: Receiver<HardwareEvent>,
    core_socket: PathBuf,
    core_lost_reported: bool,
    ty: Type,
    minute: u32,
    size: (u32, u32),
    first_frame_reported: bool,
    running: bool,
    error: Option<String>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_surface: Option<wl_surface::WlSurface>,
    cursor_buffer: Option<Buffer>,
    touch: Option<wl_touch::WlTouch>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: keymap::Modifiers,
    keyboard_reported: bool,
    input: Input,
    ui: UiState,
    session: Option<Session>,
    session_error: Option<String>,
    wake: Arc<dyn Fn() + Send + Sync>,
    terminal_ready_reported: bool,
    terminal_exit_reported: bool,
}

fn emit(record: String) {
    println!("{record}");
    let _ = std::io::stdout().flush();
}

fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn run(profile: &Profile, core: Subscription, minute: u32) -> Result<(), String> {
    let ty = Type::load()?;
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))?;
    let (globals, event_queue) =
        registry_queue_init(&conn).map_err(|e| format!("wayland registry: {e}"))?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|e| format!("xdg_wm_base: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_app_id("org.omarchy.mobile-basic-screen");
    window.set_title("Omarchy basic screen (QEMU reference)");
    window.set_fullscreen(None);
    window.commit();

    let initial = (profile.display.width, profile.display.height);
    let pool = SlotPool::new((initial.0 * initial.1 * 4) as usize, &shm)
        .map_err(|e| format!("shm pool: {e}"))?;

    let mut event_loop: EventLoop<App> =
        EventLoop::try_new().map_err(|e| format!("event loop: {e}"))?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue)
        .insert(handle.clone())
        .map_err(|e| format!("wayland event source: {e}"))?;
    let (ping, ping_source) = make_ping().map_err(|e| format!("terminal wake: {e}"))?;
    handle
        .insert_source(ping_source, |_, _, app: &mut App| app.terminal_wake())
        .map_err(|e| format!("terminal wake source: {e}"))?;
    let (hw_ping, hw_ping_source) = make_ping().map_err(|e| format!("core wake: {e}"))?;
    handle
        .insert_source(hw_ping_source, |_, _, app: &mut App| app.hardware_wake())
        .map_err(|e| format!("core wake source: {e}"))?;
    let core_socket = core.socket().to_owned();
    let hw = core.state.clone();
    let hw_events = core.spawn(Arc::new(move || hw_ping.ping()));
    let (wifi_on, bt_on) = crate::ui_toggles(&hw);
    let mut ui = UiState::new(wifi_on, bt_on);
    ui.brightness = hw.display.brightness_percent;

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        compositor_state: compositor,
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        window,
        profile: profile.clone(),
        hw,
        hw_events,
        core_socket,
        core_lost_reported: false,
        ty,
        minute,
        size: initial,
        first_frame_reported: false,
        running: true,
        error: None,
        pointer: None,
        cursor_surface: None,
        cursor_buffer: None,
        touch: None,
        keyboard: None,
        mods: keymap::Modifiers::default(),
        keyboard_reported: false,
        input: Input::default(),
        ui,
        session: None,
        session_error: None,
        wake: Arc::new(move || ping.ping()),
        terminal_ready_reported: false,
        terminal_exit_reported: false,
    };

    while app.running {
        event_loop
            .dispatch(None, &mut app)
            .map_err(|e| format!("event loop dispatch: {e}"))?;
        if let Some(err) = app.error.take() {
            return Err(err);
        }
    }
    Ok(())
}

impl App {
    const CURSOR_SIZE: i32 = 32;

    fn install_touch_cursor(&mut self, qh: &QueueHandle<Self>) -> Result<(), String> {
        let surface = self.compositor_state.create_surface(qh);
        let stride = Self::CURSOR_SIZE * 4;
        let (buffer, pixels) = self
            .pool
            .create_buffer(
                Self::CURSOR_SIZE,
                Self::CURSOR_SIZE,
                stride,
                wl_shm::Format::Argb8888,
            )
            .map_err(|error| format!("touch cursor buffer: {error}"))?;
        pixels.fill(0);

        // Little-endian premultiplied ARGB8888: a transparent center with a
        // crisp Tokyo Night blue ring, faint outer halo, and exact hotspot dot.
        let center = (Self::CURSOR_SIZE as f32 - 1.0) / 2.0;
        for y in 0..Self::CURSOR_SIZE {
            for x in 0..Self::CURSOR_SIZE {
                let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
                let alpha = if distance <= 2.25 {
                    255_u8
                } else if (11.0..=13.5).contains(&distance) {
                    230_u8
                } else if (10.0..=14.5).contains(&distance) {
                    72_u8
                } else {
                    continue;
                };
                let offset = ((y * Self::CURSOR_SIZE + x) * 4) as usize;
                let premultiply = |channel: u8| (u16::from(channel) * u16::from(alpha) / 255) as u8;
                pixels[offset] = premultiply(0xf7);
                pixels[offset + 1] = premultiply(0xa2);
                pixels[offset + 2] = premultiply(0x7a);
                pixels[offset + 3] = alpha;
            }
        }

        surface.damage_buffer(0, 0, Self::CURSOR_SIZE, Self::CURSOR_SIZE);
        buffer
            .attach_to(&surface)
            .map_err(|error| format!("touch cursor attach: {error}"))?;
        surface.commit();
        self.cursor_surface = Some(surface);
        self.cursor_buffer = Some(buffer);
        Ok(())
    }

    fn on_terminal(&self) -> bool {
        self.ui.page == Page::App(AppId::Terminal)
    }

    fn geometry(&self) -> Geometry {
        let m = screen::terminal_metrics(&self.ty, self.size.0, self.size.1);
        let clamp = |v: usize| u16::try_from(v).unwrap_or(u16::MAX);
        let clamp_px = |v: i32| u16::try_from(v.max(1)).unwrap_or(u16::MAX);
        Geometry {
            cols: clamp(m.cols),
            rows: clamp(m.rows),
            cell_w: clamp_px(m.cell_w),
            cell_h: clamp_px(m.cell_h),
        }
    }

    fn open_session(&mut self) {
        let geometry = self.geometry();
        self.terminal_ready_reported = false;
        self.terminal_exit_reported = false;
        match Session::spawn(SHELL, &[], geometry, Arc::clone(&self.wake)) {
            Ok(session) => {
                emit(format!(
                    "{{\"evidence\":\"QEMU\",\"event\":\"terminal-spawned\",\"shell\":\"{}\",\"cols\":{},\"rows\":{}}}",
                    session.program(),
                    geometry.cols,
                    geometry.rows
                ));
                self.session = Some(session);
                self.session_error = None;
            }
            Err(error) => {
                emit(format!(
                    "{{\"evidence\":\"QEMU\",\"event\":\"terminal-error\",\"shell\":\"{SHELL}\",\"error\":\"{}\"}}",
                    json_escape(&error)
                ));
                self.session_error = Some(error);
            }
        }
    }

    /// Runs after every UI change: starts the shell when the Terminal app is
    /// first opened and forwards queued terminal actions to the session. The
    /// session outlives navigation, like a backgrounded phone app; Enter on an
    /// exited shell restarts it.
    fn after_input(&mut self) {
        if self.on_terminal() && self.session.is_none() && self.session_error.is_none() {
            self.open_session();
        }
        for action in std::mem::take(&mut self.ui.pending) {
            match action {
                TermAction::Input(bytes) => {
                    let live = self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.exited().is_none());
                    if live {
                        if let Some(session) = &self.session {
                            session.write(bytes);
                        }
                    } else if bytes == b"\r" {
                        self.session = None;
                        self.session_error = None;
                        self.open_session();
                    }
                }
                TermAction::Scroll(lines) => {
                    if let Some(session) = &self.session {
                        session.scroll(lines);
                    }
                }
            }
        }
        for action in std::mem::take(&mut self.ui.pending_system) {
            let command = match action {
                SystemAction::WifiEnabled(enabled) => HardwareCommand::SetWifiEnabled { enabled },
                SystemAction::BluetoothEnabled(enabled) => {
                    HardwareCommand::SetBluetoothEnabled { enabled }
                }
                SystemAction::Brightness(percent) => HardwareCommand::SetBrightness { percent },
            };
            match mobile_protocol::command(
                &self.core_socket,
                command,
                std::time::Duration::from_secs(2),
            ) {
                Ok(state) => {
                    self.hw = state;
                    self.sync_controls();
                }
                Err(error) => {
                    self.sync_controls();
                    emit(format!(
                        "{{\"evidence\":\"QEMU\",\"event\":\"ui-command-failed\",\"error\":\"{}\"}}",
                        json_escape(&error)
                    ));
                }
            }
        }
    }

    fn sync_controls(&mut self) {
        self.ui.wifi_on = self.hw.network.wifi != WifiState::Disabled;
        self.ui.bluetooth_on = self.hw.bluetooth.enabled;
        self.ui.brightness = self.hw.display.brightness_percent;
    }

    /// Ping handler: mobile-core delivered hardware events (or went away).
    /// Every event is folded into the live state, reported, and redrawn.
    fn hardware_wake(&mut self) {
        let mut changed = false;
        loop {
            match self.hw_events.try_recv() {
                Ok(event) => {
                    event.apply_to(&mut self.hw);
                    self.sync_controls();
                    changed = true;
                    emit(format!(
                        "{{\"evidence\":\"QEMU\",\"event\":\"ui-state\",\"source\":\"{}\",\"battery_percent\":{},\"charging\":{},\"wifi\":\"{}\",\"modem\":\"{}\",\"sim\":\"{}\",\"orientation\":\"{}\",\"brightness_percent\":{}}}",
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
                        emit(
                            "{\"evidence\":\"QEMU\",\"event\":\"core-disconnected\",\"status\":\"stale-state\"}"
                                .to_owned(),
                        );
                    }
                    break;
                }
            }
        }
        if changed {
            self.draw();
        }
    }

    /// Ping handler: the PTY reader produced output or the shell exited.
    fn terminal_wake(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        session.pump();
        let exited = session.exited().map(str::to_owned);
        let first_line = (!self.terminal_ready_reported)
            .then(|| session.snapshot().first_text_line())
            .flatten();
        if let Some(reason) = exited
            && !self.terminal_exit_reported
        {
            self.terminal_exit_reported = true;
            emit(format!(
                "{{\"evidence\":\"QEMU\",\"event\":\"terminal-exited\",\"reason\":\"{}\"}}",
                json_escape(&reason)
            ));
        }
        if let Some(line) = first_line {
            self.terminal_ready_reported = true;
            emit(format!(
                "{{\"evidence\":\"QEMU\",\"event\":\"terminal-ready\",\"shell\":\"{SHELL}\",\"line\":\"{}\"}}",
                json_escape(&line)
            ));
        }
        if self.on_terminal() {
            self.draw();
        }
    }

    fn key_pressed(&mut self, code: u32) {
        if !self.on_terminal() {
            return;
        }
        let Some(bytes) = keymap::bytes(code, self.mods, self.ui.term_app_cursor) else {
            return;
        };
        if !self.keyboard_reported {
            self.keyboard_reported = true;
            emit(format!(
                "{{\"evidence\":\"QEMU\",\"event\":\"keyboard-input\",\"code\":{code},\"bytes\":{}}}",
                bytes.len()
            ));
        }
        self.ui.pending.push(TermAction::Input(bytes));
        self.after_input();
    }

    fn draw(&mut self) {
        let (w, h) = self.size;
        let stride = w as i32 * 4;
        let snapshot = if self.on_terminal() {
            match (&self.session, &self.session_error) {
                (Some(session), _) => Some(session.snapshot()),
                (None, Some(error)) => Some(Snapshot::failed(error.clone())),
                (None, None) => None,
            }
        } else {
            None
        };
        if let Some(snapshot) = &snapshot {
            self.ui.term_app_cursor = snapshot.app_cursor;
        }
        let (buffer, canvas_bytes) =
            match self
                .pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Xrgb8888)
            {
                Ok(v) => v,
                Err(e) => {
                    self.error = Some(format!("shm buffer {w}x{h}: {e}"));
                    self.running = false;
                    return;
                }
            };
        let frame = screen::compose(
            &self.hw,
            self.minute,
            &self.ty,
            w,
            h,
            &self.ui,
            snapshot.as_ref(),
        );
        canvas_bytes.copy_from_slice(&frame.xrgb_bytes());
        self.window
            .wl_surface()
            .damage_buffer(0, 0, w as i32, h as i32);
        if let Err(e) = buffer.attach_to(self.window.wl_surface()) {
            self.error = Some(format!("buffer attach: {e}"));
            self.running = false;
            return;
        }
        self.window.commit();
        if !self.first_frame_reported {
            self.first_frame_reported = true;
            emit(format!(
                "{{\"evidence\":\"QEMU\",\"event\":\"basic-screen-frame\",\"status\":\"committed\",\"width\":{w},\"height\":{h},\"profile\":\"{}\"}}",
                self.profile.name
            ));
        }
    }
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.running = false;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let w = configure
            .new_size
            .0
            .map(NonZeroU32::get)
            .unwrap_or(self.size.0);
        let h = configure
            .new_size
            .1
            .map(NonZeroU32::get)
            .unwrap_or(self.size.1);
        self.size = (w, h);
        let geometry = self.geometry();
        if let Some(session) = self.session.as_mut() {
            session.resize(geometry);
        }
        self.draw();
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
                let cursor_ready = self.pointer.is_some() && self.install_touch_cursor(qh).is_ok();
                emit(format!(
                    "{{\"evidence\":\"QEMU\",\"event\":\"input-ready\",\"capability\":\"pointer\",\"status\":\"{}\"}}",
                    if cursor_ready { "ok" } else { "failed" }
                ));
            }
            Capability::Touch if self.touch.is_none() => {
                self.touch = self.seat_state.get_touch(qh, &seat).ok();
                emit(format!(
                    "{{\"evidence\":\"QEMU\",\"event\":\"input-ready\",\"capability\":\"touch\",\"status\":\"{}\"}}",
                    if self.touch.is_some() { "ok" } else { "failed" }
                ));
            }
            Capability::Keyboard if self.keyboard.is_none() => {
                self.keyboard = Some(seat.get_keyboard(qh, ()));
                emit(
                    "{\"evidence\":\"QEMU\",\"event\":\"input-ready\",\"capability\":\"keyboard\",\"status\":\"ok\"}"
                        .to_owned(),
                );
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                if let Some(pointer) = self.pointer.take() {
                    pointer.release();
                }
                self.cursor_buffer = None;
                if let Some(surface) = self.cursor_surface.take() {
                    surface.destroy();
                }
            }
            Capability::Touch => {
                if let Some(touch) = self.touch.take() {
                    touch.release();
                }
            }
            Capability::Keyboard => {
                if let Some(keyboard) = self.keyboard.take() {
                    keyboard.release();
                }
                self.mods = keymap::Modifiers::default();
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_locked,
                ..
            } => app.mods = keymap::Modifiers::from_xkb(mods_depressed, mods_locked),
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                app.key_pressed(key);
                if app.on_terminal() {
                    app.draw();
                }
            }
            wl_keyboard::Event::Leave { .. } => app.mods = keymap::Modifiers::default(),
            _ => {}
        }
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let mut redraw = false;
        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some(surface) = self.cursor_surface.as_ref() {
                        let hotspot = Self::CURSOR_SIZE / 2;
                        pointer.set_cursor(serial, Some(surface), hotspot, hotspot);
                        emit(format!(
                            "{{\"evidence\":\"QEMU\",\"event\":\"cursor-ready\",\"style\":\"transparent-ring\",\"size\":{}}}",
                            Self::CURSOR_SIZE
                        ));
                    }
                    redraw |= self
                        .input
                        .motion(0, event.position, &mut self.ui, self.size);
                }
                PointerEventKind::Motion { .. } => {
                    redraw |= self
                        .input
                        .motion(0, event.position, &mut self.ui, self.size);
                }
                PointerEventKind::Leave { .. } => {
                    redraw |= self.input.cancel(&mut self.ui);
                }
                PointerEventKind::Press { button: 0x110, .. } => {
                    redraw |= self.input.down(0, event.position, &mut self.ui, self.size);
                    emit(format!(
                        "{{\"evidence\":\"QEMU\",\"event\":\"input-press\",\"x\":{},\"y\":{}}}",
                        event.position.0, event.position.1
                    ));
                }
                PointerEventKind::Release { button: 0x110, .. } => {
                    redraw |= self
                        .input
                        .motion(0, event.position, &mut self.ui, self.size);
                    redraw |= self.input.up(0, &mut self.ui, self.size);
                    emit(format!(
                        "{{\"evidence\":\"QEMU\",\"event\":\"input-release\",\"page\":\"{:?}\"}}",
                        self.ui.page
                    ));
                }
                _ => {}
            }
        }
        if redraw {
            self.after_input();
            self.draw();
        }
    }
}

impl TouchHandler for App {
    fn down(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: u32,
        surface: wl_surface::WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        if surface == *self.window.wl_surface()
            && self.input.down(id, position, &mut self.ui, self.size)
        {
            self.after_input();
            self.draw();
        }
    }

    fn up(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        _: u32,
        id: i32,
    ) {
        if self.input.up(id, &mut self.ui, self.size) {
            self.after_input();
            self.draw();
        }
    }

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        id: i32,
        position: (f64, f64),
    ) {
        if self.input.motion(id, position, &mut self.ui, self.size) {
            self.after_input();
            self.draw();
        }
    }

    fn shape(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
        _: f64,
    ) {
    }

    fn orientation(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
    ) {
    }

    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch) {
        if self.input.cancel(&mut self.ui) {
            self.draw();
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_pointer!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_touch!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_registry!(App);
