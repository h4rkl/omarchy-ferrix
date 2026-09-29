//! WirePlumber adapter for Ferrix's target-neutral audio contract.
//!
//! WirePlumber remains authoritative for graph and policy decisions. This
//! crate maps its available/default sinks into the deliberately coarse Ferrix
//! route model and never opens ALSA, Bluetooth, or vendor devices directly.

use std::collections::BTreeMap;
#[cfg(any(feature = "production", test))]
use std::sync::Weak;
#[cfg(any(feature = "production", test))]
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
#[cfg(any(feature = "production", test))]
use std::thread;

use ferrix_hardware_api::{
    Audio, AudioRoute, AudioRouteAvailability, AudioState, Capability, EventSource, HardwareError,
    HardwareEvent,
};

#[cfg(feature = "production")]
mod wpctl;

#[derive(Clone, Debug, Eq, PartialEq)]
struct RouteEndpoint {
    id: String,
    route: AudioRoute,
    active: bool,
}

#[cfg(any(feature = "production", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransportEvent {
    Changed,
}

trait WirePlumberTransport: Send + Sync {
    fn endpoints(&self) -> Result<Vec<RouteEndpoint>, String>;
    fn set_default(&self, id: &str) -> Result<(), String>;
    #[cfg(any(feature = "production", test))]
    fn watch(&self, sender: Sender<TransportEvent>) -> Result<(), String>;
}

#[derive(Debug)]
struct State {
    audio: AudioState,
    endpoints: BTreeMap<AudioRoute, String>,
    subscribers: Vec<Sender<HardwareEvent>>,
}

/// WirePlumber-backed implementation of Ferrix's audio capability.
pub struct WirePlumberAudio {
    transport: Arc<dyn WirePlumberTransport>,
    state: Mutex<State>,
    commands: Mutex<()>,
}

impl WirePlumberAudio {
    /// Connect to the production `wpctl` transport and read its authoritative
    /// route snapshot.
    #[cfg(feature = "production")]
    pub fn connect() -> Result<Arc<Self>, HardwareError> {
        Self::from_transport(Arc::new(wpctl::WpctlTransport::new()?))
    }

    #[cfg(any(feature = "production", test))]
    fn from_transport(
        transport: Arc<dyn WirePlumberTransport>,
    ) -> Result<Arc<Self>, HardwareError> {
        let (audio, endpoints) = map_endpoints(transport.endpoints().map_err(failed)?)?;
        let adapter = Arc::new(Self {
            transport,
            state: Mutex::new(State {
                audio,
                endpoints,
                subscribers: Vec::new(),
            }),
            commands: Mutex::new(()),
        });
        adapter.start_watcher()?;
        Ok(adapter)
    }

    #[cfg(any(feature = "production", test))]
    fn start_watcher(self: &Arc<Self>) -> Result<(), HardwareError> {
        let (sender, receiver) = mpsc::channel();
        self.transport.watch(sender).map_err(failed)?;
        let weak = Arc::downgrade(self);
        thread::Builder::new()
            .name("ferrix-audio-wireplumber-state".to_owned())
            .spawn(move || watch_loop(weak, receiver))
            .map_err(|error| failed(error.to_string()))?;
        Ok(())
    }

    fn refresh(&self) -> Result<AudioState, HardwareError> {
        let (audio, endpoints) = map_endpoints(self.transport.endpoints().map_err(failed)?)?;
        let mut state = lock(&self.state);
        state.endpoints = endpoints;
        if state.audio != audio {
            state.audio = audio;
            let event = HardwareEvent::AudioChanged(audio);
            state
                .subscribers
                .retain(|subscriber| subscriber.send(event).is_ok());
        }
        Ok(audio)
    }
}

impl Audio for WirePlumberAudio {
    fn audio_state(&self) -> AudioState {
        lock(&self.state).audio
    }

    fn set_audio_route(&self, route: AudioRoute) -> Result<AudioState, HardwareError> {
        let _command = lock(&self.commands);
        let endpoint = lock(&self.state)
            .endpoints
            .get(&route)
            .cloned()
            .ok_or_else(|| {
                HardwareError::invalid(
                    Capability::Audio,
                    format!("{} route is not available", route.name()),
                )
            })?;
        self.transport.set_default(&endpoint).map_err(failed)?;
        let audio = self.refresh()?;
        if audio.route != route {
            return Err(failed(format!(
                "WirePlumber did not make {} the default sink",
                route.name()
            )));
        }
        Ok(audio)
    }
}

impl EventSource for WirePlumberAudio {
    fn subscribe(&self, sender: Sender<HardwareEvent>) {
        lock(&self.state).subscribers.push(sender);
    }
}

#[cfg(any(feature = "production", test))]
fn watch_loop(adapter: Weak<WirePlumberAudio>, receiver: mpsc::Receiver<TransportEvent>) {
    while receiver.recv().is_ok() {
        let Some(adapter) = adapter.upgrade() else {
            break;
        };
        let _ = adapter.refresh();
    }
}

