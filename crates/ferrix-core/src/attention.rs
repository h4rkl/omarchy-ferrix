//! Incoming Bluetooth pairing attention: when a remote starts pairing with
//! this phone and its request waits on the user, the core asks
//! `ferrix-appd` to raise whichever application handles the system
//! `show-bluetooth-pairing` intent (Settings), over whatever is in front.
//! It mirrors the incoming-call raise in `ferrix-telephonyd`.
//!
//! The decision is edge-triggered on the evidence-safe session summary
//! ([`BluetoothSessionSummary::incoming_prompt`]); the intent carries
//! nothing, so no device name, id, passkey, or PIN leaves the core here.
//! Sending runs on its own thread: the event pump only enqueues, so a slow
//! or absent `ferrix-appd` never delays hardware events. The 30 s prompt
//! timeout stays the adapter's (an unanswered request is declined there).
//!
//! Records: `bluetooth-attention {status: sent|failed|disabled, reason?}`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use ferrix_app_api::AppIntent;
use ferrix_app_protocol::{Client, ClientRole};
use ferrix_hardware_api::BluetoothSessionSummary;
use serde_json::{Value, json};

/// Whether a session summary should raise the system prompt, given the
/// previous summary's answer. Only the rising edge raises.
#[derive(Debug, Default)]
pub struct IncomingEdge {
    prompting: bool,
}

impl IncomingEdge {
    /// Folds one summary; true when a new incoming request just started
    /// waiting on the user.
    pub fn update(&mut self, summary: &BluetoothSessionSummary) -> bool {
        let now = summary.incoming_prompt();
        let raise = now && !self.prompting;
        self.prompting = now;
        raise
    }
}

fn record(status: &str, reason: Option<&str>) -> Value {
    let mut value = json!({
        "evidence": crate::evidence_label(),
        "event": "bluetooth-attention",
        "status": status,
    });
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    value
}

/// Raises the `show-bluetooth-pairing` handler from a worker thread.
/// Bursts collapse into one request.
pub struct Attention {
    tx: Option<Sender<()>>,
}

impl Attention {
    /// No application service: every raise is recorded as `disabled`.
    pub fn disabled() -> Self {
        crate::emit(record("disabled", Some("no ferrix-appd socket")));
        Self { tx: None }
    }

    pub fn spawn(socket: PathBuf, timeout: Duration) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<()>();
        thread::Builder::new()
            .name("core-bluetooth-attention".to_owned())
            .spawn(move || {
                let mut link: Option<Client> = None;
                while rx.recv().is_ok() {
                    while rx.try_recv().is_ok() {}
                    let result = send(&mut link, &socket, timeout);
                    crate::emit(match &result {
                        Ok(()) => record("sent", None),
                        Err(reason) => record("failed", Some(reason)),
                    });
                }
            })
            .map_err(|e| format!("cannot start attention thread: {e}"))?;
        Ok(Self { tx: Some(tx) })
    }

    pub fn incoming_pairing(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(());
        }
    }
}

/// One attempt on the cached link, then one on a fresh link.
fn send(link: &mut Option<Client>, socket: &Path, timeout: Duration) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..2 {
        if link.is_none() {
            *link = Some(Client::connect(socket, ClientRole::Service, timeout)?);
        }
        let Some(client) = link.as_mut() else {
            break;
        };
        match client.open(&AppIntent::ShowBluetoothPairing) {
            Ok(_) => return Ok(()),
            // A typed refusal (no handler) is final; the link is fine.
            Err(Ok(error)) => return Err(error.to_string()),
            Err(Err(transport)) => {
                *link = None;
                last = transport;
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_hardware_api::PairingStage;

    #[test]
    fn only_a_new_incoming_prompt_raises() {
        let mut edge = IncomingEdge::default();
        let mut summary = BluetoothSessionSummary {
            pairing: Some(PairingStage::Authorize),
            ..BluetoothSessionSummary::default()
        };
        assert!(!edge.update(&summary), "our own prompt stays in Settings");
        summary.incoming = true;
        assert!(edge.update(&summary));
        assert!(!edge.update(&summary), "once per request");
        summary.pairing = Some(PairingStage::Pairing);
        assert!(!edge.update(&summary));
        summary.pairing = Some(PairingStage::RequestPin);
        assert!(edge.update(&summary), "a new prompt raises again");
    }
}
