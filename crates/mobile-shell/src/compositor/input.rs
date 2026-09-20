//! Seat input routing shared by the libinput (guest) and winit (nested)
//! backends: keyboard, absolute/relative pointer and touch.
//!
//! Absolute devices (QEMU's virtio-tablet is a libinput pointer with absolute
//! coordinates) are mapped through the output transform so rotation keeps
//! touch points aligned with what the client draws.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, Device, DeviceCapability, Event, InputBackend,
        InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
        PointerMotionEvent, TouchEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
        touch::{DownEvent, MotionEvent as TouchMotionEvent, UpEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use super::Shell;
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
            InputEvent::DeviceRemoved { .. } => {}
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
                self.pointer_motion(location, event.time_msec());
            }
            InputEvent::PointerButton { event } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
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
                if let Some(touch) = self.seat.get_touch() {
                    touch.frame(self);
                }
            }
            InputEvent::TouchCancel { .. } => {
                if let Some(touch) = self.seat.get_touch() {
                    touch.cancel(self);
                }
            }
            _ => {}
        }
    }
}
