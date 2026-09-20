//! Fullscreen Wayland client runner built on smithay-client-toolkit.
//!
//! Owns everything a Omarchy Ferrix reference surface needs from Wayland: registry,
//! compositor, xdg-shell, shm, seat (pointer, touch, raw keyboard), the
//! fullscreen toplevel, the software-rendered `wl_shm` buffers, the
//! transparent-ring touch cursor, and the calloop loop with one ping source
//! for background wakes. A surface implements [`ClientHandler`] and gets a
//! [`Wake`] handle at construction so PTY readers, core subscriptions, or
//! `ferrix_appd` notification threads can request a wake from any thread.
//!
//! Evidence records are emitted only after the first frame is actually
//! attached and committed. Keyboard input arrives as raw `wl_keyboard` evdev
//! codes; see [`crate::keymap`] for why libxkbcommon is not linked.

use std::num::NonZeroU32;
use std::sync::Arc;

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

use crate::draw::Canvas;
use crate::evidence::{emit, json_escape};
use crate::keymap::Modifiers;

/// Thread-safe handle that wakes the client loop; the handler's
/// [`ClientHandler::wake`] runs on the UI thread afterwards.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Static configuration of one fullscreen client.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// xdg `app_id`, e.g. `org.ferrix.terminal`.
    pub app_id: String,
    pub title: String,
    /// Size assumed until the compositor's first configure and used to size
    /// the shm pool.
    pub initial_size: (u32, u32),
    /// Evidence label for every record the runner emits (`QEMU` or `mock`).
    pub evidence: String,
    /// Event name of the first-frame record, e.g. `basic-screen-frame`.
    pub first_frame_event: String,
    /// Extra string fields appended to the first-frame record.
    pub first_frame_extra: Vec<(String, String)>,
}

/// What a surface implements. Positions are surface pixels. Every input
/// method returns whether the surface must be redrawn.
pub trait ClientHandler {
    /// Renders one frame at `size`.
    fn draw(&mut self, size: (u32, u32)) -> Canvas;
    /// The compositor configured a (possibly new) size; a redraw follows.
    fn configure(&mut self, _size: (u32, u32)) {}
    fn pointer_down(&mut self, position: (f64, f64), size: (u32, u32)) -> bool;
    fn pointer_motion(&mut self, position: (f64, f64), size: (u32, u32)) -> bool;
    fn pointer_up(&mut self, position: (f64, f64), size: (u32, u32)) -> bool;
    fn pointer_leave(&mut self) -> bool;
    fn touch_down(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool;
    fn touch_motion(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool;
    fn touch_up(&mut self, id: i32, size: (u32, u32)) -> bool;
    fn touch_cancel(&mut self) -> bool;
    /// A raw evdev key press with the current modifier state.
    fn key(&mut self, code: u32, mods: Modifiers) -> bool;
    /// The [`Wake`] handle was called from some thread; drain your channels.
    fn wake(&mut self) -> bool;
    /// Page name for the `input-release` record.
    fn page_name(&self) -> String;
}

struct Client {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    config: ClientConfig,
    handler: Box<dyn ClientHandler>,
    size: (u32, u32),
    first_frame_reported: bool,
    running: bool,
    error: Option<String>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_surface: Option<wl_surface::WlSurface>,
    cursor_buffer: Option<Buffer>,
    touch: Option<wl_touch::WlTouch>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Modifiers,
}

/// Connects to the Wayland display from the environment, builds the handler
/// with a wake handle, and runs the event loop until the toplevel is closed
/// or the handler fails. Blocks the calling thread.
pub fn run<H, F>(config: ClientConfig, build: F) -> Result<(), String>
where
    H: ClientHandler + 'static,
    F: FnOnce(Wake) -> Result<H, String>,
{
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
    window.set_app_id(config.app_id.clone());
    window.set_title(config.title.clone());
    window.set_fullscreen(None);
    window.commit();

    let initial = (config.initial_size.0.max(1), config.initial_size.1.max(1));
    let pool = SlotPool::new((initial.0 * initial.1 * 4) as usize, &shm)
        .map_err(|e| format!("shm pool: {e}"))?;

    let mut event_loop: EventLoop<Client> =
        EventLoop::try_new().map_err(|e| format!("event loop: {e}"))?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue)
        .insert(handle.clone())
        .map_err(|e| format!("wayland event source: {e}"))?;
    let (ping, ping_source) = make_ping().map_err(|e| format!("wake source: {e}"))?;
    handle
        .insert_source(ping_source, |_, _, client: &mut Client| client.on_wake())
        .map_err(|e| format!("wake source: {e}"))?;
    let wake: Wake = Arc::new(move || ping.ping());
    let handler = build(wake)?;

    let mut client = Client {
        registry_state: RegistryState::new(&globals),
        compositor_state: compositor,
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        window,
        config,
        handler: Box::new(handler),
        size: initial,
        first_frame_reported: false,
        running: true,
        error: None,
        pointer: None,
        cursor_surface: None,
        cursor_buffer: None,
        touch: None,
        keyboard: None,
        mods: Modifiers::default(),
    };

    while client.running {
        event_loop
            .dispatch(None, &mut client)
            .map_err(|e| format!("event loop dispatch: {e}"))?;
        if let Some(err) = client.error.take() {
            return Err(err);
        }
    }
    Ok(())
}

impl Client {
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

