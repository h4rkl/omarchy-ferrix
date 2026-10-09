//! Composition of the Phone pages in the Omarchy Ferrix visual system:
//! slate surfaces, hairlines, one rust accent per region (the Call button,
//! the active route chip, the pressed control), status colors only on the
//! element they describe. Geometry comes from [`Layout`].

use ferrix_telephony_api::{AudioRoute, CallState, Direction};
use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    self, ButtonKind, Tone, button, chip, clock_strip, gesture_pill, keypad, list_row, notice,
    page_header, section_label,
};

use crate::interaction::{Input, Layout, Target, hold_label};
use crate::model::{CallView, Link, Model, Page, format_duration};

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
    clock_strip(&mut c, ty, &clock, Some(&status_label(model)));
    page_header(
        &mut c,
        ty,
        "Phone",
        Some(&model.link_label()),
        input.pressed == Some(Target::Back),
    );
    let l = Layout::new(size);
    match model.page {
        Page::Dialer => dialer(&mut c, ty, &l, model, input),
        Page::Recent => recent(&mut c, ty, &l, model, input),
        Page::Incoming => incoming(&mut c, ty, &l, model, input),
        Page::InCall => in_call(&mut c, ty, &l, model, input),
        Page::Ended => ended(&mut c, ty, &l, model, input),
    }
    if model.link == Link::Unavailable && model.page != Page::Dialer {
        notice(&mut c, ty, l.error_rect(), &model.link_label(), Tone::Warn);
    }
    gesture_pill(&mut c, input.pressed == Some(Target::Home));
    c
}

/// `modem · sim · registration · bars` from the daemon, labelled `mock`
/// when the operator is the mock's.
fn status_label(model: &Model) -> String {
    let Some(state) = &model.state else {
        return "telephony \u{b7} no snapshot".to_owned();
    };
    let source = match state.operator.as_ref().map(|o| o.name.as_str()) {
        Some(name) if name.to_lowercase().contains("mock") => " \u{b7} mock",
        _ => "",
    };
    format!(
        "{} \u{b7} sim {} \u{b7} {} \u{b7} {} bars{source}",
        state.modem.name(),
        state.sim.name(),
        state.registration.name(),
        state.signal.bars
    )
}

fn tabs(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    for (r, page) in l.tab_rects() {
        let tone = if model.page == page {
            Tone::Accent
        } else {
            Tone::Neutral
        };
        let label = match page {
            Page::Recent => "Recent",
            _ => "Dialer",
        };
        chip(
            c,
            ty,
            r,
            label,
            tone,
            input.pressed == Some(Target::Tab(page)),
        );
    }
}

fn dialer(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    tabs(c, ty, l, model, input);
    let nr = l.number_rect();
    c.fill_rounded(nr, s(10), t.bg_dark);
    c.stroke_rounded(nr, s(10), widgets::hairline(l.b), t.border);
    let typed = if model.typed.is_empty() {
        "\u{2014}"
    } else {
        model.typed.as_str()
    };
    let mut style = Style::bold(
        s(72) as f32,
        if model.typed.is_empty() {
            t.fg_dim
        } else {
            t.fg
        },
    );
    while ty.width(style, typed) > nr.w - s(48) && style.px > 20.0 {
        style.px -= 4.0;
    }
    let w = ty.width(style, typed);
    ty.text(
        c,
        style,
        typed,
        nr.x + (nr.w - w) / 2,
        nr.y + nr.h / 2 + (style.px * 0.36) as i32,
    );
    let contact = model.typed_label();
    let (line, tone) = match model.dial_block() {
        Ok(()) if !contact.is_empty() => (contact, Tone::Steel),
        Ok(()) => (model.dial_string(), Tone::Neutral),
        Err(block) => (block.reason().to_owned(), Tone::Warn),
    };
    let ls = Style::regular(s(30) as f32, tone.color());
    let lw = ty.width(ls, &line);
    ty.text(c, ls, &line, (c.width() - lw) / 2, s(640));
    let keys = l.dial_keys();
    let pressed = match input.pressed {
        Some(Target::Key(ch)) => Some(ch),
        _ => None,
    };
    keypad(c, ty, &keys, pressed);
    button(
        c,
        ty,
        l.call_rect(),
        "Call",
        ButtonKind::Primary,
        input.pressed == Some(Target::Call),
        model.dial_block().is_ok(),
    );
    button(
        c,
        ty,
        l.delete_rect(),
        "\u{232b}",
        ButtonKind::Secondary,
        input.pressed == Some(Target::Delete),
        !model.typed.is_empty(),
    );
    if let Some(error) = &model.last_error {
        notice(c, ty, l.error_rect(), error, Tone::Err);
    } else if model.link == Link::Unavailable {
        notice(c, ty, l.error_rect(), &model.link_label(), Tone::Warn);
    } else if model.link == Link::Reconnected {
        notice(c, ty, l.error_rect(), &model.link_label(), Tone::Steel);
    }
}

