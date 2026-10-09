//! Terminal emulation for the Terminal app.
//!
//! `alacritty_terminal` (the VT parser, grid, and PTY layer behind Alacritty,
//! Apache-2.0) runs a real shell and owns the escape-sequence state machine.
//! This module only spawns it, forwards input, and flattens the visible grid
//! into a plain [`Snapshot`] for the software renderer. Cell colors resolve
//! through the active [`theme`] (`colors.toml` or the built-in Ferrix palette):
//! ANSI 0-15 come from `Theme::ansi`, the default foreground and background
//! from `fg` and `bg`. The snapshot keeps rendering and its tests independent
//! of any PTY.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line, Point as TermPoint};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term, TermMode, point_to_viewport, test::TermSize};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use ferrix_ui::theme;

use crate::selection::Selection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: u32,
    /// `None` when the cell uses the default background.
    pub bg: Option<u32>,
    pub bold: bool,
}

impl Cell {
    /// An empty cell in the default foreground on the default background.
    pub fn blank() -> Self {
        Self {
            c: ' ',
            fg: theme::get().fg,
            bg: None,
            bold: false,
        }
    }
}

/// Everything the renderer needs for one frame of the terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Cell>,
    /// Cursor `(column, row)` in the viewport, if visible.
    pub cursor: Option<(usize, usize)>,
    /// Lines scrolled back into history (the display offset).
    pub scrollback: usize,
    /// Lines stored in history above the live screen.
    pub history: usize,
    /// The OSC 0/2 window title the program set, if any.
    pub title: Option<String>,
    /// Bracketed paste mode (`CSI ? 2004 h`): pastes are fenced.
    pub bracketed_paste: bool,
    /// DECCKM application cursor mode (arrow keys send SS3 sequences).
    pub app_cursor: bool,
    /// Set once the shell has exited (or failed to start), with the reason.
    pub exited: Option<String>,
}

impl Snapshot {
    /// Snapshot for a session that could not start.
    pub fn failed(reason: String) -> Self {
        Self {
            cols: 0,
            rows: 0,
            cells: Vec::new(),
            cursor: None,
            scrollback: 0,
            history: 0,
            title: None,
            bracketed_paste: false,
            app_cursor: false,
            exited: Some(reason),
        }
    }

    /// The characters of one viewport row (for word selection).
    pub fn row_chars(&self, row: usize) -> Vec<char> {
        (0..self.cols)
            .filter_map(|col| self.cell(col, row))
            .map(|cell| cell.c)
            .collect()
    }

    pub fn cell(&self, col: usize, row: usize) -> Option<&Cell> {
        (col < self.cols && row < self.rows).then(|| &self.cells[row * self.cols + col])
    }

    /// Text of one viewport row with trailing blanks removed.
    pub fn line_text(&self, row: usize) -> String {
        let mut text: String = (0..self.cols)
            .filter_map(|col| self.cell(col, row))
            .map(|cell| cell.c)
            .collect();
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
        text
    }

    /// First non-empty viewport line, used as the machine-readable readiness
    /// evidence that the shell actually wrote to the terminal.
    pub fn first_text_line(&self) -> Option<String> {
        (0..self.rows)
            .map(|row| self.line_text(row))
            .find(|line| !line.is_empty())
    }

    pub fn from_term<T: EventListener>(term: &Term<T>) -> Self {
        let content = term.renderable_content();
        let cols = term.columns();
        let rows = term.screen_lines();
        let default_bg = theme::get().bg;
        let mut cells = vec![Cell::blank(); cols * rows];
        for indexed in content.display_iter {
            let Some(point) = point_to_viewport(content.display_offset, indexed.point) else {
                continue;
            };
            let (col, row) = (point.column.0, point.line);
            if col >= cols || row >= rows {
                continue;
            }
            let cell = indexed.cell;
            let flags = cell.flags;
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let mut fg = rgb(cell.fg, content.colors);
            let mut bg = rgb(cell.bg, content.colors);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::DIM) {
                fg = mix(fg, bg, 0.5);
            }
            cells[row * cols + col] = Cell {
                c: if flags.contains(Flags::HIDDEN) {
                    ' '
                } else {
                    cell.c
                },
                fg,
                bg: (bg != default_bg).then_some(bg),
                bold: flags.contains(Flags::BOLD),
            };
        }
        let cursor = (content.cursor.shape != CursorShape::Hidden)
            .then(|| point_to_viewport(content.display_offset, content.cursor.point))
            .flatten()
            .map(|point| (point.column.0, point.line));
        Self {
            cols,
            rows,
            cells,
            cursor,
            scrollback: content.display_offset,
            history: term.history_size(),
            title: None,
            bracketed_paste: content.mode.contains(TermMode::BRACKETED_PASTE),
            app_cursor: content.mode.contains(TermMode::APP_CURSOR),
            exited: None,
        }
    }

    /// Feeds raw terminal output through the emulator without a PTY. Used by
    /// the host preview and by deterministic rendering tests.
    pub fn from_bytes(cols: usize, rows: usize, bytes: &[u8]) -> Self {
        Self::from_term(&term_from_bytes(cols, rows, bytes))
    }
}

