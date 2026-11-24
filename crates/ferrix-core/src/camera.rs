//! Camera socket (ADR-009): serves the session's [`Camera`] capability to
//! applications on its own socket, `camera.sock`, so the sandbox exposes it
//! only to applications granted `camera` and that grant never reaches the
//! core's control verbs on `core.sock`.
//!
//! Every request after hello is authorized per operation (ADR-004): the
//! peer pid comes from the kernel (`SO_PEERCRED`), `ferrix-appd` resolves it
//! to an application and its effective grants, and the request is refused
//! with a typed `unauthorized` reply unless `camera` is among them.
//!
//! The mock-control channel gets four client verbs (`camera-list`,
//! `camera-capture`, `camera-preview`, `camera-video`) that drive the same
//! capability for scenarios. Records carry ids, facings, sizes, sequence
//! numbers and an FNV digest, never pixels: `camera-ready`, `camera-list`,
//! `camera-still`, `camera-preview`, `camera-request-denied`,
//! `camera-error`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_app_api::AppCapability;
use ferrix_app_protocol::{Client, ClientRole};
use ferrix_hardware_api::{
    Camera, CameraFacing, CameraFrame, CameraId, CameraInfo, Capability, HardwareError,
    PreviewRequest, SCHEMA_VERSION,
};
use ferrix_protocol::camera::{CAMERA_CAPABILITY, CameraReply, CameraRequest};
use ferrix_session::Session;
use serde_json::{Value, json};

/// A stalled reader (a frozen or wedged application) ends its preview
/// after this long instead of holding the camera.
const PREVIEW_WRITE_TIMEOUT: Duration = Duration::from_secs(3);
/// Most frames one `camera-preview` control verb samples.
pub const CONTROL_PREVIEW_FRAMES_MAX: u32 = 30;

/// Who is calling, as far as authorization is concerned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Caller {
    pub app: Option<String>,
    pub camera: bool,
}

/// Resolves a kernel peer pid to a [`Caller`].
pub trait CameraAuth: Send + Sync {
    fn identify(&self, pid: Option<u32>) -> Caller;
}

/// No application service (`--appd none`): nobody holds `camera`.
pub struct DenyAll;

impl CameraAuth for DenyAll {
    fn identify(&self, _pid: Option<u32>) -> Caller {
        Caller::default()
    }
}

/// Asks `ferrix-appd` (role `service`) per request; any failure denies, and
/// the next lookup reconnects.
pub struct AppdAuth {
    socket: PathBuf,
    timeout: Duration,
    link: Mutex<Option<Client>>,
}

impl AppdAuth {
    pub fn new(socket: PathBuf, timeout: Duration) -> Self {
        Self {
            socket,
            timeout,
            link: Mutex::new(None),
        }
    }

    fn lookup(&self, pid: u32) -> Result<Caller, String> {
        let mut link = self.link.lock().unwrap_or_else(|p| p.into_inner());
        if link.is_none() {
            *link = Some(Client::connect(
                &self.socket,
                ClientRole::Service,
                self.timeout,
            )?);
        }
        let client = link.as_mut().expect("connected above");
        match client.identify(pid) {
            Ok(identity) => Ok(Caller {
                app: identity.app.map(|a| a.to_string()),
                camera: identity.granted.contains(&AppCapability::Camera),
            }),
            Err(error) => {
                *link = None;
                Err(error)
            }
        }
    }
}

impl CameraAuth for AppdAuth {
    fn identify(&self, pid: Option<u32>) -> Caller {
        let Some(pid) = pid else {
            return Caller::default();
        };
        // One retry covers an appd restart between two lookups.
        self.lookup(pid)
            .or_else(|_| self.lookup(pid))
            .unwrap_or_default()
    }
}

#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    rustix::net::sockopt::socket_peercred(stream)
        .ok()
        .and_then(|credentials| u32::try_from(credentials.pid.as_raw_pid()).ok())
}

#[cfg(not(target_os = "linux"))]
fn peer_pid(_stream: &UnixStream) -> Option<u32> {
    None
}

fn record(event: &str, fields: Value) -> Value {
    let mut value = json!({"evidence": crate::evidence_label(), "event": event});
    if let (Some(map), Value::Object(extra)) = (value.as_object_mut(), fields) {
        map.extend(extra);
    }
    value
}

