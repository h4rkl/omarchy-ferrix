//! Text selection over the terminal buffer. Pure state: no PTY, no drawing.
//!
//! Points are buffer coordinates in `alacritty_terminal`'s convention: line
//! `0` is the top row of the live screen, negative lines are scrollback, and
//! a viewport row `r` shown at display offset `o` is line `r - o`. A selection
//! therefore stays on its text while the view scrolls; when shell output
//! pushes lines into history the owner calls [`Selection::shift`] with the
//! growth so it follows the text there too.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// A buffer cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    pub line: i32,
    pub col: usize,
}

impl Point {
    pub fn new(line: i32, col: usize) -> Self {
        Self { line, col }
    }

    /// The buffer point under viewport `row` at display offset `offset`.
    pub fn from_viewport(row: usize, col: usize, offset: usize) -> Self {
        Self {
            line: row as i32 - offset as i32,
            col,
        }
    }

    /// The viewport row showing this point, if it is on screen.
    pub fn viewport_row(self, offset: usize, rows: usize) -> Option<usize> {
        let row = self.line + offset as i32;
        (row >= 0 && (row as usize) < rows).then_some(row as usize)
    }
}

/// Which end of a selection a handle moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handle {
    Start,
    End,
}

/// An inclusive range of cells between two points in reading order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    anchor: Point,
    head: Point,
}

/// Characters that end a word for long-press selection: whitespace plus the
/// usual shell and path delimiters, so `ls -la /etc/hosts` selects `/etc/hosts`
/// and `"quoted"` selects `quoted`.
const DELIMITERS: &str = " \t'\"`()[]{}<>|;,&";

fn is_word(c: char) -> bool {
    !DELIMITERS.contains(c)
}

/// The `(first, last)` columns of the word at `col` in `line`, or the single
/// cell when `col` is on a delimiter or past the end.
pub fn word_at(line: &[char], col: usize) -> (usize, usize) {
    if col >= line.len() || !is_word(line[col]) {
        return (col, col);
    }
    let mut first = col;
    while first > 0 && is_word(line[first - 1]) {
        first -= 1;
    }
    let mut last = col;
    while last + 1 < line.len() && is_word(line[last + 1]) {
        last += 1;
    }
    (first, last)
}

impl Selection {
    /// A selection of exactly `[anchor, head]` (either order).
    pub fn new(anchor: Point, head: Point) -> Self {
        Self { anchor, head }
    }

    /// The word under `at` on a line whose characters are `line`.
    pub fn word(line: &[char], at: Point) -> Self {
        let (first, last) = word_at(line, at.col);
        Self::new(Point::new(at.line, first), Point::new(at.line, last))
    }

    /// The first and last selected points in reading order.
    pub fn range(&self) -> (Point, Point) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn start(&self) -> Point {
        self.range().0
    }

    pub fn end(&self) -> Point {
        self.range().1
    }

    /// Whether `p` lies in the selection (`cols` is the grid width, so a
    /// middle line is selected edge to edge).
    pub fn contains(&self, p: Point) -> bool {
        let (start, end) = self.range();
        start <= p && p <= end
    }

    /// Moves one handle to `to`. Dragging a handle past the other swaps
    /// their roles, as on a phone; the returned handle is the one now under
    /// the finger.
    pub fn drag(&mut self, handle: Handle, to: Point) -> Handle {
        let (start, end) = self.range();
        let (fixed, moved) = match handle {
            Handle::Start => (end, to),
            Handle::End => (start, to),
        };
        self.anchor = fixed;
        self.head = moved;
        if moved < fixed {
            Handle::Start
        } else {
            Handle::End
        }
    }

    /// Extends the head while a long-press drag continues (the anchor stays
    /// where the press began).
    pub fn extend(&mut self, to: Point) {
        self.head = to;
    }

    /// Follows text that scrolled `lines` further into history.
    pub fn shift(&mut self, lines: i32) {
        self.anchor.line -= lines;
        self.head.line -= lines;
    }

    /// Whether every selected line is still in the buffer whose oldest line
    /// is `-(history)`.
    pub fn in_buffer(&self, history: usize) -> bool {
        self.start().line >= -(history as i32)
    }

    /// Number of selected cells, for evidence (never the text).
    pub fn cells(&self, cols: usize) -> usize {
        let (start, end) = self.range();
        if start.line == end.line {
            return end.col.saturating_sub(start.col) + 1;
        }
        let middle = (end.line - start.line - 1).max(0) as usize * cols;
        cols.saturating_sub(start.col) + middle + end.col + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn long_press_selects_the_word_under_the_finger() {
        let line = chars("root@ferrix:~# cat \"/etc/hosts\" | wc -l");
        let at = |col| Selection::word(&line, Point::new(3, col));
        assert_eq!(at(2).range(), (Point::new(3, 0), Point::new(3, 13)));
        let path = at(24);
        assert_eq!(path.range(), (Point::new(3, 20), Point::new(3, 29)));
        assert_eq!(path.cells(80), 10);
        // A delimiter or blank selects just that cell; past the end too.
        assert_eq!(at(14).range(), (Point::new(3, 14), Point::new(3, 14)));
        assert_eq!(at(70).range(), (Point::new(3, 70), Point::new(3, 70)));
        assert_eq!(word_at(&[], 0), (0, 0));
    }

    #[test]
    fn handles_move_their_end_and_swap_when_crossed() {
        let mut s = Selection::new(Point::new(0, 5), Point::new(0, 9));
        assert_eq!(s.drag(Handle::End, Point::new(2, 3)), Handle::End);
        assert_eq!(s.range(), (Point::new(0, 5), Point::new(2, 3)));
        assert!(s.contains(Point::new(1, 0)) && s.contains(Point::new(1, 79)));
        assert!(!s.contains(Point::new(0, 4)) && !s.contains(Point::new(2, 4)));
        assert_eq!(s.cells(80), 75 + 80 + 4);
        assert_eq!(s.drag(Handle::Start, Point::new(-1, 0)), Handle::Start);
        assert_eq!(s.range(), (Point::new(-1, 0), Point::new(2, 3)));
        // Dragging the start past the end: the finger now holds the end.
        assert_eq!(s.drag(Handle::Start, Point::new(4, 0)), Handle::End);
        assert_eq!(s.range(), (Point::new(2, 3), Point::new(4, 0)));
        // A long-press drag extends from the anchor in either direction.
        let mut s = Selection::new(Point::new(5, 5), Point::new(5, 5));
        s.extend(Point::new(4, 2));
        assert_eq!(s.range(), (Point::new(4, 2), Point::new(5, 5)));
    }

    #[test]
    fn viewport_mapping_and_scroll_following() {
        let p = Point::from_viewport(3, 7, 10);
        assert_eq!(p, Point::new(-7, 7));
        assert_eq!(p.viewport_row(10, 30), Some(3));
        assert_eq!(p.viewport_row(0, 30), None, "scrolled out of view");
        let mut s = Selection::new(Point::new(2, 0), Point::new(3, 4));
        s.shift(2);
        assert_eq!(s.range(), (Point::new(0, 0), Point::new(1, 4)));
        assert!(s.in_buffer(0));
        s.shift(5);
        assert!(s.in_buffer(5) && !s.in_buffer(4));
    }
}