fn recent(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    tabs(c, ty, l, model, input);
    let calls = model.recent();
    if calls.is_empty() {
        let style = Style::bold(l.s(40) as f32, t.fg);
        let text = "No recent calls";
        ty.text(
            c,
            style,
            text,
            (c.width() - ty.width(style, text)) / 2,
            l.s(700),
        );
        return;
    }
    for (index, (view, r)) in calls.iter().zip(l.recent_rects(calls.len())).enumerate() {
        let title = view.party_label(&model.contacts, &model.region);
        let dir = match view.call.direction {
            Direction::Incoming => "incoming",
            Direction::Outgoing => "outgoing",
        };
        let (state, tone) = call_state_chip(view.call.state, view.call.end_reason.as_ref());
        let subtitle = match model.duration_secs(&view.call.id) {
            Some(secs) => format!("{dir} \u{b7} {}", format_duration(secs)),
            None => dir.to_owned(),
        };
        list_row(
            c,
            ty,
            r,
            &title,
            &subtitle,
            Some((state, tone)),
            input.pressed == Some(Target::Recent(index)),
        );
    }
}

/// `(label, tone)` of a call's state chip: live states steel, clean ends
/// neutral, failures `err`.
pub fn call_state_chip(
    state: CallState,
    end: Option<&ferrix_telephony_api::CallEndReason>,
) -> (&'static str, Tone) {
    use ferrix_telephony_api::CallEndReason as R;
    match (state, end) {
        (CallState::Ended, Some(reason)) => {
            let tone = match reason {
                R::LocalHangup | R::RemoteHangup => Tone::Neutral,
                R::Rejected | R::NoAnswer | R::Busy => Tone::Warn,
                R::TimedOut | R::NoService | R::ModemLost | R::Failed { .. } => Tone::Err,
            };
            (reason.name(), tone)
        }
        (CallState::Ended, None) => ("ended", Tone::Neutral),
        (CallState::Active, _) => ("active", Tone::Ok),
        (CallState::Held, _) => ("held", Tone::Warn),
        (CallState::Dialing, _) => ("dialing", Tone::Steel),
        (CallState::Ringing, _) => ("ringing", Tone::Steel),
    }
}

fn party_block(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, eyebrow: &str) {
    let t = theme::get();
    let s = |v| l.s(v);
    let Some(view) = model.focused() else {
        return;
    };
    section_label(c, ty, eyebrow, s(96), s(420));
    let name = view.party_label(&model.contacts, &model.region);
    let mut style = Style::bold(s(72) as f32, t.fg);
    while ty.width(style, &name) > c.width() - s(192) && style.px > 24.0 {
        style.px -= 4.0;
    }
    ty.text(c, style, &name, s(96), s(560));
    if let Some(sub) = party_subtitle(view, &name) {
        ty.text(
            c,
            Style::regular(s(30) as f32, t.fg_dim),
            &sub,
            s(96),
            s(640),
        );
    }
    let (label, tone) = call_state_chip(view.call.state, view.call.end_reason.as_ref());
    chip(
        c,
        ty,
        Rect {
            x: s(96),
            y: s(700),
            w: s(280),
            h: s(64),
        },
        label,
        tone,
        false,
    );
    if let Some(secs) = model.duration_secs(&view.call.id) {
        ty.text(
            c,
            Style::regular(s(36) as f32, t.fg_dim),
            &format_duration(secs),
            s(400),
            s(746),
        );
    }
}

/// The line under the party name: the number when the name is a contact
/// or network display name; nothing when the name already is the number
/// (full or redacted), so a number never appears twice.
pub fn party_subtitle(view: &CallView, name: &str) -> Option<String> {
    let number = view
        .number
        .as_ref()
        .map_or(view.call.party.party_redacted.as_str(), |n| n.as_str());
    (name != number && name != view.call.party.party_redacted).then(|| number.to_owned())
}

/// Route chip copy: title-cased, `· n/a` when the backend lacks the route.
pub fn route_label(route: AudioRoute, available: bool) -> String {
    let name = match route {
        AudioRoute::Earpiece => "Earpiece",
        AudioRoute::Speaker => "Speaker",
        AudioRoute::Wired => "Wired",
        AudioRoute::Bluetooth => "Bluetooth",
    };
    if available {
        name.to_owned()
    } else {
        format!("{name} \u{b7} n/a")
    }
}

