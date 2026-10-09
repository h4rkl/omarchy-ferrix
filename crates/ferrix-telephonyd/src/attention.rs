//! Incoming-call attention: when a new incoming call starts ringing, the
//! service asks `ferrix-appd` to raise whichever application handles the
//! `show-call` intent (Phone). The daemon never names the handler.
//!
//! The request is the raw JSON line
//! `{"request":"open","intent":{"intent":"show-call"}}` on a role-`service`
//! link, spoken like the identity link in [`crate::identity`] so this crate
//! does not link the application protocol. It runs on its own thread: the
//! event loop only enqueues, so a slow or absent `ferrix-appd` can never
//! delay call state. Records: `telephony-attention {status: sent|failed|
//! disabled, reason?}`; never a number.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::identity::{Link, connect_link};
use crate::records::Recorder;

/// Something to alert when a new incoming call rings.
pub trait Attention: Send + Sync {
    fn incoming_call(&self);
}

/// The `open show-call` request line.
pub fn show_call_request() -> Value {
    json!({"request": "open", "intent": {"intent": "show-call"}})
}

/// Classifies `ferrix-appd`'s reply: `Ok` for an instance, otherwise the
/// typed reason (`no-handler`, a protocol error, ...).
pub fn classify_open_reply(reply: &Value) -> Result<(), String> {
    match reply.get("reply").and_then(Value::as_str) {
        Some("instance") => Ok(()),
        Some("error") => Err(reply
            .pointer("/error/kind")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_owned()),
        Some("protocol-error") => Err(format!(
            "protocol-error: {}",
            reply.get("reason").and_then(Value::as_str).unwrap_or("")
        )),
        _ => Err(format!("unexpected appd reply: {reply}")),
    }
}

/// Raises the `show-call` handler through `ferrix-appd` from a worker
/// thread. Bursts collapse: alerts queued while one is in flight are sent
/// as a single request.
pub struct AppdAttention {
    tx: Mutex<Sender<()>>,
}

impl AppdAttention {
    pub fn spawn(socket: PathBuf, timeout: Duration, recorder: Recorder) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<()>();
        thread::Builder::new()
            .name("ferrix-telephonyd-attention".to_owned())
            .spawn(move || {
                let mut link: Option<Link> = None;
                while rx.recv().is_ok() {
                    while rx.try_recv().is_ok() {}
                    let result = send(&mut link, &socket, timeout);
                    recorder.attention(
                        match &result {
                            Ok(()) => "sent",
                            Err(_) => "failed",
                        },
                        result.err().as_deref(),
                    );
                }
            })
            .map_err(|e| format!("spawn attention thread: {e}"))?;
        Ok(Self { tx: Mutex::new(tx) })
    }
}

/// One attempt on the cached link, then one on a fresh link.
fn send(
    link: &mut Option<Link>,
    socket: &std::path::Path,
    timeout: Duration,
) -> Result<(), String> {
    let request = show_call_request();
    let mut last = String::new();
    for _ in 0..2 {
        if link.is_none() {
            *link = Some(connect_link(socket, timeout)?);
        }
        let Some(current) = link.as_mut() else {
            break;
        };
        match current.exchange(&request) {
            Ok(reply) => return classify_open_reply(&reply),
            Err(error) => {
                *link = None;
                last = error;
            }
        }
    }
    Err(last)
}

impl Attention for AppdAttention {
    fn incoming_call(&self) {
        let _ = lock(&self.tx).send(());
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_reply_forms() {
        assert_eq!(
            show_call_request().to_string(),
            r#"{"request":"open","intent":{"intent":"show-call"}}"#
        );
        assert_eq!(
            classify_open_reply(&json!({"reply":"instance","schema_version":1})),
            Ok(())
        );
        assert_eq!(
            classify_open_reply(
                &json!({"reply":"error","error":{"kind":"no-handler","intent":"show-call"}})
            ),
            Err("no-handler".to_owned())
        );
        assert!(
            classify_open_reply(&json!({"reply":"protocol-error","reason":"role"}))
                .unwrap_err()
                .starts_with("protocol-error")
        );
    }

    #[test]
    fn an_unreachable_appd_is_recorded_as_failed() {
        let (recorder, records) = Recorder::capturing("mock");
        let attention = AppdAttention::spawn(
            PathBuf::from("/nonexistent/ferrix-appd.sock"),
            Duration::from_millis(50),
            recorder,
        )
        .unwrap();
        attention.incoming_call();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let found = records
                .lock()
                .unwrap()
                .iter()
                .any(|r| r["event"] == "telephony-attention" && r["status"] == "failed");
            if found {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no failed record");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
