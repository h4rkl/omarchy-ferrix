//! Composition of the Files pages: folder list with breadcrumb, read-only
//! viewer, entry details, folder/rename form over the shared keyboard, and
//! delete confirmation. One rust accent per region (the primary button, the
//! focused field, the pressed row); `err` only on Delete and on failures.

use ferrix_ui::draw::{Canvas, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    ButtonKind, Tone, button, chip, clock_strip, ellipsize, gesture_pill, hairline, keyboard,
    list_row, notice, page_header, section_label, text_field, wrap_text,
};

use crate::interaction::{Crumb, Input, Layout, Target, keyboard_visible, view_lines};
use crate::model::{Model, Notice, Page};
use crate::scroll;
use crate::store::{FsError, Kind, human_size};

/// Trailing affordance of a row: the entry's details.
const MORE: &str = "\u{b7}\u{b7}\u{b7}";

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
    clock_strip(&mut c, ty, &clock, Some(&model.count_label()));
    let l = Layout::new(size);
    let title = match model.page {
        Page::Browse => model
            .cwd
            .last()
            .cloned()
            .unwrap_or_else(|| "Files".to_owned()),
        Page::View | Page::Item | Page::ConfirmDelete => {
            model.selected.clone().unwrap_or_else(|| "Files".to_owned())
        }
        Page::NewFolder => "New folder".to_owned(),
        Page::Rename => "Rename".to_owned(),
    };
    let status = match model.page {
        Page::View => model
            .file
            .as_ref()
            .and_then(|f| f.as_ref().ok())
            .map(|f| human_size(f.size)),
        _ => None,
    };
    // The shared header does not shorten its title; long names would run
    // under the status label.
    let s = |v| l.s(v);
    let title_style = Style::bold(s(54) as f32, t.fg);
    let status_w = status.as_deref().map_or(0, |st| {
        ty.width(Style::regular(s(26) as f32, t.fg_dim), st) + s(40)
    });
    let title = ellipsize(
        ty,
        title_style,
        &title,
        c.width() - s(150) - s(48) - status_w,
    );
    page_header(
        &mut c,
        ty,
        &title,
        status.as_deref(),
        input.pressed == Some(Target::Back),
    );
    match model.page {
        Page::Browse => browse(&mut c, ty, &l, model, input),
        Page::View => view(&mut c, ty, &l, model),
        Page::Item => item(&mut c, ty, &l, model, input),
        Page::NewFolder | Page::Rename => form(&mut c, ty, &l, model, input),
        Page::ConfirmDelete => confirm(&mut c, ty, &l, model, input),
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

/// A centered two-line empty or failure state (`docs/design.md`: centered
/// alignment only for a singular readout or empty state).
fn empty_state(c: &mut Canvas, ty: &Type, l: &Layout, title: &str, detail: &str) {
    let t = theme::get();
    let style = Style::bold(l.s(40) as f32, t.fg);
    ty.text(
        c,
        style,
        title,
        (c.width() - ty.width(style, title)) / 2,
        l.s(760),
    );
    let ds = Style::regular(l.s(28) as f32, t.fg_dim);
    let detail = ellipsize(ty, ds, detail, c.width() - l.s(96));
    ty.text(
        c,
        ds,
        &detail,
        (c.width() - ty.width(ds, &detail)) / 2,
        l.s(830),
    );
}

fn crumbs(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let sep = Style::regular(l.s(30) as f32, t.fg_dim);
    let crumbs = l.crumb_rects(&model.crumbs());
    let last = crumbs.len().saturating_sub(1);
    for (i, (r, crumb, label)) in crumbs.iter().enumerate() {
        let tone = match crumb {
            // The current folder reads as the location, not an action.
            Crumb::Depth(_) if i == last => Tone::Steel,
            _ => Tone::Neutral,
        };
        let pressed = matches!(crumb, Crumb::Depth(d) if input.pressed == Some(Target::Crumb(*d)));
        chip(c, ty, *r, label, tone, pressed);
        if i < last {
            let x = r.x + r.w + (l.crumb_gap() - ty.width(sep, "/")) / 2;
            ty.text(c, sep, "/", x, r.y + r.h / 2 + l.s(10));
        }
    }
}

fn browse(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    crumbs(c, ty, l, model, input);
    let entries = model.entries();
    match &model.listing {
        Err(FsError::HomeUnavailable(reason)) => {
            empty_state(c, ty, l, "Home unavailable", reason);
        }
        Err(error) => empty_state(c, ty, l, "Folder unavailable", &error.to_string()),
        Ok(_) if entries.is_empty() => {
            let title = if model.cwd.is_empty() {
                "Home is empty"
            } else {
                "Folder is empty"
            };
            empty_state(c, ty, l, title, "New folder adds one here");
        }
        Ok(_) => {}
    }
    let capacity = l.capacity();
    let window = model.scroll.window(entries.len(), capacity);
    for (index, r) in window.clone().zip(l.row_rects(window.len())) {
        let entry = &entries[index];
        let pressed = input.pressed == Some(Target::Row(index))
            || input.pressed == Some(Target::RowMore(index));
        let tone = if input.pressed == Some(Target::RowMore(index)) {
            Tone::Accent
        } else {
            Tone::Neutral
        };
        let title = if entry.kind == Kind::Folder {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        };
        list_row(
            c,
            ty,
            r,
            &title,
            &model.subtitle(entry),
            Some((MORE, tone)),
            pressed,
        );
    }
    scroll::rail(c, l.rail_track(), entries.len(), capacity, window.start);
    if let Some(n) = model
        .listing
        .as_ref()
        .ok()
        .map(|l| l.truncated)
        .filter(|n| *n > 0)
    {
        let t = theme::get();
        let style = Style::regular(l.s(24) as f32, t.fg_dim);
        let text = format!("{n} more not listed");
        let y = l.notice_rect().y - l.s(4);
        ty.text(c, style, &text, l.s(48), y);
    }
    match &model.notice {
        Some(Notice::Err(text)) => notice(c, ty, l.notice_rect(), text, Tone::Err),
        Some(Notice::Ok(text)) => notice(c, ty, l.notice_rect(), text, Tone::Ok),
        None => {}
    }
    button(
        c,
        ty,
        l.new_folder_rect(),
        "New folder",
        ButtonKind::Primary,
        input.pressed == Some(Target::NewFolder),
        model.writable(),
    );
    button(
        c,
        ty,
        l.refresh_rect(),
        "Refresh",
        ButtonKind::Secondary,
        input.pressed == Some(Target::Refresh),
        true,
    );
}

fn view(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model) {
    let t = theme::get();
    let s = |v| l.s(v);
    let Some(Ok(file)) = &model.file else {
        empty_state(c, ty, l, "File unavailable", "not found \u{b7} refresh");
        return;
    };
    let meta = format!(
        "read-only \u{b7} {} \u{b7} {}",
        human_size(file.size),
        model.date(file.modified)
    );
    ty.text(
        c,
        Style::regular(s(26) as f32, t.fg_dim),
        &meta,
        s(48),
        s(300),
    );
    let area = l.view_area();
    c.fill_rounded(area, s(10), t.bg_dark);
    c.stroke_rounded(area, s(10), hairline(l.b), t.border);
    if file.text.is_none() {
        empty_state(c, ty, l, "Not a text file", "binary content is not shown");
        return;
    }
    let lines = view_lines(model, l);
    if lines.is_empty() {
        empty_state(c, ty, l, "File is empty", "0 bytes");
        return;
    }
    let rows = l.view_rows();
    let window = model.view_scroll.window(lines.len(), rows);
    let style = Style::regular(l.view_px() as f32, t.fg);
    let x = area.x + l.view_pad();
    let mut baseline = area.y + l.view_pad() + l.view_line() - s(10);
    for line in &lines[window.clone()] {
        let mut pen = x as f32;
        for ch in line.chars() {
            pen += ty.char(c, style, ch, pen.round() as i32, baseline);
        }
        baseline += l.view_line();
    }
    if file.truncated && window.end == lines.len() {
        let note = format!(
            "first {} shown",
            human_size(crate::store::VIEW_LIMIT as u64)
        );
        let ns = Style::regular(s(24) as f32, t.warn);
        let y = (baseline + s(4)).min(area.y + area.h - s(12));
        ty.text(c, ns, &note, x, y);
    }
    scroll::rail(c, l.view_rail(), lines.len(), rows, window.start);
}

fn item(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let Some(entry) = model.selected_entry() else {
        empty_state(c, ty, l, "Not found", "the entry was moved or deleted");
        return;
    };
    let fields = [
        ("kind", Model::kind_name(entry).to_owned()),
        (
            "size",
            match entry.kind {
                Kind::File => human_size(entry.size),
                Kind::Folder => Model::items_label(entry),
                _ => "\u{2013}".to_owned(),
            },
        ),
        ("modified", model.date(entry.modified)),
        ("location", model.display_path(None)),
    ];
    let value = Style::regular(s(34) as f32, t.fg);
    let mut y = s(340);
    for (label, text) in fields {
        section_label(c, ty, label, s(96), y);
        let text = ellipsize(ty, value, &text, c.width() - s(192));
        ty.text(c, value, &text, s(96), y + s(58));
        y += s(150);
    }
    match &model.notice {
        Some(Notice::Err(text)) => notice(c, ty, l.item_notice_rect(), text, Tone::Err),
        Some(Notice::Ok(text)) => notice(c, ty, l.item_notice_rect(), text, Tone::Ok),
        None => {}
    }
    let openable = matches!(entry.kind, Kind::Folder | Kind::File);
    button(
        c,
        ty,
        l.open_rect(),
        if entry.kind == Kind::Folder {
            "Open folder"
        } else {
            "Open read-only"
        },
        ButtonKind::Primary,
        input.pressed == Some(Target::Open),
        openable,
    );
    button(
        c,
        ty,
        l.rename_rect(),
        "Rename",
        ButtonKind::Secondary,
        input.pressed == Some(Target::Rename),
        true,
    );
    button(
        c,
        ty,
        l.delete_rect(),
        "Delete",
        ButtonKind::Danger,
        input.pressed == Some(Target::Delete),
        true,
    );
}

fn form(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    section_label(c, ty, "name", s(48), s(330));
    let placeholder = if model.page == Page::NewFolder {
        "Folder name"
    } else {
        "New name"
    };
    text_field(c, ty, l.field_rect(), &model.form, placeholder, true);
    let location = format!("in {}", model.display_path(None));
    let style = Style::regular(s(26) as f32, t.fg_dim);
    let location = ellipsize(ty, style, &location, c.width() - s(96));
    ty.text(c, style, &location, s(48), s(510));
    let e = l.form_notice_rect();
    if let Some(Notice::Err(text)) = &model.notice {
        notice(c, ty, e, text, Tone::Err);
    } else if let Some(hint) = model.form_hint() {
        ty.text(c, style, &hint, e.x + s(24), e.y + e.h / 2 + s(10));
    }
    let label = if model.page == Page::NewFolder {
        "Create folder"
    } else {
        "Rename"
    };
    button(
        c,
        ty,
        l.save_rect(),
        label,
        ButtonKind::Primary,
        input.pressed == Some(Target::Save),
        model.can_submit(),
    );
}

fn confirm(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, input: &Input) {
    let t = theme::get();
    let s = |v| l.s(v);
    let name = model.selected.clone().unwrap_or_default();
    let folder = model
        .selected_entry()
        .is_some_and(|e| e.kind == Kind::Folder && !e.link);
    let heading = if folder {
        "Delete folder?"
    } else {
        "Delete file?"
    };
    ty.text(c, Style::bold(s(44) as f32, t.fg), heading, s(96), s(560));
    let place = model.display_path(None);
    let body = match (folder, model.delete_items) {
        (true, Some(0)) | (true, None) => {
            format!("{name} is removed from {place}. This cannot be undone.")
        }
        (true, Some(1)) => {
            format!(
                "{name} and the 1 item inside it are removed from {place}. This cannot be undone."
            )
        }
        (true, Some(n)) => format!(
            "{name} and the {n} items inside it are removed from {place}. This cannot be undone."
        ),
        (false, _) => format!("{name} is removed from {place}. This cannot be undone."),
    };
    wrap_text(c, ty, &body, s(96), s(660), c.width() - s(192), s(30));
    if let Some(Notice::Err(text)) = &model.notice {
        notice(c, ty, l.item_notice_rect(), text, Tone::Err);
    }
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

/// Whether `r` is untouched background (renderer tests).
#[cfg(test)]
fn blank(c: &Canvas, r: ferrix_ui::draw::Rect) -> bool {
    let bg = theme::get().bg;
    (r.y..r.y + r.h).all(|y| (r.x..r.x + r.w).all(|x| c.get(x, y) == bg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Home;
    use crate::store::testdir::TestDir;
    use ferrix_ui::draw::Rect;
    use jiff::tz::TimeZone;
    use std::fs;

    const SIZE: (u32, u32) = (1080, 2340);

    fn model(files: usize) -> (TestDir, Model) {
        let dir = TestDir::new();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        for i in 0..files {
            fs::write(home.join(format!("f{i:02}.txt")), "text\n").unwrap();
        }
        let m = Model::new(Home::new(&home), TimeZone::UTC);
        (dir, m)
    }

    #[test]
    fn the_more_glyph_and_viewer_face_are_monospace_and_present() {
        let ty = Type::load().unwrap();
        assert!(ty.has_glyphs(ferrix_ui::draw::Weight::Bold, MORE));
        assert!(ty.has_glyphs(ferrix_ui::draw::Weight::Bold, "\u{2026}"));
        let l = Layout::new(SIZE);
        let style = Style::regular(l.view_px() as f32, 0);
        let adv = ty.advance(style, 'M');
        assert!(
            (adv - l.view_px() as f32 * 0.6).abs() < 0.5,
            "advance {adv}"
        );
    }

    #[test]
    fn pages_render_with_accents_where_the_state_is() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let input = Input::default();
        let l = Layout::new(SIZE);
        let (_d, mut m) = model(20);
        let c = compose(&ty, SIZE, 600, &m, &input);
        let row = l.row_rects(1)[0];
        assert_eq!(c.get(row.x + 8, row.y + 8), t.surface);
        let n = l.new_folder_rect();
        assert_eq!(
            c.get(n.x + n.w / 2, n.y),
            t.rust,
            "New folder is the accent"
        );
        let track = l.rail_track();
        assert_eq!(
            c.get(track.x + 1, track.y + 2),
            t.fg_dim,
            "thumb at the top"
        );
        assert_eq!(c.get(track.x + 1, track.y + track.h - 4), t.border);
        assert!(blank(&c, l.notice_rect()), "no notice before an action");

        m.inspect(1);
        let c = compose(&ty, SIZE, 600, &m, &input);
        let d = l.delete_rect();
        assert_eq!(c.get(d.x + d.w / 2, d.y), t.err);
        let o = l.open_rect();
        assert_eq!(c.get(o.x + o.w / 2, o.y), t.rust);

        m.page = Page::Browse;
        m.new_folder();
        let c = compose(&ty, SIZE, 600, &m, &input);
        let f = l.field_rect();
        assert_eq!(c.get(f.x + f.w / 2, f.y), t.rust, "field focused");
        let save = l.save_rect();
        assert_eq!(
            c.get(save.x + save.w / 2, save.y),
            t.border,
            "disabled while empty"
        );
        assert!(
            blank(&c, l.form_notice_rect()),
            "no validation before an edit"
        );
        let (kb_top, _) = ferrix_ui::osk::extent(SIZE);
        assert_eq!(c.get(540, kb_top + 2), t.bg_dark, "keyboard tray");

        m.back();
        m.open_index(1);
        assert_eq!(m.page, Page::View);
        let c = compose(&ty, SIZE, 600, &m, &input);
        let a = l.view_area();
        assert_eq!(c.get(a.x + a.w / 2, a.y + a.h - 8), t.bg_dark);
    }

    #[test]
    fn empty_and_unavailable_states_are_explicit() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let input = Input::default();
        let l = Layout::new(SIZE);
        let (_d, mut m) = model(0);
        m.open_index(0);
        assert!(m.entries().is_empty());
        let c = compose(&ty, SIZE, 600, &m, &input);
        let band = Rect {
            x: 0,
            y: l.s(720),
            w: SIZE.0 as i32,
            h: l.s(120),
        };
        assert!(!blank(&c, band), "empty folder says so");
        let none = Model::new(Home::from_env(|_| None), TimeZone::UTC);
        let c = compose(&ty, SIZE, 600, &none, &input);
        assert!(!blank(&c, band));
        let n = l.new_folder_rect();
        assert_eq!(
            c.get(n.x + n.w / 2, n.y),
            t.border,
            "no create without home"
        );
    }
}
