//! Deterministic mock modem backend (ADR-001, ADR-006).
//!
//! Initial state comes from [`MockTelephonyConfig`] (derived from the target
//! profile); every later mutation comes from the host mock-control channel
//! through [`MockControl`]. Nothing here reads the wall clock: the mock owns a
//! millisecond counter that advances only on the `tick` command, and every
//! timer (ringing timeout, outgoing call outcome, SMS send/delivery, radio
//! re-registration, backend recovery) fires when a tick moves the clock past
//! its due time. The daemon's own clock follows the mock's through
//! `BackendEvent::Tick`.
//!
//! Handles are `c<n>` for calls and `m<n>` for outgoing messages in creation
//! order per process. The daemon assigns its public `CallId`s in the same
//! order and from the same starting point, so in a fresh daemon run the mock
//! handle and the public id coincide, which keeps scenario files readable.
//! Public `MessageId`s persist across daemon restarts and therefore may
//! diverge; `message_id` in commands is optional and defaults to the most
//! recent pending outgoing message.
//!
//! Synthetic fixtures only: numbers `+1555010xxxx`, operator `Ferrix Mock`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use ferrix_mock_control::{Ack, Handler};
use ferrix_target_profile::Profile;
use ferrix_telephony_adapter_api::{
    BackendCall, BackendError, BackendEvent, BackendSnapshot, CallHandle, ModemBackend, SmsHandle,
};
use ferrix_telephony_api::{
    AccessTechnology, AudioRoute, AudioRouteState, CallEndReason, CallState, Capabilities,
    DataBearer, DataProfile, Direction, Dtmf, E164, ModemState, MultipartPart, Operator,
    RadioPower, Registration, Signal, SimLock, SimState, SmsFailure, SmsState, TelephonyFeature,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Synthetic operator name used by every mock fixture.
pub const MOCK_OPERATOR: &str = "Ferrix Mock";
/// Synthetic MCC/MNC (test network).
pub const MOCK_MCC_MNC: &str = "001-01";

/// Deterministic delays (milliseconds of mock clock).
pub const RINGING_TIMEOUT_MS: u64 = 30_000;
pub const OUTGOING_PROGRESS_MS: u64 = 1_000;
pub const SMS_SEND_MS: u64 = 500;
pub const SMS_DELAYED_DELIVERY_MS: u64 = 2_000;
pub const REGISTER_MS: u64 = 1_000;
pub const RECOVER_MS: u64 = 1_000;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Initial mock state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockTelephonyConfig {
    pub modem: ModemState,
    pub sim: SimState,
    /// Synthetic PIN accepted while `sim` is locked.
    pub sim_pin: String,
    pub operator: Operator,
    pub access: AccessTechnology,
    pub signal_bars: u8,
    pub roaming: bool,
    pub features: Capabilities,
    pub audio_available: BTreeSet<AudioRoute>,
    /// Registration at start when modem and SIM are ready; `None` means
    /// "registered on the configured operator".
    pub initial_registration: Option<Registration>,
}

impl MockTelephonyConfig {
    fn initial_registration_or_default(&self, registered: bool) -> Registration {
        if !registered {
            return Registration::Unknown;
        }
        self.initial_registration
            .unwrap_or(Registration::Registered {
                roaming: self.roaming,
            })
    }
}

impl Default for MockTelephonyConfig {
    fn default() -> Self {
        Self {
            modem: ModemState::Ready,
            sim: SimState::Ready,
            sim_pin: "0000".to_owned(),
            operator: Operator {
                name: MOCK_OPERATOR.to_owned(),
                mcc_mnc: Some(MOCK_MCC_MNC.to_owned()),
            },
            access: AccessTechnology::Lte,
            signal_bars: 3,
            roaming: false,
            features: [
                TelephonyFeature::Voice,
                TelephonyFeature::Sms,
                TelephonyFeature::MultipartSms,
                TelephonyFeature::DeliveryReports,
                TelephonyFeature::Dtmf,
                TelephonyFeature::Hold,
                TelephonyFeature::AudioRoute,
                TelephonyFeature::SimPin,
                TelephonyFeature::RadioPower,
            ]
            .into_iter()
            .collect(),
            audio_available: [AudioRoute::Earpiece, AudioRoute::Speaker]
                .into_iter()
                .collect(),
            initial_registration: None,
        }
    }
}

impl MockTelephonyConfig {
    /// Derives the initial state from the target profile's `[telephony]`
    /// section (ADR-006). Older profiles without the section get the one
    /// `ferrix-target-profile` derives from `[connectivity]`; for those the
    /// coarse `connectivity.modem` summary also fixes the boot registration.
    pub fn from_profile(profile: &Profile) -> Self {
        use ferrix_target_profile::{
            ProfileAccessTechnology, ProfileModemState, ProfileSimState, ProfileTelephonyFeature,
        };
        let section = profile.telephony_section();
        let modem = match section.modem {
            ProfileModemState::Absent => ModemState::Absent,
            ProfileModemState::Disabled => ModemState::Disabled,
            ProfileModemState::Initializing => ModemState::Initializing,
            ProfileModemState::Ready => ModemState::Ready,
            ProfileModemState::Failed => ModemState::Failed {
                reason: "profile".to_owned(),
            },
        };
        let sim = match section.sim {
            ProfileSimState::Absent => SimState::Absent,
            ProfileSimState::Locked => SimState::Locked {
                lock: SimLock::Pin,
                attempts_left: 3,
            },
            ProfileSimState::Blocked => SimState::Blocked,
            ProfileSimState::Ready => SimState::Ready,
        };
        let access = match section.access {
            ProfileAccessTechnology::None => AccessTechnology::None,
            ProfileAccessTechnology::Gsm => AccessTechnology::Gsm,
            ProfileAccessTechnology::Umts => AccessTechnology::Umts,
            ProfileAccessTechnology::Lte => AccessTechnology::Lte,
            ProfileAccessTechnology::Nr => AccessTechnology::Nr,
        };
        let features: Capabilities = section
            .features
            .iter()
            .map(|f| match f {
                ProfileTelephonyFeature::Voice => TelephonyFeature::Voice,
                ProfileTelephonyFeature::Sms => TelephonyFeature::Sms,
                ProfileTelephonyFeature::MultipartSms => TelephonyFeature::MultipartSms,
                ProfileTelephonyFeature::DeliveryReports => TelephonyFeature::DeliveryReports,
                ProfileTelephonyFeature::Dtmf => TelephonyFeature::Dtmf,
                ProfileTelephonyFeature::Hold => TelephonyFeature::Hold,
                ProfileTelephonyFeature::Data => TelephonyFeature::Data,
                ProfileTelephonyFeature::AudioRoute => TelephonyFeature::AudioRoute,
                ProfileTelephonyFeature::SimPin => TelephonyFeature::SimPin,
                ProfileTelephonyFeature::RadioPower => TelephonyFeature::RadioPower,
            })
            .collect();
        // Without an explicit section the coarse summary decides whether the
        // mock boots registered; with one, `access = "none"` means not
        // registered and anything else means registered.
        let initial_registration = if profile.telephony.is_none() {
            use ferrix_hardware_api::ModemRegistration;
            Some(match profile.connectivity.modem {
                ModemRegistration::Registered => Registration::Registered {
                    roaming: section.roaming,
                },
                ModemRegistration::Searching => Registration::Searching,
                ModemRegistration::Denied => Registration::Denied,
                ModemRegistration::Unregistered => Registration::Unknown,
            })
        } else if access == AccessTechnology::None {
            Some(Registration::Unknown)
        } else {
            None
        };
        Self {
            modem,
            sim,
            sim_pin: section.sim_pin.clone(),
            operator: Operator {
                name: section.operator.clone(),
                mcc_mnc: Some(MOCK_MCC_MNC.to_owned()),
            },
            access,
            signal_bars: section.signal_bars,
            roaming: section.roaming,
            features,
            audio_available: Self::default().audio_available,
            initial_registration,
        }
    }
}

// ---------------------------------------------------------------------------
// Mock state
// ---------------------------------------------------------------------------

/// Outcome policy for the next outgoing call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutgoingResult {
    #[default]
    Connected,
    Busy,
    Rejected,
    Timeout,
    NoService,
}

