//! Omarchy Ferrix Camera: an independent Wayland client launched by
//! `ferrix-appd`. It reaches the portable camera capability only through
//! the camera socket (`ferrix-protocol::camera`, ADR-009), which the
//! sandbox shows it because its descriptor declares and the grants policy
//! grants `camera`. Preview, shutter, front/back switch, stills saved to
//! `~/Pictures` as PNG, and a hand-off to Files (`show-pictures`). When the
//! target has no camera it says so and shows nothing else.
//!
//! Usage:
//!   ferrix-camera                                    # guest/nested: Wayland
//!   ferrix-camera --preview out.ppm [--page NAME] [--size WxH]
//!
//! Pages: `live` (default), `saved`, `unsupported`, `unauthorized`,
//! `unavailable`, `no-cameras`. Preview pages render a synthetic fixture
//! frame (mock evidence), never a captured one.
//! Records: `app-context`, `camera-app-frame`, `ui-state {page, state}`,
//! `camera-app-preview {facing, width, height, rotation}`,
//! `camera-app-saved {status, width, height}`, `camera-handoff {status}`,
//! `camera-back {status}`; sizes and states only, never file names.

mod interaction;
mod model;
mod screen;
mod store;
mod worker;

#[cfg(target_os = "linux")]
mod app;

use std::path::PathBuf;
use std::process::ExitCode;

use ferrix_hardware_api::{
    CameraFacing, CameraFrame, CameraId, CameraInfo, CameraMode, CameraRotation, CapabilityStatus,
    FrameSize, PixelFormat,
};
use ferrix_protocol::camera::CameraClientError;
use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{SessionEvidence, emit, json_escape, json_opt_str};
use ferrix_ui::theme;

use crate::model::{Event, Model, Saved};

const EVIDENCE: SessionEvidence = SessionEvidence;
const PREVIEW_SIZE: (u32, u32) = (1080, 2340);
pub const APP_ID: &str = "org.ferrix.camera";

fn app_context_record(evidence: &str) -> String {
    let app_id = std::env::var(ferrix_app_api::APP_ID_ENV)
        .ok()
        .filter(|v| !v.is_empty());
    let instance = std::env::var(ferrix_app_api::APP_INSTANCE_ENV)
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    format!(
        "{{\"evidence\":\"{evidence}\",\"event\":\"app-context\",\"app\":\"{APP_ID}\",\"app_id\":{},\"instance\":{}}}",
        json_opt_str(app_id.as_deref()),
        instance.map_or_else(|| "null".to_owned(), |n| n.to_string())
    )
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or_else(|| format!("--size expects WxH, got {text:?}"))?;
    match (w.parse::<u32>(), h.parse::<u32>()) {
        (Ok(w), Ok(h)) if w > 0 && h > 0 => Ok((w, h)),
        _ => Err(format!("--size expects WxH, got {text:?}")),
    }
}

/// The camera socket: `FERRIX_CAMERA_SOCKET` or the guest default.
pub fn camera_socket() -> PathBuf {
    std::env::var_os(ferrix_protocol::camera::CAMERA_SOCKET_ENV)
        .filter(|v| !v.is_empty())
        .map_or_else(
            || PathBuf::from(ferrix_protocol::camera::DEFAULT_CAMERA_SOCKET),
            PathBuf::from,
        )
}

fn fixture_info(id: &str, facing: CameraFacing, rotation: CameraRotation) -> CameraInfo {
    CameraInfo {
        id: CameraId::new(id).expect("fixture id"),
        facing,
        rotation,
        preview: FrameSize::new(480, 360),
        still: FrameSize::new(1280, 960),
        modes: [CameraMode::Preview, CameraMode::Still].into(),
    }
}

/// A host-preview fixture frame: diagonal bands and a pale top-left marker.
/// Mock evidence only; the guest shows whatever the adapter streams.
fn fixture_frame(id: &str, rotation: CameraRotation) -> CameraFrame {
    let size = FrameSize::new(480, 360);
    let mut data = Vec::with_capacity(size.bytes(PixelFormat::Rgb24));
    for y in 0..size.height {
        for x in 0..size.width {
            let rgb = if x < 60 && y < 60 {
                [0xe6, 0xd9, 0xdb]
            } else if ((x + y) / 40).is_multiple_of(2) {
                [0x66, 0xa8, 0x91]
            } else {
                [0x2a, 0x21, 0x1b]
            };
            data.extend_from_slice(&rgb);
        }
    }
    CameraFrame {
        camera: CameraId::new(id).expect("fixture id"),
        sequence: 1,
        size,
        format: PixelFormat::Rgb24,
        rotation,
        data,
    }
}

