//! The telephony service: owns the public [`TelephonyState`], assigns call and
//! message identities, assembles multipart SMS, persists messages, and fans
//! events out to subscribers with per-capability redaction.
//!
//! The backend is a `ferrix_telephony_adapter_api::ModemBackend` trait object
//! composed by the binary; this module never names an implementation. Time
//! comes only from `BackendEvent::Tick`, so every timeout here is
//! deterministic under test.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_telephony_adapter_api::{
    BackendEvent, BackendSnapshot, CallHandle, ModemBackend, SmsHandle,
};
use ferrix_telephony_api::{
    AudioRoute, Call, CallEndReason, CallId, CallState, DataProfile, Direction, Dtmf, E164,
    MessageId, ModemState, MultipartPart, PartRef, PartyRef, RadioPower, SCHEMA_VERSION, Sms,
    SmsFailure, SmsState, TelephonyError, TelephonyEvent, TelephonyFeature, TelephonyState,
};
use ferrix_telephony_protocol::{Event, Reply, SmsFilter, StateView};

use crate::attention::Attention;
use crate::identity::{CAP_STATE, Identity, IdentityInfo};
use crate::records::Recorder;
use crate::store::{SmsStore, StoreDocument};

/// Bounded window for a concatenated SMS to complete (backend clock, ms).
pub const MULTIPART_WINDOW_MS: u64 = 60_000;

#[derive(Clone, Debug)]
pub struct ServiceOptions {
    pub multipart_window_ms: u64,
}

impl Default for ServiceOptions {
    fn default() -> Self {
        Self {
            multipart_window_ms: MULTIPART_WINDOW_MS,
        }
    }
}

/// A subscribed stream with the identity it was granted at subscribe time.
pub struct Subscriber {
    pub identity: IdentityInfo,
    pub peer_pid: Option<u32>,
    pub identity_source: Option<Arc<dyn Identity>>,
    pub sink: Box<dyn Write + Send>,
}

#[derive(Clone, Debug)]
struct PendingMultipart {
    from: E164,
    count: u8,
    parts: BTreeMap<u8, String>,
    first_seen_ms: u64,
}

struct State {
    telephony: TelephonyState,
    now_ms: u64,
    next_call: u64,
    next_message: u64,
    call_by_handle: BTreeMap<CallHandle, CallId>,
    handle_by_call: BTreeMap<CallId, CallHandle>,
    sms_by_handle: BTreeMap<SmsHandle, MessageId>,
    pending: BTreeMap<(String, u16), PendingMultipart>,
}

