//! Smithay side of the `ferrix_appd` link: turns link events and `ferrix_appd` directives
//! into window-stack changes, and turns surface/focus changes into reports.
//! The connection itself (thread, retries, de-duplication) is the
//! platform-neutral `crate::appd_link` module.

use ferrix_app_protocol::{Notification, Report};
use smithay::{
    desktop::Window,
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use super::Shell;
use crate::{
    appd_link::LinkEvent,
    evidence::{self, Value},
    gesture::HomeGestureKind,
};

/// Client-claimed xdg metadata of a toplevel: `(app_id, title)`. Evidence
/// and association hints only; never trusted for identity.
pub fn toplevel_meta(window: &Window) -> (Option<String>, Option<String>) {
    let Some(toplevel) = window.toplevel() else {
        return (None, None);
    };
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| {
                data.lock()
                    .ok()
                    .map(|d| (d.app_id.clone(), d.title.clone()))
            })
            .unwrap_or_default()
    })
}

impl Shell {
    /// Process id of the currently focused toplevel's client, if any.
    pub fn focused_pid(&self) -> Option<u32> {
        self.windows
            .focused()
            .and_then(|window| self.windows.pid_of(window))
    }

    /// Whether reports currently reach `ferrix_appd`.
    pub fn appd_connected(&self) -> bool {
        self.ferrix_appd
            .as_ref()
            .is_some_and(|link| link.connected())
    }

    fn emit_link_status(&self, status: &str, reason: Option<&str>) {
        let socket = self
            .ferrix_appd
            .as_ref()
            .map(|link| link.socket().display().to_string())
            .unwrap_or_default();
        evidence::emit(&evidence::appd_link(self.evidence, &socket, status, reason));
    }

    /// Drain the link thread's queue (calloop ping callback).
    pub(super) fn appd_wake(&mut self) {
        let events = match self.ferrix_appd.as_mut() {
            Some(link) => link.drain(),
            None => return,
        };
        for event in events {
            if let Some(status) = event.status() {
                let reason = match &event {
                    LinkEvent::Retrying { reason } => Some(reason.as_str()),
                    _ => None,
                };
                self.emit_link_status(status, reason);
            }
            match event {
                LinkEvent::Connected => self.resync_appd(),
                LinkEvent::Lost | LinkEvent::Retrying { .. } => {}
                LinkEvent::Notification(Notification::Activate {
                    pid,
                    app_id,
                    surface,
                }) => {
                    self.activate_pid(pid, surface, app_id.as_str());
                }
                LinkEvent::Notification(Notification::Withhold { pid, surface }) => {
                    self.withhold_surface(pid, surface);
                }
                LinkEvent::Notification(Notification::Close { pid, surface }) => {
                    self.close_surface(pid, surface);
                }
                LinkEvent::Notification(Notification::App { event }) => {
                    tracing::debug!("ferrix-appd lifecycle event: {event:?}");
                }
                // Intents are addressed to their handler's own streams.
                LinkEvent::Notification(Notification::Intent { .. }) => {}
            }
        }
    }

    /// After (re)connecting, replay what a fresh `ferrix_appd` needs: every mapped
    /// surface with a known pid, then the current focus.
    fn resync_appd(&mut self) {
        let mapped: Vec<(u32, u64, Window)> = self
            .windows
            .mapped()
            .filter_map(|(window, pid, surface)| pid.map(|pid| (pid, surface, window.clone())))
            .collect();
        let (focus, focus_surface) = (self.focused_pid(), self.focused_surface());
        let Some(link) = self.ferrix_appd.as_mut() else {
            return;
        };
        for (pid, surface, window) in mapped {
            let (app_id, title) = toplevel_meta(&window);
            link.report(&Report::SurfaceMapped {
                pid,
                app_id,
                title,
                surface: Some(surface),
            });
        }
        link.report_focus(focus, focus_surface);
    }

    /// `ferrix_appd` directive: bring a toplevel to the front: exactly the one
    /// with `surface` (revealing it when withheld), else the newest visible
    /// toplevel of `pid`. One without a mapped toplevel is recorded, not an
    /// error: the window will be focused when it maps (mapping raises).
    fn activate_pid(&mut self, pid: u32, surface: Option<u64>, app_id: &str) {
        let raised = match surface {
            Some(surface) => self.windows.raise_surface(pid, surface),
            None => self.windows.raise_pid(pid),
        };
        if raised {
            self.refocus();
        }
        evidence::emit(&evidence::client_activated(
            self.evidence,
            pid,
            app_id,
            raised,
        ));
    }

    /// `ferrix_appd` directive: stop presenting a toplevel that belongs to no
    /// running application (a shared runtime's unclaimed window).
    fn withhold_surface(&mut self, pid: u32, surface: u64) {
        let Some(window) = self.windows.find_surface(pid, surface).cloned() else {
            return;
        };
        if self.windows.is_held(&window) {
            return;
        }
        let was_focused = self.windows.focused() == Some(&window);
        self.windows.set_held(&window, true);
        self.emit(
            "client-withheld",
            &[
                ("pid", Value::Int(i64::from(pid))),
                ("surface", Value::Int(surface as i64)),
                ("source", "ferrix-appd".into()),
            ],
        );
        if was_focused {
            self.refocus();
        }
    }

