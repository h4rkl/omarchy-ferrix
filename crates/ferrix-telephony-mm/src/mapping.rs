//! ModemManager enum values and their mapping onto the typed contract.
//!
//! Values are the `MM*` enums from `ModemManager-enums.h` (1.x, stable ABI on
//! D-Bus). Each function is total: unknown or future values map to the most
//! conservative typed state rather than panicking.

use std::collections::BTreeSet;

use ferrix_telephony_api::{
    AccessTechnology, CallEndReason, CallState, Capabilities, Direction, ModemState, Operator,
    RadioPower, Registration, Signal, SimLock, SimState, SmsFailure, SmsState, TelephonyFeature,
};

/// `MMModemState`.
pub mod modem_state {
    pub const FAILED: i32 = -1;
    pub const UNKNOWN: i32 = 0;
    pub const INITIALIZING: i32 = 1;
    pub const LOCKED: i32 = 2;
    pub const DISABLED: i32 = 3;
    pub const DISABLING: i32 = 4;
    pub const ENABLING: i32 = 5;
    pub const ENABLED: i32 = 6;
    pub const SEARCHING: i32 = 7;
    pub const REGISTERED: i32 = 8;
    pub const DISCONNECTING: i32 = 9;
    pub const CONNECTING: i32 = 10;
    pub const CONNECTED: i32 = 11;
}

/// `MMModemStateFailedReason`.
pub mod failed_reason {
    pub const NONE: u32 = 0;
    pub const UNKNOWN: u32 = 1;
    pub const SIM_MISSING: u32 = 2;
    pub const SIM_ERROR: u32 = 3;
    pub const UNKNOWN_CAPABILITIES: u32 = 4;
    pub const ESIM_WITHOUT_PROFILES: u32 = 5;
}

/// `MMModemLock`.
pub mod lock {
    pub const UNKNOWN: u32 = 0;
    pub const NONE: u32 = 1;
    pub const SIM_PIN: u32 = 2;
    pub const SIM_PIN2: u32 = 3;
    pub const SIM_PUK: u32 = 4;
    pub const SIM_PUK2: u32 = 5;
}

/// `MMModemPowerState`.
pub mod power_state {
    pub const UNKNOWN: u32 = 0;
    pub const OFF: u32 = 1;
    pub const LOW: u32 = 2;
    pub const ON: u32 = 3;
}

/// `MMModem3gppRegistrationState`.
pub mod registration_state {
    pub const IDLE: u32 = 0;
    pub const HOME: u32 = 1;
    pub const SEARCHING: u32 = 2;
    pub const DENIED: u32 = 3;
    pub const UNKNOWN: u32 = 4;
    pub const ROAMING: u32 = 5;
    pub const HOME_SMS_ONLY: u32 = 6;
    pub const ROAMING_SMS_ONLY: u32 = 7;
    pub const EMERGENCY_ONLY: u32 = 8;
    pub const HOME_CSFB_NOT_PREFERRED: u32 = 9;
    pub const ROAMING_CSFB_NOT_PREFERRED: u32 = 10;
    pub const ATTACHED_RLOS: u32 = 11;
}

/// `MMModemAccessTechnology` bit flags.
pub mod access {
    pub const GSM: u32 = 1 << 1;
    pub const GSM_COMPACT: u32 = 1 << 2;
    pub const GPRS: u32 = 1 << 3;
    pub const EDGE: u32 = 1 << 4;
    pub const UMTS: u32 = 1 << 5;
    pub const HSDPA: u32 = 1 << 6;
    pub const HSUPA: u32 = 1 << 7;
    pub const HSPA: u32 = 1 << 8;
    pub const HSPA_PLUS: u32 = 1 << 9;
    pub const LTE: u32 = 1 << 14;
    pub const NR5G: u32 = 1 << 15;
    pub const LTE_CAT_M: u32 = 1 << 16;
    pub const LTE_NB_IOT: u32 = 1 << 17;
}