fn error_record(request: &str, error: &HardwareError) -> Value {
    record(
        "camera-error",
        json!({"request": request, "error": serde_json::to_value(error).unwrap_or(Value::Null)}),
    )
}

fn still_record(info: Option<&CameraInfo>, frame: &CameraFrame, via: &str) -> Value {
    record(
        "camera-still",
        json!({
            "camera": frame.camera.as_str(),
            "facing": info.map(|i| i.facing.name()),
            "width": frame.size.width,
            "height": frame.size.height,
            "rotation": frame.rotation.degrees(),
            "sequence": frame.sequence,
            "digest": format!("{:016x}", frame.digest()),
            "via": via,
        }),
    )
}

fn info_of(camera: &dyn Camera, id: &CameraId) -> Option<CameraInfo> {
    camera
        .cameras()
        .ok()
        .and_then(|list| list.into_iter().find(|c| &c.id == id))
}

pub struct CameraServer {
    session: Arc<Session>,
    auth: Arc<dyn CameraAuth>,
}

impl CameraServer {
    pub fn new(session: Arc<Session>, auth: Arc<dyn CameraAuth>) -> Self {
        Self { session, auth }
    }

    pub fn bind(path: &Path) -> Result<UnixListener, String> {
        crate::server::Server::bind(path)
    }

    /// The `camera-ready` record for `socket`.
    pub fn ready_record(&self, socket: &Path) -> Value {
        let status = self.session.capabilities.status(Capability::Camera);
        let cameras = self.session.services.camera.cameras();
        record(
            "camera-ready",
            json!({
                "socket": socket.display().to_string(),
                "status": status,
                "cameras": cameras.as_ref().map(Vec::len).ok(),
                "facings": cameras.map(|l| l.iter().map(|c| c.facing.name()).collect::<Vec<_>>()).ok(),
            }),
        )
    }

