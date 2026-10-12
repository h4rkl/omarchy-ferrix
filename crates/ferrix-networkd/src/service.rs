//! The networking service: owns the public `NetworkState`, folds backend
//! events into it, fans deltas out to subscribers, and executes authorized
//! control requests against the backend.
//!
//! The backend is a `ferrix_network_adapter_api::NetworkBackend` trait
//! object composed by the binary; the library never names one. Backend
//! events arrive on a channel and are applied by one pump thread, so state,
//! record, and fan-out stay ordered.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use ferrix_network_adapter_api::{BackendError, BackendEvent, NetworkBackend};
use ferrix_network_api::{NetworkError, NetworkEvent, NetworkState, RadioState};
use ferrix_network_protocol::{Reply, Request};

use crate::identity::{CAP_CONTROL, CAP_STATE, Identity, PeerIdentity};
use crate::records::Recorder;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A subscribed stream.
struct Subscriber {
    peer_pid: Option<u32>,
    reauthorize: bool,
    sink: Box<dyn Write + Send>,
}

pub struct Service {
    backend: Arc<dyn NetworkBackend>,
    identity: Arc<dyn Identity>,
    recorder: Recorder,
    state: Mutex<NetworkState>,
    subscribers: Mutex<Vec<Subscriber>>,
    barrier_seq: AtomicU64,
    barriers: Mutex<BTreeSet<u64>>,
    barrier_reached: Condvar,
}

impl Service {
    /// Takes the first snapshot from the backend. A backend that cannot
    /// answer is a startup failure, never a silent empty state.
    pub fn new(
        backend: Arc<dyn NetworkBackend>,
        identity: Arc<dyn Identity>,
        recorder: Recorder,
    ) -> Result<Arc<Self>, NetworkError> {
        let state = backend.snapshot().map_err(NetworkError::from)?;
        Ok(Arc::new(Self {
            backend,
            identity,
            recorder,
            state: Mutex::new(state),
            subscribers: Mutex::new(Vec::new()),
            barrier_seq: AtomicU64::new(0),
            barriers: Mutex::new(BTreeSet::new()),
            barrier_reached: Condvar::new(),
        }))
    }

