//! Messages view model: conversations grouped by party, per-message state,
//! multipart rendering, compose/retry, and daemon reconciliation. Pure; the
//! session owner feeds [`Model::apply`] and drains [`Model::pending`].
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::BTreeMap;

use ferrix_contacts_api::Contact;
use ferrix_telephony_api::{
    Direction, E164, MessageId, ModemState, RadioPower, RedactedSms, RedactedTelephonyEvent,
    RedactedTelephonyState, Registration, SimState, Sms, SmsFailure, SmsState, TelephonyEvent,
    TelephonyFeature, TelephonyState, is_emergency_number,
};
use ferrix_telephony_protocol::{Event, StateView, Update};
use ferrix_ui::osk::OskState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Conversations,
    Thread,
    Compose,
}

impl Page {
    pub fn name(self) -> &'static str {
        match self {
            Page::Conversations => "conversations",
            Page::Thread => "thread",
            Page::Compose => "compose",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "conversations" => Page::Conversations,
            "thread" => Page::Thread,
            "compose" => Page::Compose,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Connecting,
    Connected,
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

/// Which compose field has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    To,
    Body,
}

/// Why Send is disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendBlock {
    Unavailable,
    NoModem,
    RadioOff,
    NoSim,
    NoService,
    Unsupported,
    EmptyTo,
    Emergency,
    InvalidTo,
    EmptyBody,
}

impl SendBlock {
    pub fn reason(self) -> &'static str {
        match self {
            SendBlock::Unavailable => "telephony \u{b7} unavailable",
            SendBlock::NoModem => "modem \u{b7} absent",
            SendBlock::RadioOff => "radio \u{b7} off",
            SendBlock::NoSim => "sim \u{b7} absent",
            SendBlock::NoService => "service \u{b7} none",
            SendBlock::Unsupported => "sms \u{b7} unsupported",
            SendBlock::EmptyTo => "recipient \u{b7} empty",
            SendBlock::Emergency => "Emergency messaging not supported",
            SendBlock::InvalidTo => "recipient \u{b7} invalid",
            SendBlock::EmptyBody => "message \u{b7} empty",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SendBlock::Unavailable => "unavailable",
            SendBlock::NoModem => "no-modem",
            SendBlock::RadioOff => "radio-off",
            SendBlock::NoSim => "no-sim",
            SendBlock::NoService => "no-service",
            SendBlock::Unsupported => "unsupported",
            SendBlock::EmptyTo => "empty-to",
            SendBlock::Emergency => "emergency",
            SendBlock::InvalidTo => "invalid-to",
            SendBlock::EmptyBody => "empty-body",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SendSms { to: String, body: String },
    MarkRead(MessageId),
    Back,
}

/// One message as the UI shows it; `body` and `number` are present only
/// when the grant delivered the full form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MsgView {
    pub id: MessageId,
    pub direction: Direction,
    /// Grouping key: the E.164 number, or the redacted form.
    pub party_key: String,
    pub party_display: Option<String>,
    pub number: Option<E164>,
    pub body: Option<String>,
    pub parts: usize,
    pub state: SmsState,
    pub timestamp_ms: u64,
    pub failure: Option<SmsFailure>,
}

impl MsgView {
    fn from_full(sms: &Sms) -> Self {
        Self {
            id: sms.id.clone(),
            direction: sms.direction,
            party_key: sms.party.number.as_str().to_owned(),
            party_display: sms.party.display.clone(),
            number: Some(sms.party.number.clone()),
            body: Some(sms.body.clone()),
            parts: sms.parts.len().max(1),
            state: sms.state,
            timestamp_ms: sms.timestamp_ms,
            failure: sms.failure.clone(),
        }
    }

    fn from_redacted(sms: &RedactedSms) -> Self {
        Self {
            id: sms.id.clone(),
            direction: sms.direction,
            party_key: sms.party.party_redacted.clone(),
            party_display: sms.party.display.clone(),
            number: None,
            body: None,
            parts: sms.parts.max(1),
            state: sms.state,
            timestamp_ms: sms.timestamp_ms,
            failure: sms.failure.clone(),
        }
    }

