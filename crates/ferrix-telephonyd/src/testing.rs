//! Test doubles compiled into the library so `tests/*.rs` can use them: a
//! scripted backend whose operations record what was asked and succeed (or
//! fail with a configured error), plus an in-memory subscriber sink. Nothing
//! here touches the operating system or names a real adapter.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use ferrix_telephony_adapter_api::{
    BackendCall, BackendError, BackendEvent, BackendSnapshot, CallHandle, ModemBackend, SmsHandle,
};
use ferrix_telephony_api::{
    AccessTechnology, AudioRoute, AudioRouteState, Capabilities, DataProfile, Dtmf, E164,
    ModemState, Operator, RadioPower, Registration, Signal, SimState, TelephonyFeature,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Inner {
    snapshot: BackendSnapshot,
    ops: Vec<String>,
    fail_next: Option<BackendError>,
    /// When set, `set_audio_route` reports the change through the event
    /// channel like a real backend does.
    emit_audio: bool,
    sinks: Vec<Sender<BackendEvent>>,
    next_call: AtomicU32,
    next_sms: AtomicU32,
}

/// A backend that accepts every operation and records it. Tests drive state
/// through `Service::handle_backend_event` directly.
#[derive(Clone)]
pub struct FakeBackend(Arc<Mutex<Inner>>);

impl Default for FakeBackend {
    fn default() -> Self {
        Self::ready()
    }
}

impl FakeBackend {
    /// Registered on `Ferrix Mock`, every feature except data.
    pub fn ready() -> Self {
        let mut snapshot = BackendSnapshot {
            modem: ModemState::Ready,
            sim: SimState::Ready,
            registration: Registration::Registered { roaming: false },
            operator: Some(Operator {
                name: "Ferrix Mock".to_owned(),
                mcc_mnc: Some("001-01".to_owned()),
            }),
            access: AccessTechnology::Lte,
            signal: Signal::bars(3),
            radio: RadioPower::On,
            audio: AudioRouteState {
                active: AudioRoute::Earpiece,
                available: [AudioRoute::Earpiece, AudioRoute::Speaker].into(),
            },
            ..BackendSnapshot::default()
        };
        snapshot.capabilities = [
            TelephonyFeature::Voice,
            TelephonyFeature::Sms,
            TelephonyFeature::MultipartSms,
            TelephonyFeature::DeliveryReports,
            TelephonyFeature::Dtmf,
            TelephonyFeature::Hold,
            TelephonyFeature::AudioRoute,
            TelephonyFeature::SimPin,
            TelephonyFeature::RadioPower,
        ]
        .into_iter()
        .collect();
        Self::with_snapshot(snapshot)
    }

    pub fn with_snapshot(snapshot: BackendSnapshot) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            snapshot,
            ops: Vec::new(),
            fail_next: None,
            emit_audio: false,
            sinks: Vec::new(),
            next_call: AtomicU32::new(1),
            next_sms: AtomicU32::new(1),
        })))
    }

    /// Replaces the snapshot the service reads on `BackendRestarted`.
    pub fn set_snapshot(&self, snapshot: BackendSnapshot) {
        lock(&self.0).snapshot = snapshot;
    }

    pub fn set_capabilities(&self, capabilities: Capabilities) {
        lock(&self.0).snapshot.capabilities = capabilities;
    }

    /// Adds a call to the snapshot (for restart reconciliation tests).
    pub fn push_call(&self, call: BackendCall) {
        lock(&self.0).snapshot.calls.push(call);
    }

    /// Make `set_audio_route` emit an `Audio` event asynchronously.
    pub fn emit_audio_changes(&self) {
        lock(&self.0).emit_audio = true;
    }

    /// The next operation fails with `error`.
    pub fn fail_next(&self, error: BackendError) {
        lock(&self.0).fail_next = Some(error);
    }

    /// Operation names in order (`dial`, `answer c1`, ...).
    pub fn ops(&self) -> Vec<String> {
        lock(&self.0).ops.clone()
    }

    /// Sends an event to every subscriber the service registered.
    pub fn emit(&self, event: BackendEvent) {
        lock(&self.0)
            .sinks
            .retain(|s| s.send(event.clone()).is_ok());
    }

    fn op(&self, name: String) -> Result<(), BackendError> {
        let mut inner = lock(&self.0);
        inner.ops.push(name);
        match inner.fail_next.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl ModemBackend for FakeBackend {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> Capabilities {
        lock(&self.0).snapshot.capabilities.clone()
    }

    fn snapshot(&self) -> BackendSnapshot {
        lock(&self.0).snapshot.clone()
    }

    fn subscribe(&self, sink: Sender<BackendEvent>) {
        lock(&self.0).sinks.push(sink);
    }

    fn dial(&self, number: &E164) -> Result<CallHandle, BackendError> {
        self.op(format!("dial {}", number.redacted()))?;
        let n = lock(&self.0).next_call.fetch_add(1, Ordering::SeqCst);
        Ok(CallHandle(format!("h{n}")))
    }

    fn answer(&self, call: &CallHandle) -> Result<(), BackendError> {
        self.op(format!("answer {call}"))
    }

    fn reject(&self, call: &CallHandle) -> Result<(), BackendError> {
        self.op(format!("reject {call}"))
    }

    fn hangup(&self, call: &CallHandle) -> Result<(), BackendError> {
        self.op(format!("hangup {call}"))
    }

    fn hold(&self, call: &CallHandle) -> Result<(), BackendError> {
        self.op(format!("hold {call}"))
    }

    fn resume(&self, call: &CallHandle) -> Result<(), BackendError> {
        self.op(format!("resume {call}"))
    }

    fn dtmf(&self, call: &CallHandle, digit: Dtmf) -> Result<(), BackendError> {
        self.op(format!("dtmf {call} {}", digit.as_char()))
    }

    fn send_sms(&self, to: &E164, _body: &str) -> Result<SmsHandle, BackendError> {
        self.op(format!("send-sms {}", to.redacted()))?;
        let n = lock(&self.0).next_sms.fetch_add(1, Ordering::SeqCst);
        Ok(SmsHandle(format!("s{n}")))
    }

    fn connect_data(&self, profile: &DataProfile) -> Result<(), BackendError> {
        self.op(format!(
            "connect-data {}",
            profile.apn.as_deref().unwrap_or("default")
        ))
    }

    fn disconnect_data(&self) -> Result<(), BackendError> {
        self.op("disconnect-data".to_owned())
    }

    fn set_radio_power(&self, power: RadioPower) -> Result<(), BackendError> {
        self.op(format!("set-radio-power {}", power.name()))
    }

    fn enter_pin(&self, _pin: &str) -> Result<(), BackendError> {
        self.op("enter-pin".to_owned())
    }

    fn set_audio_route(&self, route: AudioRoute) -> Result<(), BackendError> {
        self.op(format!("set-audio-route {}", route.name()))?;
        let audio = {
            let mut inner = lock(&self.0);
            if !inner.emit_audio {
                return Ok(());
            }
            inner.snapshot.audio.active = route;
            inner.snapshot.audio.clone()
        };
        self.emit(BackendEvent::Audio(audio));
        Ok(())
    }
}

/// A subscriber sink that keeps every line written to it.
#[derive(Clone, Default)]
pub struct MemorySink(Arc<Mutex<Vec<String>>>);

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lines written so far (each a JSON document).
    pub fn lines(&self) -> Vec<String> {
        lock(&self.0).clone()
    }

    /// Parsed lines.
    pub fn values(&self) -> Vec<serde_json::Value> {
        self.lines()
            .iter()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

impl Write for MemorySink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let mut lines = lock(&self.0);
        for line in text.split_inclusive('\n') {
            if let Some(last) = lines.last_mut()
                && !last.ends_with('\n')
            {
                last.push_str(line);
            } else {
                lines.push(line.to_owned());
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Convenience for tests: a fresh data directory under the temp dir.
pub fn temp_data_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ferrix-telephonyd-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Ops keyed by name, for assertions on backend interaction counts.
pub fn count_ops(ops: &[String]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for op in ops {
        let name = op.split(' ').next().unwrap_or(op).to_owned();
        *map.entry(name).or_insert(0) += 1;
    }
    map
}
