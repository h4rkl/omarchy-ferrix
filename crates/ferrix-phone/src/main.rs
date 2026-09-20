//! Omarchy Ferrix Phone: an independent Wayland client for call flows.
//!
//! Launched by `ferrix-appd` (xdg app_id `org.ferrix.phone`), it subscribes
//! to `ferrix-telephonyd` for the snapshot and deltas, to `ferrix-contactsd`
//! for party names, and asks `ferrix-appd` to yield the foreground on Back.
//! Emergency numbers are never dialed.
//!
//! Usage:
//!   ferrix-phone                                   # guest/nested: Wayland
//!   ferrix-phone --preview out.ppm [--page NAME] [--size WxH]
//!                                                  # any host: render a page
//!
//! Pages: `dialer` (default), `recent`, `incoming`, `in-call`, `ended`.
//! Records: `app-context`, `phone-frame`, `ui-state {page, state}`,
//! `phone-action {action, status}`, `telephony-link {status}`. Records
//! carry names and ids only, never numbers or contact names.

mod interaction;
mod model;
mod screen;

#[cfg(target_os = "linux")]
mod app;

use std::process::ExitCode;

use ferrix_telephony_api::{CallEndReason, CallState, Direction};
use ferrix_telephony_protocol::{StateView, Update};
use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{emit, json_escape, json_opt_str};
use ferrix_ui::theme;

use crate::model::{Model, Page};

const EVIDENCE: &str = "QEMU";
const PREVIEW_SIZE: (u32, u32) = (1080, 2340);
pub const APP_ID: &str = "org.ferrix.phone";

struct AppContext {
    app_id: Option<String>,
    instance: Option<u32>,
}

impl AppContext {
    fn from_env() -> Self {
        Self {
            app_id: std::env::var(ferrix_app_api::APP_ID_ENV)
                .ok()
                .filter(|v| !v.is_empty()),
            instance: std::env::var(ferrix_app_api::APP_INSTANCE_ENV)
                .ok()
                .and_then(|v| v.parse().ok()),
        }
    }

    fn record(&self, evidence: &str) -> String {
        format!(
            "{{\"evidence\":\"{evidence}\",\"event\":\"app-context\",\"app\":\"{APP_ID}\",\"app_id\":{},\"instance\":{}}}",
            json_opt_str(self.app_id.as_deref()),
            self.instance
                .map_or_else(|| "null".to_owned(), |n| n.to_string())
        )
    }
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

/// A model showing `page` from the canned preview snapshot.
fn preview_model(page: Page) -> Model {
    let call = match page {
        Page::Dialer | Page::Recent => None,
        Page::Incoming => Some((Direction::Incoming, CallState::Ringing, None)),
        Page::InCall => Some((Direction::Outgoing, CallState::Active, None)),
        Page::Ended => Some((
            Direction::Outgoing,
            CallState::Ended,
            Some(CallEndReason::RemoteHangup),
        )),
    };
    let mut model = Model::new();
    model.apply(Update::Snapshot(StateView::Full(model::preview_state(
        call,
    ))));
    match page {
        Page::Dialer => {
            for ch in "5550100042".chars() {
                model.key(ch);
            }
        }
        Page::Recent => model.page = Page::Recent,
        Page::Ended => {
            model.focused_call = Some(ferrix_telephony_api::CallId("call-live".to_owned()));
            model.page = Page::Ended;
        }
        Page::InCall => model.dtmf_open = true,
        Page::Incoming => {}
    }
    model
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"phone-error\",\"error\":\"{}\"}}",
                json_escape(&err)
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut preview = None;
    let mut page = "dialer".to_owned();
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
        emit(AppContext::from_env().record("mock"));
        let page = Page::parse(&page).ok_or_else(|| {
            format!("unknown --page {page:?}; use dialer, recent, incoming, in-call, ended")
        })?;
        let ty = Type::load()?;
        let model = preview_model(page);
        let canvas = screen::compose(
            &ty,
            size,
            utc_minute_of_day(),
            &model,
            &interaction::Input::default(),
        );
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
        emit(format!(
            "{{\"evidence\":\"mock\",\"event\":\"phone-preview\",\"page\":\"{}\",\"state\":\"{}\",\"path\":\"{}\",\"width\":{},\"height\":{}}}",
            page.name(),
            json_escape(&model.state_name()),
            json_escape(&path),
            size.0,
            size.1
        ));
        return Ok(());
    }
    emit(AppContext::from_env().record(EVIDENCE));
    theme::init(EVIDENCE, None);
    #[cfg(target_os = "linux")]
    {
        app::run(size, utc_minute_of_day())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("the Wayland session is Linux-only on this host; use --preview <out.ppm>".to_owned())
    }
}

fn utc_minute_of_day() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ((secs % 86_400) / 60) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_pages_render_at_the_panel_size() {
        let ty = Type::load().unwrap();
        for name in ["dialer", "recent", "incoming", "in-call", "ended"] {
            let page = Page::parse(name).unwrap();
            let model = preview_model(page);
            assert_eq!(model.page, page, "{name}");
            let c = screen::compose(
                &ty,
                PREVIEW_SIZE,
                600,
                &model,
                &interaction::Input::default(),
            );
            assert_eq!((c.width(), c.height()), (1080, 2340));
            let ppm = c.ppm_bytes();
            assert!(ppm.starts_with(b"P6\n1080 2340\n255\n"));
            assert_eq!(ppm.len(), "P6\n1080 2340\n255\n".len() + 1080 * 2340 * 3);
        }
        assert_eq!(parse_size("830x1800"), Ok((830, 1800)));
        assert!(parse_size("0x1").is_err());
        let context = AppContext {
            app_id: None,
            instance: None,
        };
        assert_eq!(
            context.record("QEMU"),
            r#"{"evidence":"QEMU","event":"app-context","app":"org.ferrix.phone","app_id":null,"instance":null}"#
        );
    }
}
