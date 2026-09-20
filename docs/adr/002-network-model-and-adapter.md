# ADR-002: Network model and NetworkManager adapter boundary

Status: accepted by the lead 2026-09-21, subordinate to `plans/baseline.md`.

## Decision

Wi-Fi and connectivity are owned by `ferrix-networkd` with typed contracts
`ferrix-network-api` / `ferrix-network-protocol`. The three-state
`WifiState` in `ferrix-hardware-api` stays as the coarse summary for this
milestone ("summary only; `ferrix-networkd` is authoritative"). Product
`HardwareCommand::SetWifiEnabled` continues to work through `ferrix-core`;
quick settings move to `ferrix-networkd` when it is reachable.

## Contract (`ferrix-network-api`, schema 1)

- `AdapterPresence`: `absent | present { kind: wifi | ethernet-like }` — the
  QEMU virtio-net device is `ethernet-like`; it is **never** reported as Wi-Fi.
- `RadioState`: `disabled | enabled | unavailable { reason }`.
- `ScanState`: `idle | scanning | completed { count } | failed { reason }`.
- `AccessPoint { id: ApId (stable per daemon run), ssid: Ssid, bssid_hash: String (sha256 prefix, never the raw BSSID), signal_percent: u8, security: SecurityType, frequency_mhz: Option<u32> }`.
  `Ssid` is bytes shown as UTF-8 when valid; evidence records use `ssid_hash` only.
  `SecurityType`: `open | wep | wpa-psk | wpa2-psk | wpa3-sae | enterprise | unknown`.
- `ConnectionState`: `disconnected | associating | authenticating | configuring | connected { ap: ApId } | failed { error }`.
- `SavedProfile { id: ProfileId, ssid, security, autoconnect: bool }`; secrets are never returned by the protocol.
- `IpConfig { addresses: Vec<String>, gateway: Option<String>, dns: Vec<String>, dhcp: bool }`.
- `Connectivity`: `none | local | portal { url: Option<String> } | limited | full`.
- `NetworkError`: `unauthorized { capability } | radio-disabled | no-adapter | not-found { id } | authentication-failed | dhcp-failed | dns-failed | route-failed | timed-out | unsupported { feature } | backend { reason } | protocol { reason }`.
- Events: `adapter-changed`, `radio-changed`, `scan-changed`, `access-points-changed`,
  `connection-changed`, `ip-changed`, `connectivity-changed`, `profiles-changed`, `service-restarted`.

Capabilities: `network-state` (snapshot, subscribe, list APs with hashed
BSSIDs), `network-control` (enable/disable radio, scan, connect to profile or
AP with a supplied passphrase, forget profile).

## Adapter contract (`ferrix-network-adapter-api`, internal)

`trait NetworkBackend` with synchronous operations plus event subscription.

- `ferrix-network-mock`: synthetic access points from the profile `[network]`
  section; scenario steps drive scanning, association, auth failure, DHCP
  failure, captive portal, offline, recovery, and backend restart.
- `ferrix-network-nm`: NetworkManager 1.x over D-Bus via `zbus`
  (`org.freedesktop.NetworkManager`, `Device.Wireless`, `AccessPoint`,
  `Settings`, `ActiveConnection`, `IP4Config`, connectivity state). Linux-only,
  feature `production`. Handles NetworkManager restart by re-resolving object
  paths and republishing a snapshot.

## Transport truth

QEMU `virtio-net` provides real guest IP connectivity (DHCP, DNS, TLS) on the
postmarketOS route. It is `ethernet-like` transport. No QEMU evidence may
claim 802.11 scanning, association, firmware, regulatory, or radio behavior.
The Alpine smoke guest stays offline (ADR-006).
