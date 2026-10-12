# ferrix-telephonyd

Privileged telephony service (ADR-001, ADR-004, ADR-006). Owns modem, SIM,
registration, call, and SMS state through a `ferrix-telephony-adapter-api`
backend; authorizes every request by the peer's verified application identity
and capability; serves `ferrix-telephony-protocol` on a Unix socket.

Evidence produced here is `mock` (nested host) or `QEMU` (guest). Nothing in
this crate is a physical-device or carrier claim.

## Crates

| Crate | Role |
| --- | --- |
| `ferrix-telephony-api` | pure types + serde, schema 1 |
| `ferrix-telephony-protocol` | line JSON wire types, blocking `Client`, `Subscription`, `Reconnector` |
| `ferrix-telephony-adapter-api` | internal `ModemBackend` trait, `BackendEvent`, `BackendError` |
| `ferrix-telephony-mock` | deterministic backend + mock-control `Handler` |
| `ferrix-telephony-mm` | ModemManager adapter over `zbus` (feature `production`) |
| `ferrix-telephonyd` | this daemon (library + binary) |

## Binary

```text
ferrix-telephonyd --backend mock|modemmanager [--profile PATH] [--socket PATH]
                  [--mock-control auto|none|PATH] [--appd PATH|none]
                  [--data-dir PATH] [--nested] [--allow-unauthenticated]
```

- Socket: `--socket`, env `FERRIX_TELEPHONYD_SOCKET`, default
  `/run/ferrix/telephonyd.sock`. Bound before `telephonyd-ready` is printed.
- Store: `--data-dir`, env `FERRIX_TELEPHONY_DIR`, default
  `/var/lib/ferrix/telephony` (`sms.json`, schema 1, tmp + fsync + rename).
- Identity: `--appd` (env `FERRIX_APPD_SOCKET`, default
  `/run/ferrix/ferrix-appd.sock`) speaks `{"request":"hello","role":"service"}`
  then `{"request":"identify","pid":N}` and expects
  `{"reply":"identity","app":...,"granted":[...]}`. `--appd none` denies every
  peer unless `--allow-unauthenticated` (nested development only; records
  `telephonyd-unauthenticated`).
- `--mock-control` with `--backend modemmanager` is a typed startup error.
- `--backend modemmanager` needs the `production` cargo feature (Linux guest).
- The production ModemManager adapter accepts the non-secret
  `FERRIX_TELEPHONY_APN` environment value as a fallback when a
  `connect-data` client sends the default profile. An explicit request APN
  wins; malformed configured values are rejected without including the value
  in an error or record. The board OpenRC service is responsible for sourcing
  this target-local setting.

Nested host example:

```sh
ferrix-telephonyd --backend mock --nested --appd none --allow-unauthenticated \
  --socket out/qemu-aarch64/telephonyd.sock \
  --mock-control out/qemu-aarch64/telephony-control.sock \
  --data-dir out/qemu-aarch64/telephony
```

## Capabilities (ADR-004)

| Capability | Requests | Visibility |
| --- | --- | --- |
| `telephony-state` | `hello`, `state`, `subscribe` | parties redacted (`+***0042`), no bodies |
| `telephony-call` | `dial`, `answer`, `reject`, `hangup`, `hold`, `resume`, `dtmf`, `set-audio-route`, `enter-pin` | full call numbers; no messages |
| `telephony-sms` | `send-sms`, `list-sms`, `mark-read`, `delete-sms` | full message numbers and bodies; no calls |
| `telephony-data` | `connect-data`, `disconnect-data` | bearer lifecycle and interface only |
| `telephony-radio` | `set-radio-power` | cellular radio power only; no calls or data control |

A peer holding both `telephony-call` and `telephony-sms` gets the full
snapshot. Denials reply `{"kind":"unauthorized","capability":...}` and record
`telephonyd-request-denied {app, capability, request}`. Identity is re-resolved
for every request and before every subscribed event, so grant revocation and an
application-service restart fail closed even on an existing connection.

