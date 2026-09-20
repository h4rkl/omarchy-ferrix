//! Machine-readable readiness and failure records.
//!
//! Every record is a single JSON line on stdout so the guest init script and
//! host agents can grep for it. The `evidence` label is `QEMU` for the guest
//! DRM backend and `mock` for the nested host backend; neither is a
//! physical-device claim.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::fmt::Write as _;

/// Where the compositor is running; controls the `evidence` label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Evidence {
    /// Running inside the generic AArch64 QEMU `virt` guest.
    Qemu,
    /// Running nested on a development host from the mock profile.
    Mock,
}

impl Evidence {
    pub fn label(self) -> &'static str {
        match self {
            Evidence::Qemu => "QEMU",
            Evidence::Mock => "mock",
        }
    }
}

/// A JSON scalar for record fields.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_owned())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Int(i64::from(v))
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(i64::from(v))
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

/// Escape a string for inclusion in a JSON string literal.
pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Build one JSON record line: `{"evidence":..,"event":..,"compositor":"mobile-shell",...}`.
pub fn record(evidence: Evidence, event: &str, fields: &[(&str, Value)]) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"evidence\":\"{}\",\"event\":\"{}\",\"compositor\":\"mobile-shell\"",
        evidence.label(),
        escape(event)
    );
    for (key, value) in fields {
        let _ = write!(out, ",\"{}\":", escape(key));
        match value {
            Value::Str(s) => {
                let _ = write!(out, "\"{}\"", escape(s));
            }
            Value::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Value::Bool(b) => {
                let _ = write!(out, "{b}");
            }
        }
    }
    out.push('}');
    out
}

/// Print a record on stdout and flush so init sees it immediately.
pub fn emit(line: &str) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{line}");
    let _ = lock.flush();
}

pub fn compositor_ready(evidence: Evidence, socket: &str, width: u32, height: u32) -> String {
    record(
        evidence,
        "compositor-ready",
        &[
            ("socket", socket.into()),
            ("width", width.into()),
            ("height", height.into()),
        ],
    )
}

pub fn compositor_failed(evidence: Evidence, reason: &str, detail: &str) -> String {
    record(
        evidence,
        "compositor-failed",
        &[("reason", reason.into()), ("detail", detail.into())],
    )
}

pub fn client_mapped(
    evidence: Evidence,
    app_id: &str,
    title: &str,
    width: i32,
    height: i32,
    clients: usize,
) -> String {
    record(
        evidence,
        "client-mapped",
        &[
            ("app_id", app_id.into()),
            ("title", title.into()),
            ("width", width.into()),
            ("height", height.into()),
            ("toplevels", Value::Int(clients as i64)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_record_matches_contract() {
        let line = compositor_ready(Evidence::Qemu, "wayland-0", 1080, 2340);
        assert_eq!(
            line,
            "{\"evidence\":\"QEMU\",\"event\":\"compositor-ready\",\"compositor\":\"mobile-shell\",\"socket\":\"wayland-0\",\"width\":1080,\"height\":2340}"
        );
    }

    #[test]
    fn mock_label_for_nested() {
        let line = compositor_ready(Evidence::Mock, "wayland-1", 540, 1170);
        assert!(line.starts_with("{\"evidence\":\"mock\",\"event\":\"compositor-ready\""));
    }

    #[test]
    fn escapes_quotes_and_control_characters() {
        assert_eq!(escape("a\"b\\c\nd\u{1}"), "a\\\"b\\\\c\\nd\\u0001");
        let line = compositor_failed(Evidence::Qemu, "drm", "no \"card\"");
        assert!(line.contains("\"detail\":\"no \\\"card\\\"\""));
    }

    #[test]
    fn client_mapped_carries_geometry_and_count() {
        let line = client_mapped(Evidence::Qemu, "mobile-basic-screen", "", 1080, 2340, 1);
        assert!(line.contains("\"event\":\"client-mapped\""));
        assert!(line.contains("\"app_id\":\"mobile-basic-screen\""));
        assert!(line.contains("\"width\":1080,\"height\":2340,\"toplevels\":1}"));
    }
}
