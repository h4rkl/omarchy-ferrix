//! Composition of the Settings pages: a root category list and one focused
//! page per category. Network glyph state is steel, the active toggle is
//! rust, typed failures are `err`, honest absences (`no SIM`, `no
//! controller`) are `warn`; everything else is slate and steel type.

use ferrix_hardware_api::Capability;
use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    ButtonKind, Tone, button, chip, clock_strip, gesture_pill, keyboard, list_row, notice,
    page_header, section_label, slider, text_field, wrap, wrap_text,
};

use crate::interaction::{Input, Layout, Target, keyboard_visible};
use crate::model::{Link, Model, Page};

pub fn compose(
    ty: &Type,
    size: (u32, u32),
    minute_of_day: u32,
    model: &Model,
    input: &Input,
) -> Canvas {
    let t = theme::get();
    let mut c = Canvas::new(size.0 as i32, size.1 as i32, t.bg);
    let clock = ferrix_ui::clock::hhmm(minute_of_day);
    let label = format!(
        "{} \u{b7} {}",
        model.network_label(),
        model.telephony_label()
    );
    clock_strip(&mut c, ty, &clock, Some(&label));
    let subtitle = (model.page != Page::Root).then(|| model.page.title());
    page_header(
        &mut c,
        ty,
        "Settings",
        subtitle,
        input.pressed == Some(Target::Back),
    );
    let l = Layout::new(size);
    match model.page {
        Page::Root => root(&mut c, ty, &l, model, input),
        Page::Network => network(&mut c, ty, &l, model, input),
        Page::Bluetooth => bluetooth(&mut c, ty, &l, model, input),
        Page::Mobile => mobile(&mut c, ty, &l, model, input),
        Page::Display => display(&mut c, ty, &l, model, input),
        Page::Radios => radios(&mut c, ty, &l, model, input),
        Page::About => about(&mut c, ty, &l, model),
    }
    if keyboard_visible(model) {
        let pressed = match input.pressed {
            Some(Target::Key(k)) => Some(k),
            _ => None,
        };
        keyboard(&mut c, ty, &model.osk, pressed);
    }
    gesture_pill(&mut c, input.pressed == Some(Target::Home));
    c
}

/// The category list: one row per page with its current state.
fn root(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    for (r, page) in l.section_rects() {
        list_row(
            c,
            ty,
            r,
            page.title(),
            &model.section_summary(page),
            None,
            input.pressed == Some(Target::Section(page)),
        );
    }
}

/// A notice for the last refused action, at design `y`.
fn error_notice(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, y: i32) {
    if let Some(error) = &model.last_error {
        notice(
            c,
            ty,
            Rect {
                x: l.s(48),
                y: l.s(y),
                w: c.width() - l.s(96),
                h: l.s(90),
            },
            error,
            Tone::Err,
        );
    }
}

/// A dim wrapped caption at design `y`.
fn caption(c: &mut Canvas, ty: &Type, l: &Layout, text: &str, y: i32) {
    wrap_text(c, ty, text, l.s(96), l.s(y), c.width() - l.s(192), l.s(27));
}

/// Bluetooth: controller power, then the remembered devices, or a plain
/// statement of why there is no controller to control.
fn bluetooth(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let pressed = input.pressed == Some(Target::BluetoothPower);
    match model.bluetooth_block() {
        Ok(()) => toggle_row(
            c,
            ty,
            l.bluetooth_power_rect(),
            "Bluetooth",
            &model.source_caption("controller present", Capability::Bluetooth),
            model.bluetooth_on(),
            pressed,
        ),
        Err(block) => list_row(
            c,
            ty,
            l.bluetooth_power_rect(),
            "Bluetooth",
            block.reason(),
            Some(("unavailable", Tone::Warn)),
            pressed,
        ),
    }
    match model.bluetooth_block() {
        Err(crate::system::BluetoothBlock::CoreUnavailable) => {
            caption(c, ty, l, &model.core_label(), 540);
        }
        Err(_) => caption(
            c,
            ty,
            l,
            "No Bluetooth controller is attached, so there is nothing to turn on or pair. \
             Wi-Fi and calls are unaffected.",
            540,
        ),
        Ok(()) => {
            section_label(c, ty, "devices", s(96), s(570));
            let dim = Style::regular(s(26) as f32, t.fg_dim);
            match &model.bt_devices {
                crate::system::DeviceList::Unknown => {
                    ty.text(c, dim, "loading", s(96), s(660));
                }
                crate::system::DeviceList::Failed(reason) => {
                    ty.text(c, dim, &format!("devices \u{b7} {reason}"), s(96), s(660));
                }
                crate::system::DeviceList::Loaded(devices) if devices.is_empty() => {
                    ty.text(c, dim, "none remembered", s(96), s(660));
                }
                crate::system::DeviceList::Loaded(devices) => {
                    for (device, r) in devices.iter().zip(l.device_rects(devices.len())) {
                        let sub = match (device.paired, device.connected) {
                            (_, true) => "paired \u{b7} connected",
                            (true, false) => "paired",
                            (false, false) => "seen \u{b7} not paired",
                        };
                        let trailing = device.connected.then_some(("connected", Tone::Ok));
                        let name = device.name.as_deref().unwrap_or("Unnamed device");
                        list_row(c, ty, r, name, sub, trailing, false);
                    }
                }
            }
            caption(
                c,
                ty,
                l,
                "Pairing and discovery are not available yet; this list comes from the \
                 system Bluetooth service.",
                1440,
            );
        }
    }
    error_notice(c, ty, l, model, 1580);
}