/// `MMCallState`.
pub mod call_state {
    pub const UNKNOWN: i32 = 0;
    pub const DIALING: i32 = 1;
    pub const RINGING_OUT: i32 = 2;
    pub const RINGING_IN: i32 = 3;
    pub const ACTIVE: i32 = 4;
    pub const HELD: i32 = 5;
    pub const WAITING: i32 = 6;
    pub const TERMINATED: i32 = 7;
}

/// `MMCallStateReason`.
pub mod call_reason {
    pub const UNKNOWN: u32 = 0;
    pub const OUTGOING_STARTED: u32 = 1;
    pub const INCOMING_NEW: u32 = 2;
    pub const ACCEPTED: u32 = 3;
    pub const TERMINATED: u32 = 4;
    pub const REFUSED_OR_BUSY: u32 = 5;
    pub const ERROR: u32 = 6;
    pub const AUDIO_SETUP_FAILED: u32 = 7;
    pub const TRANSFERRED: u32 = 8;
    pub const DEFLECTED: u32 = 9;
}

/// `MMCallDirection`.
pub mod call_direction {
    pub const UNKNOWN: i32 = 0;
    pub const INCOMING: i32 = 1;
    pub const OUTGOING: i32 = 2;
}

/// `MMSmsState`.
pub mod sms_state {
    pub const UNKNOWN: u32 = 0;
    pub const STORED: u32 = 1;
    pub const RECEIVING: u32 = 2;
    pub const RECEIVED: u32 = 3;
    pub const SENDING: u32 = 4;
    pub const SENT: u32 = 5;
}

/// `MMSmsDeliveryState` (3GPP TS 23.040 status report values).
pub mod delivery_state {
    pub const COMPLETED_RECEIVED: u32 = 0x00;
    pub const COMPLETED_RANGE_END: u32 = 0x1F;
    pub const TEMPORARY_ERROR_RANGE_START: u32 = 0x20;
    pub const TEMPORARY_ERROR_RANGE_END: u32 = 0x3F;
    pub const PERMANENT_ERROR_RANGE_START: u32 = 0x40;
    pub const PERMANENT_ERROR_RANGE_END: u32 = 0x5F;
    pub const UNKNOWN: u32 = 0x100;
}

/// Modem lifecycle. `LOCKED` is a SIM matter and maps to a ready modem; the
/// SIM state carries the lock.
pub fn modem_state(state: i32, failed: u32) -> ModemState {
    match state {
        modem_state::FAILED => ModemState::Failed {
            reason: failed_reason_name(failed).to_owned(),
        },
        modem_state::UNKNOWN => ModemState::Failed {
            reason: "unknown".to_owned(),
        },
        modem_state::INITIALIZING | modem_state::ENABLING => ModemState::Initializing,
        modem_state::DISABLED | modem_state::DISABLING => ModemState::Disabled,
        modem_state::LOCKED
        | modem_state::ENABLED
        | modem_state::SEARCHING
        | modem_state::REGISTERED
        | modem_state::DISCONNECTING
        | modem_state::CONNECTING
        | modem_state::CONNECTED => ModemState::Ready,
        _ => ModemState::Failed {
            reason: format!("unmapped modem state {state}"),
        },
    }
}

pub fn failed_reason_name(reason: u32) -> &'static str {
    match reason {
        failed_reason::NONE => "none",
        failed_reason::UNKNOWN => "unknown",
        failed_reason::SIM_MISSING => "sim-missing",
        failed_reason::SIM_ERROR => "sim-error",
        failed_reason::UNKNOWN_CAPABILITIES => "unknown-capabilities",
        failed_reason::ESIM_WITHOUT_PROFILES => "esim-without-profiles",
        _ => "unmapped",
    }
}