    /// `(label, tone name)` of the state chip; failures carry the typed reason.
    pub fn chip(&self) -> (String, ChipTone) {
        match (self.state, &self.failure) {
            (SmsState::Failed, Some(failure)) => {
                (format!("failed \u{b7} {}", failure.name()), ChipTone::Err)
            }
            (SmsState::Failed, None) => ("failed".to_owned(), ChipTone::Err),
            (SmsState::Sending, _) => ("sending".to_owned(), ChipTone::Steel),
            (SmsState::Sent, _) => ("sent".to_owned(), ChipTone::Neutral),
            (SmsState::Delivered, _) => ("delivered".to_owned(), ChipTone::Ok),
            (SmsState::Received, _) => ("received".to_owned(), ChipTone::Steel),
            (SmsState::Read, _) => ("read".to_owned(), ChipTone::Neutral),
            (SmsState::Deleted, _) => ("deleted".to_owned(), ChipTone::Neutral),
        }
    }

    pub fn body_text(&self) -> String {
        match &self.body {
            Some(body) => body.clone(),
            None => "body redacted \u{b7} no telephony-sms grant".to_owned(),
        }
    }

    pub fn can_retry(&self) -> bool {
        self.state == SmsState::Failed && self.body.is_some() && self.number.is_some()
    }
}