    fn on_wake(&mut self) {
        if self.handler.wake() {
            self.draw();
        }
    }

    fn draw(&mut self) {
        let (w, h) = self.size;
        let stride = w as i32 * 4;
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
        let frame = self.handler.draw((w, h));
        if (frame.width(), frame.height()) != (w as i32, h as i32) {
            self.error = Some(format!(
                "handler drew {}x{} for a {w}x{h} surface",
                frame.width(),
                frame.height()
            ));
            self.running = false;
            return;
        }
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
            let mut extra = String::new();
            for (key, value) in &self.config.first_frame_extra {
                extra.push_str(&format!(
                    ",\"{}\":\"{}\"",
                    json_escape(key),
                    json_escape(value)
                ));
            }
            emit(format!(
                "{{\"evidence\":\"{}\",\"event\":\"{}\",\"status\":\"committed\",\"width\":{w},\"height\":{h}{extra}}}",
                self.config.evidence, self.config.first_frame_event
            ));
        }
    }
}

impl WindowHandler for Client {
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
        self.handler.configure(self.size);
        self.draw();
    }
}

impl CompositorHandler for Client {
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

impl OutputHandler for Client {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Client {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for Client {
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
        let evidence = self.config.evidence.clone();
        match capability {
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
                let cursor_ready = self.pointer.is_some() && self.install_touch_cursor(qh).is_ok();
                emit(format!(
                    "{{\"evidence\":\"{evidence}\",\"event\":\"input-ready\",\"capability\":\"pointer\",\"status\":\"{}\"}}",
                    if cursor_ready { "ok" } else { "failed" }
                ));
            }
            Capability::Touch if self.touch.is_none() => {
                self.touch = self.seat_state.get_touch(qh, &seat).ok();
                emit(format!(
                    "{{\"evidence\":\"{evidence}\",\"event\":\"input-ready\",\"capability\":\"touch\",\"status\":\"{}\"}}",
                    if self.touch.is_some() { "ok" } else { "failed" }
                ));
            }
            Capability::Keyboard if self.keyboard.is_none() => {
                self.keyboard = Some(seat.get_keyboard(qh, ()));
                emit(format!(
                    "{{\"evidence\":\"{evidence}\",\"event\":\"input-ready\",\"capability\":\"keyboard\",\"status\":\"ok\"}}"
                ));
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
                self.mods = Modifiers::default();
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        client: &mut Self,
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
            } => client.mods = Modifiers::from_xkb(mods_depressed, mods_locked),
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                if client.handler.key(key, client.mods) {
                    client.draw();
                }
            }
            wl_keyboard::Event::Leave { .. } => client.mods = Modifiers::default(),
            _ => {}
        }
    }
}

impl PointerHandler for Client {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let mut redraw = false;
        let size = self.size;
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
                            "{{\"evidence\":\"{}\",\"event\":\"cursor-ready\",\"style\":\"transparent-ring\",\"size\":{}}}",
                            self.config.evidence,
                            Self::CURSOR_SIZE
                        ));
                    }
                    redraw |= self.handler.pointer_motion(event.position, size);
                }
                PointerEventKind::Motion { .. } => {
                    redraw |= self.handler.pointer_motion(event.position, size);
                }
                PointerEventKind::Leave { .. } => {
                    redraw |= self.handler.pointer_leave();
                }
                PointerEventKind::Press { button: 0x110, .. } => {
                    redraw |= self.handler.pointer_down(event.position, size);
                    emit(format!(
                        "{{\"evidence\":\"{}\",\"event\":\"input-press\",\"x\":{},\"y\":{}}}",
                        self.config.evidence, event.position.0, event.position.1
                    ));
                }
                PointerEventKind::Release { button: 0x110, .. } => {
                    redraw |= self.handler.pointer_up(event.position, size);
                    emit(format!(
                        "{{\"evidence\":\"{}\",\"event\":\"input-release\",\"page\":\"{}\"}}",
                        self.config.evidence,
                        json_escape(&self.handler.page_name())
                    ));
                }
                _ => {}
            }
        }
        if redraw {
            self.draw();
        }
    }
}

impl TouchHandler for Client {
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
        if surface == *self.window.wl_surface() && self.handler.touch_down(id, position, self.size)
        {
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
        if self.handler.touch_up(id, self.size) {
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
        if self.handler.touch_motion(id, position, self.size) {
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
        if self.handler.touch_cancel() {
            self.draw();
        }
    }
}

impl ProvidesRegistryState for Client {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Client);
delegate_output!(Client);
delegate_pointer!(Client);
delegate_shm!(Client);
delegate_seat!(Client);
delegate_touch!(Client);
delegate_xdg_shell!(Client);
delegate_xdg_window!(Client);
delegate_registry!(Client);
