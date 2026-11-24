# Pixel 9a power and thermal source evidence (spec 05, deliverable 1)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`).
Spec: [`plans/todos/05-power-thermal-endurance.md`](../../../plans/todos/05-power-thermal-endurance.md)
deliverable 1. Issue #26.

This is a read-only inventory and source comparison. No charge-control,
thermal or cooling-device value was written, and no charging or load session
was run. `capabilities.toml` is unchanged: `[power]` still covers only UPower
battery and charger reporting.

As spec 05 requires, the four questions are kept separate: battery reporting,
charging safety, thermal control and endurance. This document answers the
first three from evidence. It does not measure endurance.

## Verdict

**GO-with-conditions** for bounded, attended charging and thermal sessions
(spec 05 deliverables 2–4). **NO-GO** for unattended or overnight charging, and
for any sustained-load claim, until the conditions below are met.

Kernel-only control is real but incomplete:

- **What the kernel owns:** charger and fuel-gauge drivers, battery-temperature
  soft JEITA, the per-temperature CC/CV charge table, charge termination,
  battery current limiting (BCL), the PMIC hardware thermal shutdown on
  `north_therm`, and SoC throttling in the ACPM firmware governor. All of this
  runs without Android.
- **What Android userspace owns:** on stock, the Pixel thermal HAL owns every
  skin-temperature policy. That covers CPU, GPU and TPU caps from virtual skin
  sensors, skin-temperature charge-current throttling through the `fcc` and
  `dc_icl` cooling devices, USB-port overheat protection through `usbc-port`,
  and the battery 60 °C shutdown severity. Ferrix runs none of these, and the
  inventory shows every cooling device at `cur_state=0`.

### Conditions for a full GO

1. A project-owned, typed thermal policy in the board package. It must
   reproduce the safety-relevant stock policies, using the stock thresholds
   below as the reference:
   - charge-current throttling on skin temperature through `fcc` and `dc_icl`;
   - `usbc-port` on USB connector temperature (`usb_pwr_therm`);
   - a clean shutdown at battery 60 °C;
   - skin-temperature CPU, GPU and TPU caps.

   The policy must write only the documented cooling-device `cur_state` and
   must never raise a kernel limit. Spec 05 owner-gates charge-control and
   cooling-device writes.
2. Exact-target evidence that the policy acts. Under a bounded load and a
   bounded charge, show that cooling states change and that temperatures stay
   within the stock thresholds.
3. A UPower critical action that triggers a clean OpenRC shutdown (spec 05
   deliverable 4). Stock Android performs this from framework and health HAL
   userspace, not from the kernel.

Until then, attended sessions must keep a ceiling on temperature and duration,
as spec 05 already requires. Stop at skin or battery temperature ≥ 43 °C,
which is the stock `VIRTUAL-SKIN-CHARGE` second step.

## Physical evidence

Label: `physical:google-pixel-9a-tegu-gxq96-mp1-0`, read-only inventory taken
2026-11-24 on the booted Ferrix Kleaf dev route. The kernel was
`6.1.124-android14-11-ga4662a84a15f`. The phone was on a USB-PD source at 100%.
The raw transcripts are kept out of Git as ignored
`out/pixel-9a/inventory-20261124.txt` and `inventory3-20261124.txt`, with no
unique identifiers.

### `power_supply` nodes and owners

