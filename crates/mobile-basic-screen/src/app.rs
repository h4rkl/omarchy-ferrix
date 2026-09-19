//! Guest-side Wayland client built on smithay-client-toolkit: a fullscreen
//! xdg-toplevel with a software-rendered wl_shm buffer. Emits a
//! machine-readable record only after the first frame is actually attached and
//! committed, per the evidence rules.

use std::io::Write as _;
use std::num::NonZeroU32;

use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_output, wl_pointer, wl_seat, wl_shm, wl_surface, wl_touch,
};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
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

use crate::draw::Type;
use crate::interaction::{Input, UiState};
use crate::profile::Profile;
use crate::screen;

struct App {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    profile: Profile,
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
    input: Input,
    ui: UiState,
}

pub fn run(profile: &Profile, minute: u32) -> Result<(), String> {
    let ty = Type::load()?;
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))?;
    let (globals, mut event_queue) =
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

    let initial = (profile.display_width, profile.display_height);
    let pool = SlotPool::new((initial.0 * initial.1 * 4) as usize, &shm)
        .map_err(|e| format!("shm pool: {e}"))?;

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        compositor_state: compositor,
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        shm,
        pool,
        window,
        profile: profile.clone(),
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
        input: Input::default(),
        ui: UiState::new(profile.wifi == "connected", profile.bluetooth_present),
    };

    while app.running {
        event_queue
            .blocking_dispatch(&mut app)
            .map_err(|e| format!("wayland dispatch: {e}"))?;
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
        let frame = screen::compose(&self.profile, self.minute, &self.ty, w, h, &self.ui);
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
            println!(
                "{{\"evidence\":\"QEMU\",\"event\":\"basic-screen-frame\",\"status\":\"committed\",\"width\":{w},\"height\":{h},\"profile\":\"{}\"}}",
                self.profile.name
            );
            let _ = std::io::stdout().flush();
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
                println!(
                    "{{\"evidence\":\"QEMU\",\"event\":\"input-ready\",\"capability\":\"pointer\",\"status\":\"{}\"}}",
                    if cursor_ready { "ok" } else { "failed" }
                );
                let _ = std::io::stdout().flush();
            }
            Capability::Touch if self.touch.is_none() => {
                self.touch = self.seat_state.get_touch(qh, &seat).ok();
                println!(
                    "{{\"evidence\":\"QEMU\",\"event\":\"input-ready\",\"capability\":\"touch\",\"status\":\"{}\"}}",
                    if self.touch.is_some() { "ok" } else { "failed" }
                );
                let _ = std::io::stdout().flush();
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
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
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
                        println!(
                            "{{\"evidence\":\"QEMU\",\"event\":\"cursor-ready\",\"style\":\"transparent-ring\",\"size\":{}}}",
                            Self::CURSOR_SIZE
                        );
                        let _ = std::io::stdout().flush();
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
                    println!(
                        "{{\"evidence\":\"QEMU\",\"event\":\"input-press\",\"x\":{},\"y\":{}}}",
                        event.position.0, event.position.1
                    );
                    let _ = std::io::stdout().flush();
                }
                PointerEventKind::Release { button: 0x110, .. } => {
                    redraw |= self
                        .input
                        .motion(0, event.position, &mut self.ui, self.size);
                    redraw |= self.input.up(0, &mut self.ui, self.size);
                    println!(
                        "{{\"evidence\":\"QEMU\",\"event\":\"input-release\",\"page\":\"{:?}\"}}",
                        self.ui.page
                    );
                    let _ = std::io::stdout().flush();
                }
                _ => {}
            }
        }
        if redraw {
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