fn incoming(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    party_block(c, ty, l, model, "incoming call");
    button(
        c,
        ty,
        l.answer_rect(),
        "Answer",
        ButtonKind::Primary,
        input.pressed == Some(Target::Answer),
        true,
    );
    button(
        c,
        ty,
        l.reject_rect(),
        "Reject",
        ButtonKind::Danger,
        input.pressed == Some(Target::Reject),
        true,
    );
}

fn in_call(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let s = |v| l.s(v);
    party_block(c, ty, l, model, "call");
    section_label(c, ty, "audio route", s(96), s(876));
    let available = model.routes_available();
    let active = model.active_route();
    for (r, route) in l.route_rects() {
        let tone = if active == Some(route) {
            Tone::Accent
        } else {
            Tone::Neutral
        };
        let label = route_label(route, available.contains(&route));
        chip(
            c,
            ty,
            r,
            &label,
            tone,
            input.pressed == Some(Target::Route(route)),
        );
    }
    let (hold, enabled) = hold_label(model);
    button(
        c,
        ty,
        l.hold_rect(),
        hold,
        ButtonKind::Secondary,
        input.pressed == Some(Target::Hold),
        enabled,
    );
    let dtmf_ok = model.supports(ferrix_telephony_api::TelephonyFeature::Dtmf)
        && model
            .focused()
            .is_some_and(|v| v.call.state == CallState::Active);
    button(
        c,
        ty,
        l.dtmf_toggle_rect(),
        if model.dtmf_open {
            "Hide keypad"
        } else {
            "Keypad"
        },
        ButtonKind::Secondary,
        input.pressed == Some(Target::DtmfToggle),
        dtmf_ok,
    );
    if model.dtmf_open {
        let pressed = match input.pressed {
            Some(Target::Dtmf(ch)) => Some(ch),
            _ => None,
        };
        keypad(c, ty, &l.dtmf_keys(), pressed);
    }
    button(
        c,
        ty,
        l.hangup_rect(),
        "Hang up",
        ButtonKind::Danger,
        input.pressed == Some(Target::Hangup),
        true,
    );
}

