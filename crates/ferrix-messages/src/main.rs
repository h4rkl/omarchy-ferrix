//! Omarchy Ferrix Messages: an independent Wayland client for SMS.
//!
//! Launched by `ferrix-appd` (xdg app_id `org.ferrix.messages`); follows
//! `ferrix-telephonyd` for messages and `ferrix-contactsd` for names.
//!
//! Usage:
//!   ferrix-messages                                  # guest/nested: Wayland
//!   ferrix-messages --preview out.ppm [--page NAME] [--size WxH]
//!
//! Pages: `conversations` (default), `thread`, `compose`. Records:
//! `app-context`, `messages-frame`, `ui-state {page, state}`,
//! `messages-action {action, status}`, `telephony-link {status}`; ids and
//! state names only, never bodies or numbers.

mod interaction;
mod model;
mod screen;

#[cfg(target_os = "linux")]
mod app;

use std::process::ExitCode;

use ferrix_telephony_protocol::{StateView, Update};
use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{SessionEvidence, emit, json_escape, json_opt_str};
use ferrix_ui::theme;

use crate::model::{Model, Page};

const EVIDENCE: SessionEvidence = SessionEvidence;
const PREVIEW_SIZE: (u32, u32) = (1080, 2340);
pub const APP_ID: &str = "org.ferrix.messages";

fn app_context_record(evidence: &str) -> String {
    let app_id = std::env::var(ferrix_app_api::APP_ID_ENV)
        .ok()
        .filter(|v| !v.is_empty());
    let instance = std::env::var(ferrix_app_api::APP_INSTANCE_ENV)
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    format!(
        "{{\"evidence\":\"{evidence}\",\"event\":\"app-context\",\"app\":\"{APP_ID}\",\"app_id\":{},\"instance\":{}}}",
        json_opt_str(app_id.as_deref()),
        instance.map_or_else(|| "null".to_owned(), |n| n.to_string())
    )
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

fn preview_model(page: Page) -> Model {
    let mut model = Model::new();
    model.apply(Update::Snapshot(StateView::Full(model::preview_state())));
    match page {
        Page::Conversations => {}
        Page::Thread => {
            model.open_thread("+15550100042");
            model.pending.clear();
            model.draft = "Reply from the previe".to_owned();
        }
        Page::Compose => {
            model.open_compose();
            model.compose_to = "555 010 0077".to_owned();
        }
    }
    model
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"messages-error\",\"error\":\"{}\"}}",
                json_escape(&err)
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut preview = None;
    let mut page = "conversations".to_owned();
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
        emit(app_context_record("mock"));
        let page = Page::parse(&page).ok_or_else(|| {
            format!("unknown --page {page:?}; use conversations, thread, compose")
        })?;
        let ty = Type::load()?;
        let model = preview_model(page);
        let canvas = screen::compose(
            &ty,
            size,
            ferrix_ui::clock::local_minute_of_day(),
            &model,
            &interaction::Input::default(),
        );
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
        emit(format!(
            "{{\"evidence\":\"mock\",\"event\":\"messages-preview\",\"page\":\"{}\",\"state\":\"{}\",\"path\":\"{}\",\"width\":{},\"height\":{}}}",
            page.name(),
            json_escape(&model.state_name()),
            json_escape(&path),
            size.0,
            size.1
        ));
        return Ok(());
    }
    emit(app_context_record(&EVIDENCE));
    theme::init(&EVIDENCE, None);
    #[cfg(target_os = "linux")]
    {
        app::run(size)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("the Wayland session is Linux-only on this host; use --preview <out.ppm>".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_pages_render_at_the_panel_size() {
        let ty = Type::load().unwrap();
        for name in ["conversations", "thread", "compose"] {
            let page = Page::parse(name).unwrap();
            let model = preview_model(page);
            assert_eq!(model.page, page);
            let c = screen::compose(
                &ty,
                PREVIEW_SIZE,
                600,
                &model,
                &interaction::Input::default(),
            );
            assert_eq!((c.width(), c.height()), (1080, 2340));
            assert!(c.ppm_bytes().starts_with(b"P6\n1080 2340\n255\n"));
        }
        assert!(parse_size("1x").is_err());
        assert!(app_context_record("mock").starts_with(
            r#"{"evidence":"mock","event":"app-context","app":"org.ferrix.messages""#
        ));
    }
}
