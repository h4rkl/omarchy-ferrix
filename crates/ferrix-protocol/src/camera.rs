//! Camera socket wire contract and blocking client (ADR-009).
//!
//! `ferrix-core` serves the camera on its own socket (`camera.sock`), apart
//! from `core.sock`, so the sandbox can show it only to applications granted
//! `camera`, and that grant never reaches core control. Line-delimited
//! JSON, versioned by [`SCHEMA_VERSION`]:
//!
//! ```text
//! -> {"request":"hello","schema_version":1}
//! <- {"reply":"hello","schema_version":1,"status":"mocked"}
//! -> {"request":"cameras"}
//! <- {"reply":"cameras","schema_version":1,"cameras":[...]}
//! -> {"request":"capture","camera":"mock-cam-0"}
//! <- {"reply":"still","schema_version":1,"frame":{...,"data":"<base64>"}}
//! -> {"request":"preview","camera":"mock-cam-0","max_fps":8}
//! <- {"reply":"frame","schema_version":1,"frame":{...}}   (repeats until
//!    the client closes; the connection is a stream from here on)
//! ```
//!
//! Every request after hello is authorized per operation: a caller without
//! the `camera` grant gets `{"reply":"unauthorized","capability":"camera"}`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use ferrix_hardware_api::{
    CameraFrame, CameraId, CameraInfo, CapabilityStatus, HardwareError, PreviewRequest,
    SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

/// Camera socket inside the guest.
pub const DEFAULT_CAMERA_SOCKET: &str = "/run/ferrix/camera.sock";
/// Environment override for [`DEFAULT_CAMERA_SOCKET`].
pub const CAMERA_SOCKET_ENV: &str = "FERRIX_CAMERA_SOCKET";
/// The application capability every camera request needs.
pub const CAMERA_CAPABILITY: &str = "camera";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CameraRequest {
    Hello {
        schema_version: u32,
    },
    /// Enumerate cameras.
    Cameras {},
    /// Switch this connection to a preview stream of `camera`.
    Preview {
        camera: CameraId,
        max_fps: u8,
    },
    /// Take one still.
    Capture {
        camera: CameraId,
    },
    /// Start a video recording; reserved, always a typed
    /// `feature-unsupported` error today.
    Video {
        camera: CameraId,
    },
}

impl CameraRequest {
    pub fn name(&self) -> &'static str {
        match self {
            CameraRequest::Hello { .. } => "hello",
            CameraRequest::Cameras {} => "cameras",
            CameraRequest::Preview { .. } => "preview",
            CameraRequest::Capture { .. } => "capture",
            CameraRequest::Video { .. } => "video",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "kebab-case")]
pub enum CameraReply {
    /// `status` is the target's camera capability status: `unsupported`
    /// tells a client to say so without asking for cameras.
    Hello {
        schema_version: u32,
        status: CapabilityStatus,
    },
    Cameras {
        schema_version: u32,
        cameras: Vec<CameraInfo>,
    },
    Frame {
        schema_version: u32,
        frame: CameraFrame,
    },
    Still {
        schema_version: u32,
        frame: CameraFrame,
    },
    Error {
        error: HardwareError,
    },
    /// The caller is not granted [`CAMERA_CAPABILITY`].
    Unauthorized {
        capability: String,
    },
    ProtocolError {
        reason: String,
    },
}

/// Why a camera call failed, typed so a UI can say the right thing.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraClientError {
    /// The socket is missing or the service went away.
    Unavailable(String),
    /// This application has no camera grant.
    Unauthorized,
    /// The adapter answered with a typed hardware error (unsupported,
    /// invalid, failed, feature unsupported).
    Hardware(HardwareError),
    /// A malformed or unexpected reply.
    Protocol(String),
}

impl std::fmt::Display for CameraClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CameraClientError::Unavailable(reason) => {
                write!(f, "camera service unavailable: {reason}")
            }
            CameraClientError::Unauthorized => f.write_str("camera access is not granted"),
            CameraClientError::Hardware(error) => write!(f, "{error}"),
            CameraClientError::Protocol(reason) => write!(f, "camera protocol error: {reason}"),
        }
    }
}

impl std::error::Error for CameraClientError {}

fn unexpected(reply: CameraReply) -> CameraClientError {
    match reply {
        CameraReply::Error { error } => CameraClientError::Hardware(error),
        CameraReply::Unauthorized { .. } => CameraClientError::Unauthorized,
        CameraReply::ProtocolError { reason } => CameraClientError::Protocol(reason),
        other => CameraClientError::Protocol(format!("unexpected reply {other:?}")),
    }
}

/// Writes one request line.
pub fn write_request(stream: &mut UnixStream, request: &CameraRequest) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(request).map_err(std::io::Error::other)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()
}

fn read_reply(reader: &mut BufReader<UnixStream>) -> Result<CameraReply, CameraClientError> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Err(CameraClientError::Unavailable(
            "camera service closed the connection".to_owned(),
        )),
        Ok(_) => serde_json::from_str(line.trim())
            .map_err(|e| CameraClientError::Protocol(format!("decode reply: {e}"))),
        Err(e) => Err(CameraClientError::Unavailable(format!("read: {e}"))),
    }
}

