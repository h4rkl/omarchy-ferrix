//! Trusted bottom-edge "home" gesture recognizer.
//!
//! The compositor owns this gesture so no application can swallow it: a
//! contact (touch slot or pointer left button) that begins in the bottom edge
//! zone of the output is grabbed and never forwarded to clients. When it
//! lifts, a tap or ordinary upward swipe means "go home"; a long upward
//! swipe dismisses the foreground app. This mirrors the mobile distinction
//! between Home/minimise and swiping an app away.
//!
//! This module is pure geometry in output logical coordinates; it knows
//! nothing about Wayland, Smithay, or which backend produced the contact, so
//! it builds and tests on every host. Thresholds are fractions of the output
//! so the same recognizer serves 1080x2340, a rotated 2340x1080, and the
//! smaller 830x1800 development geometry alike.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// A contact must start at or below this fraction of the output height
/// (the bottom 3%) to be considered for the home gesture. Keep this within
/// the system navigation strip so application controls near the bottom edge
/// (for example an Android browser's onboarding arrow) remain clickable.
pub const START_ZONE: f64 = 0.97;
/// Upward travel, as a fraction of `min(width, height)`, beyond which the
/// contact is a swipe (and it must be more vertical than horizontal).
pub const SWIPE_FRACTION: f64 = 0.12;
/// A deliberate long upward swipe closes the foreground app rather than
/// merely sending it Home.
pub const CLOSE_FRACTION: f64 = 0.55;
/// Total travel, as a fraction of `min(width, height)`, below which the
/// contact is a tap.
pub const TAP_FRACTION: f64 = 0.035;

/// How the home gesture was performed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HomeGestureKind {
    /// A short contact that barely moved inside the edge zone.
    Tap,
    /// An upward swipe out of the edge zone.
    Swipe,
    /// A long upward swipe that dismisses the foreground app.
    Close,
}

impl HomeGestureKind {
    pub fn label(self) -> &'static str {
        match self {
            HomeGestureKind::Tap => "tap",
            HomeGestureKind::Swipe => "swipe",
            HomeGestureKind::Close => "close",
        }
    }
}

/// A recognised home gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HomeGesture {
    pub kind: HomeGestureKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Contact {
    start: (f64, f64),
    last: (f64, f64),
    /// `min(width, height)` of the output when the contact began.
    reference: f64,
}

/// State machine for one grabbed edge contact. At most one contact is grabbed
/// at a time; [`EdgeGesture::begin`] refuses a second one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeGesture {
    contact: Option<Contact>,
}

impl EdgeGesture {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a contact is currently grabbed.
    pub fn is_active(&self) -> bool {
        self.contact.is_some()
    }

    /// Offer a new contact at `pos` on an output of `output_size` (both in
    /// logical coordinates). Returns `true` when the contact is grabbed: it
    /// started in the edge zone and no other contact is grabbed. A grabbed
    /// contact must not be forwarded to clients.
    pub fn begin(&mut self, pos: (f64, f64), output_size: (f64, f64)) -> bool {
        if self.contact.is_some() {
            return false;
        }
        let (width, height) = output_size;
        if !(width > 0.0 && height > 0.0) || !pos.0.is_finite() || !pos.1.is_finite() {
            return false;
        }
        if pos.1 < START_ZONE * height {
            return false;
        }
        self.contact = Some(Contact {
            start: pos,
            last: pos,
            reference: width.min(height),
        });
        true
    }

    /// Track movement of the grabbed contact. Ignored when nothing is grabbed.
    pub fn update(&mut self, pos: (f64, f64)) {
        if let Some(contact) = &mut self.contact
            && pos.0.is_finite()
            && pos.1.is_finite()
        {
            contact.last = pos;
        }
    }

