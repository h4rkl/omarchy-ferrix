//! Explicit schema migrations, one function per step, applied in order to a
//! parsed JSON document. Every step has a fixture under `tests/fixtures/`
//! and a test.
//!
//! - **v0 (pre-release shape)**: `{"contacts":[{"id","name","phones":[..]}]}`
//!   without a `schema_version`. Ids that already have the `c_` form are
//!   kept; others get a fresh id. `name` becomes `display_name`; each phone
//!   string becomes an `other` number normalized for the default region.
//! - **v1**: the [`Document`](crate::store::Document) of ADR-003.

use ferrix_contacts_api::{
    Contact, ContactId, ContactsError, NumberLabel, PhoneNumber, SCHEMA_VERSION,
};
use serde_json::{Map, Value, json};

use crate::store::Document;

fn store_err(reason: impl ToString) -> ContactsError {
    ContactsError::Store {
        reason: reason.to_string(),
    }
}

/// Schema version claimed by `value`: missing means the pre-release v0.
pub fn version_of(value: &Value) -> Result<u32, ContactsError> {
    match value.get("schema_version") {
        None => Ok(0),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| store_err("schema_version is not a small integer")),
        Some(_) => Err(store_err("schema_version is not a number")),
    }
}

/// Migrates `value` to the current schema, returning the parsed document and
/// the `(from, to)` steps applied. A newer version is a typed refusal.
pub fn migrate(mut value: Value) -> Result<(Document, Vec<(u32, u32)>), ContactsError> {
    let mut version = version_of(&value)?;
    if version > SCHEMA_VERSION {
        return Err(ContactsError::SchemaTooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    let mut steps = Vec::new();
    while version < SCHEMA_VERSION {
        value = match version {
            0 => v0_to_v1(value)?,
            other => return Err(store_err(format!("no migration from schema {other}"))),
        };
        steps.push((version, version + 1));
        version += 1;
    }
    let document: Document = serde_json::from_value(value)
        .map_err(|e| store_err(format!("schema {SCHEMA_VERSION}: {e}")))?;
    if document.schema_version != SCHEMA_VERSION {
        return Err(store_err("migrated document has the wrong version"));
    }
    Ok((document, steps))
}

/// Region used when migrating pre-release numbers.
fn region() -> String {
    std::env::var(ferrix_contacts_api::DEFAULT_REGION_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| ferrix_contacts_api::DEFAULT_REGION.to_owned())
}

fn v0_to_v1(value: Value) -> Result<Value, ContactsError> {
    let Value::Object(map) = value else {
        return Err(store_err("v0 document is not an object"));
    };
    let entries = match map.get("contacts") {
        Some(Value::Array(items)) => items.clone(),
        None => Vec::new(),
        Some(_) => return Err(store_err("v0 contacts is not an array")),
    };
    let region = region();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut contacts = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let Value::Object(entry) = entry else {
            return Err(store_err(format!("v0 contact {index} is not an object")));
        };
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| ContactId::parse(s).ok())
            .map_or_else(ContactId::random, Ok)?;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Unnamed");
        let numbers: Vec<PhoneNumber> = match entry.get("phones") {
            Some(Value::Array(phones)) => phones
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(|raw| PhoneNumber::new(NumberLabel::Other, raw.trim(), &region))
                .collect(),
            _ => Vec::new(),
        };
        let created = entry
            .get("created_ms")
            .and_then(Value::as_u64)
            .unwrap_or(now);
        contacts.push(Contact {
            id,
            display_name: name.to_owned(),
            given: None,
            family: None,
            numbers,
            created_ms: created,
            updated_ms: created,
        });
    }
    let mut out = Map::new();
    out.insert("schema_version".to_owned(), json!(1));
    out.insert(
        "contacts".to_owned(),
        serde_json::to_value(contacts).map_err(store_err)?,
    );
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn v1_fixture_loads_without_migration() {
        let value: Value = serde_json::from_str(&fixture("v1.json")).unwrap();
        let (document, steps) = migrate(value).unwrap();
        assert!(steps.is_empty());
        assert_eq!(document.schema_version, 1);
        assert_eq!(document.contacts.len(), 2);
        assert_eq!(
            document.contacts[0].numbers[0].e164.as_deref(),
            Some("+15550100001")
        );
    }

    #[test]
    fn v0_prerelease_fixture_migrates_to_v1() {
        let value: Value = serde_json::from_str(&fixture("v0-prerelease.json")).unwrap();
        assert_eq!(version_of(&value).unwrap(), 0);
        let (document, steps) = migrate(value).unwrap();
        assert_eq!(steps, vec![(0, 1)]);
        assert_eq!(document.contacts.len(), 2);
        let kept = &document.contacts[0];
        assert_eq!(
            kept.id.as_str(),
            "c_00000000000000a1",
            "well-formed ids are kept"
        );
        assert_eq!(kept.display_name, "Test Contact A1");
        assert_eq!(kept.numbers.len(), 2);
        assert_eq!(kept.numbers[0].label, NumberLabel::Other);
        assert_eq!(kept.numbers[0].e164.as_deref(), Some("+15550100091"));
        assert_eq!(kept.numbers[1].e164, None, "unnormalizable keeps raw only");
        assert_eq!(kept.created_ms, 1_700_000_000_000);
        let fresh = &document.contacts[1];
        assert_ne!(fresh.id.as_str(), "legacy-2", "malformed ids are replaced");
        assert!(ContactId::parse(fresh.id.as_str()).is_ok());
        assert_eq!(fresh.display_name, "Unnamed");
        // The migrated document is a valid v1 document when re-serialized.
        let text = serde_json::to_string(&document).unwrap();
        let (again, steps) = migrate(serde_json::from_str(&text).unwrap()).unwrap();
        assert!(steps.is_empty());
        assert_eq!(again, document);
    }

    #[test]
    fn newer_and_malformed_versions_are_typed() {
        assert_eq!(
            migrate(json!({"schema_version": 2, "contacts": []})).err(),
            Some(ContactsError::SchemaTooNew {
                found: 2,
                supported: 1
            })
        );
        assert!(matches!(
            migrate(json!({"schema_version": "one"})).err(),
            Some(ContactsError::Store { .. })
        ));
        assert!(matches!(
            migrate(json!({"schema_version": 1, "contacts": {}})).err(),
            Some(ContactsError::Store { .. })
        ));
        assert!(matches!(
            migrate(json!({"schema_version": 1, "contacts": [], "extra": 1})).err(),
            Some(ContactsError::Store { .. })
        ));
    }
}
