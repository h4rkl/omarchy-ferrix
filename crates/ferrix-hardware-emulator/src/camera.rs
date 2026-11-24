//! Synthetic cameras for the QEMU virtual phone and host tests (ADR-009).
//!
//! Each `[[camera.devices]]` profile entry becomes one camera with an
//! adapter-assigned id (`mock-cam-<n>`). Frames are deterministic test
//! patterns computed from the camera's pattern and the frame sequence:
//! nothing is captured and nothing reads a device. Every frame carries an
//! orientation marker (a pale block in the sensor's top-left corner) so a
//! rendered preview shows whether the consumer applied the rotation.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ferrix_hardware_api::{
    Camera, CameraFrame, CameraId, CameraInfo, CameraMode, CameraStream, Capability, FrameSize,
    HardwareError, PixelFormat, PreviewRequest, sort_cameras,
};
use ferrix_target_profile::{Profile, ProfileCamera, ProfileCameraPattern};

/// Bars, left to right: the Ferrix signal colours plus two neutrals.
const BARS: [[u8; 3]; 8] = [
    [0xe7, 0xaa, 0x5a],
    [0xbd, 0x49, 0x24],
    [0x66, 0xa8, 0x91],
    [0xf9, 0xcc, 0x6c],
    [0xfd, 0x68, 0x83],
    [0xa8, 0xa9, 0xeb],
    [0x6f, 0x68, 0x68],
    [0x2a, 0x21, 0x1b],
];
const MARKER: [u8; 3] = [0xe6, 0xd9, 0xdb];
const SWEEP: [u8; 3] = [0xf1, 0xe5, 0xe7];
const GRID_A: [u8; 3] = [0x66, 0xa8, 0x91];
const GRID_B: [u8; 3] = [0x17, 0x13, 0x11];

#[derive(Debug)]
struct Device {
    info: CameraInfo,
    pattern: ProfileCameraPattern,
    sequence: u64,
    streaming: bool,
}

#[derive(Debug)]
struct Inner {
    devices: Vec<Device>,
    max_fps: u8,
}

/// The mock camera adapter. Clones share state.
#[derive(Clone, Debug)]
pub struct MockCamera {
    inner: Arc<Mutex<Inner>>,
}

fn info_for(index: usize, device: &ProfileCamera) -> CameraInfo {
    CameraInfo {
        id: CameraId::new(format!("mock-cam-{index}")).expect("mock ids are valid"),
        facing: device.facing,
        rotation: device.rotation,
        preview: device.preview,
        still: device.still,
        modes: BTreeSet::from([CameraMode::Preview, CameraMode::Still]),
    }
}

