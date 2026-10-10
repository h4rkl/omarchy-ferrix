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
//! codes; see [`crate::keymap`] for why libxkbcommon is not linked. Held
//! hardware keys repeat at the compositor's `repeat_info`.
//!
//! Timers: a surface that needs to act later (on-screen key repeat, a
//! transient scrollbar, a bell flash) returns the instant from
//! [`ClientHandler::deadline`]; the loop sleeps no longer than that and calls
//! [`ClientHandler::timer`].
//!
//! Clipboard: the runner binds `wl_data_device_manager` (ferrix-shell serves
//! it through Smithay; the selection follows keyboard focus). A surface
//! queues [`ClipboardRequest`]s and receives [`ClipboardEvent`]s. Offers are
//! read and our own source is written on short-lived threads, so pasting what
//! this process copied cannot deadlock its own event loop. Without a data
//! device the runner keeps an in-process clipboard and says so in the event.

use std::io::{Read as _, Write as _};
use std::num::NonZeroU32;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use smithay_client_toolkit::data_device_manager::data_source::{
    CopyPasteSource, DataSourceHandler,
};
use smithay_client_toolkit::data_device_manager::{DataDeviceManagerState, WritePipe};
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::ping::{Ping, make_ping};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output,
    wl_pointer, wl_seat, wl_shm, wl_surface, wl_touch,
};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle, WEnum};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_data_device, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, delegate_touch, delegate_xdg_shell,
    delegate_xdg_window,
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
use crate::keyboard;
use crate::keymap::Modifiers;
use crate::theme;

/// Text MIME types offered when copying and accepted when pasting, in order
/// of preference.
const TEXT_MIMES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
];
/// Pastes larger than this are truncated.
const PASTE_LIMIT: u64 = 1 << 20;

/// What a surface asks of the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardRequest {
    Copy(String),
    Paste,
}

/// Which clipboard served a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardRoute {
    /// The compositor's `wl_data_device` selection (shared with other apps).
    Wayland,
    /// No data device: this process's own clipboard only.
    InProcess,
}

impl ClipboardRoute {
    pub fn name(self) -> &'static str {
        match self {
            ClipboardRoute::Wayland => "wayland",
            ClipboardRoute::InProcess => "in-process",
        }
    }
}

/// Outcome of a [`ClipboardRequest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardEvent {
    Copied(ClipboardRoute),
    Pasted(String, ClipboardRoute),
    /// Nothing (or no text) is on the clipboard.
    Empty,
    Failed(String),
}

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
    /// Evidence label for every record the runner emits: the session label
    /// from [`crate::evidence::label`] in a session, `mock` for host runs.
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
    /// The Wayland timestamp (milliseconds, arbitrary base, wraps) of the
    /// touch or pointer event the runner is about to deliver; called right
    /// before each `touch_*` and pointer press/motion/release. Surfaces that
    /// estimate gesture velocity use it instead of the wall clock, which
    /// would see batched events as simultaneous.
    fn input_time(&mut self, _time_ms: u32) {}
    /// The next instant at which [`ClientHandler::timer`] must run.
    fn deadline(&self) -> Option<Instant> {
        None
    }
    /// The [`ClientHandler::deadline`] passed.
    fn timer(&mut self, _now: Instant) -> bool {
        false
    }
    /// Clipboard work queued since the last call.
    fn clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        Vec::new()
    }
    /// The outcome of a clipboard request.
    fn clipboard(&mut self, _event: ClipboardEvent) -> bool {
        false
    }
}

/// A held hardware key and when it next repeats.
struct HeldKey {
    code: u32,
    next: Instant,
}

