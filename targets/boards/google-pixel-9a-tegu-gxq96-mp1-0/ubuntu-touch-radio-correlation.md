# Ubuntu Touch radio correlation for Pixel 9a

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`)

This is an external bring-up correlation record, not Ferrix hardware evidence.
The [public Ubuntu Touch Tegu port](https://git.deusch.me/ubports/device-google-tegu)
reports Wi-Fi, Bluetooth, cellular data, calls, and SMS working in its
experimental image. Its cellular stack is Halium/Android-based, however, so
those reports do not establish a native-Linux modem ABI and Ferrix must not
import its Android RIL, Binder, vendor HAL, libhybris, or Android service
contracts.

## Native-Linux-relevant findings

- The port reports that Tegu's Wi-Fi path can expose an `aware_nmi*` auxiliary
  interface which must be ignored by NetworkManager; touching it has caused a
  kernel panic in that port. Ferrix installs the target-local
  `10-ignore-aware-nmi.conf` guard while leaving the primary Wi-Fi interface
  managed normally.
- The port's working cellular result depends on Halium's Android radio
  bridge. It is useful evidence that the physical Exynos 5300t and its stock
  firmware can attach on this exact board, but it is not evidence for the
  checked-in `s5300_modem`/SIT/ModemManager route.
- The port builds and repacks downstream Android kernel/vendor partitions.
  Ferrix may use its public board/kernel observations to investigate device
  tree and firmware failures, but the Ferrix boot path remains native Linux
  with NetworkManager, BlueZ, and ModemManager adapters.

## Reuse boundary

Allowed: investigate the `aware_nmi*` NetworkManager guard, compare exact
Tegu kernel/DT behavior, and use the port's feature matrix to prioritize
physical evidence.

Excluded: Halium, Android framework services, RIL/HAL/Binder, libhybris,
Android vendor binaries, or the port's images as Ferrix runtime dependencies.
The authoritative Ferrix radio inputs remain the pinned kernel, factory
firmware policy, and target-local native-Linux package sources.
