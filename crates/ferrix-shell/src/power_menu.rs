//! The trusted power menu: a fullscreen surface the compositor draws itself.
//!
//! No client draws or sees it. While it is open the compositor routes every
//! pointer and touch contact here instead of to clients, so an application
//! can neither fake it nor tap through it. The actions go to `ferrix-core` as
//! the typed `HardwareCommand::PowerAction`; nothing here shells out.
//!
//! Visual system (`docs/design.md`): Ferrix black field, the title with one
//! `rust_dim` instrument rail, a factual caption, and three large touch
//! buttons in thumb reach at the bottom: Power off (the region's one rust
//! accent), Restart, and Cancel. A failure is a local `err` notice above the
//! buttons with the reason; the menu stays open so the user can retry or
//! cancel. Opaque by design: correctness needs no translucency, and no app
//! content shows behind a system decision.
//!
//! Pure drawing into a `ferrix_ui` canvas, so it builds and tests (and
//! renders a preview) on every host.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_hardware_api::PowerAction;
use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::theme;
use ferrix_ui::widgets::{
    ButtonKind, Tone, basis, button, contains, hairline, notice, scale, section_label,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuButton {
    PowerOff,
    Restart,
    Cancel,
}

impl MenuButton {
    pub fn label(self) -> &'static str {
        match self {
            MenuButton::PowerOff => "power-off",
            MenuButton::Restart => "restart",
            MenuButton::Cancel => "cancel",
        }
    }

    pub fn action(self) -> Option<PowerAction> {
        match self {
            MenuButton::PowerOff => Some(PowerAction::PowerOff),
            MenuButton::Restart => Some(PowerAction::Restart),
            MenuButton::Cancel => None,
        }
    }
}

/// What the menu shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuStatus {
    Choosing,
    /// The action was sent; waiting for the core (actions disabled).
    Working(PowerAction),
    /// The core accepted it; the system is going down.
    Accepted(PowerAction),
    /// The core refused or failed; the reason is shown.
    Failed(PowerAction, String),
}

impl MenuStatus {
    pub fn label(&self) -> &'static str {
        match self {
            MenuStatus::Choosing => "choosing",
            MenuStatus::Working(_) => "working",
            MenuStatus::Accepted(_) => "accepted",
            MenuStatus::Failed(..) => "failed",
        }
    }

    fn actions_enabled(&self) -> bool {
        matches!(self, MenuStatus::Choosing | MenuStatus::Failed(..))
    }
}

/// Geometry in output logical pixels (the canvas is the logical output).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuLayout {
    pub size: (u32, u32),
    pub power_off: Rect,
    pub restart: Rect,
    pub cancel: Rect,
    pub status: Rect,
}

impl MenuLayout {
    pub fn new(size: (u32, u32)) -> Self {
        let b = basis(size);
        let s = |v: i32| scale(v, b);
        let (w, h) = (size.0 as i32, size.1 as i32);
        // A column at most 900 design px wide, centred: portrait fills the
        // width, landscape keeps the buttons a thumb's width.
        let col_w = (w - s(192)).min(s(900)).max(1);
        let x = (w - col_w) / 2;
        let button_h = s(150);
        let gap = s(30);
        // Above the bottom gesture strip (3 %) and the home indicator.
        let bottom = h - s(150);
        let cancel = Rect {
            x,
            y: bottom - button_h,
            w: col_w,
            h: button_h,
        };
        let restart = Rect {
            y: cancel.y - gap - button_h,
            ..cancel
        };
        let power_off = Rect {
            y: restart.y - gap - button_h,
            ..cancel
        };
        let status = Rect {
            y: power_off.y - s(60) - s(120),
            h: s(120),
            ..cancel
        };
        Self {
            size,
            power_off,
            restart,
            cancel,
            status,
        }
    }

    pub fn hit(&self, position: (f64, f64)) -> Option<MenuButton> {
        [
            (self.power_off, MenuButton::PowerOff),
            (self.restart, MenuButton::Restart),
            (self.cancel, MenuButton::Cancel),
        ]
        .into_iter()
        .find(|(r, _)| contains(*r, position))
        .map(|(_, b)| b)
    }

    /// The button a contact may act on in this status: `None` while the
    /// actions are disabled (Cancel stays available unless accepted).
    pub fn target(&self, position: (f64, f64), status: &MenuStatus) -> Option<MenuButton> {
        let hit = self.hit(position)?;
        match (hit, status) {
            (_, MenuStatus::Accepted(_)) => None,
            (MenuButton::Cancel, _) => Some(hit),
            (_, s) if s.actions_enabled() => Some(hit),
            _ => None,
        }
    }
}

fn verb(action: PowerAction) -> &'static str {
    match action {
        PowerAction::PowerOff => "Power off",
        PowerAction::Restart => "Restart",
    }
}

fn progress(action: PowerAction) -> &'static str {
    match action {
        PowerAction::PowerOff => "Powering off",
        PowerAction::Restart => "Restarting",
    }
}

