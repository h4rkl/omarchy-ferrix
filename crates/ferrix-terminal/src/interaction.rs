//! Target-independent interaction model for the Terminal, shared by pointer
//! and touch input. Pure geometry and state: no Wayland, no PTY.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::osk::{self, Key, OskState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The header arrow: yield the foreground to the home surface.
    Back,
    /// The bottom gesture zone. Drawn as the pill; owned by the compositor,
    /// so the app never acts on it.
    Home,
    /// An on-screen keyboard key.
    Key(Key),
    /// The terminal grid itself: vertical drags scroll history.
    TermArea,
}

/// Terminal work produced by the interaction model for the session owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermAction {
    /// Bytes for the shell's stdin.
    Input(Vec<u8>),
    /// Viewport scroll in lines; positive moves into history.
    Scroll(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub pressed: Option<Target>,
    pub osk: OskState,
    /// Queued terminal actions; the session owner drains these after input.
    pub pending: Vec<TermAction>,
    /// Number of Back requests not yet delivered to `ferrix_appd`.
    pub pending_back: u32,
    /// Mirror of the terminal's DECCKM mode so arrows encode correctly.
    pub term_app_cursor: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        Self {
            pressed: None,
            osk: OskState::default(),
            pending: Vec::new(),
            pending_back: 0,
            term_app_cursor: false,
        }
    }

    fn back(&mut self) {
        self.pending_back += 1;
    }

    fn activate(&mut self, target: Target) {
        match target {
            Target::Back => self.back(),
            Target::Key(key) => {
                if let Some(bytes) = self.osk.press(key, self.term_app_cursor) {
                    self.pending.push(TermAction::Input(bytes));
                }
            }
            Target::Home | Target::TermArea => {}
        }
    }
}

/// Terminal scroll step in panel pixels: one text line on the 1080 panel.
fn scroll_line(size: (u32, u32)) -> f64 {
    f64::from(size.0) * 39.0 / 1080.0
}

#[derive(Clone, Copy, Debug)]
struct Contact {
    id: i32,
    start: (f64, f64),
    current: (f64, f64),
    /// Last position at which a scroll step was emitted.
    anchor: (f64, f64),
    target: Option<Target>,
}

#[derive(Default)]
pub struct Input {
    contact: Option<Contact>,
}

impl Input {
    pub fn down(
        &mut self,
        id: i32,
        position: (f64, f64),
        state: &mut UiState,
        size: (u32, u32),
    ) -> bool {
        if self.contact.is_some() {
            return false;
        }
        let target = hit_test(state, position, size);
        self.contact = Some(Contact {
            id,
            start: position,
            current: position,
            anchor: position,
            target,
        });
        // The grid itself has no press feedback; keys and controls do.
        state.pressed = target.filter(|t| *t != Target::TermArea);
        true
    }

    pub fn motion(
        &mut self,
        id: i32,
        position: (f64, f64),
        state: &mut UiState,
        size: (u32, u32),
    ) -> bool {
        let Some(contact) = self.contact.as_mut().filter(|contact| contact.id == id) else {
            return false;
        };
        contact.current = position;
        if contact.target == Some(Target::TermArea) {
            // Dragging down reveals history, one step per text line.
            let step = scroll_line(size);
            let lines = ((position.1 - contact.anchor.1) / step).trunc() as i32;
            if lines != 0 {
                contact.anchor.1 += f64::from(lines) * step;
                state.pending.push(TermAction::Scroll(lines));
                return true;
            }
            return false;
        }
        let target = hit_test(state, position, size);
        let pressed = if distance(contact.start, position) < tap_slop(size) {
            contact
                .target
                .filter(|candidate| Some(*candidate) == target)
        } else {
            None
        };
        let changed = state.pressed != pressed;
        state.pressed = pressed;
        changed
    }

    pub fn up(&mut self, id: i32, state: &mut UiState, size: (u32, u32)) -> bool {
        let Some(contact) = self.contact.filter(|contact| contact.id == id) else {
            return false;
        };
        self.contact = None;
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let dx = contact.current.0 - contact.start.0;
        let dy = contact.current.1 - contact.start.1;
        let swipe = w.min(h) * 0.12;

        if contact.start.0 <= w * 0.08 && dx > swipe && dx.abs() > dy.abs() {
            state.back();
        } else if distance(contact.start, contact.current) < tap_slop(size)
            && let Some(target) = contact.target
            && hit_test(state, contact.current, size) == Some(target)
        {
            state.activate(target);
        }
        state.pressed = None;
        true
    }

