//! Camera app state: what the service said, which camera is active, the
//! latest upright preview frame, and the save/hand-off outcomes. Pure:
//! no sockets, no Wayland. The worker feeds [`Model::apply`] and the
//! surface drains [`Model::pending`].

// The Wayland half (app.rs) is Linux-only; host builds use the rest in tests
// and previews.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_hardware_api::{
    CameraFacing, CameraFrame, CameraId, CameraInfo, CameraMode, CameraRotation, CapabilityStatus,
    HardwareError,
};
use ferrix_protocol::camera::CameraClientError;
use ferrix_ui::widgets::Tone;

/// What the camera service answered, as the user should hear it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Waiting for the camera service.
    Connecting,
    /// The target's capability matrix has no camera: say so, never fake it.
    Unsupported,
    /// This app holds no `camera` grant.
    Unauthorized,
    /// The service is not reachable (with its reason).
    Unavailable(String),
    /// The capability exists but the target lists no cameras.
    NoCameras,
    /// Cameras are listed; the preview runs or is (re)starting.
    Live,
    /// A typed hardware failure the user cannot fix from here.
    Failed(String),
}

impl Status {
    pub fn name(&self) -> &'static str {
        match self {
            Status::Connecting => "connecting",
            Status::Unsupported => "unsupported",
            Status::Unauthorized => "unauthorized",
            Status::Unavailable(_) => "unavailable",
            Status::NoCameras => "no-cameras",
            Status::Live => "live",
            Status::Failed(_) => "failed",
        }
    }
}

/// Requests the surface hands to the worker or to `ferrix-appd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    OpenPreview(CameraId),
    Capture(CameraId),
    /// Open Files on `~/Pictures` (the `show-pictures` intent).
    ShowPictures,
    /// Reconnect after the service went away.
    Retry,
    /// Leave: yield the foreground to Home.
    Back,
}

/// Events the worker reports.
#[derive(Debug)]
pub enum Event {
    Hello(CapabilityStatus),
    Cameras(Result<Vec<CameraInfo>, CameraClientError>),
    /// The preview of `camera` could not start.
    PreviewFailed(CameraId, CameraClientError),
    /// A frame of the running preview.
    Frame(CameraFrame),
    /// The running preview ended.
    PreviewEnded(CameraClientError),
    Still(Result<CameraFrame, CameraClientError>),
    /// The still was written (`name` is the bare file name).
    Saved(Result<Saved, String>),
    Unavailable(String),
}

/// A frame turned upright for display (straight-alpha RGBA8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upright {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub sequence: u64,
}

/// Rotates a sensor frame by its clockwise rotation and optionally mirrors
/// it horizontally (front previews read like a mirror). Returns packed RGB
/// when `alpha` is false, RGBA otherwise.
pub fn upright_pixels(frame: &CameraFrame, mirror: bool, alpha: bool) -> (u32, u32, Vec<u8>) {
    let (w, h) = (frame.size.width as usize, frame.size.height as usize);
    let (dw, dh) = if frame.rotation.swaps_axes() {
        (h, w)
    } else {
        (w, h)
    };
    let channels = if alpha { 4 } else { 3 };
    let mut out = Vec::with_capacity(dw * dh * channels);
    for y in 0..dh {
        for x in 0..dw {
            let x = if mirror { dw - 1 - x } else { x };
            let (sx, sy) = match frame.rotation {
                CameraRotation::Deg0 => (x, y),
                CameraRotation::Deg90 => (y, h - 1 - x),
                CameraRotation::Deg180 => (w - 1 - x, h - 1 - y),
                CameraRotation::Deg270 => (w - 1 - y, x),
            };
            let i = (sy * w + sx) * 3;
            out.extend_from_slice(&frame.data[i..i + 3]);
            if alpha {
                out.push(0xff);
            }
        }
    }
    (dw as u32, dh as u32, out)
}

/// One saved still.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub name: String,
    pub width: u32,
    pub height: u32,
}

