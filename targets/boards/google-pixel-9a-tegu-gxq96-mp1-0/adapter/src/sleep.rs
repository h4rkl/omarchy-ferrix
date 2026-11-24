//! Pure helpers for the Linux suspend interface on this target: what the
//! kernel offers and what woke it. Target-independent so they test on every
//! host; the Linux adapter does the sysfs I/O.
//!
//! Interface facts from the 2026-11-24 probe on this exact target
//! (docs/status-log/2026-11.md, "Pixel 9a: suspend/resume probe"):
//! `/sys/power/state` reads `freeze mem disk`, `/sys/power/mem_sleep` reads
//! `s2idle [deep]`, and the GKI's `/sys/kernel/wakeup_reasons/
//! last_resume_reason` names the wake IRQ (`rtc-alarm0` and
//! `dhdpcie_host_wake` were seen). The power key's IRQ name has not been
//! recorded yet (nobody pressed it), so its classification below is a
//! best guess from the `gpio_keys` / `s2mpg` PMIC driver names.

use ferrix_hardware_api::{SleepState, WakeSource};

/// The sleep states `/sys/power/state` and `/sys/power/mem_sleep` offer.
/// `freeze` is suspend-to-idle; `deep` needs both `mem` in `state` and
/// `deep` among the `mem_sleep` choices (the brackets mark the current one).
pub fn offered(state: &str, mem_sleep: Option<&str>) -> Vec<SleepState> {
    let states: Vec<&str> = state.split_whitespace().collect();
    let mem_sleep: Vec<&str> = mem_sleep
        .unwrap_or("")
        .split_whitespace()
        .map(|s| s.trim_matches(|c| c == '[' || c == ']'))
        .collect();
    let mut offered = Vec::new();
    if states.contains(&"freeze") || (states.contains(&"mem") && mem_sleep.contains(&"s2idle")) {
        offered.push(SleepState::S2idle);
    }
    if states.contains(&"mem") && mem_sleep.contains(&"deep") {
        offered.push(SleepState::Deep);
    }
    offered
}

/// The two writes that enter `state`: an optional `mem_sleep` choice, then
/// the `/sys/power/state` word. s2idle uses `freeze` and leaves `mem_sleep`
/// alone.
pub fn entry(state: SleepState) -> (Option<&'static str>, &'static str) {
    match state {
        SleepState::S2idle => (None, "freeze"),
        SleepState::Deep => (Some("deep"), "mem"),
    }
}

/// Classifies `last_resume_reason` (one `IRQ name` line per source, or an
/// `Abort:` line). Empty or unreadable is `Unknown`.
pub fn classify_wake(reason: &str) -> WakeSource {
    let text = reason.to_ascii_lowercase();
    if text.trim().is_empty() {
        WakeSource::Unknown
    } else if [
        "gpio_keys",
        "gpio-keys",
        "pwrkey",
        "pwronf",
        "pwronr",
        "power key",
    ]
    .iter()
    .any(|k| text.contains(k))
    {
        WakeSource::PowerKey
    } else if text.contains("rtc") || text.contains("alarm") {
        WakeSource::Alarm
    } else if [
        "dhd",
        "wlan",
        "wifi",
        "bt_host_wake",
        "bluetooth",
        "cpif",
        "modem",
    ]
    .iter()
    .any(|k| text.contains(k))
    {
        WakeSource::Radio
    } else {
        WakeSource::Other
    }
}

/// One line of evidence from `last_resume_reason`: newlines joined, bounded.
pub fn wake_detail(reason: &str) -> Option<String> {
    let joined = reason
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (!joined.is_empty()).then(|| joined.chars().take(160).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probed_interface_offers_both_states() {
        assert_eq!(
            offered("freeze mem disk\n", Some("s2idle [deep]\n")),
            [SleepState::S2idle, SleepState::Deep]
        );
        assert_eq!(offered("freeze mem", None), [SleepState::S2idle]);
        assert_eq!(offered("mem", Some("[s2idle]")), [SleepState::S2idle]);
        assert!(offered("disk", Some("deep")).is_empty());
        assert_eq!(entry(SleepState::Deep), (Some("deep"), "mem"));
        assert_eq!(entry(SleepState::S2idle), (None, "freeze"));
    }

    #[test]
    fn probed_wake_reasons_classify() {
        assert_eq!(classify_wake("170 rtc-alarm0\n"), WakeSource::Alarm);
        assert_eq!(classify_wake("542 dhdpcie_host_wake"), WakeSource::Radio);
        assert_eq!(classify_wake("83 gpio_keys\n"), WakeSource::PowerKey);
        assert_eq!(
            classify_wake("Abort: Pending Wakeup Sources: foo"),
            WakeSource::Other
        );
        assert_eq!(classify_wake("\n"), WakeSource::Unknown);
        assert_eq!(
            wake_detail("542 dhdpcie_host_wake\n12 other\n").as_deref(),
            Some("542 dhdpcie_host_wake; 12 other")
        );
        assert_eq!(wake_detail(""), None);
    }
}
