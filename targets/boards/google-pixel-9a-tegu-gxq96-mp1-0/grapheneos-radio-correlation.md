# GrapheneOS radio correlation for Pixel 9a

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`)

This is a source-correlation record, not Ferrix hardware evidence. GrapheneOS
is useful here because it continuously exercises the exact Pixel 9a board, but
its Android userspace is deliberately outside the Ferrix implementation. No
GrapheneOS image, Android framework, vendor HAL, RIL, Binder service, SELinux
policy, or prebuilt kernel module is a Ferrix dependency.

## Pinned GrapheneOS sources

| Source | Revision | Use in Ferrix | Boundary |
| --- | --- | --- | --- |
| [GrapheneOS Tegu kernel prebuilts](https://github.com/GrapheneOS/device_google_tegu-kernels_6.1/tree/dce102685029a89dd928ac3cf547e1d0ee6ee0ed) | `dce102685029a89dd928ac3cf547e1d0ee6ee0ed` (`17`, Android 17 QPR1, 2026-09-25) | Corroborates the exact module names and load ordering used by a maintained Pixel 9a build | Prebuilt `.ko` files are not copied or loaded by Ferrix |
| [GrapheneOS Tegu kernel setup](https://github.com/GrapheneOS-Archive/kernel_devices_google_tegu/tree/adf50bfc7e775a4ba6e94a35c62459bf5a0e8051) | `adf50bfc7e775a4ba6e94a35c62459bf5a0e8051` (`16`) | Corroborates Tegu device-tree wiring and the S5300 Kconfig fragment | The archived setup is Android/Kleaf build metadata, not a Ferrix guest |
| [GrapheneOS Tegu device sources](https://github.com/GrapheneOS-Archive/device_google_tegu/tree/27ced2f1523e23af0fc255c9451700a81d4b475e) | `27ced2f1523e23af0fc255c9451700a81d4b475e` | Confirms Android-side names such as `tegu`, `bcmdhd`, and the Bluetooth UART | Android product, HAL, RIL, and vendor configuration are not portable Linux APIs |
| [GrapheneOS vendor state](https://github.com/GrapheneOS/vendor_state/blob/1469180711293a5852bfda9a5f67b8f53d9828ad/tegu.json) | `1469180711293a5852bfda9a5f67b8f53d9828ad` | Generated inventory corroborates module filenames in a current Android release | It is not a firmware source, hash manifest, or native Linux contract |

The authoritative build inputs remain the pinned Google kernel manifest and
component revisions in [`kernel-sources.toml`](kernel-sources.toml). The
GrapheneOS records above are independent corroboration and must not silently
replace those pins.

## Wi-Fi and Bluetooth hardware facts

The archived GrapheneOS kernel setup's
[`zuma-tegu-wlan.dtsi`](https://github.com/GrapheneOS-Archive/kernel_devices_google_tegu/blob/adf50bfc7e775a4ba6e94a35c62459bf5a0e8051/dts/zuma-tegu-wlan.dtsi)
places the BCM4383 controller on `pcie_1` at Gen 2 and records the board
control signals:

- `wl_reg_on`: `gph3-4`
- `wl_host_wake`: `gpa5-3`
- `wl_dev_wake`: `gph3-5`
- device-tree compatible: `android,bcmdhd_wlan`
- channel number: `1`

The `android,bcmdhd_wlan` string is a downstream kernel binding name, not a
reason to import Android Wi-Fi services. Ferrix should retain the source-level
PCIe/GPIO facts while using NetworkManager and the Linux firmware class in the
guest. The exact firmware request names and factory provenance remain those in
[`wifi-firmware-source-evidence.md`](wifi-firmware-source-evidence.md). The
canonical Google route keeps its `bcmdhd` names; the separately pinned
radio-first route uses only the explicit, hash-pinned copy/rename rows in
[`firmware-radio-route.tsv`](firmware-radio-route.tsv) for the candidate's
`brcmfmac4383-pcie.*` requests. Neither route silently treats the contracts as
interchangeable.

The current GrapheneOS kernel prebuilt's
[`vendor_dlkm.modules.load`](https://github.com/GrapheneOS/device_google_tegu-kernels_6.1/blob/dce102685029a89dd928ac3cf547e1d0ee6ee0ed/grapheneos/vendor_dlkm.modules.load)
lists `nitrous.ko`, `bcmdhd4383.ko`, `wlan_ptracker.ko`, and `wonder.ko`.
Its [`init.insmod.tegu.cfg`](https://github.com/GrapheneOS/device_google_tegu-kernels_6.1/blob/dce102685029a89dd928ac3cf547e1d0ee6ee0ed/grapheneos/init.insmod.tegu.cfg)
loads `bcmdhd4383.ko` after common modules. The same release's
[`vendor_kernel_boot.modules.load`](https://github.com/GrapheneOS/device_google_tegu-kernels_6.1/blob/dce102685029a89dd928ac3cf547e1d0ee6ee0ed/grapheneos/vendor_kernel_boot.modules.load)
loads the S5300 transport modules (`shm_ipc.ko`, `cpif_page.ko`, and
`cpif.ko`), while `system_dlkm.modules.load` supplies generic Linux
Bluetooth modules including `hci_uart`, `btsdio`, `btbcm`, the Bluetooth
network stack, and `rfkill`.

These lists are useful implementation checks for the Ferrix kernel package:
the source-built equivalents must be present and dependency ordering must be
verified before runtime testing. They do not establish that Ferrix can load a
controller, and Android's module loader, properties, and service readiness
signals must not be copied.

The archived Android device source configures `/dev/ttySAC18` for Bluetooth.
That is useful transport corroboration only. Ferrix should derive the native
Linux HCI/serdev path from the pinned board device tree and provide GPIO/rfkill
power handling through the Linux kernel and BlueZ, not through Android's
Bluetooth vendor library.

## S5300 modem facts

The archived setup's [`tegu_defconfig`](https://github.com/GrapheneOS-Archive/kernel_devices_google_tegu/blob/adf50bfc7e775a4ba6e94a35c62459bf5a0e8051/tegu_defconfig)
enables the same open S5300 kernel pieces recorded by the Ferrix target:
`EXYNOS_MODEM_IF`, `SEC_MODEM_S5100`, `SHM_IPC`, packet processing, modem QoS,
the voice-call suspend hook, PCIe IOCC/IOMMU, channel extension, and CP
thermal support. The setup's
[`zumapro-tegu-common.dtsi`](https://github.com/GrapheneOS-Archive/kernel_devices_google_tegu/blob/adf50bfc7e775a4ba6e94a35c62459bf5a0e8051/dts/zumapro-tegu-common.dtsi)
includes the SIT board description, Bluetooth wake control, PCIe CP-S5300
description, and Tegu WLAN description.

This corroborates kernel configuration and board composition only. It does
not publish the CP PBL/MAIN/NV payloads or the userspace SIT/GEMS/carrier
protocol needed for attach, data, SMS, voice, or IMS. Ferrix therefore keeps
the S5300 capability unsupported and the Rust crate bounded to framing and
explicitly supplied transport until a separately reviewed native-Linux
userspace backend, firmware provenance, and exact-device evidence exist.

## Reuse decision

Allowed uses of this research:

- review the exact Tegu DT pins, PCIe placement, UART name, module basenames,
  and dependency/load-order expectations;
- compare source-built Ferrix kernel artifacts against a current Pixel 9a
  Android kernel's hardware-facing module inventory;
- use GrapheneOS device and kernel source history to identify regressions to
  investigate in the pinned Google source tree.

Explicitly excluded:

- Android Wi-Fi/Bluetooth framework services, vendor libraries, AIDL/HIDL HALs,
  RIL, `shared_modem_platform`, Binder interfaces, or SELinux policy;
- GrapheneOS/AOSP prebuilts or proprietary vendor blobs as a substitute for
  the target's exact factory firmware workflow;
- treating Android boot success or GrapheneOS radio operation as Ferrix Linux
  physical evidence.

The next physical radio gate remains a native Ferrix boot on the exact
`tegu`/`GXQ96`/`MP1.0` device, followed by read-only kernel/firmware/HCI/Wi-Fi
and CPIF inventory and the typed BlueZ/NetworkManager/Ferrix evidence required
by [`references/radios-and-evidence.md`](../../../skills/mobile-device-provisioning/references/radios-and-evidence.md).
