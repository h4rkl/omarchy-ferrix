//! Pure layout and focus logic shared by every backend.
//!
//! The product shell is portrait, touch-first and fullscreen-only: exactly one
//! toplevel (the newest mapped one) fills the output; there is no desktop
//! window management. Rotation is expressed as an output transform so clients
//! always see a logical portrait (or requested) surface.

// The compositor that consumes this policy is Linux-only; the policy itself
// (and its tests) build everywhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use hardware_api::Orientation;

/// Output rotation applied between the panel scan-out and client logical space.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Rotation {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
}

impl Rotation {
    pub fn label(self) -> &'static str {
        match self {
            Rotation::Normal => "normal",
            Rotation::Rotate90 => "90",
            Rotation::Rotate180 => "180",
            Rotation::Rotate270 => "270",
        }
    }

    /// Whether logical width/height are swapped relative to the panel.
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Rotate90 | Rotation::Rotate270)
    }

    /// Rotate clockwise by 90 degrees (the user turned the device).
    pub fn next_clockwise(self) -> Rotation {
        match self {
            Rotation::Normal => Rotation::Rotate90,
            Rotation::Rotate90 => Rotation::Rotate180,
            Rotation::Rotate180 => Rotation::Rotate270,
            Rotation::Rotate270 => Rotation::Normal,
        }
    }
}

/// Pinned output plan: physical mode plus the logical size clients see.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputLayout {
    pub panel_width: u32,
    pub panel_height: u32,
    pub rotation: Rotation,
}

impl OutputLayout {
    /// Choose a rotation so the logical output matches the requested
    /// orientation. A portrait profile on a portrait panel (the QEMU case) needs
    /// no transform; a landscape panel is rotated 90 degrees to present portrait.
    pub fn plan(panel_width: u32, panel_height: u32, orientation: Orientation) -> Self {
        let panel_is_portrait = panel_height >= panel_width;
        let rotation = match (panel_is_portrait, orientation) {
            (true, Orientation::Portrait) | (false, Orientation::Landscape) => Rotation::Normal,
            (true, Orientation::Landscape) | (false, Orientation::Portrait) => Rotation::Rotate90,
            (true, Orientation::PortraitFlipped) | (false, Orientation::LandscapeFlipped) => {
                Rotation::Rotate180
            }
            (true, Orientation::LandscapeFlipped) | (false, Orientation::PortraitFlipped) => {
                Rotation::Rotate270
            }
        };
        Self {
            panel_width,
            panel_height,
            rotation,
        }
    }

    pub fn with_rotation(self, rotation: Rotation) -> Self {
        Self { rotation, ..self }
    }

    /// Logical size presented to clients after the rotation.
    pub fn logical_size(&self) -> (u32, u32) {
        if self.rotation.swaps_axes() {
            (self.panel_height, self.panel_width)
        } else {
            (self.panel_width, self.panel_height)
        }
    }
}

/// Fullscreen-only toplevel stack. Newest mapped window is focused and drawn;
/// unmapped toplevels (no buffer yet) are tracked but never focused.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowStack<T: PartialEq + Clone> {
    entries: Vec<Entry<T>>,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry<T> {
    id: T,
    mapped: bool,
}