    pub fn serve(self: Arc<Self>, listener: UnixListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let server = Arc::clone(&self);
                    thread::spawn(move || server.handle(stream));
                }
                Err(e) => crate::emit(record(
                    "camera-socket-error",
                    json!({"error": e.to_string()}),
                )),
            }
        }
    }

    fn camera(&self) -> &dyn Camera {
        &*self.session.services.camera
    }

    /// One connection: hello, then requests until the client leaves or a
    /// preview takes the connection over.
    pub fn handle(&self, stream: UnixStream) {
        let pid = peer_pid(&stream);
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut writer = stream;
        let mut lines = BufReader::new(read_half).lines();
        let mut greeted = false;
        while let Some(Ok(line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let request = match serde_json::from_str::<CameraRequest>(line) {
                Ok(request) => request,
                Err(e) => {
                    let reply = CameraReply::ProtocolError {
                        reason: format!("invalid request: {e}"),
                    };
                    if write_json(&mut writer, &reply).is_err() {
                        return;
                    }
                    continue;
                }
            };
            if let CameraRequest::Hello { schema_version } = request {
                let reply = if schema_version == SCHEMA_VERSION {
                    greeted = true;
                    CameraReply::Hello {
                        schema_version: SCHEMA_VERSION,
                        status: self.session.capabilities.status(Capability::Camera),
                    }
                } else {
                    CameraReply::ProtocolError {
                        reason: format!(
                            "schema {schema_version} is incompatible with camera schema {SCHEMA_VERSION}"
                        ),
                    }
                };
                if write_json(&mut writer, &reply).is_err() || !greeted {
                    return;
                }
                continue;
            }
            if !greeted {
                let _ = write_json(
                    &mut writer,
                    &CameraReply::ProtocolError {
                        reason: "hello must come first".to_owned(),
                    },
                );
                return;
            }
            let caller = self.auth.identify(pid);
            if !caller.camera {
                crate::emit(record(
                    "camera-request-denied",
                    json!({"app": caller.app, "capability": CAMERA_CAPABILITY, "request": request.name()}),
                ));
                let reply = CameraReply::Unauthorized {
                    capability: CAMERA_CAPABILITY.to_owned(),
                };
                if write_json(&mut writer, &reply).is_err() {
                    return;
                }
                continue;
            }
            let reply = match &request {
                CameraRequest::Hello { .. } => unreachable!("handled above"),
                CameraRequest::Cameras {} => match self.camera().cameras() {
                    Ok(cameras) => CameraReply::Cameras {
                        schema_version: SCHEMA_VERSION,
                        cameras,
                    },
                    Err(error) => CameraReply::Error { error },
                },
                CameraRequest::Capture { camera } => match self.camera().capture_still(camera) {
                    Ok(frame) => {
                        crate::emit(still_record(
                            info_of(self.camera(), camera).as_ref(),
                            &frame,
                            "socket",
                        ));
                        CameraReply::Still {
                            schema_version: SCHEMA_VERSION,
                            frame,
                        }
                    }
                    Err(error) => CameraReply::Error { error },
                },
                CameraRequest::Video { camera } => match self.camera().start_video(camera) {
                    Ok(_) => CameraReply::Error {
                        error: HardwareError::failed(
                            Capability::Camera,
                            "video streams are not served",
                        ),
                    },
                    Err(error) => CameraReply::Error { error },
                },
                CameraRequest::Preview { camera, max_fps } => {
                    self.preview(&mut writer, caller.app.as_deref(), camera, *max_fps);
                    return;
                }
            };
            if let CameraReply::Error { error } = &reply {
                crate::emit(error_record(request.name(), error));
            }
            if write_json(&mut writer, &reply).is_err() {
                return;
            }
        }
    }

    /// Streams frames until the client goes away, stalls, or the camera
    /// fails; the stream (and so the camera) is released on return.
    fn preview(&self, writer: &mut UnixStream, app: Option<&str>, camera: &CameraId, max_fps: u8) {
        let request = PreviewRequest { max_fps };
        let mut stream = match self.camera().open_preview(camera, request) {
            Ok(stream) => stream,
            Err(error) => {
                crate::emit(error_record("preview", &error));
                let _ = write_json(writer, &CameraReply::Error { error });
                return;
            }
        };
        let _ = writer.set_write_timeout(Some(PREVIEW_WRITE_TIMEOUT));
        let facing = info_of(self.camera(), camera).map(|i| i.facing.name());
        crate::emit(record(
            "camera-preview",
            json!({"status": "started", "camera": camera.as_str(), "facing": facing, "fps": max_fps, "app": app}),
        ));
        let interval = request.frame_interval();
        let mut frames: u64 = 0;
        let reason = loop {
            let started = Instant::now();
            let frame = match stream.next_frame() {
                Ok(frame) => frame,
                Err(error) => {
                    let _ = write_json(
                        writer,
                        &CameraReply::Error {
                            error: error.clone(),
                        },
                    );
                    break format!("camera: {error}");
                }
            };
            let reply = CameraReply::Frame {
                schema_version: SCHEMA_VERSION,
                frame,
            };
            if let Err(e) = write_json(writer, &reply) {
                break format!("client: {e}");
            }
            frames += 1;
            if let Some(rest) = interval.checked_sub(started.elapsed()) {
                thread::sleep(rest);
            }
        };
        drop(stream);
        crate::emit(record(
            "camera-preview",
            json!({"status": "stopped", "camera": camera.as_str(), "facing": facing, "frames": frames, "reason": reason}),
        ));
    }
}

/// The first camera with `facing`, or a typed invalid-value error.
fn by_facing(camera: &dyn Camera, facing: CameraFacing) -> Result<CameraInfo, HardwareError> {
    camera
        .cameras()?
        .into_iter()
        .find(|c| c.facing == facing)
        .ok_or_else(|| {
            HardwareError::invalid(Capability::Camera, format!("no {} camera", facing.name()))
        })
}

/// What a camera control verb did: the ack name, the records it produced
/// (already emitted by the caller), and their event names for the ack.
pub struct ControlOutcome {
    pub ack: &'static str,
    pub records: Vec<Value>,
}

impl ControlOutcome {
    pub fn events(&self) -> Vec<String> {
        self.records
            .iter()
            .filter_map(|r| r["event"].as_str().map(str::to_owned))
            .collect()
    }
}

