//! Touch/pointer gesture tracking shared by the reference surfaces: a single
//! tracked contact, press feedback with tap slop, the left-edge back swipe,
//! and vertical drags. Pure state: no Wayland. Each surface supplies its own
//! `hit_test` and interprets the resulting [`Gesture`].

/// Fraction of the shorter panel side a contact may move and still be a tap.
pub const TAP_SLOP: f64 = 0.035;
/// Fraction of the shorter panel side a swipe must travel.
pub const SWIPE: f64 = 0.12;
/// Fraction of the panel width that counts as the left edge.
pub const LEFT_EDGE: f64 = 0.08;
/// Fraction of the panel height at the bottom that is the compositor's.
pub const HOME_ZONE: f64 = 0.92;

/// What a released contact turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture<T> {
    /// A tap that started and ended on the same target.
    Tap(T),
    /// A rightward swipe that started at the left edge.
    BackSwipe,
    /// A downward swipe that started at the top edge.
    PullDown,
    /// Anything else (a drag, a cancelled tap).
    Release,
}

#[derive(Clone, Copy, Debug)]
struct Contact<T> {
    id: i32,
    start: (f64, f64),
    current: (f64, f64),
    /// Last position at which a drag step was emitted.
    anchor: (f64, f64),
    target: Option<T>,
}

/// Tracks at most one contact at a time (extra fingers are ignored).
#[derive(Debug)]
pub struct Tracker<T> {
    contact: Option<Contact<T>>,
}

impl<T> Default for Tracker<T> {
    fn default() -> Self {
        Self { contact: None }
    }
}

pub fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

pub fn tap_slop(size: (u32, u32)) -> f64 {
    f64::from(size.0.min(size.1)) * TAP_SLOP
}

/// Whether `position` is inside the compositor's home-gesture zone.
pub fn in_home_zone(position: (f64, f64), size: (u32, u32)) -> bool {
    position.1 >= f64::from(size.1) * HOME_ZONE
}

impl<T: Copy + PartialEq> Tracker<T> {
    /// Starts tracking. Returns the target to show pressed, or `None` when
    /// another contact is already tracked (nothing changes).
    pub fn down(&mut self, id: i32, position: (f64, f64), target: Option<T>) -> Option<Option<T>> {
        if self.contact.is_some() {
            return None;
        }
        self.contact = Some(Contact {
            id,
            start: position,
            current: position,
            anchor: position,
            target,
        });
        Some(target)
    }

    /// Updates the contact. Returns the new pressed target (`Some(None)`
    /// when the finger left its target) or `None` when nothing changed.
    pub fn motion(
        &mut self,
        id: i32,
        position: (f64, f64),
        size: (u32, u32),
        target_at: Option<T>,
        current_pressed: Option<T>,
    ) -> Option<Option<T>> {
        let contact = self.contact.as_mut().filter(|c| c.id == id)?;
        contact.current = position;
        let pressed = if distance(contact.start, position) < tap_slop(size) {
            contact.target.filter(|t| Some(*t) == target_at)
        } else {
            None
        };
        (pressed != current_pressed).then_some(pressed)
    }

    /// Vertical drag steps of `step` pixels since the last call, for the
    /// tracked contact only; the anchor advances by whole steps.
    pub fn drag_steps(&mut self, id: i32, step: f64) -> i32 {
        let Some(contact) = self.contact.as_mut().filter(|c| c.id == id) else {
            return 0;
        };
        if step <= 0.0 {
            return 0;
        }
        let steps = ((contact.current.1 - contact.anchor.1) / step).trunc() as i32;
        contact.anchor.1 += f64::from(steps) * step;
        steps
    }

    /// Last known position of the tracked contact.
    pub fn current(&self, id: i32) -> Option<(f64, f64)> {
        self.contact.filter(|c| c.id == id).map(|c| c.current)
    }

    /// The tracked target, if any.
    pub fn target(&self) -> Option<T> {
        self.contact.and_then(|c| c.target)
    }

    pub fn is_tracking(&self, id: i32) -> bool {
        self.contact.is_some_and(|c| c.id == id)
    }

    /// Ends tracking and classifies the contact. `target_at` is the target
    /// under the final position.
    pub fn up(&mut self, id: i32, size: (u32, u32), target_at: Option<T>) -> Option<Gesture<T>> {
        let contact = self.contact.filter(|c| c.id == id)?;
        self.contact = None;
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let dx = contact.current.0 - contact.start.0;
        let dy = contact.current.1 - contact.start.1;
        let swipe = w.min(h) * SWIPE;
        if contact.start.0 <= w * LEFT_EDGE && dx > swipe && dx.abs() > dy.abs() {
            return Some(Gesture::BackSwipe);
        }
        if contact.start.1 <= h * LEFT_EDGE && dy > swipe && dy.abs() > dx.abs() {
            return Some(Gesture::PullDown);
        }
        if distance(contact.start, contact.current) < tap_slop(size)
            && let Some(target) = contact.target
            && target_at == Some(target)
        {
            return Some(Gesture::Tap(target));
        }
        Some(Gesture::Release)
    }

    /// Drops the contact. Returns whether one existed.
    pub fn cancel(&mut self) -> bool {
        self.contact.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1080, 2340);

    #[test]
    fn taps_swipes_and_drags_classify() {
        let mut t: Tracker<u8> = Tracker::default();
        assert_eq!(t.down(0, (500.0, 500.0), Some(1)), Some(Some(1)));
        assert_eq!(
            t.down(1, (10.0, 10.0), Some(2)),
            None,
            "second finger ignored"
        );
        assert_eq!(t.motion(0, (505.0, 505.0), SIZE, Some(1), Some(1)), None);
        assert_eq!(
            t.motion(0, (505.0, 505.0), SIZE, Some(3), Some(1)),
            Some(None)
        );
        assert_eq!(
            t.motion(0, (505.0, 505.0), SIZE, Some(1), None),
            Some(Some(1))
        );
        assert_eq!(t.up(0, SIZE, Some(1)), Some(Gesture::Tap(1)));
        assert_eq!(t.up(0, SIZE, Some(1)), None, "already released");

        t.down(0, (20.0, 900.0), None);
        t.motion(0, (300.0, 910.0), SIZE, None, None);
        assert_eq!(t.up(0, SIZE, None), Some(Gesture::BackSwipe));

        t.down(0, (500.0, 20.0), None);
        t.motion(0, (500.0, 400.0), SIZE, None, None);
        assert_eq!(t.up(0, SIZE, None), Some(Gesture::PullDown));

        t.down(0, (500.0, 1000.0), Some(4));
        t.motion(0, (500.0, 1200.0), SIZE, Some(4), Some(4));
        assert_eq!(t.drag_steps(0, 39.0), 5);
        assert_eq!(t.drag_steps(0, 39.0), 0, "anchor advanced");
        assert_eq!(
            t.up(0, SIZE, Some(4)),
            Some(Gesture::Release),
            "moved too far for a tap"
        );
        assert!(in_home_zone((540.0, 2290.0), SIZE));
        assert!(!in_home_zone((540.0, 2000.0), SIZE));
        t.down(0, (1.0, 1.0), Some(9));
        assert!(t.cancel());
        assert!(!t.cancel());
    }
}
