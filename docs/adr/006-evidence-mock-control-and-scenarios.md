# ADR-006: Evidence labels, mock-control channels, scenario files

Status: accepted by the lead 2026-09-21.

## Evidence labels

`mock` = host process using the mock profile (nested shell, unit and
integration tests). `QEMU` = inside the generic AArch64 `virt` guest (Alpine
smoke or postmarketOS route; the record or document names which). Physical
evidence names the exact target id and hardware revision and lives only under
`docs/board-support/`. Nothing generalizes across these labels.

## Mock-control channels

Each mocked daemon has its own host-only channel; there is no cross-daemon
forwarding and no application API for mutations.

| Daemon | QEMU virtio-serial port | Host socket (`out/qemu-aarch64/[agent/]`) | Nested host path |
| --- | --- | --- | --- |
| `ferrix-core` | `ferrix.control` (existing) | `control.sock` | `--control none` |
| `ferrix-telephonyd` | `ferrix.telephony` | `telephony-control.sock` | `--mock-control out/qemu-aarch64/telephony-control.sock` |
| `ferrix-networkd` | `ferrix.network` | `network-control.sock` | `--mock-control out/qemu-aarch64/network-control.sock` |

The shared crate `ferrix-mock-control` provides the serve loop (file or Unix
socket, line JSON in, ack out) and the typed `Ack { status: ok|error, ... }`.
A daemon started with a production backend refuses `--mock-control` with a
typed error. Commands are `#[serde(deny_unknown_fields)]`; unknown commands
are rejected, never ignored.

Successful telephony and network acknowledgements cross an ordered backend
barrier before they are written. The barrier travels on the same event channel
as state changes, is consumed by the daemon (never published to applications),
and guarantees that all records caused by one command exist before the next
scenario step begins.

## Profile extension (`ferrix-target-profile`, schema stays 1)

Optional sections with `#[serde(default)]` so the existing profile parses
unchanged; the mock adapters derive defaults from `[connectivity]` when a
section is absent:

```toml
[telephony]
modem = "ready"            # absent | disabled | initializing | ready | failed
sim = "ready"              # absent | locked | blocked | ready
sim_pin = "0000"           # synthetic; only meaningful when sim = "locked"
operator = "Ferrix Mock"
access = "lte"
signal_bars = 3
roaming = false
features = ["voice", "sms", "multipart-sms", "delivery-reports", "dtmf", "hold", "audio-route", "sim-pin", "radio-power"]

[network]
adapter = "present"        # absent | present
radio = "enabled"          # enabled | disabled
connected = "ferrix-lab"   # ssid of the AP connected at boot, or ""
[[network.access_points]]
ssid = "ferrix-lab"
security = "wpa2-psk"
signal_percent = 78
passphrase = "mock-passphrase"   # synthetic; mock only
```

## Scenario files

`targets/qemu-aarch64/scenarios/<name>.toml`, strict schema
(`deny_unknown_fields`), run by the host binary `ferrix-scenario`
(`just qemu-scenario NAME`, `just qemu-scenario-check` runs the checked-in
set bounded and headless):

```toml
schema_version = 1
name = "incoming-call-answer-hangup"
description = "..."
[[step]]
service = "telephony"                   # telephony | network | core
command = { cmd = "incoming-call", number = "+15550100042" }
expect = [ { event = "call-changed", state = "ringing" } ]
timeout_ms = 5000
[[step]]
service = "telephony"
command = { cmd = "tick", ms = 1000 }   # deterministic time
```

Steps are applied in order; each `expect` entry is matched against serial
records (guest) or the daemon's stdout (nested) as a JSON subset match. The
runner writes a machine-readable result record and a per-scenario evidence
directory under `out/qemu-aarch64/scenarios/<name>/` (serial excerpt,
optional framebuffer). Wall-clock timing is never asserted; only ordering
and bounded timeouts.

Required checked-in scenarios: no-modem, modem-init-failure-recovery,
sim-absent, sim-pin-locked-unlock, sim-ready, registration-search-deny-register-roam-loss-recover,
incoming-call-answer-route-remote-hangup, outgoing-call-connected,
outgoing-call-busy, outgoing-call-rejected, outgoing-call-timeout,
outgoing-call-no-service, call-interrupted-by-modem-loss, sms-incoming-single,
sms-incoming-multipart, sms-outgoing-success, sms-outgoing-failed,
sms-delayed-delivery-report, radios-disabled, wifi-disabled-scan-connect,
wifi-auth-failed, wifi-dhcp-failed, wifi-captive, wifi-offline-recovered,
service-crash-restart-reconcile.

## `qemu-check` and the smoke guest

`just qemu-check` stays headless, TCG, bounded, display-independent, and
offline. It gains two assertions only: `telephonyd-ready` and
`networkd-ready` records (both daemons run with mock backends in the Alpine
guest). The Alpine smoke guest **stays offline** (no virtio-net): its job is
the bounded CI contract, and real IP connectivity is already provided by the
postmarketOS route, which is where browser, DNS, and TLS evidence belongs.
Adding a NIC to the smoke guest would widen the CI surface without a fit
reason.

## Observability chain

Every important transition must be traceable as:
`adapter (mock-command ack) -> service (telephony-event record) -> protocol subscriber (client record) -> application state (app ui-state record) -> rendered frame (framebuffer capture)`.
Application surfaces emit `ui-state` records with page and state names only.