/// Draws the menu at the logical output size.
pub fn draw(
    ty: &Type,
    layout: &MenuLayout,
    status: &MenuStatus,
    pressed: Option<MenuButton>,
) -> Canvas {
    let t = theme::get();
    let size = layout.size;
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    let mut c = Canvas::new(size.0 as i32, size.1 as i32, t.bg);
    let x = layout.power_off.x;
    let col_w = layout.power_off.w;

    // Title, rail, and caption sit above the status line.
    let title_base = layout.status.y - s(260);
    section_label(&mut c, ty, "system", x, title_base - s(110));
    ty.text(
        &mut c,
        Style::bold(s(72) as f32, t.fg),
        "Power",
        x,
        title_base,
    );
    c.fill_rect(
        Rect {
            x,
            y: title_base + s(40),
            w: col_w,
            h: hairline(b),
        },
        t.rust_dim,
    );
    ty.text(
        &mut c,
        Style::regular(s(30) as f32, t.fg_dim),
        "Press the power key to turn the screen off",
        x,
        title_base + s(110),
    );

    match status {
        MenuStatus::Choosing => {}
        MenuStatus::Working(action) => notice(
            &mut c,
            ty,
            layout.status,
            &format!("{} \u{b7} waiting for ferrix-core", progress(*action)),
            Tone::Neutral,
        ),
        MenuStatus::Accepted(action) => notice(
            &mut c,
            ty,
            layout.status,
            &format!("{} \u{b7} accepted", progress(*action)),
            Tone::Accent,
        ),
        MenuStatus::Failed(action, reason) => notice(
            &mut c,
            ty,
            layout.status,
            &format!("{} unavailable \u{b7} {reason}", verb(*action)),
            Tone::Err,
        ),
    }

    let enabled = status.actions_enabled();
    let cancel_enabled = !matches!(status, MenuStatus::Accepted(_));
    button(
        &mut c,
        ty,
        layout.power_off,
        "Power off",
        ButtonKind::Primary,
        pressed == Some(MenuButton::PowerOff),
        enabled,
    );
    button(
        &mut c,
        ty,
        layout.restart,
        "Restart",
        ButtonKind::Secondary,
        pressed == Some(MenuButton::Restart),
        enabled,
    );
    button(
        &mut c,
        ty,
        layout.cancel,
        "Cancel",
        ButtonKind::Secondary,
        pressed == Some(MenuButton::Cancel),
        cancel_enabled,
    );
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_ui::widgets::center;

    const PORTRAIT: (u32, u32) = (1080, 2340);

    #[test]
    fn buttons_stack_in_thumb_reach_above_the_gesture_strip() {
        let l = MenuLayout::new(PORTRAIT);
        assert!(l.power_off.y < l.restart.y && l.restart.y < l.cancel.y);
        assert!(
            l.cancel.y + l.cancel.h <= 2340 - 70,
            "clear of the bottom 3%"
        );
        assert!(
            l.power_off.y > 2340 / 2,
            "the actions sit in the lower half"
        );
        assert!(l.status.y + l.status.h <= l.power_off.y);
        assert_eq!(l.hit(center(l.power_off)), Some(MenuButton::PowerOff));
        assert_eq!(l.hit(center(l.restart)), Some(MenuButton::Restart));
        assert_eq!(l.hit(center(l.cancel)), Some(MenuButton::Cancel));
        assert_eq!(l.hit((540.0, 200.0)), None, "outside the actions");
        let landscape = MenuLayout::new((2340, 1080));
        assert!(landscape.power_off.y > 0, "fits a rotated output");
        assert!(landscape.power_off.w <= 900);
    }

    #[test]
    fn working_disables_actions_and_accepted_disables_everything() {
        let l = MenuLayout::new(PORTRAIT);
        let off = center(l.power_off);
        let cancel = center(l.cancel);
        let working = MenuStatus::Working(PowerAction::PowerOff);
        assert_eq!(l.target(off, &working), None);
        assert_eq!(l.target(cancel, &working), Some(MenuButton::Cancel));
        let failed = MenuStatus::Failed(PowerAction::PowerOff, "unsupported".into());
        assert_eq!(l.target(off, &failed), Some(MenuButton::PowerOff), "retry");
        let accepted = MenuStatus::Accepted(PowerAction::Restart);
        assert_eq!(l.target(cancel, &accepted), None);
        assert_eq!(MenuButton::Restart.action(), Some(PowerAction::Restart));
        assert_eq!(MenuButton::Cancel.action(), None);
    }

    #[test]
    fn the_menu_draws_the_ferrix_field_and_the_rust_primary() {
        let ty = Type::load().unwrap();
        let t = theme::get();
        let l = MenuLayout::new(PORTRAIT);
        let c = draw(&ty, &l, &MenuStatus::Choosing, None);
        assert_eq!((c.width(), c.height()), (1080, 2340));
        assert_eq!(c.get(5, 5), t.bg, "opaque Ferrix black field");
        // The Power off outline carries the region's one rust accent.
        let mid = l.power_off.x + l.power_off.w / 2;
        let across = |y: i32| (y - 3..=y + 3).any(|y| c.get(mid, y) == t.rust);
        assert!(across(l.power_off.y), "primary outline");
        assert!(!across(l.restart.y), "one rust accent per region");
        let failed = draw(
            &ty,
            &l,
            &MenuStatus::Failed(PowerAction::PowerOff, "lifecycle is unsupported".into()),
            None,
        );
        let rule = failed.get(l.status.x + scale(22, 1080), l.status.y + l.status.h / 2);
        assert_eq!(rule, t.err, "a local err cue, not a full-screen alarm");
        if let Ok(dir) = std::env::var("FERRIX_SHELL_PREVIEW_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/power-menu.ppm"), c.ppm_bytes()).unwrap();
            std::fs::write(format!("{dir}/power-menu-failed.ppm"), failed.ppm_bytes()).unwrap();
        }
    }
}