/// Mobile network: SIM, operator, signal (read-only rows), then the
/// mobile-data toggle. An absent SIM reads plainly, never as an error.
fn mobile(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let [sim_r, operator_r, signal_r] = l.mobile_rects();
    let (sim_word, sim_line) = model.sim_label();
    let sim_tone = match sim_word {
        "ready" => Tone::Ok,
        "absent" | "unknown" => Tone::Warn,
        _ => Tone::Err,
    };
    list_row(
        c,
        ty,
        sim_r,
        "SIM",
        sim_line,
        Some((sim_word, sim_tone)),
        false,
    );
    list_row(
        c,
        ty,
        operator_r,
        "Operator",
        &model.operator_label(),
        None,
        false,
    );
    let bars = model.signal_bars().map(|b| format!("{b}/4"));
    list_row(
        c,
        ty,
        signal_r,
        "Signal",
        &model.signal_label(),
        bars.as_deref().map(|b| (b, Tone::Steel)),
        false,
    );
    let data_sub = match model.data_block() {
        Ok(()) => format!(
            "{} \u{b7} {}",
            model.cellular_data_label(),
            model.telephony_label()
        ),
        Err(_) if model.cellular_data_on() => model.cellular_data_label(),
        Err(block) => block.reason().to_owned(),
    };
    toggle_row(
        c,
        ty,
        l.mobile_data_rect(),
        "Mobile data",
        &data_sub,
        model.cellular_data_on(),
        input.pressed == Some(Target::CellularData),
    );
    error_notice(c, ty, l, model, 1060);
    caption(
        c,
        ty,
        l,
        "State comes from ferrix-telephonyd; mobile data needs telephony-data. \
         Calls and SMS are in Phone and Messages.",
        1220,
    );
}

/// Display: the brightness rail and auto-rotate.
fn display(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    section_label(c, ty, "brightness", s(96), s(360));
    let enabled = model.display_block().is_ok();
    let value = model.brightness();
    slider(c, l.brightness_rect(), value.unwrap_or(0), enabled);
    let label = value.map_or_else(|| "unknown".to_owned(), |v| format!("{v}%"));
    ty.text(
        c,
        Style::bold(s(34) as f32, if enabled { t.fg } else { t.fg_dim }),
        &label,
        s(96),
        s(590),
    );
    let sub = model.source_caption("display brightness", Capability::Display);
    ty.text(
        c,
        Style::regular(s(26) as f32, t.fg_dim),
        &sub,
        s(260),
        s(590),
    );
    let pressed = input.pressed == Some(Target::AutoRotate);
    if enabled && model.rotation_sensor() {
        toggle_row(
            c,
            ty,
            l.auto_rotate_rect(),
            "Auto-rotate",
            &model.rotation_label(),
            model.auto_rotate(),
            pressed,
        );
    } else {
        list_row(
            c,
            ty,
            l.auto_rotate_rect(),
            "Auto-rotate",
            &model.rotation_label(),
            Some(("unavailable", Tone::Warn)),
            pressed,
        );
    }
    error_notice(c, ty, l, model, 900);
    caption(
        c,
        ty,
        l,
        "Brightness and rotation go through ferrix-core; off pins portrait, on follows \
         the rotation sensor.",
        1060,
    );
}

