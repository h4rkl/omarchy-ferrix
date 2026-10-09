//! Composition of the Contacts pages: searchable list, detail with labelled
//! numbers, edit form over the shared keyboard, delete confirmation. One
//! rust accent per region (the focused field, the primary button, the
//! pressed row); `err` only on Delete.

use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    ButtonKind, Tone, button, chip, clock_strip, gesture_pill, keyboard, list_row, notice,
    page_header, section_label, text_field, wrap_text,
};

use crate::interaction::{Input, Layout, Target, keyboard_state, keyboard_visible};
use crate::model::{Focus, Link, Model, Page};

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
    clock_strip(
        &mut c,
        ty,
        &clock,
        Some(&format!("{} contacts", model.contacts.len())),
    );
    let l = Layout::new(size);
    let back = input.pressed == Some(Target::Back);
    let title = match model.page {
        Page::List => "Contacts".to_owned(),
        Page::Detail | Page::ConfirmDelete => model
            .selected()
            .map_or_else(|| "Contact".to_owned(), |c| c.display_name.clone()),
        Page::Edit => if model.form.editing.is_some() {
            "Edit contact"
        } else {
            "New contact"
        }
        .to_owned(),
    };
    page_header(&mut c, ty, &title, Some(&model.link_label()), back);
    match model.page {
        Page::List => list(&mut c, ty, &l, model, input),
        Page::Detail => detail(&mut c, ty, &l, model, input),
        Page::Edit => edit(&mut c, ty, &l, model, input),
        Page::ConfirmDelete => confirm(&mut c, ty, &l, model, input),
    }
    if keyboard_visible(model) {
        let pressed = match input.pressed {
            Some(Target::Key(k)) => Some(k),
            _ => None,
        };
        keyboard(&mut c, ty, &keyboard_state(model), pressed);
    }
    gesture_pill(&mut c, input.pressed == Some(Target::Home));
    c
}

fn list(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    text_field(
        c,
        ty,
        l.search_rect(),
        &model.query,
        "Search name or digits",
        model.focus == Some(Focus::Search),
    );
    let contacts = model.filtered();
    let keyboard = keyboard_visible(model);
    if contacts.is_empty() {
        let style = Style::bold(l.s(40) as f32, t.fg);
        let text = if model.contacts.is_empty() {
            "No contacts"
        } else {
            "No matches"
        };
        ty.text(
            c,
            style,
            text,
            (c.width() - ty.width(style, text)) / 2,
            l.s(760),
        );
        let detail = match model.link {
            Link::Unavailable => "contacts \u{b7} unavailable",
            Link::Connecting => "contacts \u{b7} connecting",
            _ => "the store is empty",
        };
        let ds = Style::regular(l.s(28) as f32, t.fg_dim);
        ty.text(
            c,
            ds,
            detail,
            (c.width() - ty.width(ds, detail)) / 2,
            l.s(830),
        );
    }
    for (index, (contact, r)) in contacts
        .iter()
        .zip(l.row_rects(contacts.len(), keyboard))
        .enumerate()
    {
        let (sub, trailing) = match contact.numbers.first() {
            Some(n) => (
                n.display().to_owned(),
                Some((n.label.name(), Tone::Neutral)),
            ),
            None => ("no numbers".to_owned(), None),
        };
        list_row(
            c,
            ty,
            r,
            &contact.display_name,
            &sub,
            trailing,
            input.pressed == Some(Target::Row(index)),
        );
    }
    if !keyboard {
        button(
            c,
            ty,
            l.new_rect(),
            "New",
            ButtonKind::Primary,
            input.pressed == Some(Target::New),
            model.writable(),
        );
        button(
            c,
            ty,
            l.export_rect(),
            "Export vCard",
            ButtonKind::Secondary,
            input.pressed == Some(Target::Export),
            model.writable(),
        );
        let status_rect = Rect {
            x: l.s(48),
            y: l.new_rect().y - l.s(130),
            w: c.width() - l.s(96),
            h: l.s(96),
        };
        if let Some(error) = &model.last_error {
            notice(c, ty, status_rect, error, Tone::Err);
        } else if model.link == Link::Unavailable {
            notice(c, ty, status_rect, &model.link_label(), Tone::Warn);
        } else if let Some(status) = &model.export_status {
            notice(c, ty, status_rect, status, Tone::Ok);
        }
    }
}

fn detail(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let Some(contact) = model.selected() else {
        let style = Style::bold(s(40) as f32, t.fg);
        ty.text(c, style, "No contact selected", s(96), s(700));
        return;
    };
    section_label(c, ty, "name", s(96), s(360));
    let mut name_style = Style::bold(s(64) as f32, t.fg);
    while ty.width(name_style, &contact.display_name) > c.width() - s(192) && name_style.px > 24.0 {
        name_style.px -= 4.0;
    }
    ty.text(c, name_style, &contact.display_name, s(96), s(470));
    let given = contact.given.as_deref().unwrap_or("\u{2014}");
    let family = contact.family.as_deref().unwrap_or("\u{2014}");
    ty.text(
        c,
        Style::regular(s(28) as f32, t.fg_dim),
        &format!("given {given} \u{b7} family {family}"),
        s(96),
        s(540),
    );
    section_label(c, ty, "numbers", s(96), s(660));
    if contact.numbers.is_empty() {
        ty.text(
            c,
            Style::regular(s(30) as f32, t.fg_dim),
            "no numbers",
            s(96),
            s(790),
        );
    }
    for (n, r) in contact
        .numbers
        .iter()
        .zip(l.detail_number_rects(contact.numbers.len()))
    {
        let sub = if n.e164.is_some() {
            "normalized \u{b7} E.164"
        } else {
            "raw \u{b7} not normalized"
        };
        list_row(
            c,
            ty,
            r,
            n.display(),
            sub,
            Some((n.label.name(), Tone::Steel)),
            false,
        );
    }
    wrap_text(
        c,
        ty,
        "Calls and messages start from the Phone and Messages apps; this milestone keeps Contacts radio-independent.",
        s(96),
        s(1560),
        c.width() - s(192),
        s(26),
    );
    button(
        c,
        ty,
        l.edit_rect(),
        "Edit",
        ButtonKind::Secondary,
        input.pressed == Some(Target::Edit),
        model.writable(),
    );
    button(
        c,
        ty,
        l.delete_rect(),
        "Delete",
        ButtonKind::Danger,
        input.pressed == Some(Target::Delete),
        model.writable(),
    );
    ty.text(
        c,
        Style::regular(s(26) as f32, t.fg_dim),
        &format!("id {}", contact.id),
        s(96),
        s(1920),
    );
}

