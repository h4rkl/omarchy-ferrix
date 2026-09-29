# ADR-004: Service authorization and sandbox socket visibility

Status: accepted by the lead 2026-09-21.

## Problem

`docs/app-platform.md` records that capabilities are declarations and that
applications still share filesystem and socket access. Before real call,
SMS, contacts, or Android data flows, authorization must be enforced by the
services themselves.

## Decision

1. **Identity source.** `ferrix-appd` is the only authority on "which
   `AppId` is pid P". Services connect to the appd socket with the new role
   `service` (bootstrap-trusted like `compositor`) and send
   `{"request":"identify","pid":P}`. Reply:
   `{"reply":"identity","schema_version":1,"app":"org.ferrix.phone"|null,"trust":"...","granted":["telephony-call",...]}`.
   Resolution uses the existing `SO_PEERCRED` + `/proc` ancestry walk; an
   unknown pid yields `app: null, granted: []`.
2. **Grants are separate from declarations.** A checked-in policy file
   `apps/grants.json` (`schema_version: 1`, `{ "<app id>": ["cap", ...] }`)
   is loaded by `ferrix-appd` beside the catalog. The effective grant is
   `declared ∩ granted`. A capability declared but not granted is reported in
   the `app-started` record as `denied_capabilities`. `system`/`privileged`
   trust does not bypass the file; it is simply a documented row in it.
3. **Per-operation checks.** Each daemon caches the identity for a
   connection at first request, re-fetches on `service-restarted` from appd,
   and checks the required capability on every request. Failures are typed
   `unauthorized { capability }` and produce an evidence record
   `<name>-request-denied { app, capability, request }`. Read-only state and
   privileged control are distinct requests so a `*-state` grant can never
   reach a control verb.
4. **Sandbox socket visibility.** `IsolationPolicy` gains
   `sockets: Vec<String>` (names under `/run/ferrix`). Stage 2 of
   `ferrix-sandbox` mounts a private tmpfs over `/run/ferrix` and bind-mounts
   only the listed sockets (plus the Wayland socket directory). `ferrix-appd`
   derives the list from the effective grants: `telephony-*` → `telephonyd`,
   `network-*` → `networkd`, `contacts-*` → `contactsd`, `core-*` → `core`;
   the appd socket is always present (needed for `YieldForeground`). Native
   apps on macOS host tests skip the mount step (Linux-only) and the peer
   credential check remains the enforced boundary there.
5. **Android runtime.** The Android container never receives any `/run/ferrix`
   socket. Its network access is the guest's ordinary IP stack; its filesystem
   is its own data directory; devices are only what the runtime evaluation
   (ADR-005) lists. Cross-runtime integration (notifications, share) is out of
   scope and would need its own reviewed bridge.
6. **Remaining limitations (documented, not hidden).** Same uid for all apps;
   no seccomp; no cgroups; compositor/tool/service roles are bootstrap trust
   (not launched by appd). D-Bus for ModemManager/NetworkManager is reachable
   only by the daemons running outside app sandboxes; apps never see the
   system bus socket because it is not in their `/run` view.

## New `AppCapability` variants

`telephony-state`, `telephony-call`, `telephony-sms`, `telephony-data`,
`telephony-radio`, `contacts-read`, `contacts-write`, `network-state`,
`network-control` (existing: `core-state`, `core-control`, `shell`,
`background`).

## Tests required

Negative tests in every daemon: unknown pid denied; declared-but-not-granted
denied; `*-state` grant cannot invoke control; forged `AppId` in payload
ignored; identity re-resolved after appd restart. Sandbox test (Linux): an
app whose grants exclude telephony cannot `connect()` the telephony socket.
Long-lived telephony, network, and contacts subscriptions re-resolve before
each event and are dropped immediately when the state/read grant disappears.