fn connection_tone(model: &Model) -> Tone {
    use ferrix_network_api::ConnectionState as C;
    match model.net.as_ref().map(|n| &n.connection) {
        Some(C::Connected { .. }) => Tone::Ok,
        Some(C::Failed { .. }) => Tone::Err,
        Some(C::Associating | C::Authenticating | C::Configuring) => Tone::Steel,
        _ => Tone::Neutral,
    }
}

fn connectivity_tone(model: &Model) -> Tone {
    use ferrix_network_api::Connectivity as K;
    match model.net.as_ref().map(|n| &n.connectivity) {
        Some(K::Full) => Tone::Ok,
        Some(K::Portal { .. } | K::Limited) => Tone::Warn,
        Some(K::Local) => Tone::Steel,
        _ => Tone::Neutral,
    }
}

fn network(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let radio_tone = if model.radio_enabled() {
        Tone::Accent
    } else {
        Tone::Neutral
    };
    chip(
        c,
        ty,
        l.radio_rect(),
        &model.radio_label(),
        radio_tone,
        input.pressed == Some(Target::Radio),
    );
    chip(
        c,
        ty,
        l.scan_rect(),
        &model.scan_label(),
        Tone::Neutral,
        input.pressed == Some(Target::Scan),
    );
    let dim = Style::regular(s(26) as f32, t.fg_dim);
    let evidence = model.net_evidence.as_deref().unwrap_or("no snapshot");
    ty.text(
        c,
        dim,
        &format!("{} \u{b7} {evidence}", model.adapter_label()),
        s(96),
        s(510),
    );
    chip(
        c,
        ty,
        l.connection_rect(),
        &model.connection_label(),
        connection_tone(model),
        false,
    );
    chip(
        c,
        ty,
        l.connectivity_rect(),
        &model.connectivity_label(),
        connectivity_tone(model),
        false,
    );
    for (i, line) in model.ip_lines().iter().enumerate() {
        ty.text(c, dim, line, s(96), s(660) + i as i32 * s(40));
    }
    if let Some(error) = model
        .last_error
        .as_deref()
        .map(str::to_owned)
        .or_else(|| model.connection_error())
    {
        let retry = model.last_error.is_none()
            && model.retry_target().is_some()
            && model.passphrase_for.is_none()
            && model.forget_confirm.is_none();
        notice(c, ty, l.notice_rect(retry), &error, Tone::Err);
        if retry {
            button(
                c,
                ty,
                l.retry_rect(),
                "Retry",
                ButtonKind::Primary,
                input.pressed == Some(Target::Retry),
                true,
            );
        }
    } else if model.net_link != Link::Connected {
        let tone = if model.net_link == Link::Unavailable {
            Tone::Warn
        } else {
            Tone::Steel
        };
        notice(c, ty, l.error_rect(), &model.network_label(), tone);
    }
    if model.forget_confirm.is_some() {
        forget_confirm(c, ty, l, model, input);
        return;
    }
    if let Some(ap_id) = &model.passphrase_for {
        let ssid = model
            .net
            .as_ref()
            .and_then(|n| n.access_point(ap_id))
            .map(|ap| ap.ssid.to_string())
            .unwrap_or_default();
        section_label(c, ty, &format!("passphrase \u{b7} {ssid}"), s(96), s(870));
        let shown: String = if model.passphrase_shown {
            model.passphrase.clone()
        } else {
            model.passphrase.chars().map(|_| '\u{2022}').collect()
        };
        text_field(c, ty, l.pass_field_rect(), &shown, "Passphrase", true);
        chip(
            c,
            ty,
            l.pass_reveal_rect(),
            if model.passphrase_shown {
                "hide"
            } else {
                "show"
            },
            Tone::Neutral,
            input.pressed == Some(Target::PassReveal),
        );
        button(
            c,
            ty,
            l.pass_connect_rect(),
            "Connect",
            ButtonKind::Primary,
            input.pressed == Some(Target::PassConnect),
            model.passphrase.chars().count() >= 8,
        );
        button(
            c,
            ty,
            l.pass_cancel_rect(),
            "Cancel",
            ButtonKind::Secondary,
            input.pressed == Some(Target::PassCancel),
            true,
        );
        return;
    }
    section_label(c, ty, "access points", s(96), s(870));
    let hidden = model.hidden_count();
    if hidden > 0 {
        // Hidden networks cannot be joined by name yet; say so instead of
        // drawing blank rows.
        let text = format!("{hidden} hidden \u{b7} join by name not supported");
        let w = ty.width(dim, &text);
        ty.text(c, dim, &text, c.width() - s(96) - w, s(870));
    }
    let aps = model.access_points();
    if aps.is_empty() {
        ty.text(c, dim, "none \u{b7} scan to refresh", s(96), s(960));
    }
    for (i, (ap, r)) in aps.iter().zip(l.ap_rects(aps.len())).enumerate() {
        let connected = model.connected_ap() == Some(&ap.id);
        let sub = format!(
            "{}% \u{b7} {} \u{b7} {} MHz",
            ap.signal_percent,
            ap.security.name(),
            ap.frequency_mhz.map_or("?".to_owned(), |f| f.to_string())
        );
        let trailing = if connected {
            Some(("connected", Tone::Ok))
        } else if ap.security.needs_passphrase() {
            Some(("passphrase", Tone::Neutral))
        } else {
            None
        };
        list_row(
            c,
            ty,
            r,
            &ap.ssid.to_string(),
            &sub,
            trailing,
            input.pressed == Some(Target::Ap(i)),
        );
    }
    section_label(c, ty, "saved profiles", s(96), s(1620));
    let profiles = model.profiles();
    if profiles.is_empty() {
        ty.text(c, dim, "none", s(96), s(1710));
    }
    for (i, (p, r)) in profiles
        .iter()
        .zip(l.profile_rects(profiles.len()))
        .enumerate()
    {
        let sub = format!(
            "{} \u{b7} autoconnect {}",
            p.security.name(),
            if p.autoconnect { "on" } else { "off" }
        );
        list_row(
            c,
            ty,
            r,
            &p.ssid.to_string(),
            &sub,
            Some(("forget", Tone::Err)),
            input.pressed == Some(Target::Profile(i)),
        );
    }
}