/// SIM state from the modem's `UnlockRequired`, its `UnlockRetries` entry
/// for that lock, whether a SIM object is present, and the failure reason.
pub fn sim_state(
    has_sim: bool,
    unlock_required: u32,
    retries: Option<u32>,
    modem_state_value: i32,
    failed: u32,
) -> SimState {
    if modem_state_value == modem_state::FAILED && matches!(failed, failed_reason::SIM_MISSING) {
        return SimState::Absent;
    }
    if modem_state_value == modem_state::FAILED && failed == failed_reason::SIM_ERROR {
        return SimState::Failed {
            reason: "sim-error".to_owned(),
        };
    }
    if !has_sim {
        return SimState::Absent;
    }
    let attempts = |r: Option<u32>| r.map_or(0, |v| u8::try_from(v).unwrap_or(u8::MAX));
    match unlock_required {
        lock::SIM_PIN | lock::SIM_PIN2 => match retries {
            Some(0) => SimState::Blocked,
            other => SimState::Locked {
                lock: SimLock::Pin,
                attempts_left: attempts(other),
            },
        },
        lock::SIM_PUK | lock::SIM_PUK2 => match retries {
            Some(0) => SimState::Blocked,
            other => SimState::Locked {
                lock: SimLock::Puk,
                attempts_left: attempts(other),
            },
        },
        lock::NONE | lock::UNKNOWN => SimState::Ready,
        other => SimState::Failed {
            reason: format!("unsupported lock {other}"),
        },
    }
}

pub fn registration(state: u32) -> Registration {
    match state {
        registration_state::HOME
        | registration_state::HOME_SMS_ONLY
        | registration_state::HOME_CSFB_NOT_PREFERRED => {
            Registration::Registered { roaming: false }
        }
        registration_state::ROAMING
        | registration_state::ROAMING_SMS_ONLY
        | registration_state::ROAMING_CSFB_NOT_PREFERRED => {
            Registration::Registered { roaming: true }
        }
        registration_state::SEARCHING | registration_state::ATTACHED_RLOS => {
            Registration::Searching
        }
        registration_state::DENIED => Registration::Denied,
        registration_state::EMERGENCY_ONLY => Registration::EmergencyOnly,
        _ => Registration::Unknown,
    }
}

/// Operator from `OperatorName`/`OperatorCode`; empty strings mean none.
pub fn operator(name: &str, code: &str) -> Option<Operator> {
    if name.is_empty() && code.is_empty() {
        return None;
    }
    Some(Operator {
        name: if name.is_empty() {
            code.to_owned()
        } else {
            name.to_owned()
        },
        mcc_mnc: if code.is_empty() {
            None
        } else {
            Some(code.to_owned())
        },
    })
}

/// The best technology in the bit mask.
pub fn access_technology(bits: u32) -> AccessTechnology {
    if bits & access::NR5G != 0 {
        AccessTechnology::Nr
    } else if bits & (access::LTE | access::LTE_CAT_M | access::LTE_NB_IOT) != 0 {
        AccessTechnology::Lte
    } else if bits
        & (access::UMTS | access::HSDPA | access::HSUPA | access::HSPA | access::HSPA_PLUS)
        != 0
    {
        AccessTechnology::Umts
    } else if bits & (access::GSM | access::GSM_COMPACT | access::GPRS | access::EDGE) != 0 {
        AccessTechnology::Gsm
    } else {
        AccessTechnology::None
    }
}

/// `SignalQuality` percent to bars (0..=4), rounding to nearest.
pub fn signal(quality_percent: u32) -> Signal {
    let percent = quality_percent.min(100);
    let bars = ((percent * 4) + 50) / 100;
    Signal::new(u8::try_from(bars).unwrap_or(4), None)
}

pub fn radio_power(power_state: u32) -> RadioPower {
    match power_state {
        power_state::OFF | power_state::LOW => RadioPower::Off,
        _ => RadioPower::On,
    }
}

pub fn call_direction(direction: i32) -> Option<Direction> {
    match direction {
        call_direction::INCOMING => Some(Direction::Incoming),
        call_direction::OUTGOING => Some(Direction::Outgoing),
        _ => None,
    }
}

/// `WAITING` (call waiting) is reported as ringing; `UNKNOWN` maps to the
/// direction's initial state.
pub fn call_state(state: i32, direction: Direction) -> CallState {
    match state {
        call_state::DIALING | call_state::RINGING_OUT => CallState::Dialing,
        call_state::RINGING_IN | call_state::WAITING => CallState::Ringing,
        call_state::ACTIVE => CallState::Active,
        call_state::HELD => CallState::Held,
        call_state::TERMINATED => CallState::Ended,
        _ => match direction {
            Direction::Incoming => CallState::Ringing,
            Direction::Outgoing => CallState::Dialing,
        },
    }
}

