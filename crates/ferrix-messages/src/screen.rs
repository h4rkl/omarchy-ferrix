//! Composition of the Messages pages: conversation rows, a thread of
//! bubbles with state chips, and the compose form over the shared keyboard.
//! One rust accent per region: the Send button, the focused field, the
//! pressed control. Status colors sit only on the chip they describe.

use ferrix_telephony_api::Direction;
use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    self, ButtonKind, Tone, button, chip, clock_strip, gesture_pill, keyboard, list_row, notice,
    page_header, text_field,
};

use crate::interaction::{Input, Layout, Target, keyboard_state};
use crate::model::{ChipTone, Field, Link, Model, Page};

fn tone(t: ChipTone) -> Tone {
    match t {
        ChipTone::Neutral => Tone::Neutral,
        ChipTone::Steel => Tone::Steel,
        ChipTone::Ok => Tone::Ok,
        ChipTone::Err => Tone::Err,
    }
}

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
    clock_strip(&mut c, ty, &clock, Some(&status_label(model)));
    let l = Layout::new(size);
    let back = input.pressed == Some(Target::Back);
    match model.page {
        Page::Conversations => {
            page_header(&mut c, ty, "Messages", Some(&model.link_label()), back);
            conversations(&mut c, ty, &l, model, input);
        }
        Page::Thread => {
            page_header(
                &mut c,
                ty,
                &model.thread_title(),
                Some(&model.link_label()),
                back,
            );
            thread(&mut c, ty, &l, model, input);
        }
        Page::Compose => {
            page_header(&mut c, ty, "New message", Some(&model.link_label()), back);
            compose_page(&mut c, ty, &l, model, input);
        }
    }
    gesture_pill(&mut c, input.pressed == Some(Target::Home));
    c
}

fn status_label(model: &Model) -> String {
    let Some(state) = &model.state else {
        return "telephony \u{b7} no snapshot".to_owned();
    };
    let source = if state
        .operator
        .as_ref()
        .is_some_and(|o| o.name.to_lowercase().contains("mock"))
    {
        " \u{b7} mock"
    } else {
        ""
    };
    format!(
        "{} \u{b7} sim {} \u{b7} {}{source}",
        state.modem.name(),
        state.sim.name(),
        state.registration.name()
    )
}

fn conversations(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let convs = model.conversations();
    if convs.is_empty() {
        let style = Style::bold(l.s(40) as f32, t.fg);
        let text = "No conversations";
        ty.text(
            c,
            style,
            text,
            (c.width() - ty.width(style, text)) / 2,
            l.s(700),
        );
        let detail = match model.link {
            Link::Unavailable => "telephony \u{b7} unavailable",
            Link::Connecting => "telephony \u{b7} connecting",
            _ => "no messages in the daemon's store",
        };
        let ds = Style::regular(l.s(28) as f32, t.fg_dim);
        ty.text(
            c,
            ds,
            detail,
            (c.width() - ty.width(ds, detail)) / 2,
            l.s(770),
        );
    }
    for (index, (conv, r)) in convs
        .iter()
        .zip(l.conversation_rects(convs.len()))
        .enumerate()
    {
        let trailing = if conv.unread > 0 {
            (format!("{} unread", conv.unread), Tone::Accent)
        } else {
            (conv.last_state.0.clone(), tone(conv.last_state.1))
        };
        list_row(
            c,
            ty,
            r,
            &conv.title,
            &conv.preview,
            Some((trailing.0.as_str(), trailing.1)),
            input.pressed == Some(Target::Conversation(index)),
        );
    }
    let enabled = model.link != Link::Unavailable;
    button(
        c,
        ty,
        l.new_rect(),
        "New message",
        ButtonKind::Primary,
        input.pressed == Some(Target::New),
        enabled,
    );
    if model.link == Link::Unavailable {
        let r = l.new_rect();
        notice(
            c,
            ty,
            Rect {
                y: r.y - l.s(140),
                h: l.s(100),
                ..r
            },
            &model.link_label(),
            Tone::Warn,
        );
    }
}

