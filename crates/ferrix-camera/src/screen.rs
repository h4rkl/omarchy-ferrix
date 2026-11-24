//! Camera surface: a fullscreen viewfinder under the shared clock strip,
//! one control band (gallery, shutter, front/back) above the compositor's
//! gesture zone, and honest full pages when there is no camera to show.
//!
//! The shutter is the region's one rust action. The facing chip over the
//! viewfinder is teal information; a mocked capability says `MOCK` there,
//! so a synthetic frame is never mistaken for a captured one.

use ferrix_hardware_api::CapabilityStatus;
use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::icons::{self, Icon};
use ferrix_ui::theme;
use ferrix_ui::widgets::{self, ButtonKind, Tone, basis, hairline, scale};

use crate::model::{Model, Status};

/// Hit targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Shutter,
    Switch,
    Gallery,
    Retry,
    Back,
}

/// Every rectangle the surface draws or hit-tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub viewfinder: Rect,
    pub shutter: Rect,
    pub switch: Rect,
    pub gallery: Rect,
    pub retry: Rect,
    /// Baseline of the status line between viewfinder and controls.
    pub status_baseline: i32,
}

pub fn layout(size: (u32, u32)) -> Layout {
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    let (w, h) = (size.0 as i32, size.1 as i32);
    let top = widgets::clock_strip_height(b);
    // The bottom 8 % belongs to the compositor; the control band sits
    // just above it.
    let zone = h * 92 / 100;
    let band_center = zone - s(150);
    let shutter_d = s(176);
    let side = s(132);
    let viewfinder_bottom = band_center - shutter_d / 2 - s(110);
    Layout {
        viewfinder: Rect {
            x: 0,
            y: top,
            w,
            h: viewfinder_bottom - top,
        },
        shutter: Rect {
            x: (w - shutter_d) / 2,
            y: band_center - shutter_d / 2,
            w: shutter_d,
            h: shutter_d,
        },
        gallery: Rect {
            x: s(96),
            y: band_center - side / 2,
            w: side,
            h: side,
        },
        switch: Rect {
            x: w - s(96) - s(200),
            y: band_center - side / 2,
            w: s(200),
            h: side,
        },
        retry: Rect {
            x: s(72),
            y: s(1180),
            w: w - s(144),
            h: s(132),
        },
        status_baseline: viewfinder_bottom + s(70),
    }
}

/// Which target `position` hits for the current state.
pub fn hit_test(model: &Model, size: (u32, u32), position: (f64, f64)) -> Option<Target> {
    let l = layout(size);
    if model.status == Status::Live {
        [
            (l.shutter, Target::Shutter),
            (l.gallery, Target::Gallery),
            (l.switch, Target::Switch),
        ]
        .into_iter()
        .find(|(r, _)| widgets::contains(*r, position))
        .map(|(_, t)| t)
    } else if widgets::contains(widgets::back_rect(size), position) {
        Some(Target::Back)
    } else if retryable(&model.status) && widgets::contains(l.retry, position) {
        Some(Target::Retry)
    } else {
        None
    }
}

fn retryable(status: &Status) -> bool {
    matches!(status, Status::Unavailable(_) | Status::Failed(_))
}

/// The strip's right label: whether these are mock frames.
fn status_word(model: &Model) -> Option<&'static str> {
    model.capability.map(|c| match c {
        CapabilityStatus::Mocked => "mock camera",
        CapabilityStatus::Supported => "camera",
        CapabilityStatus::Unsupported => "no camera",
    })
}

pub fn compose(
    ty: &Type,
    size: (u32, u32),
    minute_of_day: u32,
    model: &Model,
    pressed: Option<Target>,
) -> Canvas {
    let t = theme::get();
    let mut c = Canvas::new(size.0 as i32, size.1 as i32, t.bg);
    let clock = ferrix_ui::clock::hhmm(minute_of_day);
    widgets::clock_strip(&mut c, ty, &clock, status_word(model));
    if model.status == Status::Live {
        viewfinder(&mut c, ty, size, model, pressed);
    } else {
        message_page(&mut c, ty, size, model, pressed);
    }
    widgets::gesture_pill(&mut c, false);
    c
}

