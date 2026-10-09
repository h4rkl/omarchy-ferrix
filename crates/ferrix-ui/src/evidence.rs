//! Machine-readable evidence records: one JSON object per line on stdout with
//! `"evidence"` as the first key, flushed immediately so serial logs capture
//! them in order.
//!
//! The session evidence label is resolved once from `FERRIX_EVIDENCE`, which
//! the target bootstrap sets (for example `physical:<target-id>` on an
//! admitted board). When it is unset or empty the label is `QEMU`, the
//! generic development target, so QEMU output stays byte-identical. Host
//! previews emit their explicit [`EVIDENCE_MOCK`] records instead.

use std::fmt;
use std::io::Write as _;
use std::ops::Deref;
use std::sync::OnceLock;

/// Label used when the bootstrap does not name a target (the QEMU guest).
pub const EVIDENCE_DEFAULT: &str = "QEMU";
/// Label for nested host runs and host previews driven by the mock profile.
pub const EVIDENCE_MOCK: &str = "mock";
/// Environment variable the target bootstrap uses to name the evidence.
pub const EVIDENCE_ENV: &str = "FERRIX_EVIDENCE";

/// Resolves a raw `FERRIX_EVIDENCE` value: empty or missing selects
/// [`EVIDENCE_DEFAULT`].
pub fn resolve_label(raw: Option<&str>) -> &str {
    raw.filter(|label| !label.is_empty())
        .unwrap_or(EVIDENCE_DEFAULT)
}

/// The session evidence label, read from the environment once per process.
pub fn label() -> &'static str {
    static VALUE: OnceLock<String> = OnceLock::new();
    VALUE.get_or_init(|| {
        let raw = std::env::var(EVIDENCE_ENV).ok();
        resolve_label(raw.as_deref()).to_owned()
    })
}

/// Zero-sized handle for [`label`] that formats and derefs as the label, so
/// record templates can keep inline `{EVIDENCE}` arguments:
/// `const EVIDENCE: SessionEvidence = SessionEvidence;`.
#[derive(Debug)]
pub struct SessionEvidence;

impl Deref for SessionEvidence {
    type Target = str;

    fn deref(&self) -> &str {
        label()
    }
}

impl fmt::Display for SessionEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(label())
    }
}

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

    #[test]
    fn evidence_defaults_to_qemu_and_honours_the_target_label() {
        assert_eq!(resolve_label(None), "QEMU");
        assert_eq!(resolve_label(Some("")), "QEMU");
        assert_eq!(
            resolve_label(Some("physical:google-pixel-9a-tegu-gxq96-mp1-0")),
            "physical:google-pixel-9a-tegu-gxq96-mp1-0"
        );
    }

    #[test]
    fn session_evidence_formats_as_the_resolved_label() {
        assert_eq!(SessionEvidence.to_string(), label());
        assert_eq!(&*SessionEvidence, label());
    }
}
