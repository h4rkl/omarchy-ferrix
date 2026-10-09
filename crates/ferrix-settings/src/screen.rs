//! Composition of the Settings pages: a root category list and one focused
//! page per category. Network glyph state is steel, the active toggle is
//! rust, typed failures are `err`, honest absences (`no SIM`, `no
//! controller`) are `warn`; everything else is slate and steel type.

use ferrix_hardware_api::Capability;
use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    ButtonKind, Tone, button, chip, clock_strip, ellipsize, gesture_pill, keyboard, list_row,
    notice, page_header, section_label, slider, text_field, wrap, wrap_text,
};

use crate::bluetooth::{BtView, Card, device_name, failure_text, format_passkey, kind_label};
use crate::interaction::{Input, Layout, Target, card_buttons, keyboard_visible};
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
    let subtitle = match model.page {
        Page::Root => None,
        Page::Bluetooth => Some(model.bluetooth_subtitle()),
        page => Some(page.title()),
    };
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

/// Bluetooth: the saved-devices screen (power, Pair new device, saved
/// devices), Pair new device (scan, available devices), or one device's
/// detail; a pairing card replaces the lists while it applies. Without a
/// controller, a plain statement of why there is nothing to control.
fn bluetooth(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    if let Err(block) = model.bluetooth_block() {
        list_row(
            c,
            ty,
            l.bluetooth_power_rect(),
            "Bluetooth",
            block.reason(),
            Some(("unavailable", Tone::Warn)),
            input.pressed == Some(Target::BluetoothPower),
        );
        let text = match block {
            crate::system::BluetoothBlock::CoreUnavailable => model.core_label(),
            _ => "No Bluetooth controller is attached, so there is nothing to turn on or pair. \
                  Wi-Fi and calls are unaffected."
                .to_owned(),
        };
        caption(c, ty, l, &text, 540);
        error_notice(c, ty, l, model, 1580);
        return;
    }
    match model.bt_view {
        BtView::Main => bluetooth_main(c, ty, l, model, input),
        BtView::Pair => bluetooth_pair(c, ty, l, model, input),
        BtView::Device(_) => bluetooth_device(c, ty, l, model, input),
    }
}

fn bluetooth_main(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    toggle_row(
        c,
        ty,
        l.bluetooth_power_rect(),
        "Bluetooth",
        &model.source_caption("controller present", Capability::Bluetooth),
        model.bluetooth_on(),
        input.pressed == Some(Target::BluetoothPower),
    );
    if let Some(card) = model.bt_card() {
        pairing_card(c, ty, l, model, input, card);
        return;
    }
    let pair_pressed = input.pressed == Some(Target::BtPairNew);
    let (pair_sub, pair_trailing) = match (&model.bt_devices, model.bluetooth_on()) {
        (_, false) => ("turn Bluetooth on to pair", None),
        (
            crate::bluetooth::DeviceList::Loaded {
                controls: false, ..
            },
            true,
        ) => (
            "this target's Bluetooth service cannot pair",
            Some(("unavailable", Tone::Warn)),
        ),
        _ => ("find nearby devices to pair", Some(("+", Tone::Accent))),
    };
    list_row(
        c,
        ty,
        l.bt_pair_row_rect(),
        "Pair new device",
        pair_sub,
        pair_trailing,
        pair_pressed,
    );
    section_label(c, ty, "saved devices", s(96), s(680));
    let dim = Style::regular(s(26) as f32, t.fg_dim);
    match &model.bt_devices {
        crate::bluetooth::DeviceList::Unknown => ty.text(c, dim, "loading", s(96), s(770)),
        crate::bluetooth::DeviceList::Failed(reason) => {
            ty.text(c, dim, &format!("devices \u{b7} {reason}"), s(96), s(770));
        }
        crate::bluetooth::DeviceList::Loaded { .. } => {
            let saved = model.saved_devices();
            if saved.is_empty() {
                ty.text(c, dim, "none saved", s(96), s(770));
            }
            for (i, (device, r)) in saved.iter().zip(l.saved_rects(saved.len())).enumerate() {
                let sub = format!(
                    "{} \u{b7} {}",
                    kind_label(device.kind),
                    if device.connected {
                        "connected"
                    } else {
                        "paired"
                    }
                );
                let trailing = device.connected.then_some(("connected", Tone::Ok));
                list_row(
                    c,
                    ty,
                    r,
                    device_name(device),
                    &sub,
                    trailing,
                    input.pressed == Some(Target::BtSaved(i)),
                );
            }
        }
    }
    caption(
        c,
        ty,
        l,
        "Devices come from the system Bluetooth service. Tap a saved device to connect, \
         disconnect, or forget it.",
        1560,
    );
    error_notice(c, ty, l, model, 1700);
}