/// Aspect-fit rectangle of a `w`x`h` image centred in `area`.
fn fit(area: Rect, w: u32, h: u32) -> Rect {
    let (aw, ah) = (i64::from(area.w), i64::from(area.h));
    let (w, h) = (i64::from(w.max(1)), i64::from(h.max(1)));
    let (fw, fh) = if aw * h <= ah * w {
        (aw, aw * h / w)
    } else {
        (ah * w / h, ah)
    };
    Rect {
        x: area.x + ((aw - fw) / 2) as i32,
        y: area.y + ((ah - fh) / 2) as i32,
        w: fw as i32,
        h: fh as i32,
    }
}

fn viewfinder(c: &mut Canvas, ty: &Type, size: (u32, u32), model: &Model, pressed: Option<Target>) {
    let t = theme::get();
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    let l = layout(size);
    c.fill_rect(l.viewfinder, t.bg_dark);
    let image = match &model.frame {
        Some(frame) => {
            let dest = fit(l.viewfinder, frame.width, frame.height);
            c.draw_rgba(dest, frame.width, frame.height, &frame.rgba);
            dest
        }
        None => {
            let text = "Starting preview\u{2026}";
            let style = Style::regular(s(32) as f32, t.fg_dim);
            let w = ty.width(style, text);
            ty.text(
                c,
                style,
                text,
                (c.width() - w) / 2,
                l.viewfinder.y + l.viewfinder.h / 2,
            );
            l.viewfinder
        }
    };
    // Facing (and mock) chip, teal information over the image's corner.
    if let Some(camera) = model.active_camera() {
        let mut label = camera.facing.name().to_uppercase();
        if model.capability == Some(CapabilityStatus::Mocked) {
            label.push_str(" \u{b7} MOCK");
        }
        let chip = Rect {
            x: image.x + s(32),
            y: image.y + s(32),
            w: s(260),
            h: s(64),
        };
        widgets::chip(c, ty, chip, &label, Tone::Steel, false);
    }
    // Status line: the last save or failure, else the frame count.
    let (text, color) = match &model.notice {
        Some((text, tone)) => (text.clone(), tone.color()),
        None if model.capturing => ("Saving\u{2026}".to_owned(), t.fg_dim),
        None => (
            match model.active_camera() {
                Some(cam) => format!(
                    "{} \u{b7} {}\u{b0} \u{b7} frame {}",
                    cam.preview,
                    cam.rotation.degrees(),
                    model.frames
                ),
                None => String::new(),
            },
            t.fg_dim,
        ),
    };
    let style = Style::regular(s(28) as f32, color);
    let text = widgets::ellipsize(ty, style, &text, c.width() - s(96));
    let w = ty.width(style, &text);
    ty.text(c, style, &text, (c.width() - w) / 2, l.status_baseline);

    // Gallery: the hand-off to Files.
    widgets::icon_button(
        c,
        l.gallery,
        Icon::FileImage,
        ButtonKind::Secondary,
        pressed == Some(Target::Gallery),
        true,
    );
    // Shutter: a pale ring around the rust action.
    let enabled = model.can_capture();
    let cx = l.shutter.x + l.shutter.w / 2;
    let cy = l.shutter.y + l.shutter.h / 2;
    let r = l.shutter.w / 2;
    c.fill_circle(cx, cy, r, if enabled { t.fg } else { t.border });
    c.fill_circle(cx, cy, r - s(10), t.bg);
    let inner = if !enabled {
        t.surface_hi
    } else if pressed == Some(Target::Shutter) || model.capturing {
        t.rust_hi
    } else {
        t.rust
    };
    c.fill_circle(cx, cy, r - s(22), inner);
    icons::draw_centered(c, Icon::Camera, cx, cy, s(64), t.bg);
    // Front/back: names the camera it switches to.
    let next = model
        .next_camera()
        .map(|cam| cam.facing.name().to_uppercase());
    widgets::button(
        c,
        ty,
        l.switch,
        next.as_deref().unwrap_or("ONLY"),
        ButtonKind::Secondary,
        pressed == Some(Target::Switch),
        next.is_some(),
    );
    // One instrument rail ties the controls to the viewfinder.
    c.fill_rect(
        Rect {
            x: s(48),
            y: l.viewfinder.y + l.viewfinder.h,
            w: c.width() - s(96),
            h: hairline(b),
        },
        t.rust_dim,
    );
}

