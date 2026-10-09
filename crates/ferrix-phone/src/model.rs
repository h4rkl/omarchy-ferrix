//! Phone view model: a pure state machine over the telephony snapshot,
//! contact names, and the dialer buffer. No I/O; the session owner feeds
//! [`Model::apply`] with protocol updates and drains [`Model::pending`].
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::BTreeMap;

use ferrix_contacts_api::Contact;
use ferrix_telephony_api::{
    AudioRoute, Call, CallEndReason, CallId, CallState, Direction, E164, ModemState, RadioPower,
    RedactedCall, RedactedTelephonyState, Registration, SimState, TelephonyEvent, TelephonyFeature,
    TelephonyState, is_emergency_number,
};
use ferrix_telephony_protocol::{Event, StateView, Update};

/// Calls kept on the Recent page.
pub const RECENT_LIMIT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dialer,
    Recent,
    Incoming,
    InCall,
    Ended,
}

impl Page {
    pub fn name(self) -> &'static str {
        match self {
            Page::Dialer => "dialer",
            Page::Recent => "recent",
            Page::Incoming => "incoming",
            Page::InCall => "in-call",
            Page::Ended => "ended",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "dialer" => Page::Dialer,
            "recent" => Page::Recent,
            "incoming" => Page::Incoming,
            "in-call" => Page::InCall,
            "ended" => Page::Ended,
            _ => return None,
        })
    }
}

/// Link to `ferrix-telephonyd`, as shown in the header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Connecting,
    Connected,
    /// A snapshot arrived after a loss: state was replaced.
    Reconnected,
    Unavailable,
}

impl Link {
    pub fn name(self) -> &'static str {
        match self {
            Link::Connecting => "connecting",
            Link::Connected => "connected",
            Link::Reconnected => "reconnected",
            Link::Unavailable => "unavailable",
        }
    }
}

/// Why the Call button is disabled, in the order checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialBlock {
    Unavailable,
    NoModem,
    RadioOff,
    NoSim,
    SimLocked,
    NoService,
    CallInProgress,
    Empty,
    Emergency,
    Invalid,
}

impl DialBlock {
    pub fn reason(self) -> &'static str {
        match self {
            DialBlock::Unavailable => "telephony \u{b7} unavailable",
            DialBlock::NoModem => "modem \u{b7} absent",
            DialBlock::RadioOff => "radio \u{b7} off",
            DialBlock::NoSim => "sim \u{b7} absent",
            DialBlock::SimLocked => "sim \u{b7} locked",
            DialBlock::NoService => "service \u{b7} none",
            DialBlock::CallInProgress => "call \u{b7} in progress",
            DialBlock::Empty => "number \u{b7} empty",
            DialBlock::Emergency => "Emergency calling not supported",
            DialBlock::Invalid => "number \u{b7} invalid",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DialBlock::Unavailable => "unavailable",
            DialBlock::NoModem => "no-modem",
            DialBlock::RadioOff => "radio-off",
            DialBlock::NoSim => "no-sim",
            DialBlock::SimLocked => "sim-locked",
            DialBlock::NoService => "no-service",
            DialBlock::CallInProgress => "call-in-progress",
            DialBlock::Empty => "empty",
            DialBlock::Emergency => "emergency",
            DialBlock::Invalid => "invalid",
        }
    }
}

/// Work for the session owner to send to the daemons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Dial(String),
    Answer(CallId),
    Reject(CallId),
    Hangup(CallId),
    Hold(CallId),
    Resume(CallId),
    Dtmf(CallId, char),
    SetRoute(AudioRoute),
    /// Mute or unmute the live call's microphone.
    SetMuted(bool),
    /// Yield the foreground to the home surface.
    Back,
}

/// A call as the UI shows it: redacted party plus the full number when the
/// grant allowed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallView {
    pub call: RedactedCall,
    pub number: Option<E164>,
}