/// Outcome policy for the next outgoing SMS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SmsResult {
    #[default]
    Sent,
    Failed,
    DelayedDelivery,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MockCall {
    handle: CallHandle,
    direction: Direction,
    number: E164,
    state: CallState,
    end_reason: Option<CallEndReason>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MockSms {
    handle: SmsHandle,
    state: SmsState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Timer {
    RingingTimeout(CallHandle),
    OutgoingOutcome(CallHandle, OutgoingResult),
    SmsSent(SmsHandle, bool),
    SmsDelivered(SmsHandle),
    SmsFailed(SmsHandle, SmsFailure),
    Register,
    Recovering,
    Recovered,
}

#[derive(Debug)]
struct Inner {
    config: MockTelephonyConfig,
    now_ms: u64,
    modem: ModemState,
    sim: SimState,
    registration: Registration,
    operator: Option<Operator>,
    access: AccessTechnology,
    signal: Signal,
    radio: RadioPower,
    data: DataBearer,
    audio: AudioRouteState,
    capabilities: Capabilities,
    calls: BTreeMap<CallHandle, MockCall>,
    sms: BTreeMap<SmsHandle, MockSms>,
    next_call: u32,
    next_sms: u32,
    outgoing_result: OutgoingResult,
    sms_result: SmsResult,
    timers: Vec<(u64, u64, Timer)>,
    timer_seq: u64,
    sinks: Vec<Sender<BackendEvent>>,
}

/// The mock backend. Clone handles share state (`Arc` inside), so the daemon
/// gets one and the control handler another.
#[derive(Clone)]
pub struct MockModem(Arc<Mutex<Inner>>);

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl MockModem {
    pub fn new(config: MockTelephonyConfig) -> Self {
        let registered = config.modem == ModemState::Ready && config.sim == SimState::Ready;
        let initial = config.initial_registration_or_default(registered);
        let (operator, access) = if initial.is_registered() {
            (Some(config.operator.clone()), config.access)
        } else {
            (None, AccessTechnology::None)
        };
        let audio = AudioRouteState {
            active: config
                .audio_available
                .iter()
                .next()
                .copied()
                .unwrap_or_default(),
            available: config.audio_available.clone(),
        };
        let inner = Inner {
            modem: config.modem.clone(),
            sim: config.sim.clone(),
            registration: initial,
            operator,
            access,
            signal: Signal::bars(if initial.is_registered() {
                config.signal_bars
            } else {
                0
            }),
            radio: RadioPower::On,
            data: if config.features.contains(&TelephonyFeature::Data) {
                DataBearer::Disconnected
            } else {
                DataBearer::Unsupported
            },
            audio,
            capabilities: config.features.clone(),
            config,
            now_ms: 0,
            calls: BTreeMap::new(),
            sms: BTreeMap::new(),
            next_call: 1,
            next_sms: 1,
            outgoing_result: OutgoingResult::Connected,
            sms_result: SmsResult::Sent,
            timers: Vec::new(),
            timer_seq: 0,
            sinks: Vec::new(),
        };
        Self(Arc::new(Mutex::new(inner)))
    }

    /// The mock clock.
    pub fn now_ms(&self) -> u64 {
        lock(&self.0).now_ms
    }

    /// Applies one control command. Returns the names of the emitted events.
    pub fn apply(&self, command: MockCommand) -> Result<Vec<String>, String> {
        let mut inner = lock(&self.0);
        let mut out = Emitter::default();
        match command {
            MockCommand::State {} => {}
            MockCommand::SetModem { state, reason } => {
                inner.set_modem(state.into_state(reason), &mut out)
            }
            MockCommand::SetSim {
                state,
                attempts_left,
            } => inner.set_sim(state.into_state(attempts_left), &mut out),
            MockCommand::SetRegistration {
                registration,
                roaming,
                operator,
                access,
            } => {
                let registration = registration.into_state(roaming.unwrap_or(false));
                let operator = operator.map(|name| Operator {
                    name,
                    mcc_mnc: Some(MOCK_MCC_MNC.to_owned()),
                });
                inner.set_registration(registration, operator, access, &mut out);
            }
            MockCommand::SetSignal { bars } => {
                inner.signal = Signal::bars(bars);
                out.push(BackendEvent::Signal(inner.signal));
            }
            MockCommand::IncomingCall { number } => {
                let number = E164::new(number).map_err(|e| e.to_string())?;
                inner.incoming_call(number, &mut out)?;
            }
            MockCommand::RemoteAnswer { call_id } => {
                let handle = inner.resolve_call(call_id, |c| {
                    c.direction == Direction::Outgoing && c.state == CallState::Dialing
                })?;
                inner.transition_call(&handle, CallState::Active, None, &mut out);
            }
            MockCommand::RemoteHangup { call_id } => {
                let handle = inner.resolve_call(call_id, |c| c.state.is_live())?;
                inner.end_call(&handle, CallEndReason::RemoteHangup, &mut out);
            }
            MockCommand::RemoteBusy { call_id } => {
                let handle = inner.resolve_call(call_id, |c| c.state == CallState::Dialing)?;
                inner.end_call(&handle, CallEndReason::Busy, &mut out);
            }
            MockCommand::RemoteReject { call_id } => {
                let handle = inner.resolve_call(call_id, |c| c.state == CallState::Dialing)?;
                inner.end_call(&handle, CallEndReason::Rejected, &mut out);
            }
            MockCommand::OutgoingTimeout { call_id } => {
                let handle = inner.resolve_call(call_id, |c| c.state == CallState::Dialing)?;
                inner.end_call(&handle, CallEndReason::TimedOut, &mut out);
            }
            MockCommand::IncomingSms { number, body } => {
                let from = E164::new(number).map_err(|e| e.to_string())?;
                inner.require_receiving()?;
                out.push(BackendEvent::SmsReceived { from, body });
            }
            MockCommand::IncomingSmsMultipart {
                number,
                ref_id,
                parts,
            } => {
                let from = E164::new(number).map_err(|e| e.to_string())?;
                inner.require_receiving()?;
                inner.require_feature(TelephonyFeature::MultipartSms)?;
                let count = u8::try_from(parts.len()).map_err(|_| "too many parts")?;
                if count == 0 {
                    return Err("parts must not be empty".to_owned());
                }
                for (i, text) in parts.into_iter().enumerate() {
                    out.push(BackendEvent::SmsPartReceived {
                        from: from.clone(),
                        part: MultipartPart {
                            ref_id,
                            index: i as u8 + 1,
                            count,
                            text,
                        },
                    });
                }
            }
            MockCommand::IncomingSmsPart {
                number,
                ref_id,
                index,
                count,
                text,
            } => {
                let from = E164::new(number).map_err(|e| e.to_string())?;
                inner.require_receiving()?;
                inner.require_feature(TelephonyFeature::MultipartSms)?;
                if index == 0 || count == 0 || index > count {
                    return Err(format!("part index {index} of {count} is out of range"));
                }
                out.push(BackendEvent::SmsPartReceived {
                    from,
                    part: MultipartPart {
                        ref_id,
                        index,
                        count,
                        text,
                    },
                });
            }
            MockCommand::DeliverSms { message_id } => {
                let handle = inner.resolve_sms(message_id, |s| s.state == SmsState::Sent)?;
                inner.set_sms(&handle, SmsState::Delivered, None, &mut out);
            }
            MockCommand::FailSms { message_id, reason } => {
                let handle = inner.resolve_sms(message_id, |s| {
                    matches!(s.state, SmsState::Sending | SmsState::Sent)
                })?;
                inner.set_sms(
                    &handle,
                    SmsState::Failed,
                    Some(parse_sms_failure(&reason)),
                    &mut out,
                );
            }
            MockCommand::SetOutgoingResult { result } => inner.outgoing_result = result,
            MockCommand::SetSmsResult { result } => inner.sms_result = result,
            MockCommand::SetRadio { power } => inner.set_radio(power, &mut out),
            MockCommand::SetAudioAvailable { routes } => {
                let available: BTreeSet<AudioRoute> = routes.into_iter().collect();
                let active = if available.contains(&inner.audio.active) {
                    inner.audio.active
                } else {
                    available.iter().next().copied().unwrap_or_default()
                };
                inner.audio = AudioRouteState { active, available };
                out.push(BackendEvent::Audio(inner.audio.clone()));
            }
            MockCommand::Tick { ms } => inner.tick(ms, &mut out),
            MockCommand::CrashBackend {} => inner.crash(&mut out),
        }
        Ok(inner.flush(out))
    }

    /// Redacted snapshot (ids and states only) for acks and tests.
    pub fn state_json(&self) -> Value {
        lock(&self.0).state_json()
    }
}

/// Collects events produced while the lock is held; flushed after.
#[derive(Default)]
struct Emitter(Vec<BackendEvent>);

impl Emitter {
    fn push(&mut self, event: BackendEvent) {
        self.0.push(event);
    }
}

impl Inner {
    fn flush(&mut self, out: Emitter) -> Vec<String> {
        let names = out.0.iter().map(|e| e.name().to_owned()).collect();
        for event in out.0 {
            self.sinks.retain(|sink| sink.send(event.clone()).is_ok());
        }
        names
    }

    fn schedule(&mut self, delay_ms: u64, timer: Timer) {
        self.timer_seq += 1;
        self.timers
            .push((self.now_ms + delay_ms, self.timer_seq, timer));
    }

    fn cancel_call_timers(&mut self, handle: &CallHandle) {
        self.timers.retain(|(_, _, t)| match t {
            Timer::RingingTimeout(h) | Timer::OutgoingOutcome(h, _) => h != handle,
            _ => true,
        });
    }

    fn cancel_sms_timers(&mut self, handle: &SmsHandle) {
        self.timers.retain(|(_, _, t)| match t {
            Timer::SmsSent(h, _) | Timer::SmsDelivered(h) | Timer::SmsFailed(h, _) => h != handle,
            _ => true,
        });
    }

    fn cancel_timers(&mut self, pred: impl Fn(&Timer) -> bool) {
        self.timers.retain(|(_, _, t)| !pred(t));
    }

    fn require_feature(&self, feature: TelephonyFeature) -> Result<(), String> {
        if self.capabilities.contains(&feature) {
            Ok(())
        } else {
            Err(format!("feature {feature} is not enabled in this mock"))
        }
    }

    fn require_receiving(&self) -> Result<(), String> {
        if self.modem != ModemState::Ready {
            return Err(format!("modem is {}", self.modem.name()));
        }
        if self.sim != SimState::Ready {
            return Err(format!("sim is {}", self.sim.name()));
        }
        if self.radio == RadioPower::Off {
            return Err("radio is off".to_owned());
        }
        if !self.registration.is_registered() {
            return Err(format!("registration is {}", self.registration.name()));
        }
        Ok(())
    }

    fn service_check(&self) -> Result<(), BackendError> {
        match &self.modem {
            ModemState::Ready => {}
            ModemState::Absent => return Err(BackendError::NoModem),
            other => return Err(BackendError::backend(format!("modem {}", other.name()))),
        }
        match &self.sim {
            SimState::Ready => {}
            SimState::Absent => return Err(BackendError::NoSim),
            SimState::Locked { .. } | SimState::Blocked => return Err(BackendError::SimLocked),
            SimState::Failed { reason } => {
                return Err(BackendError::backend(format!("sim failed: {reason}")));
            }
        }
        if self.radio == RadioPower::Off || !self.registration.is_registered() {
            return Err(BackendError::NoService);
        }
        Ok(())
    }

    fn incoming_call(&mut self, number: E164, out: &mut Emitter) -> Result<(), String> {
        self.require_receiving()?;
        self.require_feature(TelephonyFeature::Voice)?;
        let handle = CallHandle(format!("c{}", self.next_call));
        self.next_call += 1;
        self.calls.insert(
            handle.clone(),
            MockCall {
                handle: handle.clone(),
                direction: Direction::Incoming,
                number: number.clone(),
                state: CallState::Ringing,
                end_reason: None,
            },
        );
        self.schedule(RINGING_TIMEOUT_MS, Timer::RingingTimeout(handle.clone()));
        out.push(BackendEvent::CallAdded {
            handle,
            direction: Direction::Incoming,
            number,
            state: CallState::Ringing,
        });
        Ok(())
    }

    fn resolve_call(
        &self,
        explicit: Option<String>,
        pred: impl Fn(&MockCall) -> bool,
    ) -> Result<CallHandle, String> {
        if let Some(id) = explicit {
            let handle = CallHandle(id);
            let call = self
                .calls
                .get(&handle)
                .ok_or_else(|| format!("call {handle} not found"))?;
            if !pred(call) {
                return Err(format!("call {handle} is {}", call.state));
            }
            return Ok(handle);
        }
        let mut matching = self.calls.values().filter(|c| pred(c));
        let first = matching.next().ok_or("no matching call")?;
        if matching.next().is_some() {
            return Err("more than one matching call; pass call_id".to_owned());
        }
        Ok(first.handle.clone())
    }

    fn resolve_sms(
        &self,
        explicit: Option<String>,
        pred: impl Fn(&MockSms) -> bool,
    ) -> Result<SmsHandle, String> {
        if let Some(id) = explicit {
            let handle = SmsHandle(id);
            let sms = self
                .sms
                .get(&handle)
                .ok_or_else(|| format!("message {handle} not found"))?;
            if !pred(sms) {
                return Err(format!("message {handle} is {}", sms.state));
            }
            return Ok(handle);
        }
        self.sms
            .values()
            .rfind(|s| pred(s))
            .map(|s| s.handle.clone())
            .ok_or_else(|| "no matching outgoing message".to_owned())
    }

    fn transition_call(
        &mut self,
        handle: &CallHandle,
        state: CallState,
        end_reason: Option<CallEndReason>,
        out: &mut Emitter,
    ) {
        let Some(call) = self.calls.get_mut(handle) else {
            return;
        };
        if !call.state.can_transition_to(state) {
            return;
        }
        call.state = state;
        call.end_reason = end_reason.clone();
        if state == CallState::Active {
            let handle = handle.clone();
            self.cancel_call_timers(&handle);
        }
        out.push(BackendEvent::CallChanged {
            handle: handle.clone(),
            state,
            end_reason,
        });
    }

    fn end_call(&mut self, handle: &CallHandle, reason: CallEndReason, out: &mut Emitter) {
        self.cancel_call_timers(handle);
        self.transition_call(handle, CallState::Ended, Some(reason), out);
    }

    fn end_all_calls(&mut self, reason: CallEndReason, out: &mut Emitter) {
        let live: Vec<CallHandle> = self
            .calls
            .values()
            .filter(|c| c.state.is_live())
            .map(|c| c.handle.clone())
            .collect();
        for handle in live {
            self.end_call(&handle, reason.clone(), out);
        }
    }

    fn fail_pending_sms(&mut self, failure: SmsFailure, out: &mut Emitter) {
        let pending: Vec<SmsHandle> = self
            .sms
            .values()
            .filter(|s| s.state == SmsState::Sending)
            .map(|s| s.handle.clone())
            .collect();
        for handle in pending {
            self.set_sms(&handle, SmsState::Failed, Some(failure.clone()), out);
        }
    }

    fn set_sms(
        &mut self,
        handle: &SmsHandle,
        state: SmsState,
        failure: Option<SmsFailure>,
        out: &mut Emitter,
    ) {
        self.cancel_sms_timers(handle);
        let Some(sms) = self.sms.get_mut(handle) else {
            return;
        };
        if !sms.state.can_transition_to(state) {
            return;
        }
        sms.state = state;
        out.push(BackendEvent::SmsChanged {
            handle: handle.clone(),
            state,
            failure,
        });
    }

    fn set_modem(&mut self, state: ModemState, out: &mut Emitter) {
        if self.modem == state {
            return;
        }
        self.modem = state.clone();
        out.push(BackendEvent::Modem(state.clone()));
        if state != ModemState::Ready {
            self.end_all_calls(CallEndReason::ModemLost, out);
            self.fail_pending_sms(SmsFailure::ModemLost, out);
            self.lose_registration(out);
        } else {
            self.begin_registration(out);
        }
    }

    fn set_sim(&mut self, state: SimState, out: &mut Emitter) {
        if self.sim == state {
            return;
        }
        self.sim = state.clone();
        out.push(BackendEvent::Sim(state.clone()));
        if state == SimState::Ready {
            self.begin_registration(out);
        } else {
            self.end_all_calls(CallEndReason::NoService, out);
            self.fail_pending_sms(SmsFailure::NoService, out);
            self.lose_registration(out);
        }
    }

    fn set_radio(&mut self, power: RadioPower, out: &mut Emitter) {
        if self.radio == power {
            return;
        }
        self.radio = power;
        out.push(BackendEvent::Radio(power));
        match power {
            RadioPower::Off => {
                self.end_all_calls(CallEndReason::NoService, out);
                self.fail_pending_sms(SmsFailure::NoService, out);
                self.lose_registration(out);
            }
            RadioPower::On => self.begin_registration(out),
        }
    }

    fn lose_registration(&mut self, out: &mut Emitter) {
        self.cancel_timers(|t| matches!(t, Timer::Register));
        if self.registration != Registration::Unknown
            || self.operator.is_some()
            || self.access != AccessTechnology::None
        {
            self.registration = Registration::Unknown;
            self.operator = None;
            self.access = AccessTechnology::None;
            out.push(BackendEvent::Registration {
                registration: Registration::Unknown,
                operator: None,
                access: AccessTechnology::None,
            });
        }
        if self.signal.bars != 0 {
            self.signal = Signal::bars(0);
            out.push(BackendEvent::Signal(self.signal));
        }
        self.set_data(DataBearer::Disconnected, out);
    }

    fn set_data(&mut self, data: DataBearer, out: &mut Emitter) {
        if self.data != DataBearer::Unsupported && self.data != data {
            self.data = data.clone();
            out.push(BackendEvent::Data(data));
        }
    }

    /// Modem ready + SIM ready + radio on: search now, register on tick.
    fn begin_registration(&mut self, out: &mut Emitter) {
        if self.modem != ModemState::Ready
            || self.sim != SimState::Ready
            || self.radio != RadioPower::On
        {
            return;
        }
        if self.registration.is_registered() {
            return;
        }
        self.registration = Registration::Searching;
        self.operator = None;
        self.access = AccessTechnology::None;
        out.push(BackendEvent::Registration {
            registration: Registration::Searching,
            operator: None,
            access: AccessTechnology::None,
        });
        self.cancel_timers(|t| matches!(t, Timer::Register));
        self.schedule(REGISTER_MS, Timer::Register);
    }

    fn set_registration(
        &mut self,
        registration: Registration,
        operator: Option<Operator>,
        access: Option<AccessTechnology>,
        out: &mut Emitter,
    ) {
        self.cancel_timers(|t| matches!(t, Timer::Register));
        let registered = registration.is_registered();
        self.registration = registration;
        self.operator = if registered {
            Some(operator.unwrap_or_else(|| self.config.operator.clone()))
        } else {
            None
        };
        self.access = if registered {
            access.unwrap_or(self.config.access)
        } else {
            AccessTechnology::None
        };
        out.push(BackendEvent::Registration {
            registration,
            operator: self.operator.clone(),
            access: self.access,
        });
        let bars = if registered {
            self.config.signal_bars
        } else {
            0
        };
        if self.signal.bars != bars {
            self.signal = Signal::bars(bars);
            out.push(BackendEvent::Signal(self.signal));
        }
        if !registered {
            self.end_all_calls(CallEndReason::NoService, out);
            self.fail_pending_sms(SmsFailure::NoService, out);
        }
    }

    /// The daemon owns the `failed -> recovering` transition on
    /// `backend-lost`; the mock only tracks its own state silently and
    /// announces `ready` + `backend-restarted` when it is back.
    fn crash(&mut self, out: &mut Emitter) {
        self.cancel_timers(|_| true);
        out.push(BackendEvent::BackendLost {
            reason: "crash-backend".to_owned(),
        });
        self.modem = ModemState::Failed {
            reason: "backend-lost".to_owned(),
        };
        self.end_all_calls(CallEndReason::ModemLost, out);
        self.fail_pending_sms(SmsFailure::ModemLost, out);
        self.lose_registration(out);
        self.schedule(1, Timer::Recovering);
        self.schedule(RECOVER_MS, Timer::Recovered);
    }

    /// Advances the clock by `ms`, firing due timers in due order. The clock
    /// steps to each timer's due time while it fires so that follow-up timers
    /// it schedules are relative to that moment, not to the end of the tick.
    fn tick(&mut self, ms: u64, out: &mut Emitter) {
        let target = self.now_ms.saturating_add(ms);
        loop {
            self.timers.sort_by_key(|(due, seq, _)| (*due, *seq));
            let Some(position) = self.timers.iter().position(|(due, _, _)| *due <= target) else {
                break;
            };
            let (due, _, timer) = self.timers.remove(position);
            self.now_ms = due.max(self.now_ms);
            self.fire(timer, out);
        }
        self.now_ms = target;
        out.push(BackendEvent::Tick {
            now_ms: self.now_ms,
        });
    }

    fn fire(&mut self, timer: Timer, out: &mut Emitter) {
        match timer {
            Timer::RingingTimeout(handle) => {
                if self
                    .calls
                    .get(&handle)
                    .is_some_and(|c| c.state == CallState::Ringing)
                {
                    self.end_call(&handle, CallEndReason::NoAnswer, out);
                }
            }
            Timer::OutgoingOutcome(handle, result) => {
                if !self
                    .calls
                    .get(&handle)
                    .is_some_and(|c| c.state == CallState::Dialing)
                {
                    return;
                }
                match result {
                    OutgoingResult::Connected => {
                        self.transition_call(&handle, CallState::Active, None, out)
                    }
                    OutgoingResult::Busy => self.end_call(&handle, CallEndReason::Busy, out),
                    OutgoingResult::Rejected => {
                        self.end_call(&handle, CallEndReason::Rejected, out)
                    }
                    OutgoingResult::Timeout => self.end_call(&handle, CallEndReason::TimedOut, out),
                    OutgoingResult::NoService => {
                        self.end_call(&handle, CallEndReason::NoService, out)
                    }
                }
            }
            Timer::SmsSent(handle, delayed) => {
                self.set_sms(&handle, SmsState::Sent, None, out);
                if delayed
                    && self
                        .capabilities
                        .contains(&TelephonyFeature::DeliveryReports)
                {
                    self.schedule(SMS_DELAYED_DELIVERY_MS, Timer::SmsDelivered(handle));
                }
            }
            Timer::SmsDelivered(handle) => self.set_sms(&handle, SmsState::Delivered, None, out),
            Timer::SmsFailed(handle, failure) => {
                self.set_sms(&handle, SmsState::Failed, Some(failure), out)
            }
            Timer::Register => {
                if self.modem == ModemState::Ready
                    && self.sim == SimState::Ready
                    && self.radio == RadioPower::On
                {
                    let registration = Registration::Registered {
                        roaming: self.config.roaming,
                    };
                    let operator = Some(self.config.operator.clone());
                    let access = self.config.access;
                    self.set_registration(registration, operator, Some(access), out);
                }
            }
            Timer::Recovering => self.modem = ModemState::Recovering,
            Timer::Recovered => {
                self.modem = ModemState::Ready;
                out.push(BackendEvent::Modem(ModemState::Ready));
                self.begin_registration(out);
                out.push(BackendEvent::BackendRestarted);
            }
        }
    }

    fn snapshot(&self) -> BackendSnapshot {
        BackendSnapshot {
            now_ms: self.now_ms,
            modem: self.modem.clone(),
            sim: self.sim.clone(),
            registration: self.registration,
            operator: self.operator.clone(),
            access: self.access,
            signal: self.signal,
            radio: self.radio,
            data: self.data.clone(),
            audio: self.audio.clone(),
            capabilities: self.capabilities.clone(),
            calls: self
                .calls
                .values()
                .map(|c| BackendCall {
                    handle: c.handle.clone(),
                    direction: c.direction,
                    number: c.number.clone(),
                    state: c.state,
                    end_reason: c.end_reason.clone(),
                })
                .collect(),
        }
    }

    /// Ids and states only: no numbers, no bodies.
    fn state_json(&self) -> Value {
        json!({
            "now_ms": self.now_ms,
            "modem": self.modem.name(),
            "sim": self.sim.name(),
            "registration": self.registration.name(),
            "operator": self.operator.as_ref().map(|o| o.name.clone()),
            "access": self.access.name(),
            "signal_bars": self.signal.bars,
            "radio": self.radio.name(),
            "data": self.data.name(),
            "audio": self.audio.active.name(),
            "outgoing_result": self.outgoing_result,
            "sms_result": self.sms_result,
            "calls": self.calls.values().map(|c| json!({
                "call_id": c.handle.0,
                "direction": c.direction.name(),
                "state": c.state.name(),
                "end_reason": c.end_reason.as_ref().map(CallEndReason::name),
            })).collect::<Vec<_>>(),
            "sms": self.sms.values().map(|s| json!({
                "message_id": s.handle.0,
                "state": s.state.name(),
            })).collect::<Vec<_>>(),
            "pending_timers": self.timers.len(),
        })
    }
}

fn parse_sms_failure(reason: &str) -> SmsFailure {
    match reason {
        "no-service" => SmsFailure::NoService,
        "rejected" => SmsFailure::Rejected,
        "timed-out" => SmsFailure::TimedOut,
        "modem-lost" => SmsFailure::ModemLost,
        "invalid-destination" => SmsFailure::InvalidDestination,
        "unsupported" => SmsFailure::Unsupported,
        other => SmsFailure::Failed {
            reason: other.to_owned(),
        },
    }
}

impl ModemBackend for MockModem {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn capabilities(&self) -> Capabilities {
        lock(&self.0).capabilities.clone()
    }

    fn snapshot(&self) -> BackendSnapshot {
        lock(&self.0).snapshot()
    }

    fn subscribe(&self, sink: Sender<BackendEvent>) {
        lock(&self.0).sinks.push(sink);
    }

    fn dial(&self, number: &E164) -> Result<CallHandle, BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::Voice) {
            return Err(BackendError::Unsupported(TelephonyFeature::Voice));
        }
        inner.service_check()?;
        if inner.calls.values().any(|c| {
            matches!(
                c.state,
                CallState::Dialing | CallState::Ringing | CallState::Active
            )
        }) {
            return Err(BackendError::Busy);
        }
        let handle = CallHandle(format!("c{}", inner.next_call));
        inner.next_call += 1;
        inner.calls.insert(
            handle.clone(),
            MockCall {
                handle: handle.clone(),
                direction: Direction::Outgoing,
                number: number.clone(),
                state: CallState::Dialing,
                end_reason: None,
            },
        );
        let result = inner.outgoing_result;
        let delay = if result == OutgoingResult::Timeout {
            RINGING_TIMEOUT_MS
        } else {
            OUTGOING_PROGRESS_MS
        };
        inner.schedule(delay, Timer::OutgoingOutcome(handle.clone(), result));
        let mut out = Emitter::default();
        out.push(BackendEvent::CallAdded {
            handle: handle.clone(),
            direction: Direction::Outgoing,
            number: number.clone(),
            state: CallState::Dialing,
        });
        inner.flush(out);
        Ok(handle)
    }

    fn answer(&self, call: &CallHandle) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        let state = inner
            .calls
            .get(call)
            .map(|c| (c.state, c.direction))
            .ok_or_else(|| BackendError::NotFound(call.0.clone()))?;
        if state != (CallState::Ringing, Direction::Incoming) {
            return Err(BackendError::invalid_state(state.0, "answer"));
        }
        let mut out = Emitter::default();
        inner.transition_call(call, CallState::Active, None, &mut out);
        inner.flush(out);
        Ok(())
    }

    fn reject(&self, call: &CallHandle) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        let state = inner
            .calls
            .get(call)
            .map(|c| c.state)
            .ok_or_else(|| BackendError::NotFound(call.0.clone()))?;
        if state != CallState::Ringing {
            return Err(BackendError::invalid_state(state, "reject"));
        }
        let mut out = Emitter::default();
        inner.end_call(call, CallEndReason::Rejected, &mut out);
        inner.flush(out);
        Ok(())
    }

    fn hangup(&self, call: &CallHandle) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        let state = inner
            .calls
            .get(call)
            .map(|c| c.state)
            .ok_or_else(|| BackendError::NotFound(call.0.clone()))?;
        if !state.is_live() {
            return Err(BackendError::invalid_state(state, "hangup"));
        }
        let mut out = Emitter::default();
        inner.end_call(call, CallEndReason::LocalHangup, &mut out);
        inner.flush(out);
        Ok(())
    }

    fn hold(&self, call: &CallHandle) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::Hold) {
            return Err(BackendError::Unsupported(TelephonyFeature::Hold));
        }
        let state = inner
            .calls
            .get(call)
            .map(|c| c.state)
            .ok_or_else(|| BackendError::NotFound(call.0.clone()))?;
        if state != CallState::Active {
            return Err(BackendError::invalid_state(state, "hold"));
        }
        let mut out = Emitter::default();
        inner.transition_call(call, CallState::Held, None, &mut out);
        inner.flush(out);
        Ok(())
    }

    fn resume(&self, call: &CallHandle) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::Hold) {
            return Err(BackendError::Unsupported(TelephonyFeature::Hold));
        }
        let state = inner
            .calls
            .get(call)
            .map(|c| c.state)
            .ok_or_else(|| BackendError::NotFound(call.0.clone()))?;
        if state != CallState::Held {
            return Err(BackendError::invalid_state(state, "resume"));
        }
        let mut out = Emitter::default();
        inner.transition_call(call, CallState::Active, None, &mut out);
        inner.flush(out);
        Ok(())
    }

    fn dtmf(&self, call: &CallHandle, _digit: Dtmf) -> Result<(), BackendError> {
        let inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::Dtmf) {
            return Err(BackendError::Unsupported(TelephonyFeature::Dtmf));
        }
        let state = inner
            .calls
            .get(call)
            .map(|c| c.state)
            .ok_or_else(|| BackendError::NotFound(call.0.clone()))?;
        if state != CallState::Active {
            return Err(BackendError::invalid_state(state, "dtmf"));
        }
        Ok(())
    }

    fn send_sms(&self, _to: &E164, body: &str) -> Result<SmsHandle, BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::Sms) {
            return Err(BackendError::Unsupported(TelephonyFeature::Sms));
        }
        inner.service_check()?;
        if body.is_empty() {
            return Err(BackendError::backend("empty body"));
        }
        let handle = SmsHandle(format!("m{}", inner.next_sms));
        inner.next_sms += 1;
        inner.sms.insert(
            handle.clone(),
            MockSms {
                handle: handle.clone(),
                state: SmsState::Sending,
            },
        );
        match inner.sms_result {
            SmsResult::Sent => inner.schedule(SMS_SEND_MS, Timer::SmsSent(handle.clone(), false)),
            SmsResult::DelayedDelivery => {
                inner.schedule(SMS_SEND_MS, Timer::SmsSent(handle.clone(), true))
            }
            SmsResult::Failed => inner.schedule(
                SMS_SEND_MS,
                Timer::SmsFailed(handle.clone(), SmsFailure::Rejected),
            ),
        }
        Ok(handle)
    }

    fn connect_data(&self, _profile: &DataProfile) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::Data) {
            return Err(BackendError::Unsupported(TelephonyFeature::Data));
        }
        inner.service_check()?;
        if matches!(
            inner.data,
            DataBearer::Connected { .. } | DataBearer::Connecting
        ) {
            return Ok(());
        }
        let mut out = Emitter::default();
        inner.set_data(
            DataBearer::Connected {
                interface: Some("mock-cellular0".to_owned()),
            },
            &mut out,
        );
        inner.flush(out);
        Ok(())
    }

    fn disconnect_data(&self) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::Data) {
            return Err(BackendError::Unsupported(TelephonyFeature::Data));
        }
        let mut out = Emitter::default();
        inner.set_data(DataBearer::Disconnected, &mut out);
        inner.flush(out);
        Ok(())
    }

    fn set_radio_power(&self, power: RadioPower) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::RadioPower) {
            return Err(BackendError::Unsupported(TelephonyFeature::RadioPower));
        }
        if inner.modem == ModemState::Absent {
            return Err(BackendError::NoModem);
        }
        let mut out = Emitter::default();
        inner.set_radio(power, &mut out);
        inner.flush(out);
        Ok(())
    }

    fn enter_pin(&self, pin: &str) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::SimPin) {
            return Err(BackendError::Unsupported(TelephonyFeature::SimPin));
        }
        let SimState::Locked {
            lock: SimLock::Pin,
            attempts_left,
        } = inner.sim.clone()
        else {
            return Err(BackendError::invalid_state(inner.sim.name(), "enter-pin"));
        };
        let mut out = Emitter::default();
        let result = if pin == inner.config.sim_pin {
            inner.set_sim(SimState::Ready, &mut out);
            Ok(())
        } else {
            let left = attempts_left.saturating_sub(1);
            let next = if left == 0 {
                SimState::Blocked
            } else {
                SimState::Locked {
                    lock: SimLock::Pin,
                    attempts_left: left,
                }
            };
            inner.set_sim(next, &mut out);
            Err(BackendError::SimLocked)
        };
        inner.flush(out);
        result
    }

    fn set_audio_route(&self, route: AudioRoute) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        if !inner.capabilities.contains(&TelephonyFeature::AudioRoute) {
            return Err(BackendError::Unsupported(TelephonyFeature::AudioRoute));
        }
        if !inner.audio.available.contains(&route) {
            return Err(BackendError::backend(format!(
                "audio route {} is not available",
                route.name()
            )));
        }
        if inner.audio.active != route {
            inner.audio.active = route;
            let mut out = Emitter::default();
            out.push(BackendEvent::Audio(inner.audio.clone()));
            inner.flush(out);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock-control commands
// ---------------------------------------------------------------------------

/// `set-modem` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModemName {
    Absent,
    Disabled,
    Initializing,
    Ready,
    Failed,
    Recovering,
}

