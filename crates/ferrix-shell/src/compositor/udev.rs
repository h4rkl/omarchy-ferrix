//! QEMU guest backend: libseat session, udev DRM discovery, libinput, and a
//! `DrmCompositor` fed by the pixman software renderer through DRM dumb
//! buffers. No GL/GBM rendering is involved (libgbm is linked only because
//! Smithay's `DrmCompositor` type is feature-gated on it; it is passed `None`).

use std::time::Duration;

use ferrix_protocol::Subscription;

use smithay::{
    backend::{
        allocator::{Fourcc, Modifier, dumb::DumbAllocator, format::FormatSet, gbm::GbmDevice},
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent,
            compositor::{DrmCompositor, FrameFlags},
        },
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{ImportMemWl, pixman::PixmanRenderer},
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, primary_gpu},
    },
    output::{Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::EventLoop,
        drm::{
            Device as BasicDevice,
            control::{Device as ControlDevice, ModeTypeFlags, connector, crtc},
        },
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::DeviceFd,
};

use super::{
    Shell, init_tracing, output_mode, render, rotation_to_transform, scanout::ScanoutExporter,
};
use crate::{
    Failure,
    config::ShellConfig,
    evidence::{self, Evidence, Value},
    layout::OutputLayout,
};

type ShellCompositor = DrmCompositor<DumbAllocator, ScanoutExporter, (), DrmDeviceFd>;

/// Connector, CRTC and mode chosen for the pinned geometry.
struct Target {
    connector: connector::Info,
    crtc: crtc::Handle,
    mode: smithay::reexports::drm::control::Mode,
    mode_source: &'static str,
}

/// Build a CVT-like mode for an arbitrary WxH at 60 Hz. virtio-gpu accepts any
/// timing, which is why the Weston route also used a synthetic modeline.
fn synthesize_mode(width: u32, height: u32) -> smithay::reexports::drm::control::Mode {
    let htotal = width + 160;
    let vtotal = height + 40;
    let clock_khz = htotal * vtotal * 60 / 1000;
    let mut name = [0 as std::ffi::c_char; 32];
    for (dst, byte) in name.iter_mut().zip(format!("{width}x{height}").bytes()) {
        *dst = byte as std::ffi::c_char;
    }
    drm_ffi::drm_mode_modeinfo {
        clock: clock_khz,
        hdisplay: width as u16,
        hsync_start: (width + 48) as u16,
        hsync_end: (width + 80) as u16,
        htotal: htotal as u16,
        hskew: 0,
        vdisplay: height as u16,
        vsync_start: (height + 3) as u16,
        vsync_end: (height + 8) as u16,
        vtotal: vtotal as u16,
        vscan: 0,
        vrefresh: 60,
        flags: drm_ffi::DRM_MODE_FLAG_NHSYNC | drm_ffi::DRM_MODE_FLAG_PVSYNC,
        type_: drm_ffi::DRM_MODE_TYPE_USERDEF,
        name,
    }
    .into()
}

