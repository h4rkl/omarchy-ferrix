//! Guest-side Contacts client: follows `ferrix-contactsd` through the
//! protocol reconnector, re-fetches the list on change events, sends
//! writes over a request connection, exports vCard to the user's documents
//! directory, and reports Back to `ferrix-appd`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ferrix_app_api::{AppError, AppIntent, IntentKind};
use ferrix_app_protocol::{Client as AppdClient, ClientRole, Report};
use ferrix_contacts_api::ContactsError;
use ferrix_contacts_protocol::{Client, ClientKind, Reconnector};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::clock::MinuteClock;
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape, json_opt_str};
use ferrix_ui::keymap::{self, Modifiers};

use crate::interaction::{Input, key_press};
use crate::model::{Action, Link, Model};
use crate::{APP_ID, EVIDENCE, model, screen};

const TIMEOUT: Duration = Duration::from_secs(2);

struct Contacts {
    ty: Type,
    clock: MinuteClock,
    model: Model,
    input: Input,
    socket: PathBuf,
    link: Reconnector,
    client: Option<Client>,
    appd_socket: Option<PathBuf>,
    appd: Option<AppdClient>,
    appd_unavailable_reported: bool,
    last_ui_state: (String, String),
    last_link: Link,
}

pub fn run(initial_size: (u32, u32)) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: "Contacts".to_owned(),
        initial_size,
        evidence: EVIDENCE.to_string(),
        first_frame_event: "contacts-frame".to_owned(),
        first_frame_extra: Vec::new(),
    };
    ferrix_ui::client::run(config, move |wake: Wake| {
        // Redraw the status-bar clock at each local minute boundary.
        let tick = Arc::clone(&wake);
        ferrix_ui::clock::spawn_minute_ticker(move || tick());
        let socket = ferrix_contacts_protocol::socket_path();
        let link = Reconnector::start(
            socket.clone(),
            ClientKind::Application,
            Duration::from_secs(1),
            Arc::clone(&wake),
        );
        Ok(Contacts {
            ty: Type::load()?,
            clock: MinuteClock::now(),
            model: Model::new(),
            input: Input::default(),
            socket,
            link,
            client: None,
            appd_socket: std::env::var_os(ferrix_app_api::APPD_SOCKET_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            appd: None,
            appd_unavailable_reported: false,
            last_ui_state: (String::new(), String::new()),
            last_link: Link::Connecting,
        })
    })
}