impl CallView {
    /// Contact name when resolvable, then the daemon's display, then the
    /// redacted number. The full number is never shown unless no contact
    /// matched and the grant provided it.
    pub fn party_label(&self, contacts: &[Contact], region: &str) -> String {
        if let Some(number) = &self.number
            && let Some(contact) = ferrix_contacts_api::lookup(contacts, number.as_str(), region)
        {
            return contact.display_name.clone();
        }
        if let Some(display) = &self.call.party.display {
            return display.clone();
        }
        match &self.number {
            Some(number) => number.as_str().to_owned(),
            None => self.call.party.party_redacted.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub page: Page,
    pub link: Link,
    pub state: Option<RedactedTelephonyState>,
    pub calls: BTreeMap<CallId, CallView>,
    pub contacts: Vec<Contact>,
    pub region: String,
    /// The dialer buffer (digits, `*`, `#`, a leading `+`).
    pub typed: String,
    /// The call shown on Incoming/InCall/Ended.
    pub focused_call: Option<CallId>,
    /// Whether the in-call DTMF keypad is open.
    pub dtmf_open: bool,
    pub pending: Vec<Action>,
    /// Last typed error from a request, shown until the next action.
    pub last_error: Option<String>,
    /// Whether a link loss happened since the last snapshot.
    lost_once: bool,
    /// The app's monotonic clock in milliseconds, set by the session owner
    /// ([`Model::set_clock`]). Call durations are measured on it: the
    /// daemon reports only a call's start, never when it connected.
    clock_ms: u64,
    /// Local connect/end times of calls observed by this app.
    timers: BTreeMap<CallId, CallTimer>,
}

/// When this app saw a call become active and end, on [`Model::clock_ms`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CallTimer {
    active_at_ms: Option<u64>,
    ended_at_ms: Option<u64>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            page: Page::Dialer,
            link: Link::Connecting,
            state: None,
            calls: BTreeMap::new(),
            contacts: Vec::new(),
            region: ferrix_contacts_api::DEFAULT_REGION.to_owned(),
            typed: String::new(),
            focused_call: None,
            dtmf_open: false,
            pending: Vec::new(),
            last_error: None,
            lost_once: false,
            clock_ms: 0,
            timers: BTreeMap::new(),
        }
    }

    /// `page · state` names for the `ui-state` record.
    pub fn state_name(&self) -> String {
        match self.page {
            Page::Dialer => match self.dial_block() {
                Ok(()) => "ready".to_owned(),
                Err(block) => block.name().to_owned(),
            },
            Page::Recent => format!("{}-calls", self.recent().len()),
            Page::Incoming => "ringing".to_owned(),
            Page::InCall => {
                let state = self.focused().map_or("none", |c| c.call.state.name());
                if self.muted() {
                    format!("{state}-muted")
                } else {
                    state.to_owned()
                }
            }
            Page::Ended => self
                .focused()
                .and_then(|c| c.call.end_reason.as_ref())
                .map_or("unknown", CallEndReason::name)
                .to_owned(),
        }
    }

    pub fn link_label(&self) -> String {
        format!("telephony \u{b7} {}", self.link.name())
    }

    // -- telephony updates ---------------------------------------------------

    fn replace_state(&mut self, view: &StateView) {
        self.state = Some(view.redacted());
        self.calls.clear();
        match view {
            StateView::Full(full) => {
                for (id, call) in &full.calls {
                    self.calls.insert(
                        id.clone(),
                        CallView {
                            call: call.redacted(),
                            number: Some(call.party.number.clone()),
                        },
                    );
                }
            }
            StateView::Redacted(redacted) => {
                for (id, call) in &redacted.calls {
                    self.calls.insert(
                        id.clone(),
                        CallView {
                            call: call.clone(),
                            number: None,
                        },
                    );
                }
            }
        }
        self.timers.retain(|id, _| self.calls.contains_key(id));
        let observed: Vec<(CallId, CallState)> = self
            .calls
            .values()
            .map(|c| (c.call.id.clone(), c.call.state))
            .collect();
        for (id, state) in observed {
            self.observe(&id, state);
        }
        self.refocus();
    }

    /// Records the local connect/end time of a call state change. Calls
    /// first seen already ended keep no timer: their duration is unknown.
    fn observe(&mut self, id: &CallId, state: CallState) {
        let now = self.clock_ms;
        match state {
            CallState::Active | CallState::Held => {
                let timer = self.timers.entry(id.clone()).or_default();
                timer.active_at_ms.get_or_insert(now);
            }
            CallState::Ended => {
                if let Some(timer) = self.timers.get_mut(id)
                    && timer.active_at_ms.is_some()
                {
                    timer.ended_at_ms.get_or_insert(now);
                }
            }
            CallState::Dialing | CallState::Ringing => {}
        }
    }

    fn apply_call(&mut self, call: RedactedCall, number: Option<E164>) {
        let id = call.id.clone();
        let ended_now = call.state == CallState::Ended;
        let incoming_ringing =
            call.direction == Direction::Incoming && call.state == CallState::Ringing;
        let was_live = self.calls.get(&id).is_some_and(|c| c.call.state.is_live());
        self.observe(&id, call.state);
        self.calls.insert(id.clone(), CallView { call, number });
        if ended_now {
            if self.focused_call.as_ref() == Some(&id) || (was_live && self.focused_call.is_none())
            {
                self.focused_call = Some(id);
                self.page = Page::Ended;
                self.dtmf_open = false;
            }
            return;
        }
        if incoming_ringing && self.focused_call.is_none() {
            self.focused_call = Some(id);
            self.page = Page::Incoming;
            return;
        }
        self.refocus();
    }

    /// Keeps the focused call and page consistent with the live calls.
    fn refocus(&mut self) {
        let live: Vec<&CallView> = self
            .calls
            .values()
            .filter(|c| c.call.state.is_live())
            .collect();
        let focused_live = self
            .focused_call
            .as_ref()
            .and_then(|id| self.calls.get(id))
            .is_some_and(|c| c.call.state.is_live());
        if !focused_live {
            if let Some(first) = live.first() {
                self.focused_call = Some(first.call.id.clone());
            } else if self.page != Page::Ended {
                self.focused_call = None;
                if matches!(self.page, Page::Incoming | Page::InCall) {
                    self.page = Page::Dialer;
                }
                return;
            }
        }
        if let Some(view) = self.focused() {
            self.page = match (view.call.direction, view.call.state) {
                (Direction::Incoming, CallState::Ringing) => Page::Incoming,
                (_, CallState::Ended) => Page::Ended,
                _ => Page::InCall,
            };
        }
    }

    /// Folds one reconnector update. Returns whether anything visible changed.
    pub fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Snapshot(view) => {
                self.link = if self.lost_once {
                    Link::Reconnected
                } else {
                    Link::Connected
                };
                self.lost_once = false;
                self.replace_state(&view);
                true
            }
            Update::Disconnected => {
                self.link = Link::Unavailable;
                self.lost_once = true;
                true
            }
            Update::Event(Event::Full(TelephonyEvent::ServiceRestarted { state })) => {
                self.link = Link::Reconnected;
                self.replace_state(&StateView::Full(state));
                true
            }
            Update::Event(Event::Redacted(
                ferrix_telephony_api::RedactedTelephonyEvent::ServiceRestarted { state },
            )) => {
                self.link = Link::Reconnected;
                self.replace_state(&StateView::Redacted(state));
                true
            }
            Update::Event(Event::Full(TelephonyEvent::CallChanged { call })) => {
                let number = call.party.number.clone();
                if let Some(state) = self.state.as_mut() {
                    TelephonyEvent::CallChanged { call: call.clone() }
                        .redacted()
                        .apply_to(state);
                }
                self.apply_call(call.redacted(), Some(number));
                true
            }
            Update::Event(Event::Redacted(
                ferrix_telephony_api::RedactedTelephonyEvent::CallChanged { call },
            )) => {
                if let Some(state) = self.state.as_mut() {
                    ferrix_telephony_api::RedactedTelephonyEvent::CallChanged {
                        call: call.clone(),
                    }
                    .apply_to(state);
                }
                self.apply_call(call, None);
                true
            }
            Update::Event(event) => {
                if let Some(state) = self.state.as_mut() {
                    event.apply_redacted(state);
                }
                true
            }
        }
    }

    pub fn set_contacts(&mut self, contacts: Vec<Contact>) {
        self.contacts = contacts;
    }

    // -- derived views -------------------------------------------------------

    pub fn focused(&self) -> Option<&CallView> {
        self.focused_call.as_ref().and_then(|id| self.calls.get(id))
    }

    pub fn live_call(&self) -> Option<&CallView> {
        self.calls.values().find(|c| c.call.state.is_live())
    }

    /// Ended and live calls newest first, at most [`RECENT_LIMIT`].
    pub fn recent(&self) -> Vec<&CallView> {
        let mut calls: Vec<&CallView> = self.calls.values().collect();
        calls.sort_by(|a, b| {
            b.call
                .started_at_ms
                .cmp(&a.call.started_at_ms)
                .then_with(|| a.call.id.as_str().cmp(b.call.id.as_str()))
        });
        calls.truncate(RECENT_LIMIT);
        calls
    }

    /// Advances the local monotonic clock (milliseconds since the app
    /// started). Never goes backwards.
    pub fn set_clock(&mut self, now_ms: u64) {
        self.clock_ms = self.clock_ms.max(now_ms);
    }

    /// Seconds a call has been connected by the local clock: running while
    /// active or held, frozen once it ended. `None` when it never connected
    /// while this app watched (still dialing/ringing, or ended before).
    pub fn duration_secs(&self, id: &CallId) -> Option<u64> {
        let timer = self.timers.get(id)?;
        let start = timer.active_at_ms?;
        let end = timer.ended_at_ms.unwrap_or(self.clock_ms);
        Some(end.saturating_sub(start) / 1000)
    }

    /// Whether a connected call's timer is running, so the owner should
    /// redraw once per second.
    pub fn timer_running(&self) -> bool {
        self.calls.values().any(|c| {
            c.call.state.is_live()
                && self
                    .timers
                    .get(&c.call.id)
                    .is_some_and(|t| t.active_at_ms.is_some() && t.ended_at_ms.is_none())
        })
    }

    /// Whether the typed number could be dialed now, or why not.
    pub fn dial_block(&self) -> Result<(), DialBlock> {
        let Some(state) = &self.state else {
            return Err(DialBlock::Unavailable);
        };
        if self.link == Link::Unavailable {
            return Err(DialBlock::Unavailable);
        }
        if state.modem == ModemState::Absent {
            return Err(DialBlock::NoModem);
        }
        if state.radio == RadioPower::Off {
            return Err(DialBlock::RadioOff);
        }
        match state.sim {
            SimState::Absent => return Err(DialBlock::NoSim),
            SimState::Locked { .. } | SimState::Blocked => return Err(DialBlock::SimLocked),
            _ => {}
        }
        if !matches!(state.registration, Registration::Registered { .. }) {
            return Err(DialBlock::NoService);
        }
        if self.live_call().is_some() {
            return Err(DialBlock::CallInProgress);
        }
        let digits: String = self.typed.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err(DialBlock::Empty);
        }
        if is_emergency_number(&digits) {
            return Err(DialBlock::Emergency);
        }
        let dial = self.dial_string();
        if E164::new(&dial).is_err() {
            return Err(DialBlock::Invalid);
        }
        Ok(())
    }

    /// The typed number in E.164 form when possible: `+` passes through;
    /// national digits are normalized for the region.
    pub fn dial_string(&self) -> String {
        if self.typed.starts_with('+') {
            return self.typed.clone();
        }
        ferrix_contacts_api::normalize(&self.typed, &self.region)
            .unwrap_or_else(|| self.typed.clone())
    }

    pub fn typed_label(&self) -> String {
        if self.typed.is_empty() {
            return String::new();
        }
        match ferrix_contacts_api::lookup(&self.contacts, &self.dial_string(), &self.region) {
            Some(contact) => contact.display_name.clone(),
            None => String::new(),
        }
    }

    pub fn routes_available(&self) -> Vec<AudioRoute> {
        self.state
            .as_ref()
            .map(|s| s.audio.available.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn active_route(&self) -> Option<AudioRoute> {
        self.state.as_ref().map(|s| s.audio.active)
    }

    /// Whether the call microphone is muted, as the daemon reports it.
    pub fn muted(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.audio.muted)
    }

    /// Mute needs the backend feature and a connected focused call.
    pub fn can_mute(&self) -> bool {
        self.supports(TelephonyFeature::MicMute)
            && self
                .focused()
                .is_some_and(|c| matches!(c.call.state, CallState::Active | CallState::Held))
    }

    pub fn supports(&self, feature: TelephonyFeature) -> bool {
        self.state
            .as_ref()
            .is_some_and(|s| s.capabilities.contains(&feature))
    }

    // -- user intents --------------------------------------------------------

    pub fn key(&mut self, ch: char) {
        if self.typed.chars().count() >= 20 {
            return;
        }
        // A long-press `0` would be `+`; on this keypad `*` twice at the
        // start becomes `+`, keeping the layout to twelve keys.
        if ch == '*' && self.typed == "*" {
            self.typed = "+".to_owned();
            return;
        }
        self.typed.push(ch);
        self.last_error = None;
    }

    pub fn delete(&mut self) {
        self.typed.pop();
    }

    pub fn clear(&mut self) {
        self.typed.clear();
    }

    /// Dial the typed number; refused with the typed reason otherwise.
    pub fn call(&mut self) -> Result<(), DialBlock> {
        self.dial_block()?;
        self.pending.push(Action::Dial(self.dial_string()));
        Ok(())
    }

    pub fn answer(&mut self) {
        if let Some(id) = self.focused_call.clone() {
            self.pending.push(Action::Answer(id));
        }
    }

    pub fn reject(&mut self) {
        if let Some(id) = self.focused_call.clone() {
            self.pending.push(Action::Reject(id));
        }
    }

    pub fn hangup(&mut self) {
        if let Some(id) = self.focused_call.clone() {
            self.pending.push(Action::Hangup(id));
        }
    }

    pub fn toggle_hold(&mut self) {
        let Some(view) = self.focused() else {
            return;
        };
        let id = view.call.id.clone();
        match view.call.state {
            CallState::Active => self.pending.push(Action::Hold(id)),
            CallState::Held => self.pending.push(Action::Resume(id)),
            _ => {}
        }
    }

    pub fn dtmf(&mut self, ch: char) {
        if let Some(view) = self.focused()
            && view.call.state == CallState::Active
        {
            self.pending.push(Action::Dtmf(view.call.id.clone(), ch));
        }
    }

    pub fn toggle_mute(&mut self) {
        if self.can_mute() {
            self.pending.push(Action::SetMuted(!self.muted()));
        }
    }

    pub fn set_route(&mut self, route: AudioRoute) {
        self.pending.push(Action::SetRoute(route));
    }

    /// Leaves the Ended page and clears the buffer of a finished call.
    pub fn dismiss_ended(&mut self) {
        if self.page == Page::Ended {
            self.focused_call = None;
            self.page = Page::Dialer;
            self.refocus();
        }
    }

    /// Fills the dialer from a Recent entry.
    pub fn redial(&mut self, id: &CallId) {
        if let Some(view) = self.calls.get(id)
            && let Some(number) = &view.number
        {
            self.typed = number.as_str().to_owned();
            self.page = Page::Dialer;
        }
    }

    /// Header chevron or left-edge swipe: leave a sub-page, else yield.
    pub fn back(&mut self) {
        match self.page {
            Page::Recent => self.page = Page::Dialer,
            Page::Ended => self.dismiss_ended(),
            Page::Dialer | Page::Incoming | Page::InCall => self.pending.push(Action::Back),
        }
    }

    pub fn show_recent(&mut self) {
        if self.live_call().is_none() {
            self.page = Page::Recent;
        }
    }

    pub fn show_dialer(&mut self) {
        if self.live_call().is_none() {
            self.page = Page::Dialer;
        }
    }
}

