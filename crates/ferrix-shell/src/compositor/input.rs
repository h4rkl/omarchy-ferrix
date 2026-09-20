//! Seat input routing shared by the libinput (guest) and winit (nested)
//! backends: keyboard, absolute/relative pointer and touch.
//!
//! Absolute devices (QEMU's virtio-tablet is a libinput pointer with absolute
//! coordinates) are mapped through the output transform so rotation keeps
//! touch points aligned with what the client draws.
//!
//! Two things are intercepted before clients see them: F9 (debug rotation)
//! and the bottom-edge home gesture. A contact that begins in the edge zone
//! (`gesture::EdgeGesture`) is grabbed for its whole life: its down, motion
//! and up never reach a client, so neither a stray press nor a stray release
//! can leak, and one touch slot can be grabbed while other fingers keep
//! interacting normally. Touch frames are still delivered.

use std::time::Duration;

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Device, DeviceCapability, Event,
        InputBackend, InputEvent, KeyState, KeyboardKeyEvent, MouseButton, PointerAxisEvent,
        PointerButtonEvent, PointerMotionEvent, TouchEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
        touch::{DownEvent, MotionEvent as TouchMotionEvent, UpEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use super::{GestureSource, Shell};
use crate::evidence::Value;

impl Shell {
    /// Map an absolute-position event onto the logical output.
    fn absolute_position<B: InputBackend, E: AbsolutePositionEvent<B>>(
        &self,
        event: &E,
    ) -> Point<f64, Logical> {
        let transform = self.output.current_transform();
        let logical = self.logical_size();
        // Absolute devices report in panel (untransformed) space.
        let panel = transform.invert().transform_size(logical);
        transform.transform_point_in(event.position_transformed(panel), &panel.to_f64())
    }

    fn clamp(&self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        let size = self.logical_size().to_f64();
        Point::from((
            pos.x.clamp(0.0, (size.w - 1.0).max(0.0)),
            pos.y.clamp(0.0, (size.h - 1.0).max(0.0)),
        ))
    }

    /// Logical output size as the gesture recogniser wants it.
    fn gesture_output_size(&self) -> (f64, f64) {
        let size = self.logical_size().to_f64();
        (size.w, size.h)
    }

    /// Offer a contact at `location` to the home gesture. Returns `true`
    /// when it was grabbed by `source` (and must not reach clients).
    fn try_grab_gesture(&mut self, location: Point<f64, Logical>, source: GestureSource) -> bool {
        if self.gesture_source.is_some() || self.gesture.is_active() {
            return false;
        }
        let size = self.gesture_output_size();
        if self.gesture.begin((location.x, location.y), size) {
            self.gesture_source = Some(source);
            true
        } else {
            false
        }
    }

    /// The grabbed contact lifted: recognise and act.
    fn finish_gesture(&mut self) {
        let Some(source) = self.gesture_source.take() else {
            return;
        };
        if let Some(gesture) = self.gesture.end() {
            self.go_home(gesture.kind, source.label());
        }
    }

    fn cancel_gesture(&mut self) {
        self.gesture.cancel();
        self.gesture_source = None;
    }

    fn pointer_motion(&mut self, location: Point<f64, Logical>, time: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let location = self.clamp(location);
        self.pointer_location = location;
        let under = self.surface_under(location);
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
        self.request_redraw();
    }

    /// Pointer movement while the pointer owns the home gesture: the cursor
    /// follows, the recogniser is fed, clients see nothing.
    fn gesture_pointer_motion(&mut self, location: Point<f64, Logical>) {
        let location = self.clamp(location);
        self.pointer_location = location;
        self.gesture.update((location.x, location.y));
        self.request_redraw();
    }

    /// Re-point the pointer at whatever is now under it, but only when it was
    /// already over a client surface. This matters for crash recovery: when
    /// the focused window dies while the pointer sits on it, the pointer is
    /// moved to the surface that replaced it (usually home) so the next click
    /// lands correctly. It must NOT run before the pointer has ever engaged a
    /// client, because setting focus to a surface whose client has not yet
    /// bound `wl_pointer` leaves Smithay's focus stuck there: the client never
    /// receives the enter, and a later real motion sees no focus change and so
    /// sends none either. On a touch-first device the natural motion that
    /// begins every tap re-points the pointer, so no pre-focusing is needed.
    /// No-op without a pointer capability or while the home gesture owns it.
    pub(super) fn sync_pointer_focus(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if self.gesture_source == Some(GestureSource::Pointer) {
            // While grabbed the pointer is deliberately out of client reach.
            return;
        }
        if pointer.current_focus().is_none() {
            // The pointer is not over any client yet; let the next real motion
            // deliver the enter rather than pinning focus prematurely.
            return;
        }
        let location = self.pointer_location;
        let under = self.surface_under(location);
        let time = Duration::from(self.clock.now()).as_millis() as u32;
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
    }

    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        match event {
            InputEvent::DeviceAdded { device } => {
                self.emit(
                    "input-device",
                    &[
                        ("name", device.name().into()),
                        (
                            "keyboard",
                            Value::Bool(device.has_capability(DeviceCapability::Keyboard)),
                        ),
                        (
                            "pointer",
                            Value::Bool(device.has_capability(DeviceCapability::Pointer)),
                        ),
                        (
                            "touch",
                            Value::Bool(device.has_capability(DeviceCapability::Touch)),
                        ),
                    ],
                );
            }
            InputEvent::DeviceRemoved { .. } => {
                // The grabbed contact can never lift now; forget it.
                self.cancel_gesture();
            }
            InputEvent::Keyboard { event } => {
                let Some(keyboard) = self.seat.get_keyboard() else {
                    return;
                };
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let pressed = event.state() == KeyState::Pressed;
                let rotate = keyboard.input::<(), _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |_, _, handle| {
                        if pressed && handle.modified_sym() == Keysym::F9 {
                            FilterResult::Intercept(())
                        } else {
                            FilterResult::Forward
                        }
                    },
                );
                if rotate.is_some() {
                    self.rotate_clockwise();
                }
            }
            InputEvent::PointerMotion { event } => {
                let delta = event.delta();
                let location = self.pointer_location + delta;
                if self.gesture_source == Some(GestureSource::Pointer) {
                    self.gesture_pointer_motion(location);
                    return;
                }
                let time = event.time_msec();
                if let Some(pointer) = self.seat.get_pointer() {
                    let under = self.surface_under(self.clamp(location));
                    pointer.relative_motion(
                        self,
                        under,
                        &RelativeMotionEvent {
                            delta,
                            delta_unaccel: event.delta_unaccel(),
                            utime: event.time(),
                        },
                    );
                }
                self.pointer_motion(location, time);
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let location = self.absolute_position::<B, _>(&event);
                if self.gesture_source == Some(GestureSource::Pointer) {
                    self.gesture_pointer_motion(location);
                    return;
                }
                self.pointer_motion(location, event.time_msec());
            }
            InputEvent::PointerButton { event } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                if event.button() == Some(MouseButton::Left) {
                    match event.state() {
                        ButtonState::Pressed => {
                            // The press position is the last motion we saw.
                            let location = self.pointer_location;
                            if self.try_grab_gesture(location, GestureSource::Pointer) {
                                return;
                            }
                        }
                        ButtonState::Released
                            if self.gesture_source == Some(GestureSource::Pointer) =>
                        {
                            // The press never reached a client; neither does
                            // this release.
                            self.finish_gesture();
                            return;
                        }
                        ButtonState::Released => {}
                    }
                }
                pointer.button(
                    self,
                    &ButtonEvent {
                        button: event.button_code(),
                        state: event.state(),
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let source = event.source();
                let horizontal = event.amount(Axis::Horizontal).unwrap_or_else(|| {
                    event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.0
                });
                let vertical = event.amount(Axis::Vertical).unwrap_or_else(|| {
                    event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.0
                });
                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                if horizontal != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal);
                    if let Some(discrete) = event.amount_v120(Axis::Horizontal) {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical);
                    if let Some(discrete) = event.amount_v120(Axis::Vertical) {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }
                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            InputEvent::TouchDown { event } => {
                let Some(touch) = self.seat.get_touch() else {
                    return;
                };
                let location = self.clamp(self.absolute_position::<B, _>(&event));
                if self.try_grab_gesture(location, GestureSource::Touch(event.slot())) {
                    return;
                }
                let under = self.surface_under(location);
                touch.down(
                    self,
                    under,
                    &DownEvent {
                        slot: event.slot(),
                        location,
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
            }
            InputEvent::TouchMotion { event } => {
                let Some(touch) = self.seat.get_touch() else {
                    return;
                };
                let location = self.clamp(self.absolute_position::<B, _>(&event));
                if self.gesture_source == Some(GestureSource::Touch(event.slot())) {
                    self.gesture.update((location.x, location.y));
                    return;
                }
                let under = self.surface_under(location);
                touch.motion(
                    self,
                    under,
                    &TouchMotionEvent {
                        slot: event.slot(),
                        location,
                        time: event.time_msec(),
                    },
                );
            }
            InputEvent::TouchUp { event } => {
                let Some(touch) = self.seat.get_touch() else {
                    return;
                };
                if self.gesture_source == Some(GestureSource::Touch(event.slot())) {
                    self.finish_gesture();
                    return;
                }
                touch.up(
                    self,
                    &UpEvent {
                        slot: event.slot(),
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
            }
            InputEvent::TouchFrame { .. } => {
                // Frames are delivered even while a slot is grabbed so the
                // other slots' events are grouped as the device intended.
                if let Some(touch) = self.seat.get_touch() {
                    touch.frame(self);
                }
            }
            InputEvent::TouchCancel { .. } => {
                if matches!(self.gesture_source, Some(GestureSource::Touch(_))) {
                    self.cancel_gesture();
                }
                if let Some(touch) = self.seat.get_touch() {
                    touch.cancel(self);
                }
            }
            _ => {}
        }
    }
}
