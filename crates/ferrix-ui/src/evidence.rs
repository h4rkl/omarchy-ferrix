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
///
/// Every record carries the emitting process's identity (see
/// [`process_identity`]) unless it already names a `process`, so records
/// from the many surfaces that share one log (`theme-loaded`, `input-ready`,
/// `cursor-ready`, ...) no longer have to be attributed by log order. The
/// line goes out in a single `write(2)`: the guest relays these logs onto a
/// serial console that other processes write to, and a record written in
/// pieces can have another writer's line land in the middle of it.
pub fn emit(record: String) {
    let mut line = with_identity(&record, process_identity());
    line.push('\n');
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.flush();
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.flush();
}

/// Identity of this process as JSON members with a leading comma:
/// `,"process":"<argv0 basename>","pid":<pid>` plus `,"app_id":"<id>"` when
/// `FERRIX_APP_ID` is set (as `ferrix-appd` does for every app it launches).
/// Inside an application sandbox `pid` is the pid in the app's own PID
/// namespace; `app_id` is the stable discriminator there.
pub fn process_identity() -> &'static str {
    static VALUE: OnceLock<String> = OnceLock::new();
    VALUE.get_or_init(|| {
        let process = std::env::args_os()
            .next()
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "unknown".to_owned());
        let app_id = std::env::var("FERRIX_APP_ID")
            .ok()
            .filter(|id| !id.is_empty());
        identity_members(&process, std::process::id(), app_id.as_deref())
    })
}

fn identity_members(process: &str, pid: u32, app_id: Option<&str>) -> String {
    let mut out = format!(",\"process\":\"{}\",\"pid\":{pid}", json_escape(process));
    if let Some(app_id) = app_id {
        out.push_str(&format!(",\"app_id\":\"{}\"", json_escape(app_id)));
    }
    out
}

/// Inserts `identity` (members with a leading comma) before the closing brace
/// of a one-line JSON object. Records that already carry a `process` member,
/// and anything that is not an object, pass through unchanged.
pub fn with_identity(record: &str, identity: &str) -> String {
    let trimmed = record.trim_end();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') || trimmed.contains("\"process\":") {
        return record.to_owned();
    }
    let body = trimmed[..trimmed.len() - 1].trim_end();
    if body == "{" {
        // Empty object: no member before ours, so no leading comma.
        return format!("{{{}}}", identity.trim_start_matches(','));
    }
    format!("{body}{identity}}}")
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
    fn records_gain_the_process_identity_once() {
        let identity = identity_members("ferrix-settings", 7, Some("org.ferrix.settings"));
        assert_eq!(
            identity,
            r#","process":"ferrix-settings","pid":7,"app_id":"org.ferrix.settings""#
        );
        assert_eq!(
            with_identity(r#"{"evidence":"QEMU","event":"theme-loaded"}"#, &identity),
            r#"{"evidence":"QEMU","event":"theme-loaded","process":"ferrix-settings","pid":7,"app_id":"org.ferrix.settings"}"#
        );
        assert_eq!(
            with_identity("{ }", r#","process":"x","pid":1"#),
            r#"{"process":"x","pid":1}"#
        );
        // Already attributed, or not an object: unchanged.
        let attributed = r#"{"event":"x","process":"other"}"#;
        assert_eq!(with_identity(attributed, &identity), attributed);
        assert_eq!(with_identity("plain text", &identity), "plain text");
        assert_eq!(
            identity_members("ferrix-shell", 42, None),
            r#","process":"ferrix-shell","pid":42"#
        );
    }

    #[test]
    fn process_identity_names_this_process() {
        let identity = process_identity();
        assert!(identity.starts_with(",\"process\":\""));
        assert!(identity.contains(&format!("\"pid\":{}", std::process::id())));
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
