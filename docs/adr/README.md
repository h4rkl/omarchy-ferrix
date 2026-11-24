# Architecture decision records

Subordinate to `plans/baseline.md` (implementation authority) and
`plans/plan.md`; baseline wins conflicts. Each record states the decision,
the boundary it creates, and what evidence may claim. Evidence is `QEMU` or
`mock` unless a record names an exact physical target and revision.

| ADR | Title |
| --- | --- |
| [001](001-telephony-service-boundary.md) | Telephony service boundary |
| [002](002-network-model-and-adapter.md) | Network model and NetworkManager adapter boundary |
| [003](003-contacts-ownership-and-storage.md) | Contacts ownership, storage, migrations, export |
| [004](004-service-authorization.md) | Service authorization and sandbox socket visibility |
| [005](005-android-runtime-and-brave.md) | Android compatibility runtime and the Brave reference app (browser choice superseded by ADR-007) |
| [006](006-evidence-mock-control-and-scenarios.md) | Evidence labels, mock-control channels, scenario files |
| [007](007-fulguris-browser-input.md) | Fulguris browser APK input (supersedes the browser choice in ADR-005; superseded by ADR-008) |
| [008](008-apks-as-ferrix-apps.md) | APKs as first-class Ferrix apps: per-task windows, one warm container, LineageOS apps (Jelly browser) |

## Conventions shared by every new service (normative)

- **Crate split** follows the app-platform pattern: `<domain>-api` (types,
  no I/O), `<domain>-protocol` (wire types + blocking Unix-socket client with
  reconnection), `<domain>-adapter-api` (internal backend trait, never linked
  by apps), `<domain>-mock` (deterministic adapter), `<domain>-<prod>`
  (production adapter), `<domain>d` (daemon binary + library).
- **Wire format:** line-delimited JSON. First message is
  `{"request":"hello","schema_version":N,"role":...}`; the daemon replies with
  its `schema_version` and a full state snapshot. A mismatched or missing
  schema is rejected at hello. Every request type uses
  `#[serde(deny_unknown_fields)]`; unknown or malformed lines get
  `{"reply":"protocol-error","reason":...}` and never mutate state.
- **Subscriptions:** `{"request":"subscribe"}` switches a connection to a push
  stream. On (re)connect the client receives a full snapshot first, then
  deltas, so reconciliation after a daemon restart is "replace local state
  with the snapshot".
- **Sockets:** `/run/ferrix/<name>.sock` in the guest, overridable by
  `FERRIX_<NAME>_SOCKET`; nested host runs use `out/qemu-aarch64/<name>.sock`.
  Names: `telephonyd`, `networkd`, `contactsd`.
- **Authorization:** see ADR-004. Every daemon resolves the peer pid to a
  Ferrix `AppId` and granted capabilities through `ferrix-appd` before the
  first non-hello request and re-checks per operation.
- **Evidence records:** one JSON line, `evidence` first, then `event`, on
  stdout (`QEMU` in the guest, `mock` nested). Records carry ids and states
  only. **Never** log phone numbers, message bodies, contact names, IMSI,
  ICCID, IMEI, serials, SSIDs beyond a stable hash, BSSIDs, passwords, cookies,
  or tokens. Use `call_id`, `message_id`, `contact_id`, `ap_id`.
- **Typed unsupported:** an adapter that lacks a feature returns a typed
  `Unsupported { feature }` error; nothing imitates success.
- **Rust and lints:** workspace `unsafe_code = "forbid"`, edition 2024, Rust
  1.98.1 pinned. Linux-only code behind `cfg(target_os = "linux")`;
  production D-Bus adapters behind a `production` cargo feature so the macOS
  host still builds and tests the rest.
- **Tests:** every state machine transition, malformed/incompatible message,
  unauthorized operation, reconnection/restart reconciliation has a host test
  (`mock`). QEMU evidence is separate and labelled.
