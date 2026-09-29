# Gate checklist and confirmation script

Copy the checklist into chat, filled in from the workflow's own preflight
output, before asking for a go-ahead. Do this for every mutating run
(unlock, sideload, factory restore, temporary boot, persistent flash,
inactive-slot activation/boot, any future erase or slot change), even when the same operation ran earlier in
the session. One go-ahead covers one run.

## Pre-mutation checklist

```text
MUTATION REQUEST - <operation, e.g. bootloader unlock / OTA sideload / factory restore / temp boot / flash --stage all --slot b / inactive-slot boot --slot b>
Workflow: <just recipe and flags exactly as they will be typed>
Preflight: <recipe> -> <passed | refused: verbatim reason>   (if refused, stop here)

Device (from preflight output, shown to you, not to be committed):
  serial            = <exact serial as printed>   (transcripts store sha256 prefix <12 chars>)
  transport         = <adb | fastboot | fastbootd (is-userspace=yes)>
  manufacturer/model= <..> / <..>
  codename/product  = <..>
  hardware_revision = <..>
  sku/variant       = <..>
  bootloader        = <version>, state <locked | unlocked=yes/no>
  verified_boot     = <green | orange | ... | not observed>
  current_slot      = <a | b | _a | _b>, slot_count = <2>
  dynamic_partitions= <true | false>

Operation detail:
  partition_target(s) = <exact list the workflow printed, or "bootloader (fastboot flashing unlock)">
  input image(s)      = <filename> sha256 <..> bytes <..>   (or "none (unlock-only)")
  recovery artifact   = factory <filename> sha256 <..> verified <yes>
                        ota     <filename> sha256 <..> verified <yes>
  temp-boot evidence  = <out/pixel-9a/flash/boot-temp-<stamp>-<pid>.txt | n/a>
  flags that disable a check = <--allow-unsigned-avb (needed for avb_mode=NONE on a secure=yes Pixel) | --skip-temp-boot-gate | --keep-data | none>, why: <..>
  backup/data policy  = <verified readable by owner on <date>, stored outside Git | NOT VERIFIED | not-required-dev-device + owner token>
  gates in manifest   = identity <..> image_compatibility <..> backup <..> data_policy <..> recovery_procedure <..>

Consequences if I proceed:
  - <wipes all user data | keeps data (--keep-data) | no data change>
  - <device will be left in Fastboot | recovery | booted Ferrix (unverified)>
  - <bootloader/radio firmware changes: yes/no>
  - <brick risk: bounded by verified stock recovery archives; a restore has/has not been tested on this device>
  - <slot written/selected: <a|b>; active slot before: <..>; partition writes: yes/no>

Confirmation phrases the workflow will demand (I will type them only after your go-ahead):
  1. "<exact phrase 1>"
  2. "<exact phrase 2>"
  Phone-side prompt: <yes: you must press the volume/power confirmation | no>
```

If any line reads "unknown", "not verified", or the preflight refused, do not
ask for a go-ahead. Report and stop. For guest-only radio state changes, use
`tools/pixel-9a/radios.sh`'s exact device-tree model/compatible preflight and
recheck as the identity fields; there is no storage or partition target, but
the workflow still requires a fresh typed confirmation.

## Confirmation script

Ask in this form, then wait. Do not proceed on silence, on "ok" to a
different question, or on a go-ahead from an earlier run.

> I am ready to run `<recipe and flags>` on the device with serial `<serial>`
> (`<codename>` / `<revision>`, slot `<slot>`, bootloader `<state>`). The
> preflight passed and is pasted above. This will `<consequence in one
> sentence>`. Recovery: `<factory/OTA archive names>` are verified; a stock
> restore `<has / has not>` been tested on this device. Backup: `<status>`.
> Reply "go ahead with <operation>" to authorize this one run. Anything else
> and I stop.

Acceptable authorization: an explicit sentence naming the operation ("go
ahead with the unlock", "yes, sideload it"). Not acceptable: "sure", "do
what you need", a thumbs-up, an authorization given before the checklist was
shown, or an authorization for a different run or device.

## While the workflow runs

- Type each confirmation phrase exactly as the workflow prints it, one at a
  time, when it asks. Never paste both up front, never pipe them.
- If the workflow re-prints identity between phrases (the unlock workflow
  does, after `adb reboot bootloader`), compare it with the checklist. On
  any difference, answer with an empty line so the phrase mismatches and the
  workflow aborts, then report.
- Relay any phone-side prompt to the human; you cannot press it.
- Capture the full stdout/stderr into the ignored `out/` tree, not into the
  repository.

## After the run

Report: recipe, exit code, final `result=` line, where the device is now
(Fastboot / recovery / Android / Ferrix), the evidence file path, and what
this run did not prove. Then update docs per Phase 8 with redacted values
only.
