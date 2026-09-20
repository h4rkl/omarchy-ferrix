//! Omarchy Ferrix's shared software-rendering toolkit for the reference surfaces.
//!
//! - [`draw`]: a thin `tiny-skia` + `fontdue` canvas with the embedded
//!   JetBrains Mono faces.
//! - [`theme`]: the Ferrix palette (slate surfaces, steel type, rust accent)
//!   and the `colors.toml` loader behind [`theme::get`].
//! - [`osk`] and [`keymap`]: the touch keyboard geometry and the evdev keymap
//!   shared by hit testing, rendering, and terminal input.
//! - [`widgets`] and [`gesture`]: shared chips, rows, fields, keypad, and
//!   keyboard drawing plus the touch tracker every app surface reuses.
//! - [`evidence`]: single-line JSON evidence records on stdout.
//! - [`client`] (Linux only): a fullscreen `smithay-client-toolkit` runner
//!   that owns the Wayland boilerplate so each surface only implements
//!   [`client::ClientHandler`].
//!
//! Software rendering is intentional: acceleration is never required for
//! correctness. Nothing here knows about QEMU, kernels, or boards.

pub mod draw;
pub mod evidence;
pub mod gesture;
pub mod keymap;
pub mod osk;
pub mod theme;
pub mod widgets;

#[cfg(target_os = "linux")]
pub mod client;
