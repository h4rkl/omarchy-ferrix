//! Guest-side Camera client: runs the worker against the camera socket,
//! drains its events on wake (only the newest preview frame is kept),
//! reopens a preview that ended (a frozen or restarted service), and talks
//! to `ferrix-appd` for the Files hand-off and Back.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use ferrix_app_api::AppIntent;
use ferrix_app_protocol::{Client as AppdClient, ClientRole, Report};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::clock::MinuteClock;
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape};
use ferrix_ui::keymap::Modifiers;

use crate::interaction::Input;
use crate::model::{Action, Event, Model, Status};
use crate::worker::{self, Command};
use crate::{APP_ID, EVIDENCE, screen};

const TIMEOUT: Duration = Duration::from_secs(2);
/// How long after a preview ended (or the service vanished) to try again.
const REOPEN_AFTER: Duration = Duration::from_secs(2);

struct Camera {
    ty: Type,
    clock: MinuteClock,
    model: Model,
    input: Input,
    commands: Sender<Command>,
    events: Receiver<Event>,
    appd_socket: Option<PathBuf>,
    appd: Option<AppdClient>,
    reopen_at: Option<Instant>,
    last_ui_state: (String, String),
    /// Facing whose first displayed frame was already recorded.
    preview_reported: Option<String>,
}

pub fn run(initial_size: (u32, u32)) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: "Camera".to_owned(),
        initial_size,
        evidence: EVIDENCE.to_string(),
        first_frame_event: "camera-app-frame".to_owned(),
        first_frame_extra: Vec::new(),
    };
    ferrix_ui::client::run(config, move |wake: Wake| {
        let tick = Arc::clone(&wake);
        ferrix_ui::clock::spawn_minute_ticker(move || tick());
        let (commands, events) = worker::spawn(crate::camera_socket(), Arc::clone(&wake));
        let mut camera = Camera {
            ty: Type::load()?,
            clock: MinuteClock::now(),
            model: Model::new(),
            input: Input::default(),
            commands,
            events,
            appd_socket: std::env::var_os(ferrix_app_api::APPD_SOCKET_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            appd: None,
            reopen_at: None,
            last_ui_state: (String::new(), String::new()),
            preview_reported: None,
        };
        camera.report_ui_state();
        Ok(camera)
    })
}

impl Camera {
    fn send(&mut self, command: Command) {
        if self.commands.send(command).is_err() {
            self.model
                .apply(Event::Unavailable("camera worker stopped".to_owned()));
        }
    }

    /// Hands queued model actions to the worker or to `ferrix-appd`.
    fn after_change(&mut self) {
        for action in std::mem::take(&mut self.model.pending) {
            match action {
                Action::OpenPreview(id) => {
                    self.preview_reported = None;
                    self.send(Command::OpenPreview(id));
                }
                Action::Capture(id) => self.send(Command::Capture(id)),
                Action::Retry => self.send(Command::Connect),
                Action::ShowPictures => self.show_pictures(),
                Action::Back => self.yield_foreground(),
            }
        }
        self.report_ui_state();
    }