/// Writes `text` to the durable export location (see
/// [`model::export_path`]), creating the directory, and returns the path.
fn write_export(text: &str) -> Result<PathBuf, String> {
    let path = model::export_path(|key| std::env::var(key).ok())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("export \u{b7} create {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, text.as_bytes())
        .map_err(|e| format!("export \u{b7} write {}: {e}", path.display()))?;
    Ok(path)
}

impl Contacts {
    fn client(&mut self) -> Result<&mut Client, String> {
        if self.client.is_none() {
            self.client = Some(Client::connect(
                &self.socket,
                ClientKind::Application,
                TIMEOUT,
            )?);
        }
        self.client
            .as_mut()
            .ok_or_else(|| "no contacts client".to_owned())
    }

    fn refresh(&mut self) {
        let listed = match self.client() {
            Ok(client) => client.list(),
            Err(e) => Err(Err(e)),
        };
        match listed {
            Ok(contacts) => self.model.set_contacts(contacts),
            Err(_) => self.client = None,
        }
    }

    fn send(&mut self, action: Action) {
        let (name, outcome): (&str, Result<(), Result<ContactsError, String>>) = match action {
            Action::Back => {
                self.yield_foreground();
                return;
            }
            Action::Open(intent) => {
                self.open_intent(&intent);
                return;
            }
            Action::Create(input) => (
                "create",
                self.client()
                    .map_err(Err)
                    .and_then(|c| c.create(input).map(|_| ())),
            ),
            Action::Update(id, input) => (
                "update",
                self.client()
                    .map_err(Err)
                    .and_then(|c| c.update(&id, input).map(|_| ())),
            ),
            Action::Delete(id) => (
                "delete",
                self.client().map_err(Err).and_then(|c| c.delete(&id)),
            ),
            Action::Export => {
                let text = self.client().map_err(Err).and_then(|c| c.export_vcard());
                let outcome = match text {
                    Ok(text) => match write_export(&text) {
                        Ok(path) => {
                            // The record keeps counts only; the path can
                            // carry the user's name.
                            emit(format!(
                                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-export\",\"app\":\"{APP_ID}\",\"bytes\":{}}}",
                                text.len()
                            ));
                            let home = std::env::var("HOME").ok();
                            self.model.export_status = Some(format!(
                                "exported to {}",
                                model::display_path(&path, home.as_deref())
                            ));
                            Ok(())
                        }
                        Err(reason) => {
                            // A local file problem, not a daemon failure.
                            self.model.export_status = None;
                            self.model.last_error = Some(reason);
                            emit(format!(
                                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-action\",\"app\":\"{APP_ID}\",\"action\":\"export\",\"status\":\"error\",\"error\":\"write\"}}"
                            ));
                            return;
                        }
                    },
                    Err(error) => Err(error),
                };
                ("export", outcome)
            }
        };
        let (status, error) = match outcome {
            Ok(()) => ("ok", None),
            Err(Ok(error)) => {
                self.model.last_error = Some(error.to_string());
                ("error", Some(error.to_string()))
            }
            Err(Err(transport)) => {
                self.client = None;
                self.model.last_error = Some("contacts \u{b7} unavailable".to_owned());
                ("unavailable", Some(transport))
            }
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-action\",\"app\":\"{APP_ID}\",\"action\":\"{name}\",\"status\":\"{status}\",\"error\":{}}}",
            json_opt_str(error.as_deref())
        ));
        if status == "ok" && name != "export" {
            self.refresh();
        }
    }

    fn after_input(&mut self) {
        self.model.note_form_edit();
        for action in std::mem::take(&mut self.model.pending) {
            self.send(action);
        }
        self.report_ui_state();
    }

    fn report_ui_state(&mut self) {
        let current = (self.model.page.name().to_owned(), self.model.state_name());
        if current != self.last_ui_state {
            self.last_ui_state = current.clone();
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ui-state\",\"app\":\"{APP_ID}\",\"page\":\"{}\",\"state\":\"{}\"}}",
                current.0,
                json_escape(&current.1)
            ));
        }
        if self.model.link != self.last_link {
            self.last_link = self.model.link;
            emit(format!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-link\",\"app\":\"{APP_ID}\",\"status\":\"{}\",\"contacts\":{}}}",
                self.model.link.name(),
                self.model.contacts.len()
            ));
        }
    }

    fn appd(&mut self) -> Option<&mut AppdClient> {
        if self.appd.is_none() {
            let Some(socket) = self.appd_socket.clone() else {
                self.report_appd_unavailable("FERRIX_APPD_SOCKET is not set");
                return None;
            };
            match AppdClient::connect(&socket, ClientRole::Application, TIMEOUT) {
                Ok(client) => self.appd = Some(client),
                Err(error) => {
                    self.report_appd_unavailable(&error);
                    return None;
                }
            }
        }
        self.appd.as_mut()
    }

    fn report_appd_unavailable(&mut self, reason: &str) {
        if self.appd_unavailable_reported {
            return;
        }
        self.appd_unavailable_reported = true;
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"ferrix-appd-unavailable\",\"app\":\"{APP_ID}\",\"reason\":\"{}\"}}",
            json_escape(reason)
        ));
    }

    /// Asks `ferrix-appd` to open the dial/compose handler. The record
    /// names the action only, never the number.
    fn open_intent(&mut self, intent: &AppIntent) {
        let action = match intent.kind() {
            IntentKind::Dial => "call",
            IntentKind::Compose => "message",
            IntentKind::ShowCall => return,
        };
        let result = self.appd().map(|client| client.open(intent));
        let (status, error) = match result {
            Some(Ok(_)) => ("ok", None),
            Some(Err(Ok(error))) => {
                let text = match &error {
                    AppError::NoHandler { .. } => "no app can open this".to_owned(),
                    other => other.to_string(),
                };
                self.model.last_error = Some(text);
                ("error", Some(error.to_string()))
            }
            Some(Err(Err(transport))) => {
                self.appd = None;
                self.model.last_error = Some("apps \u{b7} unavailable".to_owned());
                ("unavailable", Some(transport))
            }
            None => {
                self.model.last_error = Some("apps \u{b7} unavailable".to_owned());
                ("unavailable", None)
            }
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-action\",\"app\":\"{APP_ID}\",\"action\":\"{action}\",\"status\":\"{status}\",\"error\":{}}}",
            json_opt_str(error.as_deref())
        ));
    }

    fn yield_foreground(&mut self) {
        let result = self
            .appd()
            .map(|client| client.report(&Report::YieldForeground));
        let status = match result {
            Some(Ok(())) => "yielded",
            Some(Err(_)) => {
                self.appd = None;
                "failed"
            }
            None => "ferrix-appd-unavailable",
        };
        emit(format!(
            "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-back\",\"status\":\"{status}\"}}"
        ));
    }
}

impl ClientHandler for Contacts {
    fn draw(&mut self, size: (u32, u32)) -> Canvas {
        screen::compose(
            &self.ty,
            size,
            self.clock.minute(),
            &self.model,
            &self.input,
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
        if redraw {
            self.after_input();
        }
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
        if redraw {
            self.after_input();
        }
        redraw
    }

    fn touch_cancel(&mut self) -> bool {
        self.input.cancel()
    }

    fn key(&mut self, code: u32, mods: Modifiers) -> bool {
        let Some(key) = keymap::text_key(code, mods) else {
            return false;
        };
        key_press(&mut self.model, key);
        self.after_input();
        true
    }

    fn wake(&mut self) -> bool {
        let mut changed = false;
        let mut refresh = false;
        for link in self.link.drain() {
            refresh |= self.model.apply(link);
            changed = true;
        }
        if refresh {
            self.refresh();
        }
        if changed {
            self.report_ui_state();
        }
        self.clock.refresh() || changed
    }

    fn page_name(&self) -> String {
        self.model.page.name().to_owned()
    }
}