fn preview_model(page: &str) -> Result<Model, String> {
    let mut model = Model::new();
    let live = |m: &mut Model| {
        m.apply(Event::Hello(CapabilityStatus::Mocked));
        m.apply(Event::Cameras(Ok(vec![
            fixture_info("mock-cam-0", CameraFacing::Back, CameraRotation::Deg90),
            fixture_info("mock-cam-1", CameraFacing::Front, CameraRotation::Deg270),
        ])));
        m.apply(Event::Frame(fixture_frame(
            "mock-cam-0",
            CameraRotation::Deg90,
        )));
        m.pending.clear();
    };
    match page {
        "live" => live(&mut model),
        "saved" => {
            live(&mut model);
            model.apply(Event::Saved(Ok(Saved {
                name: "IMG_20261124_094100.png".to_owned(),
                width: 960,
                height: 1280,
            })));
        }
        "unsupported" => {
            model.apply(Event::Hello(CapabilityStatus::Unsupported));
        }
        "unauthorized" => {
            model.apply(Event::Hello(CapabilityStatus::Mocked));
            model.apply(Event::Cameras(Err(CameraClientError::Unauthorized)));
        }
        "unavailable" => {
            model.apply(Event::Unavailable(
                "/run/ferrix/camera.sock: No such file or directory".to_owned(),
            ));
        }
        "no-cameras" => {
            model.apply(Event::Hello(CapabilityStatus::Mocked));
            model.apply(Event::Cameras(Ok(Vec::new())));
        }
        other => {
            return Err(format!(
                "unknown --page {other:?}; use live, saved, unsupported, unauthorized, unavailable, no-cameras"
            ));
        }
    }
    Ok(model)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"camera-app-error\",\"error\":\"{}\"}}",
                json_escape(&err)
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut preview = None;
    let mut page = "live".to_owned();
    let mut size = PREVIEW_SIZE;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => preview = Some(it.next().ok_or("--preview requires an output path")?),
            "--page" => page = it.next().ok_or("--page requires a page name")?,
            "--size" => size = parse_size(&it.next().ok_or("--size requires WxH")?)?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if let Some(path) = preview {
        theme::init("mock", None);
        emit(app_context_record("mock"));
        return render_preview(&page, size, &path);
    }
    emit(app_context_record(&EVIDENCE));
    theme::init(&EVIDENCE, None);
    #[cfg(target_os = "linux")]
    {
        app::run(size)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("the Wayland session is Linux-only on this host; use --preview <out.ppm>".to_owned())
    }
}

fn render_preview(page: &str, size: (u32, u32), path: &str) -> Result<(), String> {
    let ty = Type::load()?;
    let model = preview_model(page)?;
    let canvas = screen::compose(
        &ty,
        size,
        ferrix_ui::clock::local_minute_of_day(),
        &model,
        None,
    );
    if let Some(parent) = PathBuf::from(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
    emit(format!(
        "{{\"evidence\":\"mock\",\"event\":\"camera-app-preview-page\",\"page\":\"{}\",\"state\":\"{}\",\"path\":\"{}\",\"width\":{},\"height\":{}}}",
        model.status.name(),
        json_escape(&model.state_name()),
        json_escape(path),
        size.0,
        size.1
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_pages_render() {
        let ty = Type::load().unwrap();
        for (page, status) in [
            ("live", "live"),
            ("saved", "live"),
            ("unsupported", "unsupported"),
            ("unauthorized", "unauthorized"),
            ("unavailable", "unavailable"),
            ("no-cameras", "no-cameras"),
        ] {
            let model = preview_model(page).unwrap();
            assert_eq!(model.status.name(), status, "{page}");
            let c = screen::compose(&ty, PREVIEW_SIZE, 581, &model, None);
            assert!(c.ppm_bytes().starts_with(b"P6\n1080 2340\n255\n"));
        }
        assert!(preview_model("nope").is_err());
        assert_eq!(parse_size("720x1560"), Ok((720, 1560)));
        assert!(parse_size("x").is_err());
    }

    #[test]
    fn the_live_preview_draws_the_upright_frame_with_its_marker_top_left() {
        let ty = Type::load().unwrap();
        let model = preview_model("live").unwrap();
        let c = screen::compose(&ty, PREVIEW_SIZE, 581, &model, None);
        // Rotated 90 clockwise, the sensor's top-left marker lands at the
        // viewfinder image's top-right corner.
        let l = screen::layout(PREVIEW_SIZE);
        let frame = model.frame.as_ref().unwrap();
        assert_eq!((frame.width, frame.height), (360, 480));
        let image_h = l.viewfinder.w * 480 / 360;
        let top = l.viewfinder.y + (l.viewfinder.h - image_h) / 2;
        let pixel = c.get(l.viewfinder.w - 40, top + 40);
        assert_eq!(
            pixel & 0xffffff,
            0xe6d9db,
            "marker at the upright top-right"
        );
    }
}