pub struct Model {
    pub status: Status,
    pub cameras: Vec<CameraInfo>,
    pub active: usize,
    pub frame: Option<Upright>,
    /// Preview frames shown since the camera was opened.
    pub frames: u64,
    pub capturing: bool,
    pub saved: u32,
    pub last_saved: Option<Saved>,
    pub notice: Option<(String, Tone)>,
    pub pending: Vec<Action>,
    /// The capability status from hello (`mock` is said out loud).
    pub capability: Option<CapabilityStatus>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

fn hardware_text(error: &HardwareError) -> String {
    match error {
        HardwareError::Unsupported { .. } => "no camera on this target".to_owned(),
        HardwareError::FeatureUnsupported { feature, .. } => format!("{feature} is not supported"),
        HardwareError::InvalidValue { reason, .. } | HardwareError::Failed { reason, .. } => {
            reason.clone()
        }
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            status: Status::Connecting,
            cameras: Vec::new(),
            active: 0,
            frame: None,
            frames: 0,
            capturing: false,
            saved: 0,
            last_saved: None,
            notice: None,
            pending: Vec::new(),
            capability: None,
        }
    }

    pub fn active_camera(&self) -> Option<&CameraInfo> {
        self.cameras.get(self.active)
    }

    /// The camera the switch button would select.
    pub fn next_camera(&self) -> Option<&CameraInfo> {
        (self.cameras.len() > 1).then(|| &self.cameras[(self.active + 1) % self.cameras.len()])
    }

    pub fn can_capture(&self) -> bool {
        self.status == Status::Live
            && !self.capturing
            && self
                .active_camera()
                .is_some_and(|c| c.supports(CameraMode::Still))
    }

    /// Records a client error as the status it implies.
    fn client_error(&mut self, error: CameraClientError) {
        self.frame = None;
        self.status = match error {
            CameraClientError::Unauthorized => Status::Unauthorized,
            CameraClientError::Unavailable(reason) => Status::Unavailable(reason),
            CameraClientError::Hardware(HardwareError::Unsupported { .. }) => Status::Unsupported,
            CameraClientError::Hardware(error) => Status::Failed(hardware_text(&error)),
            CameraClientError::Protocol(reason) => Status::Failed(reason),
        };
    }

