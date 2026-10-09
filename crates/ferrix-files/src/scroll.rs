//! A whole-row vertical viewport and its position rail.
//!
//! `ferrix-ui` has no scrolling list primitive yet. This module is written to
//! be lifted into `ferrix-ui::widgets` unchanged: it depends only on
//! `ferrix-ui` drawing types, knows nothing about files, and keeps geometry
//! pure so hit tests and rendering share it. The viewport scrolls by whole
//! rows (a row is the unit people tap), clamps lazily so the stored offset
//! survives a list shrinking and growing again, and never animates: the
//! pressed-state feedback is the motion contract (`docs/design.md`).

use std::ops::Range;

use ferrix_ui::draw::{Canvas, Rect};
use ferrix_ui::theme;
use ferrix_ui::widgets::{basis, scale};

/// First visible row of a list of uniform rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    first: usize,
}

impl Scroll {
    /// The first visible row once clamped so the last page stays full.
    pub fn first(&self, count: usize, capacity: usize) -> usize {
        self.first.min(count.saturating_sub(capacity))
    }

    /// Indices of the visible rows.
    pub fn window(&self, count: usize, capacity: usize) -> Range<usize> {
        let first = self.first(count, capacity);
        first..count.min(first + capacity)
    }

    /// Moves by `delta` rows (positive reveals later rows). Returns whether
    /// the visible window changed.
    pub fn scroll_by(&mut self, delta: i32, count: usize, capacity: usize) -> bool {
        let before = self.first(count, capacity);
        let max = count.saturating_sub(capacity);
        let next = if delta >= 0 {
            before
                .saturating_add(delta.unsigned_abs() as usize)
                .min(max)
        } else {
            before.saturating_sub(delta.unsigned_abs() as usize)
        };
        self.first = next;
        next != before
    }

    /// Brings `index` into view with the fewest rows moved.
    pub fn reveal(&mut self, index: usize, count: usize, capacity: usize) {
        let first = self.first(count, capacity);
        if index < first {
            self.first = index;
        } else if capacity > 0 && index >= first + capacity {
            self.first = index + 1 - capacity;
        } else {
            self.first = first;
        }
    }

    pub fn reset(&mut self) {
        self.first = 0;
    }
}

/// Thumb of a position rail inside `track`, or `None` when everything fits.
pub fn thumb(track: Rect, count: usize, capacity: usize, first: usize) -> Option<Rect> {
    if count <= capacity || count == 0 || track.h <= 0 {
        return None;
    }
    let h = ((track.h as usize * capacity) / count).max(track.w as usize * 4) as i32;
    let h = h.min(track.h);
    let span = (track.h - h) as usize;
    let max_first = count - capacity;
    let y = track.y + (span * first.min(max_first) / max_first) as i32;
    Some(Rect {
        x: track.x,
        y,
        w: track.w,
        h,
    })
}

/// Draws the rail: a quiet `border` track and an `fg_dim` thumb. Nothing is
/// drawn when every row fits, so a short list carries no chrome.
pub fn rail(c: &mut Canvas, track: Rect, count: usize, capacity: usize, first: usize) {
    let Some(t) = thumb(track, count, capacity, first) else {
        return;
    };
    let theme = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    c.fill_rounded(track, scale(3, b), theme.border);
    c.fill_rounded(t, scale(3, b), theme.fg_dim);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_clamps_and_scrolls_by_rows() {
        let mut s = Scroll::default();
        assert_eq!(s.window(3, 9), 0..3);
        assert!(!s.scroll_by(5, 3, 9), "a short list never scrolls");
        assert_eq!(s.window(20, 9), 0..9);
        assert!(s.scroll_by(4, 20, 9));
        assert_eq!(s.window(20, 9), 4..13);
        assert!(s.scroll_by(100, 20, 9));
        assert_eq!(s.window(20, 9), 11..20);
        assert!(!s.scroll_by(1, 20, 9));
        assert!(s.scroll_by(-3, 20, 9));
        assert_eq!(s.first(20, 9), 8);
        // A shrinking list clamps without losing the offset arithmetic.
        assert_eq!(s.window(10, 9), 1..10);
        assert!(s.scroll_by(-100, 20, 9));
        assert_eq!(s.first(20, 9), 0);
        assert_eq!(s.window(0, 9), 0..0);
    }

    #[test]
    fn reveal_moves_the_fewest_rows() {
        let mut s = Scroll::default();
        s.reveal(3, 20, 9);
        assert_eq!(s.first(20, 9), 0, "already visible");
        s.reveal(12, 20, 9);
        assert_eq!(s.window(20, 9), 4..13);
        s.reveal(2, 20, 9);
        assert_eq!(s.first(20, 9), 2);
        s.reset();
        assert_eq!(s.first(20, 9), 0);
    }

    #[test]
    fn thumb_tracks_position_inside_the_rail() {
        let track = Rect {
            x: 1040,
            y: 400,
            w: 6,
            h: 900,
        };
        assert!(thumb(track, 5, 9, 0).is_none());
        let top = thumb(track, 30, 9, 0).unwrap();
        assert_eq!((top.y, top.h), (400, 270));
        let bottom = thumb(track, 30, 9, 21).unwrap();
        assert_eq!(bottom.y + bottom.h, 1300);
        let tiny = thumb(track, 10_000, 9, 0).unwrap();
        assert_eq!(tiny.h, 24, "the thumb stays touchable-visible");
    }
}