fn map_endpoints(
    endpoints: Vec<RouteEndpoint>,
) -> Result<(AudioState, BTreeMap<AudioRoute, String>), HardwareError> {
    let mut available = AudioRouteAvailability::default();
    let mut representatives = BTreeMap::new();
    let mut active = None;
    for endpoint in endpoints {
        available.insert(endpoint.route);
        representatives
            .entry(endpoint.route)
            .or_insert_with(|| endpoint.id.clone());
        if endpoint.active && active.replace(endpoint.route).is_some() {
            return Err(failed("WirePlumber reported multiple default sinks"));
        }
    }
    let route = active.ok_or_else(|| failed("WirePlumber exposes no classified default sink"))?;
    Ok((AudioState { route, available }, representatives))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn failed(reason: impl Into<String>) -> HardwareError {
    HardwareError::failed(Capability::Audio, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    struct MockTransport {
        endpoints: Mutex<Result<Vec<RouteEndpoint>, String>>,
        watchers: Mutex<Vec<Sender<TransportEvent>>>,
        writes: Mutex<Vec<String>>,
    }

    impl MockTransport {
        fn new(endpoints: Vec<RouteEndpoint>) -> Self {
            Self {
                endpoints: Mutex::new(Ok(endpoints)),
                watchers: Mutex::new(Vec::new()),
                writes: Mutex::new(Vec::new()),
            }
        }

        fn replace(&self, endpoints: Vec<RouteEndpoint>) {
            *lock(&self.endpoints) = Ok(endpoints);
        }

        fn signal(&self) {
            lock(&self.watchers).retain(|sender| sender.send(TransportEvent::Changed).is_ok());
        }
    }

    impl WirePlumberTransport for MockTransport {
        fn endpoints(&self) -> Result<Vec<RouteEndpoint>, String> {
            lock(&self.endpoints).clone()
        }

        fn set_default(&self, id: &str) -> Result<(), String> {
            lock(&self.writes).push(id.to_owned());
            let mut endpoints = lock(&self.endpoints);
            for endpoint in endpoints.as_mut().map_err(|error| error.clone())? {
                endpoint.active = endpoint.id == id;
            }
            Ok(())
        }

        fn watch(&self, sender: Sender<TransportEvent>) -> Result<(), String> {
            lock(&self.watchers).push(sender);
            Ok(())
        }
    }

    fn endpoint(id: &str, route: AudioRoute, active: bool) -> RouteEndpoint {
        RouteEndpoint {
            id: id.to_owned(),
            route,
            active,
        }
    }

    fn subscribed(adapter: &WirePlumberAudio) -> Receiver<HardwareEvent> {
        let (sender, receiver) = mpsc::channel();
        adapter.subscribe(sender);
        receiver
    }

    #[test]
    fn maps_active_and_available_routes() {
        let transport = Arc::new(MockTransport::new(vec![
            endpoint("speaker", AudioRoute::Speaker, true),
            endpoint("headset", AudioRoute::Headphones, false),
        ]));
        let adapter = WirePlumberAudio::from_transport(transport).unwrap();
        let state = adapter.audio_state();
        assert_eq!(state.route, AudioRoute::Speaker);
        assert!(state.available.speaker);
        assert!(state.available.headphones);
        assert!(!state.available.bluetooth);
    }

    #[test]
    fn set_route_writes_representative_and_authoritatively_rereads() {
        let transport = Arc::new(MockTransport::new(vec![
            endpoint("sink-9", AudioRoute::Speaker, true),
            endpoint("sink-2", AudioRoute::Earpiece, false),
        ]));
        let adapter = WirePlumberAudio::from_transport(transport.clone()).unwrap();
        let events = subscribed(&adapter);
        let state = adapter.set_audio_route(AudioRoute::Earpiece).unwrap();
        assert_eq!(state.route, AudioRoute::Earpiece);
        assert_eq!(*lock(&transport.writes), vec!["sink-2"]);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HardwareEvent::AudioChanged(state)
        );
    }

    #[test]
    fn unavailable_route_is_a_typed_invalid_audio_value() {
        let transport = Arc::new(MockTransport::new(vec![endpoint(
            "speaker",
            AudioRoute::Speaker,
            true,
        )]));
        let adapter = WirePlumberAudio::from_transport(transport).unwrap();
        assert_eq!(
            adapter.set_audio_route(AudioRoute::Bluetooth),
            Err(HardwareError::invalid(
                Capability::Audio,
                "bluetooth route is not available"
            ))
        );
    }

    #[test]
    fn transport_changes_emit_only_state_deltas() {
        let transport = Arc::new(MockTransport::new(vec![endpoint(
            "speaker",
            AudioRoute::Speaker,
            true,
        )]));
        let adapter = WirePlumberAudio::from_transport(transport.clone()).unwrap();
        let events = subscribed(&adapter);
        transport.signal();
        assert!(events.recv_timeout(Duration::from_millis(50)).is_err());

        transport.replace(vec![
            endpoint("speaker", AudioRoute::Speaker, true),
            endpoint("bt", AudioRoute::Bluetooth, false),
        ]);
        transport.signal();
        let HardwareEvent::AudioChanged(state) =
            events.recv_timeout(Duration::from_secs(1)).unwrap()
        else {
            panic!("expected audio event");
        };
        assert!(state.available.bluetooth);
    }

    #[test]
    fn missing_or_ambiguous_default_is_a_typed_failure() {
        for endpoints in [
            vec![endpoint("speaker", AudioRoute::Speaker, false)],
            vec![
                endpoint("speaker", AudioRoute::Speaker, true),
                endpoint("bt", AudioRoute::Bluetooth, true),
            ],
        ] {
            assert!(matches!(
                WirePlumberAudio::from_transport(Arc::new(MockTransport::new(endpoints))),
                Err(HardwareError::Failed {
                    capability: Capability::Audio,
                    ..
                })
            ));
        }
    }
}