    /// Subscribes to the backend and applies its events on a pump thread.
    pub fn start(self: &Arc<Self>) {
        let (tx, rx) = mpsc::channel();
        self.backend.subscribe(tx);
        let service = Arc::clone(self);
        thread::Builder::new()
            .name("ferrix-networkd-backend".to_owned())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    service.handle_backend_event(event);
                }
            })
            .expect("spawn backend pump");
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    pub fn evidence(&self) -> &str {
        self.recorder.evidence()
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn identity(&self) -> &Arc<dyn Identity> {
        &self.identity
    }

    pub fn state(&self) -> NetworkState {
        lock(&self.state).clone()
    }

    /// Folds one backend event: delta, loss, or restart snapshot.
    pub fn handle_backend_event(&self, event: BackendEvent) {
        match event {
            BackendEvent::Changed(event) => self.apply(event),
            BackendEvent::Lost { reason } => {
                self.recorder.backend_lost(&reason);
                self.apply(NetworkEvent::RadioChanged {
                    radio: RadioState::Unavailable {
                        reason: format!("backend lost: {reason}"),
                    },
                });
            }
            BackendEvent::Restarted { state } => {
                self.apply(NetworkEvent::ServiceRestarted { state });
            }
            BackendEvent::Barrier { token } => {
                lock(&self.barriers).insert(token);
                self.barrier_reached.notify_all();
            }
        }
    }

    /// Waits until the service has folded every backend event emitted before
    /// this call. Used only to make mock-control acknowledgements exact.
    pub fn settle(&self, timeout: Duration) -> bool {
        let token = self.barrier_seq.fetch_add(1, Ordering::SeqCst) + 1;
        if self.backend.barrier(token).is_err() {
            return false;
        }
        let deadline = Instant::now() + timeout;
        let mut reached = lock(&self.barriers);
        loop {
            if reached.remove(&token) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (guard, _) = self
                .barrier_reached
                .wait_timeout(reached, deadline - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            reached = guard;
        }
    }

    /// Applies a delta to the owned state, records it, and publishes it.
    /// Fan-out happens under the state lock so subscribers see deltas in
    /// exactly the order they were applied.
    fn apply(&self, event: NetworkEvent) {
        let mut state = lock(&self.state);
        event.apply_to(&mut state);
        self.recorder.network_event(&event);
        self.publish(&event);
    }

    /// Registers a subscriber and writes the snapshot on its own stream
    /// while holding the fan-out lock, so no delta precedes the snapshot.
    pub fn subscribe(&self, sink: Box<dyn Write + Send>) -> io::Result<()> {
        self.subscribe_inner(None, false, sink)
    }

    /// Production subscription: re-resolve the kernel peer before every
    /// event so revocation or an appd restart closes the stream fail-closed.
    pub fn subscribe_authorized(
        &self,
        peer_pid: Option<u32>,
        sink: Box<dyn Write + Send>,
    ) -> io::Result<()> {
        self.subscribe_inner(peer_pid, true, sink)
    }

    fn subscribe_inner(
        &self,
        peer_pid: Option<u32>,
        authorize: bool,
        mut sink: Box<dyn Write + Send>,
    ) -> io::Result<()> {
        if authorize && !self.identity.identify(peer_pid).has(CAP_STATE) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "network-state grant was revoked",
            ));
        }
        let state = lock(&self.state);
        let mut subscribers = lock(&self.subscribers);
        write_line(
            &mut *sink,
            &Reply::State {
                state: state.clone(),
            },
        )?;
        subscribers.push(Subscriber {
            peer_pid,
            reauthorize: authorize,
            sink,
        });
        Ok(())
    }

    pub fn subscriber_count(&self) -> usize {
        lock(&self.subscribers).len()
    }

    fn publish(&self, event: &NetworkEvent) {
        let mut subscribers = lock(&self.subscribers);
        subscribers.retain_mut(|subscriber| {
            (!subscriber.reauthorize || self.identity.identify(subscriber.peer_pid).has(CAP_STATE))
                && write_line(&mut *subscriber.sink, event).is_ok()
        });
    }

    /// Resolves a peer through the identity source.
    pub fn identify(&self, pid: Option<u32>) -> PeerIdentity {
        self.identity.identify(pid)
    }

    /// The hello reply: the snapshot rides along only for `network-state`.
    pub fn hello_reply(&self, identity: &PeerIdentity) -> Reply {
        Reply::Hello {
            schema_version: ferrix_network_api::SCHEMA_VERSION,
            evidence: self.evidence().to_owned(),
            state: identity.has(CAP_STATE).then(|| self.state()),
        }
    }

    /// Capability a request needs; `hello` needs none.
    pub fn required_capability(request: &Request) -> Option<&'static str> {
        match request {
            Request::Hello { .. } => None,
            Request::State {} | Request::Subscribe {} => Some(CAP_STATE),
            _ => Some(CAP_CONTROL),
        }
    }

    /// Executes an already-authorized control request. State changes arrive
    /// through the backend event channel, not through the reply.
    pub fn control(&self, request: &Request) -> Result<(), NetworkError> {
        let result = match request {
            Request::SetRadio { enabled } => self.backend.set_radio(*enabled),
            Request::Scan {} => self.backend.scan(),
            Request::Connect { ap, passphrase } => self
                .backend
                .connect(ap, passphrase.as_ref().map(|p| p.expose())),
            Request::JoinHidden {
                ssid,
                security,
                passphrase,
            } => ferrix_network_api::validate_hidden_join(ssid, *security, passphrase.as_ref())
                .map_err(BackendError::from)
                .and_then(|()| {
                    self.backend.join_hidden(
                        ssid,
                        *security,
                        passphrase.as_ref().map(|p| p.expose()),
                    )
                }),
            Request::ConnectProfile { profile } => self.backend.connect_profile(profile),
            Request::Disconnect {} => self.backend.disconnect(),
            Request::Forget { profile } => self.backend.forget(profile),
            Request::Hello { .. } | Request::State {} | Request::Subscribe {} => {
                return Err(NetworkError::Protocol {
                    reason: format!("{} is not a control request", request.name()),
                });
            }
        };
        result.map_err(NetworkError::from)
    }
}

pub(crate) fn write_line(sink: &mut dyn Write, value: &impl serde::Serialize) -> io::Result<()> {
    let mut text = serde_json::to_string(value).map_err(io::Error::other)?;
    text.push('\n');
    sink.write_all(text.as_bytes())?;
    sink.flush()
}
