//! Stable contacts contract (ADR-003, schema 1): stable ids, names,
//! normalized numbers, search/lookup rules, typed errors, and events.
//!
//! Pure types and pure functions; no I/O. Applications, the Phone and
//! Messages apps, and `ferrix-contactsd` all depend on this crate. Nothing
//! here knows about SIM phonebooks, modems, QEMU, or storage.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub mod normalize;

pub use normalize::{DEFAULT_REGION, DEFAULT_REGION_ENV, digits, normalize};

/// Schema version of every serialized type in this crate and of the
/// `ferrix-contactsd` wire protocol and store built on it.
pub const SCHEMA_VERSION: u32 = 1;

/// Upper bound on a display name, in characters.
pub const MAX_NAME_CHARS: usize = 256;
/// Upper bound on the numbers of one contact.
pub const MAX_NUMBERS: usize = 32;
/// Trailing digits compared when no E.164 match exists.
pub const SUFFIX_DIGITS: usize = 7;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Opaque, stable contact id: `c_` followed by sixteen lowercase hex digits
/// drawn from the operating system CSPRNG at creation. Never reused.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContactId(String);

impl ContactId {
    pub const PREFIX: &'static str = "c_";
    pub const HEX_LEN: usize = 16;

    /// Validates the textual form.
    pub fn parse(text: impl Into<String>) -> Result<Self, ContactsError> {
        let text = text.into();
        let hex = text.strip_prefix(Self::PREFIX).unwrap_or("");
        let ok = hex.len() == Self::HEX_LEN
            && hex
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
        if ok {
            Ok(Self(text))
        } else {
            Err(ContactsError::InvalidInput {
                reason: format!("contact id {text:?} is not c_ plus 16 lowercase hex digits"),
            })
        }
    }

    /// A fresh id from the OS CSPRNG (`getrandom`).
    pub fn random() -> Result<Self, ContactsError> {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).map_err(|e| ContactsError::Store {
            reason: format!("csprng: {e}"),
        })?;
        Ok(Self::from_bytes(bytes))
    }

    /// Deterministic construction from eight bytes; for fixtures and tests.
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        let mut text = String::with_capacity(Self::PREFIX.len() + Self::HEX_LEN);
        text.push_str(Self::PREFIX);
        for b in bytes {
            text.push_str(&format!("{b:02x}"));
        }
        Self(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ContactId {
    type Err = ContactsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for ContactId {
    type Error = ContactsError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<ContactId> for String {
    fn from(id: ContactId) -> Self {
        id.0
    }
}

// ---------------------------------------------------------------------------
// Numbers and contacts
// ---------------------------------------------------------------------------

/// Label of one number.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum NumberLabel {
    Mobile,
    Home,
    Work,
    #[default]
    Other,
}

impl NumberLabel {
    pub const ALL: [NumberLabel; 4] = [
        NumberLabel::Mobile,
        NumberLabel::Home,
        NumberLabel::Work,
        NumberLabel::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            NumberLabel::Mobile => "mobile",
            NumberLabel::Home => "home",
            NumberLabel::Work => "work",
            NumberLabel::Other => "other",
        }
    }

    /// The next label in the cycle; used by an edit form's label chip.
    pub fn next(self) -> Self {
        match self {
            NumberLabel::Mobile => NumberLabel::Home,
            NumberLabel::Home => NumberLabel::Work,
            NumberLabel::Work => NumberLabel::Other,
            NumberLabel::Other => NumberLabel::Mobile,
        }
    }
}

impl fmt::Display for NumberLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One stored number: the text as entered plus its E.164 form when the
/// entry could be normalized. Matching falls back to digit suffixes when
/// `e164` is `None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneNumber {
    #[serde(default)]
    pub label: NumberLabel,
    pub raw: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e164: Option<String>,
}

impl PhoneNumber {
    /// Normalizes `raw` for `default_region` (an ISO 3166-1 alpha-2 code).
    pub fn new(label: NumberLabel, raw: impl Into<String>, default_region: &str) -> Self {
        let raw = raw.into();
        let e164 = normalize(&raw, default_region);
        Self { label, raw, e164 }
    }

    /// The text to show: E.164 when known, else the raw entry.
    pub fn display(&self) -> &str {
        self.e164.as_deref().unwrap_or(&self.raw)
    }
}

/// A stored contact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub id: ContactId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default)]
    pub numbers: Vec<PhoneNumber>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

