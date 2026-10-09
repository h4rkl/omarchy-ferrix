# ferrix-bluetooth-bluez

Target-neutral production adapter from BlueZ's system D-Bus API to the
`ferrix-hardware-api::Bluetooth` contract.

## Power and devices

The adapter enumerates `org.bluez.Adapter1` and `org.bluez.Device1` objects
through `org.freedesktop.DBus.ObjectManager`. Adapter presence maps to
`present`, the managed adapter's `Powered` property to `enabled`; enable and
disable set that property and re-read it as authoritative state. BlueZ
signals and service-owner changes trigger a coalesced full re-enumeration,
so lost or reordered property signals cannot accumulate stale state.

If BlueZ exports multiple adapters, the lexicographically first object path
is the managed adapter (deterministic, matching the single-adapter
contract). An absent adapter is `{ present: false, enabled: false }`; a
command with no adapter returns a typed Bluetooth failure.

Devices carry `Name` (never `Alias`, which falls back to the address),
`Paired`, `Connected`, a coarse kind from `Icon`/`Appearance`, and signal
bars from `RSSI` while a discovery sees them. Product ids are a keyed hash
of the object path with a per-process random key: stable for the session,
not reversible to the address. Error texts that cross the boundary are
redacted (`AA:BB:..` and `AA_BB_..` become `<device>`).

## Discovery, pairing, connections

* **Discovery:** `SetDiscoveryFilter({Transport: auto})` then
  `StartDiscovery`. The adapter stops it itself after 30 s
  (`BLUETOOTH_DISCOVERY_TIMEOUT_MS`); a restart extends the deadline and
  supersedes the old timer. Pairing stops our discovery first.
* **Pairing:** `Device1.Pair` on a worker thread; on success the device is
  marked `Trusted` and connected (as a phone's Settings does). The adapter
  exports `org.bluez.Agent1` at `/org/ferrix/bluetooth/agent` and registers
  it with `AgentManager1.RegisterAgent(.., "DisplayYesNo")` and
  `RequestDefaultAgent` (again after a BlueZ restart).
  * `RequestConfirmation` (numeric comparison) and `RequestAuthorization`
    (Just Works) become `confirm-passkey` / `authorize` prompts in the
    session; Settings answers through `reply_bluetooth_pairing`. An
    unanswered prompt is declined after 30 s (`timed-out`).
  * `DisplayPasskey` (keyboards) shows the code to type on the remote.
  * `RequestPinCode` (legacy PIN) is **refused**: the attempt fails as
    `pin-required`. This is the documented choice for now; a typed PIN
    prompt can be added without changing the contract. `RequestPasskey` is
    refused too (a `DisplayYesNo` agent is not asked to type).
  * `AuthorizeService` is allowed for bonded devices only, without a
    prompt.
  * BlueZ errors map to typed failures (`AuthenticationRejected` ->
    `rejected`, `AuthenticationTimeout` -> `timed-out`,
    `ConnectionAttemptFailed` -> `unreachable`, ...); a locally decided
    reason (declined, prompt timeout, PIN refused) wins.
* **Connect/disconnect:** `Device1.Connect`/`Disconnect` on a worker thread;
  a failure stays in the session's `operation` with BlueZ's short reason.
* **Forget:** `Adapter1.RemoveDevice`.

## Threading (deadlock rules)

zbus's socket reader stops while a `MessageIterator` queue is full. So each
signal iterator has a consumer thread that only forwards an event over an
unbounded std channel (no D-Bus call, no sleep); blocking calls run on the
state thread or workers. Agent methods run on zbus's executor and only touch
in-memory state, then `await` the user's answer on an `async-channel`, so a
pending prompt never blocks the executor or the `Pair` reply it belongs to.
No lock is held across a D-Bus call.

## Builds and evidence

The default build includes the state/reconciliation engine and mocked-bus
unit tests (discovery bounds, prompt flow, typed failures, redaction).
`--features production` adds the `zbus` blocking system-bus transport and
the agent. Discovery, pairing, and connections through this crate have
**not** been exercised on a physical controller yet; the QEMU target uses the
deterministic emulator instead. This crate does not model profiles or audio
routing.