/// One hello'd connection to the camera socket.
pub struct CameraClient {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    status: CapabilityStatus,
}

impl CameraClient {
    /// Connects and exchanges hello. `timeout` bounds each reply (a still
    /// may take longer than a list; pass the larger bound).
    pub fn connect(path: &Path, timeout: Duration) -> Result<Self, CameraClientError> {
        let stream = UnixStream::connect(path)
            .map_err(|e| CameraClientError::Unavailable(format!("{}: {e}", path.display())))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| CameraClientError::Unavailable(e.to_string()))?;
        let mut writer = stream
            .try_clone()
            .map_err(|e| CameraClientError::Unavailable(e.to_string()))?;
        let mut reader = BufReader::new(stream);
        write_request(
            &mut writer,
            &CameraRequest::Hello {
                schema_version: SCHEMA_VERSION,
            },
        )
        .map_err(|e| CameraClientError::Unavailable(format!("write: {e}")))?;
        match read_reply(&mut reader)? {
            CameraReply::Hello {
                schema_version,
                status,
            } if schema_version == SCHEMA_VERSION => Ok(Self {
                writer,
                reader,
                status,
            }),
            CameraReply::Hello { schema_version, .. } => Err(CameraClientError::Protocol(format!(
                "camera schema {schema_version} is incompatible with client schema {SCHEMA_VERSION}"
            ))),
            other => Err(unexpected(other)),
        }
    }

    /// The target's camera capability status from hello.
    pub fn status(&self) -> CapabilityStatus {
        self.status
    }

    fn call(&mut self, request: &CameraRequest) -> Result<CameraReply, CameraClientError> {
        write_request(&mut self.writer, request)
            .map_err(|e| CameraClientError::Unavailable(format!("write: {e}")))?;
        read_reply(&mut self.reader)
    }

    pub fn cameras(&mut self) -> Result<Vec<CameraInfo>, CameraClientError> {
        match self.call(&CameraRequest::Cameras {})? {
            CameraReply::Cameras { cameras, .. } => Ok(cameras),
            other => Err(unexpected(other)),
        }
    }

    pub fn capture(&mut self, camera: &CameraId) -> Result<CameraFrame, CameraClientError> {
        match self.call(&CameraRequest::Capture {
            camera: camera.clone(),
        })? {
            CameraReply::Still { frame, .. } => frame
                .validate()
                .map(|()| frame)
                .map_err(CameraClientError::Hardware),
            other => Err(unexpected(other)),
        }
    }

    /// Reserved: always a typed error today.
    pub fn video(&mut self, camera: &CameraId) -> Result<(), CameraClientError> {
        let reply = self.call(&CameraRequest::Video {
            camera: camera.clone(),
        })?;
        Err(unexpected(reply))
    }

    /// Turns this connection into a preview stream. The first frame (or
    /// the typed refusal) is read before returning, so an unauthorized or
    /// busy camera fails here, not on the reader thread.
    pub fn preview(
        mut self,
        camera: &CameraId,
        request: PreviewRequest,
    ) -> Result<PreviewStream, CameraClientError> {
        request.validate().map_err(CameraClientError::Hardware)?;
        match self.call(&CameraRequest::Preview {
            camera: camera.clone(),
            max_fps: request.max_fps,
        })? {
            CameraReply::Frame { frame, .. } => Ok(PreviewStream {
                first: Some(frame),
                reader: self.reader,
                _writer: self.writer,
            }),
            other => Err(unexpected(other)),
        }
    }
}

/// What a preview reader thread delivers.
#[derive(Debug)]
pub enum PreviewEvent {
    Frame(CameraFrame),
    /// The stream ended (service gone, camera error, or a stalled client
    /// was dropped); reported once, then the channel closes.
    Ended(CameraClientError),
}

/// A running preview. Dropping the receiver returned by
/// [`PreviewStream::spawn`] and the stream closes the connection, which
/// stops the camera on the service side.
pub struct PreviewStream {
    first: Option<CameraFrame>,
    reader: BufReader<UnixStream>,
    _writer: UnixStream,
}