/// Forget confirmation, in the Contacts delete-confirmation pattern: a
/// plain question, what is lost, a danger button and Cancel.
fn forget_confirm(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let ssid = model
        .forget_ssid()
        .unwrap_or_else(|| "this network".to_owned());
    ty.text(
        c,
        Style::bold(s(44) as f32, t.fg),
        "Forget network?",
        s(96),
        s(890),
    );
    wrap_text(
        c,
        ty,
        &format!(
            "{ssid} and its saved passphrase are removed. You will need the passphrase to join again."
        ),
        s(96),
        s(950),
        c.width() - s(192),
        s(30),
    );
    button(
        c,
        ty,
        l.forget_confirm_rect(),
        "Forget",
        ButtonKind::Danger,
        input.pressed == Some(Target::ForgetConfirm),
        true,
    );
    button(
        c,
        ty,
        l.forget_cancel_rect(),
        "Cancel",
        ButtonKind::Secondary,
        input.pressed == Some(Target::ForgetCancel),
        true,
    );
}

fn toggle_row(c: &mut Canvas, ty: &Type, r: Rect, title: &str, sub: &str, on: bool, pressed: bool) {
    let state = if on {
        ("on", Tone::Accent)
    } else {
        ("off", Tone::Neutral)
    };
    list_row(c, ty, r, title, sub, Some(state), pressed);
}

fn radios(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    toggle_row(
        c,
        ty,
        l.airplane_rect(),
        "Airplane mode",
        "wi-fi radio and cellular radio off",
        model.airplane(),
        input.pressed == Some(Target::Airplane),
    );
    toggle_row(
        c,
        ty,
        l.wifi_rect(),
        "Wi-Fi radio",
        &format!("{} \u{b7} {}", model.radio_label(), model.network_label()),
        model.radio_enabled(),
        input.pressed == Some(Target::Wifi),
    );
    toggle_row(
        c,
        ty,
        l.cellular_rect(),
        "Cellular radio",
        &format!(
            "{} \u{b7} {}",
            model.cellular_label(),
            model.telephony_label()
        ),
        model.cellular_on(),
        input.pressed == Some(Target::Cellular),
    );
    error_notice(c, ty, l, model, 1000);
    ty.text(
        c,
        Style::regular(l.s(26) as f32, t.fg_dim),
        "cellular radio needs telephony-radio; mobile data is on Mobile network",
        l.s(96),
        l.s(1200),
    );
}