fn ended(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    party_block(c, ty, l, model, "call ended");
    let can_redial = model.focused().is_some_and(|v| v.number.is_some());
    button(
        c,
        ty,
        l.again_rect(),
        "Call again",
        ButtonKind::Primary,
        input.pressed == Some(Target::Again),
        can_redial,
    );
    button(
        c,
        ty,
        l.done_rect(),
        "Done",
        ButtonKind::Secondary,
        input.pressed == Some(Target::Done),
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preview_state;
    use ferrix_telephony_api::CallEndReason;
    use ferrix_telephony_protocol::{StateView, Update};

    fn model(call: Option<(Direction, CallState, Option<CallEndReason>)>) -> Model {
        let mut m = Model::new();
        m.apply(Update::Snapshot(StateView::Full(preview_state(call))));
        m
    }

    #[test]
    fn every_page_renders_with_its_accent() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let input = Input::default();
        let l = Layout::new((1080, 2340));
        let dialer = compose(&ty, (1080, 2340), 600, &model(None), &input);
        assert_eq!((dialer.width(), dialer.height()), (1080, 2340));
        assert_eq!(dialer.get(540, 10), t.bg_dark);
        assert_eq!(dialer.get(540, 230), t.rust_dim, "header rule");
        let call = l.call_rect();
        assert_eq!(
            dialer.get(call.x + call.w / 2, call.y),
            t.border,
            "Call disabled while empty"
        );
        let mut m = model(None);
        for ch in "5550100042".chars() {
            m.key(ch);
        }
        let ready = compose(&ty, (1080, 2340), 600, &m, &input);
        assert_eq!(
            ready.get(call.x + call.w / 2, call.y),
            t.rust,
            "Call enabled takes the accent"
        );
        let (one, _) = l.dial_keys()[0];
        assert_eq!(ready.get(one.x + 6, one.y + 6), t.surface);

        let mut m = model(None);
        m.page = Page::Recent;
        let recent = compose(&ty, (1080, 2340), 600, &m, &input);
        let row = l.recent_rects(3)[0];
        assert_eq!(recent.get(row.x + 8, row.y + 8), t.surface);

        let incoming = compose(
            &ty,
            (1080, 2340),
            600,
            &model(Some((Direction::Incoming, CallState::Ringing, None))),
            &input,
        );
        let a = l.answer_rect();
        assert_eq!(incoming.get(a.x + a.w / 2, a.y), t.rust);
        let r = l.reject_rect();
        assert_eq!(incoming.get(r.x + r.w / 2, r.y), t.err);

        let mut active = model(Some((Direction::Outgoing, CallState::Active, None)));
        active.dtmf_open = true;
        let in_call = compose(&ty, (1080, 2340), 600, &active, &input);
        let (ear, _) = l.route_rects()[0];
        assert_eq!(
            in_call.get(ear.x + ear.w / 2, ear.y),
            t.rust,
            "active route chip"
        );
        let (spk, _) = l.route_rects()[1];
        assert_eq!(in_call.get(spk.x + spk.w / 2, spk.y), t.border);
        let h = l.hangup_rect();
        assert_eq!(in_call.get(h.x + h.w / 2, h.y), t.err);
        let (k1, _) = l.dtmf_keys()[0];
        assert_eq!(in_call.get(k1.x + 6, k1.y + 6), t.surface);

        let mut ended_m = model(Some((
            Direction::Outgoing,
            CallState::Ended,
            Some(CallEndReason::Busy),
        )));
        ended_m.focused_call = Some(ferrix_telephony_api::CallId("call-live".into()));
        ended_m.page = Page::Ended;
        let ended = compose(&ty, (1080, 2340), 600, &ended_m, &input);
        let again = l.again_rect();
        assert_eq!(ended.get(again.x + again.w / 2, again.y), t.rust);
        assert_eq!(
            call_state_chip(CallState::Ended, Some(&CallEndReason::ModemLost)),
            ("modem-lost", Tone::Err)
        );
        assert_eq!(call_state_chip(CallState::Held, None), ("held", Tone::Warn));
    }

    #[test]
    fn unavailable_link_shows_a_warning_notice_and_stays_usable() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let mut m = model(None);
        m.apply(Update::Disconnected);
        let c = compose(&ty, (1080, 2340), 600, &m, &Input::default());
        let l = Layout::new((1080, 2340));
        let e = l.error_rect();
        assert_eq!(c.get(e.x + 22, e.y + e.h / 2), t.warn);
        assert!(status_label(&m).contains("ready"));
        assert!(status_label(&m).ends_with("mock"));
    }

    #[test]
    fn route_chips_fit_their_labels_without_ellipsis() {
        let ty = Type::load().unwrap();
        for size in [(1080, 2340), (720, 1600), (1080, 2424)] {
            let l = Layout::new(size);
            for (r, route) in l.route_rects() {
                // Matches `widgets::chip`: bold at 42% of the chip height,
                // ellipsized beyond the width less 24 scaled px.
                let style = Style::bold((r.h as f32 * 0.42).max(12.0), theme::get().fg);
                for available in [true, false] {
                    let label = route_label(route, available);
                    assert!(
                        ty.width(style, &label) <= r.w - l.s(24),
                        "{label:?} overflows {r:?} at {size:?}"
                    );
                    assert_eq!(widgets::ellipsize(&ty, style, &label, r.w - l.s(24)), label);
                }
            }
            let rects = l.route_rects();
            let hold = l.hold_rect();
            assert!(
                rects.iter().all(|(r, _)| r.y + r.h < hold.y),
                "chips clear Hold"
            );
            let first_key = l.dtmf_keys()[0].0;
            assert!(hold.y + hold.h < first_key.y);
            let last_key = l.dtmf_keys()[11].0;
            assert!(last_key.y + last_key.h < l.hangup_rect().y);
        }
    }

    #[test]
    fn the_number_appears_on_one_line_only() {
        let m = model(Some((Direction::Incoming, CallState::Ringing, None)));
        let view = m.focused().unwrap();
        let name = view.party_label(&[], "US");
        assert_eq!(name, "+15550100042");
        assert_eq!(party_subtitle(view, &name), None, "no redacted copy");
        assert_eq!(
            party_subtitle(view, "Test Contact 42").as_deref(),
            Some("+15550100042")
        );
        let mut r = Model::new();
        r.apply(Update::Snapshot(StateView::Redacted(
            preview_state(Some((Direction::Incoming, CallState::Ringing, None))).redacted(),
        )));
        let view = r.focused().unwrap();
        let name = view.party_label(&[], "US");
        assert_eq!(party_subtitle(view, &name), None);
        assert_eq!(
            party_subtitle(view, "Test Contact 42").as_deref(),
            Some("+***0042")
        );
    }
}
