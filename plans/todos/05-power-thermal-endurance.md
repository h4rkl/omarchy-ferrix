# 05 — Charging, thermal safety and battery endurance

Status: open · Issue: #26 · Depends on: 00, 04 (screen-off and standby
drain) · Feeds: 14

## Goal

Charging is safe, thermal protection is active, low battery shuts the phone
down cleanly, and battery life is measured and acceptable. Today the
`[power]` claim covers only UPower battery and charger reporting.

## Deliverables

1. **Inventory (read-only).** On the Kleaf image, list:
   - the `power_supply` nodes (charger, fuel gauge, USB-PD, wireless if
     present);
   - the thermal zones, trip points, cooling devices and governor;
   - which kernel modules own them.
   Compare with documented stock behaviour from pinned sources. Write it up
   as `power-thermal-source-evidence.md`.
2. **Charging.** Bounded, attended sessions with a ceiling on temperature
   and duration:
   - attach and detach;
   - charge rate reported against what was measured;
   - termination at full;
   - behaviour on a weak or PD charger.
   Show that the kernel and firmware, not Ferrix, own the charging safety
   limits, and that Ferrix doesn't override them. Add an optional charge
   limit (for example 80%) only if the kernel exposes a supported control.
3. **Thermal.**
   - Run a sustained load: browser video, GPU and CPU. Record zone
     temperatures, throttling state and frequency caps.
   - Confirm that critical trips shut the phone down.
   - Surface a typed thermal-warning state to `ferrix-core` and a UI
     indication.
4. **Low battery.**
   - A UPower critical action that triggers a clean OpenRC shutdown before
     the gauge reaches 0.
   - Warnings at 15% and 5%.
   - No false "charging" state.
5. **Endurance.** Measure repeatable intervals, each run at least 3 times,
   with the image hash recorded:
   - screen-on idle;
   - Wi-Fi web use;
   - video playback (hardware VP9);
   - screen-off standby, after spec 04's suspend works;
   - cellular idle, after spec 01.
   Record mW or %/h.
6. **Power policy hooks.** If measurement points to them, apply:
   - Wi-Fi power save;
   - Bluetooth idle;
   - Waydroid freeze when no APK is in front;
   - cpufreq/uclamp policy, shared with spec 10.
   Board-specific values go in the board package, behind existing adapters.

## Acceptance

- The evidence keeps four things separate: battery reporting, charging
  safety, thermal control, and measured endurance.
- Charging and low-battery runs end with no unexpected shutdown, no
  overheating and no false UI state.
- Standby drain is measured with suspend working. Without suspend, the
  standby claim stays open.

## Owner-gated

Unattended overnight charging runs, any charge-control writes, and the
capability evidence change.

## Risks

- Charging and thermal control on Tensor may depend on vendor userspace
  that Android runs. If kernel-only control is incomplete, that is a safety
  NO-GO for unattended charging until it's solved.