/// evdev codes that never repeat: modifiers and locks.
fn is_modifier(code: u32) -> bool {
    matches!(
        code,
        29 | 42 | 54 | 56 | 58 | 69 | 70 | 97 | 100 | 125 | 126
    )
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
    qh: QueueHandle<Client>,
    /// `wl_keyboard.repeat_info`: (interval, delay); `None` disables repeat.
    hw_repeat: Option<(Duration, Duration)>,
    held_key: Option<HeldKey>,
    data_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    copy_source: Option<CopyPasteSource>,
    /// The text this process last copied (served to the compositor and used
    /// directly when there is no data device).
    copied: Option<String>,
    last_serial: u32,
    paste_tx: mpsc::Sender<Result<String, String>>,
    paste_rx: mpsc::Receiver<Result<String, String>>,
    ping: Ping,
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
    let loop_ping = ping.clone();
    let wake: Wake = Arc::new(move || ping.ping());
    let handler = build(wake)?;
    let data_manager = DataDeviceManagerState::bind(&globals, &qh).ok();
    let (paste_tx, paste_rx) = mpsc::channel();
    emit(format!(
        "{{\"evidence\":\"{}\",\"event\":\"clipboard-ready\",\"route\":\"{}\"}}",
        config.evidence,
        if data_manager.is_some() {
            ClipboardRoute::Wayland.name()
        } else {
            ClipboardRoute::InProcess.name()
        }
    ));

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
        qh: qh.clone(),
        hw_repeat: Some((Duration::from_millis(40), Duration::from_millis(400))),
        held_key: None,
        data_manager,
        data_device: None,
        copy_source: None,
        copied: None,
        last_serial: 0,
        paste_tx,
        paste_rx,
        ping: loop_ping,
    };

    while client.running {
        let timeout = client
            .next_deadline()
            .map(|at| at.saturating_duration_since(Instant::now()));
        event_loop
            .dispatch(timeout, &mut client)
            .map_err(|e| format!("event loop dispatch: {e}"))?;
        client.run_timers();
        client.serve_clipboard();
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
        // crisp ring in the theme's bright rust, faint outer halo, and exact
        // hotspot dot. The color is read at install time so a loaded
        // `colors.toml` recolors the cursor too.
        let ring = theme::get().rust_hi;
        let [_, red, green, blue] = ring.to_be_bytes();
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
                pixels[offset] = premultiply(blue);
                pixels[offset + 1] = premultiply(green);
                pixels[offset + 2] = premultiply(red);
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
        let mut redraw = false;
        while let Ok(result) = self.paste_rx.try_recv() {
            let event = match result {
                Ok(text) if text.is_empty() => ClipboardEvent::Empty,
                Ok(text) => ClipboardEvent::Pasted(text, ClipboardRoute::Wayland),
                Err(error) => ClipboardEvent::Failed(error),
            };
            redraw |= self.clipboard_event(event);
        }
        redraw |= self.handler.wake();
        if redraw {
            self.draw();
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        let held = self.held_key.as_ref().map(|h| h.next);
        match (self.handler.deadline(), held) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn run_timers(&mut self) {
        let now = Instant::now();
        let mut redraw = false;
        if self.handler.deadline().is_some_and(|at| at <= now) {
            redraw |= self.handler.timer(now);
        }
        if let (Some(held), Some((interval, _))) = (self.held_key.as_mut(), self.hw_repeat)
            && held.next <= now
        {
            held.next = now + interval;
            let code = held.code;
            redraw |= self.handler.key(code, self.mods);
        }
        if redraw {
            self.draw();
        }
    }

    fn clipboard_event(&mut self, event: ClipboardEvent) -> bool {
        let (status, route, bytes) = match &event {
            ClipboardEvent::Copied(route) => ("copied", route.name(), None),
            ClipboardEvent::Pasted(text, route) => ("pasted", route.name(), Some(text.len())),
            ClipboardEvent::Empty => ("empty", "", None),
            ClipboardEvent::Failed(_) => ("failed", "", None),
        };
        // Counts only: clipboard contents never reach the serial log.
        emit(format!(
            "{{\"evidence\":\"{}\",\"event\":\"clipboard\",\"status\":\"{status}\",\"route\":\"{route}\",\"bytes\":{}}}",
            self.config.evidence,
            bytes.map_or_else(|| "null".to_owned(), |n| n.to_string())
        ));
        self.handler.clipboard(event)
    }

    fn serve_clipboard(&mut self) {
        let requests = self.handler.clipboard_requests();
        let mut redraw = false;
        for request in requests {
            let event = match request {
                ClipboardRequest::Copy(text) => Some(self.copy(text)),
                ClipboardRequest::Paste => self.paste(),
            };
            if let Some(event) = event {
                redraw |= self.clipboard_event(event);
            }
        }
        if redraw {
            self.draw();
        }
    }

    fn copy(&mut self, text: String) -> ClipboardEvent {
        self.copied = Some(text);
        let (Some(manager), Some(device)) = (self.data_manager.as_ref(), self.data_device.as_ref())
        else {
            return ClipboardEvent::Copied(ClipboardRoute::InProcess);
        };
        let source = manager.create_copy_paste_source(&self.qh, TEXT_MIMES);
        source.set_selection(device, self.last_serial);
        self.copy_source = Some(source);
        ClipboardEvent::Copied(ClipboardRoute::Wayland)
    }

    /// Starts reading the current selection; the text arrives on a wake.
    fn paste(&mut self) -> Option<ClipboardEvent> {
        let offer = self
            .data_device
            .as_ref()
            .and_then(|device| device.data().selection_offer());
        let Some(offer) = offer else {
            return Some(if self.data_device.is_some() {
                ClipboardEvent::Empty
            } else {
                match self.copied.clone() {
                    Some(text) => ClipboardEvent::Pasted(text, ClipboardRoute::InProcess),
                    None => ClipboardEvent::Empty,
                }
            });
        };
        let mime = offer.with_mime_types(|offered| {
            TEXT_MIMES
                .iter()
                .find(|m| offered.iter().any(|o| o == *m))
                .map(|m| (*m).to_owned())
        });
        let Some(mime) = mime else {
            return Some(ClipboardEvent::Empty);
        };
        let pipe = match offer.receive(mime) {
            Ok(pipe) => pipe,
            Err(error) => return Some(ClipboardEvent::Failed(error.to_string())),
        };
        let fd = OwnedFd::from(pipe);
        let tx = self.paste_tx.clone();
        let ping = self.ping.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = std::fs::File::from(fd)
                .take(PASTE_LIMIT)
                .read_to_end(&mut bytes)
                .map(|_| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ping.ping();
        });
        None
    }

    fn draw(&mut self) {
        keyboard::refresh();
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
        // SlotPool rounds each buffer up to 64 bytes, so the slice can be
        // longer than w*h*4 (e.g. 1080x2215); fill exactly the frame.
        let bytes = frame.xrgb_bytes();
        canvas_bytes[..bytes.len()].copy_from_slice(&bytes);
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
                if self.data_device.is_none()
                    && let Some(manager) = self.data_manager.as_ref()
                {
                    self.data_device = Some(manager.get_data_device(qh, &seat));
                }
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
                self.held_key = None;
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
                serial,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                client.last_serial = serial;
                client.held_key = match client.hw_repeat {
                    // The longer of the compositor's delay and the user's
                    // keyboard.toml delay, so a slow (TCG) guest delivering a
                    // tap's release late does not read as a hold.
                    Some((_, delay)) if !is_modifier(key) => Some(HeldKey {
                        code: key,
                        next: Instant::now() + delay.max(keyboard::active().repeat.delay),
                    }),
                    _ => None,
                };
                if client.handler.key(key, client.mods) {
                    client.draw();
                }
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Released),
                ..
            } => {
                if client.held_key.as_ref().is_some_and(|h| h.code == key) {
                    client.held_key = None;
                }
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                client.hw_repeat = (rate > 0).then(|| {
                    (
                        Duration::from_millis(1000 / u64::from(rate.unsigned_abs()).max(1)),
                        Duration::from_millis(u64::from(delay.unsigned_abs())),
                    )
                });
            }
            wl_keyboard::Event::Leave { .. } => {
                client.mods = Modifiers::default();
                client.held_key = None;
            }
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
                PointerEventKind::Motion { time } => {
                    self.handler.input_time(time);
                    redraw |= self.handler.pointer_motion(event.position, size);
                }
                PointerEventKind::Leave { .. } => {
                    redraw |= self.handler.pointer_leave();
                }
                PointerEventKind::Press {
                    button: 0x110,
                    serial,
                    time,
                } => {
                    self.last_serial = serial;
                    self.handler.input_time(time);
                    redraw |= self.handler.pointer_down(event.position, size);
                    emit(format!(
                        "{{\"evidence\":\"{}\",\"event\":\"input-press\",\"x\":{},\"y\":{}}}",
                        self.config.evidence, event.position.0, event.position.1
                    ));
                }
                PointerEventKind::Release {
                    button: 0x110,
                    time,
                    ..
                } => {
                    self.handler.input_time(time);
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
        serial: u32,
        time: u32,
        surface: wl_surface::WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        self.last_serial = serial;
        self.handler.input_time(time);
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
        time: u32,
        id: i32,
    ) {
        self.handler.input_time(time);
        if self.handler.touch_up(id, self.size) {
            self.draw();
        }
    }

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        time: u32,
        id: i32,
        position: (f64, f64),
    ) {
        self.handler.input_time(time);
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

impl DataDeviceHandler for Client {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
    ) {
    }

    /// The selection changed; it is read only when the surface pastes.
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }

    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
}

impl DataOfferHandler for Client {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl DataSourceHandler for Client {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: Option<String>,
    ) {
    }

    /// Writes our copied text on a thread: the reader may be this process.
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
        mime: String,
        pipe: WritePipe,
    ) {
        let ours = self
            .copy_source
            .as_ref()
            .is_some_and(|s| s.inner() == source);
        let Some(text) = self
            .copied
            .clone()
            .filter(|_| ours && TEXT_MIMES.contains(&mime.as_str()))
        else {
            return;
        };
        let fd = OwnedFd::from(pipe);
        std::thread::spawn(move || {
            let _ = std::fs::File::from(fd).write_all(text.as_bytes());
        });
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
        if self
            .copy_source
            .as_ref()
            .is_some_and(|s| s.inner() == source)
        {
            self.copy_source = None;
        }
    }

    fn dnd_dropped(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn dnd_finished(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }

    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: DndAction,
    ) {
    }
}

impl ProvidesRegistryState for Client {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Client);
delegate_data_device!(Client);
delegate_output!(Client);
delegate_pointer!(Client);
delegate_shm!(Client);
delegate_seat!(Client);
delegate_touch!(Client);
delegate_xdg_shell!(Client);
delegate_xdg_window!(Client);
delegate_registry!(Client);