    /// `ferrix_appd` directive: ask a toplevel to close (`xdg_toplevel.close`).
    /// For an Android task window, Android then removes the task.
    fn close_surface(&mut self, pid: u32, surface: u64) {
        let toplevel = self
            .windows
            .find_surface(pid, surface)
            .and_then(|window| window.toplevel().cloned());
        let sent = toplevel.is_some();
        if let Some(toplevel) = toplevel {
            toplevel.send_close();
        }
        self.emit(
            "client-close-requested",
            &[
                ("pid", Value::Int(i64::from(pid))),
                ("surface", Value::Int(surface as i64)),
                ("sent", Value::Bool(sent)),
            ],
        );
    }

    /// Report a toplevel's first buffer to `ferrix_appd`.
    pub(super) fn report_surface_mapped(&mut self, window: &Window) {
        let Some(pid) = self.windows.pid_of(window) else {
            return;
        };
        let surface = self.windows.surface_of(window);
        let (app_id, title) = toplevel_meta(window);
        if let Some(link) = self.ferrix_appd.as_mut() {
            link.report(&Report::SurfaceMapped {
                pid,
                app_id,
                title,
                surface,
            });
        }
    }

    /// Report that a mapped toplevel (`surface`) of the client with `pid`
    /// went away.
    pub(super) fn report_surface_unmapped(&mut self, pid: u32, surface: Option<u64>) {
        if let Some(link) = self.ferrix_appd.as_mut() {
            link.report(&Report::SurfaceUnmapped { pid, surface });
        }
    }

    /// Surface id of the focused toplevel, if any.
    pub fn focused_surface(&self) -> Option<u64> {
        self.windows
            .focused()
            .and_then(|window| self.windows.surface_of(window))
    }

    /// Report the focused pid and toplevel (de-duplicated by the link).
    pub(super) fn report_focus(&mut self) {
        let (pid, surface) = (self.focused_pid(), self.focused_surface());
        if let Some(link) = self.ferrix_appd.as_mut() {
            link.report_focus(pid, surface);
        }
    }

    /// A client asked (xdg_activation_v1) to raise `window`. With
    /// `ferrix_appd` linked, appd decides and answers with an activation;
    /// without it the compositor raises a visible window directly.
    pub(super) fn raise_on_request(&mut self, window: &Window) {
        let (Some(pid), Some(surface)) =
            (self.windows.pid_of(window), self.windows.surface_of(window))
        else {
            return;
        };
        let linked = self.appd_connected()
            && self
                .ferrix_appd
                .as_mut()
                .is_some_and(|link| link.report(&Report::ActivationRequested { pid, surface }));
        let raised = !linked && self.windows.raise(window);
        if raised {
            self.refocus();
        }
        self.emit(
            "client-activation-requested",
            &[
                ("pid", Value::Int(i64::from(pid))),
                ("surface", Value::Int(surface as i64)),
                ("linked", Value::Bool(linked)),
                ("raised", Value::Bool(raised)),
            ],
        );
    }

    /// The trusted home gesture completed. With `ferrix_appd` linked, `ferrix_appd` decides
    /// what home is and answers with an activate directive; without it the
    /// compositor raises the oldest mapped window (the first client to map,
    /// which is the home surface in every supported boot).
    pub fn go_home(&mut self, kind: HomeGestureKind, source: &str) {
        if kind == HomeGestureKind::Close {
            self.close_foreground(source);
            return;
        }
        let linked = self.appd_connected()
            && self
                .ferrix_appd
                .as_mut()
                .is_some_and(|link| link.report(&Report::HomeGesture));
        if !linked
            && let Some(oldest) = self.windows.oldest_mapped().cloned()
            && self.windows.raise(&oldest)
        {
            self.refocus();
        }
        evidence::emit(&evidence::home_gesture(
            self.evidence,
            kind.label(),
            source,
            linked,
        ));
    }

    /// A deliberate long bottom-edge swipe dismisses the foreground app.
    /// `ferrix_appd` owns normal termination; without that link, ask the
    /// focused xdg toplevel to close itself. Home is protected by appd and is
    /// the oldest mapped fallback surface, so the offline fallback only closes
    /// when another window is in front of it.
    fn close_foreground(&mut self, source: &str) {
        let linked = self.appd_connected()
            && self
                .ferrix_appd
                .as_mut()
                .is_some_and(|link| link.report(&Report::CloseGesture));
        if !linked
            && self.windows.visible_count() > 1
            && let Some(toplevel) = self.windows.focused().and_then(|w| w.toplevel())
        {
            toplevel.send_close();
        }
        evidence::emit(&evidence::close_gesture(self.evidence, source, linked));
    }
}
