# ADR-001: Telephony service boundary

Status: accepted by the lead 2026-09-21, subordinate to `plans/baseline.md`.

## Decision

Calls, SMS, cellular data bearers, SIM PIN handling, and modem lifecycle are
owned by a **separate privileged service, `ferrix-telephonyd`**, with its own
typed public contract (`ferrix-telephony-api`, `ferrix-telephony-protocol`).
They are **not** added to the `ferrix-hardware-api` state traits.

Rationale: `ferrix-hardware-api` models small, immediate hardware facts
(`state()` reads plus a few commands). Calls and messages are long-lived
domain objects with lifecycles, identities, persistence, multipart assembly,
and per-application authorization. Folding them into `HardwareState` would
put application domain state into the hardware waist and force every shell
client that subscribes to battery events to carry call and SMS state. The
existing `Telephony` trait (`ModemRegistration` x `SimState`) stays as the
coarse status-bar summary for this milestone and is documented as
"summary only; `ferrix-telephonyd` is authoritative". The home surface's
status bar switches to the telephony service when it is reachable and falls
back to the summary otherwise.

## Contract (`ferrix-telephony-api`, schema 1)

Typed enums and structs, kebab-case serde, no vendor types:

- `ModemState`: `absent | disabled | initializing | ready | failed { reason } | recovering`.
- `SimState`: `absent | locked { lock: pin | puk, attempts_left: u8 } | blocked | ready | failed { reason }`.
- `Registration`: `unknown | searching | denied | registered { roaming: bool } | emergency-only`;
  `Operator { name: String, mcc_mnc: Option<String> }` (mock uses synthetic
  values); `AccessTechnology`: `none | gsm | umts | lte | nr`;
  `Signal { bars: u8 (0..=4), rssi_dbm: Option<i16> }`.
- `Call { id: CallId, direction: incoming | outgoing, state: CallState, party: PartyRef, started_at_ms: u64, end_reason: Option<CallEndReason> }`.
  `CallState`: `dialing | ringing | active | held | ended`.
  `CallEndReason`: `local-hangup | remote-hangup | busy | rejected | no-answer | timed-out | no-service | modem-lost | failed { reason }`.
  `PartyRef { number: E164 (typed newtype), display: Option<String> }`;
  `E164` validates `+` and 8..=15 digits. Serialized on the protocol only to
  authorized clients; never in evidence records.
- `Dtmf(char)` restricted to `0-9*#A-D`.
- `Sms { id: MessageId, direction, party, body: String, parts: Vec<PartRef>, state: SmsState, timestamp_ms, failure: Option<SmsFailure> }`.
  `SmsState`: `sending | sent | delivered | failed | received | read | deleted`.
  `SmsFailure`: `no-service | rejected | timed-out | modem-lost | invalid-destination | unsupported | failed { reason }`.
  Multipart: the adapter reports parts (`MultipartPart { ref_id, index, count, text }`);
  the daemon assembles into one `Sms` and publishes once complete; incomplete
  concatenations time out to a typed `failed { reason: "incomplete-multipart" }` after a bounded window.
- `DataBearer`: `unsupported | disconnected | connecting | connected { interface: Option<String> } | failed { reason }`.
- `AudioRoute`: `earpiece | speaker | wired | bluetooth`; `AudioRouteState { active, available: BTreeSet<AudioRoute> }`.
- `RadioPower`: `on | off` (airplane/radios-disabled).
- `Capabilities: BTreeSet<TelephonyFeature>` with `voice | sms | multipart-sms | delivery-reports | dtmf | hold | data | audio-route | sim-pin | radio-power`.
  Unsupported features return `TelephonyError::Unsupported { feature }`.
- `TelephonyState` aggregate snapshot and `TelephonyEvent` deltas
  (`modem-changed`, `sim-changed`, `registration-changed`, `signal-changed`,
  `call-changed`, `sms-changed`, `data-changed`, `audio-route-changed`,
  `radio-power-changed`, `capabilities-changed`, `service-restarted`).