fn thread(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let msgs = model.thread_messages();
    let outgoing: Vec<bool> = msgs
        .iter()
        .map(|m| m.direction == Direction::Outgoing)
        .collect();
    let (first, rects) = l.bubble_rects(msgs.len(), &outgoing);
    for (i, r) in rects.into_iter().enumerate() {
        let m = msgs[first + i];
        let out = m.direction == Direction::Outgoing;
        let pressed = input.pressed == Some(Target::Bubble(first + i));
        c.fill_rounded(
            r,
            s(10),
            if pressed || out {
                t.surface_hi
            } else {
                t.surface
            },
        );
        c.stroke_rounded(
            r,
            s(10),
            widgets::hairline(l.b),
            if out { t.rust_dim } else { t.border },
        );
        let style = Style::regular(s(30) as f32, t.fg);
        let lines = widgets::wrap(ty, style, &m.body_text(), r.w - s(48));
        let mut baseline = r.y + s(46);
        for line in lines.iter().take(2) {
            let line = if lines.len() > 2 && baseline > r.y + s(46) {
                widgets::ellipsize(ty, style, &format!("{line} \u{2026}"), r.w - s(48))
            } else {
                line.clone()
            };
            ty.text(c, style, &line, r.x + s(24), baseline);
            baseline += s(40);
        }
        let (label, ct) = m.chip();
        let chip_w = ty.width(Style::bold(s(22) as f32, t.fg), &label) + s(32);
        let chip_r = Rect {
            x: if out { r.x + r.w - chip_w } else { r.x },
            y: r.y + r.h + s(8),
            w: chip_w,
            h: s(48),
        };
        chip(c, ty, chip_r, &label, tone(ct), false);
        if m.parts > 1 {
            let parts = format!("{} parts", m.parts);
            let pw = ty.width(Style::bold(s(22) as f32, t.fg), &parts) + s(32);
            let pr = Rect {
                x: if out {
                    chip_r.x - pw - s(12)
                } else {
                    chip_r.x + chip_w + s(12)
                },
                w: pw,
                ..chip_r
            };
            chip(c, ty, pr, &parts, Tone::Steel, false);
        }
        if m.can_retry() {
            let retry = "Retry";
            let rw = ty.width(Style::bold(s(22) as f32, t.fg), retry) + s(32);
            // Sits beyond the parts chip when both are present.
            let shift = if m.parts > 1 {
                ty.width(
                    Style::bold(s(22) as f32, t.fg),
                    &format!("{} parts", m.parts),
                ) + s(44)
            } else {
                0
            };
            let rr = Rect {
                x: if out {
                    chip_r.x - rw - s(12) - shift
                } else {
                    chip_r.x + chip_w + s(12) + shift
                },
                w: rw,
                ..chip_r
            };
            chip(c, ty, rr, retry, Tone::Accent, pressed);
        }
    }
    if let Some(error) = &model.last_error {
        let d = l.draft_rect();
        notice(
            c,
            ty,
            Rect {
                x: s(48),
                y: d.y - s(120),
                w: c.width() - s(96),
                h: s(96),
            },
            error,
            Tone::Err,
        );
    } else if model.link != Link::Connected {
        let d = l.draft_rect();
        let tone = if model.link == Link::Unavailable {
            Tone::Warn
        } else {
            Tone::Steel
        };
        notice(
            c,
            ty,
            Rect {
                x: s(48),
                y: d.y - s(120),
                w: c.width() - s(96),
                h: s(96),
            },
            &model.link_label(),
            tone,
        );
    }
    text_field(
        c,
        ty,
        l.draft_rect(),
        &model.draft,
        "Message",
        model.focus == Field::Body,
    );
    let to = model.thread_number().unwrap_or_default();
    button(
        c,
        ty,
        l.send_rect(),
        "Send",
        ButtonKind::Primary,
        input.pressed == Some(Target::Send),
        model.send_block(&to).is_ok(),
    );
    let pressed_key = match input.pressed {
        Some(Target::Key(k)) => Some(k),
        _ => None,
    };
    keyboard(c, ty, &keyboard_state(model), pressed_key);
}

