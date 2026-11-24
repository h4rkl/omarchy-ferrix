//! Camera contract (ADR-009).
//!
//! Enumerate cameras with their facing and sensor rotation, open a preview
//! stream, and take a still. Video is reserved: [`Camera::start_video`]
//! always answers [`HardwareError::FeatureUnsupported`] until an adapter
//! implements it. Every method defaults to a typed unsupported error, so a
//! target without a camera adapter (`UnsupportedCamera`) never imitates
//! one.
//!
//! Frames are packed 8-bit RGB in the sensor's orientation: the consumer
//! rotates by [`CameraInfo::rotation`] (clockwise degrees that bring the
//! frame upright on a portrait display) and may mirror a front preview.
//! Nothing here knows about libcamera, PipeWire, V4L2, or a board; those
//! live behind a project-owned adapter (ADR-009).

use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Capability, HardwareError};

/// The largest frame edge any adapter may hand out, in pixels. Bounds the
/// wire size of one frame (`4096 * 4096 * 3` bytes).
pub const CAMERA_FRAME_EDGE_MAX: u32 = 4096;
/// Highest preview rate a client may ask for.
pub const CAMERA_PREVIEW_FPS_MAX: u8 = 30;
/// Longest adapter-assigned camera id.
pub const CAMERA_ID_MAX: usize = 32;

/// Which way a camera looks, relative to the display.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraFacing {
    /// Away from the user (the main camera).
    Back,
    /// Toward the user (the selfie camera).
    Front,
    /// Attached externally (USB); no fixed relation to the display.
    External,
}

impl CameraFacing {
    pub fn name(self) -> &'static str {
        match self {
            CameraFacing::Back => "back",
            CameraFacing::Front => "front",
            CameraFacing::External => "external",
        }
    }
}

/// Clockwise rotation, in degrees, that turns a sensor frame upright on the
/// display in its natural (portrait) orientation. Wire form: `0`, `90`,
/// `180`, or `270`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum CameraRotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl CameraRotation {
    pub fn degrees(self) -> u16 {
        match self {
            CameraRotation::Deg0 => 0,
            CameraRotation::Deg90 => 90,
            CameraRotation::Deg180 => 180,
            CameraRotation::Deg270 => 270,
        }
    }

    /// Whether the upright image swaps the frame's width and height.
    pub fn swaps_axes(self) -> bool {
        matches!(self, CameraRotation::Deg90 | CameraRotation::Deg270)
    }
}

impl TryFrom<u16> for CameraRotation {
    type Error = String;

    fn try_from(degrees: u16) -> Result<Self, Self::Error> {
        match degrees {
            0 => Ok(CameraRotation::Deg0),
            90 => Ok(CameraRotation::Deg90),
            180 => Ok(CameraRotation::Deg180),
            270 => Ok(CameraRotation::Deg270),
            other => Err(format!(
                "camera rotation must be 0, 90, 180, or 270 (got {other})"
            )),
        }
    }
}

impl From<CameraRotation> for u16 {
    fn from(rotation: CameraRotation) -> Self {
        rotation.degrees()
    }
}

/// What a camera can do. Only `preview` and `still` are implemented by
/// any adapter today; `video` is reserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraMode {
    Preview,
    Still,
    Video,
}

impl CameraMode {
    pub fn name(self) -> &'static str {
        match self {
            CameraMode::Preview => "preview",
            CameraMode::Still => "still",
            CameraMode::Video => "video",
        }
    }
}

/// Frame geometry in pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

impl FrameSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Both edges are `1..=CAMERA_FRAME_EDGE_MAX`.
    pub fn validate(self) -> Result<(), HardwareError> {
        let ok = |edge: u32| (1..=CAMERA_FRAME_EDGE_MAX).contains(&edge);
        if ok(self.width) && ok(self.height) {
            Ok(())
        } else {
            Err(HardwareError::invalid(
                Capability::Camera,
                format!(
                    "frame size {}x{} is outside 1..={CAMERA_FRAME_EDGE_MAX}",
                    self.width, self.height
                ),
            ))
        }
    }

    /// Byte length of one packed frame in `format`.
    pub fn bytes(self, format: PixelFormat) -> usize {
        self.width as usize * self.height as usize * format.bytes_per_pixel()
    }
}

impl fmt::Display for FrameSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Pixel layout of a frame. One format today; adapters convert.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PixelFormat {
    /// Packed 8-bit R, G, B; rows are `width * 3` bytes with no padding.
    #[default]
    Rgb24,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgb24 => 3,
        }
    }
}

/// An adapter-assigned opaque camera token: 1–32 characters of `a-z`,
/// `0-9`, and `-`. Never a sensor serial, bus path, or vendor name.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CameraId(String);