/// The text of `selection` in `term`, clamped to the stored buffer. Lines
/// that wrapped join without a newline; trailing blanks are dropped, as
/// Alacritty copies.
pub fn selection_text<T: EventListener>(term: &Term<T>, selection: &Selection) -> String {
    let (start, end) = selection.range();
    let top = -(term.history_size() as i32);
    let bottom = term.screen_lines() as i32 - 1;
    let last_col = term.columns().saturating_sub(1);
    if end.line < top || start.line > bottom {
        return String::new();
    }
    let clamp = |line: i32, col: usize| {
        TermPoint::new(Line(line.clamp(top, bottom)), Column(col.min(last_col)))
    };
    let start = if start.line < top {
        clamp(top, 0)
    } else {
        clamp(start.line, start.col)
    };
    let end = if end.line > bottom {
        clamp(bottom, last_col)
    } else {
        clamp(end.line, end.col)
    };
    term.bounds_to_string(start, end)
}

/// Builds a terminal from raw output without a PTY (tests and previews).
pub fn term_from_bytes(
    cols: usize,
    rows: usize,
    bytes: &[u8],
) -> Term<alacritty_terminal::event::VoidListener> {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
    let config = Config {
        scrolling_history: 2000,
        ..Config::default()
    };
    let mut term = Term::new(
        config,
        &TermSize::new(cols.max(2), rows.max(1)),
        VoidListener,
    );
    let mut processor = Processor::<StdSyncHandler>::new();
    processor.advance(&mut term, bytes);
    term
}

fn pack(rgb: Rgb) -> u32 {
    u32::from(rgb.r) << 16 | u32::from(rgb.g) << 8 | u32::from(rgb.b)
}

