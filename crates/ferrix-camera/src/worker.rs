//! The camera worker thread: owns the blocking camera-socket clients so the
//! UI thread never waits on the service. Commands in, [`Event`]s out; every
//! event is followed by a wake.
//!
//! Each preview runs on its own connection with a forwarding thread. Opening
//! another preview (a camera switch, a reopen) moves the generation, and the
//! previous forwarder then drops its stream: the connection closes and the
//! service releases that camera within one frame.

// The Wayland half (app.rs) is Linux-only; host builds use the rest in tests
// and previews.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use ferrix_hardware_api::{CameraFrame, CameraId, CapabilityStatus, HardwareError, PreviewRequest};
use ferrix_protocol::camera::{CameraClient, CameraClientError, PreviewEvent};

use crate::model::Event;
use crate::store;

/// Preview rate the app asks for; the adapter may deliver less.
pub const PREVIEW_FPS: u8 = 8;
const LIST_TIMEOUT: Duration = Duration::from_secs(3);
/// A full-size still crosses the socket as base64; allow for TCG guests.
const STILL_TIMEOUT: Duration = Duration::from_secs(20);
/// A camera the previous stream still holds frees up within one frame.
const BUSY_RETRIES: u32 = 10;

#[derive(Debug)]
pub enum Command {
    /// Hello and list again (start, or the user pressed Retry).
    Connect,
    OpenPreview(CameraId),
    Capture(CameraId),
}

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Starts the worker; it connects and lists right away.
pub fn spawn(socket: PathBuf, wake: Wake) -> (Sender<Command>, Receiver<Event>) {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let _ = command_tx.send(Command::Connect);
    let failed = event_tx.clone();
    let spawned = thread::Builder::new()
        .name("camera-worker".to_owned())
        .spawn(move || run(&socket, &command_rx, &event_tx, &wake));
    if let Err(error) = spawned {
        let _ = failed.send(Event::Unavailable(format!(
            "cannot start camera worker: {error}"
        )));
    }
    (command_tx, event_rx)
}

fn send(events: &Sender<Event>, wake: &Wake, event: Event) -> bool {
    let ok = events.send(event).is_ok();
    wake();
    ok
}

fn run(socket: &Path, commands: &Receiver<Command>, events: &Sender<Event>, wake: &Wake) {
    let generation = Arc::new(AtomicU64::new(0));
    while let Ok(command) = commands.recv() {
        let alive = match command {
            Command::Connect => {
                // Any running preview belongs to the old session.
                generation.fetch_add(1, Ordering::SeqCst);
                connect(socket, events, wake)
            }
            Command::OpenPreview(camera) => {
                let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
                open_preview(socket, camera, &generation, mine, events, wake)
            }
            Command::Capture(camera) => {
                let result = capture(socket, &camera);
                let save = result.as_ref().ok().map(|frame| {
                    store::home_from_env().and_then(|home| {
                        let now = jiff::Zoned::now();
                        store::save_still(&home, frame, &store::still_stem(&now))
                    })
                });
                // The pixels stay here; the UI only needs the outcome.
                let still = result.map(|frame| CameraFrame {
                    data: Vec::new(),
                    ..frame
                });
                let alive = send(events, wake, Event::Still(still));
                match save {
                    Some(save) => alive && send(events, wake, Event::Saved(save)),
                    None => alive,
                }
            }
        };
        if !alive {
            return;
        }
    }
}

fn connect(socket: &Path, events: &Sender<Event>, wake: &Wake) -> bool {
    match CameraClient::connect(socket, LIST_TIMEOUT) {
        Ok(mut client) => {
            let status = client.status();
            if !send(events, wake, Event::Hello(status)) {
                return false;
            }
            if status == CapabilityStatus::Unsupported {
                return true;
            }
            let cameras = client.cameras();
            send(events, wake, Event::Cameras(cameras))
        }
        Err(CameraClientError::Unavailable(reason)) => {
            send(events, wake, Event::Unavailable(reason))
        }
        Err(error) => send(events, wake, Event::Cameras(Err(error))),
    }
}

/// A still is large: it uses its own connection with the long timeout.
fn capture(socket: &Path, camera: &CameraId) -> Result<CameraFrame, CameraClientError> {
    CameraClient::connect(socket, STILL_TIMEOUT)?.capture(camera)
}

fn open_preview(
    socket: &Path,
    camera: CameraId,
    current: &Arc<AtomicU64>,
    mine: u64,
    events: &Sender<Event>,
    wake: &Wake,
) -> bool {
    let request = PreviewRequest {
        max_fps: PREVIEW_FPS,
    };
    let mut attempt = 0;
    let stream = loop {
        let result = CameraClient::connect(socket, LIST_TIMEOUT)
            .and_then(|client| client.preview(&camera, request));
        match result {
            Ok(stream) => break stream,
            Err(CameraClientError::Hardware(HardwareError::Failed { .. }))
                if attempt < BUSY_RETRIES =>
            {
                attempt += 1;
                thread::sleep(Duration::from_millis(150));
            }
            Err(error) => return send(events, wake, Event::PreviewFailed(camera, error)),
        }
    };
    let forward = events.clone();
    let forward_wake = Arc::clone(wake);
    let current = Arc::clone(current);
    let rx = stream.spawn(Arc::new(|| {}));
    let spawned = thread::Builder::new()
        .name(format!("camera-frames-{mine}"))
        .spawn(move || {
            for event in rx {
                // Superseded: returning drops `rx`, the reader's next send
                // fails, and the connection (and camera) is released.
                if current.load(Ordering::SeqCst) != mine {
                    return;
                }
                let event = match event {
                    PreviewEvent::Frame(frame) => Event::Frame(frame),
                    PreviewEvent::Ended(error) => Event::PreviewEnded(error),
                };
                if forward.send(event).is_err() {
                    return;
                }
                forward_wake();
            }
        });
    spawned.is_ok()
}