impl ModemName {
    fn into_state(self, reason: Option<String>) -> ModemState {
        match self {
            ModemName::Absent => ModemState::Absent,
            ModemName::Disabled => ModemState::Disabled,
            ModemName::Initializing => ModemState::Initializing,
            ModemName::Ready => ModemState::Ready,
            ModemName::Failed => ModemState::Failed {
                reason: reason.unwrap_or_else(|| "mock-control".to_owned()),
            },
            ModemName::Recovering => ModemState::Recovering,
        }
    }
}

/// `set-sim` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SimName {
    Absent,
    Locked,
    PukLocked,
    Blocked,
    Ready,
    Failed,
}

impl SimName {
    fn into_state(self, attempts_left: Option<u8>) -> SimState {
        match self {
            SimName::Absent => SimState::Absent,
            SimName::Locked => SimState::Locked {
                lock: SimLock::Pin,
                attempts_left: attempts_left.unwrap_or(3),
            },
            SimName::PukLocked => SimState::Locked {
                lock: SimLock::Puk,
                attempts_left: attempts_left.unwrap_or(10),
            },
            SimName::Blocked => SimState::Blocked,
            SimName::Ready => SimState::Ready,
            SimName::Failed => SimState::Failed {
                reason: "mock-control".to_owned(),
            },
        }
    }
}