/// Runs a mock-control camera verb through the portable capability.
/// `None` when `command` is not a camera verb.
pub fn control(
    camera: &dyn Camera,
    command: &crate::protocol::ControlCommand,
) -> Option<Result<ControlOutcome, String>> {
    use crate::protocol::ControlCommand as C;
    let result = match command {
        C::CameraList => camera.cameras().map(|list| ControlOutcome {
            ack: "camera-list",
            records: vec![record(
                "camera-list",
                json!({
                    "count": list.len(),
                    "facings": list.iter().map(|c| c.facing.name()).collect::<Vec<_>>(),
                    "rotations": list.iter().map(|c| c.rotation.degrees()).collect::<Vec<_>>(),
                }),
            )],
        }),
        C::CameraCapture { facing } => by_facing(camera, *facing).and_then(|info| {
            let frame = camera.capture_still(&info.id)?;
            frame.validate()?;
            Ok(ControlOutcome {
                ack: "camera-capture",
                records: vec![still_record(Some(&info), &frame, "control")],
            })
        }),
        C::CameraPreview { facing, frames } => {
            if !(1..=CONTROL_PREVIEW_FRAMES_MAX).contains(frames) {
                return Some(Err(format!(
                    "camera-preview frames must be 1..={CONTROL_PREVIEW_FRAMES_MAX}"
                )));
            }
            by_facing(camera, *facing).and_then(|info| {
                let mut stream = camera.open_preview(&info.id, PreviewRequest::default())?;
                let mut sequences = Vec::new();
                let mut digests = std::collections::BTreeSet::new();
                for _ in 0..*frames {
                    let frame = stream.next_frame()?;
                    frame.validate()?;
                    sequences.push(frame.sequence);
                    digests.insert(frame.digest());
                }
                Ok(ControlOutcome {
                    ack: "camera-preview",
                    records: vec![record(
                        "camera-preview",
                        json!({
                            "status": "sampled",
                            "camera": info.id.as_str(),
                            "facing": info.facing.name(),
                            "frames": sequences.len(),
                            "width": info.preview.width,
                            "height": info.preview.height,
                            "monotonic": sequences.windows(2).all(|w| w[0] < w[1]),
                            "distinct": digests.len(),
                        }),
                    )],
                })
            })
        }
        C::CameraVideo { facing } => by_facing(camera, *facing)
            .and_then(|info| camera.start_video(&info.id).map(|_| ()))
            .map(|()| ControlOutcome {
                ack: "camera-video",
                records: Vec::new(),
            }),
        _ => return None,
    };
    Some(result.map_err(|error| {
        crate::emit(error_record(command.name(), &error));
        error.to_string()
    }))
}

