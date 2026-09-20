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
use crate::{appd_link::LinkEvent, evidence, gesture::HomeGestureKind};

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
                LinkEvent::Notification(Notification::Activate { pid, app_id }) => {
                    self.activate_pid(pid, app_id.as_str());
                }
                LinkEvent::Notification(Notification::App { event }) => {
                    tracing::debug!("ferrix-appd lifecycle event: {event:?}");
                }
            }
        }
    }

    /// After (re)connecting, replay what a fresh `ferrix_appd` needs: every mapped
    /// surface with a known pid, then the current focus.
    fn resync_appd(&mut self) {
        let mapped: Vec<(u32, Window)> = self
            .windows
            .mapped()
            .filter_map(|(window, pid, _)| pid.map(|pid| (pid, window.clone())))
            .collect();
        let focus = self.focused_pid();
        let Some(link) = self.ferrix_appd.as_mut() else {
            return;
        };
        for (pid, window) in mapped {
            let (app_id, title) = toplevel_meta(&window);
            link.report(&Report::SurfaceMapped { pid, app_id, title });
        }
        link.report_focus(focus);
    }

    /// `ferrix_appd` directive: bring the toplevel of the client with `pid` to the
    /// front. A pid without a mapped toplevel is recorded, not an error: the
    /// window will be focused when it maps (mapping raises).
    fn activate_pid(&mut self, pid: u32, app_id: &str) {
        let raised = self.windows.raise_pid(pid);
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

    /// Report a toplevel's first buffer to `ferrix_appd`.
    pub(super) fn report_surface_mapped(&mut self, window: &Window) {
        let Some(pid) = self.windows.pid_of(window) else {
            return;
        };
        let (app_id, title) = toplevel_meta(window);
        if let Some(link) = self.ferrix_appd.as_mut() {
            link.report(&Report::SurfaceMapped { pid, app_id, title });
        }
    }

    /// Report that the client with `pid` has no mapped toplevel left.
    pub(super) fn report_surface_unmapped(&mut self, pid: u32) {
        if let Some(link) = self.ferrix_appd.as_mut() {
            link.report(&Report::SurfaceUnmapped { pid });
        }
    }

    /// Report the focused pid (de-duplicated by the link).
    pub(super) fn report_focus(&mut self) {
        let pid = self.focused_pid();
        if let Some(link) = self.ferrix_appd.as_mut() {
            link.report_focus(pid);
        }
    }

    /// The trusted home gesture completed. With `ferrix_appd` linked, `ferrix_appd` decides
    /// what home is and answers with an activate directive; without it the
    /// compositor raises the oldest mapped window (the first client to map,
    /// which is the home surface in every supported boot).
    pub fn go_home(&mut self, kind: HomeGestureKind, source: &str) {
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
}