/// One number as submitted by a client; the service normalizes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberInput {
    #[serde(default)]
    pub label: NumberLabel,
    pub raw: String,
}

/// The create/update payload: everything a client may set. Ids and
/// timestamps are owned by the service.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactInput {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default)]
    pub numbers: Vec<NumberInput>,
}

impl ContactInput {
    /// Trims and validates; returns the typed reason otherwise.
    pub fn validate(&self) -> Result<(), ContactsError> {
        let invalid = |reason: &str| ContactsError::InvalidInput {
            reason: reason.to_owned(),
        };
        let name = self.display_name.trim();
        if name.is_empty() {
            return Err(invalid("display_name is empty"));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(invalid("display_name is longer than 256 characters"));
        }
        if self.numbers.len() > MAX_NUMBERS {
            return Err(invalid("more than 32 numbers"));
        }
        for number in &self.numbers {
            if number.raw.trim().is_empty() {
                return Err(invalid("a number is empty"));
            }
            if number.raw.chars().count() > 64 {
                return Err(invalid("a number is longer than 64 characters"));
            }
        }
        Ok(())
    }

    /// Builds the stored contact for `id` at `now_ms`, normalizing numbers
    /// for `default_region`. `created_ms` is kept from `existing` on update.
    pub fn into_contact(
        self,
        id: ContactId,
        now_ms: u64,
        default_region: &str,
        existing: Option<&Contact>,
    ) -> Result<Contact, ContactsError> {
        self.validate()?;
        let numbers = self
            .numbers
            .into_iter()
            .map(|n| PhoneNumber::new(n.label, n.raw.trim(), default_region))
            .collect();
        Ok(Contact {
            id,
            display_name: self.display_name.trim().to_owned(),
            given: self
                .given
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            family: self
                .family
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            numbers,
            created_ms: existing.map_or(now_ms, |c| c.created_ms),
            updated_ms: now_ms,
        })
    }
}

impl From<&Contact> for ContactInput {
    fn from(contact: &Contact) -> Self {
        Self {
            display_name: contact.display_name.clone(),
            given: contact.given.clone(),
            family: contact.family.clone(),
            numbers: contact
                .numbers
                .iter()
                .map(|n| NumberInput {
                    label: n.label,
                    raw: n.raw.clone(),
                })
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Search and lookup
// ---------------------------------------------------------------------------

/// How a number matched a contact; E.164 equality ranks above a suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchQuality {
    Suffix,
    Exact,
}

/// Case-insensitive name substring, plus digit-substring against every
/// number when the query contains a digit. An empty or blank query matches
/// everything.
pub fn matches_search(contact: &Contact, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let needle = query.to_lowercase();
    let name_hit = [
        Some(contact.display_name.as_str()),
        contact.given.as_deref(),
        contact.family.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|field| field.to_lowercase().contains(&needle));
    if name_hit {
        return true;
    }
    let query_digits = digits(query);
    !query_digits.is_empty()
        && contact.numbers.iter().any(|n| {
            digits(n.display()).contains(&query_digits) || digits(&n.raw).contains(&query_digits)
        })
}

/// Contacts matching `query`, in catalog order.
pub fn search<'a>(contacts: &'a [Contact], query: &str) -> Vec<&'a Contact> {
    contacts
        .iter()
        .filter(|c| matches_search(c, query))
        .collect()
}

/// Whether `contact` holds `number`: E.164 equality first (after
/// normalizing `number` for `default_region`), then the trailing seven
/// digits of both sides.
pub fn match_number(contact: &Contact, number: &str, default_region: &str) -> Option<MatchQuality> {
    let wanted_e164 = normalize(number, default_region);
    let wanted_digits = digits(number);
    let mut best = None;
    for stored in &contact.numbers {
        if let (Some(a), Some(b)) = (&stored.e164, &wanted_e164)
            && a == b
        {
            return Some(MatchQuality::Exact);
        }
        let stored_digits = digits(stored.e164.as_deref().unwrap_or(&stored.raw));
        if suffix_matches(&stored_digits, &wanted_digits) {
            best = Some(MatchQuality::Suffix);
        }
    }
    best
}

fn suffix_matches(a: &str, b: &str) -> bool {
    if a.len() < SUFFIX_DIGITS || b.len() < SUFFIX_DIGITS {
        return false;
    }
    a[a.len() - SUFFIX_DIGITS..] == b[b.len() - SUFFIX_DIGITS..]
}