impl PreviewStream {
    /// Reads frames on a thread; `wake` runs after every delivery. Invalid
    /// frames end the stream.
    pub fn spawn(self, wake: Arc<dyn Fn() + Send + Sync>) -> Receiver<PreviewEvent> {
        let (tx, rx) = mpsc::channel();
        let PreviewStream {
            first,
            mut reader,
            _writer,
        } = self;
        let _ = reader.get_ref().set_read_timeout(None);
        let spawned = thread::Builder::new()
            .name("camera-preview".to_owned())
            .spawn(move || {
                let _keep = _writer;
                if let Some(frame) = first {
                    if tx.send(PreviewEvent::Frame(frame)).is_err() {
                        return;
                    }
                    wake();
                }
                loop {
                    let event = match read_reply(&mut reader) {
                        Ok(CameraReply::Frame { frame, .. }) => match frame.validate() {
                            Ok(()) => PreviewEvent::Frame(frame),
                            Err(error) => PreviewEvent::Ended(CameraClientError::Hardware(error)),
                        },
                        Ok(other) => PreviewEvent::Ended(unexpected(other)),
                        Err(error) => PreviewEvent::Ended(error),
                    };
                    let ended = matches!(event, PreviewEvent::Ended(_));
                    if tx.send(event).is_err() {
                        return;
                    }
                    wake();
                    if ended {
                        return;
                    }
                }
            });
        if let Err(error) = spawned {
            let (dead_tx, dead_rx) = mpsc::channel();
            let _ = dead_tx.send(PreviewEvent::Ended(CameraClientError::Unavailable(
                format!("cannot spawn preview reader: {error}"),
            )));
            return dead_rx;
        }
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_hardware_api::{Capability, FrameSize, PixelFormat};
    use std::os::unix::net::UnixListener;

    #[test]
    fn requests_use_the_documented_wire_form_and_reject_unknown_fields() {
        let preview = CameraRequest::Preview {
            camera: CameraId::new("mock-cam-0").unwrap(),
            max_fps: 8,
        };
        assert_eq!(
            serde_json::to_string(&preview).unwrap(),
            r#"{"request":"preview","camera":"mock-cam-0","max_fps":8}"#
        );
        assert_eq!(preview.name(), "preview");
        assert_eq!(
            serde_json::from_str::<CameraRequest>(r#"{"request":"cameras"}"#).unwrap(),
            CameraRequest::Cameras {}
        );
        for bad in [
            r#"{"request":"cameras","app":"org.ferrix.camera"}"#,
            r#"{"request":"capture","camera":"BAD ID"}"#,
            r#"{"request":"explode"}"#,
        ] {
            assert!(serde_json::from_str::<CameraRequest>(bad).is_err(), "{bad}");
        }
        assert_eq!(
            serde_json::to_string(&CameraReply::Unauthorized {
                capability: CAMERA_CAPABILITY.to_owned()
            })
            .unwrap(),
            r#"{"reply":"unauthorized","capability":"camera"}"#
        );
    }

    fn frame(sequence: u64) -> CameraFrame {
        CameraFrame {
            camera: CameraId::new("mock-cam-0").unwrap(),
            sequence,
            size: FrameSize::new(2, 2),
            format: PixelFormat::Rgb24,
            rotation: Default::default(),
            data: vec![sequence as u8; 12],
        }
    }

    fn reply(stream: &mut UnixStream, reply: &CameraReply) {
        let mut line = serde_json::to_vec(reply).unwrap();
        line.push(b'\n');
        stream.write_all(&line).unwrap();
    }

    #[test]
    fn client_speaks_hello_then_maps_typed_replies_and_streams_frames() {
        let dir = std::env::temp_dir().join(format!("ferrix-camera-proto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("camera.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut lines = BufReader::new(stream).lines();
            let mut next =
                || serde_json::from_str::<CameraRequest>(&lines.next().unwrap().unwrap()).unwrap();
            assert_eq!(next(), CameraRequest::Hello { schema_version: 1 });
            reply(
                &mut writer,
                &CameraReply::Hello {
                    schema_version: 1,
                    status: CapabilityStatus::Mocked,
                },
            );
            assert_eq!(next(), CameraRequest::Cameras {});
            reply(
                &mut writer,
                &CameraReply::Unauthorized {
                    capability: "camera".into(),
                },
            );
            assert!(matches!(next(), CameraRequest::Video { .. }));
            reply(
                &mut writer,
                &CameraReply::Error {
                    error: HardwareError::feature_unsupported(Capability::Camera, "video"),
                },
            );
            assert!(matches!(next(), CameraRequest::Preview { max_fps: 4, .. }));
            for sequence in 1..=3 {
                reply(
                    &mut writer,
                    &CameraReply::Frame {
                        schema_version: 1,
                        frame: frame(sequence),
                    },
                );
            }
        });
        let mut client = CameraClient::connect(&path, Duration::from_secs(2)).unwrap();
        assert_eq!(client.status(), CapabilityStatus::Mocked);
        assert_eq!(client.cameras(), Err(CameraClientError::Unauthorized));
        let id = CameraId::new("mock-cam-0").unwrap();
        assert!(matches!(
            client.video(&id),
            Err(CameraClientError::Hardware(
                HardwareError::FeatureUnsupported { .. }
            ))
        ));
        let stream = client.preview(&id, PreviewRequest { max_fps: 4 }).unwrap();
        let rx = stream.spawn(Arc::new(|| {}));
        let mut sequences = Vec::new();
        let ended = loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                PreviewEvent::Frame(frame) => sequences.push(frame.sequence),
                PreviewEvent::Ended(error) => break error,
            }
        };
        assert_eq!(sequences, [1, 2, 3]);
        assert!(matches!(ended, CameraClientError::Unavailable(_)));
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_socket_is_unavailable() {
        let missing = std::env::temp_dir().join("ferrix-camera-proto-missing/none.sock");
        assert!(matches!(
            CameraClient::connect(&missing, Duration::from_millis(50)),
            Err(CameraClientError::Unavailable(_))
        ));
    }
}
