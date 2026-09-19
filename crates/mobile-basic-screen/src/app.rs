//! Guest-side Wayland client built on smithay-client-toolkit: a fullscreen
//! xdg-toplevel with a software-rendered wl_shm buffer. Emits a
//! machine-readable record only after the first frame is actually attached and
//! committed, per the evidence rules.

use std::io::Write as _;
use std::num::NonZeroU32;

use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::WaylandSurface,
    shell::xdg::XdgShell,
    shell::xdg::window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
    shm::slot::SlotPool,
    shm::{Shm, ShmHandler},
};

use crate::draw::Type;
use crate::profile::Profile;
use crate::screen;

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
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
        output_state: OutputState::new(&globals, &qh),
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
        let frame = screen::compose(&self.profile, self.minute, &self.ty, w, h);
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

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_registry!(App);