impl<T: PartialEq + Clone> WindowStack<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Track a new (not yet mapped) toplevel.
    pub fn push(&mut self, id: T) {
        if !self.contains(&id) {
            self.entries.push(Entry { id, mapped: false });
        }
    }

    pub fn contains(&self, id: &T) -> bool {
        self.entries.iter().any(|e| &e.id == id)
    }

    pub fn is_mapped(&self, id: &T) -> bool {
        self.entries.iter().any(|e| &e.id == id && e.mapped)
    }

    /// Mark a toplevel mapped (first buffer committed) and raise it. Returns
    /// `true` the first time it maps.
    pub fn mark_mapped(&mut self, id: &T) -> bool {
        let Some(index) = self.entries.iter().position(|e| &e.id == id) else {
            return false;
        };
        let was_mapped = self.entries[index].mapped;
        let mut entry = self.entries.remove(index);
        entry.mapped = true;
        self.entries.push(entry);
        !was_mapped
    }

    pub fn remove(&mut self, id: &T) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| &e.id != id);
        before != self.entries.len()
    }

    /// The single visible, focused toplevel: the newest mapped one.
    pub fn focused(&self) -> Option<&T> {
        self.entries.iter().rev().find(|e| e.mapped).map(|e| &e.id)
    }

    pub fn mapped_count(&self) -> usize {
        self.entries.iter().filter(|e| e.mapped).count()
    }

    /// All tracked toplevels, bottom to top.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|e| &e.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_profile_on_portrait_panel_needs_no_rotation() {
        let layout = OutputLayout::plan(1080, 2340, Orientation::Portrait);
        assert_eq!(layout.rotation, Rotation::Normal);
        assert_eq!(layout.logical_size(), (1080, 2340));
    }

    #[test]
    fn portrait_profile_on_landscape_panel_rotates() {
        let layout = OutputLayout::plan(1920, 1080, Orientation::Portrait);
        assert_eq!(layout.rotation, Rotation::Rotate90);
        assert_eq!(layout.logical_size(), (1080, 1920));
    }

    #[test]
    fn landscape_profile_on_portrait_panel_rotates() {
        let layout = OutputLayout::plan(1080, 2340, Orientation::Landscape);
        assert_eq!(layout.rotation, Rotation::Rotate90);
        assert_eq!(layout.logical_size(), (2340, 1080));
    }

    #[test]
    fn manual_rotation_cycles_and_swaps_axes() {
        let layout = OutputLayout::plan(1080, 2340, Orientation::Portrait);
        let turned = layout.with_rotation(layout.rotation.next_clockwise());
        assert_eq!(turned.rotation, Rotation::Rotate90);
        assert_eq!(turned.logical_size(), (2340, 1080));
        let back = (0..4).fold(layout.rotation, |r, _| r.next_clockwise());
        assert_eq!(back, Rotation::Normal);
        assert!(!Rotation::Rotate180.swaps_axes());
    }

    #[test]
    fn flipped_orientations_preserve_direction() {
        assert_eq!(
            OutputLayout::plan(1080, 2340, Orientation::PortraitFlipped).rotation,
            Rotation::Rotate180
        );
        assert_eq!(
            OutputLayout::plan(1080, 2340, Orientation::LandscapeFlipped).rotation,
            Rotation::Rotate270
        );
    }

    #[test]
    fn newest_mapped_window_is_focused() {
        let mut stack: WindowStack<u32> = WindowStack::new();
        assert_eq!(stack.focused(), None);
        stack.push(1);
        assert_eq!(
            stack.focused(),
            None,
            "unmapped toplevels are never focused"
        );
        assert!(stack.mark_mapped(&1));
        assert!(!stack.mark_mapped(&1), "mapping is reported once");
        assert_eq!(stack.focused(), Some(&1));
        stack.push(2);
        assert_eq!(stack.focused(), Some(&1));
        assert!(stack.mark_mapped(&2));
        assert_eq!(stack.focused(), Some(&2));
        assert_eq!(stack.mapped_count(), 2);
    }

    #[test]
    fn removing_the_top_window_falls_back_to_previous() {
        let mut stack: WindowStack<&'static str> = WindowStack::new();
        stack.push("launcher");
        stack.mark_mapped(&"launcher");
        stack.push("terminal");
        stack.mark_mapped(&"terminal");
        assert!(stack.remove(&"terminal"));
        assert!(!stack.remove(&"terminal"));
        assert_eq!(stack.focused(), Some(&"launcher"));
        assert!(stack.remove(&"launcher"));
        assert_eq!(stack.focused(), None);
        assert_eq!(stack.mapped_count(), 0);
    }

    #[test]
    fn push_is_idempotent_and_keeps_order() {
        let mut stack: WindowStack<u8> = WindowStack::new();
        stack.push(1);
        stack.push(2);
        stack.push(1);
        assert_eq!(stack.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
        stack.mark_mapped(&1);
        assert_eq!(
            stack.iter().copied().collect::<Vec<_>>(),
            vec![2, 1],
            "mapping raises"
        );
        assert!(stack.is_mapped(&1));
        assert!(!stack.is_mapped(&2));
        assert!(!stack.mark_mapped(&9), "unknown toplevels are ignored");
    }
}
