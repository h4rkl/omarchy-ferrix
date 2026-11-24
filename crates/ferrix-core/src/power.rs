//! Power-off and restart from the shell's trusted power menu.
//!
//! The request is a typed product command (`HardwareCommand::PowerAction`)
//! that the target's `Lifecycle` adapter accepts or refuses. Some targets'
//! init does the final transition: when `--power-marker PATH` is set, an
//! accepted action is written there (`poweroff` or `reboot`) for init to act
//! on, the same hand-off the shutdown marker uses. A physical target whose
//! adapter performs the OpenRC transition itself runs without a marker.

use std::path::PathBuf;

use ferrix_hardware_api::{HardwareError, PowerAction};
use serde_json::json;

#[derive(Clone, Debug, Default)]
pub struct PowerHandoff {
    pub marker: Option<PathBuf>,
}

/// The marker's contents for `action` (what init passes to busybox).
pub fn marker_word(action: PowerAction) -> &'static str {
    match action {
        PowerAction::PowerOff => "poweroff",
        PowerAction::Restart => "reboot",
    }
}

impl PowerHandoff {
    /// One record for the request's outcome at the adapter.
    pub fn report(&self, action: PowerAction, result: &Result<(), HardwareError>) {
        let (status, reason) = match result {
            Ok(()) => ("accepted", None),
            Err(error) => ("refused", Some(error.to_string())),
        };
        crate::emit(json!({
            "evidence": crate::evidence_label(),
            "event": "power-action",
            "action": action.name(),
            "status": status,
            "reason": reason,
            "handoff": self.marker.as_ref().map(|p| p.display().to_string()),
        }));
    }

    /// Leaves the marker for init, when this target uses one.
    pub fn hand_off(&self, action: PowerAction) {
        let Some(marker) = &self.marker else {
            return;
        };
        let result = marker
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(marker, format!("{}\n", marker_word(action))));
        if let Err(error) = result {
            crate::emit(json!({
                "evidence": crate::evidence_label(),
                "event": "power-action",
                "action": action.name(),
                "status": "handoff-failed",
                "reason": error.to_string(),
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_actions_leave_the_init_word() {
        let dir = std::env::temp_dir().join(format!("ferrix-power-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let handoff = PowerHandoff {
            marker: Some(dir.join("power-action")),
        };
        handoff.hand_off(PowerAction::Restart);
        assert_eq!(
            std::fs::read_to_string(dir.join("power-action")).unwrap(),
            "reboot\n"
        );
        handoff.hand_off(PowerAction::PowerOff);
        assert_eq!(
            std::fs::read_to_string(dir.join("power-action")).unwrap(),
            "poweroff\n"
        );
        PowerHandoff::default().hand_off(PowerAction::PowerOff);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
