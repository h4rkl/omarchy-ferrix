# ferrix-bluetooth-bluez

Target-neutral production adapter from BlueZ's system D-Bus API to the coarse
`ferrix-hardware-api::Bluetooth` contract.

The adapter enumerates `org.bluez.Adapter1` objects through
`org.freedesktop.DBus.ObjectManager`. Adapter presence maps to `present`, and
the managed adapter's `Powered` property maps to `enabled`. Product enable and
disable commands set that same property and then re-read it as authoritative
state. BlueZ signals and service-owner changes trigger full re-enumeration, so
lost or reordered property signals cannot accumulate stale state.

If BlueZ exports multiple adapters, the lexicographically first object path is
the managed adapter. The choice is deterministic and matches Ferrix's current
single-adapter Bluetooth contract. An absent adapter is `{ present: false,
enabled: false }`; a command with no adapter returns a typed Bluetooth failure.

The default build includes the state/reconciliation engine and mocked-D-Bus
unit tests. `--features production` adds the `zbus` blocking system-bus
transport. No test or repository claim establishes that a physical Bluetooth
controller, firmware, pairing, profiles, or Bluetooth audio works. This crate
does not model discovery, pairing, connections, profiles, or audio routing.
