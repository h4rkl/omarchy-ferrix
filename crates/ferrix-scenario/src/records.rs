//! Tolerant extraction of JSON evidence records from a shared log.
//!
//! The QEMU serial console is shared by every guest process (and the kernel),
//! so a captured line is not always exactly one record:
//!
//! * console noise can precede a record (`[    4.2] `, a shell prompt);
//! * a writer that emitted one record in several `write(2)` calls can have
//!   another writer's whole line land in the middle of it, which leaves
//!   `HEAD{injected record}` on one line and the record's `TAIL` on the next.
//!
//! The guest writers now emit one `write(2)` per record (see the `emit`
//! helpers and `relay_log` in the initramfs init), so the second case should
//! no longer occur; this parser keeps the runner from losing records or, worse,
//! resynchronising onto a nested object when it does.
//!
//! Rules:
//! * Records start at `{"evidence":` (every Ferrix record leads with its
//!   evidence label). Lines without that anchor fall back to "the first `{`
//!   to the end of the line is one object". Nested objects are therefore never
//!   mistaken for records.
//! * Every anchored record that parses on a line is returned, in order; text
//!   between them is skipped.
//! * An anchored start that does not parse is kept as a fragment. If the very
//!   next line does not itself begin a record, `fragment + line` is tried as
//!   one record. Only a complete JSON object is accepted; otherwise both
//!   pieces are dropped. A fragment never outlives that one attempt.

use serde_json::Value;

const ANCHOR: &str = "{\"evidence\":";

/// Line-by-line record extractor; feed it the lines of one log in order.
#[derive(Debug, Default)]
pub struct RecordParser {
    fragment: Option<String>,
}

impl RecordParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records found in `line`, plus a record completed from the previous
    /// line's fragment.
    pub fn push(&mut self, line: &str) -> Vec<Value> {
        let mut records = Vec::new();
        let mut text = line.to_owned();
        if let Some(head) = self.fragment.take()
            && !line.trim_start().starts_with('{')
        {
            let merged = format!("{head}{line}");
            if let Some((record, end)) = first_object(&merged) {
                records.push(record);
                text = merged[end..].to_owned();
            }
        }
        let (found, fragment) = scan(&text);
        records.extend(found);
        self.fragment = fragment;
        records
    }
}

/// One-shot form for a single, self-contained line.
pub fn parse_line(line: &str) -> Vec<Value> {
    scan(line).0
}

/// The first JSON value in `text` when it is an object, and the byte offset
/// just past it. Trailing text is allowed.
fn first_object(text: &str) -> Option<(Value, usize)> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    match stream.next() {
        Some(Ok(value)) if value.is_object() => Some((value, stream.byte_offset())),
        _ => None,
    }
}

fn scan(text: &str) -> (Vec<Value>, Option<String>) {
    let starts: Vec<usize> = text.match_indices(ANCHOR).map(|(i, _)| i).collect();
    if starts.is_empty() {
        // Unanchored line (tests, foreign logs): first `{` to end of line.
        let record = text
            .find('{')
            .and_then(|start| serde_json::from_str::<Value>(&text[start..]).ok())
            .filter(Value::is_object);
        return (record.into_iter().collect(), None);
    }
    let mut records = Vec::new();
    let mut cursor = 0;
    // Start of the most recent anchored fragment that failed to parse, and
    // where that fragment ends (the next parsed record, or end of line).
    let mut failed: Option<usize> = None;
    let mut fragment: Option<String> = None;
    for start in starts {
        if start < cursor {
            continue;
        }
        match first_object(&text[start..]) {
            Some((record, len)) => {
                if let Some(head) = failed.take() {
                    fragment = Some(text[head..start].to_owned());
                }
                records.push(record);
                cursor = start + len;
            }
            None => {
                failed.get_or_insert(start);
            }
        }
    }
    if let Some(head) = failed {
        fragment = Some(text[head..].to_owned());
    }
    (records, fragment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn events(records: &[Value]) -> Vec<&str> {
        records
            .iter()
            .map(|r| r["event"].as_str().unwrap_or("?"))
            .collect()
    }

    #[test]
    fn console_prefixes_and_plain_text_are_tolerated() {
        assert_eq!(parse_line("[    1.234] {\"event\":\"x\"}")[0]["event"], "x");
        assert_eq!(
            events(&parse_line(
                "/ # \u{1b}[6n{\"evidence\":\"QEMU\",\"event\":\"compositor-ready\"}"
            )),
            ["compositor-ready"]
        );
        assert!(parse_line("plain text").is_empty());
        assert!(parse_line("[1] not json {").is_empty());
    }

    #[test]
    fn an_injected_record_and_the_record_it_split_are_both_recovered() {
        let telephony = json!({
            "evidence": "QEMU",
            "event": "telephony-event",
            "kind": "call-changed",
            "call": {"state": "active", "muted": true},
        })
        .to_string();
        let injected =
            r#"{"evidence":"QEMU","event":"input-ready","capability":"pointer","status":"ok"}"#;
        let cut = telephony.find("\"muted\"").unwrap();
        let mut parser = RecordParser::new();
        let first = parser.push(&format!("{}{injected}", &telephony[..cut]));
        assert_eq!(events(&first), ["input-ready"]);
        let second = parser.push(&telephony[cut..]);
        assert_eq!(events(&second), ["telephony-event"]);
        assert_eq!(second[0]["call"]["muted"], true);
    }

    #[test]
    fn nested_objects_of_a_broken_record_are_never_taken_as_records() {
        let mut parser = RecordParser::new();
        let records = parser.push(
            r#"{"evidence":"QEMU","event":"telephony-event","state":{"modem":{"state":"ready"},"sim":{"sta"#,
        );
        assert!(records.is_empty());
        // The next line starts a fresh record: the fragment is dropped, not
        // glued onto it.
        let records = parser.push(r#"{"evidence":"QEMU","event":"networkd-ready"}"#);
        assert_eq!(events(&records), ["networkd-ready"]);
        // And the fragment is gone for good.
        assert!(parser.push(r#"te":"ready"}}}"#).is_empty());
    }

    #[test]
    fn a_fragment_gets_exactly_one_merge_attempt() {
        let mut parser = RecordParser::new();
        assert!(
            parser
                .push(r#"{"evidence":"QEMU","event":"sms-event","kind""#)
                .is_empty()
        );
        // A kernel line in between: merge fails, both pieces are dropped.
        assert!(parser.push("[   12.5] virtio: noise").is_empty());
        assert!(parser.push(r#":"delivered"}"#).is_empty());
    }

    #[test]
    fn several_records_on_one_line_come_back_in_order() {
        let line = concat!(
            r#"{"evidence":"QEMU","event":"a"}"#,
            "garbage",
            r#"{"evidence":"QEMU","event":"b","nested":{"evidence":"x"}}"#
        );
        assert_eq!(events(&parse_line(line)), ["a", "b"]);
    }
}