    pub fn cancel(&mut self, state: &mut UiState) -> bool {
        self.contact.take().is_some() || state.pressed.take().is_some()
    }
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn tap_slop(size: (u32, u32)) -> f64 {
    f64::from(size.0.min(size.1)) * 0.035
}

pub fn hit_test(state: &UiState, position: (f64, f64), size: (u32, u32)) -> Option<Target> {
    let (x, y) = position;
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    if y >= h * 0.92 {
        return Some(Target::Home);
    }
    let area = osk::terminal_area(size);
    if x <= w * 0.22 && y < f64::from(area.y) {
        return Some(Target::Back);
    }
    if let Some((_, key)) = osk::keys(size, &state.osk)
        .into_iter()
        .find(|(rect, _)| osk::contains(*rect, position))
    {
        return Some(Target::Key(key));
    }
    osk::contains(area, position).then_some(Target::TermArea)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1080, 2340);

    fn center(keys: &[(ferrix_ui::draw::Rect, Key)], wanted: Key) -> (f64, f64) {
        let (r, _) = keys
            .iter()
            .find(|(_, k)| *k == wanted)
            .expect("key on layout");
        (f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2))
    }

    #[test]
    fn keys_queue_shell_input_and_drags_scroll_history() {
        let mut state = UiState::new();
        let mut input = Input::default();
        let keys = osk::keys(SIZE, &state.osk);
        for key in [Key::Char('l'), Key::Char('s'), Key::Enter] {
            input.down(0, center(&keys, key), &mut state, SIZE);
            assert_eq!(state.pressed, Some(Target::Key(key)));
            input.up(0, &mut state, SIZE);
        }
        assert_eq!(
            state.pending,
            vec![
                TermAction::Input(b"l".to_vec()),
                TermAction::Input(b"s".to_vec()),
                TermAction::Input(b"\r".to_vec()),
            ]
        );
        state.pending.clear();

        // Ctrl is sticky for one key: ctrl, c -> ETX.
        input.down(0, center(&keys, Key::Ctrl), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        input.down(0, center(&keys, Key::Char('c')), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending, vec![TermAction::Input(vec![0x03])]);
        state.pending.clear();

        // Shift is sticky for one letter too.
        input.down(0, center(&keys, Key::Shift), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert!(state.osk.shift);
        let shifted = osk::keys(SIZE, &state.osk);
        input.down(0, center(&shifted, Key::Char('a')), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending, vec![TermAction::Input(b"A".to_vec())]);
        assert!(!state.osk.shift);
        state.pending.clear();

        // Dragging down inside the grid scrolls into history without pressing.
        let area = osk::terminal_area(SIZE);
        let x = f64::from(area.x + area.w / 2);
        let y = f64::from(area.y + area.h / 2);
        input.down(0, (x, y), &mut state, SIZE);
        assert_eq!(state.pressed, None);
        input.motion(0, (x, y + 200.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending, vec![TermAction::Scroll(5)]);
    }

    #[test]
    fn back_header_and_left_edge_swipe_request_yield() {
        let mut state = UiState::new();
        let mut input = Input::default();
        assert_eq!(hit_test(&state, (80.0, 160.0), SIZE), Some(Target::Back));
        input.down(0, (80.0, 160.0), &mut state, SIZE);
        assert_eq!(state.pressed, Some(Target::Back));
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending_back, 1);

        input.down(0, (20.0, 900.0), &mut state, SIZE);
        input.motion(0, (300.0, 900.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending_back, 2);
        assert!(state.pending.is_empty(), "a swipe types nothing");
    }

    #[test]
    fn bottom_zone_belongs_to_the_compositor() {
        let mut state = UiState::new();
        let mut input = Input::default();
        assert_eq!(hit_test(&state, (540.0, 2290.0), SIZE), Some(Target::Home));
        input.down(0, (540.0, 2290.0), &mut state, SIZE);
        assert_eq!(state.pressed, Some(Target::Home));
        input.motion(0, (540.0, 1900.0), &mut state, SIZE);
        input.up(0, &mut state, SIZE);
        assert_eq!(state.pending_back, 0);
        assert!(state.pending.is_empty());
        assert_eq!(state.pressed, None);
    }
}