fn bluetooth_pair(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    // The card is the whole screen while it applies (the scan is paused
    // for pairing anyway).
    if let Some(card) = model.bt_card() {
        if let Some(device) = model.pairing().and_then(|p| model.bt_device(&p.device)) {
            let sub = match device.signal_bars {
                Some(bars) => format!("{} \u{b7} signal {bars}/4", kind_label(device.kind)),
                None => kind_label(device.kind).to_owned(),
            };
            list_row(
                c,
                ty,
                l.bt_scan_rect(),
                device_name(device),
                &sub,
                None,
                false,
            );
        }
        pairing_card(c, ty, l, model, input, card);
        return;
    }
    let t = theme::get();
    let s = |v| l.s(v);
    let discovery = model.discovery();
    let available = model.available_devices();
    let mut found = match available.len() {
        1 => "1 available".to_owned(),
        n => format!("{n} available"),
    };
    let unnamed = model.unnamed_count();
    if unnamed > 0 {
        found.push_str(&format!(" \u{b7} {unnamed} unnamed hidden"));
    }
    let (title, sub, trailing) = if discovery.active {
        let sub = match discovery.remaining_ms {
            Some(ms) => format!("{found} \u{b7} stops in {} s", ms.div_ceil(1000)),
            None => found,
        };
        ("Scanning", sub, ("scanning", Tone::Steel))
    } else {
        (
            "Scan stopped",
            format!("{found} \u{b7} tap to scan again"),
            ("scan", Tone::Accent),
        )
    };
    list_row(
        c,
        ty,
        l.bt_scan_rect(),
        title,
        &sub,
        Some(trailing),
        input.pressed == Some(Target::BtScan),
    );
    if discovery.active {
        // Remaining scan time: the rail empties as the bound approaches.
        let rail = l.bt_progress_rect();
        c.fill_rounded(rail, s(4), t.surface_hi);
        let total = ferrix_hardware_api::BLUETOOTH_DISCOVERY_TIMEOUT_MS;
        let left = discovery.remaining_ms.unwrap_or(total).min(total);
        let w = (i64::from(rail.w) * i64::from(left) / i64::from(total)) as i32;
        c.fill_rounded(
            Rect {
                w: w.max(s(8)),
                ..rail
            },
            s(4),
            t.rust,
        );
    }
    section_label(c, ty, "available devices", s(96), s(560));
    let dim = Style::regular(s(26) as f32, t.fg_dim);
    if available.is_empty() {
        let text = if discovery.active {
            "Looking for devices\u{2026}"
        } else {
            "No devices found"
        };
        ty.text(c, dim, text, s(96), s(650));
    }
    for (i, (device, r)) in available
        .iter()
        .zip(l.available_rects(available.len()))
        .enumerate()
    {
        let sub = match device.signal_bars {
            Some(bars) => format!("{} \u{b7} signal {bars}/4", kind_label(device.kind)),
            None => format!("{} \u{b7} not in range", kind_label(device.kind)),
        };
        list_row(
            c,
            ty,
            r,
            device_name(device),
            &sub,
            Some(("pair", Tone::Neutral)),
            input.pressed == Some(Target::BtAvailable(i)),
        );
    }
    caption(
        c,
        ty,
        l,
        "Put the other device in pairing mode. Scanning stops by itself after 30 s; \
         device names stay on this phone.",
        1700,
    );
    error_notice(c, ty, l, model, 1840);
}

