//! Nested development backend: a winit window on a Linux host session, driven
//! from the same versioned mock profile. Evidence from this route is `mock`.

use std::time::Duration;

use ferrix_protocol::Subscription;

use smithay::{
    backend::{
        renderer::{ImportDma, ImportMemWl, damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent},
    },
    output::{Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::EventLoop,
        wayland_server::Display,
        winit::{
            dpi::PhysicalSize, platform::pump_events::PumpStatus, window::Window as WinitWindow,
        },
    },
};

use super::{Shell, init_tracing, output_mode, render, rotation_to_transform};
use crate::{
    Failure,
    config::ShellConfig,
    evidence::{self, Evidence, Value},
    layout::OutputLayout,
};

pub fn run(
    config: ShellConfig,
    plan: OutputLayout,
    evidence: Evidence,
    core: Subscription,
) -> Result<(), Failure> {
    init_tracing();
    let mut event_loop: EventLoop<'static, Shell> =
        EventLoop::try_new().map_err(|e| ("event-loop", e.to_string()))?;
    let mut display: Display<Shell> = Display::new().map_err(|e| ("display", e.to_string()))?;

    let attributes = WinitWindow::default_attributes()
        .with_inner_size(PhysicalSize::new(plan.panel_width, plan.panel_height))
        .with_title(format!(
            "ferrix-shell nested ({} {}x{}, mock profile)",
            config.profile_name, plan.panel_width, plan.panel_height
        ))
        .with_visible(true);
    let (mut backend, mut winit) = winit::init_from_attributes::<GlesRenderer>(attributes)
        .map_err(|e| {
            (
                "winit",
                format!("{e} (nested mode needs a Linux Wayland/X11 host session)"),
            )
        })?;
    let size = backend.window_size();

    let output = Output::new(
        "nested-1".to_owned(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "ferrix-shell".into(),
            model: "nested".into(),
        },
    );
    let plan = OutputLayout::plan(size.w as u32, size.h as u32, config.orientation);
    let mode = output_mode(size.w, size.h, 60_000);
    output.change_current_state(
        Some(mode),
        Some(rotation_to_transform(plan.rotation)),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    let mut shell = Shell::new(
        &mut display,
        &event_loop,
        config,
        plan,
        evidence,
        output.clone(),
        "nested",
        core,
    )?;
    shell
        .shm_state
        .update_formats(backend.renderer().shm_formats());
    shell.enable_dmabuf(backend.renderer().dmabuf_formats());
    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    let (logical_w, logical_h) = shell.plan.logical_size();
    evidence::emit(&evidence::compositor_ready(
        evidence,
        &shell.socket_name,
        logical_w,
        logical_h,
    ));

    let mut frames: u64 = 0;
    while shell.running {
        let status = winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, .. } => {
                let mode = output_mode(size.w, size.h, 60_000);
                output.change_current_state(Some(mode), None, None, None);
                output.set_preferred(mode);
                let plan =
                    OutputLayout::plan(size.w as u32, size.h as u32, shell.config.orientation);
                shell.apply_plan(plan);
            }
            WinitEvent::Input(event) => shell.process_input_event(event),
            WinitEvent::CloseRequested => shell.running = false,
            WinitEvent::Redraw => shell.request_redraw(),
            WinitEvent::Focus(_) => {}
        });
        if let PumpStatus::Exit(_) = status {
            shell.running = false;
            break;
        }

        event_loop
            .dispatch(Some(Duration::from_millis(8)), &mut shell)
            .map_err(|e| ("event-loop", e.to_string()))?;
        shell.dispatch_clients(&mut display)?;
        shell.cleanup();

        if shell.take_redraw() {
            let age = backend.buffer_age().unwrap_or(0);
            let (renderer, mut framebuffer) = backend
                .bind()
                .map_err(|e| ("render", format!("bind: {e}")))?;
            let elements = render::collect_elements(&shell, renderer);
            let result = damage_tracker
                .render_output(
                    renderer,
                    &mut framebuffer,
                    age,
                    &elements,
                    render::clear_color(),
                )
                .map_err(|e| ("render", format!("render_output: {e}")))?;
            let damage = result.damage.cloned();
            drop(framebuffer);
            if let Some(damage) = damage {
                backend
                    .submit(Some(&damage))
                    .map_err(|e| ("render", format!("submit: {e}")))?;
                frames += 1;
                if frames == 1 {
                    shell.emit(
                        "compositor-frame",
                        &[("status", "presented".into()), ("renderer", "gles".into())],
                    );
                }
            }
            shell.send_frames(shell.clock.now().into());
        }
    }
    shell.emit("compositor-exit", &[("frames", Value::Int(frames as i64))]);
    Ok(())
}
