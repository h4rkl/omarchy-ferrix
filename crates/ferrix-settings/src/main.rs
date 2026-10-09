//! Omarchy Ferrix Settings: an independent Wayland client for network and
//! radio settings over `ferrix-networkd` and `ferrix-telephonyd`.
//!
//! Usage:
//!   ferrix-settings [--target ID]                    # guest/nested: Wayland
//!   ferrix-settings --preview out.ppm [--page NAME] [--size WxH]
//!
//! Pages: `network` (default), `radios`, `about`. `--target` (or
//! `FERRIX_TARGET`) names the target on the About page. Records:
//! `app-context`, `settings-frame`, `ui-state {page, state}`,
//! `settings-action {action, status}`, `network-link`, `telephony-link`;
//! never SSIDs, passphrases, APNs, or addresses. The
//! `FERRIX_TELEPHONY_APN` launch-environment contract is an optional
//! non-secret runtime setting used when the selected modem plugin requires an
//! explicit carrier APN.

mod interaction;
mod model;
mod screen;

#[cfg(target_os = "linux")]
mod app;

use std::process::ExitCode;

use ferrix_network_protocol::Feed;
use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{SessionEvidence, emit, json_escape, json_opt_str};
use ferrix_ui::theme;

use crate::model::{Model, Page};

const EVIDENCE: SessionEvidence = SessionEvidence;
const PREVIEW_SIZE: (u32, u32) = (1080, 2340);
pub const APP_ID: &str = "org.ferrix.settings";

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

fn target_from_env() -> String {
    std::env::var("FERRIX_TARGET")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn preview_model(page: Page, target: &str) -> Model {
    let mut model = Model::new();
    model.target = target.to_owned();
    model.apply_network(Feed::Snapshot {
        state: model::preview_network(true),
        evidence: "mock".into(),
    });
    let mut tele = ferrix_telephony_api::TelephonyState::new();
    tele.modem = ferrix_telephony_api::ModemState::Ready;
    tele.radio = ferrix_telephony_api::RadioPower::On;
    model.apply_telephony(ferrix_telephony_protocol::Update::Snapshot(
        ferrix_telephony_protocol::StateView::Redacted(tele.redacted()),
    ));
    model.tele_evidence = Some("mock".to_owned());
    model.show(page);
    model
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"settings-error\",\"error\":\"{}\"}}",
                json_escape(&err)
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut preview = None;
    let mut page = "network".to_owned();
    let mut size = PREVIEW_SIZE;
    let mut target = target_from_env();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => preview = Some(it.next().ok_or("--preview requires an output path")?),
            "--page" => page = it.next().ok_or("--page requires a page name")?,
            "--size" => size = parse_size(&it.next().ok_or("--size requires WxH")?)?,
            "--target" => target = it.next().ok_or("--target requires an id")?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if let Some(path) = preview {
        theme::init("mock", None);
        emit(app_context_record("mock"));
        let page = Page::parse(&page)
            .ok_or_else(|| format!("unknown --page {page:?}; use network, radios, about"))?;
        let ty = Type::load()?;
        let model = preview_model(page, &target);
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
            "{{\"evidence\":\"mock\",\"event\":\"settings-preview\",\"page\":\"{}\",\"state\":\"{}\",\"path\":\"{}\",\"width\":{},\"height\":{}}}",
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
        app::run(size, target)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = target;
        Err("the Wayland session is Linux-only on this host; use --preview <out.ppm>".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_pages_render_at_the_panel_size() {
        let ty = Type::load().unwrap();
        for name in ["network", "radios", "about"] {
            let page = Page::parse(name).unwrap();
            let model = preview_model(page, "qemu-aarch64");
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
        assert!(parse_size("10x0").is_err());
    }
}