- `TelephonyError`: `unsupported { feature } | unauthorized { capability } | no-modem | no-sim | sim-locked | no-service | invalid-number | busy | not-found { id } | invalid-state { from, op } | backend { reason } | protocol { reason }`.

## Protocol roles and capabilities

Client capabilities map to `ferrix-app-api::AppCapability`: `telephony-state`
(read snapshot, subscribe; party numbers redacted to `display` only),
`telephony-call` (dial, answer, reject, hangup, hold, resume, dtmf, set audio
route, enter SIM PIN, see full party numbers on calls), `telephony-sms` (send,
list, mark read, delete, see bodies), `telephony-data` (connect and disconnect
packet-data bearers), and `telephony-radio` (set cellular radio power without
voice-call control). Emergency numbers are **rejected** with
`Unsupported { feature: emergency }`; no emergency path exists.

### Subscription handshake (issue #68)

`subscribe` is answered with `{"reply":"state","state":...}`: the caller's
projected snapshot, taken atomically with registration, as the shared
convention in `docs/adr/README.md` and `ferrix-networkd` already require.
Every event that follows was applied after that snapshot and is sent once, in
apply order, so a client replaces local state with the snapshot and folds
each event without deduplication. The daemon applies an event to its state
and appends it to an outbox under the state lock; the outbox is drained only
under the subscribers lock, and registration drains it (to the existing
subscribers) under the same state lock that copies the snapshot. Socket
writes and per-event identity re-resolution therefore never run under the
state lock. The snapshot is projected for the identity re-resolved at
registration, after the subscribers lock is held and immediately before the
reply is written, so grants revoked or narrowed while registration waited
for the lock are honoured; each later event re-resolves again (ADR-004).

Until 2026-10-12 `subscribe` replied `ok` and the client fetched the snapshot
with a separate `state` request first, so an event between the two was in
neither. The schema stays `1`: the data model is unchanged, the daemon and
its only subscribers ship from one workspace, and a client expecting the old
`ok` fails closed on the `state` reply (and a new client on a bare `ok`)
instead of silently missing events. `state` remains a standalone read, not a
subscription anchor.

## Adapter contract (`ferrix-telephony-adapter-api`, internal)

`trait ModemBackend: Send + Sync` with synchronous operations
(`snapshot`, `dial`, `answer`, `hangup`, `hold`, `resume`, `dtmf`,
`send_sms`, `set_radio_power`, `enter_pin`, `set_audio_route`,
`capabilities`) and an event `subscribe(Sender<BackendEvent>)`. The daemon
owns call/SMS identity, multipart assembly, persistence of SMS to a
versioned store under `/var/lib/ferrix/telephony/` (atomic writes; bodies
never logged). A corrupt document remains authoritative, is copied to a
timestamped forensic file, and prevents startup rather than silently replacing
the user's message history. On start the daemon reads the backend snapshot and
publishes `service-restarted` with the full state.

- `ferrix-telephony-mock`: state from the profile `[telephony]` section;
  mutations from the host mock-control channel (ADR-006). Deterministic:
  timers only advance on explicit `tick`/scheduled steps in scenarios, never
  wall clock by default.
- `ferrix-telephony-mm`: ModemManager 1.x over D-Bus through `zbus`
  (`org.freedesktop.ModemManager1`, `Modem`, `Modem.Voice`, `Call`,
  `Modem.Messaging`, `Sms`, `Modem.Simple`, `Sim`). Linux-only, feature
  `production`. It never opens QMI/MBIM/QRTR/AT devices. Audio routing is
  delegated to the audio service when it exists; until then `audio-route`
  is `Unsupported` on the production adapter and documented as such.

## Selection

`ferrix-telephonyd --backend mock|modemmanager` is chosen by the session
bootstrap scripts (QEMU init and pmOS product start), never by apps.