fn pick_target(drm: &DrmDevice, width: u32, height: u32) -> Result<Target, Failure> {
    let resources = drm
        .resource_handles()
        .map_err(|e| ("drm", format!("resource handles: {e}")))?;
    for handle in resources.connectors() {
        let Ok(info) = drm.get_connector(*handle, true) else {
            continue;
        };
        if info.state() != connector::State::Connected {
            continue;
        }
        let crtc = info
            .current_encoder()
            .and_then(|e| drm.get_encoder(e).ok())
            .and_then(|e| e.crtc())
            .or_else(|| {
                info.encoders()
                    .iter()
                    .filter_map(|e| drm.get_encoder(*e).ok())
                    .find_map(|encoder| {
                        resources
                            .filter_crtcs(encoder.possible_crtcs())
                            .first()
                            .copied()
                    })
            });
        let Some(crtc) = crtc else {
            continue;
        };
        let wanted = (width as u16, height as u16);
        let (mode, mode_source) = match info.modes().iter().find(|m| m.size() == wanted) {
            Some(mode) => (*mode, "connector"),
            None if width <= u16::MAX as u32 && height <= u16::MAX as u32 => {
                (synthesize_mode(width, height), "synthesized")
            }
            None => {
                let preferred = info
                    .modes()
                    .iter()
                    .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
                    .or_else(|| info.modes().first())
                    .copied()
                    .ok_or(("drm", "connector has no modes".to_owned()))?;
                (preferred, "preferred")
            }
        };
        return Ok(Target {
            connector: info,
            crtc,
            mode,
            mode_source,
        });
    }
    Err(("drm", "no connected DRM connector".to_owned()))
}

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

    let (mut session, notifier) = LibSeatSession::new().map_err(|e| {
        (
            "session",
            format!(
                "libseat session failed ({e}); is seatd running or LIBSEAT_BACKEND=builtin set?"
            ),
        )
    })?;
    let seat_name = session.seat();

    let udev = UdevBackend::new(&seat_name).map_err(|e| ("udev", e.to_string()))?;
    let path = match std::env::var("FERRIX_DRM_DEVICE") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => primary_gpu(&seat_name)
            .ok()
            .flatten()
            .or_else(|| udev.device_list().next().map(|(_, p)| p.to_path_buf()))
            .ok_or(("drm", "no DRM device found by udev".to_owned()))?,
    };
    let fd = session
        .open(
            &path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|e| ("drm", format!("open {}: {e}", path.display())))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm, drm_notifier) =
        DrmDevice::new(fd.clone(), true).map_err(|e| ("drm", format!("device: {e}")))?;
    let driver = drm
        .get_driver()
        .map(|d| d.name().to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".to_owned());

    let target = pick_target(&drm, plan.panel_width, plan.panel_height)?;
    let (mode_w, mode_h) = target.mode.size();
    let output_name = format!(
        "{}-{}",
        target.connector.interface().as_str(),
        target.connector.interface_id()
    );
    evidence::emit(&evidence::record(
        evidence,
        "drm-output",
        &[
            ("device", path.display().to_string().into()),
            ("driver", driver.as_str().into()),
            ("connector", output_name.as_str().into()),
            ("mode_width", u32::from(mode_w).into()),
            ("mode_height", u32::from(mode_h).into()),
            ("mode_source", target.mode_source.into()),
            ("atomic", Value::Bool(drm.is_atomic())),
            ("renderer", "pixman".into()),
            ("allocator", "drm-dumb-buffers".into()),
        ],
    ));

    let surface = drm
        .create_surface(target.crtc, target.mode, &[target.connector.handle()])
        .map_err(|e| ("drm", format!("surface: {e}")))?;

    let (phys_w, phys_h) = target.connector.size().unwrap_or((0, 0));
    let output = Output::new(
        output_name,
        PhysicalProperties {
            size: (phys_w as i32, phys_h as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "QEMU".into(),
            model: driver.clone(),
        },
    );
    let wl_mode = output_mode(
        i32::from(mode_w),
        i32::from(mode_h),
        (target.mode.vrefresh() * 1000) as i32,
    );
    // The plan was computed for the requested geometry; if the connector only
    // offered another mode, re-plan so the logical size matches scan-out.
    let plan = OutputLayout::plan(u32::from(mode_w), u32::from(mode_h), config.orientation);
    let scanout_byte_order = config.scanout_byte_order;
    output.change_current_state(
        Some(wl_mode),
        Some(rotation_to_transform(plan.rotation)),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(wl_mode);

    let mut shell = Shell::new(
        &mut display,
        &event_loop,
        config,
        plan,
        evidence,
        output.clone(),
        &seat_name,
        core,
    )?;

    let mut renderer = PixmanRenderer::new().map_err(|e| ("renderer", format!("pixman: {e}")))?;
    shell.shm_state.update_formats(renderer.shm_formats());

    // Software composition only: no overlay/cursor planes, everything is drawn
    // by pixman into the primary-plane dumb buffer.
    let mut planes = drm
        .planes(&target.crtc)
        .map_err(|e| ("drm", format!("planes: {e}")))?;
    planes.overlay.clear();
    planes.cursor.clear();
    // Dumb buffers are linear. Drivers without IN_FORMATS (e.g. the Pixel 9a
    // exynos DPU) advertise plane formats with the implicit modifier, so offer
    // both or the format negotiation finds no common plane format.
    let color_formats = scanout_formats();
    let renderer_formats: FormatSet = color_formats
        .iter()
        .copied()
        .flat_map(|code| {
            [Modifier::Linear, Modifier::Invalid]
                .map(|modifier| smithay::backend::allocator::Format { code, modifier })
        })
        .collect();
    let mut compositor: ShellCompositor = DrmCompositor::new(
        &output,
        surface,
        Some(planes),
        DumbAllocator::new(fd.clone()),
        ScanoutExporter::new(fd.clone(), scanout_byte_order),
        color_formats,
        renderer_formats,
        drm.cursor_size(),
        None::<GbmDevice<DrmDeviceFd>>,
    )
    .map_err(|e| ("drm", format!("compositor: {e}")))?;

    // libinput through the libseat session (virtio-keyboard, virtio-tablet).
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat_name)
        .map_err(|_| ("libinput", "udev_assign_seat failed".to_owned()))?;
    let input_backend = LibinputInputBackend::new(libinput.clone());
    shell
        .loop_handle
        .insert_source(input_backend, |event, _, shell| {
            shell.process_input_event::<LibinputInputBackend>(event);
        })
        .map_err(|e| ("libinput", e.to_string()))?;
    let mut libinput_for_session = libinput.clone();
    shell
        .loop_handle
        .insert_source(notifier, move |event, _, shell| match event {
            SessionEvent::PauseSession => {
                libinput_for_session.suspend();
                shell.emit("session", &[("state", "paused".into())]);
            }
            SessionEvent::ActivateSession => {
                if let Err(e) = libinput_for_session.resume() {
                    tracing::warn!("libinput resume failed: {e:?}");
                }
                shell.emit("session", &[("state", "active".into())]);
                shell.request_redraw();
            }
        })
        .map_err(|e| ("session", e.to_string()))?;

    // VBlank completions are flagged on the shell and drained by the loop
    // below (the compositor object lives here, not in the loop data).
    let vblank = std::rc::Rc::new(std::cell::Cell::new(false));
    let vblank_flag = vblank.clone();
    shell
        .loop_handle
        .insert_source(drm_notifier, move |event, _, _shell| match event {
            DrmEvent::VBlank(_) => vblank_flag.set(true),
            DrmEvent::Error(e) => tracing::error!("drm event error: {e}"),
        })
        .map_err(|e| ("drm", e.to_string()))?;

    let (logical_w, logical_h) = shell.plan.logical_size();
    evidence::emit(&evidence::compositor_ready(
        evidence,
        &shell.socket_name,
        logical_w,
        logical_h,
    ));

    let mut frame_pending = false;
    let mut frames_presented: u64 = 0;
    let mut first_frame_reported = false;
    let mut idle_ticks: u32 = 0;
    while shell.running {
        event_loop
            .dispatch(Some(Duration::from_millis(16)), &mut shell)
            .map_err(|e| ("event-loop", e.to_string()))?;
        shell.dispatch_clients(&mut display)?;
        shell.cleanup();

        if vblank.replace(false) {
            match compositor.frame_submitted() {
                Ok(_) => {
                    frames_presented += 1;
                    if !first_frame_reported {
                        first_frame_reported = true;
                        shell.emit(
                            "compositor-frame",
                            &[
                                ("status", "presented".into()),
                                ("renderer", "pixman".into()),
                            ],
                        );
                    }
                }
                Err(e) => tracing::warn!("frame_submitted: {e}"),
            }
            frame_pending = false;
            // Anything committed while the flip was in flight is drawn now.
            shell.request_redraw();
        }

        if !frame_pending && shell.take_redraw() {
            let full_redraw = shell.take_full_redraw();
            let focused_pid = shell.focused_pid();
            if full_redraw {
                // A fullscreen focus switch replaces the entire scene. Reset
                // buffer ages so DrmCompositor redraws the new client even if
                // its surface damage/element id resembles the old one.
                compositor.reset_buffer_ages();
            }
            let elements = render::collect_elements(&shell, &mut renderer);
            match compositor.render_frame(
                &mut renderer,
                &elements,
                render::clear_color(),
                FrameFlags::empty(),
            ) {
                Ok(result) => {
                    if full_redraw {
                        shell.emit(
                            "focus-frame",
                            &[
                                ("pid", focused_pid.into()),
                                ("empty", Value::Bool(result.is_empty)),
                            ],
                        );
                    }
                    if !result.is_empty {
                        match compositor.queue_frame(()) {
                            Ok(()) => frame_pending = true,
                            Err(e) => {
                                tracing::warn!("queue_frame: {e}");
                                compositor.reset_buffers();
                                shell.request_redraw();
                            }
                        }
                    }
                    shell.send_frames(shell.clock.now().into());
                }
                Err(e) => {
                    tracing::warn!("render_frame: {e}");
                    idle_ticks += 1;
                    if idle_ticks > 600 {
                        return Err(("render", format!("repeated render failures: {e}")));
                    }
                    compositor.reset_buffers();
                    shell.request_redraw();
                }
            }
        }
    }
    shell.emit(
        "compositor-exit",
        &[("frames", Value::Int(frames_presented as i64))],
    );
    Ok(())
}

/// Scan-out formats in preference order. `FERRIX_SHELL_SCANOUT_FORMATS`
/// (comma-separated fourcc codes such as `XB24,AB24`) is a hardware bring-up
/// diagnostic for display controllers whose byte order or alpha handling
/// differs from what their format list implies.
fn scanout_formats() -> Vec<Fourcc> {
    let default = vec![Fourcc::Xrgb8888, Fourcc::Argb8888];
    let Ok(value) = std::env::var("FERRIX_SHELL_SCANOUT_FORMATS") else {
        return default;
    };
    let parsed: Vec<Fourcc> = value
        .split(',')
        .filter_map(|code| match code.trim() {
            "XR24" => Some(Fourcc::Xrgb8888),
            "AR24" => Some(Fourcc::Argb8888),
            "XB24" => Some(Fourcc::Xbgr8888),
            "AB24" => Some(Fourcc::Abgr8888),
            "RX24" => Some(Fourcc::Rgbx8888),
            "BX24" => Some(Fourcc::Bgrx8888),
            "RA24" => Some(Fourcc::Rgba8888),
            "BA24" => Some(Fourcc::Bgra8888),
            _ => None,
        })
        .collect();
    if parsed.is_empty() { default } else { parsed }
}