pub struct Service {
    backend: Arc<dyn ModemBackend>,
    store: SmsStore,
    recorder: Recorder,
    options: ServiceOptions,
    state: Mutex<State>,
    subscribers: Mutex<Vec<Subscriber>>,
    /// Names of events published while a host-control command runs
    /// ([`Service::capture_events`]); `None` when nothing is capturing.
    capture: Mutex<Option<Vec<String>>>,
    /// Sender into the event loop (set by [`Service::spawn_event_loop`]) and
    /// barrier bookkeeping for [`Service::settle`].
    loop_tx: Mutex<Option<Sender<BackendEvent>>>,
    barrier_seq: AtomicU64,
    barriers: Mutex<BTreeSet<u64>>,
    barrier_reached: Condvar,
    /// Alerted (outside every lock) when a new incoming call rings.
    attention: Mutex<Option<Arc<dyn Attention>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Projects the snapshot for a peer: full when it holds both `telephony-call`
/// and `telephony-sms`; full minus the other domain when it holds one; the
/// redacted form when it holds only `telephony-state`.
pub fn project_state(state: &TelephonyState, identity: &IdentityInfo) -> StateView {
    match (identity.has_call(), identity.has_sms()) {
        (true, true) => StateView::Full(state.clone()),
        (true, false) => {
            let mut s = state.clone();
            s.sms.clear();
            StateView::Full(s)
        }
        (false, true) => {
            let mut s = state.clone();
            s.calls.clear();
            StateView::Full(s)
        }
        (false, false) => StateView::Redacted(state.redacted()),
    }
}

/// Projects one event for a peer (see [`project_state`]).
pub fn project_event(event: &TelephonyEvent, identity: &IdentityInfo) -> Event {
    match event {
        TelephonyEvent::CallChanged { .. } if identity.has_call() => Event::Full(event.clone()),
        TelephonyEvent::SmsChanged { .. } if identity.has_sms() => Event::Full(event.clone()),
        TelephonyEvent::CallChanged { .. } | TelephonyEvent::SmsChanged { .. } => {
            Event::Redacted(event.redacted())
        }
        TelephonyEvent::ServiceRestarted { state } => match project_state(state, identity) {
            StateView::Full(state) => Event::Full(TelephonyEvent::ServiceRestarted { state }),
            StateView::Redacted(state) => Event::Redacted(
                ferrix_telephony_api::RedactedTelephonyEvent::ServiceRestarted { state },
            ),
        },
        other => Event::Full(other.clone()),
    }
}

impl Service {
    /// Builds the service: loads the store, reads the backend snapshot, and
    /// publishes `service-restarted`. Does not start the event loop; call
    /// [`Service::spawn_event_loop`] (the binary) or feed events directly
    /// through [`Service::handle_backend_event`] (tests).
    pub fn new(
        backend: Arc<dyn ModemBackend>,
        store: SmsStore,
        recorder: Recorder,
        options: ServiceOptions,
    ) -> Result<Arc<Self>, String> {
        let (document, _loaded) = store.load()?;
        let StoreDocument {
            next_message,
            messages,
            ..
        } = document;
        let mut telephony = TelephonyState::new();
        telephony.sms = messages;
        let mut state = State {
            telephony,
            now_ms: 0,
            next_call: 1,
            next_message: next_message.max(1),
            call_by_handle: BTreeMap::new(),
            handle_by_call: BTreeMap::new(),
            sms_by_handle: BTreeMap::new(),
            pending: BTreeMap::new(),
        };
        apply_snapshot(&mut state, backend.snapshot());
        let service = Arc::new(Self {
            backend,
            store,
            recorder,
            options,
            state: Mutex::new(state),
            subscribers: Mutex::new(Vec::new()),
            capture: Mutex::new(None),
            loop_tx: Mutex::new(None),
            barrier_seq: AtomicU64::new(0),
            barriers: Mutex::new(BTreeSet::new()),
            barrier_reached: Condvar::new(),
            attention: Mutex::new(None),
        });
        let snapshot = lock(&service.state).telephony.clone();
        service.publish(TelephonyEvent::ServiceRestarted { state: snapshot });
        Ok(service)
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    /// Composes the incoming-call alert (the binary: `ferrix-appd`).
    pub fn set_attention(&self, attention: Arc<dyn Attention>) {
        *lock(&self.attention) = Some(attention);
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn capabilities(&self) -> ferrix_telephony_api::Capabilities {
        lock(&self.state).telephony.capabilities.clone()
    }

    /// Subscribes to the backend and handles its events on a thread.
    pub fn spawn_event_loop(self: &Arc<Self>) {
        let (tx, rx) = channel();
        self.backend.subscribe(tx.clone());
        *lock(&self.loop_tx) = Some(tx);
        let service = Arc::clone(self);
        thread::Builder::new()
            .name("ferrix-telephonyd-events".to_owned())
            .spawn(move || service.run_event_loop(rx))
            .expect("spawn telephony event loop");
    }

    /// Handles events until the channel closes.
    pub fn run_event_loop(&self, rx: Receiver<BackendEvent>) {
        for event in rx {
            self.handle_backend_event(event);
        }
        *lock(&self.loop_tx) = None;
        self.recorder.backend_lost("event channel closed");
    }

    /// Waits until the event loop has handled everything the backend emitted
    /// before this call: a barrier token travels through the same channel,
    /// so ordering is exact. Used by the host mock-control channel so an ack
    /// is written only after the records the command caused. Returns `true`
    /// immediately when no event loop runs (tests drive events directly),
    /// `false` on timeout.
    pub fn settle(&self, timeout: Duration) -> bool {
        let Some(tx) = lock(&self.loop_tx).clone() else {
            return true;
        };
        let token = self.barrier_seq.fetch_add(1, Ordering::SeqCst) + 1;
        if tx.send(BackendEvent::Barrier { token }).is_err() {
            return false;
        }
        let deadline = Instant::now() + timeout;
        let mut reached = lock(&self.barriers);
        loop {
            if reached.remove(&token) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (guard, _) = self
                .barrier_reached
                .wait_timeout(reached, deadline - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            reached = guard;
        }
    }

    /// Full snapshot (callers project it per peer).
    pub fn state(&self) -> TelephonyState {
        lock(&self.state).telephony.clone()
    }

    pub fn hello_reply(&self, identity: &IdentityInfo) -> Reply {
        Reply::Hello {
            schema_version: SCHEMA_VERSION,
            evidence: self.recorder.evidence().to_owned(),
            state: project_state(&self.state(), identity),
        }
    }

    /// Registers a subscriber and writes the `ok` reply on its stream first.
    pub fn subscribe(
        &self,
        identity: IdentityInfo,
        sink: Box<dyn Write + Send>,
    ) -> std::io::Result<()> {
        self.register(Subscriber {
            identity,
            peer_pid: None,
            identity_source: None,
            sink,
        })
    }

    /// Production subscription with continuous authorization. The initial
    /// identity is retained only for the already-authorized subscribe reply;
    /// each event re-resolves the peer and drops the stream on revocation.
    pub fn subscribe_authorized(
        &self,
        peer_pid: Option<u32>,
        identity_source: Arc<dyn Identity>,
        identity: IdentityInfo,
        sink: Box<dyn Write + Send>,
    ) -> std::io::Result<()> {
        if !identity_source.identify(peer_pid).has(CAP_STATE) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "telephony-state grant was revoked",
            ));
        }
        self.register(Subscriber {
            identity,
            peer_pid,
            identity_source: Some(identity_source),
            sink,
        })
    }

