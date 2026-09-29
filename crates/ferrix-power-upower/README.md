# ferrix-power-upower

Target-neutral production adapter from UPower's system D-Bus API to
`ferrix-hardware-api::Power`.

The adapter reads UPower's aggregate display device at
`/org/freedesktop/UPower/devices/DisplayDevice`. Its `Percentage` is rounded
and bounded to Ferrix's `u8` percentage; `Charging` and `PendingCharge` map to
`charging: true`. UPower property signals cause an authoritative re-read and
publish only actual Ferrix state changes.

The default build includes the reconciliation engine and deterministic fake-bus
tests. `--features production` adds the `zbus` blocking system-bus transport.
If UPower temporarily disappears or a refresh fails, the adapter retains its
last authoritative reading: the current portable contract has no unavailable
power state, and publishing a manufactured empty battery would be misleading.

This crate delegates kernel and device handling to UPower. It contains no
board, vendor, sysfs, charger, or battery-protocol logic and makes no physical
device support claim.
