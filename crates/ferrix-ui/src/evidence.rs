//! Machine-readable evidence records: one JSON object per line on stdout with
//! `"evidence"` as the first key, flushed immediately so serial logs capture
//! them in order.

use std::io::Write as _;

/// Prints one complete record line and flushes stdout.
pub fn emit(record: String) {
    println!("{record}");
    let _ = std::io::stdout().flush();
}

/// Escapes a string for inclusion inside a JSON string literal.
pub fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Formats an optional string as a JSON value (`null` when absent).
pub fn json_opt_str(value: Option<&str>) -> String {
    match value {
        Some(text) => format!("\"{}\"", json_escape(text)),
        None => "null".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_backslashes_and_controls() {
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\u000a");
        assert_eq!(json_opt_str(None), "null");
        assert_eq!(json_opt_str(Some("x")), "\"x\"");
    }
}