impl CameraId {
    pub fn new(id: impl Into<String>) -> Result<Self, HardwareError> {
        let id = id.into();
        let valid = !id.is_empty()
            && id.len() <= CAMERA_ID_MAX
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if valid {
            Ok(Self(id))
        } else {
            Err(HardwareError::invalid(
                Capability::Camera,
                format!("camera id must be 1-{CAMERA_ID_MAX} characters of a-z, 0-9 and -"),
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CameraId {
    type Error = String;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        CameraId::new(id).map_err(|e| e.to_string())
    }
}

impl From<CameraId> for String {
    fn from(id: CameraId) -> Self {
        id.0
    }
}

impl fmt::Display for CameraId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One camera the target exposes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CameraInfo {
    pub id: CameraId,
    pub facing: CameraFacing,
    pub rotation: CameraRotation,
    /// Sensor-oriented preview frame size.
    pub preview: FrameSize,
    /// Sensor-oriented still size.
    pub still: FrameSize,
    /// What [`Camera`] operations this camera answers.
    pub modes: BTreeSet<CameraMode>,
}

impl CameraInfo {
    pub fn supports(&self, mode: CameraMode) -> bool {
        self.modes.contains(&mode)
    }
}

/// How a client wants its preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PreviewRequest {
    /// Upper bound on the delivered frame rate, `1..=CAMERA_PREVIEW_FPS_MAX`.
    pub max_fps: u8,
}

impl Default for PreviewRequest {
    fn default() -> Self {
        Self { max_fps: 10 }
    }
}

impl PreviewRequest {
    pub fn validate(self) -> Result<(), HardwareError> {
        if (1..=CAMERA_PREVIEW_FPS_MAX).contains(&self.max_fps) {
            Ok(())
        } else {
            Err(HardwareError::invalid(
                Capability::Camera,
                format!(
                    "preview max_fps must be 1..={CAMERA_PREVIEW_FPS_MAX} (got {})",
                    self.max_fps
                ),
            ))
        }
    }

    /// Shortest spacing between two delivered frames.
    pub fn frame_interval(self) -> Duration {
        Duration::from_millis(1000 / u64::from(self.max_fps.max(1)))
    }
}

/// One frame from a preview stream or a still capture. Wire form carries
/// `data` as standard base64.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CameraFrame {
    pub camera: CameraId,
    /// Monotonic per camera; a gap means frames were dropped.
    pub sequence: u64,
    pub size: FrameSize,
    pub format: PixelFormat,
    /// Copied from [`CameraInfo::rotation`] so a frame is self-describing.
    pub rotation: CameraRotation,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

impl fmt::Debug for CameraFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CameraFrame")
            .field("camera", &self.camera)
            .field("sequence", &self.sequence)
            .field("size", &self.size)
            .field("format", &self.format)
            .field("rotation", &self.rotation)
            .field("bytes", &self.data.len())
            .finish()
    }
}

impl CameraFrame {
    /// The buffer length matches the size and format, and the size is in
    /// range. Consumers check this before touching pixels.
    pub fn validate(&self) -> Result<(), HardwareError> {
        self.size.validate()?;
        let expected = self.size.bytes(self.format);
        if self.data.len() == expected {
            Ok(())
        } else {
            Err(HardwareError::invalid(
                Capability::Camera,
                format!(
                    "frame carries {} bytes; {} {:?} needs {expected}",
                    self.data.len(),
                    self.size,
                    self.format
                ),
            ))
        }
    }

    /// FNV-1a over the pixels: an evidence-safe fingerprint that tells two
    /// synthetic frames apart without recording any image content.
    pub fn digest(&self) -> u64 {
        self.data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// The RGB value at sensor coordinates `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.size.width || y >= self.size.height {
            return None;
        }
        let i = (y as usize * self.size.width as usize + x as usize) * 3;
        self.data.get(i..i + 3).map(|p| [p[0], p[1], p[2]])
    }
}

/// A running preview (or, later, video) stream. Dropping it stops the
/// stream and releases the camera.
pub trait CameraStream: Send {
    fn camera(&self) -> &CameraId;
    /// The next frame. Real adapters block until one is ready; the caller
    /// paces delivery to the requested rate.
    fn next_frame(&mut self) -> Result<CameraFrame, HardwareError>;
}

/// The camera capability. Every method defaults to a typed unsupported
/// error; adapters override only what they really provide.
pub trait Camera: Send + Sync {
    /// Every camera the target exposes, in a stable order (back cameras
    /// first, then front, then external).
    fn cameras(&self) -> Result<Vec<CameraInfo>, HardwareError> {
        Err(HardwareError::unsupported(Capability::Camera))
    }

    /// Starts a preview stream on `camera`. One stream per camera at a
    /// time; a second open while one runs fails with
    /// [`HardwareError::Failed`] (`busy`).
    fn open_preview(
        &self,
        _camera: &CameraId,
        _request: PreviewRequest,
    ) -> Result<Box<dyn CameraStream>, HardwareError> {
        Err(HardwareError::unsupported(Capability::Camera))
    }

    /// Takes one still at [`CameraInfo::still`] size.
    fn capture_still(&self, _camera: &CameraId) -> Result<CameraFrame, HardwareError> {
        Err(HardwareError::unsupported(Capability::Camera))
    }

    /// Video recording is reserved: no adapter implements it yet.
    fn start_video(&self, _camera: &CameraId) -> Result<Box<dyn CameraStream>, HardwareError> {
        Err(HardwareError::feature_unsupported(
            Capability::Camera,
            CameraMode::Video.name(),
        ))
    }
}