impl Model {
    /// Applies an intent `ferrix-appd` delivered (at launch or while
    /// running). `dial` fills the dialer buffer the way typing would and
    /// never places the call; it waits while a call is live. `show-call`
    /// brings the focused live or ringing call forward. Returns whether
    /// anything visible changed.
    pub fn apply_intent(&mut self, intent: &ferrix_app_api::AppIntent) -> bool {
        use ferrix_app_api::AppIntent;
        match intent {
            AppIntent::Dial { number } => {
                self.typed.clear();
                for ch in number.chars() {
                    if ch.is_ascii_digit() || matches!(ch, '*' | '#') {
                        self.key(ch);
                    } else if ch == '+' && self.typed.is_empty() {
                        self.typed.push('+');
                    }
                }
                if self.live_call().is_none() {
                    if self.page == Page::Ended {
                        self.dismiss_ended();
                    }
                    self.page = Page::Dialer;
                }
                true
            }
            AppIntent::ShowCall => {
                self.refocus();
                true
            }
            AppIntent::Compose { .. } => false,
        }
    }
}

/// `m:ss`, or `h:mm:ss` from an hour.
pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A preview snapshot for the host: a ready mock modem with one call.
pub fn preview_state(
    call: Option<(Direction, CallState, Option<CallEndReason>)>,
) -> TelephonyState {
    use ferrix_telephony_api::{AccessTechnology, AudioRouteState, Operator, PartyRef, Signal};
    let mut state = TelephonyState::new();
    state.modem = ModemState::Ready;
    state.sim = SimState::Ready;
    state.registration = Registration::Registered { roaming: false };
    state.operator = Some(Operator {
        name: "Ferrix Mock".to_owned(),
        mcc_mnc: None,
    });
    state.access = AccessTechnology::Lte;
    state.signal = Signal::bars(3);
    state.radio = RadioPower::On;
    state.audio = AudioRouteState {
        active: AudioRoute::Earpiece,
        available: [
            AudioRoute::Earpiece,
            AudioRoute::Speaker,
            AudioRoute::Bluetooth,
        ]
        .into_iter()
        .collect(),
        muted: false,
    };
    state.capabilities = [
        TelephonyFeature::Voice,
        TelephonyFeature::Dtmf,
        TelephonyFeature::Hold,
        TelephonyFeature::AudioRoute,
        TelephonyFeature::MicMute,
    ]
    .into_iter()
    .collect();
    let number = E164::new("+15550100042").expect("valid synthetic number");
    for (i, secs) in [900_u64, 620, 300].iter().enumerate() {
        let id = CallId(format!("call-{i}"));
        state.calls.insert(
            id.clone(),
            Call {
                id,
                direction: if i % 2 == 0 {
                    Direction::Outgoing
                } else {
                    Direction::Incoming
                },
                state: CallState::Ended,
                party: PartyRef::new(E164::new(format!("+1555010{:04}", i + 1)).unwrap()),
                started_at_ms: 1_700_000_000_000 - secs * 1000,
                end_reason: Some(match i {
                    0 => CallEndReason::RemoteHangup,
                    1 => CallEndReason::NoAnswer,
                    _ => CallEndReason::Busy,
                }),
            },
        );
    }
    if let Some((direction, call_state, end_reason)) = call {
        state.calls.insert(
            CallId("call-live".to_owned()),
            Call {
                id: CallId("call-live".to_owned()),
                direction,
                state: call_state,
                party: PartyRef::new(number),
                started_at_ms: 1_700_000_000_000,
                end_reason,
            },
        );
        state.sms.clear();
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_telephony_api::PartyRef;

    fn connected(call: Option<(Direction, CallState, Option<CallEndReason>)>) -> Model {
        let mut m = Model::new();
        m.apply(Update::Snapshot(StateView::Full(preview_state(call))));
        m
    }

    fn call_event(
        id: &str,
        direction: Direction,
        state: CallState,
        reason: Option<CallEndReason>,
    ) -> Update {
        Update::Event(Event::Full(TelephonyEvent::CallChanged {
            call: Call {
                id: CallId(id.to_owned()),
                direction,
                state,
                party: PartyRef::new(E164::new("+15550100042").unwrap()),
                started_at_ms: 1_700_000_000_500,
                end_reason: reason,
            },
        }))
    }

    #[test]
    fn dial_blocks_are_typed_and_ordered() {
        let mut m = Model::new();
        assert_eq!(m.dial_block(), Err(DialBlock::Unavailable));
        m = connected(None);
        assert_eq!(m.dial_block(), Err(DialBlock::Empty));
        for ch in "911".chars() {
            m.key(ch);
        }
        assert_eq!(m.dial_block(), Err(DialBlock::Emergency));
        assert_eq!(m.call(), Err(DialBlock::Emergency));
        assert!(m.pending.is_empty(), "emergency numbers never dial");
        m.clear();
        for ch in "12".chars() {
            m.key(ch);
        }
        assert_eq!(m.dial_block(), Err(DialBlock::Invalid));
        m.clear();
        for ch in "5550100042".chars() {
            m.key(ch);
        }
        assert_eq!(m.dial_string(), "+15550100042");
        assert_eq!(m.dial_block(), Ok(()));
        assert_eq!(m.call(), Ok(()));
        assert_eq!(m.pending, vec![Action::Dial("+15550100042".to_owned())]);
        // `**` at the start types `+`.
        m.clear();
        m.key('*');
        m.key('*');
        assert_eq!(m.typed, "+");

        let mut s = preview_state(None);
        s.modem = ModemState::Absent;
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.dial_block(), Err(DialBlock::NoModem));
        s.modem = ModemState::Ready;
        s.radio = RadioPower::Off;
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.dial_block(), Err(DialBlock::RadioOff));
        s.radio = RadioPower::On;
        s.sim = SimState::Absent;
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.dial_block(), Err(DialBlock::NoSim));
        s.sim = SimState::Locked {
            lock: ferrix_telephony_api::SimLock::Pin,
            attempts_left: 3,
        };
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.dial_block(), Err(DialBlock::SimLocked));
        s.sim = SimState::Ready;
        s.registration = Registration::Searching;
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.dial_block(), Err(DialBlock::NoService));
        s.registration = Registration::Registered { roaming: true };
        m.apply(Update::Snapshot(StateView::Full(s)));
        m.apply(Update::Disconnected);
        assert_eq!(m.dial_block(), Err(DialBlock::Unavailable));
        assert_eq!(m.link, Link::Unavailable);
    }

    #[test]
    fn outgoing_call_flow_ends_with_every_reason() {
        let reasons = [
            CallEndReason::LocalHangup,
            CallEndReason::RemoteHangup,
            CallEndReason::Busy,
            CallEndReason::Rejected,
            CallEndReason::NoAnswer,
            CallEndReason::TimedOut,
            CallEndReason::NoService,
            CallEndReason::ModemLost,
            CallEndReason::Failed {
                reason: "backend".to_owned(),
            },
        ];
        for reason in reasons {
            let mut m = connected(None);
            m.apply(call_event(
                "c1",
                Direction::Outgoing,
                CallState::Dialing,
                None,
            ));
            assert_eq!(m.page, Page::InCall);
            assert_eq!(m.state_name(), "dialing");
            m.apply(call_event(
                "c1",
                Direction::Outgoing,
                CallState::Active,
                None,
            ));
            assert_eq!(m.state_name(), "active");
            m.toggle_hold();
            assert_eq!(m.pending.pop(), Some(Action::Hold(CallId("c1".into()))));
            m.apply(call_event("c1", Direction::Outgoing, CallState::Held, None));
            assert_eq!(m.page, Page::InCall);
            m.toggle_hold();
            assert_eq!(m.pending.pop(), Some(Action::Resume(CallId("c1".into()))));
            m.apply(call_event(
                "c1",
                Direction::Outgoing,
                CallState::Active,
                None,
            ));
            m.dtmf('5');
            assert_eq!(
                m.pending.pop(),
                Some(Action::Dtmf(CallId("c1".into()), '5'))
            );
            m.hangup();
            assert_eq!(m.pending.pop(), Some(Action::Hangup(CallId("c1".into()))));
            m.apply(call_event(
                "c1",
                Direction::Outgoing,
                CallState::Ended,
                Some(reason.clone()),
            ));
            assert_eq!(m.page, Page::Ended, "{reason:?}");
            assert_eq!(m.state_name(), reason.name());
            m.back();
            assert_eq!(m.page, Page::Dialer);
            assert!(m.focused_call.is_none());
            assert_eq!(
                m.dial_block(),
                Err(DialBlock::Empty),
                "dialer usable after {reason:?}"
            );
        }
    }

    #[test]
    fn incoming_call_answer_and_reject() {
        let mut m = connected(None);
        m.apply(call_event(
            "in1",
            Direction::Incoming,
            CallState::Ringing,
            None,
        ));
        assert_eq!(m.page, Page::Incoming);
        assert_eq!(m.state_name(), "ringing");
        m.answer();
        assert_eq!(m.pending.pop(), Some(Action::Answer(CallId("in1".into()))));
        m.apply(call_event(
            "in1",
            Direction::Incoming,
            CallState::Active,
            None,
        ));
        assert_eq!(m.page, Page::InCall);
        assert_eq!(m.dial_block(), Err(DialBlock::CallInProgress));
        m.apply(call_event(
            "in1",
            Direction::Incoming,
            CallState::Ended,
            Some(CallEndReason::RemoteHangup),
        ));
        assert_eq!(m.page, Page::Ended);

        let mut m = connected(None);
        m.apply(call_event(
            "in2",
            Direction::Incoming,
            CallState::Ringing,
            None,
        ));
        m.reject();
        assert_eq!(m.pending.pop(), Some(Action::Reject(CallId("in2".into()))));
        m.apply(call_event(
            "in2",
            Direction::Incoming,
            CallState::Ended,
            Some(CallEndReason::Rejected),
        ));
        assert_eq!((m.page, m.state_name().as_str()), (Page::Ended, "rejected"));
    }

    #[test]
    fn dial_intent_prefills_without_dialing_and_show_call_refocuses() {
        use ferrix_app_api::AppIntent;
        let mut m = connected(None);
        m.page = Page::Recent;
        m.key('9');
        assert!(m.apply_intent(&AppIntent::Dial {
            number: "+1 (555) 010-0042".to_owned()
        }));
        assert_eq!(m.typed, "+15550100042");
        assert_eq!(m.page, Page::Dialer);
        assert!(m.pending.is_empty(), "an intent never places a call");
        assert_eq!(m.dial_block(), Ok(()));
        // While a call rings the buffer fills but the call stays in front.
        m.apply(call_event(
            "in1",
            Direction::Incoming,
            CallState::Ringing,
            None,
        ));
        m.apply_intent(&AppIntent::Dial {
            number: "5550100".to_owned(),
        });
        assert_eq!(m.page, Page::Incoming);
        assert_eq!(m.typed, "5550100");
        m.page = Page::Dialer;
        assert!(m.apply_intent(&AppIntent::ShowCall));
        assert_eq!(m.page, Page::Incoming);
        assert!(m.pending.is_empty());
        assert!(!m.apply_intent(&AppIntent::Compose {
            number: "1".to_owned()
        }));
    }

    #[test]
    fn party_label_prefers_contacts_and_hides_numbers_when_redacted() {
        let m = connected(Some((Direction::Incoming, CallState::Ringing, None)));
        let view = m.focused().unwrap();
        assert_eq!(view.party_label(&[], "US"), "+15550100042");
        let contact = ferrix_contacts_api::Contact {
            id: ferrix_contacts_api::ContactId::from_bytes([1; 8]),
            display_name: "Test Contact 42".to_owned(),
            given: None,
            family: None,
            numbers: vec![ferrix_contacts_api::PhoneNumber::new(
                ferrix_contacts_api::NumberLabel::Mobile,
                "+1 555 010 0042",
                "US",
            )],
            created_ms: 0,
            updated_ms: 0,
        };
        assert_eq!(view.party_label(&[contact], "US"), "Test Contact 42");
        // A redacted snapshot never exposes the full number.
        let mut r = Model::new();
        r.apply(Update::Snapshot(StateView::Redacted(
            preview_state(Some((Direction::Incoming, CallState::Ringing, None))).redacted(),
        )));
        let view = r.focused().unwrap();
        assert_eq!(view.number, None);
        assert_eq!(view.party_label(&[], "US"), "+***0042");
    }

    #[test]
    fn recent_is_bounded_and_newest_first() {
        let mut m = connected(Some((Direction::Outgoing, CallState::Active, None)));
        let recent = m.recent();
        assert_eq!(recent[0].call.id.as_str(), "call-live");
        assert!(
            recent
                .windows(2)
                .all(|w| w[0].call.started_at_ms >= w[1].call.started_at_ms)
        );
        for i in 0..30 {
            m.apply(call_event(
                &format!("x{i}"),
                Direction::Outgoing,
                CallState::Ended,
                Some(CallEndReason::Busy),
            ));
        }
        assert_eq!(m.recent().len(), RECENT_LIMIT);
        // Calls that ended before the app saw them have no known duration.
        assert_eq!(m.duration_secs(&CallId("x0".into())), None);
        m.redial(&CallId("call-0".into()));
        assert_eq!(m.typed, "+15550100001");
        assert_eq!(m.page, Page::Dialer);
    }

    #[test]
    fn reconnect_replaces_state_and_labels_the_link() {
        let mut m = connected(Some((Direction::Outgoing, CallState::Active, None)));
        assert_eq!(m.link, Link::Connected);
        assert_eq!(m.page, Page::InCall);
        m.apply(Update::Disconnected);
        assert_eq!(m.link_label(), "telephony \u{b7} unavailable");
        assert_eq!(m.page, Page::InCall, "UI stays usable while unavailable");
        m.apply(Update::Snapshot(StateView::Full(preview_state(None))));
        assert_eq!(m.link, Link::Reconnected);
        assert_eq!(m.page, Page::Dialer, "the snapshot had no live call");
        assert!(m.live_call().is_none());
        m.apply(Update::Event(Event::Full(
            TelephonyEvent::ServiceRestarted {
                state: preview_state(Some((Direction::Incoming, CallState::Ringing, None))),
            },
        )));
        assert_eq!((m.link, m.page), (Link::Reconnected, Page::Incoming));
        assert_eq!(Page::parse("in-call"), Some(Page::InCall));
        assert_eq!(Page::parse("nope"), None);
    }

    #[test]
    fn call_timer_counts_from_active_on_the_local_clock_and_freezes_at_end() {
        let mut m = connected(None);
        let id = CallId("c1".into());
        m.set_clock(10_000);
        m.apply(call_event(
            "c1",
            Direction::Outgoing,
            CallState::Dialing,
            None,
        ));
        assert_eq!(m.duration_secs(&id), None, "dialing is not connected");
        assert!(!m.timer_running());
        m.set_clock(14_000);
        m.apply(call_event(
            "c1",
            Direction::Outgoing,
            CallState::Active,
            None,
        ));
        assert!(m.timer_running());
        assert_eq!(m.duration_secs(&id), Some(0));
        // Every event carries the same daemon start time; only the local
        // clock moves the timer.
        m.set_clock(56_900);
        assert_eq!(m.duration_secs(&id), Some(42));
        m.apply(call_event("c1", Direction::Outgoing, CallState::Held, None));
        m.set_clock(74_000);
        assert_eq!(m.duration_secs(&id), Some(60), "hold keeps counting");
        m.set_clock(5_000);
        assert_eq!(m.duration_secs(&id), Some(60), "clock never goes back");
        m.apply(call_event(
            "c1",
            Direction::Outgoing,
            CallState::Ended,
            Some(CallEndReason::RemoteHangup),
        ));
        assert_eq!(m.page, Page::Ended);
        assert!(!m.timer_running());
        m.set_clock(500_000);
        assert_eq!(m.duration_secs(&id), Some(60), "final duration is frozen");
        // A reconnect snapshot keeps the timer of a call it still lists.
        let mut live = connected(Some((Direction::Outgoing, CallState::Active, None)));
        live.set_clock(3_000);
        live.apply(Update::Snapshot(StateView::Full(preview_state(Some((
            Direction::Outgoing,
            CallState::Active,
            None,
        ))))));
        assert_eq!(live.duration_secs(&CallId("call-live".into())), Some(3));
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(62), "1:02");
        assert_eq!(format_duration(3_725), "1:02:05");
    }
}