/// Tone of a chip, kept free of the drawing crate so the model stays pure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipTone {
    Neutral,
    Steel,
    Ok,
    Err,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub key: String,
    pub number: Option<E164>,
    pub title: String,
    pub preview: String,
    pub latest_ms: u64,
    pub unread: usize,
    pub count: usize,
    pub last_state: (String, ChipTone),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub page: Page,
    pub link: Link,
    pub state: Option<RedactedTelephonyState>,
    pub messages: BTreeMap<MessageId, MsgView>,
    pub contacts: Vec<Contact>,
    pub region: String,
    pub thread: Option<String>,
    pub draft: String,
    pub compose_to: String,
    pub focus: Field,
    pub osk: OskState,
    pub pending: Vec<Action>,
    pub last_error: Option<String>,
    lost_once: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            page: Page::Conversations,
            link: Link::Connecting,
            state: None,
            messages: BTreeMap::new(),
            contacts: Vec::new(),
            region: ferrix_contacts_api::DEFAULT_REGION.to_owned(),
            thread: None,
            draft: String::new(),
            compose_to: String::new(),
            focus: Field::To,
            osk: OskState::default(),
            pending: Vec::new(),
            last_error: None,
            lost_once: false,
        }
    }

    pub fn link_label(&self) -> String {
        format!("telephony \u{b7} {}", self.link.name())
    }

    pub fn state_name(&self) -> String {
        match self.page {
            Page::Conversations => format!("{}-conversations", self.conversations().len()),
            Page::Thread => format!("{}-messages", self.thread_messages().len()),
            Page::Compose => match self.send_block(&self.compose_to) {
                Ok(()) => "ready".to_owned(),
                Err(block) => block.name().to_owned(),
            },
        }
    }

    // -- updates -------------------------------------------------------------

    fn replace_state(&mut self, view: &StateView) {
        self.state = Some(view.redacted());
        self.messages.clear();
        match view {
            StateView::Full(full) => {
                for (id, sms) in &full.sms {
                    self.messages.insert(id.clone(), MsgView::from_full(sms));
                }
            }
            StateView::Redacted(redacted) => {
                for (id, sms) in &redacted.sms {
                    self.messages
                        .insert(id.clone(), MsgView::from_redacted(sms));
                }
            }
        }
        if self.page == Page::Thread
            && self
                .thread
                .as_ref()
                .is_some_and(|key| !self.messages.values().any(|m| m.party_key == *key))
        {
            self.page = Page::Conversations;
            self.thread = None;
        }
    }

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
            }
            Update::Disconnected => {
                self.link = Link::Unavailable;
                self.lost_once = true;
            }
            Update::Event(Event::Full(TelephonyEvent::ServiceRestarted { state })) => {
                self.link = Link::Reconnected;
                self.replace_state(&StateView::Full(state));
            }
            Update::Event(Event::Redacted(RedactedTelephonyEvent::ServiceRestarted { state })) => {
                self.link = Link::Reconnected;
                self.replace_state(&StateView::Redacted(state));
            }
            Update::Event(Event::Full(TelephonyEvent::SmsChanged { sms })) => {
                if let Some(state) = self.state.as_mut() {
                    TelephonyEvent::SmsChanged { sms: sms.clone() }
                        .redacted()
                        .apply_to(state);
                }
                if sms.state == SmsState::Deleted {
                    self.messages.remove(&sms.id);
                } else {
                    self.messages
                        .insert(sms.id.clone(), MsgView::from_full(&sms));
                }
            }
            Update::Event(Event::Redacted(RedactedTelephonyEvent::SmsChanged { sms })) => {
                if let Some(state) = self.state.as_mut() {
                    RedactedTelephonyEvent::SmsChanged { sms: sms.clone() }.apply_to(state);
                }
                if sms.state == SmsState::Deleted {
                    self.messages.remove(&sms.id);
                } else {
                    self.messages
                        .insert(sms.id.clone(), MsgView::from_redacted(&sms));
                }
            }
            Update::Event(event) => {
                if let Some(state) = self.state.as_mut() {
                    event.apply_redacted(state);
                }
            }
        }
        true
    }

    pub fn set_contacts(&mut self, contacts: Vec<Contact>) {
        self.contacts = contacts;
    }

    // -- derived -------------------------------------------------------------

    /// The name for a party: contact, then daemon display, then the key.
    pub fn party_title(&self, key: &str, number: Option<&E164>, display: Option<&str>) -> String {
        if let Some(number) = number
            && let Some(contact) =
                ferrix_contacts_api::lookup(&self.contacts, number.as_str(), &self.region)
        {
            return contact.display_name.clone();
        }
        display.map(str::to_owned).unwrap_or_else(|| key.to_owned())
    }

    /// Conversations newest first.
    pub fn conversations(&self) -> Vec<Conversation> {
        let mut groups: BTreeMap<&str, Vec<&MsgView>> = BTreeMap::new();
        for m in self.messages.values() {
            groups.entry(m.party_key.as_str()).or_default().push(m);
        }
        let mut out: Vec<Conversation> = groups
            .into_iter()
            .map(|(key, mut msgs)| {
                msgs.sort_by_key(|m| (m.timestamp_ms, m.id.as_str().to_owned()));
                let latest = msgs[msgs.len() - 1];
                Conversation {
                    key: key.to_owned(),
                    number: latest.number.clone(),
                    title: self.party_title(
                        key,
                        latest.number.as_ref(),
                        latest.party_display.as_deref(),
                    ),
                    preview: latest.body_text(),
                    latest_ms: latest.timestamp_ms,
                    unread: msgs
                        .iter()
                        .filter(|m| m.state == SmsState::Received)
                        .count(),
                    count: msgs.len(),
                    last_state: latest.chip(),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.latest_ms
                .cmp(&a.latest_ms)
                .then_with(|| a.key.cmp(&b.key))
        });
        out
    }

    /// Messages of the open thread, oldest first.
    pub fn thread_messages(&self) -> Vec<&MsgView> {
        let Some(key) = &self.thread else {
            return Vec::new();
        };
        let mut msgs: Vec<&MsgView> = self
            .messages
            .values()
            .filter(|m| m.party_key == *key)
            .collect();
        msgs.sort_by_key(|m| (m.timestamp_ms, m.id.as_str().to_owned()));
        msgs
    }

    pub fn thread_title(&self) -> String {
        let Some(key) = &self.thread else {
            return "Thread".to_owned();
        };
        let latest = self.thread_messages().last().cloned();
        match latest {
            Some(m) => self.party_title(key, m.number.as_ref(), m.party_display.as_deref()),
            None => key.clone(),
        }
    }

    pub fn thread_number(&self) -> Option<String> {
        self.thread_messages()
            .iter()
            .find_map(|m| m.number.as_ref().map(|n| n.as_str().to_owned()))
    }

    fn radio_block(&self) -> Result<(), SendBlock> {
        let Some(state) = &self.state else {
            return Err(SendBlock::Unavailable);
        };
        if self.link == Link::Unavailable {
            return Err(SendBlock::Unavailable);
        }
        if state.modem == ModemState::Absent {
            return Err(SendBlock::NoModem);
        }
        if state.radio == RadioPower::Off {
            return Err(SendBlock::RadioOff);
        }
        if state.sim == SimState::Absent {
            return Err(SendBlock::NoSim);
        }
        if !matches!(state.registration, Registration::Registered { .. }) {
            return Err(SendBlock::NoService);
        }
        if !state.capabilities.contains(&TelephonyFeature::Sms) {
            return Err(SendBlock::Unsupported);
        }
        Ok(())
    }

    /// Normalized destination for `to`.
    pub fn destination(&self, to: &str) -> Result<String, SendBlock> {
        let digits: String = to.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err(SendBlock::EmptyTo);
        }
        if is_emergency_number(&digits) {
            return Err(SendBlock::Emergency);
        }
        let dest = if to.trim().starts_with('+') {
            to.trim().to_owned()
        } else {
            ferrix_contacts_api::normalize(to, &self.region).ok_or(SendBlock::InvalidTo)?
        };
        E164::new(&dest).map_err(|_| SendBlock::InvalidTo)?;
        Ok(dest)
    }

    /// Whether `draft` could be sent to `to` now.
    pub fn send_block(&self, to: &str) -> Result<(), SendBlock> {
        self.radio_block()?;
        self.destination(to)?;
        if self.draft.trim().is_empty() {
            return Err(SendBlock::EmptyBody);
        }
        Ok(())
    }

    // -- intents -------------------------------------------------------------

    /// Opens a conversation and marks its received messages read.
    pub fn open_thread(&mut self, key: &str) {
        self.thread = Some(key.to_owned());
        self.page = Page::Thread;
        self.focus = Field::Body;
        self.last_error = None;
        let unread: Vec<MessageId> = self
            .thread_messages()
            .iter()
            .filter(|m| m.state == SmsState::Received)
            .map(|m| m.id.clone())
            .collect();
        for id in unread {
            self.pending.push(Action::MarkRead(id));
        }
    }

    pub fn open_compose(&mut self) {
        self.page = Page::Compose;
        self.compose_to.clear();
        self.draft.clear();
        self.focus = Field::To;
        self.last_error = None;
    }

    /// Applies a `compose` intent from `ferrix-appd`: a new message to
    /// `number` with the body focused. Never sends; other kinds are
    /// ignored. Returns whether anything visible changed.
    pub fn apply_intent(&mut self, intent: &ferrix_app_api::AppIntent) -> bool {
        let ferrix_app_api::AppIntent::Compose { number } = intent else {
            return false;
        };
        self.open_compose();
        self.compose_to = number.clone();
        self.focus = Field::Body;
        true
    }

    /// Sends the draft in the current thread or compose form.
    pub fn send(&mut self) -> Result<(), SendBlock> {
        let to = match self.page {
            Page::Thread => self.thread_number().unwrap_or_default(),
            _ => self.compose_to.clone(),
        };
        self.send_block(&to)?;
        let dest = self.destination(&to)?;
        self.pending.push(Action::SendSms {
            to: dest.clone(),
            body: self.draft.trim().to_owned(),
        });
        self.draft.clear();
        self.last_error = None;
        if self.page == Page::Compose {
            self.thread = Some(dest);
            self.page = Page::Thread;
            self.focus = Field::Body;
        }
        Ok(())
    }

    /// Resends a failed message with the same destination and body.
    pub fn retry(&mut self, id: &MessageId) -> bool {
        let Some(m) = self.messages.get(id) else {
            return false;
        };
        if !m.can_retry() {
            return false;
        }
        if let Err(block) = self.radio_block() {
            self.last_error = Some(block.reason().to_owned());
            return false;
        }
        self.pending.push(Action::SendSms {
            to: m
                .number
                .as_ref()
                .map(|n| n.as_str().to_owned())
                .unwrap_or_default(),
            body: m.body.clone().unwrap_or_default(),
        });
        true
    }

    /// The text buffer the keyboard edits.
    pub fn focused_text(&mut self) -> &mut String {
        match (self.page, self.focus) {
            (Page::Compose, Field::To) => &mut self.compose_to,
            _ => &mut self.draft,
        }
    }

    pub fn back(&mut self) {
        match self.page {
            Page::Conversations => self.pending.push(Action::Back),
            Page::Thread => {
                self.page = Page::Conversations;
                self.thread = None;
                self.draft.clear();
            }
            Page::Compose => {
                self.page = Page::Conversations;
                self.draft.clear();
            }
        }
    }
}

