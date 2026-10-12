//! Omarchy Ferrix Settings: an independent Wayland client for Wi-Fi,
//! Bluetooth, mobile network, display, and radio settings over
//! `ferrix-networkd`, `ferrix-telephonyd`, and `ferrix-core`.
//!
//! Usage:
//!   ferrix-settings [--target ID]                    # guest/nested: Wayland
//!   ferrix-settings --preview out.ppm [--page NAME] [--size WxH]
//!
//! Pages: `root` (default, the category list), `network`, `bluetooth`,
//! `mobile`, `display`, `keyboard`, `radios`, `about`; previews also accept
//! `bluetooth-absent` and `mobile-no-sim` for the honest absent states, and
//! `bluetooth-pair`, `bluetooth-confirm`, `bluetooth-paired`,
//! `bluetooth-failed`, `bluetooth-device`, `bluetooth-forget` for the
//! pairing flows (canned mock session).
//! `--target` (or `FERRIX_TARGET`) names the target on the About page.
//! Records: `app-context`, `settings-frame`, `ui-state {page, state}`,
//! `settings-action {action, service, status}`, `network-link`,
//! `telephony-link`, `core-link`; never SSIDs, passphrases, APNs, device
//! names, device ids, pairing codes, or addresses. The
//! `FERRIX_TELEPHONY_APN` launch-environment contract is an optional
//! non-secret runtime setting used when the selected modem plugin requires an
//! explicit carrier APN.

mod bluetooth;
mod interaction;
mod keyboard;
mod model;
mod screen;
mod system;

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

/// Preview page names: every [`Page`], the two absent-state variants, and
/// the Bluetooth pairing flows.
const PREVIEW_PAGES: &str = "root, network, bluetooth, bluetooth-absent, bluetooth-pair, \
     bluetooth-confirm, bluetooth-paired, bluetooth-failed, bluetooth-device, bluetooth-forget, \
     mobile, mobile-no-sim, display, sound, sound-unavailable, keyboard, radios, about";

fn preview_model(name: &str, target: &str) -> Option<Model> {
    use ferrix_hardware_api::{PairingFailure, PairingPhase};
    let (page, absent) = match name {
        "bluetooth-absent" => (Page::Bluetooth, true),
        "mobile-no-sim" => (Page::Mobile, true),
        "sound-unavailable" => (Page::Sound, true),
        other if other.starts_with("bluetooth-") => (Page::Bluetooth, false),
        other => (Page::parse(other)?, false),
    };
    let (flow, phase) = match name {
        "bluetooth-pair" => (Some(bluetooth::BtView::Pair), None),
        "bluetooth-confirm" => (
            Some(bluetooth::BtView::Pair),
            Some(PairingPhase::ConfirmPasskey { passkey: 482_913 }),
        ),
        "bluetooth-paired" => (Some(bluetooth::BtView::Pair), Some(PairingPhase::Paired)),
        "bluetooth-pin" => (
            Some(bluetooth::BtView::Pair),
            Some(PairingPhase::RequestPin),
        ),
        "bluetooth-failed" => (
            Some(bluetooth::BtView::Pair),
            Some(PairingPhase::Failed {
                reason: PairingFailure::Rejected,
            }),
        ),
        "bluetooth-device" | "bluetooth-forget" => (
            Some(bluetooth::BtView::Device("mock-bt-0".to_owned())),
            None,
        ),
        "bluetooth" | "bluetooth-absent" => (None, None),
        other if other.starts_with("bluetooth-") => return None,
        _ => (None, None),
    };
    let mut model = Model::new();
    model.target = target.to_owned();
    model.apply_network(Feed::Snapshot {
        state: model::preview_network(true),
        evidence: "mock".into(),
    });
    let mut tele = model::preview_telephony();
    if absent && page == Page::Mobile {
        tele.sim = ferrix_telephony_api::SimState::Absent;
        tele.registration = ferrix_telephony_api::Registration::Unknown;
        tele.operator = None;
        tele.signal = ferrix_telephony_api::Signal::bars(0);
    }
    model.apply_telephony(ferrix_telephony_protocol::Update::Snapshot(
        ferrix_telephony_protocol::StateView::Redacted(tele.redacted()),
    ));
    model.tele_evidence = Some("mock".to_owned());
    let mut hw = system::preview_hardware();
    if absent && page == Page::Bluetooth {
        hw.bluetooth.present = false;
        hw.bluetooth.enabled = false;
    }
    let mut capabilities = system::preview_capabilities();
    if absent && page == Page::Sound {
        // An audio service without a mixer level on an unsupported target.
        hw.audio.output = None;
        capabilities = capabilities.with(
            ferrix_hardware_api::Capability::Audio,
            ferrix_hardware_api::CapabilityStatus::Unsupported,
        );
    }
    model.apply_core(ferrix_protocol::Feed::Snapshot {
        target: target.to_owned(),
        profile: "preview".to_owned(),
        capabilities,
        state: hw,
    });
    model.show(page);
    model.pending.clear();
    if model.bluetooth_block().is_ok() {
        model.set_bluetooth_session(Ok(bluetooth::preview_session(phase)));
    }
    if let Some(view) = flow {
        model.bt_view = view;
    }
    model.bt_forget_confirm = name == "bluetooth-forget";
    Some(model)
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
    let mut page = "root".to_owned();
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
        let model = preview_model(&page, &target)
            .ok_or_else(|| format!("unknown --page {page:?}; use {PREVIEW_PAGES}"))?;
        let ty = Type::load()?;
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
            model.page.name(),
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
        for name in PREVIEW_PAGES.split(", ").map(str::trim) {
            let model = preview_model(name, "qemu-aarch64").unwrap();
            assert!(name.starts_with(model.page.name()), "{name}");
            assert!(model.pending.is_empty(), "previews send nothing");
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
        assert!(preview_model("bogus", "qemu-aarch64").is_none());
        let absent = preview_model("bluetooth-absent", "x").unwrap();
        assert_eq!(absent.state_name(), "no-controller");
        let no_sim = preview_model("mobile-no-sim", "x").unwrap();
        assert_eq!(no_sim.state_name(), "no-sim");
        let sound = preview_model("sound", "x").unwrap();
        assert_eq!(sound.state_name(), "volume-40-unmuted");
        let no_level = preview_model("sound-unavailable", "x").unwrap();
        assert_eq!(no_level.state_name(), "no-output-level");
    }
}