/// The focused pairing card: the question and code, or the outcome.
fn pairing_card(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input, card: Card) {
    let t = theme::get();
    let s = |v| l.s(v);
    let name = model.pairing_name();
    let width = c.width() - s(192);
    let title = Style::bold(s(44) as f32, t.fg);
    let code = |c: &mut Canvas, passkey: u32| {
        let style = Style::bold(s(96) as f32, t.fg);
        let text = format_passkey(passkey);
        let w = ty.width(style, &text);
        ty.text(c, style, &text, (c.width() - w) / 2, s(860));
    };
    match card {
        Card::Confirm { passkey } => {
            ty.text(
                c,
                title,
                &ellipsize(ty, title, &format!("Pair with {name}?"), width),
                s(96),
                s(620),
            );
            wrap_text(
                c,
                ty,
                &format!("Check that {name} shows the same code."),
                s(96),
                s(680),
                width,
                s(30),
            );
            code(c, passkey);
            caption(
                c,
                ty,
                l,
                "Answer within 30 s; an unanswered request is declined.",
                940,
            );
        }
        Card::Authorize => {
            ty.text(
                c,
                title,
                &ellipsize(ty, title, &format!("Pair with {name}?"), width),
                s(96),
                s(620),
            );
            wrap_text(
                c,
                ty,
                &format!("{name} asks to pair without a code. Pair only if you started this."),
                s(96),
                s(680),
                width,
                s(30),
            );
            caption(
                c,
                ty,
                l,
                "Answer within 30 s; an unanswered request is declined.",
                940,
            );
        }
        Card::Display { passkey } => {
            ty.text(
                c,
                title,
                &ellipsize(ty, title, &format!("Type this code on {name}"), width),
                s(96),
                s(620),
            );
            wrap_text(
                c,
                ty,
                "Then press Enter on that device.",
                s(96),
                s(680),
                width,
                s(30),
            );
            code(c, passkey);
        }
        Card::Pairing => {
            ty.text(
                c,
                title,
                &ellipsize(ty, title, &format!("Pairing with {name}\u{2026}"), width),
                s(96),
                s(620),
            );
            wrap_text(
                c,
                ty,
                "Keep the device close and in pairing mode.",
                s(96),
                s(680),
                width,
                s(30),
            );
        }
        Card::Paired => {
            notice(
                c,
                ty,
                Rect {
                    x: s(48),
                    y: s(600),
                    w: c.width() - s(96),
                    h: s(90),
                },
                &format!("Paired with {name}"),
                Tone::Ok,
            );
            caption(c, ty, l, "It is saved and connects when in range.", 760);
        }
        Card::Failed(reason) => {
            notice(
                c,
                ty,
                Rect {
                    x: s(48),
                    y: s(600),
                    w: c.width() - s(96),
                    h: s(90),
                },
                &failure_text(reason, &name),
                Tone::Err,
            );
            caption(
                c,
                ty,
                l,
                "Make sure the other device is in pairing mode, then try again.",
                760,
            );
        }
    }
    let label = |target: Target| match target {
        Target::BtAccept => "Pair",
        Target::BtDecline => "Cancel",
        Target::BtDone => "Done",
        Target::BtRetry => "Try again",
        _ => "",
    };
    let (left, right) = card_buttons(card);
    if let Some(target) = left {
        button(
            c,
            ty,
            l.bt_card_left_rect(),
            label(target),
            ButtonKind::Primary,
            input.pressed == Some(target),
            true,
        );
    }
    if let Some(target) = right {
        button(
            c,
            ty,
            l.bt_card_right_rect(),
            label(target),
            ButtonKind::Secondary,
            input.pressed == Some(target),
            true,
        );
    }
}