/// The best contact for `number`: an exact E.164 match wins. A trailing-digit
/// fallback is returned only when it identifies exactly one contact; an
/// ambiguous suffix must never display the wrong caller identity.
pub fn lookup<'a>(
    contacts: &'a [Contact],
    number: &str,
    default_region: &str,
) -> Option<&'a Contact> {
    let mut suffix: Option<&Contact> = None;
    let mut suffix_ambiguous = false;
    for contact in contacts {
        if let Some(quality) = match_number(contact, number, default_region) {
            if quality == MatchQuality::Exact {
                return Some(contact);
            }
            if suffix.is_some() {
                suffix_ambiguous = true;
            } else {
                suffix = Some(contact);
            }
        }
    }
    if suffix_ambiguous { None } else { suffix }
}

// ---------------------------------------------------------------------------
// Errors and events
// ---------------------------------------------------------------------------

/// Typed failures. Nothing imitates success.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ContactsError {
    InvalidInput {
        reason: String,
    },
    NotFound {
        id: ContactId,
    },
    /// The connection lacks the named capability (`contacts-read`, `contacts-write`).
    Unauthorized {
        capability: String,
    },
    Store {
        reason: String,
    },
    /// The store was written by a newer schema; the service refuses to start.
    SchemaTooNew {
        found: u32,
        supported: u32,
    },
    Vcard {
        reason: String,
    },
    Protocol {
        reason: String,
    },
}

impl fmt::Display for ContactsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContactsError::InvalidInput { reason } => write!(f, "invalid input: {reason}"),
            ContactsError::NotFound { id } => write!(f, "contact {id} not found"),
            ContactsError::Unauthorized { capability } => {
                write!(f, "unauthorized: {capability} not granted")
            }
            ContactsError::Store { reason } => write!(f, "store: {reason}"),
            ContactsError::SchemaTooNew { found, supported } => {
                write!(
                    f,
                    "store schema {found} is newer than supported {supported}"
                )
            }
            ContactsError::Vcard { reason } => write!(f, "vcard: {reason}"),
            ContactsError::Protocol { reason } => write!(f, "protocol: {reason}"),
        }
    }
}

impl std::error::Error for ContactsError {}

/// Kind of a contact change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeKind {
    Created,
    Updated,
    Deleted,
}

impl ChangeKind {
    pub fn name(self) -> &'static str {
        match self {
            ChangeKind::Created => "created",
            ChangeKind::Updated => "updated",
            ChangeKind::Deleted => "deleted",
        }
    }
}

/// Events pushed to subscribers. Records carry only `contact_id` and the
/// kind, never names or numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum ContactEvent {
    ContactChanged {
        id: ContactId,
        kind: ChangeKind,
    },
    /// The service (re)started; subscribers replace their state with the
    /// snapshot they receive on (re)connect.
    ServiceRestarted,
}

impl ContactEvent {
    pub fn name(&self) -> &'static str {
        match self {
            ContactEvent::ContactChanged { .. } => "contact-changed",
            ContactEvent::ServiceRestarted => "service-restarted",
        }
    }
}

/// Capability names checked by the service (mirroring `ferrix-app-api`'s
/// `AppCapability` kebab-case forms).
pub const CAP_READ: &str = "contacts-read";
pub const CAP_WRITE: &str = "contacts-write";