    /// Writes the `ok` and registers under one lock: a client that has read
    /// the ack is already registered, and no event precedes the ack.
    fn register(&self, mut subscriber: Subscriber) -> std::io::Result<()> {
        let mut subscribers = lock(&self.subscribers);
        write_json(&mut subscriber.sink, &Reply::Ok)?;
        subscribers.push(subscriber);
        Ok(())
    }

    /// Runs `f` and returns the names of the events published meanwhile.
    /// Used by the host mock-control channel to ack with the events a local
    /// command caused. Captures do not nest; the caller serializes them.
    pub fn capture_events<T>(&self, f: impl FnOnce() -> T) -> (T, Vec<String>) {
        *lock(&self.capture) = Some(Vec::new());
        let result = f();
        let events = lock(&self.capture).take().unwrap_or_default();
        (result, events)
    }

    /// Ids and state names only, for mock-control acks.
    pub fn control_state_json(&self) -> serde_json::Value {
        let state = lock(&self.state);
        let t = &state.telephony;
        serde_json::json!({
            "now_ms": state.now_ms,
            "modem": t.modem.name(),
            "sim": t.sim.name(),
            "registration": t.registration.name(),
            "access": t.access.name(),
            "signal_bars": t.signal.bars,
            "radio": t.radio.name(),
            "audio": t.audio.active.name(),
            "muted": t.audio.muted,
            "calls": t.calls.values().map(|c| serde_json::json!({
                "call_id": c.id,
                "direction": c.direction.name(),
                "state": c.state.name(),
                "end_reason": c.end_reason.as_ref().map(CallEndReason::name),
            })).collect::<Vec<_>>(),
            "sms": t.sms.values().map(|m| serde_json::json!({
                "message_id": m.id,
                "direction": m.direction.name(),
                "state": m.state.name(),
                "failure": m.failure.as_ref().map(SmsFailure::name),
            })).collect::<Vec<_>>(),
        })
    }

    fn publish(&self, event: TelephonyEvent) {
        if let Some(capture) = lock(&self.capture).as_mut() {
            capture.push(event.name().to_owned());
        }
        self.recorder.telephony_event(&event);
        let mut subscribers = lock(&self.subscribers);
        subscribers.retain_mut(|subscriber| {
            if let Some(source) = &subscriber.identity_source {
                let identity = source.identify(subscriber.peer_pid);
                if !identity.has(CAP_STATE) {
                    return false;
                }
                subscriber.identity = identity;
            }
            let projected = project_event(&event, &subscriber.identity);
            write_json(&mut subscriber.sink, &projected).is_ok()
        });
    }

    fn persist(&self, state: &State) {
        let document = StoreDocument {
            schema_version: crate::store::STORE_SCHEMA_VERSION,
            next_message: state.next_message,
            messages: state.telephony.sms.clone(),
        };
        if let Err(reason) = self.store.save(&document) {
            self.recorder.store_error(&self.store.path(), &reason);
        }
    }

    // -----------------------------------------------------------------------
    // Backend events
    // -----------------------------------------------------------------------

