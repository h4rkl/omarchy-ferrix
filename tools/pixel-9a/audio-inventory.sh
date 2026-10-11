#!/bin/sh
set -eu

# Read-only audio inventory for a Google Pixel 9a (tegu / GXQ96 / MP1.0) that
# is ALREADY running a Ferrix dev image on the postmarketOS substrate, reached
# over its USB CDC-ACM bring-up shell (issue #52; the rootfs has no sshd). It
# uses the exact device-tree identity and redaction rules of radios.sh and
# guest-evidence.py: one tools/pixel-9a/acm-probe.py session sends the exact
# bytes of the checked-in guest probe (audio-inventory-remote.sh), whose first
# action is the exact tegu device-tree gate. No Android tooling, storage, slot,
# bootloader, module, sysfs, mixer, stream, or service change is made.
#
#   tools/pixel-9a/audio-inventory.sh inventory
#
# Physical execution is gated by issue #25: do not run this against the phone
# until the stock restore and recovery proof are re-established on the host.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained.
#
# Evidence policy: the redacted transcript and audio-evidence-latest.tsv are
# written under ignored out/pixel-9a/audio/ and every line carries
# `physical: google-pixel-9a-tegu-gxq96-mp1-0`. Stub-transport tests set
# FERRIX_PIXEL9A_AUDIO_EVIDENCE=mock, which only downgrades the label to mock.
# Every row stays `unsupported`: enumeration is not playback or capture
# evidence, and this script never edits capabilities.toml.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
remote_probe="$root/tools/pixel-9a/audio-inventory-remote.sh"
transport="$root/tools/pixel-9a/acm-probe.py"
radios="$root/tools/pixel-9a/radios.sh"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/audio"
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
max_output=1048576
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-180}

case "${FERRIX_PIXEL9A_AUDIO_EVIDENCE:-physical}" in
  physical) evidence_label="physical: $target_id" ;;
  mock) evidence_label="mock: stubbed transport for $target_id (not physical evidence)" ;;
  *) printf 'FAIL pixel-9a-audio: FERRIX_PIXEL9A_AUDIO_EVIDENCE must be physical or mock\n' >&2; exit 2 ;;
esac

usage() {
  cat >&2 <<'USAGE'
usage: tools/pixel-9a/audio-inventory.sh inventory

  inventory   verify the exact tegu guest identity, then record ALSA cards and
              PCMs, audio modules/bindings/probe and firmware failures, UCM and
              ALSA tool availability, the PipeWire/WirePlumber graph, and the
              Ferrix audio adapter state; writes a redacted transcript and
              audio-evidence-latest.tsv under out/pixel-9a/audio/

Physical execution is gated by issue #25 (stock restore/recovery proof).

environment: FERRIX_SHELL (CDC-ACM node, /dev/cu.usbmodemferrix_tegu3)
             FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, 180) FERRIX_OUT_DIR (out)
USAGE
  exit 2
}

die() {
  code=${2:-1}
  printf 'FAIL pixel-9a-audio: %s\n' "$1" >&2
  printf '{"event":"pixel-9a-audio-inventory","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
    "$evidence_label" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
  exit "$code"
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing host tool '$1'; install it explicitly"
}

# radios.sh owns the transcript redactor (MAC, IMEI/IMSI/ICCID, serial, SSID,
# IPv6). PipeWire spells Bluetooth MACs with underscores in node names and
# exposes session cookies and host names, so normalise those first.
redact() {
  sed -e 's/\([0-9A-Fa-f][0-9A-Fa-f]\)_\([0-9A-Fa-f][0-9A-Fa-f]\)_\([0-9A-Fa-f][0-9A-Fa-f]\)_\([0-9A-Fa-f][0-9A-Fa-f]\)_\([0-9A-Fa-f][0-9A-Fa-f]\)_\([0-9A-Fa-f][0-9A-Fa-f]\)/\1:\2:\3:\4:\5:\6/g' \
    -e 's/\([Cc]ookie[:=]* *\)[0-9A-Fa-fx:]*/\1<redacted-cookie>/g' \
    -e 's/\(Host Name: \).*/\1<redacted-host>/' \
    -e 's/\(\[[^],]*, \)[^],@]*@[^],]*\(, \)/\1<redacted-user-host>\2/' |
    "$radios" redact
}

# The only guest session in this workflow. The transport sends the probe's
# exact bytes, runs them only if the guest-side SHA-256 matches, merges guest
# stderr into the framed output, bounds time and output, and exits with the
# probe's status (255: transport failure, with one sanitised reason token on
# stderr).
remote() {
  python3 "$transport" --probe "$remote_probe" --timeout "$acm_timeout" --max-bytes "$max_output" --stderr merge 2> "$transport_log"
}

