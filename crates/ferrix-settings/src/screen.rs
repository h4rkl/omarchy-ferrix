//! Composition of the Settings pages. Network glyph state is steel, the
//! active toggle is rust, typed failures are `err`; everything else is
//! slate and steel type.

use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    ButtonKind, Tone, button, chip, clock_strip, gesture_pill, keyboard, list_row, notice,
    page_header, section_label, text_field,
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
    let clock = format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60);
    let label = format!(
        "{} \u{b7} {}",
        model.network_label(),
        model.telephony_label()
    );
    clock_strip(&mut c, ty, &clock, Some(&label));
    page_header(
        &mut c,
        ty,
        "Settings",
        Some(model.page.title()),
        input.pressed == Some(Target::Back),
    );
    let l = Layout::new(size);
    for (r, page) in l.tab_rects() {
        let tone = if model.page == page {
            Tone::Accent
        } else {
            Tone::Neutral
        };
        chip(
            &mut c,
            ty,
            r,
            page.title(),
            tone,
            input.pressed == Some(Target::Tab(page)),
        );
    }
    match model.page {
        Page::Network => network(&mut c, ty, &l, model, input),
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
        notice(c, ty, l.error_rect(), &error, Tone::Err);
    } else if model.net_link != Link::Connected {
        let tone = if model.net_link == Link::Unavailable {
            Tone::Warn
        } else {
            Tone::Steel
        };
        notice(c, ty, l.error_rect(), &model.network_label(), tone);
    }
    if let Some(ap_id) = &model.passphrase_for {
        let ssid = model
            .net
            .as_ref()
            .and_then(|n| n.access_point(ap_id))
            .map(|ap| ap.ssid.to_string())
            .unwrap_or_default();
        section_label(c, ty, &format!("passphrase \u{b7} {ssid}"), s(96), s(870));
        let shown: String = model.passphrase.chars().map(|_| '\u{2022}').collect();
        text_field(c, ty, l.pass_field_rect(), &shown, "Passphrase", true);
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
    if let Some(error) = &model.last_error {
        notice(
            c,
            ty,
            Rect {
                x: l.s(48),
                y: l.s(1000),
                w: c.width() - l.s(96),
                h: l.s(90),
            },
            error,
            Tone::Err,
        );
    }
    ty.text(
        c,
        Style::regular(l.s(26) as f32, t.fg_dim),
        "cellular power needs the telephony-call grant; a refusal shows here as a typed error",
        l.s(96),
        l.s(1150),
    );
}

fn about(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model) {
    let t = theme::get();
    let s = |v| l.s(v);
    let rows: [(&str, String); 7] = [
        ("target", model.target.clone()),
        (
            "network evidence",
            model
                .net_evidence
                .clone()
                .unwrap_or_else(|| "no snapshot".to_owned()),
        ),
        (
            "telephony evidence",
            model
                .tele_evidence
                .clone()
                .unwrap_or_else(|| "no snapshot".to_owned()),
        ),
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
    ];
    for (i, (key, value)) in rows.iter().enumerate() {
        let y = s(430) + i as i32 * s(120);
        section_label(c, ty, key, s(96), y);
        ty.text(c, Style::bold(s(36) as f32, t.fg), value, s(96), y + s(52));
    }
    ty.text(
        c,
        Style::regular(s(26) as f32, t.fg_dim),
        "state comes from the daemons named above; nothing here is a physical-device claim",
        s(96),
        s(1330),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preview_network;
    use ferrix_network_api::ApId;
    use ferrix_network_protocol::Feed;

    fn model(connected: bool) -> Model {
        let mut m = Model::new();
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
        let (tab, _) = l.tab_rects()[0];
        assert_eq!(c.get(tab.x + tab.w / 2, tab.y), t.rust, "active tab");

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
}
