//! Settings > Keyboard: choose the shared on-screen keyboard's letter
//! layout and try it in a scratch field. The choice is written to the user's
//! `keyboard.toml` (see `ferrix_ui::keyboard`), which every app's keyboard
//! reads on its next frame; the repeat timings in that file are shown, not
//! edited here. Model, geometry, hit test, and drawing for the page live
//! together so the page stays one self-contained unit.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::draw::{Canvas, Rect, Type};
use ferrix_ui::icons::Icon;
use ferrix_ui::keyboard::{self as kbd, KeyboardConfig, LetterLayout};
use ferrix_ui::osk::{self, Key, OskState};
use ferrix_ui::widgets::{
    Edit, Tone, Trailing, contains, edit_text, list_rects, list_row_with, section_label,
    text_field, wrap_text,
};

use crate::interaction::{Layout, Target};
use crate::model::{Action, Model, Page};

/// What the Keyboard page shows: the built-in layouts, in file order.
pub fn layouts() -> &'static [LetterLayout] {
    kbd::layouts()
}

/// `q w e r t z u i o p ü`: the first letter row, as a recognisable preview.
pub fn preview_row(layout: &LetterLayout) -> String {
    let row: Vec<String> = layout.rows[0].iter().map(char::to_string).collect();
    row.join(" ")
}

/// `repeat · 400 ms delay · 20 per second`.
pub fn repeat_caption(config: &KeyboardConfig) -> String {
    let interval = config.repeat.interval.as_millis().max(1);
    format!(
        "Held backspace, arrows, and space repeat after {} ms, {} times a second. \
         Timings are set in keyboard.toml.",
        config.repeat.delay.as_millis(),
        1000 / interval
    )
}

impl Model {
    /// Root-page summary: the active layout's name.
    pub fn keyboard_summary(&self) -> String {
        kbd::layout(&self.kbd_layout).map_or_else(|| self.kbd_layout.clone(), |l| l.name.clone())
    }

    pub fn keyboard_state_name(&self) -> String {
        if self.kbd_try_focused {
            format!("layout-{}-trying", self.kbd_layout)
        } else {
            format!("layout-{}", self.kbd_layout)
        }
    }

    /// Chooses a layout: shown at once, persisted by the session owner.
    pub fn select_keyboard_layout(&mut self, index: usize) {
        let Some(layout) = layouts().get(index) else {
            return;
        };
        if layout.id != self.kbd_layout {
            self.kbd_layout = layout.id.clone();
            self.pending
                .push(Action::SetKeyboardLayout(layout.id.clone()));
        }
        self.last_error = None;
    }

    pub fn focus_keyboard_try(&mut self) {
        self.kbd_try_focused = !self.kbd_try_focused;
        self.osk = OskState::new(osk::Layout::Text);
    }

    /// Back while trying the layout closes the keyboard first.
    pub fn keyboard_back(&mut self) -> bool {
        if self.page == Page::Keyboard && self.kbd_try_focused {
            self.kbd_try_focused = false;
            return true;
        }
        false
    }

    /// Feeds a key into the scratch field. Returns `false` when the field is
    /// not focused.
    pub fn keyboard_key(&mut self, key: Key) -> bool {
        if self.page != Page::Keyboard || !self.kbd_try_focused {
            return false;
        }
        let mut state = self.osk;
        let outcome = edit_text(&mut self.kbd_try, key, &mut state);
        self.osk = state;
        if matches!(outcome, Edit::Submit | Edit::Dismiss) {
            self.kbd_try_focused = false;
        }
        true
    }
}

impl Layout {
    /// One row per built-in layout.
    pub fn keyboard_layout_rects(&self) -> Vec<Rect> {
        list_rects(self.size, self.s(330), layouts().len(), 140)
    }

    fn keyboard_rows_bottom(&self) -> i32 {
        self.keyboard_layout_rects()
            .last()
            .map_or(self.s(330), |r| r.y + r.h)
    }

    /// The scratch field below the repeat caption.
    pub fn keyboard_try_rect(&self) -> Rect {
        let s = |v| self.s(v);
        Rect {
            x: s(48),
            y: self.keyboard_rows_bottom() + s(250),
            w: self.size.0 as i32 - s(96),
            h: s(100),
        }
    }
}

pub fn hit(l: &Layout, position: (f64, f64)) -> Option<Target> {
    if contains(l.keyboard_try_rect(), position) {
        return Some(Target::KbdTry);
    }
    l.keyboard_layout_rects()
        .into_iter()
        .position(|r| contains(r, position))
        .map(Target::KbdLayout)
}

