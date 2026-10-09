//! Guest-side Contacts client: follows `ferrix-contactsd` through the
//! protocol reconnector, re-fetches the list on change events, sends
//! writes over a request connection, exports vCard to a private path, and
//! reports Back to `ferrix-appd`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ferrix_app_protocol::{Client as AppdClient, ClientRole, Report};
use ferrix_contacts_api::ContactsError;
use ferrix_contacts_protocol::{Client, ClientKind, Reconnector};
use ferrix_ui::client::{ClientConfig, ClientHandler, Wake};
use ferrix_ui::draw::{Canvas, Type};
use ferrix_ui::evidence::{emit, json_escape, json_opt_str};
use ferrix_ui::keymap::{self, Modifiers};

use crate::interaction::{Input, key_press};
use crate::model::{Action, Link, Model};
use crate::{APP_ID, EVIDENCE, screen};

const TIMEOUT: Duration = Duration::from_secs(2);
/// Export target when `FERRIX_CONTACTS_EXPORT_DIR` is unset: the sandbox's
/// private `/tmp`.
const DEFAULT_EXPORT: &str = "/tmp/ferrix-contacts-export.vcf";

struct Contacts {
    ty: Type,
    minute: u32,
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

pub fn run(initial_size: (u32, u32), minute: u32) -> Result<(), String> {
    let config = ClientConfig {
        app_id: APP_ID.to_owned(),
        title: "Contacts".to_owned(),
        initial_size,
        evidence: EVIDENCE.to_owned(),
        first_frame_event: "contacts-frame".to_owned(),
        first_frame_extra: Vec::new(),
    };
    ferrix_ui::client::run(config, move |wake: Wake| {
        let socket = ferrix_contacts_protocol::socket_path();
        let link = Reconnector::start(
            socket.clone(),
            ClientKind::Application,
            Duration::from_secs(1),
            Arc::clone(&wake),
        );
        Ok(Contacts {
            ty: Type::load()?,
            minute,
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

fn export_path() -> PathBuf {
    match std::env::var_os("FERRIX_CONTACTS_EXPORT_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("contacts.vcf"),
        None => PathBuf::from(DEFAULT_EXPORT),
    }
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
                let outcome = text.and_then(|text| {
                    let path = export_path();
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(&path, text.as_bytes())
                        .map(|()| {
                            emit(format!(
                                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"contacts-export\",\"app\":\"{APP_ID}\",\"bytes\":{}}}",
                                text.len()
                            ));
                            self.model.export_status = Some(format!("exported \u{b7} {} bytes", text.len()));
                        })
                        .map_err(|e| Err(format!("write export: {e}")))
                });
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
        screen::compose(&self.ty, size, self.minute, &self.model, &self.input)
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
        changed
    }

    fn page_name(&self) -> String {
        self.model.page.name().to_owned()
    }
}
