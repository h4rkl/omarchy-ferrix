//! WirePlumber adapter for Ferrix's target-neutral audio contract.
//!
//! WirePlumber remains authoritative for graph and policy decisions. This
//! crate maps its available/default sinks into the deliberately coarse Ferrix
//! route model, reports and sets the default sink's volume and mute through
//! WirePlumber's mixer, and never opens ALSA, Bluetooth, or vendor devices
//! directly.

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
    Audio, AudioOutputLevel, AudioRoute, AudioRouteAvailability, AudioState, Capability,
    EventSource, HardwareError, HardwareEvent,
};

#[cfg(feature = "production")]
mod wpctl;

#[derive(Clone, Debug, Eq, PartialEq)]
struct RouteEndpoint {
    id: String,
    route: AudioRoute,
    active: bool,
    /// WirePlumber's mixer level; transports read it for the default sink
    /// only, and `None` means the node exposes no volume.
    level: Option<AudioOutputLevel>,
}

#[cfg(any(feature = "production", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransportEvent {
    Changed,
}

trait WirePlumberTransport: Send + Sync {
    fn endpoints(&self) -> Result<Vec<RouteEndpoint>, String>;
    fn set_default(&self, id: &str) -> Result<(), String>;
    /// Sets the node's mixer volume, `0..=100` on WirePlumber's cubic
    /// (user-facing) scale, never above 100%.
    fn set_volume(&self, id: &str, percent: u8) -> Result<(), String>;
    fn set_muted(&self, id: &str, muted: bool) -> Result<(), String>;
    #[cfg(any(feature = "production", test))]
    fn watch(&self, sender: Sender<TransportEvent>) -> Result<(), String>;
}