/// The About page's key/value rows.
pub fn about_rows(model: &Model) -> [(&'static str, String); 7] {
    [
        ("target", model.target.clone()),
        ("network evidence", model.net_evidence_label()),
        ("telephony evidence", model.tele_evidence_label()),
        ("app schema", ferrix_app_api::SCHEMA_VERSION.to_string()),
        (
            "telephony schema",
            ferrix_telephony_api::SCHEMA_VERSION.to_string(),
        ),
        (
            "network schema",
            ferrix_network_api::SCHEMA_VERSION.to_string(),
        ),
        (
            "contacts schema",
            ferrix_contacts_api::SCHEMA_VERSION.to_string(),
        ),
    ]
}

pub const ABOUT_FOOTER: &str =
    "State comes from the daemons named above; nothing here is a physical-device claim.";

fn about(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model) {
    let t = theme::get();
    let s = |v| l.s(v);
    let max = c.width() - s(192);
    let rows = about_rows(model);
    for (i, (key, value)) in rows.iter().enumerate() {
        let y = s(430) + i as i32 * s(120);
        section_label(c, ty, key, s(96), y);
        let mut style = Style::bold(s(36) as f32, t.fg);
        while ty.width(style, value) > max && style.px > 16.0 {
            style.px -= 2.0;
        }
        ty.text(c, style, value, s(96), y + s(52));
    }
    let footer = Style::regular(s(26) as f32, t.fg_dim);
    let size = (c.width() as u32, c.height() as u32);
    for (i, line) in about_footer_lines(ty, size).iter().enumerate() {
        ty.text(c, footer, line, s(96), s(1330) + i as i32 * s(39));
    }
}

/// Lines of the About footer at `size`, as drawn.
pub fn about_footer_lines(ty: &Type, size: (u32, u32)) -> Vec<String> {
    let l = Layout::new(size);
    let style = Style::regular(l.s(26) as f32, theme::get().fg_dim);
    wrap(ty, style, ABOUT_FOOTER, size.0 as i32 - l.s(192))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preview_network;
    use ferrix_network_api::ApId;
    use ferrix_network_protocol::Feed;

    fn model(connected: bool) -> Model {
        let mut m = Model::new();
        m.show(Page::Network);
        m.apply_network(Feed::Snapshot {
            state: preview_network(connected),
            evidence: "mock".into(),
        });
        m
    }

    #[test]
    fn pages_render_chips_rows_and_toggles() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let input = Input::default();
        let l = Layout::new((1080, 2340));
        let m = model(true);
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let r = l.radio_rect();
        assert_eq!(
            c.get(r.x + r.w / 2, r.y),
            t.rust,
            "enabled radio chip is the accent"
        );
        let cn = l.connection_rect();
        assert_eq!(c.get(cn.x + cn.w / 2, cn.y), t.ok, "connected chip is ok");
        let ap = l.ap_rects(3)[0];
        assert_eq!(c.get(ap.x + 8, ap.y + 8), t.surface);

        let mut m = model(false);
        m.select_ap(&ApId::new("ap-2"));
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let f = l.pass_field_rect();
        assert_eq!(
            c.get(f.x + f.w / 2, f.y),
            t.rust,
            "passphrase field focused"
        );
        let (kb_top, _) = ferrix_ui::osk::extent((1080, 2340));
        assert_eq!(c.get(540, kb_top + 2), t.bg_dark);
        let reveal = l.pass_reveal_rect();
        assert!(reveal.x >= f.x + f.w, "toggle sits beside the field");
        assert_eq!(c.get(reveal.x + 8, reveal.y + 8), t.surface);
        m.passphrase = "Mock-Pass1".to_owned();
        let masked = compose(&ty, (1080, 2340), 600, &m, &input);
        m.passphrase_shown = true;
        let shown = compose(&ty, (1080, 2340), 600, &m, &input);
        let differs =
            (f.x..f.x + f.w).any(|x| (f.y..f.y + f.h).any(|y| masked.get(x, y) != shown.get(x, y)));
        assert!(differs, "show/hide changes the field text");
        m.passphrase_shown = false;

        m.cancel_passphrase();
        m.show(Page::Radios);
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let a = l.airplane_rect();
        assert_eq!(c.get(a.x + 8, a.y + 8), t.surface);
        m.show(Page::About);
        m.target = "qemu-aarch64".to_owned();
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let rendered =
            (l.s(400)..l.s(500)).any(|y| (l.s(96)..l.s(400)).any(|x| c.get(x, y) != t.bg));
        assert!(rendered, "about text rendered");
    }

    #[test]
    fn about_reports_live_telephony_and_keeps_the_footer_on_screen() {
        let ty = Type::load().unwrap();
        let mut m = model(true);
        let mut tele = ferrix_telephony_api::TelephonyState::new();
        tele.radio = ferrix_telephony_api::RadioPower::On;
        m.apply_telephony(ferrix_telephony_protocol::Update::Snapshot(
            ferrix_telephony_protocol::StateView::Redacted(tele.redacted()),
        ));
        m.set_tele_evidence("QEMU");
        let rows = about_rows(&m);
        assert_eq!(rows[1], ("network evidence", "mock".to_owned()));
        assert_eq!(rows[2], ("telephony evidence", "QEMU".to_owned()));
        for size in [(1080, 2340), (720, 1600), (1080, 2424)] {
            let l = Layout::new(size);
            let style = Style::regular(l.s(26) as f32, theme::get().fg_dim);
            let lines = about_footer_lines(&ty, size);
            assert!(lines.len() > 1, "footer wraps at {size:?}");
            for line in &lines {
                assert!(
                    l.s(96) + ty.width(style, line) <= size.0 as i32 - l.s(96),
                    "{line:?} overflows at {size:?}"
                );
            }
        }
    }

    #[test]
    fn forget_confirmation_and_auth_retry_render() {
        use ferrix_network_api::{ConnectionState, NetworkError, NetworkEvent, ProfileId};
        let t = theme::get();
        let ty = Type::load().unwrap();
        let l = Layout::new((1080, 2340));
        let mut m = model(true);
        m.forget(&ProfileId::new("profile-lab"));
        let c = compose(&ty, (1080, 2340), 600, &m, &Input::default());
        let f = l.forget_confirm_rect();
        assert_eq!(
            c.get(f.x + f.w / 2, f.y),
            t.err,
            "Forget is the danger button"
        );
        m.cancel_forget();
        m.select_ap(&ApId::new("ap-1"));
        m.apply_network(Feed::Event(NetworkEvent::ConnectionChanged {
            connection: ConnectionState::Failed {
                error: NetworkError::AuthenticationFailed,
            },
        }));
        let c = compose(&ty, (1080, 2340), 600, &m, &Input::default());
        let r = l.retry_rect();
        assert_eq!(c.get(r.x + r.w / 2, r.y), t.rust, "Retry is primary");
    }

    fn system_model() -> Model {
        let mut m = model(true);
        m.apply_core(ferrix_protocol::Feed::Snapshot {
            target: "qemu-aarch64".into(),
            profile: "emulator-dev".into(),
            capabilities: crate::system::preview_capabilities(),
            state: crate::system::preview_hardware(),
        });
        m
    }

    /// Whether any pixel of `r`'s outline row is `color`.
    fn outlined(c: &Canvas, r: Rect, color: u32) -> bool {
        (r.x..r.x + r.w).any(|x| c.get(x, r.y) == color)
    }

    #[test]
    fn root_display_bluetooth_and_mobile_pages_render() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let size = (1080, 2340);
        let l = Layout::new(size);
        let mut m = system_model();
        m.show(Page::Root);
        let c = compose(&ty, size, 600, &m, &Input::default());
        for (r, _) in l.section_rects() {
            assert_eq!(c.get(r.x + 8, r.y + 8), t.surface, "category row");
        }
        m.show(Page::Display);
        let c = compose(&ty, size, 600, &m, &Input::default());
        let rail = l.brightness_rect();
        assert_eq!(c.get(rail.x + 4, rail.y + rail.h / 2), t.rust, "rail fill");
        assert!(outlined(&c, l.auto_rotate_rect(), t.border));
        // Bluetooth without a controller: a warn chip, no device list.
        let mut hw = crate::system::preview_hardware();
        hw.bluetooth.present = false;
        m.apply_core(ferrix_protocol::Feed::Snapshot {
            target: "qemu-aarch64".into(),
            profile: "emulator-dev".into(),
            capabilities: crate::system::preview_capabilities(),
            state: hw,
        });
        m.show(Page::Bluetooth);
        let c = compose(&ty, size, 600, &m, &Input::default());
        let row = l.bluetooth_power_rect();
        let warn =
            (row.x..row.x + row.w).any(|x| (row.y..row.y + row.h).any(|y| c.get(x, y) == t.warn));
        assert!(warn, "unavailable chip is warn");
        // Mobile with no telephony snapshot still renders every row.
        m.show(Page::Mobile);
        let c = compose(&ty, size, 600, &m, &Input::default());
        for r in l.mobile_rects() {
            assert_eq!(c.get(r.x + 8, r.y + 8), t.surface);
        }
    }
}