/// End reason from the `StateChanged` reason and the state the call left.
pub fn call_end_reason(
    reason: u32,
    previous: CallState,
    direction: Direction,
    local_hangup_requested: bool,
) -> CallEndReason {
    if local_hangup_requested {
        return CallEndReason::LocalHangup;
    }
    match reason {
        call_reason::REFUSED_OR_BUSY => match (direction, previous) {
            (Direction::Outgoing, CallState::Dialing) => CallEndReason::Busy,
            _ => CallEndReason::Rejected,
        },
        call_reason::ERROR | call_reason::AUDIO_SETUP_FAILED => CallEndReason::Failed {
            reason: if reason == call_reason::ERROR {
                "error".to_owned()
            } else {
                "audio-setup-failed".to_owned()
            },
        },
        call_reason::TERMINATED | call_reason::UNKNOWN => match (direction, previous) {
            (Direction::Incoming, CallState::Ringing) => CallEndReason::NoAnswer,
            (Direction::Outgoing, CallState::Dialing) => CallEndReason::NoAnswer,
            _ => CallEndReason::RemoteHangup,
        },
        call_reason::TRANSFERRED | call_reason::DEFLECTED => CallEndReason::RemoteHangup,
        _ => CallEndReason::Failed {
            reason: format!("unmapped reason {reason}"),
        },
    }
}

/// Outgoing message progress from `State` and `DeliveryState`.
pub fn sms_state(state: u32, delivery_state: u32) -> (SmsState, Option<SmsFailure>) {
    match state {
        sms_state::SENT => {
            if delivery_state <= delivery_state::COMPLETED_RANGE_END {
                (SmsState::Delivered, None)
            } else if (delivery_state::TEMPORARY_ERROR_RANGE_START
                ..=delivery_state::TEMPORARY_ERROR_RANGE_END)
                .contains(&delivery_state)
            {
                (SmsState::Failed, Some(SmsFailure::TimedOut))
            } else if (delivery_state::PERMANENT_ERROR_RANGE_START
                ..=delivery_state::PERMANENT_ERROR_RANGE_END)
                .contains(&delivery_state)
            {
                (SmsState::Failed, Some(SmsFailure::Rejected))
            } else {
                (SmsState::Sent, None)
            }
        }
        sms_state::SENDING => (SmsState::Sending, None),
        sms_state::RECEIVED => (SmsState::Received, None),
        sms_state::RECEIVING => (SmsState::Received, None),
        _ => (SmsState::Sending, None),
    }
}

/// Whether a stored message is a complete inbound message worth publishing.
pub fn is_complete_inbound(state: u32) -> bool {
    state == sms_state::RECEIVED
}

