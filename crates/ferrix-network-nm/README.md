# ferrix-network-nm

NetworkManager 1.x adapter for `ferrix-networkd` (ADR-002), built on the
`zbus` blocking API. Linux production only; compiled into the daemon with
`cargo build -p ferrix-networkd --features production`.

## Layers

| Module | Compiled | Purpose |
| --- | --- | --- |
| `map` | always | Pure tables: `NMDeviceType` → `AdapterKind`, AP flag words → `SecurityType`, `NMDeviceState`/`NMDeviceStateReason` → `ConnectionState`/`NetworkError`, `NMConnectivityState` → `Connectivity`, object path → stable `ApId`/`ProfileId`. Unit-tested on the host. |
| `diff` | always | Deltas between two snapshots, in publish order. |
| `backend` | feature `production` | `NetworkManagerBackend`: proxies for `Manager`, `Device`, `Device.Wireless`, `AccessPoint`, `Settings`, `Settings.Connection`, `Connection.Active`, `IP4Config`; snapshot enumeration; change watcher; restart handling. |

## Behaviour

- **State is derived, not accumulated.** Every `PropertiesChanged` from
  `org.freedesktop.NetworkManager` triggers a fresh enumeration; the diff
  against the previous snapshot is published as typed deltas. Lost or
  reordered signals cannot leave stale state behind.
- **Restart.** `NameOwnerChanged` for `org.freedesktop.NetworkManager` with
  an empty new owner publishes `BackendEvent::Lost`; a new owner re-resolves
  every object path and publishes `BackendEvent::Restarted { state }`, which
  the daemon forwards as `service-restarted`.
- **Hardware addresses.** The BSSID is read once per access point and hashed
  with `ferrix_network_api::hash_hardware_address` inside
  `NetworkManagerBackend::access_point`; the raw string is dropped before the
  function returns. No other type in the crate holds it.
- **Ethernet-like transport.** The QEMU `virtio-net` device has
  `NMDeviceType` `ETHERNET` and is reported as
  `AdapterPresence::Present { kind: ethernet-like }` with
  `RadioState::Unavailable`. It is **never** reported as Wi-Fi: `scan`,
  `set_radio`, `connect`, and `connect_profile` return
  `Unsupported { feature: "wifi (ethernet-like transport)" }`. Its link state
  is reported through `ConnectionState` with a synthetic `link-<n>` id, and
  its `IP4Config` and the manager's connectivity state are reported normally,
  so guest IP evidence (DHCP, DNS, TLS) stays honest without any 802.11 claim.
- **Joining.** `connect` uses `AddAndActivateConnection` with `key-mgmt`
  `none` (open), `wpa-psk` (WPA/WPA2), or `sae` (WPA3). WEP, enterprise, and
  unknown security types return `Unsupported`; nothing imitates success.
  The passphrase goes into the `psk` secret of that call and nowhere else.
- **Scan completion** is detected by `Device.Wireless.LastScan` moving past
  the value read when the scan was requested.

## Limitations

- `IpConfig.dhcp` is reported as `true`; NetworkManager does not expose the
  per-address method on `IP4Config`, and static addressing is not modelled.
- `Connectivity::Portal { url }` never carries a URL (NetworkManager does not
  publish one on the bus).
- Only the first managed Wi-Fi device (or, failing that, the first managed
  Ethernet-like device) is exposed; multi-adapter systems are out of scope.
- No integration test runs against a real NetworkManager in this repository.
  Evidence for this adapter is limited to compilation (`cargo check
  --features production`) and the pure mapping tests until the postmarketOS
  route exercises it in QEMU (`ethernet-like` only; no 802.11 claims are
  possible there).