    pub fn handle_backend_event(&self, event: BackendEvent) {
        let mut events = Vec::new();
        let mut ringing = false;
        {
            let mut state = lock(&self.state);
            let mut persist = false;
            match event {
                BackendEvent::Barrier { token } => {
                    drop(state);
                    lock(&self.barriers).insert(token);
                    self.barrier_reached.notify_all();
                    return;
                }
                BackendEvent::Tick { now_ms } => {
                    state.now_ms = now_ms;
                    persist |=
                        expire_multipart(&mut state, self.options.multipart_window_ms, &mut events);
                }
                BackendEvent::Modem(modem) => {
                    if state.telephony.modem != modem {
                        state.telephony.modem = modem.clone();
                        events.push(TelephonyEvent::ModemChanged { modem });
                    }
                }
                BackendEvent::Sim(sim) => {
                    if state.telephony.sim != sim {
                        state.telephony.sim = sim.clone();
                        events.push(TelephonyEvent::SimChanged { sim });
                    }
                }
                BackendEvent::Registration {
                    registration,
                    operator,
                    access,
                } => {
                    let t = &mut state.telephony;
                    if t.registration != registration
                        || t.operator != operator
                        || t.access != access
                    {
                        t.registration = registration;
                        t.operator = operator.clone();
                        t.access = access;
                        events.push(TelephonyEvent::RegistrationChanged {
                            registration,
                            operator,
                            access,
                        });
                    }
                }
                BackendEvent::Signal(signal) => {
                    if state.telephony.signal != signal {
                        state.telephony.signal = signal;
                        events.push(TelephonyEvent::SignalChanged { signal });
                    }
                }
                BackendEvent::Radio(radio) => {
                    if state.telephony.radio != radio {
                        state.telephony.radio = radio;
                        events.push(TelephonyEvent::RadioPowerChanged { radio });
                    }
                }
                BackendEvent::Data(data) => {
                    if state.telephony.data != data {
                        state.telephony.data = data.clone();
                        events.push(TelephonyEvent::DataChanged { data });
                    }
                }
                BackendEvent::Audio(audio) => {
                    if state.telephony.audio != audio {
                        state.telephony.audio = audio.clone();
                        events.push(TelephonyEvent::AudioRouteChanged { audio });
                    }
                }
                BackendEvent::Capabilities(capabilities) => {
                    if state.telephony.capabilities != capabilities {
                        state.telephony.capabilities = capabilities.clone();
                        events.push(TelephonyEvent::CapabilitiesChanged { capabilities });
                    }
                }
                BackendEvent::CallAdded {
                    handle,
                    direction,
                    number,
                    state: call_state,
                } => {
                    if !state.call_by_handle.contains_key(&handle) {
                        ringing |=
                            direction == Direction::Incoming && call_state == CallState::Ringing;
                        let call = new_call(&mut state, &handle, direction, number, call_state);
                        events.push(TelephonyEvent::CallChanged { call });
                    }
                }
                BackendEvent::CallChanged {
                    handle,
                    state: call_state,
                    end_reason,
                } => {
                    if let Some(call) = set_call_state(&mut state, &handle, call_state, end_reason)
                    {
                        events.push(TelephonyEvent::CallChanged { call });
                    }
                }
                BackendEvent::SmsReceived { from, body } => {
                    let sms = new_incoming_sms(&mut state, from, body, Vec::new(), None);
                    persist = true;
                    events.push(TelephonyEvent::SmsChanged { sms });
                }
                BackendEvent::SmsPartReceived { from, part } => {
                    if let Some(sms) = add_part(&mut state, from, part) {
                        persist = true;
                        events.push(TelephonyEvent::SmsChanged { sms });
                    }
                }
                BackendEvent::SmsChanged {
                    handle,
                    state: sms_state,
                    failure,
                } => {
                    if let Some(sms) = set_sms_state(&mut state, &handle, sms_state, failure) {
                        persist = true;
                        events.push(TelephonyEvent::SmsChanged { sms });
                    }
                }
                BackendEvent::BackendLost { reason } => {
                    self.recorder.backend_lost(&reason);
                    if !matches!(state.telephony.modem, ModemState::Failed { .. }) {
                        let modem = ModemState::Failed {
                            reason: reason.clone(),
                        };
                        state.telephony.modem = modem.clone();
                        events.push(TelephonyEvent::ModemChanged { modem });
                    }
                    persist |= lose_backend(&mut state, &mut events);
                    state.telephony.modem = ModemState::Recovering;
                    events.push(TelephonyEvent::ModemChanged {
                        modem: ModemState::Recovering,
                    });
                }
                BackendEvent::BackendRestarted => {
                    let snapshot = self.backend.snapshot();
                    apply_snapshot(&mut state, snapshot);
                    events.push(TelephonyEvent::ServiceRestarted {
                        state: state.telephony.clone(),
                    });
                }
            }
            if persist {
                self.persist(&state);
            }
        }
        for event in events {
            self.publish(event);
        }
        if ringing {
            let attention = lock(&self.attention).clone();
            if let Some(attention) = attention {
                attention.incoming_call();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Operations (already authorized by the server)
    // -----------------------------------------------------------------------

    fn require_feature(&self, feature: TelephonyFeature) -> Result<(), TelephonyError> {
        if lock(&self.state).telephony.capabilities.contains(&feature) {
            Ok(())
        } else {
            Err(TelephonyError::Unsupported { feature })
        }
    }

    pub fn dial(&self, number: &str) -> Result<Call, TelephonyError> {
        let number = E164::classify_dial_string(number)?;
        self.require_feature(TelephonyFeature::Voice)?;
        let handle = self.backend.dial(&number)?;
        let (call, event) = {
            let mut state = lock(&self.state);
            if let Some(id) = state.call_by_handle.get(&handle).cloned() {
                (state.telephony.calls[&id].clone(), None)
            } else {
                let call = new_call(
                    &mut state,
                    &handle,
                    Direction::Outgoing,
                    number,
                    CallState::Dialing,
                );
                (call.clone(), Some(TelephonyEvent::CallChanged { call }))
            }
        };
        if let Some(event) = event {
            self.publish(event);
        }
        Ok(call)
    }

    fn call_handle(&self, id: &CallId) -> Result<(CallHandle, CallState), TelephonyError> {
        let state = lock(&self.state);
        let handle = state
            .handle_by_call
            .get(id)
            .cloned()
            .ok_or_else(|| TelephonyError::not_found(id))?;
        let call_state = state.telephony.calls[id].state;
        Ok((handle, call_state))
    }

    fn call_op(
        &self,
        id: &CallId,
        op: &str,
        allowed: &[CallState],
        next: CallState,
        end_reason: Option<CallEndReason>,
        f: impl Fn(
            &dyn ModemBackend,
            &CallHandle,
        ) -> Result<(), ferrix_telephony_adapter_api::BackendError>,
    ) -> Result<Call, TelephonyError> {
        let (handle, current) = self.call_handle(id)?;
        if !allowed.contains(&current) {
            return Err(TelephonyError::invalid_state(current, op));
        }
        f(self.backend.as_ref(), &handle)?;
        let (call, event) = {
            let mut state = lock(&self.state);
            match set_call_state(&mut state, &handle, next, end_reason) {
                Some(call) => (call.clone(), Some(TelephonyEvent::CallChanged { call })),
                None => (state.telephony.calls[id].clone(), None),
            }
        };
        if let Some(event) = event {
            self.publish(event);
        }
        Ok(call)
    }

    pub fn answer(&self, id: &CallId) -> Result<Call, TelephonyError> {
        self.call_op(
            id,
            "answer",
            &[CallState::Ringing],
            CallState::Active,
            None,
            |b, h| b.answer(h),
        )
    }

    pub fn reject(&self, id: &CallId) -> Result<Call, TelephonyError> {
        self.call_op(
            id,
            "reject",
            &[CallState::Ringing],
            CallState::Ended,
            Some(CallEndReason::Rejected),
            |b, h| b.reject(h),
        )
    }

    pub fn hangup(&self, id: &CallId) -> Result<Call, TelephonyError> {
        self.call_op(
            id,
            "hangup",
            &[
                CallState::Dialing,
                CallState::Ringing,
                CallState::Active,
                CallState::Held,
            ],
            CallState::Ended,
            Some(CallEndReason::LocalHangup),
            |b, h| b.hangup(h),
        )
    }

    pub fn hold(&self, id: &CallId) -> Result<Call, TelephonyError> {
        self.require_feature(TelephonyFeature::Hold)?;
        self.call_op(
            id,
            "hold",
            &[CallState::Active],
            CallState::Held,
            None,
            |b, h| b.hold(h),
        )
    }

    pub fn resume(&self, id: &CallId) -> Result<Call, TelephonyError> {
        self.require_feature(TelephonyFeature::Hold)?;
        self.call_op(
            id,
            "resume",
            &[CallState::Held],
            CallState::Active,
            None,
            |b, h| b.resume(h),
        )
    }

    pub fn dtmf(&self, id: &CallId, digit: Dtmf) -> Result<(), TelephonyError> {
        self.require_feature(TelephonyFeature::Dtmf)?;
        let (handle, current) = self.call_handle(id)?;
        if current != CallState::Active {
            return Err(TelephonyError::invalid_state(current, "dtmf"));
        }
        self.backend.dtmf(&handle, digit)?;
        Ok(())
    }

    pub fn set_audio_route(&self, route: AudioRoute) -> Result<(), TelephonyError> {
        self.require_feature(TelephonyFeature::AudioRoute)?;
        self.backend.set_audio_route(route)?;
        Ok(())
    }

    /// Mutes or unmutes the live call's microphone. Refused while no call
    /// is live: mute is per call, never a standing setting.
    pub fn set_mic_muted(&self, muted: bool) -> Result<(), TelephonyError> {
        self.require_feature(TelephonyFeature::MicMute)?;
        if lock(&self.state).telephony.live_calls().next().is_none() {
            return Err(TelephonyError::invalid_state("idle", "set-mic-muted"));
        }
        self.backend.set_mic_muted(muted)?;
        Ok(())
    }

    pub fn set_radio_power(&self, power: RadioPower) -> Result<(), TelephonyError> {
        self.require_feature(TelephonyFeature::RadioPower)?;
        self.backend.set_radio_power(power)?;
        Ok(())
    }

    pub fn connect_data(&self, profile: &DataProfile) -> Result<(), TelephonyError> {
        self.require_feature(TelephonyFeature::Data)?;
        profile.validate().map_err(TelephonyError::protocol)?;
        self.backend.connect_data(profile)?;
        Ok(())
    }

    pub fn disconnect_data(&self) -> Result<(), TelephonyError> {
        self.require_feature(TelephonyFeature::Data)?;
        self.backend.disconnect_data()?;
        Ok(())
    }

    pub fn enter_pin(&self, pin: &str) -> Result<(), TelephonyError> {
        self.require_feature(TelephonyFeature::SimPin)?;
        if pin.is_empty() || pin.len() > 8 || !pin.bytes().all(|b| b.is_ascii_digit()) {
            return Err(TelephonyError::protocol("pin must be 1 to 8 digits"));
        }
        self.backend.enter_pin(pin)?;
        Ok(())
    }

    pub fn send_sms(&self, to: &str, body: &str) -> Result<Sms, TelephonyError> {
        let to = E164::classify_dial_string(to).map_err(|e| match e {
            TelephonyError::Unsupported { .. } => TelephonyError::InvalidNumber {
                reason: "emergency numbers cannot receive messages".to_owned(),
            },
            other => other,
        })?;
        self.require_feature(TelephonyFeature::Sms)?;
        if body.is_empty() {
            return Err(TelephonyError::protocol("body must not be empty"));
        }
        let handle = self.backend.send_sms(&to, body)?;
        let sms = {
            let mut state = lock(&self.state);
            if let Some(id) = state.sms_by_handle.get(&handle).cloned() {
                state.telephony.sms[&id].clone()
            } else {
                let id = next_message_id(&mut state);
                let sms = Sms {
                    id: id.clone(),
                    direction: Direction::Outgoing,
                    party: PartyRef::new(to),
                    body: body.to_owned(),
                    parts: Vec::new(),
                    state: SmsState::Sending,
                    timestamp_ms: state.now_ms,
                    failure: None,
                };
                state.sms_by_handle.insert(handle, id.clone());
                state.telephony.sms.insert(id, sms.clone());
                self.persist(&state);
                sms
            }
        };
        self.publish(TelephonyEvent::SmsChanged { sms: sms.clone() });
        Ok(sms)
    }

    /// Newest first; `total` counts matches before `limit`.
    pub fn list_sms(&self, filter: &SmsFilter) -> (Vec<Sms>, usize) {
        let state = lock(&self.state);
        let mut matches: Vec<Sms> = state
            .telephony
            .sms
            .values()
            .filter(|s| filter.party.as_ref().is_none_or(|p| &s.party.number == p))
            .filter(|s| !filter.unread_only || s.state == SmsState::Received)
            .cloned()
            .collect();
        matches.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms).then(b.id.cmp(&a.id)));
        let total = matches.len();
        if filter.limit > 0 {
            matches.truncate(filter.limit);
        }
        (matches, total)
    }

    pub fn mark_read(&self, id: &MessageId) -> Result<Sms, TelephonyError> {
        self.local_sms_transition(id, "mark-read", SmsState::Read)
    }

    pub fn delete_sms(&self, id: &MessageId) -> Result<Sms, TelephonyError> {
        self.local_sms_transition(id, "delete-sms", SmsState::Deleted)
    }

    fn local_sms_transition(
        &self,
        id: &MessageId,
        op: &str,
        next: SmsState,
    ) -> Result<Sms, TelephonyError> {
        let sms = {
            let mut state = lock(&self.state);
            let sms = state
                .telephony
                .sms
                .get_mut(id)
                .ok_or_else(|| TelephonyError::not_found(id))?;
            if !sms.state.can_transition_to(next) {
                return Err(TelephonyError::invalid_state(sms.state, op));
            }
            sms.state = next;
            let sms = sms.clone();
            if next == SmsState::Deleted {
                state.telephony.sms.remove(id);
                state.sms_by_handle.retain(|_, v| v != id);
            }
            self.persist(&state);
            sms
        };
        self.publish(TelephonyEvent::SmsChanged { sms: sms.clone() });
        Ok(sms)
    }
}

// ---------------------------------------------------------------------------
// State helpers (pure over `State`)
// ---------------------------------------------------------------------------

fn apply_snapshot(state: &mut State, snapshot: BackendSnapshot) {
    let BackendSnapshot {
        now_ms,
        modem,
        sim,
        registration,
        operator,
        access,
        signal,
        radio,
        data,
        audio,
        capabilities,
        calls,
    } = snapshot;
    state.now_ms = now_ms.max(state.now_ms);
    let t = &mut state.telephony;
    t.schema_version = SCHEMA_VERSION;
    t.modem = modem;
    t.sim = sim;
    t.registration = registration;
    t.operator = operator;
    t.access = access;
    t.signal = signal;
    t.radio = radio;
    t.data = data;
    t.audio = audio;
    t.capabilities = capabilities;
    // Live calls the backend no longer knows are lost.
    let known: Vec<CallHandle> = calls.iter().map(|c| c.handle.clone()).collect();
    let lost: Vec<CallHandle> = state
        .call_by_handle
        .keys()
        .filter(|h| !known.contains(h))
        .cloned()
        .collect();
    for handle in lost {
        set_call_state(
            state,
            &handle,
            CallState::Ended,
            Some(CallEndReason::ModemLost),
        );
    }
    for call in calls {
        if state.call_by_handle.contains_key(&call.handle) {
            set_call_state(state, &call.handle, call.state, call.end_reason);
        } else {
            let created = new_call(state, &call.handle, call.direction, call.number, call.state);
            if call.state == CallState::Ended {
                let id = created.id.clone();
                if let Some(c) = state.telephony.calls.get_mut(&id) {
                    c.end_reason = call.end_reason;
                }
            }
        }
    }
}

fn new_call(
    state: &mut State,
    handle: &CallHandle,
    direction: Direction,
    number: E164,
    call_state: CallState,
) -> Call {
    let id = CallId(format!("c{}", state.next_call));
    state.next_call += 1;
    let call = Call {
        id: id.clone(),
        direction,
        state: call_state,
        party: PartyRef::new(number),
        started_at_ms: state.now_ms,
        end_reason: None,
    };
    state.call_by_handle.insert(handle.clone(), id.clone());
    state.handle_by_call.insert(id.clone(), handle.clone());
    state.telephony.calls.insert(id, call.clone());
    call
}

/// Applies a transition if it is legal and changes something.
fn set_call_state(
    state: &mut State,
    handle: &CallHandle,
    next: CallState,
    end_reason: Option<CallEndReason>,
) -> Option<Call> {
    let id = state.call_by_handle.get(handle)?.clone();
    let call = state.telephony.calls.get_mut(&id)?;
    if call.state == next {
        return None;
    }
    if !call.state.can_transition_to(next) {
        return None;
    }
    call.state = next;
    call.end_reason = if next == CallState::Ended {
        end_reason.or(Some(CallEndReason::Failed {
            reason: "unspecified".to_owned(),
        }))
    } else {
        None
    };
    Some(call.clone())
}

fn next_message_id(state: &mut State) -> MessageId {
    let id = MessageId(format!("m{}", state.next_message));
    state.next_message += 1;
    id
}

fn new_incoming_sms(
    state: &mut State,
    from: E164,
    body: String,
    parts: Vec<PartRef>,
    failure: Option<SmsFailure>,
) -> Sms {
    let id = next_message_id(state);
    let sms = Sms {
        id: id.clone(),
        direction: Direction::Incoming,
        party: PartyRef::new(from),
        body,
        parts,
        state: if failure.is_some() {
            SmsState::Failed
        } else {
            SmsState::Received
        },
        timestamp_ms: state.now_ms,
        failure,
    };
    state.telephony.sms.insert(id, sms.clone());
    sms
}

fn set_sms_state(
    state: &mut State,
    handle: &SmsHandle,
    next: SmsState,
    failure: Option<SmsFailure>,
) -> Option<Sms> {
    let id = state.sms_by_handle.get(handle)?.clone();
    let sms = state.telephony.sms.get_mut(&id)?;
    if sms.state == next || !sms.state.can_transition_to(next) {
        return None;
    }
    sms.state = next;
    sms.failure = if next == SmsState::Failed {
        failure.or(Some(SmsFailure::Failed {
            reason: "unspecified".to_owned(),
        }))
    } else {
        None
    };
    Some(sms.clone())
}

/// Adds a segment; returns the assembled message once all parts are present.
fn add_part(state: &mut State, from: E164, part: MultipartPart) -> Option<Sms> {
    if part.count == 0 || part.index == 0 || part.index > part.count {
        return None;
    }
    let key = (from.as_str().to_owned(), part.ref_id);
    let now = state.now_ms;
    let pending = state
        .pending
        .entry(key.clone())
        .or_insert(PendingMultipart {
            from: from.clone(),
            count: part.count,
            parts: BTreeMap::new(),
            first_seen_ms: now,
        });
    pending.parts.insert(part.index, part.text);
    if pending.parts.len() < usize::from(pending.count) {
        return None;
    }
    let pending = state.pending.remove(&key)?;
    let (body, refs) = assemble(&pending, part.ref_id);
    Some(new_incoming_sms(state, pending.from, body, refs, None))
}

fn assemble(pending: &PendingMultipart, ref_id: u16) -> (String, Vec<PartRef>) {
    let body = pending.parts.values().cloned().collect::<Vec<_>>().concat();
    let refs = pending
        .parts
        .keys()
        .map(|index| PartRef {
            ref_id,
            index: *index,
            count: pending.count,
        })
        .collect();
    (body, refs)
}

/// Fails concatenations older than the window. Returns whether anything
/// changed (the caller persists).
fn expire_multipart(state: &mut State, window_ms: u64, events: &mut Vec<TelephonyEvent>) -> bool {
    let now = state.now_ms;
    let expired: Vec<(String, u16)> = state
        .pending
        .iter()
        .filter(|(_, p)| now.saturating_sub(p.first_seen_ms) >= window_ms)
        .map(|(k, _)| k.clone())
        .collect();
    let changed = !expired.is_empty();
    for key in expired {
        if let Some(pending) = state.pending.remove(&key) {
            let (body, refs) = assemble(&pending, key.1);
            let sms = new_incoming_sms(
                state,
                pending.from,
                body,
                refs,
                Some(SmsFailure::incomplete_multipart()),
            );
            events.push(TelephonyEvent::SmsChanged { sms });
        }
    }
    changed
}

/// Ends live calls and fails in-flight messages when the backend is gone.
fn lose_backend(state: &mut State, events: &mut Vec<TelephonyEvent>) -> bool {
    let live: Vec<CallHandle> = state
        .telephony
        .calls
        .values()
        .filter(|c| c.state.is_live())
        .filter_map(|c| state.handle_by_call.get(&c.id).cloned())
        .collect();
    for handle in live {
        if let Some(call) = set_call_state(
            state,
            &handle,
            CallState::Ended,
            Some(CallEndReason::ModemLost),
        ) {
            events.push(TelephonyEvent::CallChanged { call });
        }
    }
    let sending: Vec<SmsHandle> = state
        .sms_by_handle
        .iter()
        .filter(|(_, id)| {
            state
                .telephony
                .sms
                .get(*id)
                .is_some_and(|s| s.state == SmsState::Sending)
        })
        .map(|(h, _)| h.clone())
        .collect();
    let changed = !sending.is_empty();
    for handle in sending {
        if let Some(sms) = set_sms_state(
            state,
            &handle,
            SmsState::Failed,
            Some(SmsFailure::ModemLost),
        ) {
            events.push(TelephonyEvent::SmsChanged { sms });
        }
    }
    changed
}

fn write_json(
    sink: &mut (impl Write + ?Sized),
    value: &impl serde::Serialize,
) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(std::io::Error::other)?;
    text.push('\n');
    sink.write_all(text.as_bytes())?;
    sink.flush()
}