    fn report_ui_state(&mut self) {
        let current = (self.model.status.name().to_owned(), self.model.state_name());
        if current != self.last_ui_state {
            self.last_ui_state = current.clone();
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-state\",\"app\":\"{APP_ID}\",\"page\":\"{}\",\"state\":\"{}\"}}",
                current.0,
                json_escape(&current.1)
            ));
        }
    }

    fn appd(&mut self) -> Option<&mut AppdClient> {
        if self.appd.is_none() {
            let socket = self.appd_socket.clone()?;
            self.appd = AppdClient::connect(&socket, ClientRole::Application, TIMEOUT).ok();
        }
        self.appd.as_mut()
    }

    fn show_pictures(&mut self) {
        let result = self.appd().map(|c| c.open(&AppIntent::ShowPictures));
        let status = match result {
            Some(Ok(_)) => "opened",
            Some(Err(Ok(_))) => "no-handler",
            Some(Err(Err(_))) => {
                self.appd = None;
                "failed"
            }
            None => "ferrix-appd-unavailable",
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"camera-handoff\",\"intent\":\"show-pictures\",\"status\":\"{status}\"}}"
        ));
    }

    fn yield_foreground(&mut self) {
        let result = self.appd().map(|c| c.report(&Report::YieldForeground));
        let status = match result {
            Some(Ok(())) => "yielded",
            Some(Err(_)) => {
                self.appd = None;
                "failed"
            }
            None => "ferrix-appd-unavailable",
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"camera-back\",\"status\":\"{status}\"}}"
        ));
    }

    /// Drains worker events, keeping only the newest frame.
    fn drain(&mut self) -> bool {
        let mut changed = false;
        let mut newest = None;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Frame(frame) => newest = Some(frame),
                Event::Saved(result) => {
                    let (status, size) = match &result {
                        Ok(saved) => ("ok", Some((saved.width, saved.height))),
                        Err(_) => ("error", None),
                    };
                    emit(format!(
                        "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"camera-app-saved\",\"status\":\"{status}\",\"width\":{},\"height\":{}}}",
                        size.map_or("null".to_owned(), |s| s.0.to_string()),
                        size.map_or("null".to_owned(), |s| s.1.to_string()),
                    ));
                    changed |= self.model.apply(Event::Saved(result));
                }
                other => {
                    let ended = matches!(other, Event::PreviewEnded(_) | Event::Unavailable(_));
                    changed |= self.model.apply(other);
                    // Only a lost connection is worth retrying by itself.
                    if ended && matches!(self.model.status, Status::Unavailable(_)) {
                        self.reopen_at = Some(Instant::now() + REOPEN_AFTER);
                    }
                }
            }
        }
        if let Some(frame) = newest
            && self.model.apply(Event::Frame(frame))
        {
            changed = true;
            self.report_first_preview_frame();
        }
        if changed {
            self.after_change();
        }
        changed
    }

    fn report_first_preview_frame(&mut self) {
        let Some(camera) = self.model.active_camera() else {
            return;
        };
        let facing = camera.facing.name().to_owned();
        if self.preview_reported.as_deref() == Some(facing.as_str()) {
            return;
        }
        let rotation = camera.rotation.degrees();
        if let Some(frame) = &self.model.frame {
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"camera-app-preview\",\"facing\":\"{facing}\",\"width\":{},\"height\":{},\"rotation\":{rotation},\"sequence\":{}}}",
                frame.width, frame.height, frame.sequence
            ));
        }
        self.preview_reported = Some(facing);
    }
}

impl ClientHandler for Camera {
    fn draw(&mut self, size: (u32, u32)) -> Canvas {
        screen::compose(
            &self.ty,
            size,
            self.clock.minute(),
            &self.model,
            self.input.pressed,
        )
    }

    fn pointer_down(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.down(0, position, &self.model, size)
    }

    fn pointer_motion(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.motion(0, position, &self.model, size)
    }

    fn pointer_up(&mut self, position: (f64, f64), size: (u32, u32)) -> bool {
        let redraw = self.input.up(0, Some(position), &mut self.model, size);
        self.after_change();
        redraw
    }

    fn pointer_leave(&mut self) -> bool {
        self.input.cancel()
    }

    fn touch_down(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.down(id, position, &self.model, size)
    }

    fn touch_motion(&mut self, id: i32, position: (f64, f64), size: (u32, u32)) -> bool {
        self.input.motion(id, position, &self.model, size)
    }

    fn touch_up(&mut self, id: i32, size: (u32, u32)) -> bool {
        let redraw = self.input.up(id, None, &mut self.model, size);
        self.after_change();
        redraw
    }

    fn touch_cancel(&mut self) -> bool {
        self.input.cancel()
    }

    fn key(&mut self, _code: u32, _mods: Modifiers) -> bool {
        false
    }

    fn wake(&mut self) -> bool {
        let changed = self.drain();
        self.clock.refresh() || changed
    }

    fn deadline(&self) -> Option<Instant> {
        self.reopen_at
    }

    fn timer(&mut self, now: Instant) -> bool {
        if self.reopen_at.is_some_and(|at| now >= at) {
            self.reopen_at = None;
            // A preview that ended reopens; a lost service reconnects.
            match self.model.status {
                Status::Unavailable(_) => self.send(Command::Connect),
                _ => {
                    if let Some(camera) = self.model.active_camera() {
                        let id = camera.id.clone();
                        self.model.status = Status::Live;
                        self.send(Command::OpenPreview(id));
                    }
                }
            }
            self.report_ui_state();
            return true;
        }
        false
    }

    fn page_name(&self) -> String {
        self.model.status.name().to_owned()
    }
}