/// Canned snapshot for the host preview: two conversations, every state,
/// one multipart message, one typed failure. Synthetic numbers only.
pub fn preview_state() -> TelephonyState {
    use ferrix_telephony_api::{AccessTechnology, PartRef, PartyRef, Signal};
    let mut state = TelephonyState::new();
    state.modem = ModemState::Ready;
    state.sim = SimState::Ready;
    state.registration = Registration::Registered { roaming: false };
    state.access = AccessTechnology::Lte;
    state.signal = Signal::bars(3);
    state.radio = RadioPower::On;
    state.capabilities = [
        TelephonyFeature::Sms,
        TelephonyFeature::MultipartSms,
        TelephonyFeature::DeliveryReports,
    ]
    .into_iter()
    .collect();
    let a = E164::new("+15550100042").unwrap();
    let b = E164::new("+15550100007").unwrap();
    let base = 1_700_000_000_000_u64;
    let msgs = [
        (
            "m1",
            Direction::Incoming,
            &a,
            "Ferrix mock \u{b7} incoming test message one.",
            SmsState::Read,
            None,
            1,
        ),
        (
            "m2",
            Direction::Outgoing,
            &a,
            "Reply from the emulator.",
            SmsState::Delivered,
            None,
            1,
        ),
        (
            "m3",
            Direction::Incoming,
            &a,
            "A long multipart message assembled by the daemon from three concatenated parts so the thread shows one bubble with a parts chip.",
            SmsState::Received,
            None,
            3,
        ),
        (
            "m4",
            Direction::Outgoing,
            &a,
            "Did this go out?",
            SmsState::Failed,
            Some(SmsFailure::NoService),
            1,
        ),
        (
            "m5",
            Direction::Outgoing,
            &b,
            "Sending now",
            SmsState::Sending,
            None,
            1,
        ),
        (
            "m6",
            Direction::Outgoing,
            &b,
            "Sent earlier",
            SmsState::Sent,
            None,
            1,
        ),
    ];
    for (i, (id, direction, party, body, sms_state, failure, parts)) in msgs.into_iter().enumerate()
    {
        let parts: Vec<PartRef> = (0..parts)
            .map(|index| PartRef {
                ref_id: 7,
                index: index as u8,
                count: parts as u8,
            })
            .collect();
        state.sms.insert(
            MessageId(id.to_owned()),
            Sms {
                id: MessageId(id.to_owned()),
                direction,
                party: PartyRef::new(party.clone()),
                body: body.to_owned(),
                parts,
                state: sms_state,
                timestamp_ms: base + i as u64 * 60_000,
                failure,
            },
        );
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_telephony_api::PartyRef;

    fn connected() -> Model {
        let mut m = Model::new();
        m.apply(Update::Snapshot(StateView::Full(preview_state())));
        m
    }

    #[test]
    fn conversations_group_by_party_newest_first_with_unread_counts() {
        let m = connected();
        let convs = m.conversations();
        assert_eq!(convs.len(), 2);
        assert_eq!(convs[0].key, "+15550100007", "newest activity first");
        assert_eq!(convs[1].key, "+15550100042");
        assert_eq!(convs[1].unread, 1);
        assert_eq!(convs[1].count, 4);
        assert_eq!(convs[1].last_state.0, "failed \u{b7} no-service");
        assert_eq!(convs[0].last_state, ("sent".to_owned(), ChipTone::Neutral));
        assert_eq!(m.state_name(), "2-conversations");
    }

    #[test]
    fn message_chips_cover_every_state_and_multipart_is_one_bubble() {
        let m = connected();
        let chips: Vec<(String, ChipTone)> = m.messages.values().map(MsgView::chip).collect();
        assert!(chips.contains(&("read".to_owned(), ChipTone::Neutral)));
        assert!(chips.contains(&("delivered".to_owned(), ChipTone::Ok)));
        assert!(chips.contains(&("received".to_owned(), ChipTone::Steel)));
        assert!(chips.contains(&("failed \u{b7} no-service".to_owned(), ChipTone::Err)));
        assert!(chips.contains(&("sending".to_owned(), ChipTone::Steel)));
        assert!(chips.contains(&("sent".to_owned(), ChipTone::Neutral)));
        let multipart = &m.messages[&MessageId("m3".into())];
        assert_eq!(multipart.parts, 3);
        assert_eq!(m.thread_messages().len(), 0, "no thread open");
        let mut m = m;
        m.open_thread("+15550100042");
        let thread = m.thread_messages();
        assert_eq!(thread.len(), 4, "three parts render as one message");
        assert_eq!(thread[0].id.as_str(), "m1");
        assert_eq!(
            m.pending,
            vec![Action::MarkRead(MessageId("m3".into()))],
            "received marked read on open"
        );
        assert_eq!(m.thread_title(), "+15550100042");
        assert_eq!(m.state_name(), "4-messages");
    }

    #[test]
    fn compose_intent_prefills_the_recipient_and_never_sends() {
        use ferrix_app_api::AppIntent;
        let mut m = connected();
        m.open_thread("+15550100042");
        m.draft = "half written".to_owned();
        m.pending.clear();
        assert!(m.apply_intent(&AppIntent::Compose {
            number: "+15550100077".to_owned()
        }));
        assert_eq!(m.page, Page::Compose);
        assert_eq!(m.compose_to, "+15550100077");
        assert_eq!(m.focus, Field::Body, "the body is ready to type");
        assert!(m.draft.is_empty());
        assert!(m.pending.is_empty(), "an intent never sends");
        assert_eq!(
            m.send_block(&m.compose_to.clone()),
            Err(SendBlock::EmptyBody)
        );
        assert!(!m.apply_intent(&AppIntent::ShowCall));
    }

    #[test]
    fn send_blocks_retry_and_compose_flow() {
        let mut m = connected();
        m.open_compose();
        assert_eq!(m.send_block(""), Err(SendBlock::EmptyTo));
        m.compose_to = "911".to_owned();
        assert_eq!(m.send_block(&m.compose_to), Err(SendBlock::Emergency));
        m.compose_to = "12".to_owned();
        assert_eq!(m.send_block(&m.compose_to), Err(SendBlock::InvalidTo));
        m.compose_to = "(555) 010-0099".to_owned();
        assert_eq!(m.send_block(&m.compose_to), Err(SendBlock::EmptyBody));
        m.draft = "hello".to_owned();
        assert_eq!(m.send(), Ok(()));
        assert_eq!(
            m.pending,
            vec![Action::SendSms {
                to: "+15550100099".to_owned(),
                body: "hello".to_owned()
            }]
        );
        assert_eq!(
            (m.page, m.thread.as_deref()),
            (Page::Thread, Some("+15550100099"))
        );
        assert!(m.draft.is_empty());
        m.pending.clear();

        // Retry resends the failed message verbatim; a delivered one cannot retry.
        assert!(m.retry(&MessageId("m4".into())));
        assert_eq!(
            m.pending,
            vec![Action::SendSms {
                to: "+15550100042".to_owned(),
                body: "Did this go out?".to_owned()
            }]
        );
        assert!(!m.retry(&MessageId("m2".into())));

        // Radio blocks in order.
        let mut s = preview_state();
        s.registration = Registration::Searching;
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.radio_block(), Err(SendBlock::NoService));
        s.registration = Registration::Registered { roaming: false };
        s.capabilities.clear();
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.radio_block(), Err(SendBlock::Unsupported));
        s.sim = SimState::Absent;
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.radio_block(), Err(SendBlock::NoSim));
        s.radio = RadioPower::Off;
        m.apply(Update::Snapshot(StateView::Full(s.clone())));
        assert_eq!(m.radio_block(), Err(SendBlock::RadioOff));
        s.modem = ModemState::Absent;
        m.apply(Update::Snapshot(StateView::Full(s)));
        assert_eq!(m.radio_block(), Err(SendBlock::NoModem));
        m.apply(Update::Disconnected);
        assert_eq!(m.radio_block(), Err(SendBlock::Unavailable));
        m.pending.clear();
        assert!(
            !m.retry(&MessageId("m4".into())),
            "retry refused while unavailable"
        );
        assert_eq!(
            m.last_error.as_deref(),
            Some("telephony \u{b7} unavailable")
        );
    }

    #[test]
    fn events_upsert_and_reconnect_replaces_state() {
        let mut m = connected();
        let sms = Sms {
            id: MessageId("m7".into()),
            direction: Direction::Incoming,
            party: PartyRef::new(E164::new("+15550100042").unwrap()),
            body: "new".to_owned(),
            parts: Vec::new(),
            state: SmsState::Received,
            timestamp_ms: 1_700_000_900_000,
            failure: None,
        };
        m.apply(Update::Event(Event::Full(TelephonyEvent::SmsChanged {
            sms: sms.clone(),
        })));
        assert_eq!(m.messages.len(), 7);
        assert_eq!(
            m.conversations()[0].key,
            "+15550100042",
            "new activity reorders"
        );
        let mut deleted = sms.clone();
        deleted.state = SmsState::Deleted;
        m.apply(Update::Event(Event::Full(TelephonyEvent::SmsChanged {
            sms: deleted,
        })));
        assert_eq!(m.messages.len(), 6);
        m.open_thread("+15550100007");
        m.apply(Update::Disconnected);
        assert_eq!(m.link, Link::Unavailable);
        assert_eq!(m.page, Page::Thread, "UI stays usable");
        let mut s = preview_state();
        s.sms
            .retain(|_, v| v.party.number.as_str() == "+15550100042");
        m.apply(Update::Snapshot(StateView::Full(s)));
        assert_eq!(m.link, Link::Reconnected);
        assert_eq!(m.page, Page::Conversations, "vanished thread closes");
        // A redacted snapshot keeps conversations without bodies.
        let mut r = Model::new();
        r.apply(Update::Snapshot(StateView::Redacted(
            preview_state().redacted(),
        )));
        let convs = r.conversations();
        assert_eq!(convs.len(), 2);
        assert!(convs[0].key.starts_with("+***"));
        assert!(convs[0].preview.contains("redacted"));
        assert!(!r.retry(&MessageId("m4".into())), "no body to resend");
        assert_eq!(Page::parse("thread"), Some(Page::Thread));
    }
}
