//! Omarchy Ferrix's shared software-rendering toolkit for the reference surfaces.
//!
//! - [`draw`]: a thin `tiny-skia` + `fontdue` canvas with the embedded
//!   JetBrains Mono faces.
//! - [`theme`]: the Ferrix palette (black surfaces, amber type, rust accent)
//!   and the `colors.toml` loader behind [`theme::get`].
//! - [`osk`] and [`keymap`]: the touch keyboard geometry and the evdev keymap
//!   shared by hit testing, rendering, and terminal input.
//! - [`keyboard`]: the data-driven letter layouts, the per-user
//!   `keyboard.toml` (layout, repeat timing), and the key-repeat timer.
//! - [`widgets`] and [`gesture`]: shared chips, rows, fields, keypad, and
//!   keyboard drawing plus the touch tracker every app surface reuses.
//! - [`fling`]: release-velocity estimation and the decelerating glide for
//!   touch-scrolled lists (time-injected, no Wayland).
//! - [`icons`]: the pinned Google Material Symbols (Apache-2.0) glyph subset,
//!   compiled to outlines at build time, rasterised once per pixel size and
//!   tinted with theme colours.
//! - [`evidence`]: single-line JSON evidence records on stdout and the
//!   session evidence label.
//! - [`clock`]: the local wall-clock minute every status bar shows and a
//!   ticker that wakes a surface at each minute boundary.
//! - [`client`] (Linux only): a fullscreen `smithay-client-toolkit` runner
//!   that owns the Wayland boilerplate so each surface only implements
//!   [`client::ClientHandler`].
//!
//! Software rendering is intentional: acceleration is never required for
//! correctness. Nothing here knows about QEMU, kernels, or boards.

pub mod clock;
pub mod draw;
pub mod evidence;
pub mod fling;
pub mod gesture;
pub mod icons;
pub mod keyboard;
pub mod keymap;
pub mod osk;
pub mod theme;
pub mod widgets;

#[cfg(target_os = "linux")]
pub mod client;