#[derive(Debug)]
struct State {
    audio: AudioState,
    endpoints: BTreeMap<AudioRoute, String>,
    /// The node behind `audio.route`, which volume and mute act on.
    default_sink: String,
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
        let snapshot = map_endpoints(transport.endpoints().map_err(failed)?)?;
        let adapter = Arc::new(Self {
            transport,
            state: Mutex::new(State {
                audio: snapshot.audio,
                endpoints: snapshot.endpoints,
                default_sink: snapshot.default_sink,
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
        let snapshot = map_endpoints(self.transport.endpoints().map_err(failed)?)?;
        let audio = snapshot.audio;
        let mut state = lock(&self.state);
        state.endpoints = snapshot.endpoints;
        state.default_sink = snapshot.default_sink;
        if state.audio != audio {
            state.audio = audio;
            let event = HardwareEvent::AudioChanged(audio);
            state
                .subscribers
                .retain(|subscriber| subscriber.send(event).is_ok());
        }
        Ok(audio)
    }

    /// The default sink to apply a level change to, refusing with
    /// `unsupported` when WirePlumber exposes no mixer level for it.
    fn level_target(&self) -> Result<String, HardwareError> {
        let state = lock(&self.state);
        if state.audio.output.is_none() {
            return Err(HardwareError::unsupported(Capability::Audio));
        }
        Ok(state.default_sink.clone())
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

    fn set_audio_volume(&self, percent: u8) -> Result<AudioState, HardwareError> {
        if percent > 100 {
            return Err(HardwareError::invalid(
                Capability::Audio,
                format!("volume {percent} is outside 0..=100"),
            ));
        }
        let _command = lock(&self.commands);
        let sink = self.level_target()?;
        self.transport.set_volume(&sink, percent).map_err(failed)?;
        let audio = self.refresh()?;
        // wpctl reports two decimals of the cubic volume; allow one step.
        match audio.output {
            Some(level) if level.volume_percent.abs_diff(percent) <= 1 => Ok(audio),
            _ => Err(failed(format!(
                "WirePlumber did not apply volume {percent}% to the default sink"
            ))),
        }
    }

    fn set_audio_muted(&self, muted: bool) -> Result<AudioState, HardwareError> {
        let _command = lock(&self.commands);
        let sink = self.level_target()?;
        self.transport.set_muted(&sink, muted).map_err(failed)?;
        let audio = self.refresh()?;
        match audio.output {
            Some(level) if level.muted == muted => Ok(audio),
            _ => Err(failed(format!(
                "WirePlumber did not {} the default sink",
                if muted { "mute" } else { "unmute" }
            ))),
        }
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

struct Snapshot {
    audio: AudioState,
    endpoints: BTreeMap<AudioRoute, String>,
    default_sink: String,
}

fn map_endpoints(endpoints: Vec<RouteEndpoint>) -> Result<Snapshot, HardwareError> {
    let mut available = AudioRouteAvailability::default();
    let mut representatives = BTreeMap::new();
    let mut active: Option<RouteEndpoint> = None;
    for endpoint in endpoints {
        available.insert(endpoint.route);
        representatives
            .entry(endpoint.route)
            .or_insert_with(|| endpoint.id.clone());
        if endpoint.active && active.replace(endpoint).is_some() {
            return Err(failed("WirePlumber reported multiple default sinks"));
        }
    }
    let active = active.ok_or_else(|| failed("WirePlumber exposes no classified default sink"))?;
    // The default sink is the route's representative, so a route change
    // targets the node that is actually default.
    representatives.insert(active.route, active.id.clone());
    Ok(Snapshot {
        audio: AudioState {
            route: active.route,
            available,
            output: active.level,
        },
        endpoints: representatives,
        default_sink: active.id,
    })
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
        /// Accept mixer writes without applying them (a policy override).
        ignore_level_writes: Mutex<bool>,
    }

    impl MockTransport {
        fn new(endpoints: Vec<RouteEndpoint>) -> Self {
            Self {
                endpoints: Mutex::new(Ok(endpoints)),
                watchers: Mutex::new(Vec::new()),
                writes: Mutex::new(Vec::new()),
                ignore_level_writes: Mutex::new(false),
            }
        }

        fn replace(&self, endpoints: Vec<RouteEndpoint>) {
            *lock(&self.endpoints) = Ok(endpoints);
        }

        fn signal(&self) {
            lock(&self.watchers).retain(|sender| sender.send(TransportEvent::Changed).is_ok());
        }

        /// Applies a mixer write unless the mock was told to ignore them.
        fn change_level(
            &self,
            id: &str,
            change: impl FnOnce(&mut AudioOutputLevel),
        ) -> Result<(), String> {
            if *lock(&self.ignore_level_writes) {
                return Ok(());
            }
            let mut endpoints = lock(&self.endpoints);
            let endpoint = endpoints
                .as_mut()
                .map_err(|error| error.clone())?
                .iter_mut()
                .find(|endpoint| endpoint.id == id)
                .ok_or_else(|| format!("no node {id}"))?;
            change(endpoint.level.as_mut().ok_or("node has no volume")?);
            Ok(())
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

        fn set_volume(&self, id: &str, percent: u8) -> Result<(), String> {
            lock(&self.writes).push(format!("volume {id} {percent}"));
            self.change_level(id, |level| level.volume_percent = percent)
        }

        fn set_muted(&self, id: &str, muted: bool) -> Result<(), String> {
            lock(&self.writes).push(format!("mute {id} {muted}"));
            self.change_level(id, |level| level.muted = muted)
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
            level: None,
        }
    }

    fn level(volume_percent: u8, muted: bool) -> Option<AudioOutputLevel> {
        Some(AudioOutputLevel {
            volume_percent,
            muted,
        })
    }

    /// An endpoint whose node exposes a WirePlumber mixer level.
    fn leveled(id: &str, route: AudioRoute, active: bool, volume: u8) -> RouteEndpoint {
        RouteEndpoint {
            level: level(volume, false),
            ..endpoint(id, route, active)
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

    #[test]
    fn default_sink_level_is_reported_and_set_through_the_transport() {
        let transport = Arc::new(MockTransport::new(vec![
            leveled("51", AudioRoute::Speaker, true, 60),
            leveled("52", AudioRoute::Headphones, false, 20),
        ]));
        let adapter = WirePlumberAudio::from_transport(transport.clone()).unwrap();
        assert_eq!(adapter.audio_state().output, level(60, false));
        let events = subscribed(&adapter);

        let state = adapter.set_audio_volume(35).unwrap();
        assert_eq!(state.output, level(35, false));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HardwareEvent::AudioChanged(state)
        );
        let state = adapter.set_audio_muted(true).unwrap();
        assert_eq!(state.output, level(35, true));
        assert_eq!(state.route, AudioRoute::Speaker);
        assert_eq!(
            *lock(&transport.writes),
            vec!["volume 51 35", "mute 51 true"],
            "only the default sink is written"
        );

        // The level follows the default sink when the route changes.
        let state = adapter.set_audio_route(AudioRoute::Headphones).unwrap();
        assert_eq!(state.output, level(20, false));
        adapter.set_audio_volume(80).unwrap();
        assert_eq!(lock(&transport.writes).last().unwrap(), "volume 52 80");
    }

    #[test]
    fn level_changes_without_a_mixer_or_out_of_range_are_typed_refusals() {
        let transport = Arc::new(MockTransport::new(vec![endpoint(
            "51",
            AudioRoute::Speaker,
            true,
        )]));
        let adapter = WirePlumberAudio::from_transport(transport.clone()).unwrap();
        assert_eq!(adapter.audio_state().output, None);
        assert_eq!(
            adapter.set_audio_volume(40),
            Err(HardwareError::unsupported(Capability::Audio))
        );
        assert_eq!(
            adapter.set_audio_muted(true),
            Err(HardwareError::unsupported(Capability::Audio))
        );
        assert!(matches!(
            adapter.set_audio_volume(101),
            Err(HardwareError::InvalidValue {
                capability: Capability::Audio,
                ..
            })
        ));
        assert!(lock(&transport.writes).is_empty(), "nothing was written");
    }

    #[test]
    fn level_writes_wireplumber_did_not_apply_fail_with_the_true_state_kept() {
        let transport = Arc::new(MockTransport::new(vec![leveled(
            "51",
            AudioRoute::Speaker,
            true,
            60,
        )]));
        *lock(&transport.ignore_level_writes) = true;
        let adapter = WirePlumberAudio::from_transport(transport).unwrap();
        assert!(matches!(
            adapter.set_audio_volume(10),
            Err(HardwareError::Failed { .. })
        ));
        assert!(matches!(
            adapter.set_audio_muted(true),
            Err(HardwareError::Failed { .. })
        ));
        assert_eq!(adapter.audio_state().output, level(60, false));
    }

    #[test]
    fn external_level_changes_emit_audio_events() {
        let transport = Arc::new(MockTransport::new(vec![leveled(
            "51",
            AudioRoute::Speaker,
            true,
            60,
        )]));
        let adapter = WirePlumberAudio::from_transport(transport.clone()).unwrap();
        let events = subscribed(&adapter);
        transport.replace(vec![RouteEndpoint {
            level: level(60, true),
            ..endpoint("51", AudioRoute::Speaker, true)
        }]);
        transport.signal();
        let HardwareEvent::AudioChanged(state) =
            events.recv_timeout(Duration::from_secs(1)).unwrap()
        else {
            panic!("expected audio event");
        };
        assert_eq!(state.output, level(60, true));
    }

    #[test]
    fn route_change_targets_the_active_sink_of_a_route() {
        let transport = Arc::new(MockTransport::new(vec![
            endpoint("40", AudioRoute::Speaker, false),
            endpoint("41", AudioRoute::Speaker, true),
            endpoint("42", AudioRoute::Headphones, false),
        ]));
        let adapter = WirePlumberAudio::from_transport(transport.clone()).unwrap();
        adapter.set_audio_route(AudioRoute::Speaker).unwrap();
        assert_eq!(*lock(&transport.writes), vec!["41"]);
    }
}