fn edit(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    text_field(
        c,
        ty,
        l.name_rect(),
        &model.form.name,
        "Display name",
        model.focus == Some(Focus::Name),
    );
    for (i, ((label_r, field_r), (label, raw))) in l
        .number_rows(model.form.numbers.len())
        .into_iter()
        .zip(&model.form.numbers)
        .enumerate()
    {
        chip(
            c,
            ty,
            label_r,
            label.name(),
            Tone::Steel,
            input.pressed == Some(Target::FormLabel(i)),
        );
        text_field(
            c,
            ty,
            field_r,
            raw,
            "Number",
            model.focus == Some(Focus::Number(i)),
        );
    }
    if model.form.numbers.len() < crate::model::FORM_NUMBERS {
        chip(
            c,
            ty,
            l.add_number_rect(model.form.numbers.len()),
            "+ add number",
            Tone::Neutral,
            input.pressed == Some(Target::AddNumber),
        );
    }
    let can_save = model.can_save();
    if let Some(error) = &model.last_error {
        notice(c, ty, l.error_rect(), error, Tone::Err);
    } else if let Err(reason) = &can_save {
        let style = Style::regular(s(26) as f32, t.fg_dim);
        let e = l.error_rect();
        ty.text(c, style, reason, e.x + s(24), e.y + e.h / 2 + s(10));
    }
    button(
        c,
        ty,
        l.save_rect(),
        "Save",
        ButtonKind::Primary,
        input.pressed == Some(Target::Save),
        can_save.is_ok(),
    );
}

fn confirm(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let name = model
        .selected()
        .map_or_else(|| "this contact".to_owned(), |c| c.display_name.clone());
    let style = Style::bold(s(44) as f32, t.fg);
    ty.text(c, style, "Delete contact?", s(96), s(560));
    wrap_text(
        c,
        ty,
        &format!("{name} and its numbers are removed from the store. This cannot be undone."),
        s(96),
        s(660),
        c.width() - s(192),
        s(30),
    );
    button(
        c,
        ty,
        l.confirm_rect(),
        "Delete",
        ButtonKind::Danger,
        input.pressed == Some(Target::ConfirmDelete),
        true,
    );
    button(
        c,
        ty,
        l.cancel_rect(),
        "Cancel",
        ButtonKind::Secondary,
        input.pressed == Some(Target::Cancel),
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preview_contacts;
    use ferrix_contacts_protocol::Link as ProtoLink;

    fn model() -> Model {
        let mut m = Model::new();
        m.apply(ProtoLink::Connected {
            contacts: preview_contacts(),
        });
        m
    }

    #[test]
    fn pages_render_with_accents_where_the_state_is() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let input = Input::default();
        let l = Layout::new((1080, 2340));
        let m = model();
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let row = l.row_rects(8, false)[0];
        assert_eq!(c.get(row.x + 8, row.y + 8), t.surface);
        let n = l.new_rect();
        assert_eq!(c.get(n.x + n.w / 2, n.y), t.rust);
        let sr = l.search_rect();
        assert_eq!(c.get(sr.x + sr.w / 2, sr.y), t.border, "search unfocused");

        let mut m = model();
        let id = m.contacts[0].id.clone();
        m.open(&id);
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let d = l.delete_rect();
        assert_eq!(c.get(d.x + d.w / 2, d.y), t.err);
        let nr = l.detail_number_rects(1)[0];
        assert_eq!(c.get(nr.x + 8, nr.y + 8), t.surface);

        m.new_contact();
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let name = l.name_rect();
        assert_eq!(c.get(name.x + name.w / 2, name.y), t.rust, "name focused");
        let save = l.save_rect();
        assert_eq!(
            c.get(save.x + save.w / 2, save.y),
            t.border,
            "Save disabled while empty"
        );
        let (kb_top, _) = ferrix_ui::osk::extent((1080, 2340));
        assert_eq!(c.get(540, kb_top + 2), t.bg_dark, "keyboard tray");
        m.form.name = "x".to_owned();
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        assert_eq!(c.get(save.x + save.w / 2, save.y), t.rust);

        m.page = Page::ConfirmDelete;
        m.selected = Some(id);
        let c = compose(&ty, (1080, 2340), 600, &m, &input);
        let cf = l.confirm_rect();
        assert_eq!(c.get(cf.x + cf.w / 2, cf.y), t.err);
    }
}