/// The default for every target without a camera adapter: every operation
/// is [`HardwareError::Unsupported`].
#[derive(Clone, Copy, Debug, Default)]
pub struct UnsupportedCamera;

impl Camera for UnsupportedCamera {}

/// Sorts cameras into the documented order: back, front, external, then
/// by id.
pub fn sort_cameras(cameras: &mut [CameraInfo]) {
    cameras.sort_by(|a, b| a.facing.cmp(&b.facing).then_with(|| a.id.cmp(&b.id)));
}

mod base64_bytes {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        STANDARD
            .decode(text.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, facing: CameraFacing) -> CameraInfo {
        CameraInfo {
            id: CameraId::new(id).unwrap(),
            facing,
            rotation: CameraRotation::Deg90,
            preview: FrameSize::new(4, 3),
            still: FrameSize::new(8, 6),
            modes: [CameraMode::Preview, CameraMode::Still].into(),
        }
    }

    #[test]
    fn unsupported_camera_refuses_everything_with_typed_errors() {
        let camera = UnsupportedCamera;
        let id = CameraId::new("cam-0").unwrap();
        let unsupported = HardwareError::unsupported(Capability::Camera);
        assert_eq!(camera.cameras(), Err(unsupported.clone()));
        assert_eq!(
            camera
                .open_preview(&id, PreviewRequest::default())
                .err()
                .unwrap(),
            unsupported
        );
        assert_eq!(camera.capture_still(&id), Err(unsupported));
        let video = camera.start_video(&id).err().unwrap();
        assert_eq!(
            serde_json::to_string(&video).unwrap(),
            r#"{"kind":"feature-unsupported","capability":"camera","feature":"video"}"#
        );
        assert_eq!(
            video.to_string(),
            "camera video is unsupported on this target"
        );
    }

    #[test]
    fn ids_rotations_and_sizes_are_validated() {
        assert!(CameraId::new("mock-cam-0").is_ok());
        for bad in ["", "Cam", "a/b", "x y", &"a".repeat(CAMERA_ID_MAX + 1)] {
            assert!(CameraId::new(bad).is_err(), "{bad:?}");
            assert!(serde_json::from_str::<CameraId>(&format!("{bad:?}")).is_err());
        }
        assert_eq!(
            serde_json::to_string(&CameraRotation::Deg270).unwrap(),
            "270"
        );
        assert_eq!(
            serde_json::from_str::<CameraRotation>("90").unwrap(),
            CameraRotation::Deg90
        );
        assert!(serde_json::from_str::<CameraRotation>("45").is_err());
        assert!(CameraRotation::Deg90.swaps_axes() && !CameraRotation::Deg180.swaps_axes());
        assert!(FrameSize::new(0, 10).validate().is_err());
        assert!(
            FrameSize::new(CAMERA_FRAME_EDGE_MAX + 1, 10)
                .validate()
                .is_err()
        );
        assert_eq!(FrameSize::new(4, 3).bytes(PixelFormat::Rgb24), 36);
        assert!(PreviewRequest { max_fps: 0 }.validate().is_err());
        assert!(PreviewRequest { max_fps: 31 }.validate().is_err());
        assert_eq!(
            PreviewRequest { max_fps: 10 }.frame_interval(),
            Duration::from_millis(100)
        );
    }

    #[test]
    fn frames_round_trip_as_base64_and_validate_their_length() {
        let frame = CameraFrame {
            camera: CameraId::new("cam-0").unwrap(),
            sequence: 7,
            size: FrameSize::new(2, 1),
            format: PixelFormat::Rgb24,
            rotation: CameraRotation::Deg0,
            data: vec![1, 2, 3, 4, 5, 6],
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""data":"AQIDBAUG""#), "{json}");
        let back: CameraFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, frame);
        assert!(back.validate().is_ok());
        assert_eq!(back.pixel(1, 0), Some([4, 5, 6]));
        assert_eq!(back.pixel(2, 0), None);
        assert_ne!(back.digest(), 0);
        let short = CameraFrame {
            data: vec![0; 5],
            ..frame
        };
        assert!(short.validate().is_err());
        assert!(
            !format!("{short:?}").contains("data"),
            "debug never dumps pixels"
        );
    }

    #[test]
    fn cameras_sort_back_front_external() {
        let mut list = vec![
            info("c", CameraFacing::External),
            info("b", CameraFacing::Front),
            info("z", CameraFacing::Back),
            info("a", CameraFacing::Back),
        ];
        sort_cameras(&mut list);
        let ids: Vec<&str> = list.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["a", "z", "b", "c"]);
        assert!(list[0].supports(CameraMode::Still));
        assert!(!list[0].supports(CameraMode::Video));
        let json = serde_json::to_string(&list[0]).unwrap();
        assert_eq!(
            json,
            r#"{"id":"a","facing":"back","rotation":90,"preview":{"width":4,"height":3},"still":{"width":8,"height":6},"modes":["preview","still"]}"#
        );
    }
}
