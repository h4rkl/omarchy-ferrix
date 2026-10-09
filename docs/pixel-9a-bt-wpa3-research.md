# Pixel 9a Bluetooth bring-up and WPA3-SAE research

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`), Kleaf
GKI route (`android-gs-tegu-6.1-android16`).

**Status: research only, not evidence.** Everything here comes from reading
source, build outputs and upstream projects. No phone was used for it: no
ADB, Fastboot or serial access, and nothing was flashed. None of it shows that
Bluetooth or WPA3 works on the device. Device identifiers (serials, MAC/BD
addresses) are not recorded here. Firmware hashes are the ones already in
`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/bluetooth-firmware-source-evidence.md`.

Inputs read:

- Kernel workspace on the build host, synced by `tools/pixel-9a/kernel.sh`:
  `.cache/ferrix/pixel-9a/kernel/source`. Build output:
  `out/pixel-9a/kernel/tegu-20261009T073303Z/` (manifest `source manifest
  893fbfd2…`, `bluetooth-broadcom d7bd4dd2…`, `wifi-bcm4383 3526a8cb…`,
  `zuma-board 42395e8a…`, `gki-common a4662a84…`, `ferrix_gki_defconfig
  27c74464…`).
- AOSP `device/google/tegu` (`android-15.0.0_r25`), AOSP
  `external/wpa_supplicant_8` (`main`), NetworkManager `main`, and Alpine
  aports `3.24-stable` (`bluez` 5.86, `wpa_supplicant` 2.11-r4).

---

## A. Bluetooth (BCM4383 over UART)

### A.1 How Android attaches the controller

| Piece | Finding | Source |
| --- | --- | --- |
| UART | `serial_18` (`serial@155D0000`, `samsung,exynos-uart`, alias `uart18`, APM USI1) is enabled by the BT dtsi, so the node is `/dev/ttySAC18`. | `private/devices/google/zumapro/dts/zumapro-usi.dtsi:65,986`; `zumapro-board-bluetooth_wakectrl.dtsi` |
| Board DT used by tegu MP | `zumapro-tegu-common.dtsi` includes `zumapro/zumapro-board-bluetooth_wakectrl.dtsi`. The `zuma-board-bluetooth.dtsi` file cited in older notes is only used by `-dev1`. | `private/devices/google/tegu/dts/zumapro-tegu-common.dtsi:17` |
| Power/LPM node | `&odm { btbcm { compatible = "goog,nitrous"; shutdown-gpios = <&gpn8 0>; host-wakeup-gpios = <&gpa5 6>; device-wakeup-gpios = <&gpn9 0>; bt-ble-dbo-le-gpios = <&gpp24 3>; timesync-gpios = <&gpp11 1>; goog,wakelock-ctrl = <300>; } }`. This is a platform device, **not** a serdev child of `serial_18`. | `zumapro-board-bluetooth_wakectrl.dtsi` |
| Power driver | `nitrous.ko` (`google-modules/bluetooth/broadcom`, `CONFIG_NITROUS`). It registers an rfkill named **`nitrous_bluetooth`** (type bluetooth), which starts blocked and powers the chip off at probe. Unblocking drives BT_REG_ON (`shutdown` GPIO) low, waits 30 ms, drives it high, toggles the BLE-DBO GPIO and raises DEV_WAKE. It also exposes `/proc/bluetooth/sleep/{lpm,btwrite,btwake}`, `/proc/bluetooth/timesync` and `/proc/bluetooth/wakelock_ctrl` for host-wake/dev-wake low-power handling. LPM stays **off** until something writes `1` to `/proc/bluetooth/sleep/lpm`. With LPM off, DEV_WAKE stays asserted, the controller never sleeps, and no extra handshake is needed. Nitrous does no firmware loading and no HCI. | `nitrous.c:580-680`, `:263-360`, `:465-540` |
| HCI transport on Android | The proprietary Broadcom HAL opens the tty directly in userspace: `UartPort = /dev/ttySAC18`, `UserialTypePriority = 1` (SerialUartPort). It downloads the patchram itself and uses rfkill plus the nitrous `/proc` nodes. A2DP is offloaded to AoC (`BtOffloadCap = AOC`, `BtA2dpOffloadCap = …`). The kernel `hci_uart` is **not** used on Android. | [`bt_vendor_overlay.conf`](https://android.googlesource.com/device/google/tegu/+/refs/tags/android-15.0.0_r25/bluetooth/bt_vendor_overlay.conf) |
| Firmware | The factory `vendor.img:/etc/bluetooth/firmware_config.json` selects `vendor/firmware/brcm/BCM.hcd`. It is already pinned and allowlisted as `lib/firmware/brcm/BCM.hcd`. | `bluetooth-firmware-source-evidence.md`, `firmware-policy.toml`, `pmos/device-google-tegu/firmware-allowlist.tsv` |
| Wi-Fi/BT coordinator | bcmdhd4383 builds `wb_regon_coordinator.c` (`-DWBRC -DWBRC_WLAN_ON_FIRST_ALWAYS`). Opening the `wbrc` char device (BT side) **forces WLAN on first**. The coordinator also asks BT to reset when WLAN power-cycles. The combo chip probably expects WLAN to be up before BT. The BT firmware-download-via-WBRC path is compiled only for 4390 (`WBRC_HW_QUIRKS`), not 4383. | `google-modules/wlan/bcm4383/Kbuild:363-367`, `wb_regon_coordinator.c:347-430` |

### A.2 What our GKI build already has

From `dist/.config` of `tegu-20261009T073303Z`:

```
CONFIG_BT=m  CONFIG_BT_BREDR=y  CONFIG_BT_LE=y  CONFIG_BT_RFCOMM=m  CONFIG_BT_HIDP=m
CONFIG_BT_BCM=m  CONFIG_BT_HCIUART=m  CONFIG_BT_HCIUART_SERDEV=y
CONFIG_BT_HCIUART_H4=y  CONFIG_BT_HCIUART_BCM=y  CONFIG_BT_HCIUART_LL=y
CONFIG_RFKILL=m  CONFIG_SERIAL_DEV_BUS=y  CONFIG_SERIAL_DEV_CTRL_TTYPORT=y
CONFIG_SERIAL_EXYNOS=m (exynos_tty.ko, MAX_BAUD 4000000)
# CONFIG_BT_BNEP is not set   (PAN only; not needed for hci0)
# CONFIG_BT_HCIVHCI is not set
```

The modules are built and already listed in the Kleaf load lists:

- `system_dlkm.modules.load`: `hci_uart.ko`, `btbcm.ko`, `bluetooth.ko`,
  `rfkill.ko` (plus `rfcomm`, `hidp`).
- `vendor_dlkm.modules.load`: `nitrous.ko`.
- `vendor_kernel_boot.modules.load`: `exynos_tty.ko`.

`initramfs-init-kleaf` loads `system_dlkm.modules.load` and then
`vendor_dlkm.modules.load` (lines 377-378). `dev-compose.py stage_modules()`
merges both trees into the rootfs. **No kernel config fragment change and no
module-list change is needed for a first hci0.** This matches
`docs/status.md`: "the chip powers via rfkill but no HCI is attached".

### A.3 The kernel path that works without DT changes: `hci_uart` line discipline

Code reading of `aosp/drivers/bluetooth/hci_bcm.c` and `btbcm.c` (6.1 GKI):

- `btattach -B /dev/ttySAC18 -P bcm` sets N_HCI plus `HCI_UART_BCM`, then
  `bcm_open()`. With no serdev and no ACPI `bcm_device` (the platform driver
  only matches ACPI IDs), `bcm->dev == NULL`. Driver-side GPIO/power control,
  host-wake IRQ and the sleep VSC are all skipped (`bcm_request_irq()` returns
  `-ENODEV`). Power therefore has to come from nitrous's rfkill, which is the
  same split Android uses.
- `bcm_open()` requires hardware flow control. `btattach` sets `CRTSCTS`, and
  `serial_18` has `samsung,rts-alive-control`.
- `bcm_proto.init_speed = 115200` with **no `oper_speed`**. In the ldisc path
  nothing sets `hu->oper_speed`, so the link stays at **115200 baud** (about
  11 KB/s). `btattach -S` only sets the termios before attach, and
  `hci_uart_setup()` then resets it to 115200. That is enough for HCI
  bring-up, BLE and HID. Downloading the 628 KB patchram takes roughly a
  minute. It is not enough for A2DP/HFP audio over HCI. Android offloads
  audio to AoC, which BlueZ cannot use.
- `btbcm_initialize()`: BCM4383's subversion is not in `bcm_uart_subver_table`,
  so `hw_name` is NULL. It requests `brcm/BCM.<root-compatible>.hcd` (the
  board name comes from the root DT `compatible`, with `/` replaced by `-`),
  then **`brcm/BCM.hcd`**. The second name matches the factory-selected,
  already-allowlisted blob exactly (`firmware_class.path=/lib/firmware` is on
  the Kleaf cmdline). The first miss uses `firmware_request_nowarn`. **No
  alias or new firmware row is needed.**
- BD address: `btbcm_check_bdaddr()` only recognises older Broadcom default
  addresses. Whatever BCM4383 reports before configuration may be a shared
  default. The ldisc path has no DT `local-bd-address`.
  `HCI_QUIRK_USE_BDADDR_PROPERTY` does nothing without a fwnode. Android
  takes the per-device address from the bootloader, as `androidboot.btmacaddr`
  in bootconfig (to verify on device by reading `/proc/bootconfig`; do not
  record the value). `hdev->set_bdaddr = btbcm_set_bdaddr`, so
  `btmgmt public-addr` works while the controller is powered off.
- Userspace: Alpine 3.24 `bluez` 5.86 ships `btattach` in the main package.
  Only `hciattach`/`hciconfig`/`hcitool` and similar move to
  `bluez-deprecated`. `btmgmt` is the separate `bluez-btmgmt` subpackage. Both
  are to verify against the built rootfs (`apk info -L bluez`).
- `hciattach … bcm43xx` (deprecated, userspace patchram plus speed change)
  also works in principle. It looks the firmware up by chip name under
  `/etc/firmware`, which conflicts with the allowlist policy. Not recommended.

### A.4 Step-by-step bring-up plan

Phase 1: first hci0 at 115200, with no kernel, DT or firmware-policy changes.

1. **New OpenRC service**
   `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-bluetooth-uart.initd`.
   The `ferrix-*.initd` name means `tools/pixel-9a/dev-compose.py
   integrate_rootfs()` installs it automatically on the dev route. Behaviour:
   - `depend()`: `need ferrix-radio-firmware udev`; `after networkmanager`
     (WLAN-first, see WBRC); `before bluetooth ferrix-core ferrix-product`.
   - `start_pre`: fail closed unless `/lib/firmware/brcm/BCM.hcd` exists (the
     gate service has already hash-checked it), `/dev/ttySAC18` exists, and
     exactly one `/sys/class/rfkill/rfkill*/name` reads `nitrous_bluetooth`.
     Optionally wait for `wlan0` (or open `/dev/wbrc` read-only and keep it
     open; see the open question below).
   - Write `0` to that rfkill's `soft` file. Do **not** run `rfkill unblock
     bluetooth` globally. Wait about 150 ms (nitrous already waits 30 ms plus
     10-20 ms; the ROM boot time of BCM4383 is unknown).
   - Run `btattach -B /dev/ttySAC18 -P bcm` under `supervise-daemon`
     (`command_background`/`supervisor=supervise-daemon`), because `btattach`
     must stay alive to hold the line discipline.
   - `start_post`: wait (with a timeout of about 120 s at 115200) for
     `/sys/class/bluetooth/hci0`. If the bootconfig carries
     `androidboot.btmacaddr`, apply it with `btmgmt --index 0 public-addr`
     before BlueZ powers the adapter, and never log the value. Emit a
     machine-readable `ferrix-bt: hci-ready` / `hci-timeout` line for
     guest-evidence collection.
   - `stop`: stop `btattach`, then write `1` to the nitrous rfkill `soft` file.
2. **`APKBUILD`** (`pmos/device-google-tegu/APKBUILD`): add the initd to
   `source=`, install it to `/etc/init.d/ferrix-bluetooth-uart`, add it to the
   default-runlevel loop before `bluetooth`, and update `sha512sums`. Add
   `bluez-btmgmt` to `depends` only if the BD-address step is used.
   `ferrix-radio-firmware.initd` already lists `bluetooth` in `before`. Add
   `ferrix-bluetooth-uart` there too.
3. **Firmware**: no change. The `firmware-policy.toml`
   `[[controller]] bluetooth-broadcom` row, the allowlist row
   `lib/firmware/brcm/BCM.hcd`, and the gate's required row
   `bluetooth-broadcom /firmware/brcm/BCM.hcd lib/firmware/brcm/BCM.hcd` are
   already correct. Do **not** add `BTFW_B.hcd`, `BTFW_D.hcd` or a
   board-suffixed copy.
4. **Kernel config / module lists**: no change.
   `kernel-config.ferrix-gki` can optionally gain `CONFIG_BT_BNEP=m` later,
   for PAN only. Optionally make the rootfs/static checks assert that
   `hci_uart`, `btbcm`, `bluetooth` and `nitrous` are present in the staged
   load lists.
5. **Evidence**: once hci0 is up, run `btmgmt info` (address redacted), check
   for the `BCM: … 'brcm/BCM.hcd'` dmesg line, and confirm BlueZ
   `Adapter1.Powered` toggles through ferrix-core. Only then change
   `capabilities.toml` `[bluetooth]` and `docs/status.md`, labelled with the
   exact target.

Phase 2: operational speed, needed for audio.

- **2a (recommended next)**: add a small target-scoped GKI patch,
  `kernel-patches/aosp/0001-hci_bcm-ldisc-oper-speed.patch`. `kernel.sh
  apply_kernel_patches` already applies `kernel-patches/<component>/*.patch`
  in that component, and `aosp` is the GKI component. The patch adds a
  `hci_uart` module parameter, for example `bcm_ldisc_oper_speed`, used by
  `bcm_open()` when `!bcm->dev && !hu->serdev`. `bcm_setup()` then issues the
  `0xfc18` vendor command and switches the host UART after patchram. Set it
  with `hci_uart.bcm_ldisc_oper_speed=3000000` in
  `kernel-cmdline-kleaf.conf` (cmdline parameters apply to modules inserted
  later). Try 3 Mbaud first; the Exynos driver allows up to 4 Mbaud. This
  rebuilds the GKI `hci_uart.ko`, which is acceptable because the route
  builds GKI from source.
- **2b (canonical-Linux alternative, more invasive)**: describe the
  controller as a serdev child of `&serial_18`. Disable the `goog,nitrous`
  node (or keep nitrous out of the load list). Patch `hci_bcm.c` to add a
  `brcm,bcm4383-bt` compatible with `max-speed`, `shutdown-gpios`,
  `device-wakeup-gpios` and `host-wakeup-gpios`. Then the kernel owns power,
  baud, the host-wake IRQ and sleep mode, and no `btattach` is needed. Costs:
  a DT patch in `private/devices/google/zumapro` (the dtbo partition, which
  the factory image rewrites), and losing nitrous's BLE-DBO and timesync
  handling, whose purpose is undocumented. This is the model the radio-first
  route already describes (`README.md:661`). Do not load `nitrous.ko` at the
  same time.

Open questions for on-device checks:

- Does BCM4383 answer HCI_Reset at 115200 straight after unblock, or does it
  need WLAN up first?
- Does the BCM.hcd download complete inside the per-command timeout at
  115200? (Each `0xfc4c` write is small, so it should.)
- Is `/dev/wbrc` needed?
- Does exynos_tty runtime-PM or `samsung,uart-logging` interfere at idle?

---

## B. WPA3-SAE on bcmdhd4383

### B.1 How the pinned driver does SAE

From `google-modules/wlan/bcm4383` at `3526a8cb`:

- `Kbuild:794-803`: `-DWL_CLIENT_SAE` ("Using external supplicant for WPA3"),
  `-DWL_OWE`, `-DWL_SAE_FT`. `WL_SAE` (dongle-offloaded SAE) is set only for
  `CONFIG_CFG80211_SAE_BKPORT`. `wl_cfg80211.h:93-101` makes `WL_SAE` and
  `WL_CLIENT_SAE` mutually exclusive. `WL_SAE_OFFLOAD` and `WL_SAE_STD_API`
  are **not** enabled, so the driver does not advertise
  `NL80211_EXT_FEATURE_SAE_OFFLOAD`.
- So SAE is **host-supplicant SAE over `NL80211_CMD_EXTERNAL_AUTH`**:
  - `wiphy->features |= NL80211_FEATURE_SAE` (`wl_cfg80211.c:13036`).
  - Station mgmt RX includes AUTH frames (`:1914`).
  - Auth TX goes through `assoc_mgr_cmd` (`wl_cfg80211_mgmt_auth_tx`, `:11400`).
  - The firmware `WLC_E_START_AUTH` event triggers
    `cfg80211_external_auth_request()` (`wl_notify_start_auth`, `:26290`).
  - The `.external_auth` op plumbs the PMKID and resumes the association
    (`:26480`).
  - `auth_type NL80211_AUTHTYPE_SAE` maps to `DOT11_SAE` when no PMKID is
    cached (`:4753`).
  - The PMK arrives through `set_pmksa` with `pmksa->pmk` (`:10717`).
  - `-DMFP` is on for 4383 (`Kbuild:957`).
  - H2E: the host RSNXE is plumbed through the `rsnxe` iovar, and the
    multi-AKM path forces `SAE_PWE_LOOP|SAE_PWE_H2E` (`:7339-7360`).
- The driver also advertises:
  - `NL80211_EXT_FEATURE_4WAY_HANDSHAKE_STA_PSK/1X` (`BCMSUP_4WAY_HANDSHAKE`,
    `dhd_use_idsup=1`).
  - `wiphy->max_num_akm_suites = 4` (`:13084`).
  - A **multi-AKM "seamless PSK"** connect mode: when `n_akm_suites > 1`,
    `wl_set_multi_akm()` programs `join_pref` tuples, sets
    `auto_wpa_enabled` (firmware builds the RSN IE and the host IE is not
    plumbed) and `seamless_psk`.
  - It does **not** advertise `NL80211_EXT_FEATURE_MFP_OPTIONAL`.
- This driver is tuned for AOSP's `wpa_supplicant_8` built with
  `CONFIG_DRIVER_NL80211_BRCM`. That build has Broadcom-specific logic:
  - It collapses `key_mgmt` to `SAE|PSK` for "seamless roaming"
    (`wpa_supplicant.c:2240`).
  - It decides the PSK handoff differently (`:4544`).
  - It re-reads the AKM from the firmware-built assoc IE (`events.c:3589`).
  - It inserts the PSK for driver roaming (`events.c:4441`).

  Upstream wpa_supplicant 2.11 (Alpine, `CONFIG_DRIVER_NL80211_QCA=y`, no
  BRCM) uses the generic `#else` branches.

### B.2 What our userspace sends

- **NetworkManager** for `key-mgmt=wpa-psk` sends
  `key_mgmt="WPA-PSK WPA-PSK-SHA256 [FT-PSK] SAE [FT-SAE]"` when the
  supplicant reports SAE+PMF+BIP and pmf is not disabled.
  `nm-supplicant-config.c` has the WPA3 transition-mode comment.
- NM already sets the supplicant globals **`Pmf=1`** and **`SaePwe=2`**
  (H2E and hunting-and-pecking), and has done so since 1.36
  (`nm-supplicant-interface.c`: "enabling SAE-H2E (SaePwe=2)").
  **Setting `sae_pwe=2` by hand is therefore unlikely to change anything.**
  Every wpa_supplicant global is also a D-Bus `Interface` property
  (`dbus_new.c` builds them from the global field table, for example
  `SaePwe` and `Pmf`), so it can be inspected with `busctl get-property`.
- **wpa_supplicant 2.11** in a transition BSS selects SAE, because
  `NL80211_FEATURE_SAE` sets `WPA_DRIVER_FLAGS_SAE`. Then, because the driver
  is not SME and reports `max_num_akms = 4`, `wpas_update_allowed_key_mgmt()`
  adds PSK and PSK-SHA256 as allowed AKMs. `nl80211_connect_common()` then
  sends **several AKM suites** plus `EXTERNAL_AUTH_SUPPORT`, and the PSK/PMK
  as well, because `4WAY_HANDSHAKE_STA_PSK` is set and PSK is among the
  allowed AKMs. That puts bcmdhd into its BRCM-supplicant-specific
  **multi-AKM/seamless-PSK/auto-WPA mode**, a combination upstream
  wpa_supplicant has not been checked against. This is the most likely cause
  of the observed WPA2-works/SAE-fails split. It is a hypothesis to confirm
  with logs, not evidence.
- Because the driver lacks `MFP_OPTIONAL`, `NL80211_ATTR_USE_MFP` is omitted
  for pmf=optional. The driver infers MFP from IEs and `sme->mfp`.

### B.3 Fix options, ranked

1. **Collect the deciding logs first. No code change.**
   - Raise supplicant debug with `busctl set-property fi.w1.wpa_supplicant1
     /fi/w1/wpa_supplicant1 fi.w1.wpa_supplicant1 DebugLevel s msgdump`, and
     set NM `[logging] level=DEBUG domains=WIFI,SUPPLICANT`.
   - Look for:
     - `nl80211: AKM suites` / `num_suites`, and whether connect carried more
       than one AKM;
     - `EXTERNAL_AUTH` / `SME: External authentication request`;
     - `SAE: …` commit/confirm, and the auth/assoc status codes.
   - In dmesg, look for the bcmdhd `WL_INFORM_MEM` lines:
     - `No of akms N`
     - `multi_akm_auth:… selected_akm:…`
     - `call cfg80211_external_auth_request`
     - `auth tx triggered`
     - `External Auth with Peer … Done - Status N`
     - `sae auth failed. force disassoc`
   - Redact BSSIDs/MACs before committing any evidence.
2. **Single-AKM SAE profile (NM configuration).** For WPA3-capable networks,
   create the connection with `802-11-wireless-security.key-mgmt sae` and
   `802-11-wireless-security.pmf required` (NM forces PMF required for `sae`
   anyway). The supplicant `key_mgmt` is then `SAE [FT-SAE]`.
   `wpas_update_allowed_key_mgmt()` adds nothing for SAE without PSK in the
   network block, so connect carries one AKM. The driver takes the classic
   `WL_CLIENT_SAE` external-auth path, the one that does not depend on BRCM
   supplicant behaviour. In Ferrix this is a choice in `ferrix-networkd`'s
   connection builder: when the scan shows SAE in a transition BSS, emit
   `sae` + `pmf=required`, keeping `wpa-psk` as fallback. This needs no kernel
   change and is the cheapest real fix if option 1 confirms the multi-AKM
   hypothesis.
3. **Stop advertising multi-AKM connect to upstream supplicants (driver
   patch).** Add `kernel-patches/private/google-modules/wlan/bcm4383/0002-…`
   so that `wiphy->max_num_akm_suites` stays at the cfg80211 default of 2.
   cfg80211 rejects values below `NL80211_MAX_NR_AKM_SUITES`, so 1 is not
   possible. Alternatively, build with `-DDISABLE_WPAPSK_MULTIAKM` /
   `LEGACY_CROSS_AKM` semantics (`wl_cfg80211.c:7155-7165`) so that
   `skip_seamless_psk` stays TRUE for upstream connects. With 2, upstream
   still sends `[SAE, PSK]`, so the stronger variant forces
   `skip_seamless_psk = TRUE` and single-AKM handling when the
   connect carries `EXTERNAL_AUTH_SUPPORT` from a non-BRCM supplicant. This
   keeps NM's default `wpa-psk` transition behaviour working. It needs a
   rebuild of `bcmdhd4383.ko` only. Rank it below option 2 because it changes
   the behaviour of the Google driver.
4. **Optionally disable firmware 4-way offload for SAE** with
   `bcmdhd4383.dhd_use_idsup=0` (a module parameter, `dhd_linux.c:996`), so
   the host supplicant does the 4-way after external auth. This affects WPA2
   too (it currently works with idsup). Try it only if the logs show SAE
   auth succeeding and the 4-way timing out or getting
   `WLC_E_PSK_SUP`-style failures.
5. **Not recommended:**
   - Switching to `WL_SAE` (dongle-offloaded SAE) needs
     `CONFIG_CFG80211_SAE_BKPORT` semantics and upstream-supplicant
     `SAE_OFFLOAD` support that this driver build does not advertise.
     `WL_SAE_OFFLOAD` exists in the source, but it is gated by
     `idauth_enable` and untested here.
   - Building AOSP's `wpa_supplicant_8` with `CONFIG_DRIVER_NL80211_BRCM`
     replaces a maintained Alpine package and conflicts with the
     "popular, maintained libraries" policy.
   - `sae_pwe`, `pmf` and `key_mgmt` overrides in a supplicant config file:
     NM already sets `SaePwe=2` and `Pmf=1`, and NM-created interfaces do not
     read a config file.

Files a fix would touch:

- Option 2: `crates/ferrix-network-nm` (NM adapter; `crates/ferrix-networkd` if the profile choice lives there) connection
  construction, plus a mock/unit test for the transition-network profile
  choice. It does not touch `networkmanager-ferrix.conf`.
- Option 3/4:
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-patches/private/google-modules/wlan/bcm4383/`
  (new patch), or a `modprobe`/cmdline option in
  `pmos/device-google-tegu/kernel-cmdline-kleaf.conf`
  (`bcmdhd4383.dhd_use_idsup=0`).

---

## References

- AOSP tegu BT overlay: <https://android.googlesource.com/device/google/tegu/+/refs/tags/android-15.0.0_r25/bluetooth/bt_vendor_overlay.conf>
- Nitrous source/binding (pinned): <https://android.googlesource.com/kernel/google-modules/bluetooth/broadcom/+/d7bd4dd23cc52fedd2e36c6e45c12f0c2a595ae0>
- Linux `hci_bcm.c` / `btbcm.c` (GKI copy under `aosp/drivers/bluetooth/` in the kernel workspace).
- AOSP wpa_supplicant_8: <https://android.googlesource.com/platform/external/wpa_supplicant_8/> (`wpa_supplicant/wpa_supplicant.c`, `events.c`, `dbus/dbus_new.c`, `src/drivers/driver_nl80211.c`).
- NetworkManager supplicant config: <https://gitlab.freedesktop.org/NetworkManager/NetworkManager/-/blob/main/src/core/supplicant/nm-supplicant-config.c>, `nm-supplicant-interface.c`.
- Alpine aports 3.24: `main/bluez/APKBUILD`, `main/wpa_supplicant/{APKBUILD,config}`.