/// Set of capability names, as granted by `ferrix-appd`.
pub type Capabilities = BTreeSet<String>;

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(name: &str, numbers: &[&str]) -> Contact {
        Contact {
            id: ContactId::from_bytes([1, 2, 3, 4, 5, 6, 7, 8]),
            display_name: name.to_owned(),
            given: None,
            family: None,
            numbers: numbers
                .iter()
                .map(|n| PhoneNumber::new(NumberLabel::Mobile, *n, "US"))
                .collect(),
            created_ms: 1,
            updated_ms: 1,
        }
    }

    #[test]
    fn contact_ids_are_c_plus_sixteen_hex() {
        let id = ContactId::random().unwrap();
        assert_eq!(id.as_str().len(), 18);
        assert!(id.as_str().starts_with("c_"));
        assert!(ContactId::parse(id.as_str()).is_ok());
        assert_eq!(
            ContactId::from_bytes([0xde, 0xad, 0xbe, 0xef, 0, 1, 2, 3]).as_str(),
            "c_deadbeef00010203"
        );
        for bad in [
            "",
            "c_",
            "c_DEADBEEF00010203",
            "x_deadbeef00010203",
            "c_deadbeef0001020",
        ] {
            assert!(ContactId::parse(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<ContactId>("\"c_zz\"").is_err());
        let text = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<ContactId>(&text).unwrap(), id);
        assert_ne!(ContactId::random().unwrap(), ContactId::random().unwrap());
    }

    #[test]
    fn inputs_validate_and_become_contacts() {
        let input = ContactInput {
            display_name: "  Test Contact 01 ".to_owned(),
            given: Some("Test".to_owned()),
            family: Some(" ".to_owned()),
            numbers: vec![NumberInput {
                label: NumberLabel::Mobile,
                raw: "(555) 010-0001".to_owned(),
            }],
        };
        let id = ContactId::from_bytes([9; 8]);
        let c = input
            .clone()
            .into_contact(id.clone(), 42, "US", None)
            .unwrap();
        assert_eq!(c.display_name, "Test Contact 01");
        assert_eq!(c.family, None);
        assert_eq!(c.numbers[0].e164.as_deref(), Some("+15550100001"));
        assert_eq!((c.created_ms, c.updated_ms), (42, 42));
        let updated = input.into_contact(id, 99, "US", Some(&c)).unwrap();
        assert_eq!((updated.created_ms, updated.updated_ms), (42, 99));

        let empty = ContactInput::default();
        assert!(matches!(
            empty.validate(),
            Err(ContactsError::InvalidInput { .. })
        ));
        let blank_number = ContactInput {
            display_name: "x".to_owned(),
            numbers: vec![NumberInput {
                label: NumberLabel::Other,
                raw: "   ".to_owned(),
            }],
            ..ContactInput::default()
        };
        assert!(blank_number.validate().is_err());
        assert!(
            serde_json::from_str::<ContactInput>(r#"{"display_name":"a","bogus":1}"#).is_err(),
            "unknown fields are rejected"
        );
    }

    #[test]
    fn search_matches_names_case_insensitively_and_digit_substrings() {
        let c = contact("Test Contact 01", &["+1 555 010 0001"]);
        assert!(matches_search(&c, "test"));
        assert!(matches_search(&c, "CONTACT 01"));
        assert!(matches_search(&c, "0100001"));
        assert!(matches_search(&c, "555-010"));
        assert!(matches_search(&c, ""));
        assert!(!matches_search(&c, "nobody"));
        assert!(!matches_search(&c, "999"));
        let list = vec![c.clone(), contact("Other", &["+15550100002"])];
        assert_eq!(search(&list, "other").len(), 1);
        assert_eq!(search(&list, "555").len(), 2);
    }

    #[test]
    fn lookup_prefers_e164_then_trailing_seven_digits() {
        let exact = contact("Exact", &["+15550100042"]);
        let suffix = contact("Suffix", &["+44 20 0100042"]);
        let unrelated = contact("Unrelated", &["+15550100099"]);
        let list = vec![suffix.clone(), exact.clone(), unrelated];
        assert_eq!(
            lookup(&list, "+1 555 010 0042", "US").unwrap().display_name,
            "Exact"
        );
        assert_eq!(
            lookup(&list, "(555) 010-0042", "US").unwrap().display_name,
            "Exact"
        );
        // Both contacts share the suffix, so caller identity is ambiguous.
        assert_eq!(lookup(&list, "0100042", "US"), None);
        assert_eq!(
            match_number(&exact, "0100042", "US"),
            Some(MatchQuality::Suffix)
        );
        assert_eq!(lookup(&list, "12345", "US"), None);
        // A raw-only stored number still matches by suffix.
        let raw_only = contact("Raw", &["ext 0100077"]);
        assert_eq!(raw_only.numbers[0].e164, None);
        assert_eq!(
            match_number(&raw_only, "+15550100077", "US"),
            Some(MatchQuality::Suffix)
        );
        assert_eq!(
            lookup(std::slice::from_ref(&raw_only), "+15550100077", "US"),
            Some(&raw_only)
        );
    }

    #[test]
    fn errors_and_events_are_tagged() {
        let e = ContactsError::Unauthorized {
            capability: CAP_WRITE.to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"kind":"unauthorized","capability":"contacts-write"}"#
        );
        let ev = ContactEvent::ContactChanged {
            id: ContactId::from_bytes([0; 8]),
            kind: ChangeKind::Created,
        };
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"event":"contact-changed","id":"c_0000000000000000","kind":"created"}"#
        );
        assert_eq!(ContactEvent::ServiceRestarted.name(), "service-restarted");
        assert_eq!(NumberLabel::Other.next(), NumberLabel::Mobile);
    }
}