/// Capabilities ModemManager exposes through this adapter.
pub fn capabilities(has_voice: bool, has_messaging: bool) -> Capabilities {
    let mut set: BTreeSet<TelephonyFeature> =
        [TelephonyFeature::SimPin, TelephonyFeature::RadioPower]
            .into_iter()
            .collect();
    if has_voice {
        set.extend([
            TelephonyFeature::Voice,
            TelephonyFeature::Dtmf,
            TelephonyFeature::Hold,
        ]);
    }
    if has_messaging {
        set.extend([
            TelephonyFeature::Sms,
            TelephonyFeature::MultipartSms,
            TelephonyFeature::DeliveryReports,
        ]);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modem_states_map_to_the_contract() {
        assert_eq!(
            modem_state(modem_state::FAILED, failed_reason::SIM_MISSING),
            ModemState::Failed {
                reason: "sim-missing".into()
            }
        );
        assert_eq!(
            modem_state(modem_state::INITIALIZING, 0),
            ModemState::Initializing
        );
        assert_eq!(
            modem_state(modem_state::ENABLING, 0),
            ModemState::Initializing
        );
        assert_eq!(modem_state(modem_state::DISABLED, 0), ModemState::Disabled);
        assert_eq!(modem_state(modem_state::DISABLING, 0), ModemState::Disabled);
        for s in [
            modem_state::LOCKED,
            modem_state::ENABLED,
            modem_state::SEARCHING,
            modem_state::REGISTERED,
            modem_state::CONNECTED,
        ] {
            assert_eq!(modem_state(s, 0), ModemState::Ready, "{s}");
        }
        assert!(matches!(modem_state(99, 0), ModemState::Failed { .. }));
    }

    #[test]
    fn sim_states_follow_lock_and_retries() {
        assert_eq!(
            sim_state(true, lock::NONE, None, modem_state::REGISTERED, 0),
            SimState::Ready
        );
        assert_eq!(
            sim_state(false, lock::UNKNOWN, None, modem_state::ENABLED, 0),
            SimState::Absent
        );
        assert_eq!(
            sim_state(
                true,
                lock::UNKNOWN,
                None,
                modem_state::FAILED,
                failed_reason::SIM_MISSING
            ),
            SimState::Absent
        );
        assert_eq!(
            sim_state(true, lock::SIM_PIN, Some(3), modem_state::LOCKED, 0),
            SimState::Locked {
                lock: SimLock::Pin,
                attempts_left: 3
            }
        );
        assert_eq!(
            sim_state(true, lock::SIM_PUK, Some(10), modem_state::LOCKED, 0),
            SimState::Locked {
                lock: SimLock::Puk,
                attempts_left: 10
            }
        );
        assert_eq!(
            sim_state(true, lock::SIM_PUK, Some(0), modem_state::LOCKED, 0),
            SimState::Blocked
        );
        assert!(matches!(
            sim_state(
                true,
                lock::UNKNOWN,
                None,
                modem_state::FAILED,
                failed_reason::SIM_ERROR
            ),
            SimState::Failed { .. }
        ));
    }

    #[test]
    fn registration_and_operator() {
        assert_eq!(
            registration(registration_state::HOME),
            Registration::Registered { roaming: false }
        );
        assert_eq!(
            registration(registration_state::ROAMING_SMS_ONLY),
            Registration::Registered { roaming: true }
        );
        assert_eq!(
            registration(registration_state::SEARCHING),
            Registration::Searching
        );
        assert_eq!(
            registration(registration_state::DENIED),
            Registration::Denied
        );
        assert_eq!(
            registration(registration_state::EMERGENCY_ONLY),
            Registration::EmergencyOnly
        );
        assert_eq!(
            registration(registration_state::IDLE),
            Registration::Unknown
        );
        assert_eq!(registration(77), Registration::Unknown);
        assert_eq!(operator("", ""), None);
        assert_eq!(
            operator("Ferrix Mock", "00101"),
            Some(Operator {
                name: "Ferrix Mock".into(),
                mcc_mnc: Some("00101".into())
            })
        );
        assert_eq!(operator("", "00101").unwrap().name, "00101");
    }

    #[test]
    fn access_technology_picks_the_best_bit() {
        assert_eq!(access_technology(0), AccessTechnology::None);
        assert_eq!(
            access_technology(access::GPRS | access::EDGE),
            AccessTechnology::Gsm
        );
        assert_eq!(access_technology(access::HSPA_PLUS), AccessTechnology::Umts);
        assert_eq!(
            access_technology(access::LTE | access::UMTS),
            AccessTechnology::Lte
        );
        assert_eq!(
            access_technology(access::NR5G | access::LTE),
            AccessTechnology::Nr
        );
        assert_eq!(access_technology(access::LTE_NB_IOT), AccessTechnology::Lte);
    }

    #[test]
    fn signal_percent_to_bars_and_power() {
        assert_eq!(signal(0).bars, 0);
        assert_eq!(signal(12).bars, 0);
        assert_eq!(signal(13).bars, 1);
        assert_eq!(signal(50).bars, 2);
        assert_eq!(signal(75).bars, 3);
        assert_eq!(signal(88).bars, 4);
        assert_eq!(signal(100).bars, 4);
        assert_eq!(signal(250).bars, 4);
        assert_eq!(radio_power(power_state::OFF), RadioPower::Off);
        assert_eq!(radio_power(power_state::LOW), RadioPower::Off);
        assert_eq!(radio_power(power_state::ON), RadioPower::On);
        assert_eq!(radio_power(power_state::UNKNOWN), RadioPower::On);
    }

    #[test]
    fn call_states_directions_and_end_reasons() {
        assert_eq!(
            call_direction(call_direction::INCOMING),
            Some(Direction::Incoming)
        );
        assert_eq!(call_direction(0), None);
        assert_eq!(
            call_state(call_state::RINGING_OUT, Direction::Outgoing),
            CallState::Dialing
        );
        assert_eq!(
            call_state(call_state::WAITING, Direction::Incoming),
            CallState::Ringing
        );
        assert_eq!(
            call_state(call_state::HELD, Direction::Incoming),
            CallState::Held
        );
        assert_eq!(
            call_state(call_state::TERMINATED, Direction::Incoming),
            CallState::Ended
        );
        assert_eq!(
            call_state(call_state::UNKNOWN, Direction::Outgoing),
            CallState::Dialing
        );
        assert_eq!(
            call_end_reason(
                call_reason::REFUSED_OR_BUSY,
                CallState::Dialing,
                Direction::Outgoing,
                false
            ),
            CallEndReason::Busy
        );
        assert_eq!(
            call_end_reason(
                call_reason::REFUSED_OR_BUSY,
                CallState::Ringing,
                Direction::Incoming,
                false
            ),
            CallEndReason::Rejected
        );
        assert_eq!(
            call_end_reason(
                call_reason::TERMINATED,
                CallState::Active,
                Direction::Incoming,
                false
            ),
            CallEndReason::RemoteHangup
        );
        assert_eq!(
            call_end_reason(
                call_reason::TERMINATED,
                CallState::Ringing,
                Direction::Incoming,
                false
            ),
            CallEndReason::NoAnswer
        );
        assert_eq!(
            call_end_reason(
                call_reason::ERROR,
                CallState::Active,
                Direction::Incoming,
                false
            ),
            CallEndReason::Failed {
                reason: "error".into()
            }
        );
        assert_eq!(
            call_end_reason(
                call_reason::TERMINATED,
                CallState::Active,
                Direction::Incoming,
                true
            ),
            CallEndReason::LocalHangup
        );
    }

    #[test]
    fn sms_states_and_delivery_reports() {
        assert_eq!(
            sms_state(sms_state::SENDING, delivery_state::UNKNOWN),
            (SmsState::Sending, None)
        );
        assert_eq!(
            sms_state(sms_state::SENT, delivery_state::UNKNOWN),
            (SmsState::Sent, None)
        );
        assert_eq!(
            sms_state(sms_state::SENT, delivery_state::COMPLETED_RECEIVED),
            (SmsState::Delivered, None)
        );
        assert_eq!(
            sms_state(sms_state::SENT, 0x21),
            (SmsState::Failed, Some(SmsFailure::TimedOut))
        );
        assert_eq!(
            sms_state(sms_state::SENT, 0x41),
            (SmsState::Failed, Some(SmsFailure::Rejected))
        );
        assert_eq!(
            sms_state(sms_state::RECEIVED, delivery_state::UNKNOWN),
            (SmsState::Received, None)
        );
        assert!(is_complete_inbound(sms_state::RECEIVED));
        assert!(!is_complete_inbound(sms_state::RECEIVING));
    }

    #[test]
    fn capabilities_never_include_audio_route_or_data() {
        let all = capabilities(true, true);
        assert!(all.contains(&TelephonyFeature::Voice));
        assert!(all.contains(&TelephonyFeature::MultipartSms));
        assert!(!all.contains(&TelephonyFeature::AudioRoute));
        assert!(!all.contains(&TelephonyFeature::Data));
        let none = capabilities(false, false);
        assert_eq!(none.len(), 2);
    }
}