fn bluetooth_device(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let Some(device) = model.detail_device() else {
        return;
    };
    let name = device_name(device);
    let sub = format!(
        "{} \u{b7} paired \u{b7} {}",
        kind_label(device.kind),
        if device.connected {
            "connected"
        } else {
            "not connected"
        }
    );
    list_row(
        c,
        ty,
        l.bt_device_rect(),
        name,
        &sub,
        device.connected.then_some(("connected", Tone::Ok)),
        false,
    );
    let status = Rect {
        x: s(48),
        y: s(480),
        w: c.width() - s(96),
        h: s(90),
    };
    match model.detail_operation() {
        Some(op) => match &op.failed {
            Some(reason) => notice(
                c,
                ty,
                status,
                &format!("Couldn't {} \u{b7} {reason}", op.action.name()),
                Tone::Err,
            ),
            None => notice(
                c,
                ty,
                status,
                match op.action {
                    ferrix_hardware_api::BluetoothAction::Connect => "Connecting\u{2026}",
                    ferrix_hardware_api::BluetoothAction::Disconnect => "Disconnecting\u{2026}",
                },
                Tone::Steel,
            ),
        },
        None => {
            if let Some(error) = &model.last_error {
                notice(c, ty, status, error, Tone::Err);
            }
        }
    }
    let busy = model
        .detail_operation()
        .is_some_and(|op| op.failed.is_none());
    if model.bt_forget_confirm {
        ty.text(
            c,
            Style::bold(s(44) as f32, t.fg),
            "Forget device?",
            s(96),
            s(700),
        );
        wrap_text(
            c,
            ty,
            &format!("{name} is unpaired and removed from this phone. Pair it again to use it."),
            s(96),
            s(760),
            c.width() - s(192),
            s(30),
        );
        button(
            c,
            ty,
            l.bt_forget_confirm_rect(),
            "Forget",
            ButtonKind::Danger,
            input.pressed == Some(Target::BtForgetConfirm),
            true,
        );
        button(
            c,
            ty,
            l.bt_forget_cancel_rect(),
            "Cancel",
            ButtonKind::Secondary,
            input.pressed == Some(Target::BtForgetCancel),
            true,
        );
        return;
    }
    button(
        c,
        ty,
        l.bt_connect_rect(),
        if device.connected {
            "Disconnect"
        } else {
            "Connect"
        },
        ButtonKind::Primary,
        input.pressed == Some(Target::BtConnect),
        model.bluetooth_on() && !busy,
    );
    button(
        c,
        ty,
        l.bt_forget_rect(),
        "Forget",
        ButtonKind::Danger,
        input.pressed == Some(Target::BtForget),
        true,
    );
    caption(
        c,
        ty,
        l,
        "Connections go through ferrix-core and the system Bluetooth service, which keeps \
         the bond.",
        860,
    );
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

    #[test]
    fn bluetooth_pair_flow_renders_scan_rail_card_and_detail() {
        use crate::bluetooth::{BtView, preview_session};
        use ferrix_hardware_api::{PairingFailure, PairingPhase};
        let t = theme::get();
        let ty = Type::load().unwrap();
        let size = (1080, 2340);
        let l = Layout::new(size);
        let mut m = system_model();
        m.show(Page::Bluetooth);
        m.set_bluetooth_session(Ok(preview_session(None)));
        m.bt_view = BtView::Pair;
        let c = compose(&ty, size, 600, &m, &Input::default());
        let rail = l.bt_progress_rect();
        assert_eq!(c.get(rail.x + 4, rail.y + rail.h / 2), t.rust, "scan rail");
        for r in l.available_rects(3) {
            assert_eq!(c.get(r.x + 8, r.y + 8), t.surface, "available row");
        }
        m.set_bluetooth_session(Ok(preview_session(Some(PairingPhase::ConfirmPasskey {
            passkey: 482_913,
        }))));
        let c = compose(&ty, size, 600, &m, &Input::default());
        let pair = l.bt_card_left_rect();
        assert_eq!(
            c.get(pair.x + pair.w / 2, pair.y),
            t.rust,
            "Pair is primary"
        );
        let code_drawn =
            (l.s(780)..l.s(880)).any(|y| (l.s(300)..l.s(780)).any(|x| c.get(x, y) == t.fg));
        assert!(code_drawn, "the passkey is drawn large");
        m.set_bluetooth_session(Ok(preview_session(Some(PairingPhase::Failed {
            reason: PairingFailure::TimedOut,
        }))));
        let c = compose(&ty, size, 600, &m, &Input::default());
        let err = (l.s(600)..l.s(690)).any(|y| c.get(l.s(70), y) == t.err);
        assert!(err, "failure notice is err");
        m.set_bluetooth_session(Ok(preview_session(None)));
        m.bt_view = BtView::Device("mock-bt-0".into());
        let c = compose(&ty, size, 600, &m, &Input::default());
        let forget = l.bt_forget_rect();
        assert_eq!(
            c.get(forget.x + forget.w / 2, forget.y),
            t.err,
            "Forget is danger"
        );
    }
}
