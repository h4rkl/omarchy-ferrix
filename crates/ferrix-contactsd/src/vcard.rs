//! vCard 4.0 export and 3.0/4.0 import of the `FN`, `N`, `TEL`, and `UID`
//! properties.
//!
//! Hand-rolled subset, fit reason (ADR-003): the maintained Rust vCard
//! crates are either 3.0-only or unmaintained, and the milestone needs only
//! names and numbers. This module stays under 300 lines and is fully tested
//! (line folding, CRLF and LF input, parameter forms of both versions,
//! escaping, `tel:` URIs, unknown properties ignored).

use ferrix_contacts_api::{Contact, ContactInput, ContactsError, NumberInput, NumberLabel};

const CRLF: &str = "\r\n";
/// RFC 6350 recommends folding at 75 octets.
const FOLD_AT: usize = 75;

/// Escapes a text value per RFC 6350 §3.4.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Folds a content line at [`FOLD_AT`] bytes on character boundaries.
fn fold(line: &str, out: &mut String) {
    let mut budget = FOLD_AT;
    let mut first = true;
    let mut current = String::new();
    for ch in line.chars() {
        if current.len() + ch.len_utf8() > budget {
            out.push_str(&current);
            out.push_str(CRLF);
            current = String::from(" ");
            budget = FOLD_AT;
            first = false;
        }
        current.push(ch);
    }
    let _ = first;
    out.push_str(&current);
    out.push_str(CRLF);
}

fn tel_type(label: NumberLabel) -> &'static str {
    match label {
        NumberLabel::Mobile => "cell",
        NumberLabel::Home => "home",
        NumberLabel::Work => "work",
        NumberLabel::Other => "voice",
    }
}

/// vCard 4.0 text for `contacts`: `FN`, `N`, one `TEL` per number, `UID`.
pub fn export(contacts: &[Contact]) -> String {
    let mut out = String::new();
    for contact in contacts {
        fold("BEGIN:VCARD", &mut out);
        fold("VERSION:4.0", &mut out);
        fold(&format!("FN:{}", escape(&contact.display_name)), &mut out);
        fold(
            &format!(
                "N:{};{};;;",
                escape(contact.family.as_deref().unwrap_or("")),
                escape(contact.given.as_deref().unwrap_or(""))
            ),
            &mut out,
        );
        for number in &contact.numbers {
            let value = match &number.e164 {
                Some(e164) => format!("tel:{e164}"),
                None => escape(&number.raw),
            };
            fold(
                &format!("TEL;TYPE={}:{}", tel_type(number.label), value),
                &mut out,
            );
        }
        fold(&format!("UID:{}", contact.id), &mut out);
        fold("END:VCARD", &mut out);
    }
    out
}

/// Joins folded lines (a line starting with a space or tab continues the
/// previous one) and drops CR.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t'))
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
        } else if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    lines
}

/// Splits `NAME;PARAM=V;PARAM2:value`, honoring quoted parameter values.
fn split_line(line: &str) -> Option<(String, Vec<String>, String)> {
    let mut in_quotes = false;
    let mut colon = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    // `group.NAME` prefixes are allowed by both versions.
    let name = name.rsplit('.').next().unwrap_or("").to_owned();
    let params = parts.map(|p| p.trim().to_ascii_lowercase()).collect();
    Some((name, params, value.to_owned()))
}

fn label_from_params(params: &[String]) -> NumberLabel {
    let mut types: Vec<&str> = Vec::new();
    for param in params {
        match param.split_once('=') {
            Some(("type", value)) => {
                types.extend(value.trim_matches('"').split(','));
            }
            // vCard 3.0 allowed bare type names (`TEL;CELL;VOICE:`).
            None => types.push(param.as_str()),
            _ => {}
        }
    }
    for t in &types {
        match t.trim() {
            "cell" | "mobile" => return NumberLabel::Mobile,
            _ => {}
        }
    }
    for t in &types {
        match t.trim() {
            "home" => return NumberLabel::Home,
            "work" => return NumberLabel::Work,
            _ => {}
        }
    }
    NumberLabel::Other
}