/// `set-registration` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistrationName {
    Unknown,
    Searching,
    Denied,
    Registered,
    EmergencyOnly,
}

impl RegistrationName {
    fn into_state(self, roaming: bool) -> Registration {
        match self {
            RegistrationName::Unknown => Registration::Unknown,
            RegistrationName::Searching => Registration::Searching,
            RegistrationName::Denied => Registration::Denied,
            RegistrationName::Registered => Registration::Registered { roaming },
            RegistrationName::EmergencyOnly => Registration::EmergencyOnly,
        }
    }
}

/// Host mock-control commands (ADR-006). Unknown commands and unknown fields
/// are rejected, never ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MockCommand {
    State {},
    SetModem {
        state: ModemName,
        /// Failure reason recorded with `failed` (default `mock-control`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    SetSim {
        state: SimName,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attempts_left: Option<u8>,
    },
    SetRegistration {
        registration: RegistrationName,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        roaming: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operator: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        access: Option<AccessTechnology>,
    },
    SetSignal {
        bars: u8,
    },
    IncomingCall {
        number: String,
    },
    RemoteAnswer {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    RemoteHangup {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    RemoteBusy {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    RemoteReject {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    OutgoingTimeout {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    IncomingSms {
        number: String,
        body: String,
    },
    IncomingSmsMultipart {
        number: String,
        ref_id: u16,
        parts: Vec<String>,
    },
    IncomingSmsPart {
        number: String,
        ref_id: u16,
        index: u8,
        count: u8,
        text: String,
    },
    DeliverSms {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    FailSms {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        reason: String,
    },
    SetOutgoingResult {
        result: OutgoingResult,
    },
    SetSmsResult {
        result: SmsResult,
    },
    SetRadio {
        power: RadioPower,
    },
    SetAudioAvailable {
        routes: Vec<AudioRoute>,
    },
    Tick {
        ms: u64,
    },
    CrashBackend {},
}

impl MockCommand {
    pub fn name(&self) -> &'static str {
        match self {
            MockCommand::State {} => "state",
            MockCommand::SetModem { .. } => "set-modem",
            MockCommand::SetSim { .. } => "set-sim",
            MockCommand::SetRegistration { .. } => "set-registration",
            MockCommand::SetSignal { .. } => "set-signal",
            MockCommand::IncomingCall { .. } => "incoming-call",
            MockCommand::RemoteAnswer { .. } => "remote-answer",
            MockCommand::RemoteHangup { .. } => "remote-hangup",
            MockCommand::RemoteBusy { .. } => "remote-busy",
            MockCommand::RemoteReject { .. } => "remote-reject",
            MockCommand::OutgoingTimeout { .. } => "outgoing-timeout",
            MockCommand::IncomingSms { .. } => "incoming-sms",
            MockCommand::IncomingSmsMultipart { .. } => "incoming-sms-multipart",
            MockCommand::IncomingSmsPart { .. } => "incoming-sms-part",
            MockCommand::DeliverSms { .. } => "deliver-sms",
            MockCommand::FailSms { .. } => "fail-sms",
            MockCommand::SetOutgoingResult { .. } => "set-outgoing-result",
            MockCommand::SetSmsResult { .. } => "set-sms-result",
            MockCommand::SetRadio { .. } => "set-radio",
            MockCommand::SetAudioAvailable { .. } => "set-audio-available",
            MockCommand::Tick { .. } => "tick",
            MockCommand::CrashBackend {} => "crash-backend",
        }
    }
}

/// [`Handler`] for the mock-control channel: parses a command line, applies
/// it, and acks with the event names and a redacted state snapshot.
pub struct MockControl {
    modem: MockModem,
}

impl MockControl {
    pub fn new(modem: MockModem) -> Self {
        Self { modem }
    }

    /// Applies one line; usable directly by tests and by the serve loop.
    pub fn apply_line(&self, line: &str) -> Ack {
        let name = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("cmd").and_then(Value::as_str).map(str::to_owned));
        let command: MockCommand = match serde_json::from_str(line) {
            Ok(command) => command,
            Err(error) => {
                return Ack::error(name.as_deref(), format!("invalid command: {error}"));
            }
        };
        let cmd = command.name();
        match self.modem.apply(command) {
            Ok(events) => Ack::ok(cmd, events, self.modem.state_json()),
            Err(reason) => Ack::error(Some(cmd), reason),
        }
    }
}

impl Handler for MockControl {
    fn handle(&self, line: &str) -> Ack {
        self.apply_line(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, channel};

    const NUMBER: &str = "+15550100042";

    fn number() -> E164 {
        E164::new(NUMBER).unwrap()
    }

    fn modem() -> (MockModem, Receiver<BackendEvent>) {
        let modem = MockModem::new(MockTelephonyConfig::default());
        let (tx, rx) = channel();
        modem.subscribe(tx);
        (modem, rx)
    }

    fn cmd(modem: &MockModem, json: Value) -> Vec<String> {
        let control = MockControl::new(modem.clone());
        match control.apply_line(&json.to_string()) {
            Ack::Ok { events, .. } => events,
            Ack::Error { reason, .. } => panic!("{json}: {reason}"),
        }
    }

    fn cmd_err(modem: &MockModem, json: Value) -> String {
        let control = MockControl::new(modem.clone());
        match control.apply_line(&json.to_string()) {
            Ack::Ok { .. } => panic!("{json} unexpectedly succeeded"),
            Ack::Error { reason, .. } => reason,
        }
    }

    fn drain(rx: &Receiver<BackendEvent>) -> Vec<BackendEvent> {
        rx.try_iter().collect()
    }

    fn call_states(events: &[BackendEvent]) -> Vec<(String, CallState, Option<CallEndReason>)> {
        events
            .iter()
            .filter_map(|e| match e {
                BackendEvent::CallAdded { handle, state, .. } => {
                    Some((handle.0.clone(), *state, None))
                }
                BackendEvent::CallChanged {
                    handle,
                    state,
                    end_reason,
                } => Some((handle.0.clone(), *state, end_reason.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn default_config_starts_registered_on_the_mock_operator() {
        let (modem, _rx) = modem();
        let snapshot = modem.snapshot();
        assert_eq!(snapshot.modem, ModemState::Ready);
        assert_eq!(snapshot.sim, SimState::Ready);
        assert_eq!(
            snapshot.registration,
            Registration::Registered { roaming: false }
        );
        assert_eq!(snapshot.operator.unwrap().name, MOCK_OPERATOR);
        assert_eq!(snapshot.access, AccessTechnology::Lte);
        assert_eq!(snapshot.signal.bars, 3);
        assert_eq!(snapshot.audio.active, AudioRoute::Earpiece);
        assert!(snapshot.capabilities.contains(&TelephonyFeature::Voice));
        assert!(!snapshot.capabilities.contains(&TelephonyFeature::Data));
        assert_eq!(snapshot.data, DataBearer::Unsupported);
        assert_eq!(modem.name(), "mock");
    }

    #[test]
    fn data_feature_controls_a_deterministic_mock_bearer() {
        let mut config = MockTelephonyConfig::default();
        config.features.insert(TelephonyFeature::Data);
        let modem = MockModem::new(config);
        let (tx, rx) = channel();
        modem.subscribe(tx);

        assert_eq!(modem.snapshot().data, DataBearer::Disconnected);
        modem
            .connect_data(&DataProfile {
                apn: Some("internet".into()),
            })
            .unwrap();
        assert_eq!(
            modem.snapshot().data,
            DataBearer::Connected {
                interface: Some("mock-cellular0".into())
            }
        );
        assert!(
            drain(&rx).contains(&BackendEvent::Data(DataBearer::Connected {
                interface: Some("mock-cellular0".into())
            }))
        );
        modem.disconnect_data().unwrap();
        assert_eq!(modem.snapshot().data, DataBearer::Disconnected);
    }

    #[test]
    fn checked_in_profile_telephony_section_drives_the_mock() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../targets/qemu-aarch64/profile.toml"
        );
        let profile = Profile::load(path).unwrap();
        let config = MockTelephonyConfig::from_profile(&profile);
        assert_eq!(config.sim_pin, profile.telephony_section().sim_pin);
        assert_eq!(config.operator.name, profile.telephony_section().operator);
        let modem = MockModem::new(config);
        let snapshot = modem.snapshot();
        assert_eq!(snapshot.modem, ModemState::Ready);
        assert_eq!(snapshot.sim, SimState::Ready);
        assert!(snapshot.registration.is_registered());
        assert_eq!(snapshot.access, AccessTechnology::Lte);
        assert_eq!(snapshot.signal.bars, 3);
        assert!(snapshot.capabilities.contains(&TelephonyFeature::Voice));
        assert!(!snapshot.capabilities.contains(&TelephonyFeature::Data));
    }

    #[test]
    fn profile_without_telephony_section_maps_connectivity_summary() {
        let legacy = r#"
schema_version = 1
name = "legacy"
[display]
width = 1080
height = 2340
scale = 1.0
orientation = "portrait"
[input]
touch = true
rotation = true
[power]
battery_percent = 75
charging = false
[connectivity]
wifi = "connected"
bluetooth_present = true
modem = "searching"
sim = "locked"
[audio]
route = "speaker"
"#;
        let profile = Profile::parse(legacy).unwrap();
        assert!(profile.telephony.is_none());
        let config = MockTelephonyConfig::from_profile(&profile);
        assert_eq!(
            config.sim,
            SimState::Locked {
                lock: SimLock::Pin,
                attempts_left: 3
            }
        );
        assert_eq!(config.initial_registration, Some(Registration::Searching));
        let modem = MockModem::new(config);
        // A locked SIM never registers regardless of the summary.
        assert_eq!(modem.snapshot().registration, Registration::Unknown);
        assert_eq!(modem.enter_pin("0000"), Ok(()));
        assert_eq!(modem.snapshot().registration, Registration::Searching);

        let explicit = format!(
            "{legacy}\n[telephony]\nmodem = \"ready\"\nsim = \"ready\"\naccess = \"none\"\nsignal_bars = 0\nfeatures = [\"voice\"]\n"
        );
        let profile = Profile::parse(&explicit).unwrap();
        let config = MockTelephonyConfig::from_profile(&profile);
        assert_eq!(config.initial_registration, Some(Registration::Unknown));
        assert_eq!(config.features.len(), 1);
        let modem = MockModem::new(config);
        assert_eq!(modem.snapshot().registration, Registration::Unknown);
        assert_eq!(modem.dial(&number()), Err(BackendError::NoService));
    }

    #[test]
    fn commands_are_strict_and_unknown_are_rejected() {
        let (modem, _rx) = modem();
        let control = MockControl::new(modem.clone());
        assert!(!control.apply_line("{").is_ok());
        assert!(!control.apply_line(r#"{"cmd":"explode"}"#).is_ok());
        assert!(
            !control
                .apply_line(r#"{"cmd":"tick","ms":1,"extra":true}"#)
                .is_ok()
        );
        assert!(!control.apply_line(r#"{"cmd":"set-signal"}"#).is_ok());
        assert!(!control.apply_line(r#"{"nope":1}"#).is_ok());
        let ack = control.apply_line(r#"{"cmd":"state"}"#);
        match ack {
            Ack::Ok { ack, events, state } => {
                assert_eq!(ack, "state");
                assert!(events.is_empty());
                assert_eq!(state["modem"], "ready");
                assert_eq!(state["registration"], "registered");
                let text = state.to_string();
                assert!(!text.contains("number"), "{text}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn acks_never_carry_numbers_or_bodies() {
        let (modem, _rx) = modem();
        let control = MockControl::new(modem.clone());
        control.apply_line(&json!({"cmd":"incoming-call","number":NUMBER}).to_string());
        let ack = control.apply_line(
            &json!({"cmd":"incoming-sms","number":NUMBER,"body":"secret body"}).to_string(),
        );
        let text = serde_json::to_string(&ack).unwrap();
        assert!(!text.contains("0100042"), "{text}");
        assert!(!text.contains("secret"), "{text}");
        assert!(text.contains(r#""call_id":"c1""#), "{text}");
    }

    // --- modem / sim / registration scenarios (ADR-006 required list) ---

    #[test]
    fn no_modem_rejects_everything_typed() {
        let modem = MockModem::new(MockTelephonyConfig {
            modem: ModemState::Absent,
            ..MockTelephonyConfig::default()
        });
        assert_eq!(modem.dial(&number()), Err(BackendError::NoModem));
        assert_eq!(
            modem.send_sms(&number(), "x").unwrap_err(),
            BackendError::NoModem
        );
        assert_eq!(
            modem.set_radio_power(RadioPower::Off),
            Err(BackendError::NoModem)
        );
        assert_eq!(modem.snapshot().registration, Registration::Unknown);
        assert!(cmd_err(&modem, json!({"cmd":"incoming-call","number":NUMBER})).contains("absent"));
    }

    #[test]
    fn modem_init_failure_then_recovery() {
        let (modem, rx) = modem();
        cmd(&modem, json!({"cmd":"set-modem","state":"failed"}));
        let events = drain(&rx);
        assert!(matches!(
            events[0],
            BackendEvent::Modem(ModemState::Failed { .. })
        ));
        assert!(events.iter().any(|e| matches!(
            e,
            BackendEvent::Registration {
                registration: Registration::Unknown,
                ..
            }
        )));
        assert_eq!(
            modem.dial(&number()).unwrap_err(),
            BackendError::backend("modem failed")
        );
        cmd(&modem, json!({"cmd":"set-modem","state":"ready"}));
        assert_eq!(modem.snapshot().registration, Registration::Searching);
        cmd(&modem, json!({"cmd":"tick","ms":REGISTER_MS}));
        assert!(modem.snapshot().registration.is_registered());
        assert_eq!(modem.snapshot().signal.bars, 3);
    }

    #[test]
    fn sim_absent_and_sim_ready() {
        let (modem, _rx) = modem();
        cmd(&modem, json!({"cmd":"set-sim","state":"absent"}));
        assert_eq!(modem.dial(&number()), Err(BackendError::NoSim));
        assert_eq!(modem.snapshot().registration, Registration::Unknown);
        cmd(&modem, json!({"cmd":"set-sim","state":"ready"}));
        assert_eq!(modem.snapshot().registration, Registration::Searching);
        cmd(&modem, json!({"cmd":"tick","ms":1000}));
        assert!(modem.dial(&number()).is_ok());
    }

    #[test]
    fn sim_pin_locked_unlock_and_block() {
        let (modem, rx) = modem();
        cmd(
            &modem,
            json!({"cmd":"set-sim","state":"locked","attempts_left":2}),
        );
        assert_eq!(modem.dial(&number()), Err(BackendError::SimLocked));
        assert_eq!(modem.enter_pin("9999"), Err(BackendError::SimLocked));
        assert_eq!(
            modem.snapshot().sim,
            SimState::Locked {
                lock: SimLock::Pin,
                attempts_left: 1
            }
        );
        assert_eq!(modem.enter_pin("0000"), Ok(()));
        assert_eq!(modem.snapshot().sim, SimState::Ready);
        assert_eq!(modem.snapshot().registration, Registration::Searching);
        cmd(&modem, json!({"cmd":"tick","ms":1000}));
        assert!(modem.snapshot().registration.is_registered());
        assert!(
            drain(&rx)
                .iter()
                .any(|e| matches!(e, BackendEvent::Sim(SimState::Ready)))
        );

        cmd(
            &modem,
            json!({"cmd":"set-sim","state":"locked","attempts_left":1}),
        );
        assert_eq!(modem.enter_pin("1111"), Err(BackendError::SimLocked));
        assert_eq!(modem.snapshot().sim, SimState::Blocked);
        assert!(matches!(
            modem.enter_pin("0000"),
            Err(BackendError::InvalidState { .. })
        ));
    }

    #[test]
    fn registration_search_deny_register_roam_loss_recover() {
        let (modem, rx) = modem();
        cmd(
            &modem,
            json!({"cmd":"set-registration","registration":"searching"}),
        );
        assert_eq!(modem.dial(&number()), Err(BackendError::NoService));
        cmd(
            &modem,
            json!({"cmd":"set-registration","registration":"denied"}),
        );
        assert_eq!(modem.snapshot().registration, Registration::Denied);
        assert_eq!(modem.snapshot().signal.bars, 0);
        cmd(
            &modem,
            json!({"cmd":"set-registration","registration":"registered","operator":"Ferrix Mock","access":"umts"}),
        );
        assert_eq!(modem.snapshot().access, AccessTechnology::Umts);
        cmd(
            &modem,
            json!({"cmd":"set-registration","registration":"registered","roaming":true}),
        );
        assert_eq!(
            modem.snapshot().registration,
            Registration::Registered { roaming: true }
        );
        let handle = modem.dial(&number()).unwrap();
        cmd(
            &modem,
            json!({"cmd":"set-registration","registration":"unknown"}),
        );
        let events = drain(&rx);
        assert!(call_states(&events).iter().any(|(h, s, r)| {
            h == &handle.0 && *s == CallState::Ended && *r == Some(CallEndReason::NoService)
        }));
        cmd(
            &modem,
            json!({"cmd":"set-registration","registration":"registered"}),
        );
        assert!(modem.dial(&number()).is_ok());
    }

    // --- calls ---

    #[test]
    fn incoming_call_answer_route_remote_hangup() {
        let (modem, rx) = modem();
        let events = cmd(&modem, json!({"cmd":"incoming-call","number":NUMBER}));
        assert_eq!(events, ["call-added"]);
        let handle = CallHandle::from("c1");
        assert_eq!(modem.snapshot().calls[0].state, CallState::Ringing);
        modem.answer(&handle).unwrap();
        assert_eq!(modem.snapshot().calls[0].state, CallState::Active);
        modem.set_audio_route(AudioRoute::Speaker).unwrap();
        assert_eq!(modem.snapshot().audio.active, AudioRoute::Speaker);
        assert_eq!(
            modem.set_audio_route(AudioRoute::Bluetooth),
            Err(BackendError::backend(
                "audio route bluetooth is not available"
            ))
        );
        cmd(&modem, json!({"cmd":"remote-hangup"}));
        let events = drain(&rx);
        let states = call_states(&events);
        assert_eq!(states[0].1, CallState::Ringing);
        assert_eq!(states[1].1, CallState::Active);
        assert_eq!(
            states[2],
            (
                "c1".into(),
                CallState::Ended,
                Some(CallEndReason::RemoteHangup)
            )
        );
        assert!(events.iter().any(|e| matches!(e, BackendEvent::Audio(_))));
        // Ringing timer was cancelled by the answer: a long tick changes nothing.
        cmd(&modem, json!({"cmd":"tick","ms":RINGING_TIMEOUT_MS * 2}));
        assert!(call_states(&drain(&rx)).is_empty());
    }

    #[test]
    fn incoming_call_unanswered_times_out_and_reject_works() {
        let (modem, rx) = modem();
        cmd(&modem, json!({"cmd":"incoming-call","number":NUMBER}));
        cmd(&modem, json!({"cmd":"tick","ms":RINGING_TIMEOUT_MS - 1}));
        assert_eq!(modem.snapshot().calls[0].state, CallState::Ringing);
        cmd(&modem, json!({"cmd":"tick","ms":1}));
        let call = &modem.snapshot().calls[0];
        assert_eq!(call.state, CallState::Ended);
        assert_eq!(call.end_reason, Some(CallEndReason::NoAnswer));
        drain(&rx);

        cmd(&modem, json!({"cmd":"incoming-call","number":NUMBER}));
        let handle = CallHandle::from("c2");
        modem.reject(&handle).unwrap();
        let states = call_states(&drain(&rx));
        assert_eq!(states.last().unwrap().2, Some(CallEndReason::Rejected));
        assert!(matches!(
            modem.answer(&handle),
            Err(BackendError::InvalidState { .. })
        ));
        assert_eq!(
            modem.answer(&CallHandle::from("c99")),
            Err(BackendError::NotFound("c99".into()))
        );
    }

    #[test]
    fn outgoing_call_connected_hold_resume_dtmf_hangup() {
        let (modem, rx) = modem();
        let handle = modem.dial(&number()).unwrap();
        assert_eq!(handle, CallHandle::from("c1"));
        assert_eq!(modem.dial(&number()), Err(BackendError::Busy));
        assert!(matches!(
            modem.hold(&handle),
            Err(BackendError::InvalidState { .. })
        ));
        cmd(&modem, json!({"cmd":"tick","ms":OUTGOING_PROGRESS_MS}));
        assert_eq!(modem.snapshot().calls[0].state, CallState::Active);
        modem.dtmf(&handle, Dtmf::new('5').unwrap()).unwrap();
        modem.hold(&handle).unwrap();
        assert!(matches!(
            modem.dtmf(&handle, Dtmf::new('5').unwrap()),
            Err(BackendError::InvalidState { .. })
        ));
        modem.resume(&handle).unwrap();
        modem.hangup(&handle).unwrap();
        let states = call_states(&drain(&rx));
        let seq: Vec<CallState> = states.iter().map(|s| s.1).collect();
        assert_eq!(
            seq,
            [
                CallState::Dialing,
                CallState::Active,
                CallState::Held,
                CallState::Active,
                CallState::Ended
            ]
        );
        assert_eq!(states.last().unwrap().2, Some(CallEndReason::LocalHangup));
    }

    #[test]
    fn outgoing_call_policies_busy_rejected_timeout_no_service() {
        for (result, reason, delay) in [
            ("busy", CallEndReason::Busy, OUTGOING_PROGRESS_MS),
            ("rejected", CallEndReason::Rejected, OUTGOING_PROGRESS_MS),
            ("timeout", CallEndReason::TimedOut, RINGING_TIMEOUT_MS),
            ("no-service", CallEndReason::NoService, OUTGOING_PROGRESS_MS),
        ] {
            let (modem, rx) = modem();
            cmd(&modem, json!({"cmd":"set-outgoing-result","result":result}));
            let handle = modem.dial(&number()).unwrap();
            cmd(&modem, json!({"cmd":"tick","ms":delay - 1}));
            assert_eq!(
                modem.snapshot().calls[0].state,
                CallState::Dialing,
                "{result}"
            );
            cmd(&modem, json!({"cmd":"tick","ms":1}));
            let states = call_states(&drain(&rx));
            assert_eq!(
                states.last().unwrap(),
                &(handle.0.clone(), CallState::Ended, Some(reason.clone())),
                "{result}"
            );
        }
    }

    #[test]
    fn remote_commands_drive_outgoing_calls_before_the_timer() {
        for (command, reason) in [
            ("remote-busy", CallEndReason::Busy),
            ("remote-reject", CallEndReason::Rejected),
            ("outgoing-timeout", CallEndReason::TimedOut),
            ("remote-hangup", CallEndReason::RemoteHangup),
        ] {
            let (modem, rx) = modem();
            modem.dial(&number()).unwrap();
            cmd(&modem, json!({"cmd":command}));
            let states = call_states(&drain(&rx));
            assert_eq!(states.last().unwrap().2, Some(reason.clone()), "{command}");
            // The scheduled outcome must not resurrect the call.
            cmd(&modem, json!({"cmd":"tick","ms":OUTGOING_PROGRESS_MS}));
            assert!(call_states(&drain(&rx)).is_empty(), "{command}");
        }
        let (modem, rx) = modem();
        modem.dial(&number()).unwrap();
        cmd(&modem, json!({"cmd":"remote-answer","call_id":"c1"}));
        assert_eq!(
            call_states(&drain(&rx)).last().unwrap().1,
            CallState::Active
        );
        assert!(cmd_err(&modem, json!({"cmd":"remote-answer"})).contains("no matching"));
        assert!(cmd_err(&modem, json!({"cmd":"remote-busy","call_id":"c7"})).contains("not found"));
    }

    #[test]
    fn call_interrupted_by_modem_loss() {
        let (modem, rx) = modem();
        cmd(&modem, json!({"cmd":"incoming-call","number":NUMBER}));
        modem.answer(&CallHandle::from("c1")).unwrap();
        cmd(&modem, json!({"cmd":"set-modem","state":"failed"}));
        let states = call_states(&drain(&rx));
        assert_eq!(
            states.last().unwrap(),
            &(
                "c1".into(),
                CallState::Ended,
                Some(CallEndReason::ModemLost)
            )
        );
    }

    #[test]
    fn outgoing_call_no_service_when_unregistered() {
        let (modem, _rx) = modem();
        cmd(
            &modem,
            json!({"cmd":"set-registration","registration":"emergency-only"}),
        );
        assert_eq!(modem.dial(&number()), Err(BackendError::NoService));
        assert_eq!(modem.send_sms(&number(), "x"), Err(BackendError::NoService));
    }

    #[test]
    fn features_can_be_disabled_and_return_unsupported() {
        let modem = MockModem::new(MockTelephonyConfig {
            features: [TelephonyFeature::Voice].into_iter().collect(),
            ..MockTelephonyConfig::default()
        });
        let handle = modem.dial(&number()).unwrap();
        cmd(&modem, json!({"cmd":"tick","ms":OUTGOING_PROGRESS_MS}));
        assert_eq!(
            modem.hold(&handle),
            Err(BackendError::Unsupported(TelephonyFeature::Hold))
        );
        assert_eq!(
            modem.dtmf(&handle, Dtmf::new('1').unwrap()),
            Err(BackendError::Unsupported(TelephonyFeature::Dtmf))
        );
        assert_eq!(
            modem.send_sms(&number(), "x"),
            Err(BackendError::Unsupported(TelephonyFeature::Sms))
        );
        assert_eq!(
            modem.set_audio_route(AudioRoute::Speaker),
            Err(BackendError::Unsupported(TelephonyFeature::AudioRoute))
        );
        assert_eq!(
            modem.set_radio_power(RadioPower::Off),
            Err(BackendError::Unsupported(TelephonyFeature::RadioPower))
        );
        assert_eq!(
            modem.enter_pin("0000"),
            Err(BackendError::Unsupported(TelephonyFeature::SimPin))
        );
        assert!(
            cmd_err(
                &modem,
                json!({"cmd":"incoming-sms-multipart","number":NUMBER,"ref_id":1,"parts":["a"]})
            )
            .contains("multipart-sms")
        );
    }

    // --- SMS ---

    #[test]
    fn sms_incoming_single_and_multipart_forms() {
        let (modem, rx) = modem();
        cmd(
            &modem,
            json!({"cmd":"incoming-sms","number":NUMBER,"body":"hello"}),
        );
        assert_eq!(
            drain(&rx),
            [BackendEvent::SmsReceived {
                from: number(),
                body: "hello".into()
            }]
        );
        let events = cmd(
            &modem,
            json!({"cmd":"incoming-sms-multipart","number":NUMBER,"ref_id":7,"parts":["b","a"]}),
        );
        assert_eq!(events, ["sms-part-received", "sms-part-received"]);
        let parts: Vec<MultipartPart> = drain(&rx)
            .into_iter()
            .filter_map(|e| match e {
                BackendEvent::SmsPartReceived { part, .. } => Some(part),
                _ => None,
            })
            .collect();
        assert_eq!(parts[0].index, 1);
        assert_eq!(parts[0].count, 2);
        assert_eq!(parts[0].text, "b");
        assert_eq!(parts[1].index, 2);
        cmd(
            &modem,
            json!({"cmd":"incoming-sms-part","number":NUMBER,"ref_id":8,"index":2,"count":2,"text":"z"}),
        );
        assert!(matches!(
            drain(&rx)[0],
            BackendEvent::SmsPartReceived {
                part: MultipartPart {
                    ref_id: 8,
                    index: 2,
                    count: 2,
                    ..
                },
                ..
            }
        ));
        assert!(cmd_err(
            &modem,
            json!({"cmd":"incoming-sms-part","number":NUMBER,"ref_id":8,"index":3,"count":2,"text":"z"})
        )
        .contains("out of range"));
        cmd(&modem, json!({"cmd":"set-radio","power":"off"}));
        assert!(
            cmd_err(
                &modem,
                json!({"cmd":"incoming-sms","number":NUMBER,"body":"x"})
            )
            .contains("radio is off")
        );
    }

    #[test]
    fn sms_outgoing_success_failed_and_delayed_delivery() {
        let (modem, rx) = modem();
        let handle = modem.send_sms(&number(), "hi").unwrap();
        assert_eq!(handle, SmsHandle::from("m1"));
        cmd(&modem, json!({"cmd":"tick","ms":SMS_SEND_MS}));
        assert_eq!(
            drain(&rx)
                .iter()
                .filter(|e| matches!(e, BackendEvent::SmsChanged { .. }))
                .count(),
            1
        );
        cmd(&modem, json!({"cmd":"deliver-sms"}));
        assert!(matches!(
            drain(&rx)[0],
            BackendEvent::SmsChanged {
                state: SmsState::Delivered,
                ..
            }
        ));
        assert!(cmd_err(&modem, json!({"cmd":"deliver-sms"})).contains("no matching"));

        cmd(&modem, json!({"cmd":"set-sms-result","result":"failed"}));
        let handle = modem.send_sms(&number(), "hi").unwrap();
        cmd(&modem, json!({"cmd":"tick","ms":SMS_SEND_MS}));
        assert!(drain(&rx).iter().any(|e| e
            == &BackendEvent::SmsChanged {
                handle: handle.clone(),
                state: SmsState::Failed,
                failure: Some(SmsFailure::Rejected)
            }));

        cmd(
            &modem,
            json!({"cmd":"set-sms-result","result":"delayed-delivery"}),
        );
        let handle = modem.send_sms(&number(), "hi").unwrap();
        cmd(&modem, json!({"cmd":"tick","ms":SMS_SEND_MS}));
        assert!(matches!(
            drain(&rx)[0],
            BackendEvent::SmsChanged {
                state: SmsState::Sent,
                ..
            }
        ));
        cmd(
            &modem,
            json!({"cmd":"tick","ms":SMS_DELAYED_DELIVERY_MS - 1}),
        );
        assert!(
            drain(&rx)
                .iter()
                .all(|e| matches!(e, BackendEvent::Tick { .. }))
        );
        cmd(&modem, json!({"cmd":"tick","ms":1}));
        assert!(drain(&rx).iter().any(|e| e
            == &BackendEvent::SmsChanged {
                handle: handle.clone(),
                state: SmsState::Delivered,
                failure: None
            }));
    }

    #[test]
    fn fail_sms_command_parses_typed_reasons() {
        let (modem, rx) = modem();
        modem.send_sms(&number(), "hi").unwrap();
        cmd(
            &modem,
            json!({"cmd":"fail-sms","message_id":"m1","reason":"timed-out"}),
        );
        assert!(matches!(
            drain(&rx)[0],
            BackendEvent::SmsChanged {
                state: SmsState::Failed,
                failure: Some(SmsFailure::TimedOut),
                ..
            }
        ));
        cmd(&modem, json!({"cmd":"tick","ms":SMS_SEND_MS}));
        assert!(
            drain(&rx)
                .iter()
                .all(|e| matches!(e, BackendEvent::Tick { .. }))
        );
        modem.send_sms(&number(), "hi").unwrap();
        cmd(&modem, json!({"cmd":"fail-sms","reason":"carrier-said-no"}));
        assert!(matches!(
            drain(&rx)[0],
            BackendEvent::SmsChanged {
                failure: Some(SmsFailure::Failed { .. }),
                ..
            }
        ));
    }

    // --- radio / crash ---

    #[test]
    fn radios_disabled_and_re_enabled() {
        let (modem, rx) = modem();
        modem.send_sms(&number(), "hi").unwrap();
        modem.set_radio_power(RadioPower::Off).unwrap();
        let events = drain(&rx);
        assert!(events.contains(&BackendEvent::Radio(RadioPower::Off)));
        assert!(events.iter().any(|e| matches!(
            e,
            BackendEvent::SmsChanged {
                state: SmsState::Failed,
                failure: Some(SmsFailure::NoService),
                ..
            }
        )));
        assert_eq!(modem.snapshot().registration, Registration::Unknown);
        assert_eq!(modem.snapshot().signal.bars, 0);
        assert_eq!(modem.dial(&number()), Err(BackendError::NoService));
        cmd(&modem, json!({"cmd":"set-radio","power":"on"}));
        assert_eq!(modem.snapshot().registration, Registration::Searching);
        cmd(&modem, json!({"cmd":"tick","ms":REGISTER_MS}));
        assert!(modem.snapshot().registration.is_registered());
        assert_eq!(modem.snapshot().signal.bars, 3);
    }

    #[test]
    fn crash_backend_marks_failed_then_recovering_then_restarted() {
        let (modem, rx) = modem();
        cmd(&modem, json!({"cmd":"incoming-call","number":NUMBER}));
        let events = cmd(&modem, json!({"cmd":"crash-backend"}));
        assert_eq!(events[0], "backend-lost");
        assert!(matches!(modem.snapshot().modem, ModemState::Failed { .. }));
        let states = call_states(&drain(&rx));
        assert_eq!(states.last().unwrap().2, Some(CallEndReason::ModemLost));
        cmd(&modem, json!({"cmd":"tick","ms":1}));
        assert_eq!(modem.snapshot().modem, ModemState::Recovering);
        cmd(&modem, json!({"cmd":"tick","ms":RECOVER_MS}));
        let events = drain(&rx);
        assert!(events.contains(&BackendEvent::Modem(ModemState::Ready)));
        assert!(events.contains(&BackendEvent::BackendRestarted));
        assert_eq!(modem.snapshot().registration, Registration::Searching);
        cmd(&modem, json!({"cmd":"tick","ms":REGISTER_MS}));
        assert!(modem.snapshot().registration.is_registered());
        assert_eq!(modem.now_ms(), 1 + RECOVER_MS + REGISTER_MS);
    }

    #[test]
    fn audio_available_changes_move_the_active_route() {
        let (modem, rx) = modem();
        cmd(
            &modem,
            json!({"cmd":"set-audio-available","routes":["speaker","bluetooth"]}),
        );
        assert_eq!(modem.snapshot().audio.active, AudioRoute::Speaker);
        modem.set_audio_route(AudioRoute::Bluetooth).unwrap();
        assert!(
            drain(&rx)
                .iter()
                .all(|e| matches!(e, BackendEvent::Audio(_)))
        );
    }

    #[test]
    fn ticks_are_delivered_and_timers_fire_in_due_order() {
        let (modem, rx) = modem();
        cmd(
            &modem,
            json!({"cmd":"set-sms-result","result":"delayed-delivery"}),
        );
        modem.send_sms(&number(), "a").unwrap(); // sent @500, delivered @2500
        cmd(&modem, json!({"cmd":"incoming-call","number":NUMBER})); // no-answer @30000
        drain(&rx);
        cmd(&modem, json!({"cmd":"tick","ms":40_000}));
        let names: Vec<&str> = drain(&rx).iter().map(BackendEvent::name).collect();
        assert_eq!(
            names,
            ["sms-changed", "sms-changed", "call-changed", "tick"]
        );
        assert_eq!(modem.now_ms(), 40_000);
    }
}