/// Layout rows (the active one marked `active` in rust: the page's one
/// accent), the repeat caption, and the scratch field.
pub fn draw(c: &mut Canvas, ty: &Type, l: &Layout, model: &Model, pressed: Option<Target>) {
    let s = |v| l.s(v);
    section_label(c, ty, "Layout", s(64), s(300));
    for (i, (r, layout)) in l
        .keyboard_layout_rects()
        .into_iter()
        .zip(layouts())
        .enumerate()
    {
        let active = layout.id == model.kbd_layout;
        let t = ferrix_ui::theme::get();
        // The filled keyboard glyph follows the row's state: rust when active.
        list_row_with(
            c,
            ty,
            r,
            Some((Icon::Keyboard, if active { t.rust } else { t.fg_dim })),
            &layout.name,
            &preview_row(layout),
            if active {
                Trailing::Chip("active", Tone::Accent)
            } else {
                Trailing::None
            },
            pressed == Some(Target::KbdLayout(i)),
        );
    }
    let config = KeyboardConfig {
        layout: model.kbd_layout.clone(),
        repeat: kbd::active().repeat,
    };
    wrap_text(
        c,
        ty,
        &repeat_caption(&config),
        s(64),
        l.keyboard_rows_bottom() + s(70),
        c.width() - s(128),
        s(27),
    );
    let field = l.keyboard_try_rect();
    section_label(c, ty, "Try it", s(64), field.y - s(24));
    text_field(
        c,
        ty,
        field,
        &model.kbd_try,
        "Tap to type with this layout",
        model.kbd_try_focused,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::{Input, hit_test, keyboard_visible};
    use ferrix_ui::widgets::center;

    const SIZE: (u32, u32) = (1080, 2340);

    fn tap(input: &mut Input, model: &mut Model, at: (f64, f64)) {
        input.down(0, at, model, SIZE);
        input.up(0, None, model, SIZE);
    }

    #[test]
    fn choosing_a_layout_queues_it_and_the_try_field_types_with_it() {
        let mut m = Model::new();
        assert_eq!(m.kbd_layout, "us");
        let mut input = Input::default();
        let l = Layout::new(SIZE);
        let row = Page::SECTIONS
            .iter()
            .position(|p| *p == Page::Keyboard)
            .expect("listed on the root page");
        tap(&mut input, &mut m, center(l.section_rects()[row].0));
        assert_eq!(m.page, Page::Keyboard);
        assert_eq!(m.state_name(), "layout-us");
        let rows = l.keyboard_layout_rects();
        assert_eq!(rows.len(), layouts().len());
        let de = layouts().iter().position(|x| x.id == "de").unwrap();
        tap(&mut input, &mut m, center(rows[de]));
        assert_eq!(m.kbd_layout, "de");
        assert_eq!(m.pending, vec![Action::SetKeyboardLayout("de".into())]);
        assert_eq!(m.section_summary(Page::Keyboard), "Deutsch (QWERTZ)");
        m.pending.clear();
        tap(&mut input, &mut m, center(rows[de]));
        assert!(
            m.pending.is_empty(),
            "re-choosing the active layout is a no-op"
        );

        // The scratch field opens the keyboard; keys type into it.
        assert!(!keyboard_visible(&m));
        tap(&mut input, &mut m, center(l.keyboard_try_rect()));
        assert!(keyboard_visible(&m));
        assert_eq!(m.state_name(), "layout-de-trying");
        assert!(l.keyboard_try_rect().y + l.keyboard_try_rect().h < osk::extent(SIZE).0);
        let letters = kbd::layout("de").unwrap();
        for ch in ['\u{fc}', 'z'] {
            assert!(
                osk::keys_with(SIZE, &m.osk, letters)
                    .iter()
                    .any(|(_, k)| *k == Key::Char(ch))
            );
            assert!(m.keyboard_key(Key::Char(ch)));
        }
        assert_eq!(m.kbd_try, "\u{fc}z");
        // Back closes the keyboard, then leaves the page.
        m.back();
        assert!(!keyboard_visible(&m) && m.page == Page::Keyboard);
        m.back();
        assert_eq!(m.page, Page::Root);
        assert_eq!(hit_test(&m, (540.0, 2290.0), SIZE), Some(Target::Home));
    }

    #[test]
    fn page_renders_rows_caption_and_field() {
        let ty = Type::load().unwrap();
        let mut m = Model::new();
        m.show(Page::Keyboard);
        let input = Input::default();
        let c = crate::screen::compose(&ty, SIZE, 600, &m, &input);
        let t = ferrix_ui::theme::get();
        let l = Layout::new(SIZE);
        let us = layouts().iter().position(|x| x.id == "us").unwrap();
        let r = l.keyboard_layout_rects()[us];
        // The active row carries the rust `active` chip outline.
        let rust =
            (r.x + r.w / 2..r.x + r.w).any(|x| (r.y..r.y + r.h).any(|y| c.get(x, y) == t.rust));
        assert!(rust, "active chip");
        let f = l.keyboard_try_rect();
        assert_eq!(c.get(f.x + f.w / 2, f.y + f.h / 2), t.bg_dark);
        assert!(repeat_caption(&KeyboardConfig::default()).contains("400 ms"));
        assert_eq!(preview_row(kbd::us()), "q w e r t y u i o p");
    }
}
