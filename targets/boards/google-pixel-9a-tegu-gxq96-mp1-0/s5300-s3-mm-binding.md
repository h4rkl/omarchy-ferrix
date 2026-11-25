# S5300 S3: ModemManager binding on the Google Kleaf CPIF route (issue #1)

Status: **S3 partial on the exact device.** On
`physical:google-pixel-9a-tegu-gxq96-mp1-0` the modem object is created, but
initialization has failed on an AT-port assumption twice:
- 2026-11-24 with ModemManager r4 (patch 0001), in `initialization_started()`;
  see [r4 result](#physical-result-2026-11-24-0001-only).
- 2026-11-25 with r5 (adds patch 0002), in the Modem interface's primary-port
  step; see [r5 result](#physical-result-2026-11-25-0001--0002-r5).

ModemManager r6 adds patch 0003. It came with a one-pass audit of the whole
init/enable path ([audit](#sit-only-initenable-audit-r6)). A native Alpine
3.24 aarch64 build exists for hot-swapping
([hot-swap](#native-build-and-hot-swap-r6)). A mock run of that build gets
through init to both SIT logical stacks being open. r6 has not yet run on
the device. Whether `mmcli -L` lists a usable modem is the S3 gate in
[`s5300-route-decision.md`](s5300-route-decision.md#stopgo-summary), and it
is still open.

Target: `google-pixel-9a-tegu-gxq96-mp1-0`, route A (Kleaf GKI 6.1.124 with
Google's `cpif`). Route B (`linux-google-tegu-wwan`, `wwan0sit*`) keeps its
behaviour.

## Physical result 2026-11-24 (0001 only)

Run by the lead on `physical:google-pixel-9a-tegu-gxq96-mp1-0`, image
`tegu-kleaf-dev-20261124T231905Z` (main `0a10575`), `modemmanager-samsung-sit`
r4, `device-google-tegu` r9. The redacted `ModemManager --debug` log stays out
of Git (it contained no IMEI, IMSI or ICCID).

- The CP reached `ONLINE` through the S2 path. Before `sit-proxy` started,
  no CPIF device carried an `ID_MM_*` tag and `mmcli -L` listed nothing.
- After `rc-service sit-proxy start`, `umts_ipc0/1` carried exactly the
  expected tags (candidate, device-process, `KERNEL_DRIVER=cp_interface`,
  `PHYSDEV_UID=google-tegu-s5300-cpif`, SIT, proxy, proxy-required, carrier
  directory), and `rmnet0` carried candidate, device-process, driver and
  physdev UID. **The udev gates and the re-trigger worked.**
- **The ModemManager instance already running did not create a modem.** A
  foreground `ModemManager --debug` did; the cause is not established (see
  the test procedure, step 5).
- In that run, all three ports passed the filter as one device and got the
  SIT probe hint. The `samsung-sit` plugin was chosen for all three, and
  `umts_ipc1` finished probing with "no more probings needed", so no AT
  probe was sent. It logged "creating modem with plugin 'samsung-sit' and
  '3' ports" and grabbed `cpif/umts_ipc1`, `cpif/umts_ipc0` and `net/rmnet0`.
- Initialization then failed: `couldn't start initialization: Couldn't open
  ports during modem initialization: Couldn't get primary port`, followed by
  `fatal error initializing: Fatal error: modem is unusable`. No SIT client
  was opened (no "opened Samsung SIT logical stack" line), and there was no
  fallback line. The log shows no attempt to open a CPIF node; the opener
  scan was not part of this report.

### Why it failed (fork `14a0013a`)

- `src/mm-broadband-modem.c`, `initialization_started()`, calls
  `ports_context_open()`. That needs `mm_base_modem_get_port_primary()`, the
  primary **AT** port, and returns "Couldn't get primary port" without one.
  `enabling_started()` opens the same context, so enabling would fail the
  same way.
- `mm-broadband-modem-samsung-sit.c`, `mm_broadband_modem_samsung_sit_class_init()`,
  overrides only `enabling_modem_init` (its comment refers to "the Shannon AT
  service"). The fork assumes the WWAN route's AT port is present.
  `mm_base_modem_organize_ports()` already allows a SIT-only modem without
  an AT port, but the broadband modem it creates cannot start.
- The next fatal step would have been `mm-broadband-modem.c`
  `modem_load_current_capabilities()`, which asks over QCDM or AT. In
  `mm-iface-modem.c`, `load_current_capabilities_ready()` turns a failure
  into `fatal_error`, so fixing only the primary port would have moved the
  failure.
- **Why only `umts_ipc0` was listed as `sit`:** `mm_base_modem_organize_ports()`
  keeps a single `sit` port for its summary and logs it once
  (`log_port (…, sit, "sit")`). Both SIT ports stay grabbed (the shutdown log
  cleans up both), and the plugin opens every SIT port through
  `mm_base_modem_find_ports (…, MM_PORT_TYPE_SIT)`. This is expected.
- **Why it says `wwan/`:** 0001 builds the SIT port with
  `create_wwan_port()`, which sets `MM_PORT_SUBSYS_WWAN`. The port table key
  still uses the kernel subsystem (`cpif/umts_ipc0` when grabbed). The prefix
  is cosmetic.

## Physical result 2026-11-25 (0001 + 0002, r5)

Run by the lead on image `tegu-kleaf-dev-20261125T002336Z`, with the CP
`ONLINE` through S2, `sit-proxy` started, and `ModemManager --debug` in the
foreground. The redacted log stays out of Git.

- The modem was created with `samsung-sit` and the same three ports, and
  0002's `(samsung-sit) no primary AT port: initializing over SIT only`
  appeared.
- It then failed with `modem couldn't be initialized: Primary port
  initialization failed: none found`, and the state went from `unknown` to
  `failed`. `couldn't build device ids: No valid primary port` followed (a
  warning), then `error initializing: Modem in failed state: unknown`.
- 0002's capability line never ran, because that step comes later, and no
  SIT stack was opened.

**Cause.** In `src/mm-iface-modem.c` `interface_initialization_step()`,
`INITIALIZATION_STEP_FIRST` publishes the D-Bus `PrimaryPort`. It accepts
only a QMI, MBIM or primary AT port and otherwise returns that error. The
device-ids warning comes from
`mm-iface-modem-firmware.c` `mm_iface_firmware_build_generic_device_ids()`
and is non-fatal (`load_update_settings_ready()` logs it and continues).

## Why no modem appeared

What the lead saw on 2026-11-24: the CP was `ONLINE`, `exynos-modem` and
`sit-proxy` ran, `/run/sit-proxy/0` and `/1` existed, and a restarted
ModemManager said `No modems were found`. The pinned sources explain that,
and they also show that udev rules alone could not fix it.

### Pinned inputs

| Input | Pin | Verified |
| --- | --- | --- |
| ModemManager fork | `git.deusch.me/pixel-mainline/ModemManager` archive `14a0013a28e5d5eec513c65e75d7549678ecc61e` | sha512 `9add05d1…ccfd8ea` matches `pmos/modemmanager-samsung-sit/APKBUILD` |
| `exynos-modem` / `sit-proxy` | `git.deusch.me/pixel-mainline/exynos-modem` archive `634e8eb404c3fcf959dce6a0b0d97b33ba0fc346` | sha512 `fa081999…ff4357c` matches `pmos/exynos-modem/APKBUILD` |
| Google CPIF driver | `private/google-modules/radio/samsung/s5300` at `a8976934fe7f396627f9900958eb1457a74ebcca` in the Kleaf tree on the build host | read-only `grep`/`sed`; nothing built or written |

### Kernel: what the CPIF nodes look like (CPIF `a8976934`)

- `modem_main.c`, `cpif_cdev_alloc_region()`: `class_create(THIS_MODULE,
  "cpif")`. The FMT nodes are in the **`cpif` subsystem**, not `wwan`.
- `modem_io_device.c`, `cpif_cdev_create_device()`:
  `device_create(iod->msd->cdev_class, NULL, …, "%s", iod->name)`. The
  parent is `NULL`, so `umts_ipc0`/`umts_ipc1` sit under
  `/sys/devices/virtual/cpif/` with **no parent device and no driver**.
- `modem_io_device.c`, `sipc5_init_io_device()`, `IODEV_NET`:
  `alloc_netdev_mqs()` plus `register_netdev()`, with no `SET_NETDEV_DEV()`.
  The `rmnet*` netdevs are also **virtual and driverless**.
- `modem_main.c:980`: the platform driver bound to `samsung,exynos-cp` is
  `cp_interface` (E1 observed it bound).

### ModemManager fork (`14a0013a`): why nothing bound

1. `src/plugins/samsung-sit/mm-plugin-samsung-sit.c`,
   `mm_plugin_create_samsung_sit()`: allowed subsystems `{ "wwan", "net" }`,
   allowed drivers `{ "s5300-modem" }`. ModemManager's udev client watches
   only the union of plugin subsystems (`mm-base-manager.c`, `process_scan()`
   and `g_udev_client_new (mm_plugin_manager_get_subsystems (…))`), so
   **`cpif` devices are never seen**.
2. `src/80-mm-candidate.rules` sets `ID_MM_CANDIDATE` only for `tty`, `net`,
   `cdc-wdm` and `wwan`, and `mm-base-manager.c` `device_added()` drops any
   port without it.
3. `src/mm-plugin.c`, `apply_pre_probing_filters()`: when
   `mm_device_get_drivers()` is `NULL`, the port is "filtered as couldn't
   retrieve drivers". For a parentless device,
   `src/kerneldevice/mm-kernel-device-udev.c` `preload_contents_other()`
   takes the driver from `g_udev_device_get_driver()`, which is `NULL`.
4. `src/mm-base-modem.c`, `base_modem_internal_grab_port()`: only `net`,
   `tty`, `usbmisc`, `rpmsg`, `qrtr`, `virtual` and `wwan` ports can be
   grabbed. Anything else is "unhandled port type".
5. `src/plugins/samsung-sit/mm-sit-client.c`, `mm_sit_proxy_socket_path()`:
   the socket index is the digit after `sit`. Any other name maps to `0`, so
   **`umts_ipc0` and `umts_ipc1` would both go to `/run/sit-proxy/0`**.
6. `mm-sit-client.c`, `mm_sit_client_open()`: if the proxy socket does not
   answer, it warns and **opens `/dev/<node>` directly**. On CPIF the nodes
   exist from module load, before the CP boots. A plain udev tag would
   therefore let ModemManager open `umts_ipc*` at boot, breaking the
   "only `sit-proxy` opens FMT" invariant and the preflight opener scan.

What the udev properties do (same fork):

| Property | Read in | Effect |
| --- | --- | --- |
| `ID_MM_PORT_TYPE_SIT` | `src/mm-port-probe.c`, `ADD_HINT_FROM_UDEV_TAG` | Marks the port as SIT. `mm_port_probe_run()` then presets the AT/QCDM/QMI/MBIM results to FALSE, so nothing is sent ("no destructive probing"). The `wwan` `type` attribute `SIT` does the same on route B. |
| `ID_MM_SIT_PROXY` | `mm-plugin-samsung-sit.c` `create_modem()` → `mm-broadband-modem-samsung-sit.c` `mm_broadband_modem_samsung_sit_peek_client()` | Use `mm_sit_client_new_proxy()`. That connects an AF_UNIX stream to `/run/sit-proxy/<index>` (`mm-port-serial-sit.c` `mm_port_serial_sit_new_socket()`, `MM_PORT_SUBSYS_UNIX`), and falls back to the node (item 6). |
| `ID_MM_SIT_CARRIER_CONFIG_DIRECTORY` | `create_modem()` | Directory holding `cfg.db`, `manifests/` and `confseqs/`. The default is `/vendor/firmware/carrierconfig`. The RFS server must see the same path. |
| `ID_MM_DEVICE_PROCESS` | `src/mm-filter.c` `mm_filter_port()` | Explicit allowlist, checked before the "virtual device" rule. |
| `ID_MM_PHYSDEV_UID` | `mm-kernel-device-udev.c` `kernel_device_get_physdev_uid()` | Ports with the same value become one device (one modem). Without a physdev each parentless port would be its own device. |

The SIT ports are opened in name order, one SIT stack per port
(`compare_sit_ports()`; `mm_sit_client_new_proxy()`). A SIT-only modem is
allowed without an AT port (`mm-base-modem.c` `organize_ports()`:
`if (sit) allow_modem_without_at_port = TRUE`).

### sit-proxy (`634e8eb4`): what each socket carries

- `crates/sit-proxy/src/main.rs`: one listening socket per `--dev`, named by
  its position (`args.socket_dir.join(index.to_string())`).
- On this route `sit-proxy.initd` passes `--kernel-backend google-kleaf-cpif
  --dev /dev/umts_ipc0 --dev /dev/umts_ipc1 --socket-dir /run/sit-proxy`.
  So `/run/sit-proxy/0` is FMT channel 245 (`umts_ipc0`) and
  `/run/sit-proxy/1` is FMT channel 246 (`umts_ipc1`).
- Module doc in `main.rs` and `port.rs`: a socket carries whole SIT frames
  exactly as the node would, with no wrapping and no handshake. The proxy
  routes on the 8-byte header, rewrites the token per client (`mux.rs`), and
  unions `SET_INDICATION_FILTER`. It is the single owner of the node.

## Options considered

- **(a) udev rules only.** Not possible. ModemManager never watches `cpif`
  (item 1), the ports have no driver (item 3), and the core cannot grab them
  (item 4).
- **(b) Bind through the sockets without a kernel device.** ModemManager
  creates modems only from kernel (udev) devices or `mmcli
  --report-kernel-event`. A socket has no udev device to hang a modem on, so
  (b) still needs a device to represent the port. The socket is the right
  *transport*, not a replacement for the device.
- **(c) Chosen: tag the CPIF nodes as SIT ports, patch the fork to accept
  them, and use the sit-proxy sockets as the only transport.**

## Chosen approach

### ModemManager patch: `pmos/modemmanager-samsung-sit/0001-samsung-sit-google-kleaf-cpif-binding.patch`

The patch is hash-pinned in `APKBUILD` `source`/`sha512sums`, and `pkgrel` goes
from 3 to 4. `default_prepare` applies it with `patch -p1`. It changes 9
files (+134/−8 lines) and leaves SIT protocol behaviour alone:

- `mm-kernel-device-udev.c` `preload_contents_other()`: if a parentless device
  reports no driver, use the udev property `ID_MM_KERNEL_DRIVER`. A
  kernel-reported driver always takes precedence.
- `mm-plugin.c`: add `{ "cpif", MM_PORT_PROBE_AT }` to `subsys_flags_map`.
  `apply_pre_probing_filters()` filters a `cpif` port unless udev declared it
  `ID_MM_PORT_TYPE_SIT`. SIT probe results are preset, so the probe finishes
  with "no more probings needed" and sends nothing. The flag matters because
  with no flags every port would be "deferred until suggested", and no plugin
  would ever claim the device.
- `mm-base-modem.c` `base_modem_internal_grab_port()`: grab a `cpif` port
  only when its type is `MM_PORT_TYPE_SIT`. It becomes the same plain SIT
  `MMPort` as on `wwan`.
- `mm-plugin-samsung-sit.c`: watch `cpif` too, and accept the `cp_interface`
  driver. The new `ID_MM_SIT_PROXY_REQUIRED` implies `ID_MM_SIT_PROXY` and
  calls `mm_broadband_modem_samsung_sit_set_proxy_required()`.
- `mm-broadband-modem-samsung-sit.[ch]`: a proxy-required modem builds its
  clients with `mm_sit_client_new_proxy_only()`.
- `mm-sit-client.[ch]`: `mm_sit_client_new_proxy_only()` clears the fallback
  device, so `open()` returns the socket error and **never opens the node**.
  `mm_sit_proxy_socket_path()` maps `umts_ipcN` to `/run/sit-proxy/N`.
- `tests/test-sit-protocol.c`: `/MM/sit/client/proxy-socket-path` covers
  `wwan0sit0/1`, `umts_ipc0/1` and an unknown name.
  `/MM/sit/client/proxy-only-no-fallback` checks that a proxy-only client is
  proxied from the start and keeps the node name. The open-failure path logs
  at warning level, and that test binary has no log backend (it segfaults in
  `_mm_log`, before and after this patch alike), so the missing fallback is
  covered by source review only.

### ModemManager patch 0002: `pmos/modemmanager-samsung-sit/0002-samsung-sit-sit-only-modem-without-at-port.patch`

This patch was added after the physical result above. It is applied after
0001 and hash-pinned in `APKBUILD`, and `pkgrel` goes from 4 to 5. It touches
only `mm-broadband-modem-samsung-sit.c` (+167 lines) and adds no SIT message.
Each override first asks `mm_base_modem_peek_port_primary()`. **With an AT
port, every call goes to the parent unchanged**, so the WWAN route keeps its
behaviour. Without one:

- `initialization_started()` returns no ports context and no error.
  `mm-broadband-modem.c` `initialization_started_ready()` accepts that ("May
  return NULL without error"). `initialization_stopped()` is skipped for a
  NULL context.
- `enabling_started()` returns TRUE without opening or flashing a port.
  `disabling_stopped()` already tolerates a missing `enabled_ports_ctx`.
  This follows `mm-broadband-modem-mbim.c`, which treats the parent's "no
  primary AT port" error as non-fatal in both places.
- `load_current_capabilities()` reports `GSM_UMTS | LTE | 5GNR`. Those are
  the families the plugin already declares: `load_supported_modes()` builds
  2G|3G|4G|5G, and the constructor sets PS, EPS and 5GS network support.
  The Modem interface derives supported capabilities from current
  capabilities when no loader exists. With an AT or QCDM port, the parent
  loader still asks the modem.

Steps that were already non-fatal stay so. Manufacturer, model, revision,
equipment identifier (`STR_REPLY_READY_FN`, logged at debug) and the 3GPP
IMEI (`load_imei_ready()`) still have no SIT source. `setup_ports()` already
skips a missing primary.

Verified offline: 0001 and 0002 apply in order to the pinned tree and
compile with the plugin enabled in Alpine edge. The test results are in the
[2026-11-25 status-log entry](../../../docs/status-log/2026-11.md). There is
no unit test for 0002, because exercising the broadband modem's
initialization needs the daemon. The repository test checks the patch order,
the three overrides, the AT-port guard, and that no SIT message is added.

### ModemManager patch 0003: `pmos/modemmanager-samsung-sit/0003-iface-modem-sit-port-as-primary.patch`

This patch was added after the r5 result. It is applied after 0002 and
hash-pinned in `APKBUILD`, and `pkgrel` goes from 5 to 6. It changes only
`src/mm-iface-modem.c` (+27 lines). When there is no QMI, MBIM or AT
primary, `INITIALIZATION_STEP_FIRST` uses the lowest-named non-ignored SIT
port (`umts_ipc0`), found through `mm_base_modem_find_ports (…,
MM_PORT_TYPE_SIT)`, as the D-Bus primary port. Modems with any other control
port are unaffected. It adds no SIT message.

### SIT-only init/enable audit (r6)

The audit covered every step of the broadband modem's
`initialize_step()`/`enable` sequences and of each interface's init and
enable, for the pinned fork with 0001–0003. A failure ends initialization or
enabling only where `INTERFACE_INIT_READY_FN`/`INTERFACE_ENABLE_READY_FN`
marks the interface fatal: Modem, 3GPP, and CDMA (CDMA is skipped because
the capabilities exclude it). Profile manager, USSD, messaging, voice,
signal, time, location, OMA, SAR, cell broadcast and firmware never stop
the modem.

| Path | Steps that need AT or a primary port | Without an AT port, after 0001–0003 |
| --- | --- | --- |
| Broadband `initialization_started` / `enabling_started` | `ports_context_open()` (primary AT) | 0002: no context, or TRUE |
| Modem init FIRST (`PrimaryPort`) | QMI/MBIM/AT primary, else fatal | 0003: SIT port |
| Modem init CURRENT_CAPABILITIES (fatal) | QCDM/AT loader | 0002: `GSM_UMTS\|LTE\|5GNR` |
| Modem init SUPPORTED_CAPABILITIES (fatal) | no broadband loader; derived from current | ok |
| Charsets, manufacturer, model, revision, hardware revision, equipment and device id, supported/current bands | AT | each logs "No AT port available" and continues |
| Bearers, carrier config, supported modes, IP families, power state, current modes, SIM slots, unlock (fatal), SIM, setup carrier config, own numbers | SIT plugin | SIT (needs CP answers) |
| SIM hot swap | AT ports context | non-fatal |
| `MMBaseSim` init (wait ready, type, eSIM, identifier, IMSI, operator, emergency numbers, preferred networks, EID, removability) | SIT overrides, rest AT | every handler non-fatal |
| 3GPP init (facility locks, IMEI, EPS UE mode, initial EPS bearer settings, 5GNR registration settings) | SIT locks; AT for the rest | all non-fatal; no IMEI |
| Modem enable: power state | SIT | SIT |
| Modem enable: SIM swap check, flow control | AT | non-fatal (`ifc_test_ready()` ignores errors) |
| 3GPP enable (unsolicited and registration events, initial EPS bearer) | SIT (events); AT (EPS bearer) | all non-fatal |
| Firmware device ids | primary port | warning only |
| `setup_ports()`, `disabling_stopped()` | primary optional | already guarded |
| Bearer, call, SMS objects | `MMBaseBearer`/`MMBaseCall`/`MMBaseSms` subclasses | no AT bearer path |

After 0003, nothing in initialization or enabling needs an AT port. The
outcome depends only on the CP's SIT answers: radio state, SIM status (the
fatal unlock check), carrier configuration and registration.

### Native build and hot swap (r6)

`sh tools/pixel-9a/s5300-mm-hotswap.sh build` runs `abuild -F -r` on the
checked-in `exynos-modem` and `modemmanager-samsung-sit` recipes inside
`alpine:3.24` (3.24.4, linux/arm64, which matches the device's
postmarketOS v26.06). It writes the `.apk` files, the extracted
`/usr/sbin/ModemManager` and `/usr/lib/ModemManager/*.so`, `SHA256SUMS` and
a README with the swap commands under the ignored `out/s3-device/build/`.
On 2026-11-25 all three patches applied, and the package's own `check()`
passed 39 of 39 tests.

`… s5300-mm-hotswap.sh sim` is **mock only** and runs those exact apks in a
privileged throwaway container:
- A fake `/sys` and udev database present `cpif/umts_ipc0`,
  `cpif/umts_ipc1` and `net/rmnet0` with the `ID_MM_*` properties seen on
  the device, read by the real udev kernel-device path without udevd.
- `/run/sit-proxy/0` and `/1` are silent listeners: they accept and never
  answer, so no SIT protocol is emulated.

Result:
- The log shows, in order: `no primary AT port: initializing over SIT only`,
  `using SIT port umts_ipc0 as primary`, `reporting the SIT 3GPP
  capabilities`, and `opened Samsung SIT logical stack 0 on umts_ipc0 (via
  sit-proxy)` and `… 1 on umts_ipc1 …`.
- It then sends `GET_RADIO_STATE`, `GET_ALLOWED_NETWORK_TYPE_BITMAP`,
  `SIM_GET_SLOT_STATUS` and `SIM_GET_STATUS`. Each times out after 5 s, and
  the unlock check is fatal, as expected with a CP that never answers.
- `mmcli -m 0` shows plugin `samsung-sit`, drivers `cp_interface`, primary
  port `umts_ipc0`, ports `rmnet0 (net)`, `umts_ipc0 (sit)` and
  `umts_ipc1 (sit)`, and capabilities `gsm-umts, lte, 5gnr`.
- Enabling cannot be exercised this way.

### udev: `pmos/device-google-tegu/99-google-tegu-sit.rules`

The route B lines are unchanged. A new gated block (`ferrix_tegu_cpif_end`)
applies only on `add|change`, only to `cpif`/`net`, only when
`/sys/class/cpif` exists, and only while **both** `/run/sit-proxy/0` and
`/run/sit-proxy/1` exist:

- `umts_ipc[01]` (`cpif`): `ID_MM_CANDIDATE`, `ID_MM_DEVICE_PROCESS`,
  `ID_MM_PHYSDEV_UID=google-tegu-s5300-cpif`,
  `ID_MM_KERNEL_DRIVER=cp_interface`, `ID_MM_PORT_TYPE_SIT`,
  `ID_MM_SIT_PROXY`, `ID_MM_SIT_PROXY_REQUIRED`, and
  `ID_MM_SIT_CARRIER_CONFIG_DIRECTORY=/vendor/firmware/carrierconfig`.
- `rmnet0` (`net`; `ID_MM_CANDIDATE` already comes from `80-mm-candidate.rules`):
  `ID_MM_DEVICE_PROCESS`, the same `ID_MM_PHYSDEV_UID`, and
  `ID_MM_KERNEL_DRIVER=cp_interface`. The three ports form one modem.

No other CPIF node is ever tagged: not `umts_boot0`, `umts_rfs0`, `oem_ipc*`,
`umts_router`, or the other 29 `rmnet*`.

### sit-proxy service: `pmos/device-google-tegu/sit-proxy.initd`

The rules are evaluated when a device appears, and the nodes appear long
before `sit-proxy` runs, so the service re-runs them itself. This applies to
`google-kleaf-cpif` only; on route B both hooks return at once.

- `start_post` waits up to 5 s for both sockets. It then runs `udevadm trigger
  --action=change` for `cpif` `umts_ipc[01]` and for `net` `rmnet0`, followed by
  `udevadm settle --timeout=10`. A synthesised uevent opens no node.
- `stop_post` removes the sockets as before, then re-triggers, so the tags
  disappear and ModemManager drops the ports (`device_added()` →
  not candidate → `device_removed()`).

The pinned SHA-256s in `s5300-cpif-services.sha256` (rules and
`/etc/init.d/sit-proxy`) are regenerated, the `device-google-tegu` sha512sums
are updated, and `pkgrel` goes from 7 to 8. No service is added to any
runlevel. `exynos-modem` and its library pins are untouched.

### Data path grouping

Only `rmnet0` is grouped. The bearer uses
`mm_base_modem_get_best_data_port (…, MM_PORT_TYPE_NET)`
(`mm-bearer-samsung-sit.c`), and the SIT data-call record (`MMSitDataCall`,
`mm-sit-protocol.h`) carries a `cid` but no interface name. Which `rmnetN`
(DT channels 181–210) the CP uses for a given PDN is not in the GPL sources,
so S5 has to establish it. Grouping all 30 would only make the choice
arbitrary.

## What remains unproven

- **r6 on the device.** No step past `initialization_started()` has run on
  hardware. The next unknowns are the plugin's SIT init sequence (radio
  state, SIM status, carrier configuration), whether the CP answers through
  `sit-proxy` at all (U3), and enabling. The audit expects no further
  AT-port failure; the mock run confirms that only up to the first SIT
  answer.
- **Why the boot-time ModemManager ignored the re-triggered ports.** The
  `TEST` gates and the re-trigger did work on the device (2026-11-24).
- **U3:** that the bytes on `umts_ipc0/1` match what `wwan0sit0/1` give
  `sit-proxy`.
- **That the plugin's SIT init sequence works with this CP firmware build**
  (U4 skew, U5 vocabulary).
- **Identity fields.** The plugin does not implement equipment-identifier,
  manufacturer, model or revision loading. They come from generic AT, and the
  plugin docs say that needs the s5300 AT WWAN port. No AT port is tagged on
  CPIF: `umts_router` is the DUN channel 21, and opening it is out of S3
  scope. The fork's message catalogue lists `GET_IMEI` (`0x0401`), but there
  is no request or parser for it. So **expect `mmcli -m 0` to show no
  equipment identifier**. The S3 "IMEI is readable" criterion will probably
  not be met by this patch alone. It is a plugin vocabulary gap to record
  upstream (route decision S4–S7 rule) and must not be guessed in Ferrix.
- **Carrier-profile selection without the AT port** depends on the SIT
  `0x4602` ICCID/IMSI path alone, so MVNO disambiguation by SPN/GID may be
  weaker.
- **The second SIT stack.** `/run/sit-proxy/1` maps to `umts_ipc1` by
  construction, but DSDS behaviour is unexercised.
- **`rmnet0` as the data interface for the default PDN** (S5).
- **How ModemManager behaves when `sit-proxy` stops under it:** socket HUP,
  then port removal from the re-trigger.

## On-device test procedure (lead only)

Prerequisites: an image built through the checked-in builder that contains
`device-google-tegu` r9 or later and `modemmanager-samsung-sit`
1.25.95_git20260731-r6 (`exynos-modem` stays r3). An r5 image can be brought
to r6 with the hot-swap artifacts ([above](#native-build-and-hot-swap-r6))., flashed through the recovery-gated workflow, and a
completed S2 path (preflight, arm, start) with the CP `ONLINE`. Do not start
`sit-proxy` before the CP is `ONLINE`.

1. **Check packages and pins (read-only).**

   ```sh
   apk info -v device-google-tegu modemmanager-samsung-sit exynos-modem
   cd / && sha256sum -c /usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256
   grep -c ferrix_tegu_cpif_end /usr/lib/udev/rules.d/99-google-tegu-sit.rules   # 6
   ls /etc/runlevels/*/ | grep -E 'exynos-modem|sit-proxy|ferrix-s5300-mounts'   # nothing
   ```

2. **Before `sit-proxy` starts, nothing CPIF-side is tagged.**

   ```sh
   udevadm info /sys/class/cpif/umts_ipc0 | grep ID_MM_     # expect no output
   udevadm info /sys/class/net/rmnet0 | grep ID_MM_         # only ID_MM_CANDIDATE=1
   mmcli -L                                                 # No modems were found
   ```

3. **Raise ModemManager logging (no restart needed).**

   ```sh
   mmcli -G debug
   ```

4. **Start sit-proxy (after `exynos-modem` has the CP `ONLINE`).**

   ```sh
   rc-service sit-proxy start
   ls -l /run/sit-proxy/                                    # sockets 0 and 1
   udevadm info /sys/class/cpif/umts_ipc0 | grep ID_MM_
   # expect: ID_MM_CANDIDATE=1, ID_MM_DEVICE_PROCESS=1,
   #   ID_MM_PHYSDEV_UID=google-tegu-s5300-cpif, ID_MM_KERNEL_DRIVER=cp_interface,
   #   ID_MM_PORT_TYPE_SIT=1, ID_MM_SIT_PROXY=1, ID_MM_SIT_PROXY_REQUIRED=1,
   #   ID_MM_SIT_CARRIER_CONFIG_DIRECTORY=/vendor/firmware/carrierconfig
   udevadm info /sys/class/cpif/umts_ipc1 | grep ID_MM_     # same set
   udevadm info /sys/class/net/rmnet0 | grep ID_MM_         # CANDIDATE, DEVICE_PROCESS, PHYSDEV_UID, KERNEL_DRIVER
   ```

   If the tags are missing, run the trigger by hand once and record that:
   `udevadm trigger --action=change --subsystem-match=cpif
   --sysname-match='umts_ipc[01]'; udevadm trigger --action=change
   --subsystem-match=net --sysname-match=rmnet0; udevadm settle`.

5. **Confirm the opener invariant.** Only `sit-proxy` may hold the FMT nodes:

   ```sh
   for p in /proc/[0-9]*; do ls -l "$p/fd" 2>/dev/null | grep -q 'umts_ipc' && echo "${p#/proc/} $(cat "$p/comm")"; done
   # expect exactly one line: <pid> sit-proxy
   ```

6. **S3 gate.** Allow about 10 s for probing. On 2026-11-24 the ModemManager
   started at boot did not pick up the re-triggered ports. If `mmcli -L` is
   still empty after 15 s, record that, then run `rc-service modemmanager
   restart` (or stop it and run `ModemManager --debug` in the foreground, as
   the lead did) and wait again:

   ```sh
   mmcli -L
   # GO shape: /org/freedesktop/ModemManager1/Modem/0 [<manufacturer or unknown>] <model or unknown>
   mmcli -m 0
   ```

   Expected shape (values vary; redact as below):

   ```text
     General  |                path: /org/freedesktop/ModemManager1/Modem/0
              |           device id: <hash>
     Hardware |        manufacturer: --        (probably empty: no AT port)
              |               model: --
              |   equipment id: --              (IMEI gap, see above)
     System   |              device: google-tegu-s5300-cpif
              |             drivers: cp_interface
              |              plugin: samsung-sit
              |        primary port: <not established; there is no primary AT port>
              |               ports: rmnet0 (net), umts_ipc0 (sit), umts_ipc1 (sit)
     Status   |               state: disabled | locked | failed (sim-missing)
     3GPP     |                imei: --
     SIM      |    primary sim path: /org/freedesktop/ModemManager1/SIM/0   (if present)
   ```

   GO for S3 means one modem, `plugin: samsung-sit`, the three ports above,
   a state other than `failed (unknown)`, and a reported SIM state (present
   or missing). With r6 the debug log should show, in order:
   - `no primary AT port: initializing over SIT only`;
   - `no AT, QMI or MBIM port: using SIT port umts_ipc0 as primary`;
   - `no AT or QCDM port: reporting the SIT 3GPP capabilities`;
   - the SIT client lines in step 7, then SIT requests starting with
     `GET_RADIO_STATE`.

   Lines saying "No AT port available to run command" are expected and
   harmless. A fatal error after the first SIT request is the next finding:
   it depends on the CP's answers. Capture it the same way. Record whether the IMEI is
   readable. The expected answer is no; that is the known gap, not a STOP.

7. **Logs to keep.** Collect with `logread` or the service output capture used
   for S2, and `dmesg`. Look for these lines: `opened Samsung SIT logical stack
   0 on umts_ipc0 (via sit-proxy)`, the same for stack 1 on `umts_ipc1`, and the
   absence of `couldn't reach sit-proxy` and `falling back`. If a
   `failed to open serial device` refers to `/run/sit-proxy/N`, it means the
   proxy was not reachable. It must never refer to `/dev/umts_ipc*`.

8. **Stop order (unchanged):** `rc-service sit-proxy stop`. `mmcli -L`
   should then return to `No modems were found`, and the `udevadm info` tags
   from step 4 should be gone. Leave `exynos-modem` as the S2 procedure says;
   a re-arm needs a reboot.

**Redact before committing evidence:** IMEI/IMEISV, IMSI, ICCID, MSISDN/own
numbers, the SIM path's identifiers, operator-specific SPN/GID values if
unusual, cell IDs/TAC/LAC/PCI and any `mmcli --location-get` output, the
`device id` hash, unit serials, and any `sit-proxy --trace` file. A trace
contains subscriber data and must stay out of Git.