    /// Folds one worker event; returns whether the surface must redraw.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Hello(status) => {
                self.capability = Some(status);
                if status == CapabilityStatus::Unsupported {
                    self.status = Status::Unsupported;
                }
            }
            Event::Cameras(Ok(mut cameras)) => {
                ferrix_hardware_api::sort_cameras(&mut cameras);
                // A re-list (reconnect) keeps the camera the user chose.
                let chosen = self.active_camera().map(|c| c.id.clone());
                self.active = chosen
                    .and_then(|id| cameras.iter().position(|c| c.id == id))
                    .unwrap_or(0);
                self.cameras = cameras;
                match self.active_camera().map(|c| c.id.clone()) {
                    Some(id) => {
                        self.status = Status::Live;
                        self.pending.push(Action::OpenPreview(id));
                    }
                    None => self.status = Status::NoCameras,
                }
            }
            Event::Cameras(Err(error)) => self.client_error(error),
            Event::PreviewFailed(camera, error) => {
                if self.active_camera().map(|c| &c.id) == Some(&camera) {
                    self.client_error(error);
                }
            }
            Event::Frame(frame) => {
                let Some(active) = self.active_camera() else {
                    return false;
                };
                if frame.camera != active.id {
                    return false;
                }
                let mirror = active.facing == CameraFacing::Front;
                let (width, height, rgba) = upright_pixels(&frame, mirror, true);
                self.frame = Some(Upright {
                    width,
                    height,
                    rgba,
                    sequence: frame.sequence,
                });
                self.frames += 1;
                if self.status != Status::Live {
                    self.status = Status::Live;
                }
            }
            Event::PreviewEnded(error) => self.client_error(error),
            Event::Still(Err(error)) => {
                self.capturing = false;
                let text = match &error {
                    CameraClientError::Hardware(error) => hardware_text(error),
                    other => other.to_string(),
                };
                self.notice = Some((format!("Not saved \u{b7} {text}"), Tone::Err));
            }
            Event::Still(Ok(_)) => {
                // Still capturing until the worker reports the save.
            }
            Event::Saved(Ok(saved)) => {
                self.capturing = false;
                self.saved += 1;
                self.notice = Some((format!("Saved {} \u{b7} Pictures", saved.name), Tone::Ok));
                self.last_saved = Some(saved);
            }
            Event::Saved(Err(reason)) => {
                self.capturing = false;
                self.notice = Some((format!("Not saved \u{b7} {reason}"), Tone::Err));
            }
            Event::Unavailable(reason) => {
                self.frame = None;
                self.status = Status::Unavailable(reason);
            }
        }
        true
    }

    pub fn shutter(&mut self) -> bool {
        if !self.can_capture() {
            return false;
        }
        let Some(id) = self.active_camera().map(|c| c.id.clone()) else {
            return false;
        };
        self.capturing = true;
        self.notice = None;
        self.pending.push(Action::Capture(id));
        true
    }

    pub fn switch(&mut self) -> bool {
        if self.status != Status::Live || self.next_camera().is_none() {
            return false;
        }
        self.active = (self.active + 1) % self.cameras.len();
        self.frame = None;
        self.frames = 0;
        let id = self.cameras[self.active].id.clone();
        self.pending.push(Action::OpenPreview(id));
        true
    }

    pub fn gallery(&mut self) -> bool {
        self.pending.push(Action::ShowPictures);
        false
    }

    pub fn retry(&mut self) -> bool {
        if !matches!(self.status, Status::Unavailable(_) | Status::Failed(_)) {
            return false;
        }
        self.status = Status::Connecting;
        self.pending.push(Action::Retry);
        true
    }

    /// The `ui-state` state string: facing, frame count, saves. No names.
    pub fn state_name(&self) -> String {
        let facing = self.active_camera().map_or("none", |c| c.facing.name());
        let preview = if self.frame.is_some() {
            "streaming"
        } else {
            "idle"
        };
        format!(
            "{facing} {preview} cameras={} saved={}{}",
            self.cameras.len(),
            self.saved,
            if self.capturing { " capturing" } else { "" }
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ferrix_hardware_api::{FrameSize, PixelFormat};

    pub fn info(id: &str, facing: CameraFacing, rotation: CameraRotation) -> CameraInfo {
        CameraInfo {
            id: CameraId::new(id).unwrap(),
            facing,
            rotation,
            preview: FrameSize::new(4, 3),
            still: FrameSize::new(8, 6),
            modes: [CameraMode::Preview, CameraMode::Still].into(),
        }
    }

    /// A 3x2 frame whose pixel (x, y) is `[x, y, 0]`.
    pub fn frame(id: &str, rotation: CameraRotation, sequence: u64) -> CameraFrame {
        let (w, h) = (3u32, 2u32);
        let data = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, 0]))
            .collect();
        CameraFrame {
            camera: CameraId::new(id).unwrap(),
            sequence,
            size: FrameSize::new(w, h),
            format: PixelFormat::Rgb24,
            rotation,
            data,
        }
    }

    fn at(pixels: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 2] {
        let i = ((y * pixels.0 + x) * 3) as usize;
        [pixels.2[i], pixels.2[i + 1]]
    }

    #[test]
    fn rotation_and_mirroring_turn_sensor_frames_upright() {
        let r0 = upright_pixels(&frame("a", CameraRotation::Deg0, 1), false, false);
        assert_eq!((r0.0, r0.1), (3, 2));
        assert_eq!(at(&r0, 2, 1), [2, 1]);
        // 90 clockwise: the sensor's top-left lands top-right.
        let r90 = upright_pixels(&frame("a", CameraRotation::Deg90, 1), false, false);
        assert_eq!((r90.0, r90.1), (2, 3));
        assert_eq!(at(&r90, 1, 0), [0, 0]);
        assert_eq!(at(&r90, 0, 0), [0, 1]);
        let r180 = upright_pixels(&frame("a", CameraRotation::Deg180, 1), false, false);
        assert_eq!(at(&r180, 2, 1), [0, 0]);
        // 270 clockwise: the sensor's top-left lands bottom-left.
        let r270 = upright_pixels(&frame("a", CameraRotation::Deg270, 1), false, false);
        assert_eq!((r270.0, r270.1), (2, 3));
        assert_eq!(at(&r270, 0, 2), [0, 0]);
        let mirrored = upright_pixels(&frame("a", CameraRotation::Deg0, 1), true, false);
        assert_eq!(at(&mirrored, 0, 0), [2, 0]);
        let rgba = upright_pixels(&frame("a", CameraRotation::Deg0, 1), false, true);
        assert_eq!(rgba.2.len(), 3 * 2 * 4);
    }

    fn live() -> Model {
        let mut m = Model::new();
        m.apply(Event::Hello(CapabilityStatus::Mocked));
        m.apply(Event::Cameras(Ok(vec![
            info("cam-f", CameraFacing::Front, CameraRotation::Deg270),
            info("cam-b", CameraFacing::Back, CameraRotation::Deg90),
        ])));
        m
    }

    #[test]
    fn cameras_open_back_first_and_switch_alternates() {
        let mut m = live();
        assert_eq!(m.status, Status::Live);
        assert_eq!(m.active_camera().unwrap().facing, CameraFacing::Back);
        assert_eq!(
            std::mem::take(&mut m.pending),
            [Action::OpenPreview(CameraId::new("cam-b").unwrap())]
        );
        // A frame from another camera (a late one after a switch) is ignored.
        assert!(!m.apply(Event::Frame(frame("cam-f", CameraRotation::Deg270, 1))));
        assert!(m.apply(Event::Frame(frame("cam-b", CameraRotation::Deg90, 2))));
        assert_eq!(m.frame.as_ref().unwrap().width, 2);
        assert!(m.switch());
        assert_eq!(m.active_camera().unwrap().facing, CameraFacing::Front);
        assert!(m.frame.is_none());
        assert_eq!(
            std::mem::take(&mut m.pending),
            [Action::OpenPreview(CameraId::new("cam-f").unwrap())]
        );
        assert!(m.state_name().starts_with("front idle cameras=2"));
    }

    #[test]
    fn shutter_waits_for_the_save_and_reports_typed_failures() {
        let mut m = live();
        m.pending.clear();
        assert!(m.shutter());
        assert!(!m.shutter(), "one capture at a time");
        assert_eq!(
            m.pending,
            [Action::Capture(CameraId::new("cam-b").unwrap())]
        );
        m.apply(Event::Still(Ok(frame("cam-b", CameraRotation::Deg90, 3))));
        assert!(m.capturing);
        m.apply(Event::Saved(Ok(Saved {
            name: "IMG_20261124_094100.png".into(),
            width: 6,
            height: 8,
        })));
        assert!(!m.capturing);
        assert_eq!(m.saved, 1);
        assert_eq!(m.notice.as_ref().unwrap().1, Tone::Ok);
        assert!(m.shutter());
        m.apply(Event::Still(Err(CameraClientError::Hardware(
            HardwareError::failed(ferrix_hardware_api::Capability::Camera, "busy"),
        ))));
        assert_eq!(
            m.notice.as_ref().unwrap(),
            &("Not saved \u{b7} busy".to_owned(), Tone::Err)
        );
        assert!(!m.gallery());
        assert_eq!(m.pending.last(), Some(&Action::ShowPictures));
    }

    #[test]
    fn unsupported_unauthorized_and_unavailable_are_honest_states() {
        let mut m = Model::new();
        m.apply(Event::Hello(CapabilityStatus::Unsupported));
        assert_eq!(m.status, Status::Unsupported);
        assert!(!m.shutter() && !m.switch() && !m.retry());

        let mut m = Model::new();
        m.apply(Event::Cameras(Err(CameraClientError::Unauthorized)));
        assert_eq!(m.status, Status::Unauthorized);

        let mut m = Model::new();
        m.apply(Event::Cameras(Err(CameraClientError::Hardware(
            HardwareError::unsupported(ferrix_hardware_api::Capability::Camera),
        ))));
        assert_eq!(m.status, Status::Unsupported);

        let mut m = live();
        m.apply(Event::PreviewEnded(CameraClientError::Unavailable(
            "gone".into(),
        )));
        assert_eq!(m.status.name(), "unavailable");
        assert!(m.retry());
        assert_eq!(m.pending.last(), Some(&Action::Retry));

        let mut m = Model::new();
        m.apply(Event::Cameras(Ok(Vec::new())));
        assert_eq!(m.status, Status::NoCameras);
    }
}