`subscribe` replies with the projected snapshot (`{"reply":"state",...}`),
taken atomically with registration; the events that follow are exactly those
applied after it, once each and in apply order (ADR-001, issue #68).

## Records (stdout, `evidence` first)

`telephonyd-ready {socket, backend, capabilities}`, `telephonyd-failed`,
`telephony-mock-control {status, path, detail}`, `telephony-event` (below),
`telephonyd-request-denied {app, capability, request}`,
`telephonyd-request-rejected`, `telephony-store-error`, `telephony-backend-lost`,
`telephonyd-unauthenticated`. Records carry ids and state names only.

`telephony-event` mirrors the published event: `kind` is the event name and
the payload is the event's wire form without parties or bodies:

```json
{"evidence":"mock","event":"telephony-event","kind":"call-changed","call_id":"c1",
 "call":{"id":"c1","direction":"incoming","state":"ended","end_reason":{"kind":"remote-hangup"},"started_at_ms":0}}
{"evidence":"mock","event":"telephony-event","kind":"sms-changed","message_id":"m1",
 "sms":{"id":"m1","direction":"outgoing","state":"failed","failure":{"kind":"no-service"},"parts":[],"timestamp_ms":0}}
{"evidence":"mock","event":"telephony-event","kind":"modem-changed","modem":{"state":"ready"}}
{"evidence":"mock","event":"telephony-event","kind":"sim-changed","sim":{"state":"locked","lock":"pin","attempts_left":3}}
{"evidence":"mock","event":"telephony-event","kind":"registration-changed","registration":{"state":"registered","roaming":false},"operator":{"name":"Ferrix Mock","mcc_mnc":"001-01"},"access":"lte"}
{"evidence":"mock","event":"telephony-event","kind":"signal-changed","signal":{"bars":4}}
{"evidence":"mock","event":"telephony-event","kind":"audio-route-changed","audio":{"active":"speaker","available":["earpiece","speaker"]}}
{"evidence":"mock","event":"telephony-event","kind":"radio-power-changed","radio":"off"}
{"evidence":"mock","event":"telephony-event","kind":"service-restarted","state":{"modem":{"state":"ready"},"sim":{"state":"ready"},"registration":{"state":"registered","roaming":false},"radio":"on","calls":0,"sms":0}}
```

Multipart `parts` entries are `{ref_id, index, count}` with **1-based**
`index` (GSM UDH convention): a two-part message records
`[{"index":1,...},{"index":2,...}]`.

## Mock-control channel (host only)

The daemon owns the channel (`--mock-control`; virtio port `ferrix.telephony`
in the guest, a Unix socket nested). It exists only with `--backend mock`; a
production backend refuses it, and no application sandbox can see it. It is
**not an application API**. One JSON object per line, tagged `cmd`, unknown
fields rejected. Every ack is
`{"status":"ok","ack":NAME,"events":[...],"state":{...}}` (ids and states
only) or `{"status":"error","ack":NAME?,"reason":...}`.

### Test-only local client verbs (executed by the daemon)

These run through the same request path a protocol client uses, under the
synthetic identity `org.ferrix.host-tool` that holds every telephony
capability. Typed rejections (`no-modem`, `no-sim`, `sim-locked`,
`no-service`, `unsupported` for emergency numbers, `invalid-state`,
`not-found`) come back as `{"status":"error","reason":"<kind>: <error json>"}`.

| Command | Fields |
| --- | --- |
| `dial` | `number` |
| `answer`, `reject` | `call_id?` (default: the single ringing call) |
| `hangup` | `call_id?` (default: the single live call) |
| `hold` | `call_id?` (default: the single active call) |
| `resume` | `call_id?` (default: the single held call) |
| `dtmf` | `call_id?` (default: the single active call), `digit` (`0-9*#A-D`) |
| `send-sms` | `number`, `body` |
| `enter-pin` | `pin` |
| `set-audio-route` | `route` |
| `set-radio-power` | `power` |
| `mark-read` | `message_id` |

The ack's `events` are the `telephony-event` kinds the verb published; its
`state` is the daemon's ids-and-states summary. Every ack (local or
forwarded) is written only after the daemon has published the records the
command caused, so a scenario may chain commands without waiting.

### Backend mutation commands (forwarded to `ferrix-telephony-mock`)

| Command | Fields |
| --- | --- |
| `state` | – |
| `set-modem` | `state`: `absent`, `disabled`, `initializing`, `ready`, `failed`, `recovering`; `reason?` (recorded with `failed`) |
| `set-sim` | `state`: `absent`, `locked`, `puk-locked`, `blocked`, `ready`, `failed`; `attempts_left?` |
| `set-registration` | `registration`: `unknown`, `searching`, `denied`, `registered`, `emergency-only`; `roaming?`, `operator?`, `access?` |
| `set-signal` | `bars` (0..=4) |
| `incoming-call` | `number` (E.164) |
| `remote-answer`, `remote-hangup`, `remote-busy`, `remote-reject`, `outgoing-timeout` | `call_id?` (defaults to the single matching call) |
| `incoming-sms` | `number`, `body` |
| `incoming-sms-multipart` | `number`, `ref_id`, `parts: [String]` |
| `incoming-sms-part` | `number`, `ref_id`, `index` (1-based), `count`, `text` |
| `deliver-sms`, `fail-sms` | `message_id?`; `fail-sms` also `reason` (`no-service`, `rejected`, `timed-out`, `modem-lost`, `invalid-destination`, `unsupported`, or free text) |
| `set-outgoing-result` | `result`: `connected`, `busy`, `rejected`, `timeout`, `no-service` (applies to the next dial; fires on `tick`) |
| `set-sms-result` | `result`: `sent`, `failed`, `delayed-delivery` |
| `set-radio` | `power`: `on`, `off` |
| `set-audio-available` | `routes: [earpiece|speaker|wired|bluetooth]` |
| `tick` | `ms` (the only way time advances in the mock) |
| `crash-backend` | – (backend-lost; `ready` + `backend-restarted` after `tick 1001`) |

Deterministic delays (mock clock): outgoing outcome 1000 ms, ringing timeout
30 000 ms, SMS send 500 ms, delayed delivery +2000 ms, re-registration
1000 ms, recovery 1 ms to `recovering` then 1000 ms to `ready`. The daemon's
multipart window is 60 000 ms of the same clock.

Mock call handles are `c<n>` in creation order, which coincide with the
daemon's public `CallId`s in a fresh run. Message handles `m<n>` may diverge
from public `MessageId`s after a daemon restart because the daemon's counter
persists; `message_id` therefore defaults to the latest pending message.

## Limitations

- The mock backend keeps cellular data `unsupported` in the checked-in QEMU
  profile. Profiles that include the `data` feature exercise a deterministic
  mock bearer. The ModemManager adapter exposes the same target-neutral
  connect/disconnect API through `Modem.Simple` and observes `Bearer` state.
- Audio route is `unsupported` on the ModemManager adapter (delegated to a
  future audio service); the mock emulates it.
- The ModemManager adapter is type-checked (host and `aarch64-unknown-linux-gnu`)
  but has not been executed against a ModemManager instance.
- Emergency numbers are rejected with `unsupported { feature: emergency }`;
  no emergency path exists.
- Peer credentials (`SO_PEERCRED`) exist on Linux only; on macOS every peer is
  `None` and only `--allow-unauthenticated` lets a nested run proceed.
