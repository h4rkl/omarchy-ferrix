//! Test doubles compiled into the library so `tests/*.rs` can use them: a
//! scripted backend that records calls and lets a test inject events, and an
//! in-memory subscriber sink. Nothing here names a real backend.

use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use ferrix_network_adapter_api::{BackendError, BackendEvent, NetworkBackend};
use ferrix_network_api::{ApId, NetworkState, ProfileId};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One backend call, by name, with the ids it received. Passphrases are
/// recorded only as "present"/"absent".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub op: &'static str,
    pub id: Option<String>,
    pub passphrase_present: bool,
}

struct FakeInner {
    state: NetworkState,
    calls: Vec<Call>,
    fail_next: Option<BackendError>,
    sinks: Vec<Sender<BackendEvent>>,
}

/// A backend whose behavior the test scripts. Clone handles share state.
#[derive(Clone)]
pub struct FakeBackend(Arc<Mutex<FakeInner>>);

impl FakeBackend {
    pub fn new(state: NetworkState) -> Self {
        Self(Arc::new(Mutex::new(FakeInner {
            state,
            calls: Vec::new(),
            fail_next: None,
            sinks: Vec::new(),
        })))
    }

    /// The next control call fails with `error`.
    pub fn fail_next(&self, error: BackendError) {
        lock(&self.0).fail_next = Some(error);
    }

    pub fn calls(&self) -> Vec<Call> {
        lock(&self.0).calls.clone()
    }

    /// Pushes an event to every subscribed pump.
    pub fn push(&self, event: BackendEvent) {
        let mut inner = lock(&self.0);
        inner.sinks.retain(|sink| sink.send(event.clone()).is_ok());
    }

    fn record(
        &self,
        op: &'static str,
        id: Option<&str>,
        passphrase_present: bool,
    ) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        inner.calls.push(Call {
            op,
            id: id.map(str::to_owned),
            passphrase_present,
        });
        match inner.fail_next.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl NetworkBackend for FakeBackend {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn snapshot(&self) -> Result<NetworkState, BackendError> {
        Ok(lock(&self.0).state.clone())
    }

    fn set_radio(&self, enabled: bool) -> Result<(), BackendError> {
        self.record(
            if enabled {
                "set-radio:on"
            } else {
                "set-radio:off"
            },
            None,
            false,
        )
    }

    fn scan(&self) -> Result<(), BackendError> {
        self.record("scan", None, false)
    }

    fn connect(&self, ap: &ApId, passphrase: Option<&str>) -> Result<(), BackendError> {
        self.record("connect", Some(ap.as_str()), passphrase.is_some())
    }

    fn connect_profile(&self, profile: &ProfileId) -> Result<(), BackendError> {
        self.record("connect-profile", Some(profile.as_str()), false)
    }

    fn disconnect(&self) -> Result<(), BackendError> {
        self.record("disconnect", None, false)
    }

    fn forget(&self, profile: &ProfileId) -> Result<(), BackendError> {
        self.record("forget", Some(profile.as_str()), false)
    }

    fn subscribe(&self, sink: Sender<BackendEvent>) {
        lock(&self.0).sinks.push(sink);
    }

    fn barrier(&self, token: u64) -> Result<(), BackendError> {
        self.push(BackendEvent::Barrier { token });
        Ok(())
    }
}

/// A `Write` that forwards complete lines to a channel.
pub struct ChannelSink {
    tx: Sender<String>,
    buffer: Vec<u8>,
}

impl ChannelSink {
    pub fn new() -> (Self, Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx,
                buffer: Vec::new(),
            },
            rx,
        )
    }
}

impl Write for ChannelSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        while let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            if self.tx.send(text).is_err() {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "receiver gone"));
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
