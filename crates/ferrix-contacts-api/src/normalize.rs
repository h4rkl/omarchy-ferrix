//! E.164 normalization through the `phonenumber` crate (MIT/Apache-2.0; a
//! Rust port of Google's libphonenumber metadata), per ADR-003.
//!
//! Only parsing and E.164 formatting are used. Validity against the live
//! numbering plan is deliberately **not** required: the mock profile and the
//! fixtures use synthetic `+1 555 010 xxxx` numbers that libphonenumber
//! classes as invalid, and a contact may legitimately store a number the
//! metadata does not know yet. A parse failure, or a result outside the
//! E.164 envelope (`+` and 8..=15 digits, the same rule the telephony
//! contract uses), keeps `raw` and yields `None`.

use std::str::FromStr;

/// Environment variable naming the default region (ISO 3166-1 alpha-2).
pub const DEFAULT_REGION_ENV: &str = "FERRIX_DEFAULT_REGION";
/// Default region when none is configured; the mock profile's value.
pub const DEFAULT_REGION: &str = "US";

/// Shortest and longest digit counts of an E.164 number.
const E164_DIGITS: std::ops::RangeInclusive<usize> = 8..=15;

/// The digits of `text` only, in order (`(555) 010-0042` → `5550100042`).
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// Normalizes `raw` to `+<digits>` for `default_region`, or `None` when the
/// text is not a phone number. `+`-prefixed input ignores the region.
pub fn normalize(raw: &str, default_region: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || digits(raw).is_empty() {
        return None;
    }
    let region = phonenumber::country::Id::from_str(&default_region.to_ascii_uppercase()).ok();
    let parsed = phonenumber::parse(region, raw).ok()?;
    let text = parsed.format().mode(phonenumber::Mode::E164).to_string();
    let body = text.strip_prefix('+')?;
    if !E164_DIGITS.contains(&body.len()) || !body.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // libphonenumber parses leniently (`ext 0100077` becomes `+10100077`);
    // the North American plan is fixed-length, so require its ten national
    // digits before claiming an E.164 form.
    if body.starts_with('1') && body.len() != 11 {
        return None;
    }
    Some(text)
}

/// Whether `text` already has the E.164 shape.
pub fn is_e164(text: &str) -> bool {
    text.strip_prefix('+').is_some_and(|body| {
        E164_DIGITS.contains(&body.len()) && body.chars().all(|c| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_international_and_regional_input() {
        assert_eq!(
            normalize("+1 555 010 0042", "US").as_deref(),
            Some("+15550100042")
        );
        assert_eq!(
            normalize("(555) 010-0042", "US").as_deref(),
            Some("+15550100042")
        );
        assert_eq!(
            normalize("(555) 010-0042", "us").as_deref(),
            Some("+15550100042")
        );
        assert_eq!(
            normalize("+44 20 7946 0958", "US").as_deref(),
            Some("+442079460958")
        );
        assert_eq!(
            normalize("1-555-010-0042", "US").as_deref(),
            Some("+15550100042")
        );
    }

    #[test]
    fn invalid_input_keeps_raw() {
        assert_eq!(normalize("", "US"), None);
        assert_eq!(normalize("garbage", "US"), None);
        assert_eq!(normalize("12", "US"), None);
        assert_eq!(normalize("ext 0100077", "US"), None);
        assert_eq!(normalize("+", "US"), None);
        // An unknown region cannot resolve a national number; `+` still works.
        assert_eq!(normalize("555 010 0042", "ZZ"), None);
        assert_eq!(
            normalize("+15550100042", "ZZ").as_deref(),
            Some("+15550100042")
        );
    }

    #[test]
    fn digits_and_e164_shape() {
        assert_eq!(digits("(555) 010-0042 x9"), "55501000429");
        assert!(is_e164("+15550100042"));
        assert!(!is_e164("15550100042"));
        assert!(!is_e164("+1234567"));
        assert!(!is_e164("+1234567890123456"));
    }
}