/// Parses every `VCARD` in `text` into inputs (ids and timestamps are the
/// service's). Unknown properties are ignored; a card without `FN` takes its
/// name from `N`, and a card with neither is an error.
pub fn import(text: &str) -> Result<Vec<ContactInput>, ContactsError> {
    let err = |reason: String| ContactsError::Vcard { reason };
    let mut out = Vec::new();
    let mut current: Option<ContactInput> = None;
    let mut saw_fn = false;
    for line in unfold(text) {
        let Some((name, params, value)) = split_line(&line) else {
            continue;
        };
        match name.as_str() {
            "BEGIN" if value.trim().eq_ignore_ascii_case("VCARD") => {
                if current.is_some() {
                    return Err(err("nested BEGIN:VCARD".to_owned()));
                }
                current = Some(ContactInput::default());
                saw_fn = false;
            }
            "END" if value.trim().eq_ignore_ascii_case("VCARD") => {
                let card = current
                    .take()
                    .ok_or_else(|| err("END:VCARD without BEGIN".to_owned()))?;
                if card.display_name.trim().is_empty() {
                    return Err(err("card has neither FN nor N".to_owned()));
                }
                out.push(card);
            }
            _ => {
                let Some(card) = current.as_mut() else {
                    continue;
                };
                match name.as_str() {
                    "FN" => {
                        card.display_name = unescape(value.trim());
                        saw_fn = true;
                    }
                    "N" => {
                        let mut parts = value.split(';');
                        let family = unescape(parts.next().unwrap_or("").trim());
                        let given = unescape(parts.next().unwrap_or("").trim());
                        card.family = (!family.is_empty()).then_some(family.clone());
                        card.given = (!given.is_empty()).then_some(given.clone());
                        if !saw_fn {
                            card.display_name = format!("{given} {family}").trim().to_owned();
                        }
                    }
                    "TEL" => {
                        let raw = value.trim();
                        let raw = raw
                            .strip_prefix("tel:")
                            .or_else(|| raw.strip_prefix("TEL:"))
                            .unwrap_or(raw);
                        let raw = unescape(raw);
                        if !raw.is_empty() {
                            card.numbers.push(NumberInput {
                                label: label_from_params(&params),
                                raw,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if current.is_some() {
        return Err(err("unterminated VCARD".to_owned()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_contacts_api::{ContactId, PhoneNumber};

    fn contact() -> Contact {
        Contact {
            id: ContactId::from_bytes([0, 0, 0, 0, 0, 0, 0, 7]),
            display_name: "Test Contact 07, Jr.; Esq".to_owned(),
            given: Some("Test".to_owned()),
            family: Some("Contact".to_owned()),
            numbers: vec![
                PhoneNumber::new(NumberLabel::Mobile, "+1 555 010 0007", "US"),
                PhoneNumber::new(NumberLabel::Work, "ext 4242", "US"),
            ],
            created_ms: 1,
            updated_ms: 2,
        }
    }

    #[test]
    fn export_writes_vcard_4_with_crlf_and_escapes() {
        let text = export(&[contact()]);
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(lines[0], "BEGIN:VCARD");
        assert_eq!(lines[1], "VERSION:4.0");
        assert_eq!(lines[2], "FN:Test Contact 07\\, Jr.\\; Esq");
        assert_eq!(lines[3], "N:Contact;Test;;;");
        assert_eq!(lines[4], "TEL;TYPE=cell:tel:+15550100007");
        assert_eq!(lines[5], "TEL;TYPE=work:ext 4242");
        assert_eq!(lines[6], "UID:c_0000000000000007");
        assert_eq!(lines[7], "END:VCARD");
        assert!(!text.contains("\n\n"));
    }

    #[test]
    fn round_trip_preserves_names_labels_and_numbers() {
        let cards = import(&export(&[contact()])).unwrap();
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.display_name, "Test Contact 07, Jr.; Esq");
        assert_eq!(card.given.as_deref(), Some("Test"));
        assert_eq!(card.family.as_deref(), Some("Contact"));
        assert_eq!(card.numbers.len(), 2);
        assert_eq!(card.numbers[0].label, NumberLabel::Mobile);
        assert_eq!(card.numbers[0].raw, "+15550100007");
        assert_eq!(card.numbers[1].label, NumberLabel::Work);
        assert_eq!(card.numbers[1].raw, "ext 4242");
    }

    #[test]
    fn imports_vcard_3_with_folding_and_lf_only_input() {
        let text = "BEGIN:VCARD\nVERSION:3.0\nFN:Test Con\n tact 03\nN:Contact;Test;;;\nTEL;TYPE=CELL,VOICE:(555) 010-0003\nTEL;HOME:+1 555 010 0033\nitem1.TEL;TYPE=\"work\":555-010-0333\nEMAIL:ignored@example.invalid\nEND:VCARD\n";
        let cards = import(text).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].display_name, "Test Contact 03");
        let labels: Vec<NumberLabel> = cards[0].numbers.iter().map(|n| n.label).collect();
        assert_eq!(
            labels,
            [NumberLabel::Mobile, NumberLabel::Home, NumberLabel::Work]
        );
        assert_eq!(cards[0].numbers[0].raw, "(555) 010-0003");
    }

    #[test]
    fn long_lines_fold_and_unfold() {
        let long = Contact {
            display_name: "X".repeat(200),
            ..contact()
        };
        let text = export(&[long]);
        assert!(
            text.lines().all(|l| l.len() <= FOLD_AT),
            "every line folded"
        );
        assert!(text.contains("\r\n X"));
        let cards = import(&text).unwrap();
        assert_eq!(cards[0].display_name.len(), 200);
    }

    #[test]
    fn name_falls_back_to_n_and_malformed_cards_are_typed_errors() {
        let text = "BEGIN:VCARD\r\nVERSION:4.0\r\nN:Contact;Test;;;\r\nEND:VCARD\r\n";
        assert_eq!(import(text).unwrap()[0].display_name, "Test Contact");
        assert!(matches!(
            import("BEGIN:VCARD\r\nFN:x\r\n"),
            Err(ContactsError::Vcard { .. })
        ));
        assert!(import("BEGIN:VCARD\r\nEND:VCARD\r\n").is_err(), "no name");
        assert!(
            import("FN:stray\r\nEND:VCARD\r\n").is_err(),
            "END without BEGIN"
        );
        assert_eq!(import("").unwrap().len(), 0);
        assert_eq!(unescape("a\\,b\\;c\\\\d\\ne"), "a,b;c\\d\ne");
    }
}