fn write_json(stream: &mut UnixStream, value: &impl serde::Serialize) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ControlCommand;
    use ferrix_hardware_api::{CapabilityStatus, UnsupportedCamera};
    use ferrix_protocol::camera::{CameraClient, CameraClientError, PreviewEvent};
    use ferrix_session::{BootstrapOptions, bootstrap};

    fn session() -> Arc<Session> {
        Arc::new(
            bootstrap(BootstrapOptions::default().with_profile_path(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../targets/qemu-aarch64/profile.toml"
            )))
            .unwrap(),
        )
    }

    struct AllowAll;
    impl CameraAuth for AllowAll {
        fn identify(&self, _pid: Option<u32>) -> Caller {
            Caller {
                app: Some("org.ferrix.camera".into()),
                camera: true,
            }
        }
    }

    fn serve(auth: Arc<dyn CameraAuth>, name: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("ferrix-core-camera-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("camera.sock");
        let listener = CameraServer::bind(&path).unwrap();
        let server = Arc::new(CameraServer::new(session(), auth));
        thread::spawn(move || server.serve(listener));
        (dir, path)
    }

    #[test]
    fn granted_clients_list_capture_and_stream_and_video_stays_unsupported() {
        let (dir, path) = serve(Arc::new(AllowAll), "granted");
        let mut client = CameraClient::connect(&path, Duration::from_secs(5)).unwrap();
        assert_eq!(client.status(), CapabilityStatus::Mocked);
        let cameras = client.cameras().unwrap();
        assert_eq!(cameras.len(), 2);
        let still = client.capture(&cameras[0].id).unwrap();
        assert_eq!(still.size, cameras[0].still);
        assert!(matches!(
            client.video(&cameras[0].id),
            Err(CameraClientError::Hardware(
                HardwareError::FeatureUnsupported { .. }
            ))
        ));
        let stream = client
            .preview(&cameras[1].id, PreviewRequest { max_fps: 30 })
            .unwrap();
        let rx = stream.spawn(Arc::new(|| {}));
        let mut seen = Vec::new();
        while seen.len() < 3 {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                PreviewEvent::Frame(frame) => {
                    assert_eq!(frame.size, cameras[1].preview);
                    seen.push(frame.sequence);
                }
                PreviewEvent::Ended(error) => panic!("{error}"),
            }
        }
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ungranted_callers_get_typed_unauthorized_and_hello_comes_first() {
        let (dir, path) = serve(Arc::new(DenyAll), "denied");
        let mut client = CameraClient::connect(&path, Duration::from_secs(5)).unwrap();
        assert_eq!(client.cameras(), Err(CameraClientError::Unauthorized));
        let id = CameraId::new("mock-cam-0").unwrap();
        assert_eq!(
            client.capture(&id).err(),
            Some(CameraClientError::Unauthorized)
        );
        assert!(matches!(
            client.preview(&id, PreviewRequest::default()),
            Err(CameraClientError::Unauthorized)
        ));
        // A raw request before hello is refused and the connection closed.
        let mut raw = UnixStream::connect(&path).unwrap();
        raw.write_all(b"{\"request\":\"cameras\"}\n").unwrap();
        let mut line = String::new();
        BufReader::new(raw).read_line(&mut line).unwrap();
        assert!(line.contains("protocol-error"), "{line}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Replays every checked-in `camera-*` scenario against the camera
    /// control verbs in-process (`mock`; `just qemu-scenario-check` runs
    /// the same files in QEMU). Error records are emitted on stdout, not
    /// returned, so only success records are matched here.
    #[test]
    fn camera_scenarios_replay_against_the_mock_session() {
        use ferrix_scenario::{Scenario, Service, is_subset, match_in_order};
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/scenarios"
        );
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("camera-") && n.ends_with(".toml"))
            })
            .collect();
        files.sort();
        assert!(!files.is_empty(), "a camera scenario is checked in");
        let session = session();
        for file in files {
            let scenario = Scenario::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
            for (index, step) in scenario.steps.iter().enumerate() {
                let at = format!("{} step {index}", scenario.name);
                assert_eq!(step.service, Service::Core, "{at}");
                let command: ControlCommand = serde_json::from_value(step.command.clone()).unwrap();
                let result = control(&*session.services.camera, &command).expect("a camera verb");
                assert_eq!(result.is_err(), step.expect_ack_error, "{at}");
                let records = match &result {
                    Ok(outcome) => {
                        let ack =
                            json!({"status": "ok", "ack": outcome.ack, "events": outcome.events()});
                        if let Some(expected) = &step.ack {
                            assert!(is_subset(expected, &ack), "{at}: ack {ack}");
                        }
                        outcome.records.clone()
                    }
                    Err(_) => Vec::new(),
                };
                let expect: Vec<Value> = step
                    .expect
                    .iter()
                    .filter(|e| e["event"] != "camera-error")
                    .cloned()
                    .collect();
                if let Err(missing) = match_in_order(&records, &expect) {
                    panic!("{at}: missing {} in {records:?}", expect[missing]);
                }
            }
        }
    }

    #[test]
    fn control_verbs_drive_the_capability_and_unsupported_targets_say_so() {
        let session = session();
        let camera = &*session.services.camera;
        let list = control(camera, &ControlCommand::CameraList)
            .unwrap()
            .unwrap();
        assert_eq!(list.records[0]["count"], 2);
        let still = control(
            camera,
            &ControlCommand::CameraCapture {
                facing: CameraFacing::Front,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(still.events(), ["camera-still"]);
        assert_eq!(still.records[0]["width"], 960);
        let preview = control(
            camera,
            &ControlCommand::CameraPreview {
                facing: CameraFacing::Back,
                frames: 3,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(preview.records[0]["monotonic"], true);
        assert_eq!(preview.records[0]["distinct"], 3);
        assert!(
            control(
                camera,
                &ControlCommand::CameraVideo {
                    facing: CameraFacing::Back
                }
            )
            .unwrap()
            .is_err()
        );
        assert!(
            control(
                camera,
                &ControlCommand::CameraCapture {
                    facing: CameraFacing::External
                }
            )
            .unwrap()
            .is_err()
        );
        assert!(control(camera, &ControlCommand::State).is_none());
        let err = control(&UnsupportedCamera, &ControlCommand::CameraList)
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(err, "camera is unsupported on this target");
    }
}
