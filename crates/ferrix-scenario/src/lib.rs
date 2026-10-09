//! Host-side deterministic scenario runner (ADR-006).
//!
//! Replays checked-in scenario files (`targets/qemu-aarch64/scenarios/*.toml`)
//! over the per-daemon mock-control channels and asserts the resulting
//! evidence records as ordered JSON subsets. It is a test tool: it drives
//! *mock* state in the QEMU guest or a nested host session and is never a
//! product interface or a physical-device control.

pub mod matcher;
pub mod runner;
pub mod scenario;

pub use matcher::{is_subset, match_in_order};
pub use runner::{FrameCapture, Mode, Outcome, run};
pub use scenario::{Scenario, ScenarioError, Service, Step};

/// Scenario names ADR-006 requires to be checked in.
pub const REQUIRED_SCENARIOS: &[&str] = &[
    "no-modem",
    "modem-init-failure-recovery",
    "sim-absent",
    "sim-pin-locked-unlock",
    "sim-ready",
    "registration-search-deny-register-roam-loss-recover",
    "incoming-call-answer-route-remote-hangup",
    "outgoing-call-connected",
    "outgoing-call-busy",
    "outgoing-call-rejected",
    "outgoing-call-timeout",
    "outgoing-call-no-service",
    "call-interrupted-by-modem-loss",
    "sms-incoming-single",
    "sms-incoming-multipart",
    "sms-outgoing-success",
    "sms-outgoing-failed",
    "sms-delayed-delivery-report",
    "radios-disabled",
    "wifi-disabled-scan-connect",
    "wifi-auth-failed",
    "wifi-dhcp-failed",
    "wifi-captive",
    "wifi-offline-recovered",
    "service-crash-restart-reconcile",
    "bluetooth-discovery-bounded",
    "bluetooth-pair-confirm-success",
    "bluetooth-pair-failures",
    "bluetooth-pair-pin",
    "bluetooth-incoming-request",
];

#[cfg(test)]
mod checked_in {
    use super::*;
    use std::path::Path;

    #[test]
    fn every_required_scenario_is_checked_in_and_valid() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../targets/qemu-aarch64/scenarios");
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir)
            .expect("scenarios directory")
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let scenario =
                Scenario::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for step in &scenario.steps {
                let text = step.command.to_string();
                // Synthetic numbers only (+1555010xxxx), never a real range.
                if let Some(pos) = text.find("\"+") {
                    assert!(
                        text[pos + 1..].starts_with("+1555010"),
                        "{}: non-synthetic number in {text}",
                        scenario.name
                    );
                }
            }
            names.push(scenario.name);
        }
        for required in REQUIRED_SCENARIOS {
            assert!(
                names.iter().any(|n| n == required),
                "missing required scenario {required}"
            );
        }
    }
}