fn compose_page(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    text_field(
        c,
        ty,
        l.to_rect(),
        &model.compose_to,
        "To \u{b7} number",
        model.focus == Field::To,
    );
    text_field(
        c,
        ty,
        l.body_rect(),
        &model.draft,
        "Message",
        model.focus == Field::Body,
    );
    let block = model.send_block(&model.compose_to);
    button(
        c,
        ty,
        l.compose_send_rect(),
        "Send",
        ButtonKind::Primary,
        input.pressed == Some(Target::Send),
        block.is_ok(),
    );
    let line = match (&model.last_error, block) {
        (Some(error), _) => (error.clone(), Tone::Err),
        (None, Err(b)) => (b.reason().to_owned(), Tone::Warn),
        (None, Ok(())) => (
            format!(
                "to {}",
                model.destination(&model.compose_to).unwrap_or_default()
            ),
            Tone::Neutral,
        ),
    };
    let style = Style::regular(
        l.s(28) as f32,
        if line.1 == Tone::Neutral {
            t.fg_dim
        } else {
            line.1.color()
        },
    );
    ty.text(c, style, &line.0, l.s(96), l.s(760));
    let pressed_key = match input.pressed {
        Some(Target::Key(k)) => Some(k),
        _ => None,
    };
    keyboard(c, ty, &keyboard_state(model), pressed_key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preview_state;
    use ferrix_telephony_protocol::{StateView, Update};

    fn model() -> Model {
        let mut m = Model::new();
        m.apply(Update::Snapshot(StateView::Full(preview_state())));
        m
    }

    #[test]
    fn pages_render_rows_bubbles_chips_and_keyboard() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let input = Input::default();
        let l = Layout::new((1080, 2340));
        let m = model();
        let convs = compose(&ty, (1080, 2340), 600, &m, &input);
        let row = l.conversation_rects(2)[0];
        assert_eq!(convs.get(row.x + 8, row.y + 8), t.surface);
        let nr = l.new_rect();
        assert_eq!(
            convs.get(nr.x + nr.w / 2, nr.y),
            t.rust,
            "New message carries the accent"
        );

        let mut m = model();
        m.open_thread("+15550100042");
        let th = compose(&ty, (1080, 2340), 600, &m, &input);
        let msgs = m.thread_messages();
        let outgoing: Vec<bool> = msgs
            .iter()
            .map(|x| x.direction == Direction::Outgoing)
            .collect();
        let (_, rects) = l.bubble_rects(msgs.len(), &outgoing);
        assert_eq!(
            th.get(rects[0].x + 12, rects[0].y + 12),
            t.surface,
            "incoming bubble"
        );
        assert_eq!(
            th.get(rects[1].x + 12, rects[1].y + 12),
            t.surface_hi,
            "outgoing bubble"
        );
        assert_eq!(
            th.get(rects[1].x + rects[1].w / 2, rects[1].y),
            t.rust_dim,
            "outgoing hairline"
        );
        let d = l.draft_rect();
        assert_eq!(th.get(d.x + d.w / 2, d.y), t.rust, "focused draft field");
        let send = l.send_rect();
        assert_eq!(
            th.get(send.x + send.w / 2, send.y),
            t.border,
            "Send disabled with an empty draft"
        );
        m.draft = "x".to_owned();
        let th = compose(&ty, (1080, 2340), 600, &m, &input);
        assert_eq!(th.get(send.x + send.w / 2, send.y), t.rust);
        let (kb_top, _) = ferrix_ui::osk::extent((1080, 2340));
        assert_eq!(th.get(540, kb_top + 2), t.bg_dark, "keyboard tray");

        let mut m = model();
        m.open_compose();
        let cp = compose(&ty, (1080, 2340), 600, &m, &input);
        let to = l.to_rect();
        assert_eq!(cp.get(to.x + to.w / 2, to.y), t.rust, "To focused first");
        let body = l.body_rect();
        assert_eq!(cp.get(body.x + body.w / 2, body.y), t.border);
        assert!(status_label(&m).contains("ready"));
    }
}