impl MockCamera {
    /// Cameras from `[camera]`; an absent section means none.
    pub fn from_profile(profile: &Profile) -> Self {
        let (devices, max_fps) = match &profile.camera {
            Some(section) => (
                section
                    .devices
                    .iter()
                    .enumerate()
                    .map(|(i, d)| Device {
                        info: info_for(i, d),
                        pattern: d.pattern,
                        sequence: 0,
                        streaming: false,
                    })
                    .collect(),
                section.preview_fps,
            ),
            None => (Vec::new(), 1),
        };
        Self {
            inner: Arc::new(Mutex::new(Inner { devices, max_fps })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The profile's preview-rate ceiling.
    pub fn max_fps(&self) -> u8 {
        self.lock().max_fps
    }

    fn with_device<T>(
        &self,
        camera: &CameraId,
        f: impl FnOnce(&mut Device) -> Result<T, HardwareError>,
    ) -> Result<T, HardwareError> {
        let mut inner = self.lock();
        let device = inner
            .devices
            .iter_mut()
            .find(|d| &d.info.id == camera)
            .ok_or_else(|| {
                HardwareError::invalid(Capability::Camera, format!("no camera {camera}"))
            })?;
        f(device)
    }

    /// One frame of `device` at `size`, advancing its sequence.
    fn frame(device: &mut Device, size: FrameSize) -> CameraFrame {
        device.sequence += 1;
        CameraFrame {
            camera: device.info.id.clone(),
            sequence: device.sequence,
            size,
            format: PixelFormat::Rgb24,
            rotation: device.info.rotation,
            data: render(device.pattern, size, device.sequence),
        }
    }
}

/// Draws one pattern frame. Pure: the same inputs give the same pixels.
pub fn render(pattern: ProfileCameraPattern, size: FrameSize, sequence: u64) -> Vec<u8> {
    let (w, h) = (size.width as usize, size.height as usize);
    let mut data = Vec::with_capacity(w * h * 3);
    let marker = (w / 8, h / 6);
    let sweep_w = (w / 120).max(2);
    let sweep_x = (sequence as usize * (w / 60).max(1)) % w;
    let cell = (h / 9).max(1);
    let phase = (sequence as usize * (cell / 4).max(1)) % (2 * cell);
    for y in 0..h {
        for x in 0..w {
            let rgb = if x < marker.0 && y < marker.1 {
                MARKER
            } else {
                match pattern {
                    ProfileCameraPattern::Bars => {
                        if x >= sweep_x && x < sweep_x + sweep_w {
                            SWEEP
                        } else if y * 3 < h * 2 {
                            BARS[x * BARS.len() / w]
                        } else {
                            let v = (x * 255 / w.max(1)) as u8;
                            [v, v, v]
                        }
                    }
                    ProfileCameraPattern::Grid => {
                        if ((x + phase) / cell + y / cell).is_multiple_of(2) {
                            GRID_A
                        } else {
                            GRID_B
                        }
                    }
                }
            };
            data.extend_from_slice(&rgb);
        }
    }
    data
}

impl Camera for MockCamera {
    fn cameras(&self) -> Result<Vec<CameraInfo>, HardwareError> {
        let mut list: Vec<CameraInfo> =
            self.lock().devices.iter().map(|d| d.info.clone()).collect();
        sort_cameras(&mut list);
        Ok(list)
    }

    fn open_preview(
        &self,
        camera: &CameraId,
        request: PreviewRequest,
    ) -> Result<Box<dyn CameraStream>, HardwareError> {
        request.validate()?;
        self.with_device(camera, |device| {
            if device.streaming {
                return Err(HardwareError::failed(
                    Capability::Camera,
                    format!("{camera} busy: a preview is already running"),
                ));
            }
            device.streaming = true;
            Ok(())
        })?;
        let fps = u64::from(self.max_fps().max(1));
        Ok(Box::new(MockStream {
            camera: self.clone(),
            id: camera.clone(),
            min_interval: Duration::from_millis(1000 / fps),
            last: None,
        }))
    }

    fn capture_still(&self, camera: &CameraId) -> Result<CameraFrame, HardwareError> {
        self.with_device(camera, |device| {
            let size = device.info.still;
            Ok(Self::frame(device, size))
        })
    }
}

/// A preview stream that never runs faster than the profile's
/// `preview_fps`, as a sensor has a fixed maximum rate.
struct MockStream {
    camera: MockCamera,
    id: CameraId,
    min_interval: Duration,
    last: Option<Instant>,
}

impl CameraStream for MockStream {
    fn camera(&self) -> &CameraId {
        &self.id
    }

    fn next_frame(&mut self) -> Result<CameraFrame, HardwareError> {
        if let Some(rest) = self
            .last
            .and_then(|last| self.min_interval.checked_sub(last.elapsed()))
        {
            std::thread::sleep(rest);
        }
        self.last = Some(Instant::now());
        self.camera.with_device(&self.id, |device| {
            let size = device.info.preview;
            Ok(MockCamera::frame(device, size))
        })
    }
}

impl Drop for MockStream {
    fn drop(&mut self) {
        let _ = self.camera.with_device(&self.id, |device| {
            device.streaming = false;
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_hardware_api::{CameraFacing, CameraRotation};

    fn camera() -> MockCamera {
        let profile = Profile::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        ))
        .expect("checked-in profile");
        MockCamera::from_profile(&profile)
    }

    #[test]
    fn profile_cameras_enumerate_back_first_with_facing_and_rotation() {
        let cameras = camera().cameras().unwrap();
        assert_eq!(cameras.len(), 2);
        assert_eq!(cameras[0].facing, CameraFacing::Back);
        assert_eq!(cameras[0].rotation, CameraRotation::Deg90);
        assert_eq!(cameras[1].facing, CameraFacing::Front);
        assert_eq!(cameras[1].rotation, CameraRotation::Deg270);
        assert_eq!(cameras[0].id.as_str(), "mock-cam-0");
        assert!(cameras.iter().all(|c| c.supports(CameraMode::Preview)
            && c.supports(CameraMode::Still)
            && !c.supports(CameraMode::Video)));
        assert_eq!(camera().max_fps(), 8);
    }

    #[test]
    fn preview_frames_are_valid_sequenced_and_change_over_time() {
        let mock = camera();
        let back = mock.cameras().unwrap().remove(0);
        let mut stream = mock
            .open_preview(&back.id, PreviewRequest { max_fps: 8 })
            .unwrap();
        let a = stream.next_frame().unwrap();
        let b = stream.next_frame().unwrap();
        assert!(a.validate().is_ok() && b.validate().is_ok());
        assert_eq!(a.size, back.preview);
        assert_eq!((a.sequence, b.sequence), (1, 2));
        assert_ne!(a.digest(), b.digest(), "the sweep moves");
        assert_eq!(a.pixel(0, 0), Some(MARKER), "orientation marker top-left");
        // One preview per camera at a time; dropping the stream frees it.
        let busy = mock.open_preview(&back.id, PreviewRequest::default());
        assert!(matches!(busy, Err(HardwareError::Failed { .. })));
        drop(stream);
        assert!(
            mock.open_preview(&back.id, PreviewRequest::default())
                .is_ok()
        );
    }

    #[test]
    fn stills_use_the_still_size_and_rendering_is_deterministic() {
        let mock = camera();
        let front = mock.cameras().unwrap().remove(1);
        let still = mock.capture_still(&front.id).unwrap();
        assert_eq!(still.size, front.still);
        assert!(still.validate().is_ok());
        assert_eq!(
            render(ProfileCameraPattern::Grid, front.still, still.sequence),
            still.data
        );
        assert!(
            mock.start_video(&front.id).is_err(),
            "video stays typed unsupported"
        );
    }

    #[test]
    fn unknown_cameras_and_bad_requests_are_typed_errors() {
        let mock = camera();
        let ghost = CameraId::new("mock-cam-9").unwrap();
        assert!(matches!(
            mock.capture_still(&ghost),
            Err(HardwareError::InvalidValue { .. })
        ));
        let back = mock.cameras().unwrap().remove(0);
        assert!(matches!(
            mock.open_preview(&back.id, PreviewRequest { max_fps: 0 }),
            Err(HardwareError::InvalidValue { .. })
        ));
    }

    #[test]
    fn a_profile_without_a_camera_section_lists_none() {
        let mut profile = Profile::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        ))
        .unwrap();
        profile.camera = None;
        assert_eq!(MockCamera::from_profile(&profile).cameras().unwrap(), []);
    }
}
