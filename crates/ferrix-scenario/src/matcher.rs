//! JSON subset matching for `expect` entries and ack checks.
//!
//! `expected` is a subset of `actual` when: objects require every expected
//! key to be present and (recursively) match; arrays match element-wise in
//! order and `actual` may be longer; scalars must be equal with equal types
//! (a number never matches a string). Field names live only in the scenario
//! files, so record shapes can change without touching this crate.

use serde_json::Value;

pub fn is_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(want), Value::Object(have)) => want
            .iter()
            .all(|(key, value)| have.get(key).is_some_and(|got| is_subset(value, got))),
        (Value::Array(want), Value::Array(have)) => {
            want.len() <= have.len() && want.iter().zip(have.iter()).all(|(w, h)| is_subset(w, h))
        }
        (Value::Number(want), Value::Number(have)) => {
            want == have || want.as_f64() == have.as_f64()
        }
        (Value::String(want), Value::String(have)) => want == have,
        (Value::Bool(want), Value::Bool(have)) => want == have,
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

/// Finds `expects` in `lines` in order (each match must come after the
/// previous one). Returns the matched line indexes, or the index of the first
/// expect that was not found.
pub fn match_in_order(lines: &[Value], expects: &[Value]) -> Result<Vec<usize>, usize> {
    let mut matched = Vec::with_capacity(expects.len());
    let mut from = 0;
    for (i, expect) in expects.iter().enumerate() {
        let found = lines[from.min(lines.len())..]
            .iter()
            .position(|line| is_subset(expect, line))
            .map(|offset| from + offset)
            .ok_or(i)?;
        matched.push(found);
        from = found + 1;
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_objects_and_extra_keys() {
        let actual = json!({"evidence":"QEMU","event":"telephony-event","call":{"id":"c1","state":"ringing","extra":1},"more":true});
        assert!(is_subset(&json!({"event":"telephony-event"}), &actual));
        assert!(is_subset(&json!({"call":{"state":"ringing"}}), &actual));
        assert!(!is_subset(&json!({"call":{"state":"active"}}), &actual));
        assert!(!is_subset(&json!({"missing":1}), &actual));
        assert!(is_subset(&json!({}), &actual));
    }

    #[test]
    fn arrays_match_in_order_and_allow_longer_actual() {
        let actual =
            json!({"routes":["earpiece","speaker","bluetooth"],"parts":[{"index":0},{"index":1}]});
        assert!(is_subset(
            &json!({"routes":["earpiece","speaker"]}),
            &actual
        ));
        assert!(!is_subset(
            &json!({"routes":["speaker","earpiece"]}),
            &actual
        ));
        assert!(!is_subset(
            &json!({"routes":["earpiece","speaker","bluetooth","wired"]}),
            &actual
        ));
        assert!(is_subset(&json!({"parts":[{"index":0},{}]}), &actual));
        assert!(!is_subset(&json!({"parts":[{"index":1}]}), &actual));
    }

    #[test]
    fn numbers_never_match_strings() {
        let actual = json!({"bars":3,"pct":78.0,"id":"3"});
        assert!(is_subset(&json!({"bars":3}), &actual));
        assert!(is_subset(&json!({"pct":78}), &actual));
        assert!(!is_subset(&json!({"bars":"3"}), &actual));
        assert!(!is_subset(&json!({"id":3}), &actual));
        assert!(!is_subset(&json!({"bars":4}), &actual));
        assert!(!is_subset(&json!({"bars":true}), &actual));
        assert!(!is_subset(&json!({"bars":null}), &actual));
    }

    #[test]
    fn ordered_matching() {
        let lines = vec![
            json!({"event":"a","n":1}),
            json!({"event":"b","n":2}),
            json!({"event":"a","n":3}),
        ];
        assert_eq!(
            match_in_order(&lines, &[json!({"event":"a"}), json!({"event":"b"})]),
            Ok(vec![0, 1])
        );
        assert_eq!(
            match_in_order(&lines, &[json!({"event":"b"}), json!({"event":"a"})]),
            Ok(vec![1, 2])
        );
        // The same line may not satisfy two expects; ordering is strict.
        assert_eq!(
            match_in_order(&lines, &[json!({"event":"a","n":3}), json!({"event":"b"})]),
            Err(1)
        );
        assert_eq!(match_in_order(&lines, &[json!({"event":"zzz"})]), Err(0));
        assert_eq!(match_in_order(&[], &[]), Ok(vec![]));
    }
}