| Node | Observed (redacted) | Owning driver (`offline`, pinned source) |
| --- | --- | --- |
| `battery` | `type=Battery status=Full capacity=100 health=Good temp=300` (30.0 °C), `charge_full=5215000`, `charge_full_design=5120000`, plus Google extensions (`charge_limit`, `charge_deadline`, `aacr_*`, `bd_trickle_*`, `health_*`) | `google_battery.c` `.psy_dsc.name = "battery"` ([`bms` `183509af`](https://android.googlesource.com/kernel/google-modules/bms/+/183509aff980610401869deca9f6ce8836980fe7/google_battery.c)) |
| `max77779fg` | `status=Full capacity=99 temp=300` | `max77779_fg.c` (`max77779_fg_psy_desc.psy_dsc.name = "max77779fg"`), the fuel gauge at I2C `6-0036` |
| `main-charger` | `online=1 status=Not charging` (full) | `max77779_charger.c`, named by DT `max77779,psy-name = "main-charger"` ([`zuma-max777xx.dtsi` `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-max777xx.dtsi)), I2C `6-0069` |
| `gccd` | `online=1 status=Not charging` | `google_ccd.c` `.psy_dsc.name = "gccd"` |
| `rt9471` | `online=1 health=Good`, `constant_charge_current_max=0` | `rt9471_charger.c`, the `richtek,swchg` buck charger at I2C `10-005b` ([`zumapro-buck-charge-rt9471.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zumapro-buck-charge-rt9471.dtsi)) |
| `dc` | `online=0 present=0` | `max77779_charger.c` `.psy_dsc.name = "dc"` |
| `usb` | `online=1`, `usb_type=Unknown SDP [CDP] DCP`, `input_current_limit=0` | `usb_psy.c` (`soc/gs` `20407875`, `drivers/usb/typec/tcpm/google/usb_psy.c`) |
| `tcpm-source-psy-i2c-max77759tcpc` | `type=USB`, `usb_type=C [PD] PD_PPS`, 5 V / 3 A | GKI TCPM (`drivers/usb/typec/tcpm/tcpm.c` `tcpm_psy_name_prefix`, `a4662a84`) with `tcpci_max77759` |
| `wireless` | `online=0 present=0` | `p9221_charger.c` `.psy_dsc.name = "wireless"`, the `idt,p9222` at I2C `10-0061` |

Loaded power modules include `google_battery`, `google_charger`, `google_cpm`,
`google_ccd`, `google_bms`, `google_bcl`, `gvotable`, the `max77779_*` family,
`rt9471_charger`, `ln8411`, `pca9468`, `p9221`, `tcpci_max77759`,
`google_tcpci_shim` and `usbc_cooling_dev`.

During boot, dmesg repeats `max77759tcpc ... TCPCI: GBMS_MODE_VOTABLE get
failed: 0` about 40 times between 6.7 s and 9.7 s. It stops once
`google_bms: initialize gbms_storage` runs at 10.2 s. This looks like a
module probe-order race, not a charging failure. It is recorded here and not
investigated.

### Thermal zones

All zones report `policy=step_wise` (see the Next read-only checks below).

| Zones | Mode | Trips (m°C) | Owner |
| --- | --- | --- | --- |
| `BIG`, `MID`, `LITTLE`, `G3D`, `ISP`, `TPU`, `AUR` (zones 0–6) | **disabled** | 8 trips each; for example `BIG`: active 20000/65000/93000/70000/97000/100000, passive 90000, hot 112000 | `gs_tmu_v3.c`. dmesg at 2.6 s shows `failed to read out thermal zone (-22)` and `Disable tz_id: N thermal genl event` for zones 0–6. |
| `rfpa_therm`, `quiet_therm`, `charging_therm`, `usb_pwr_therm`, `btmspkr_therm`, `disp_therm`, `soc_therm` | enabled | passive 125000 only, a placeholder | `s2mpg15_spmic_thermal.c` (NTC ADC). Each logs `Failed to set trips: -11`. |
| `north_therm` | enabled | passive 60000, **critical 64700**, **hot 66700** | `s2mpg15_spmic_thermal.c`. dmesg: `Set sensor 6 hot trip(mdegC):66700, ret:0`. |
| `battery`, `max77779fg`, `batt_vs` | enabled / disabled | none | battery and fuel-gauge temperature |
| `battery_cycle`, `soc`, `mdis_morepower`, `thb_hda` | mixed | pseudo-zones | Google BMS and metrics |
| `cp_on_chip_0..3`, `rf1_therm`, `rf2_therm` | disabled, no temperature | passive 125000 | `cp_thermal_zone` (S5300). They read empty while the CP is not running. |

### Cooling devices

All 29 cooling devices read `cur_state=0`.

| Cooling device | Max state | Owner (`offline`) |
| --- | --- | --- |
| `BIG-tj` … `AUR-tj` | 0 | `gs_tmu_v3.c` (`"%s-tj"`) |
| `thermal-cpufreq-0/1/2` | 9 / 16 / 17 | `exynos_cpu_cooling.c` |
| `thermal-gpufreq-0` | 13 | `gpu_cooling.c` |
| `thermal-uclamp-0/4/7` | 9 / 16 / 17 | `cdev_uclamp.c` |
| `MIF`, `INT`, `INTCAM`, `DISP`, `CAM`, `TNR`, `MFC`, `BW`, `DSU` | 6–16 | Exynos devfreq cooling |
| `BCI` | 13 | `google_bcl` (battery current limiting) |
| `fcc`, `dc_icl` | 26 / 26 | `google_charger.c` (`FCC_CDEV_NAME`, `WLC_CDEV_NAME`) |
| `usbc-port` | 1 | `usbc_cooling_dev.c` |
| `tpu_cooling`, `gxp-cooling` | 7 / 6 | EdgeTPU and GXP drivers |

### Other power surfaces

- `/sys/power/state` reads `freeze mem disk`, and `mem_sleep` reads
  `s2idle [deep]`. Suspend is spec 04's to prove.
- All three cpufreq policies use `schedutil`.
- The backlight is `panel0-backlight`, `max_brightness=3939`, brightness 1829
  at capture time.
- `iio:device0` and `iio:device1` are the `s2mpg15-odpm` and `s2mpg14-odpm`
  on-device power meters, with per-rail `in_energyN_mean_raw`. They are the
  natural source for spec 05's mW endurance figures, read-only.

## Offline source evidence

Label: `offline`. The sources are pinned by
[`kernel-sources.toml`](kernel-sources.toml) (manifest `893fbfd2`), plus the
hash-verified factory `vendor.img` (SHA-256 `41c5d717…9bf7`). The image was
listed read-only in a scratch directory outside the repository, and no bytes
are committed.

### Charging safety owned by the kernel

- **Soft JEITA:** `msc_logic_soft_jeita()` in
  [`google_battery.c`](https://android.googlesource.com/kernel/google-modules/bms/+/183509aff980610401869deca9f6ce8836980fe7/google_battery.c)
  stops charging outside the battery temperature window and logs
  `MSC_JEITA temp=… ok, enabling charging` on recovery.
- **CC/CV table by temperature:** `gbms_msc_temp_idx()` in
  [`google_bms.c`](https://android.googlesource.com/kernel/google-modules/bms/+/183509aff980610401869deca9f6ce8836980fe7/google_bms.c)
  selects the CC/CV row by temperature. For the MP board, the final include
  [`zuma-tegu-battery-evt.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-battery-evt.dtsi)
  sets `google,chg-temp-limits = <0 100 200 440 460 480 550>` (deci-°C, so
  0–55 °C). Its `google,chg-cc-limits` falls to 0 at the high-voltage tiers
  above 44 °C, and to 30 % / 0 above 48 °C.
  [`zuma-tegu-battery.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-battery.dtsi)
  sets `google,chg-cv-limits = <4200000 … 4500000>`.
- **Supported charge limit:** `google_charger.c` exposes `charge_stop_level` and
  `charge_start_level` (`DEVICE_ATTR(charge_stop_level, 0660, …)`,
  `DEFAULT_CHARGE_STOP_LEVEL 100`). `set_charge_stop_level()` rejects values at
  or below `charge_start_level` and values above 100. This is the
  kernel-supported control that spec 05's optional 80 % limit would use. Writing
  it is owner-gated.

### Thermal control: kernel and firmware side

- **SoC TMU zones are disabled by design.**
  [`gs_tmu_v3.c` `gs_tmu_probe()`](https://android.googlesource.com/kernel/google-modules/soc/gs/+/204078758737776959ecc85c527a00fd2cabb62f/drivers/thermal/samsung/gs_tmu_v3.c)
  registers each zone and then calls `thermal_zone_device_disable(data->tzd)`.
  It hands the PI loop (`PI_LOOP`), early throttle (`EARLY_THROTTLE`) and
  hard-limit parameters to the ACPM firmware governor through
  `exynos_acpm_tmu_ipc_set_pi_param(…, GOV_SELECT, …)`. When
  `acpm_gov_common.turn_on` is set, the AP interrupt is disabled. So
  `mode=disabled` for `BIG`…`AUR` is the stock design, not a Ferrix fault.
  SoC throttling runs in ACPM firmware whether or not Linux userspace exists.
  The SoC hardware-shutdown path is not visible in the AP source and is
  recorded as unverified.
- **Board thermistors:**
  [`zuma-tegu-thermal.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-thermal.dtsi)
  declares every `zuma_tm1` NTC zone with `thermal-governor = "user_space"` and
  `polling-delay = <0>`. On stock, userspace is expected to act on them.
  `north_therm` carries `backup-shutdown-sw` (critical, 64.7 °C) and
  `backup-shutdown-hw` (hot, 66.7 °C).
- **PMIC hardware shutdown:**
  [`s2mpg15_spmic_thermal.c`](https://android.googlesource.com/kernel/google-modules/soc/gs/+/204078758737776959ecc85c527a00fd2cabb62f/drivers/thermal/google/s2mpg15_spmic_thermal.c)
  programs only `THERMAL_TRIP_HOT` as the PMIC hardware shutdown threshold
  (`s2mpg15_spmic_thermal_set_hot_trip()`). The physical dmesg confirms this
  for sensor 6 (`north_therm`), so the 66.7 °C hardware backstop exists
  without Android. The 64.7 °C critical trip is a software trip. The GKI
  thermal core acts on it with an orderly shutdown only when the zone is
  updated. These zones have `polling-delay = <0>`, and this boot logged
  `Failed to set trips: -11` for each of them. Whether the software trip is
  ever evaluated is unverified, so treat the PMIC hot trip as the only proven
  backstop.

### Stock Android userspace that Ferrix does not run (factory `vendor.img`)

| Component | Stock role |
| --- | --- |
| `/bin/hw/android.hardware.thermal-service.pixel` with `/etc/thermal_info_config.json` | Defines virtual skin sensors and binds them to cooling devices. `VIRTUAL-SKIN` (39/43/45/46.5/52/55 °C) drives `gxp-cooling`. `VIRTUAL-SKIN-SOC` drives `thermal-cpufreq-0..2`, `thermal-gpufreq-0` and `tpu_cooling`. `VIRTUAL-SKIN-CPU-*` drives `thermal-cpufreq-*` and `thermal-uclamp-*`. `VIRTUAL-SKIN-CHARGE` (34/38/41/45/47/55 °C) drives **`fcc`**. `VIRTUAL-SKIN-CHARGE-WLC` drives **`dc_icl`**. `VIRTUAL-USB-THROTTLING` drives **`usbc-port`**. `battery` has a SHUTDOWN severity at **60 °C**. `thermal_info_config_charge.json` keeps the charge and USB subset for off-mode charging. |
| `/bin/hw/android.hardware.health-service.zumapro` (`--charger` for off-mode charging), `libpixelhealth.so` | Battery health reporting and charger mode. Low-battery shutdown is an Android framework decision. |
| `/bin/hw/vendor.google.google_battery-service` and its rc file | Owns the `battery` longevity sysfs (`aacr_*`, `aafv_*`, `aact_*`, `bd_trickle_*`, `charge_deadline`, `health_*`). These are adaptive-charging and longevity features, not safety limits. |
| `/bin/battery_mitigation`, `vendor.google.battery_mitigation-default.rc` | BCL mitigation userspace. The `BCI` kernel cooling device and the `google_mitigation` driver exist without it. |

## Comparison: stock vs Ferrix today

| Function | Stock | Ferrix Kleaf route (2026-11-24) |
| --- | --- | --- |
| Battery and charger reporting | Kernel, plus health HAL | Kernel nodes present. UPower reporting is already the `[power]` claim. |
| Battery-temperature charge limits (JEITA, CC/CV) | Kernel `google_battery` | Same kernel code is running |
| Charge termination at full | Kernel | Observed `status=Full` and `Not charging` at 100 % while plugged in. A single sample, not a charge session. |
| Skin-temperature charge throttling (`fcc`, `dc_icl`) | Thermal HAL | **Not running.** `cur_state=0` |
| USB connector overheat (`usbc-port`) | Thermal HAL | **Not running** |
| CPU, GPU and TPU skin caps | Thermal HAL | **Not running.** ACPM SoC junction throttling still applies. |
| Battery 60 °C shutdown | Thermal HAL | **Not running.** Only the JEITA stop of charging applies. |
| `north_therm` 66.7 °C hardware shutdown | PMIC | Present (hot trip programmed, `ret:0`) |
| `north_therm` 64.7 °C orderly shutdown | Kernel thermal core | Declared, but evaluation is unverified (no polling, `set_trips` returned -11) |
| Low-battery clean shutdown | Framework | **Not implemented.** Spec 05 deliverable 4. |

## Next read-only checks

For the owner to run on the booted Ferrix dev route. All of them read sysfs
or the kernel log and write nothing.

```sh
ls /sys/devices/platform/google,charger/
cat /sys/devices/platform/google,charger/charge_stop_level /sys/devices/platform/google,charger/charge_start_level
for z in /sys/class/thermal/thermal_zone*; do echo "$(cat $z/type) policy=$(cat $z/policy) avail=$(cat $z/available_policies 2>/dev/null)"; done
find /sys/devices/platform -maxdepth 3 -name acpm_gov_select 2>/dev/null | while read f; do echo "$f $(cat $f)"; done
cat /sys/class/power_supply/battery/charge_limit /sys/class/power_supply/battery/charging_policy /sys/class/power_supply/battery/charge_stage
cat /sys/bus/iio/devices/iio:device0/enabled_rails /sys/bus/iio/devices/iio:device1/enabled_rails
dmesg | grep -E 'MSC_|JEITA|google_charger|google_battery|gbms' | tail -60
```

These checks answer the following:

- The `google,charger` path and the current stop and start levels, before any
  owner-gated charge-limit work.
- Why the zones report `step_wise` although the DT asks for `user_space`. The
  answer decides whether a Ferrix policy should set `user_space` for the
  thermistor zones or read them by polling.
- Whether ACPM governor selection is active on this boot.
- Which ODPM rails can back spec 05 endurance figures.
