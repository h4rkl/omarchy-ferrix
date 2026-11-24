//! Touch and pointer handling: one tracked contact, press feedback, taps on
//! the controls, and an inward edge swipe for Back (it yields to Home).

// The Wayland half (app.rs) is Linux-only; host builds use the rest in tests
// and previews.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use ferrix_ui::gesture::{Gesture, Tracker};

use crate::model::{Action, Model};
use crate::screen::{Target, hit_test};

#[derive(Debug, Default)]
pub struct Input {
    tracker: Tracker<Target>,
    pub pressed: Option<Target>,
}

impl Input {
    pub fn down(&mut self, id: i32, position: (f64, f64), model: &Model, size: (u32, u32)) -> bool {
        match self
            .tracker
            .down(id, position, hit_test(model, size, position))
        {
            Some(pressed) => {
                self.pressed = pressed;
                pressed.is_some()
            }
            None => false,
        }
    }

    pub fn motion(
        &mut self,
        id: i32,
        position: (f64, f64),
        model: &Model,
        size: (u32, u32),
    ) -> bool {
        let target_at = hit_test(model, size, position);
        match self
            .tracker
            .motion(id, position, size, target_at, self.pressed)
        {
            Some(pressed) => {
                self.pressed = pressed;
                true
            }
            None => false,
        }
    }

    /// Ends the contact at `position` (the last known one when `None`).
    pub fn up(
        &mut self,
        id: i32,
        position: Option<(f64, f64)>,
        model: &mut Model,
        size: (u32, u32),
    ) -> bool {
        let Some(at) = position.or_else(|| self.tracker.current(id)) else {
            return false;
        };
        let target_at = hit_test(model, size, at);
        let was_pressed = self.pressed.take().is_some();
        let changed = match self.tracker.up(id, size, target_at) {
            Some(Gesture::Tap(target)) => activate(model, target),
            Some(Gesture::BackSwipe) => {
                model.pending.push(Action::Back);
                true
            }
            _ => false,
        };
        changed || was_pressed
    }

    pub fn cancel(&mut self) -> bool {
        self.pressed = None;
        self.tracker.cancel()
    }
}

pub fn activate(model: &mut Model, target: Target) -> bool {
    match target {
        Target::Shutter => model.shutter(),
        Target::Switch => model.switch(),
        Target::Gallery => model.gallery(),
        Target::Retry => model.retry(),
        Target::Back => {
            model.pending.push(Action::Back);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Event, tests::info};
    use crate::screen::layout;
    use ferrix_hardware_api::{CameraFacing, CameraId, CameraRotation};
    use ferrix_ui::widgets::center;

    const SIZE: (u32, u32) = (1080, 2340);

    #[test]
    fn taps_capture_switch_and_hand_off_and_edge_swipes_go_back() {
        let mut m = Model::new();
        m.apply(Event::Cameras(Ok(vec![
            info("b", CameraFacing::Back, CameraRotation::Deg90),
            info("f", CameraFacing::Front, CameraRotation::Deg270),
        ])));
        m.pending.clear();
        let l = layout(SIZE);
        let mut input = Input::default();
        let tap = |input: &mut Input, m: &mut Model, at| {
            input.down(0, at, m, SIZE);
            input.up(0, Some(at), m, SIZE)
        };
        assert!(tap(&mut input, &mut m, center(l.shutter)));
        assert_eq!(m.pending, [Action::Capture(CameraId::new("b").unwrap())]);
        m.pending.clear();
        assert!(tap(&mut input, &mut m, center(l.switch)));
        assert_eq!(
            m.pending,
            [Action::OpenPreview(CameraId::new("f").unwrap())]
        );
        m.pending.clear();
        tap(&mut input, &mut m, center(l.gallery));
        assert_eq!(m.pending, [Action::ShowPictures]);
        m.pending.clear();
        input.down(0, (10.0, 1000.0), &m, SIZE);
        input.motion(0, (400.0, 1010.0), &m, SIZE);
        assert!(input.up(0, None, &mut m, SIZE));
        assert_eq!(m.pending, [Action::Back]);
    }
}