/// The full page shown whenever there is no preview to show.
fn message_page(
    c: &mut Canvas,
    ty: &Type,
    size: (u32, u32),
    model: &Model,
    pressed: Option<Target>,
) {
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    widgets::page_header(c, ty, "Camera", None, pressed == Some(Target::Back));
    let (title, tone, detail): (&str, Tone, String) = match &model.status {
        Status::Connecting => (
            "Connecting to the camera",
            Tone::Neutral,
            "Waiting for the camera service.".to_owned(),
        ),
        Status::Unsupported => (
            "No camera on this target",
            Tone::Neutral,
            "This target's capability matrix lists camera as unsupported. \
             Ferrix has no camera adapter for it, so nothing is shown rather \
             than an imitation."
                .to_owned(),
        ),
        Status::Unauthorized => (
            "Camera access is not granted",
            Tone::Warn,
            "The grants policy gives Camera no camera grant, so the camera \
             service refuses every request."
                .to_owned(),
        ),
        Status::Unavailable(reason) => (
            "Camera service unavailable",
            Tone::Warn,
            format!("The camera socket could not be reached: {reason}"),
        ),
        Status::NoCameras => (
            "No cameras listed",
            Tone::Neutral,
            "The camera capability is present but reports no cameras.".to_owned(),
        ),
        Status::Failed(reason) => ("Camera failed", Tone::Err, reason.clone()),
        Status::Live => unreachable!("live renders the viewfinder"),
    };
    let top = widgets::header_bottom(b) + s(96);
    widgets::notice(
        c,
        ty,
        Rect {
            x: s(48),
            y: top,
            w: c.width() - s(96),
            h: s(112),
        },
        title,
        tone,
    );
    widgets::wrap_text(
        c,
        ty,
        &detail,
        s(72),
        top + s(200),
        c.width() - s(144),
        s(30),
    );
    if retryable(&model.status) {
        widgets::button(
            c,
            ty,
            layout(size).retry,
            "Retry",
            ButtonKind::Primary,
            pressed == Some(Target::Retry),
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Event, tests::info};
    use ferrix_hardware_api::{CameraFacing, CameraRotation};

    const SIZE: (u32, u32) = (1080, 2340);

    #[test]
    fn controls_stay_above_the_compositor_zone_and_targets_hit() {
        let l = layout(SIZE);
        let zone = (f64::from(SIZE.1) * ferrix_ui::gesture::HOME_ZONE) as i32;
        for r in [l.shutter, l.gallery, l.switch] {
            assert!(r.y + r.h < zone, "{r:?} enters the gesture zone");
            assert!(r.w >= 48 && r.h >= 48, "touch target {r:?}");
        }
        assert!(l.viewfinder.y + l.viewfinder.h < l.shutter.y);
        let mut m = Model::new();
        m.apply(Event::Cameras(Ok(vec![
            info("b", CameraFacing::Back, CameraRotation::Deg90),
            info("f", CameraFacing::Front, CameraRotation::Deg270),
        ])));
        assert_eq!(
            hit_test(&m, SIZE, widgets::center(l.shutter)),
            Some(Target::Shutter)
        );
        assert_eq!(
            hit_test(&m, SIZE, widgets::center(l.switch)),
            Some(Target::Switch)
        );
        assert_eq!(
            hit_test(&m, SIZE, widgets::center(l.gallery)),
            Some(Target::Gallery)
        );
        m.apply(Event::Unavailable("gone".into()));
        assert_eq!(
            hit_test(&m, SIZE, widgets::center(l.retry)),
            Some(Target::Retry)
        );
        assert_eq!(
            hit_test(&m, SIZE, widgets::center(widgets::back_rect(SIZE))),
            Some(Target::Back)
        );
    }

    #[test]
    fn fit_keeps_the_aspect_and_centres() {
        let area = Rect {
            x: 0,
            y: 100,
            w: 1080,
            h: 1600,
        };
        let r = fit(area, 360, 480);
        assert_eq!((r.w, r.h), (1080, 1440));
        assert_eq!(r.y, 180);
    }
}