pub fn unpack(color: u32) -> Rgb {
    Rgb {
        r: (color >> 16) as u8,
        g: (color >> 8) as u8,
        b: color as u8,
    }
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let x = (a >> shift & 0xff) as f32;
        let y = (b >> shift & 0xff) as f32;
        ((x * (1.0 - t) + y * t).round() as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
}

/// Resolves a cell color: dynamic (OSC) overrides first, then the theme.
fn rgb(color: Color, colors: &Colors) -> u32 {
    match color {
        Color::Spec(rgb) => pack(rgb),
        Color::Named(named) => colors[named]
            .map(pack)
            .unwrap_or_else(|| named_default(named)),
        Color::Indexed(index) => colors[usize::from(index)]
            .map(pack)
            .unwrap_or_else(|| indexed_default(index)),
    }
}

fn named_default(named: NamedColor) -> u32 {
    let t = theme::get();
    let index = named as usize;
    if index < t.ansi.len() {
        return t.ansi[index];
    }
    match named {
        NamedColor::Background => t.bg,
        // The block cursor is drawn by the renderer in the accent; the cell
        // color a program asks for stays plain foreground.
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => t.fg,
        NamedColor::DimForeground => t.fg_dim,
        NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => mix(t.ansi[index - NamedColor::DimBlack as usize], t.bg, 0.4),
        _ => t.fg,
    }
}

/// xterm 256-color defaults: the theme's ANSI palette, the 6x6x6 cube, then
/// grays.
pub fn indexed_default(index: u8) -> u32 {
    match index {
        0..=15 => theme::get().ansi[usize::from(index)],
        16..=231 => {
            let i = u32::from(index - 16);
            let level = |x: u32| if x == 0 { 0 } else { 55 + 40 * x };
            level(i / 36) << 16 | level(i % 36 / 6) << 8 | level(i % 6)
        }
        232..=255 => {
            let v = 8 + 10 * u32::from(index - 232);
            v << 16 | v << 8 | v
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(target_os = "linux"), allow(unused_imports))]
pub use session::{Geometry, Session};

#[cfg(unix)]
mod session {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::mpsc;

    use alacritty_terminal::event::{Event, EventListener, Notify as _, OnResize as _, WindowSize};
    use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
    use alacritty_terminal::grid::Scroll;
    use alacritty_terminal::sync::FairMutex;
    use alacritty_terminal::term::{Config, Term, test::TermSize};
    use alacritty_terminal::tty;

    use super::{Snapshot, indexed_default, unpack};
    use crate::selection::Selection;
    use ferrix_ui::theme;

    /// Grid size in cells plus the cell size in pixels (reported to the PTY
    /// as the window size).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Geometry {
        pub cols: u16,
        pub rows: u16,
        pub cell_w: u16,
        pub cell_h: u16,
    }

    impl Geometry {
        fn window_size(self) -> WindowSize {
            WindowSize {
                num_lines: self.rows.max(1),
                num_cols: self.cols.max(2),
                cell_width: self.cell_w,
                cell_height: self.cell_h,
            }
        }

        fn term_size(self) -> TermSize {
            TermSize::new(usize::from(self.cols.max(2)), usize::from(self.rows.max(1)))
        }
    }

    /// Forwards emulator events to the UI thread and wakes its event loop.
    #[derive(Clone)]
    struct Listener {
        events: mpsc::Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    }

    impl EventListener for Listener {
        fn send_event(&self, event: Event) {
            let _ = self.events.send(event);
            (self.wake)();
        }
    }

    /// A live shell on a PTY, driven by alacritty's reader thread.
    pub struct Session {
        term: Arc<FairMutex<Term<Listener>>>,
        notifier: Notifier,
        events: mpsc::Receiver<Event>,
        geometry: Geometry,
        exited: Option<String>,
        program: String,
        title: Option<String>,
        bells: u32,
    }

    impl Session {
        /// Spawns `program` on a new PTY. `wake` is invoked from the reader
        /// thread whenever the UI should call [`Session::pump`].
        pub fn spawn(
            program: &str,
            args: &[String],
            geometry: Geometry,
            wake: Arc<dyn Fn() + Send + Sync>,
        ) -> Result<Self, String> {
            let mut env = HashMap::new();
            env.insert("TERM".to_owned(), "xterm-256color".to_owned());
            env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
            env.insert("PS1".to_owned(), "\\u@\\h:\\w\\$ ".to_owned());
            let options = tty::Options {
                shell: Some(tty::Shell::new(program.to_owned(), args.to_vec())),
                working_directory: std::env::var_os("HOME").map(PathBuf::from),
                drain_on_exit: true,
                env,
            };
            let pty = tty::new(&options, geometry.window_size(), 0)
                .map_err(|e| format!("pty for {program}: {e}"))?;
            let (tx, rx) = mpsc::channel();
            let listener = Listener { events: tx, wake };
            let config = Config {
                scrolling_history: 2000,
                ..Config::default()
            };
            let term = Arc::new(FairMutex::new(Term::new(
                config,
                &geometry.term_size(),
                listener.clone(),
            )));
            let event_loop = EventLoop::new(Arc::clone(&term), listener, pty, true, false)
                .map_err(|e| format!("terminal io loop: {e}"))?;
            let notifier = Notifier(event_loop.channel());
            let _reader = event_loop.spawn();
            Ok(Self {
                term,
                notifier,
                events: rx,
                geometry,
                exited: None,
                program: program.to_owned(),
                title: None,
                bells: 0,
            })
        }

        pub fn program(&self) -> &str {
            &self.program
        }

        pub fn exited(&self) -> Option<&str> {
            self.exited.as_deref()
        }

        /// Queues bytes for the shell's stdin.
        pub fn write(&self, bytes: Vec<u8>) {
            if self.exited.is_none() {
                self.notifier.notify(bytes);
            }
        }

        pub fn resize(&mut self, geometry: Geometry) {
            if geometry == self.geometry {
                return;
            }
            self.geometry = geometry;
            self.term.lock().resize(geometry.term_size());
            self.notifier.on_resize(geometry.window_size());
        }

        /// Scrolls the viewport; positive values move into history.
        pub fn scroll(&self, lines: i32) {
            self.term.lock().scroll_display(Scroll::Delta(lines));
        }

        /// Scrolls the viewport to an absolute display offset (0 is live).
        pub fn scroll_to(&self, offset: usize) {
            let mut term = self.term.lock();
            let current = term.grid().display_offset() as i32;
            let target = i32::try_from(offset).unwrap_or(i32::MAX);
            term.scroll_display(Scroll::Delta(target - current));
        }

        /// Drains emulator events. Returns `true` when anything arrived, which
        /// means the visible content may have changed.
        pub fn pump(&mut self) -> bool {
            let mut changed = false;
            while let Ok(event) = self.events.try_recv() {
                changed = true;
                match event {
                    Event::PtyWrite(text) => self.notifier.notify(text.into_bytes()),
                    Event::ChildExit(status) => {
                        self.exited = Some(match status.code() {
                            Some(code) => format!("exit code {code}"),
                            None => "terminated by signal".to_owned(),
                        });
                    }
                    Event::Exit => {
                        if self.exited.is_none() {
                            self.exited = Some("exited".to_owned());
                        }
                    }
                    Event::TextAreaSizeRequest(format) => {
                        self.notifier
                            .notify(format(self.geometry.window_size()).into_bytes());
                    }
                    Event::Title(title) => {
                        let title: String = title.chars().filter(|c| !c.is_control()).collect();
                        self.title = (!title.trim().is_empty()).then_some(title);
                    }
                    Event::ResetTitle => self.title = None,
                    Event::Bell => self.bells = self.bells.saturating_add(1),
                    Event::ColorRequest(index, format) => {
                        let color = u8::try_from(index)
                            .map(indexed_default)
                            .unwrap_or(theme::get().fg);
                        self.notifier.notify(format(unpack(color)).into_bytes());
                    }
                    _ => {}
                }
            }
            changed
        }

        pub fn snapshot(&self) -> Snapshot {
            let mut snapshot = Snapshot::from_term(&self.term.lock());
            snapshot.exited = self.exited.clone();
            snapshot.title = self.title.clone();
            snapshot
        }

        /// The OSC 0/2 title, if the program set one.
        pub fn title(&self) -> Option<&str> {
            self.title.as_deref()
        }

        /// Bells rung since the last call.
        pub fn take_bells(&mut self) -> u32 {
            std::mem::take(&mut self.bells)
        }

        /// The selected text.
        pub fn selection_text(&self, selection: &Selection) -> String {
            super::selection_text(&self.term.lock(), selection)
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.notifier.0.send(Msg::Shutdown);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_maps_text_colors_and_cursor() {
        let s = Snapshot::from_bytes(20, 4, b"hi \x1b[31mred\x1b[0m\r\n\x1b[1;44m B \x1b[0m");
        assert_eq!((s.cols, s.rows), (20, 4));
        assert_eq!(s.line_text(0), "hi red");
        let t = theme::get();
        assert_eq!(s.cell(0, 0).unwrap().fg, t.fg);
        assert_eq!(s.cell(3, 0).unwrap().fg, t.ansi[1]);
        let b = s.cell(1, 1).unwrap();
        assert_eq!(b.c, 'B');
        assert!(b.bold);
        assert_eq!(b.bg, Some(t.ansi[4]));
        assert_eq!(s.cursor, Some((3, 1)));
        assert_eq!(s.first_text_line().as_deref(), Some("hi red"));
        assert!(!s.app_cursor);
        let app = Snapshot::from_bytes(10, 2, b"\x1b[?1h");
        assert!(app.app_cursor);
    }

    #[test]
    fn selection_text_follows_buffer_lines_and_history() {
        use crate::selection::{Point, Selection};
        let mut bytes = Vec::new();
        for i in 0..8 {
            bytes.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        bytes.extend_from_slice(b"$ echo hello world");
        let term = term_from_bytes(20, 4, &bytes);
        let snap = Snapshot::from_term(&term);
        assert_eq!(snap.history, 5, "9 lines on a 4-row screen");
        assert_eq!(snap.line_text(3), "$ echo hello world");
        let word = Selection::word(&snap.row_chars(3), Point::new(3, 9));
        assert_eq!(selection_text(&term, &word), "hello");
        // Across lines, from history into the screen; trailing blanks drop.
        let across = Selection::new(Point::new(-1, 0), Point::new(1, 5));
        assert_eq!(selection_text(&term, &across), "line 4\nline 5\nline 6");
        // Points outside the buffer are clamped, or yield nothing.
        let all = Selection::new(Point::new(-50, 0), Point::new(50, 99));
        assert!(selection_text(&term, &all).starts_with("line 0\n"));
        assert!(selection_text(&term, &all).ends_with("hello world"));
        let gone = Selection::new(Point::new(-90, 0), Point::new(-80, 3));
        assert_eq!(selection_text(&term, &gone), "");
    }

    #[test]
    fn indexed_defaults_cover_cube_and_grays() {
        assert_eq!(indexed_default(1), theme::get().ansi[1]);
        assert_eq!(indexed_default(16), 0x000000);
        assert_eq!(indexed_default(231), 0xffffff);
        assert_eq!(indexed_default(232), 0x080808);
        assert_eq!(indexed_default(255), 0xeeeeee);
    }

    #[cfg(unix)]
    #[test]
    fn session_runs_a_real_shell_and_reports_exit() {
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        let geometry = Geometry {
            cols: 40,
            rows: 5,
            cell_w: 18,
            cell_h: 39,
        };
        let mut session = Session::spawn(
            "/bin/sh",
            &["-c".to_owned(), "printf 'terminal-ok'; exit 7".to_owned()],
            geometry,
            Arc::new(|| {}),
        )
        .expect("spawn /bin/sh on a pty");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            session.pump();
            let snapshot = session.snapshot();
            if snapshot.exited.is_some() && snapshot.line_text(0).contains("terminal-ok") {
                assert_eq!(snapshot.exited.as_deref(), Some("exit code 7"));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "shell output did not arrive: {snapshot:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