field() {
  value=$(printf '%s\n' "$probe_out" | sed -n "s/^$1=//p" | head -n 1)
  printf '%s\n' "${value:-unknown}"
}

# Treat a missing or non-numeric count as unknown so absence is never zero.
count() {
  case "$1" in
    ''|*[!0-9]*) printf 'unknown\n' ;;
    *) printf '%s\n' "$1" ;;
  esac
}

gt0() {
  case "$1" in
    ''|*[!0-9]*) return 1 ;;
    *) [ "$1" -gt 0 ] ;;
  esac
}

inventory() {
  need python3
  need awk
  need sed
  [ -x "$radios" ] || die "missing redactor: $radios"
  [ -f "$remote_probe" ] && [ ! -L "$remote_probe" ] || die "guest probe is missing or linked: $remote_probe"
  [ -f "$transport" ] && [ ! -L "$transport" ] || die "CDC-ACM transport is missing or linked: $transport"
  case "$acm_timeout" in ''|*[!0-9]*) die "FERRIX_PIXEL9A_ACM_TIMEOUT is malformed" 2 ;; esac
  [ "$acm_timeout" -ge 1 ] && [ "$acm_timeout" -le 3600 ] || die "FERRIX_PIXEL9A_ACM_TIMEOUT is out of range" 2

  mkdir -p "$out"
  transport_log=$(mktemp "${TMPDIR:-/tmp}/pixel-9a-audio-transport.XXXXXX")
  trap 'rm -f "$transport_log"' EXIT HUP INT TERM
  stamp=$(date -u +%Y%m%dT%H%M%SZ)
  transcript="$out/inventory-$stamp.txt"
  {
    printf '# Google Pixel 9a audio inventory, %s UTC, host %s\n' "$stamp" "$(uname -s) $(uname -m)"
    printf '# evidence: %s\n' "$evidence_label"
    printf '# transport cdc-acm, guest port redacted; probe sha256 %s\n' \
      "$( (shasum -a 256 "$remote_probe" 2>/dev/null || sha256sum "$remote_probe") | cut -c1-64)"
    printf '# read-only: no stream, mixer, module, sysfs, service, storage, or slot change.\n'
    printf '# MAC, IMEI/IMSI/ICCID, serial, SSID, IPv6, cookie, and host values are redacted. Never commit this file.\n'
  } > "$transcript"

  probe_status=0
  probe_out=$(remote) || probe_status=$?
  printf '%s\n[exit %s]\n' "$probe_out" "$probe_status" | redact >> "$transcript"
  case "$probe_status" in
    0) ;;
    255) reason=$(sed -n 's/^ferrix-acm: .* reason=\([a-z-]*\)$/\1/p' "$transport_log" | sed -n '1p')
         printf '[transport cdc-acm failed: %s]\n' "${reason:-unknown}" >> "$transcript"
         die "guest CDC-ACM shell transport failed (${reason:-unknown}); transcript $transcript" 3 ;;
    5) printf 'guest_identity=refused-qemu\n'
       die "guest is a QEMU virt machine; QEMU provides no physical audio evidence" 5 ;;
    4) printf 'guest_identity=mismatch\n'
       die "guest device tree is not the exact '$dt_model' / '$dt_compatible' board; refusing" 4 ;;
    *) die "guest audio probe failed (exit $probe_status); transcript $transcript" 1 ;;
  esac

  # Re-verify what the guest reported; a probe that skipped its own gate or a
  # truncated session must not yield target evidence.
  [ "$(field probe)" = read-only ] && [ "$(field probe_version)" = 1 ] ||
    die "guest probe did not report the reviewed read-only probe version" 4
  [ "$(field identity)" = verified ] && [ "$(field dt_model)" = "$dt_model" ] &&
    [ "$(field dt_compatible)" = "$dt_compatible" ] ||
    die "guest probe output does not carry the exact verified tegu identity" 4
  [ "$(field probe_complete)" = yes ] || die "guest audio probe output is incomplete" 1
  printf 'guest_identity=verified\n'

  alsa_proc=$(field alsa_proc)
  cards=$(count "$(field alsa_cards)")
  pcm_total=$(count "$(field alsa_pcm_total)")
  pcm_play=$(count "$(field alsa_pcm_playback)")
  pcm_cap=$(count "$(field alsa_pcm_capture)")
  snd_nodes=$(field dev_snd_nodes)
  snd_play=$(count "$(field dev_snd_pcm_playback)")
  snd_cap=$(count "$(field dev_snd_pcm_capture)")
  if [ "$alsa_proc" != present ]; then
    alsa_progress=alsa-proc-interface-unavailable
  elif [ "$cards" = unknown ]; then
    alsa_progress=alsa-card-count-unknown
  elif [ "$cards" -eq 0 ]; then
    alsa_progress=no-alsa-cards
  elif gt0 "$pcm_play" && gt0 "$pcm_cap"; then
    alsa_progress=alsa-playback-and-capture-pcms-enumerated
  elif gt0 "$pcm_play"; then
    alsa_progress=alsa-playback-pcm-only
  elif gt0 "$pcm_cap"; then
    alsa_progress=alsa-capture-pcm-only
  else
    alsa_progress=alsa-cards-without-pcms
  fi
  alsa_evidence="proc_asound=$alsa_proc cards=$cards pcms=$pcm_total playback=$pcm_play capture=$pcm_cap dev_snd=$snd_nodes dev_snd_playback=$snd_play dev_snd_capture=$snd_cap; enumeration only, no PCM opened"

  modules=$(count "$(field audio_modules)")
  dt_nodes=$(count "$(field audio_dt_nodes)")
  dt_bound=$(count "$(field audio_dt_bound)")
  dt_unbound=$(count "$(field audio_dt_unbound)")
  dmesg_access=$(field dmesg_access)
  dmesg_fail=$(count "$(field dmesg_audio_failure_lines)")
  deferred=$(count "$(field deferred_audio_devices)")
  snd_config=$(field kernel_config_snd)
  if gt0 "$dmesg_fail" || gt0 "$dt_unbound" || gt0 "$deferred"; then
    kernel_progress=audio-probe-failures-or-unbound-nodes-observed
  elif gt0 "$modules" || gt0 "$dt_nodes"; then
    if [ "$dmesg_fail" = unknown ]; then
      kernel_progress=audio-kernel-surface-present-probe-log-unavailable
    else
      kernel_progress=audio-kernel-surface-present-no-probe-failure-logged
    fi
  elif [ "$modules" = unknown ] || [ "$dt_nodes" = unknown ]; then
    kernel_progress=kernel-audio-surface-unknown
  else
    kernel_progress=no-audio-kernel-surface
  fi
  kernel_evidence="config=$snd_config audio_modules=$modules dt_nodes=$dt_nodes bound=$dt_bound unbound=$dt_unbound deferred=$deferred dmesg=$dmesg_access dmesg_audio_lines=$(count "$(field dmesg_audio_lines)") failure_lines=$dmesg_fail; module/regex match is name-based and unpinned"

  fw_files=$(count "$(field audio_firmware_files)")
  fw_fail=$(count "$(field dmesg_audio_firmware_failures)")
  if gt0 "$fw_fail"; then
    fw_progress=audio-firmware-load-failures-logged
  elif [ "$fw_fail" = unknown ]; then
    fw_progress=audio-firmware-log-unavailable
  elif gt0 "$fw_files"; then
    fw_progress=audio-firmware-files-present-no-load-failure-logged
  else
    fw_progress=no-audio-firmware-files-found
  fi
  fw_evidence="audio-named firmware files=$fw_files firmware failure lines=$fw_fail (dmesg=$dmesg_access); files are not hash-verified against a pinned audio manifest"

  ucm_dir=$(field ucm_dir)
  ucm_conf=$(count "$(field ucm_conf_files)")
  ucm_matched=$(count "$(field ucm_matched_card_profiles)")
  if [ "$ucm_dir" = unavailable ] || [ "$ucm_dir" = unknown ]; then
    ucm_progress=ucm-data-unavailable
  elif [ "$cards" = unknown ]; then
    ucm_progress=ucm-data-present-card-state-unknown
  elif ! gt0 "$cards"; then
    ucm_progress=ucm-data-present-no-alsa-card
  elif gt0 "$ucm_matched"; then
    ucm_progress=ucm-card-profile-present
  else
    ucm_progress=ucm-data-present-no-card-profile
  fi
  tools=""
  for tool in aplay arecord amixer alsactl alsaucm speaker_test wpctl pw_cli pw_dump pactl pipewire wireplumber pipewire_pulse socat; do
    tools="$tools $tool=$(field "tool_$tool")"
  done
  ucm_evidence="ucm_dir=$ucm_dir conf_files=$ucm_conf matched_card_profiles=$ucm_matched alsaucm_listcards_exit=$(field alsaucm_listcards_exit); tools:$tools (availability only; none executed except alsaucm listcards)"

  graph=$(field pipewire_graph)
  sinks=$(count "$(field graph_sinks)")
  sources=$(count "$(field graph_sources)")
  case "$graph" in
    wpctl-unavailable) pw_progress=wpctl-unavailable ;;
    socket-absent) pw_progress=pipewire-socket-absent ;;
    unreachable) pw_progress=pipewire-graph-unreachable ;;
    reachable)
      if [ "$sinks" = unknown ] || [ "$sources" = unknown ]; then
        pw_progress=graph-reachable-endpoint-count-unknown
      elif gt0 "$sinks" && gt0 "$sources"; then
        pw_progress=graph-sinks-and-sources-present
      elif gt0 "$sinks"; then
        pw_progress=graph-sinks-only
      elif gt0 "$sources"; then
        pw_progress=graph-sources-only
      else
        pw_progress=graph-reachable-no-audio-endpoints
      fi
      ;;
    *) pw_progress=pipewire-graph-unknown ;;
  esac
  pw_evidence="graph=$graph socket=$(field pipewire_socket) pipewire=$(field process_pipewire) wireplumber=$(field process_wireplumber) pipewire_pulse=$(field process_pipewire_pulse) pulse_socket=$(field pulse_socket) devices=$(count "$(field graph_devices)") sinks=$sinks sources=$sources default_sink=$(field graph_default_sink) default_source=$(field graph_default_source) adapter_list_sinks_exit=$(field adapter_list_sinks_exit)"

  core_state=$(field core_state)
  route=$(field core_audio_route)
  capability=$(field core_audio_capability)
  svc_audio=$(field service_ferrix_audio)
  case "$core_state" in
    socket-absent) adapter_progress=ferrix-core-socket-absent ;;
    socket-client-unavailable) adapter_progress=core-socket-client-unavailable ;;
    not-observed) adapter_progress=core-state-not-observed ;;
    observed)
      if [ "$route" = absent ] || [ "$route" = unknown ]; then
        adapter_progress=core-state-without-audio
      else
        adapter_progress=core-audio-state-observed
      fi
      ;;
    *) adapter_progress=ferrix-adapter-state-unknown ;;
  esac
  adapter_evidence="backend=$(field ferrix_audio_backend) ferrix-pipewire=$(field service_ferrix_pipewire) ferrix-audio=$svc_audio ferrix-core=$(field service_ferrix_core) core_socket=$(field core_socket) core_state=$core_state audio_capability=$capability audio_route=$route; route changes not exercised"

  tsv="$out/audio-evidence-latest.tsv"
  {
    printf '# Google Pixel 9a audio inventory, %s UTC. evidence label: %s. Redacted transcript: %s\n' "$stamp" "$evidence_label" "$(basename "$transcript")"
    printf '# Read-only enumeration; status stays unsupported (CapabilityStatus) until speaker playback, microphone capture, and Ferrix route changes are evidenced on the physical target. Not QEMU.\n'
    printf 'subsystem\tstatus\tprogress\tevidence\n'
    printf 'alsa\tunsupported\t%s\t%s; %s; %s\n' "$alsa_progress" "$evidence_label" "$stamp" "$alsa_evidence"
    printf 'kernel-audio\tunsupported\t%s\t%s; %s; %s\n' "$kernel_progress" "$evidence_label" "$stamp" "$kernel_evidence"
    printf 'audio-firmware\tunsupported\t%s\t%s; %s; %s\n' "$fw_progress" "$evidence_label" "$stamp" "$fw_evidence"
    printf 'ucm-and-tools\tunsupported\t%s\t%s; %s; %s\n' "$ucm_progress" "$evidence_label" "$stamp" "$ucm_evidence"
    printf 'pipewire-graph\tunsupported\t%s\t%s; %s; %s\n' "$pw_progress" "$evidence_label" "$stamp" "$pw_evidence"
    printf 'ferrix-audio-adapter\tunsupported\t%s\t%s; %s; %s\n' "$adapter_progress" "$evidence_label" "$stamp" "$adapter_evidence"
    printf 'audio\tunsupported\tinventory-only-playback-capture-not-exercised\t%s; %s; no aplay/speaker-test/arecord/amixer/route mutation was run; capabilities.toml is not edited\n' "$evidence_label" "$stamp"
  } | redact > "$tsv"

  printf 'evidence=%s\n' "$evidence_label"
  printf 'alsa=unsupported progress=%s\n' "$alsa_progress"
  printf 'kernel_audio=unsupported progress=%s\n' "$kernel_progress"
  printf 'audio_firmware=unsupported progress=%s\n' "$fw_progress"
  printf 'ucm_and_tools=unsupported progress=%s\n' "$ucm_progress"
  printf 'pipewire_graph=unsupported progress=%s\n' "$pw_progress"
  printf 'ferrix_audio_adapter=unsupported progress=%s\n' "$adapter_progress"
  printf 'transcript=%s\n' "$transcript"
  printf 'audio_evidence_tsv=%s\n' "$tsv"
}

command_name=${1:-}
[ -n "$command_name" ] || usage
shift
case "$command_name" in
  inventory) [ "$#" -eq 0 ] || usage; inventory ;;
  *) usage ;;
esac