    /// The grabbed contact lifted. Returns the recognised gesture, or `None`
    /// when it was neither a tap nor an upward swipe (or nothing was grabbed).
    pub fn end(&mut self) -> Option<HomeGesture> {
        let contact = self.contact.take()?;
        let dx = contact.last.0 - contact.start.0;
        // Screen coordinates grow downwards; an upward swipe has negative dy.
        let dy = contact.last.1 - contact.start.1;
        let up = -dy;
        let travelled = (dx * dx + dy * dy).sqrt();
        if up > CLOSE_FRACTION * contact.reference && dy.abs() > dx.abs() {
            Some(HomeGesture {
                kind: HomeGestureKind::Close,
            })
        } else if up > SWIPE_FRACTION * contact.reference && dy.abs() > dx.abs() {
            Some(HomeGesture {
                kind: HomeGestureKind::Swipe,
            })
        } else if travelled < TAP_FRACTION * contact.reference {
            Some(HomeGesture {
                kind: HomeGestureKind::Tap,
            })
        } else {
            None
        }
    }

    /// Drop the grabbed contact without recognising anything (touch cancel,
    /// device removal).
    pub fn cancel(&mut self) {
        self.contact = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORTRAIT: (f64, f64) = (1080.0, 2340.0);
    const LANDSCAPE: (f64, f64) = (2340.0, 1080.0);
    const SMALL: (f64, f64) = (830.0, 1800.0);

    fn swipe(size: (f64, f64), start: (f64, f64), end: (f64, f64)) -> Option<HomeGesture> {
        let mut gesture = EdgeGesture::new();
        assert!(
            gesture.begin(start, size),
            "start {start:?} must be grabbed"
        );
        gesture.update(((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0));
        gesture.update(end);
        let result = gesture.end();
        assert!(!gesture.is_active(), "end releases the grab");
        result
    }

    fn kind(result: Option<HomeGesture>) -> Option<HomeGestureKind> {
        result.map(|g| g.kind)
    }

    #[test]
    fn contacts_outside_the_bottom_zone_are_not_grabbed() {
        for size in [PORTRAIT, LANDSCAPE, SMALL] {
            let mut gesture = EdgeGesture::new();
            let zone = START_ZONE * size.1;
            assert!(!gesture.begin((size.0 / 2.0, zone - 1.0), size), "{size:?}");
            assert!(!gesture.is_active());
            assert!(gesture.begin((size.0 / 2.0, zone), size), "{size:?}");
            assert!(gesture.is_active());
            gesture.cancel();
            assert!(!gesture.is_active());
            assert!(gesture.begin((0.0, size.1 - 1.0), size), "corner {size:?}");
        }
        // 1080x2340: zone starts at y = 2269.8; 830x1800: y = 1746; 2340x1080: y = 1047.6.
        let mut gesture = EdgeGesture::new();
        assert!(!gesture.begin((540.0, 2269.0), PORTRAIT));
        assert!(gesture.begin((540.0, 2270.0), PORTRAIT));
        let mut gesture = EdgeGesture::new();
        assert!(!gesture.begin((415.0, 1745.0), SMALL));
        assert!(gesture.begin((415.0, 1746.0), SMALL));
        let mut gesture = EdgeGesture::new();
        assert!(!gesture.begin((1170.0, 1047.0), LANDSCAPE));
        assert!(gesture.begin((1170.0, 1048.0), LANDSCAPE));
    }

    #[test]
    fn app_controls_above_the_system_strip_are_not_grabbed() {
        let mut gesture = EdgeGesture::new();
        assert!(
            !gesture.begin((900.0, 2260.0), PORTRAIT),
            "the Android app action area must remain available"
        );
        assert!(gesture.begin((900.0, 2280.0), PORTRAIT));
        gesture.cancel();
    }

    #[test]
    fn only_one_contact_is_grabbed_at_a_time() {
        let mut gesture = EdgeGesture::new();
        assert!(gesture.begin((540.0, 2300.0), PORTRAIT));
        assert!(
            !gesture.begin((600.0, 2300.0), PORTRAIT),
            "second finger in the zone is forwarded normally"
        );
        assert_eq!(kind(gesture.end()), Some(HomeGestureKind::Tap));
        assert!(gesture.begin((600.0, 2300.0), PORTRAIT));
        gesture.cancel();
    }

    #[test]
    fn upward_swipe_is_recognised_on_portrait_output() {
        // min(1080, 2340) * 0.12 = 129.6 px of upward travel.
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2300.0), (540.0, 2300.0 - 130.0))),
            Some(HomeGestureKind::Swipe)
        );
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2300.0), (560.0, 1800.0))),
            Some(HomeGestureKind::Swipe),
            "a long swipe with a little drift"
        );
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2300.0), (540.0, 2300.0 - 129.0))),
            None,
            "just under the swipe threshold and far beyond the tap threshold"
        );
    }

    #[test]
    fn long_upward_swipe_requests_close() {
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2300.0), (540.0, 1600.0))),
            Some(HomeGestureKind::Close)
        );
        assert_eq!(
            kind(swipe(SMALL, (415.0, 1780.0), (415.0, 1250.0))),
            Some(HomeGestureKind::Close)
        );
    }

    #[test]
    fn upward_swipe_is_recognised_on_landscape_output() {
        // Rotated: min(2340, 1080) * 0.12 = 129.6 px, zone starts at y = 993.6.
        assert_eq!(
            kind(swipe(LANDSCAPE, (1170.0, 1060.0), (1170.0, 1060.0 - 130.0))),
            Some(HomeGestureKind::Swipe)
        );
        assert_eq!(
            kind(swipe(LANDSCAPE, (1170.0, 1060.0), (1170.0, 1060.0 - 129.0))),
            None
        );
    }

    #[test]
    fn upward_swipe_is_recognised_on_small_output() {
        // min(830, 1800) * 0.12 = 99.6 px; zone starts at y = 1656.
        assert_eq!(
            kind(swipe(SMALL, (415.0, 1780.0), (415.0, 1780.0 - 100.0))),
            Some(HomeGestureKind::Swipe)
        );
        assert_eq!(
            kind(swipe(SMALL, (415.0, 1780.0), (415.0, 1780.0 - 99.0))),
            None
        );
    }

    #[test]
    fn tap_is_recognised_within_the_tap_radius() {
        // Tap radius: 1080 * 0.035 = 37.8 px; 830 * 0.035 = 29.05 px.
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2300.0), (540.0, 2300.0))),
            Some(HomeGestureKind::Tap)
        );
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2300.0), (540.0 + 37.0, 2300.0))),
            Some(HomeGestureKind::Tap)
        );
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2300.0), (540.0 + 38.0, 2300.0))),
            None,
            "sideways drag past the tap radius is neither"
        );
        assert_eq!(
            kind(swipe(SMALL, (415.0, 1780.0), (415.0, 1780.0 - 29.0))),
            Some(HomeGestureKind::Tap)
        );
        assert_eq!(kind(swipe(SMALL, (415.0, 1780.0), (415.0, 1750.0))), None);
        assert_eq!(
            kind(swipe(LANDSCAPE, (1170.0, 1060.0), (1190.0, 1040.0))),
            Some(HomeGestureKind::Tap),
            "diagonal 28 px within the 37.8 px radius"
        );
    }

    #[test]
    fn horizontal_or_downward_movement_is_not_home() {
        assert_eq!(
            kind(swipe(PORTRAIT, (540.0, 2270.0), (540.0, 2339.0))),
            None,
            "downward drag"
        );
        assert_eq!(
            kind(swipe(PORTRAIT, (300.0, 2300.0), (800.0, 2300.0 - 140.0))),
            None,
            "moved up past the threshold but mostly sideways"
        );
        assert_eq!(
            kind(swipe(LANDSCAPE, (200.0, 1060.0), (2000.0, 1060.0))),
            None,
            "horizontal drag along the bottom edge"
        );
    }

    #[test]
    fn end_uses_the_final_position_not_the_extreme() {
        let mut gesture = EdgeGesture::new();
        assert!(gesture.begin((540.0, 2300.0), PORTRAIT));
        gesture.update((540.0, 1500.0));
        gesture.update((540.0, 2290.0));
        assert_eq!(
            kind(gesture.end()),
            Some(HomeGestureKind::Tap),
            "swiped up and came back: a tap"
        );
    }

    #[test]
    fn end_and_update_without_a_grab_are_harmless() {
        let mut gesture = EdgeGesture::new();
        gesture.update((1.0, 1.0));
        assert_eq!(gesture.end(), None);
        gesture.cancel();
        assert!(
            !gesture.begin((10.0, 10.0), (0.0, 0.0)),
            "degenerate output"
        );
        assert!(!gesture.begin((f64::NAN, 2300.0), PORTRAIT));
    }
}
