# Pixel 9a development test tools

Host and guest helpers for exercising the Ferrix UI on the exact target
`google-pixel-9a-tegu-gxq96-mp1-0` over the dev route's USB CDC-ACM
bring-up shell (`ttyGS1`, exposed on macOS as
`/dev/cu.usbmodemferrix_tegu3`; override with `FERRIX_SHELL`). They change
no storage, slots or partitions; they run only on a Ferrix dev image.

Host side:

- `shell-run.py 'CMD' [WAIT]` runs one shell command and prints its output.
- `shell-put.py LOCAL REMOTE` pushes a file (base64); verify with
  `sha256sum` through `shell-run.py`.
- `shell-get.py REMOTE LOCAL` fetches a file (gzip+base64, sha256-checked).

Guest side (push to `/tmp`, run as root):

- `fbgrab.py OUT.ppm [SCALE] [--reversed]` reads the scanned-out DRM
  framebuffer (GETFB2 + dumb-buffer map) as PPM. Pass `--reversed` on a
  profile with `scanout_byte_order = "reversed"` to decode the rendered bytes.
  This is the framebuffer evidence AGENTS.md asks for graphical claims.
- `inject.py` creates a uinput touchscreen and keyboard that ferrix-shell
  picks up through libinput, then serves `/tmp/inj.fifo`:
  `tap X Y`, `swipe X1 Y1 X2 Y2 [MS]`, `type TEXT`, `key NAME` (panel
  pixels, portrait 1080x2424).

Typical loop:

```sh
tools/pixel-9a/devtest/shell-put.py tools/pixel-9a/devtest/inject.py /tmp/inject.py
tools/pixel-9a/devtest/shell-run.py '(python3 /tmp/inject.py &) ; sleep 2'
tools/pixel-9a/devtest/shell-run.py 'echo "tap 540 1665" > /tmp/inj.fifo'
tools/pixel-9a/devtest/shell-run.py 'python3 /tmp/fbgrab.py /tmp/fb.ppm 3 --reversed'
tools/pixel-9a/devtest/shell-get.py /tmp/fb.ppm out/pixel-9a/devtest/fb.ppm
```

Keep captures under the ignored `out/` tree; they may show personal data.

## Suspend (opt-in per boot)

`dev/profile.toml` ships `[lifecycle] suspend = "off"`. Two files under
`/run/ferrix` (tmpfs) control ferrix-core's suspend policy. A reboot clears
both, so a hard reset always comes back with suspend off.

```sh
# Opt in for this boot only: s2idle or deep. Then blank with the power key.
tools/pixel-9a/devtest/shell-run.py 'echo s2idle > /run/ferrix/suspend-opt-in'
# Kill switch: while this file exists ferrix-core never suspends.
tools/pixel-9a/devtest/shell-run.py 'touch /run/ferrix/no-suspend'
# Watch the decisions in the core's log.
tools/pixel-9a/devtest/shell-run.py "grep -E 'suspend-(request|resumed|blocked)' /var/log/ferrix-core.log | tail"
```

- The core reads both files whenever the policy changes, and reads them
  again at the deadline.
- The kill switch always wins over the opt-in file and the profile.
- Write the opt-in file before blanking the screen.
- The policy suspends after the screen has been blanked for
  `suspend_after_blank_secs` (30 s) with no inhibitor held. After a
  background wake with the screen still off, it suspends again after
  another delay.
- While Wi-Fi is on, its host-wake IRQ ends s2idle within about 0.3 s, so
  expect short cycles until Wi-Fi wake filtering exists.
- The CDC-ACM shell survived every probe cycle. A cycle that never resumes
  needs the human to hold Volume Down + Power. After that reboot, suspend
  is off again.

The read-only inventory workflows (`just pixel-9a-audio-inventory`,
`just pixel-9a-s5300-source-inventory`) use the same port through
`tools/pixel-9a/acm-probe.py`, which adds exact-byte transfer, a guest-side
hash check, nonce framing, exit status and bounds. Only one process may use
the port at a time.
