#!/bin/sh
set -eu

# Host-side radio workflow for a Google Pixel 9a (tegu / GXQ96 / MP1.0) that is
# ALREADY running Ferrix on the postmarketOS substrate and is reachable over
# the pmOS USB network with SSH. It never touches storage, partitions, slots,
# or the bootloader, and it uses no Android tooling at all: every remote
# command goes through the single `remote()` helper below.
#
#   tools/pixel-9a/radios.sh preflight
#   tools/pixel-9a/radios.sh inventory
#   tools/pixel-9a/radios.sh associated
#   tools/pixel-9a/radios.sh enable [wifi|bluetooth|modem|all]
#   tools/pixel-9a/radios.sh connect-wifi <SSID>
#   tools/pixel-9a/radios.sh evidence
#
# Evidence policy: the transcripts written under out/pixel-9a/radios/ are
# redacted (MAC, IMEI/IMSI/ICCID, serial, SSID, IPv6) and ignored by Git. The
# `evidence` command prints a proposed capabilities.toml fragment for the
# OWNER to paste after review; this script never edits capabilities.toml.
# Every evidence line carries `physical: google-pixel-9a-tegu-gxq96-mp1-0`
# so it can never be confused with QEMU or mock evidence.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
target="$root/targets/boards/$target_id"
pmos_cfg="$target/pmos/pmbootstrap.cfg"
firmware_allowlist="$target/pmos/device-google-tegu/firmware-allowlist.tsv"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/radios"
evidence_label="physical: $target_id"
cpif_sentence='S5300 CPIF exposes no ModemManager modem; CPIF backend not implemented'

# Connection settings. Defaults are the postmarketOS USB-network conventions:
# the guest is 172.16.42.1 and the SSH user is the one configured for the
# rootfs in pmbootstrap.cfg.
ssh_host=${FERRIX_PIXEL9A_SSH_HOST:-172.16.42.1}
ssh_port=${FERRIX_PIXEL9A_SSH_PORT:-22}
ssh_opts=${FERRIX_PIXEL9A_SSH_OPTS:-}
if [ -z "${FERRIX_PIXEL9A_SSH_USER:-}" ]; then
  ssh_user=$(sed -n 's/^user = *//p' "$pmos_cfg" 2>/dev/null | head -n 1)
  ssh_user=${ssh_user:-user}
else
  ssh_user=$FERRIX_PIXEL9A_SSH_USER
fi

# Exact-target device-tree check. The pinned `zumapro-tegu-mp.dts` root node
# emits this exact, case-sensitive vendor string. Keep the override for a
# future pinned source revision, but never fall back to a generic `tegu`
# substring that could admit another board.
dt_compatible=${FERRIX_PIXEL9A_DT_COMPATIBLE:-google,ZUMA PRO TEGU}
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
qemu_compatible='linux,dummy-virt'

# Wi-Fi driver names accepted as "driver loaded". Google's BCM4383 driver is
# the Broadcom DHD (`bcmdhd*`, firmware fw_bcmdhd.bin); mainline would be
# brcmfmac. Extend when the physical device shows another name.
wifi_driver_re=${FERRIX_PIXEL9A_WIFI_DRIVER_RE:-brcmfmac|bcmdhd}

usage() {
  cat >&2 <<'USAGE'
usage: tools/pixel-9a/radios.sh <command>

  preflight              verify host tools, SSH reachability, and the exact
                         guest identity (device tree, hostname, kernel, OS)
  inventory              read-only radio/service inventory; writes a redacted
                         transcript and evidence-latest.tsv
  associated             read-only GNSS/NFC/ST54/UWB kernel-surface probe;
                         writes associated-evidence-latest.tsv (unsupported)
  enable [wifi|bluetooth|modem|all]
                         unblock and power on the selected radios after one
                         typed confirmation (device state only; no storage)
  connect-wifi <SSID>    join a Wi-Fi network; PSK from FERRIX_PIXEL9A_WIFI_PSK
                         or an interactive no-echo prompt, never logged
  evidence               run inventory, then print a proposed capabilities.toml
                         fragment for the owner to paste manually
  redact                 internal: filter stdin through the transcript redactor

environment: FERRIX_PIXEL9A_SSH_HOST (172.16.42.1) FERRIX_PIXEL9A_SSH_USER
             (pmbootstrap.cfg user) FERRIX_PIXEL9A_SSH_PORT (22)
             FERRIX_PIXEL9A_SSH_OPTS FERRIX_PIXEL9A_DT_COMPATIBLE (google,ZUMA PRO TEGU)
             FERRIX_PIXEL9A_WIFI_PSK FERRIX_OUT_DIR (out)
USAGE
  exit 2
}

die() {
  code=${2:-1}
  printf 'FAIL pixel-9a-radios: %s\n' "$1" >&2
  printf '{"event":"pixel-9a-radios","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
    "$evidence_label" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
  exit "$code"
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing host tool '$1'; install it explicitly"
}

if command -v shasum >/dev/null 2>&1; then
  hasher='shasum -a 256'
else
  hasher=sha256sum
fi

sha256_prefix() {
  printf '%s' "$1" | $hasher | cut -c1-12
}

# Single-quote a value for the remote POSIX shell.
shquote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

# Redaction filter applied to every transcript and stdout line. It replaces
# MAC addresses with mac:<sha256 prefix>, hashes SSIDs (the literal SSID in
# REDACT_SSID and structural nmcli/wpa fields), blanks IMEI/IMSI/ICCID/MEID
# runs, serial numbers, and phone numbers after their keywords, and replaces
# global/ULA/link-local IPv6 addresses. Hashing is done by the host sha256
# tool with the value passed as octal escapes, so arbitrary bytes never reach
# a shell as words. Runs under LC_ALL=C for byte semantics.
redact() {
  LC_ALL=C REDACT_HASHER="$hasher" awk '
    function hash12(value,   i, esc, cmd, out) {
      esc = ""
      for (i = 1; i <= length(value); i++)
        esc = esc sprintf("\\%03o", ord[substr(value, i, 1)])
      cmd = "printf \047" esc "\047 | " ENVIRON["REDACT_HASHER"]
      out = ""
      cmd | getline out
      close(cmd)
      return substr(out, 1, 12)
    }
    function replace_literal(line, needle, replacement,   out, position) {
      out = ""
      while ((position = index(line, needle)) != 0) {
        out = out substr(line, 1, position - 1) replacement
        line = substr(line, position + length(needle))
      }
      return out line
    }
    function redact_regex(line, re, label,   out, seg) {
      out = ""
      while (match(line, re)) {
        seg = substr(line, RSTART, RLENGTH)
        out = out substr(line, 1, RSTART - 1) label ":" hash12(tolower(seg))
        line = substr(line, RSTART + RLENGTH)
      }
      return out line
    }
    # Keyword-anchored value redaction on a lowercased copy; positions align
    # because tolower() preserves length in the C locale.
    function redact_after_keyword(line, re, valuere, replacement,   out, low, seg, start, len) {
      out = ""
      low = tolower(line)
      while (match(low, re)) {
        start = RSTART
        len = RLENGTH
        seg = substr(line, start, len)
        if (match(seg, valuere)) seg = substr(seg, 1, RSTART - 1) replacement
        out = out substr(line, 1, start - 1) seg
        line = substr(line, start + len)
        low = tolower(line)
      }
      return out line
    }
    BEGIN {
      for (i = 1; i < 256; i++) ord[sprintf("%c", i)] = i
      ssid = ENVIRON["REDACT_SSID"]
      d14 = "[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]"
      d7 = "[0-9][0-9][0-9][0-9][0-9][0-9][0-9]"
      idre = "(^|[^a-z])(imei|imsi|iccid|meid|equipment id|equipment identifier|sim identifier|sim id)[^0-9]*" d14 "[0-9]*"
      serialre = "(^|[^a-z])(serial ?number|serial-number|serialnumber|serialno|serial)[^a-z0-9]*[a-z0-9._-][a-z0-9._-][a-z0-9._-][a-z0-9._-]*"
      phonere = "(^|[^a-z])(own|numbers?|msisdn)[^0-9+]*[+]?" d7 "[0-9]*"
      macre = "[0-9A-Fa-f][0-9A-Fa-f](:[0-9A-Fa-f][0-9A-Fa-f])(:[0-9A-Fa-f][0-9A-Fa-f])(:[0-9A-Fa-f][0-9A-Fa-f])(:[0-9A-Fa-f][0-9A-Fa-f])(:[0-9A-Fa-f][0-9A-Fa-f])"
      ipv6re = "[23fF][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]:[0-9A-Fa-f:]*[0-9A-Fa-f:]"
    }
    {
      line = $0
      if (ssid != "" && ssid != "--") line = replace_literal(line, ssid, "ssid:" hash12(ssid))
      if (match(line, /^(SSID|GENERAL\.CONNECTION|CONNECTION|802-11-wireless\.ssid|connection\.id|ssid)[:=]/)) {
        value = substr(line, RLENGTH + 1)
        if (value != "" && value != "--") line = substr(line, 1, RLENGTH) "ssid:" hash12(value)
      }
      line = redact_regex(line, macre, "mac")
      line = redact_regex(line, ipv6re, "ipv6")
      line = redact_after_keyword(line, idre, "[0-9]+$", "<redacted-id>")
      line = redact_after_keyword(line, serialre, "[A-Za-z0-9._-]+$", "<redacted-serial>")
      line = redact_after_keyword(line, phonere, "[+]?[0-9]+$", "<redacted-number>")
      print line
    }
  '
}

# The only SSH invocation in this workflow. stdin comes from fd 3, which is
# /dev/null unless a caller deliberately re-opens it (connect-wifi feeds the
# PSK that way), so remote commands never consume the operator confirmation.
exec 3</dev/null
remote() {
  # shellcheck disable=SC2086 # FERRIX_PIXEL9A_SSH_OPTS is intentionally word-split.
  ssh -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new -p "$ssh_port" $ssh_opts "$ssh_user@$ssh_host" "$1" <&3
}

transcript=""
stamp=""
open_transcript() {
  mkdir -p "$out"
  stamp=$(date -u +%Y%m%dT%H%M%SZ)
  transcript="$out/$1-$stamp.txt"
  {
    printf '# Google Pixel 9a radios %s, %s UTC, host %s\n' "$1" "$stamp" "$(uname -s) $(uname -m)"
    printf '# evidence: %s (not QEMU, not mock)\n' "$evidence_label"
    printf '# guest: %s@%s:%s\n' "$ssh_user" "$ssh_host" "$ssh_port"
    printf '# MAC, IMEI/IMSI/ICCID, serial, SSID, and IPv6 values are redacted. Never commit this file.\n'
  } > "$transcript"
}

# Print a redacted line to stdout and the transcript.
say() {
  printf '%s\n' "$*" | redact
  [ -n "$transcript" ] && printf '%s\n' "$*" | redact >> "$transcript"
  return 0
}

# Run a remote command, record it and its redacted output. Sets $captured and
# $captured_status. Exit 255 is an SSH transport failure.
captured=""
captured_status=0
capture() {
  label=$1
  cmd=$2
  captured_status=0
  captured=$(remote "$cmd" 2>&1) || captured_status=$?
  if [ -n "$transcript" ]; then
    {
      printf '## %s\n$ %s\n' "$label" "$cmd"
      [ -z "$captured" ] || printf '%s\n' "$captured"
      printf '[exit %s]\n' "$captured_status"
    } | redact >> "$transcript"
  fi
  [ "$captured_status" -ne 255 ] || die "guest unreachable over SSH ($ssh_user@$ssh_host:$ssh_port): $captured" 3
}

field() {
  value=$(printf '%s\n' "$captured" | sed -n "s/^$1=//p" | head -n 1)
  case "$1" in
    dt_model|dt_compatible) printf '%s\n' "$value" | sed 's/[[:space:]]*$//' ;;
    *) printf '%s\n' "$value" ;;
  esac
}

# --- preflight -------------------------------------------------------------

identity_cmd='printf "dt_model=%s\n" "$(tr "\0" " " </proc/device-tree/model 2>/dev/null)"; printf "dt_compatible=%s\n" "$(tr "\0" " " </proc/device-tree/compatible 2>/dev/null)"; printf "hostname=%s\n" "$(hostname 2>/dev/null)"; printf "kernel=%s\n" "$(uname -r)"; printf "os_release=%s\n" "$(. /etc/os-release 2>/dev/null && printf "%s %s" "${ID:-unknown}" "${VERSION_ID:-unknown}")"; printf "uid=%s\n" "$(id -u)"; if [ "$(id -u)" = 0 ]; then echo priv=; elif command -v doas >/dev/null 2>&1; then echo priv=doas; elif command -v sudo >/dev/null 2>&1; then echo "priv=sudo -n"; else echo priv=none; fi'

priv=""
guest_verified=0
verified_dt_model=""
verified_dt_compatible=""
preflight() {
  need ssh
  need awk
  need sed
  [ -f "$pmos_cfg" ] || die "missing pmOS configuration: $pmos_cfg"
  [ -f "$firmware_allowlist" ] || die "missing firmware allow-list: $firmware_allowlist"
  say "host_tools=ok"
  say "ssh_target=$ssh_user@$ssh_host:$ssh_port"
  capture guest-identity "$identity_cmd"
  [ "$captured_status" -eq 0 ] || die "guest identity command failed (exit $captured_status): $captured" 3
  say "guest_reachable=yes"
  model=$(field dt_model)
  compatible=$(field dt_compatible)
  case "$compatible $model" in
    *"$qemu_compatible"*)
      say "guest_identity=refused-qemu"
      die "guest is a QEMU virt machine ($qemu_compatible); QEMU is not the physical target and provides no physical evidence" 5
      ;;
  esac
  case "$compatible" in
    *"$dt_compatible"*) ;;
    *)
      say "guest_identity=mismatch"
      die "device-tree compatible '$compatible' does not contain '$dt_compatible'; refusing (set FERRIX_PIXEL9A_DT_COMPATIBLE only after verifying the exact tegu string)" 4
      ;;
  esac
  [ "$model" = "$dt_model" ] || {
    say "guest_identity=mismatch"
    die "device-tree model '$model' is not the pinned $dt_model board" 4
  }
  priv=$(field priv)
  [ "$priv" != none ] || priv=""
  say "guest_identity=verified"
  say "dt_model=$model"
  say "dt_compatible=$compatible"
  say "hostname=$(field hostname)"
  say "kernel=$(field kernel)"
  say "os_release=$(field os_release)"
  say "uid=$(field uid)"
  say "privilege_prefix=${priv:-none}"
  say "evidence=$evidence_label"
  verified_dt_model=$model
  verified_dt_compatible=$compatible
  guest_verified=1
}

# Re-read the exact board identity after the operator confirms a state change.
# Each SSH command opens a fresh session, so the preflight observation alone is
# not sufficient if the USB-network endpoint or guest was replaced while the
# operator was reviewing the prompt.
recheck_guest_identity() {
  capture guest-identity-recheck "$identity_cmd"
  [ "$captured_status" -eq 0 ] ||
    die "guest identity recheck failed (exit $captured_status): $captured" 4
  recheck_model=$(field dt_model)
  recheck_compatible=$(field dt_compatible)
  [ "$recheck_model" = "$verified_dt_model" ] ||
    die "guest device-tree model changed before mutation: '$recheck_model' (expected '$verified_dt_model')" 4
  [ "$recheck_compatible" = "$verified_dt_compatible" ] ||
    die "guest device-tree compatible changed before mutation: '$recheck_compatible'" 4
  say "guest_identity=rechecked"
}

# --- inventory -------------------------------------------------------------

# Line-JSON probes of the Ferrix daemons. socat is the only widely packaged
# Unix-socket client on Alpine/pmOS; without it the probe reports so honestly.
core_probe='if command -v socat >/dev/null 2>&1; then printf {\"request\":\"state\"}\\n | timeout 5 socat -t 2 - UNIX-CONNECT:/run/ferrix/core.sock; else echo ferrix_socket_client=unavailable; fi'
networkd_probe='if command -v socat >/dev/null 2>&1; then printf {\"request\":\"hello\",\"schema_version\":1,\"client_kind\":\"tool\"}\\n{\"request\":\"state\"}\\n | timeout 5 socat -t 2 - UNIX-CONNECT:/run/ferrix/networkd.sock; else echo ferrix_socket_client=unavailable; fi'
telephonyd_probe='if command -v socat >/dev/null 2>&1; then printf {\"request\":\"hello\",\"schema_version\":1,\"client_kind\":\"tool\"}\\n{\"request\":\"state\"}\\n | timeout 5 socat -t 2 - UNIX-CONNECT:/run/ferrix/telephonyd.sock; else echo ferrix_socket_client=unavailable; fi'
services_cmd='for s in dbus networkmanager modemmanager bluetooth upower ferrix-core ferrix-networkd ferrix-telephonyd ferrix-audio; do printf %s= $s; if command -v rc-service >/dev/null 2>&1; then rc-service $s status 2>&1 | xargs; elif command -v systemctl >/dev/null 2>&1; then systemctl is-active $s 2>&1 | xargs; else echo no-service-manager; fi; done'
sockets_cmd='for s in core networkd telephonyd ferrix-appd; do if [ -S /run/ferrix/$s.sock ]; then echo socket=/run/ferrix/$s.sock=present; else echo socket=/run/ferrix/$s.sock=absent; fi; done'
logs_cmd='grep -hE "(networkd|telephonyd)-(ready|failed)|boot-health" /var/log/ferrix-networkd.log /var/log/ferrix-telephonyd.log /var/log/ferrix-core.log 2>/dev/null | tail -n 12'
cpif_probe_cmd='cpif_platform=absent; cpif_driver=unbound; cpif_state=unknown; for entry in /sys/bus/platform/devices/*; do if [ -f "$entry/of_node/compatible" ] && tr "\0" " " < "$entry/of_node/compatible" 2>/dev/null | grep -q "samsung,exynos-cp"; then cpif_platform=present; [ -e "$entry/driver" ] && cpif_driver=bound; [ -r "$entry/modem_state" ] && cpif_state=$(tr -d "\r\n" < "$entry/modem_state"); fi; done; s5300_modem_module=absent; [ -d /sys/module/s5300_modem ] && s5300_modem_module=loaded; s5300_modem_driver=unbound; wwan_devices=0; for entry in /sys/class/wwan/*; do [ -e "$entry" ] || continue; wwan_devices=$((wwan_devices + 1)); driver_link="$entry/device/driver"; if [ -L "$driver_link" ]; then driver_name=$(readlink "$driver_link" 2>/dev/null | sed "s#.*/##; s#/$##"); case "$driver_name" in s5300_modem|*s5300*) s5300_modem_driver=bound;; esac; fi; done; fmt_channels=0; for node in /dev/umts_ipc0 /dev/umts_ipc1; do [ -c "$node" ] && fmt_channels=$((fmt_channels + 1)); done; sit_ports=0; for node in /dev/wwan0sit*; do [ -c "$node" ] && sit_ports=$((sit_ports + 1)); done; at_ports=0; for node in /dev/wwan0at*; do [ -c "$node" ] && at_ports=$((at_ports + 1)); done; rmnet_count=0; for node in /sys/class/net/rmnet*; do [ -e "$node" ] && rmnet_count=$((rmnet_count + 1)); done; if [ "$s5300_modem_module" = loaded ] || [ "$s5300_modem_driver" = bound ] || [ "$wwan_devices" -gt 0 ] || [ "$sit_ports" -gt 0 ]; then radio_backend=wwan-sit; elif [ "$cpif_platform" = present ] || [ "$fmt_channels" -gt 0 ]; then radio_backend=google-cpif; else radio_backend=unknown; fi; printf "radio_backend=%s\ncpif_platform=%s\ncpif_driver=%s\ncpif_state=%s\ns5300_modem_module=%s\ns5300_modem_driver=%s\nwwan_devices=%s\nfmt_channels=%s\nsit_ports=%s\nat_ports=%s\nrmnet_count=%s\n" "$radio_backend" "$cpif_platform" "$cpif_driver" "$cpif_state" "$s5300_modem_module" "$s5300_modem_driver" "$wwan_devices" "$fmt_channels" "$sit_ports" "$at_ports" "$rmnet_count"'
associated_radio_probe_cmd='
probe=read-only
gnss_devices=0
gnss_class_devices=0
gnss_kepler_nodes=0
gnss_kepler_bound=0
gnss_kepler_module=absent
for node in /dev/gnss*; do
  [ -c "$node" ] || continue
  gnss_devices=$((gnss_devices + 1))
done
for entry in /sys/class/gnss/*; do
  [ -e "$entry" ] || continue
  gnss_class_devices=$((gnss_class_devices + 1))
done
for entry in /sys/bus/spi/devices/* /sys/bus/platform/devices/* /sys/class/gnss/*; do
  [ -e "$entry" ] || continue
  driver_link="$entry/driver"
  [ -L "$driver_link" ] || driver_link="$entry/device/driver"
  for compatible_path in "$entry/of_node/compatible" "$entry/device/of_node/compatible"; do
    [ -r "$compatible_path" ] || continue
    compatible=$(tr "\0" " " < "$compatible_path" 2>/dev/null)
    case "$compatible" in
      *samsung,kepler*)
        gnss_kepler_nodes=$((gnss_kepler_nodes + 1))
        [ -L "$driver_link" ] && gnss_kepler_bound=$((gnss_kepler_bound + 1))
        break
        ;;
    esac
  done
done
[ -d /sys/module/gnss_kepler ] || [ -d /sys/module/kepler ] && gnss_kepler_module=loaded
nfc_class_devices=0
nfc_st21_nodes=0
nfc_st21_bound=0
st21_module=absent
st54_nodes=0
st54_bound=0
st54_module=absent
for entry in /sys/class/nfc/*; do
  [ -e "$entry" ] || continue
  nfc_class_devices=$((nfc_class_devices + 1))
done
[ -d /sys/module/st21nfc ] && st21_module=loaded
[ -d /sys/module/st54spi ] || [ -d /sys/module/st54 ] && st54_module=loaded
for entry in /sys/bus/i2c/devices/* /sys/bus/spi/devices/* /sys/bus/platform/devices/*; do
  [ -e "$entry" ] || continue
  driver_link="$entry/driver"
  [ -L "$driver_link" ] || driver_link="$entry/device/driver"
  compatible=""
  for compatible_path in "$entry/of_node/compatible" "$entry/device/of_node/compatible"; do
    [ -r "$compatible_path" ] || continue
    compatible=$(tr "\0" " " < "$compatible_path" 2>/dev/null)
    break
  done
  case "$compatible" in
    *st,st21nfc*|*st21nfc*)
      nfc_st21_nodes=$((nfc_st21_nodes + 1))
      [ -L "$driver_link" ] && nfc_st21_bound=$((nfc_st21_bound + 1))
      ;;
    *st54*)
      st54_nodes=$((st54_nodes + 1))
      [ -L "$driver_link" ] && st54_bound=$((st54_bound + 1))
      ;;
  esac
done
uwb_class_devices=0
uwb_nodes=0
uwb_reset_nodes=0
for entry in /sys/class/uwb/*; do
  [ -e "$entry" ] || continue
  uwb_class_devices=$((uwb_class_devices + 1))
done
for entry in /sys/bus/i2c/devices/* /sys/bus/spi/devices/* /sys/bus/platform/devices/*; do
  [ -e "$entry" ] || continue
  driver_link="$entry/driver"
  [ -L "$driver_link" ] || driver_link="$entry/device/driver"
  compatible=""
  for compatible_path in "$entry/of_node/compatible" "$entry/device/of_node/compatible"; do
    [ -r "$compatible_path" ] || continue
    compatible=$(tr "\0" " " < "$compatible_path" 2>/dev/null)
    break
  done
  case "$compatible" in
    *uwb*|*UWB*) uwb_nodes=$((uwb_nodes + 1));;
  esac
  [ -e "$entry/of_node/google,aoc_uwb_rst" ] || [ -e "$entry/device/of_node/google,aoc_uwb_rst" ] && uwb_reset_nodes=$((uwb_reset_nodes + 1))
done
printf "probe=%s\ngnss_devices=%s\ngnss_class_devices=%s\ngnss_kepler_nodes=%s\ngnss_kepler_bound=%s\ngnss_kepler_module=%s\nnfc_class_devices=%s\nnfc_st21_nodes=%s\nnfc_st21_bound=%s\nst21_module=%s\nst54_nodes=%s\nst54_bound=%s\nst54_module=%s\nuwb_class_devices=%s\nuwb_nodes=%s\nuwb_reset_nodes=%s\n" "$probe" "$gnss_devices" "$gnss_class_devices" "$gnss_kepler_nodes" "$gnss_kepler_bound" "$gnss_kepler_module" "$nfc_class_devices" "$nfc_st21_nodes" "$nfc_st21_bound" "$st21_module" "$st54_nodes" "$st54_bound" "$st54_module" "$uwb_class_devices" "$uwb_nodes" "$uwb_reset_nodes"'
firmware_integrity_cmd='manifest=/usr/share/ferrix/pixel-9a/firmware-manifest.tsv; if [ ! -f "$manifest" ]; then echo firmware_manifest=missing; else hash_file() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk "{print \$1}"; elif command -v sha256 >/dev/null 2>&1; then sha256 -q "$1"; else return 1; fi; }; rows=0; verified=0; while IFS="$(printf "\t")" read -r kind controller image source runtime bytes expected extra; do case "$kind" in firmware) rows=$((rows + 1)); file="/$runtime"; status=missing; if [ -n "$runtime" ] && [ "${runtime#/}" = "$runtime" ] && [ "${runtime#*..}" = "$runtime" ] && [ -f "$file" ] && [ ! -L "$file" ]; then actual_bytes=$(wc -c <"$file" | tr -d "[:space:]"); actual_hash=$(hash_file "$file" 2>/dev/null || true); if [ "$actual_bytes" = "$bytes" ] && [ "$actual_hash" = "$expected" ]; then status=verified; verified=$((verified + 1)); else status=mismatch; fi; fi; echo "firmware=$file=$status" ;; esac; done <"$manifest"; if [ "$rows" -gt 0 ] && [ "$verified" -eq "$rows" ]; then echo firmware_manifest=verified; else echo firmware_manifest=invalid; fi; fi'

# Status vocabulary is the ferrix-hardware-api CapabilityStatus set minus the
# emulator-only value: `supported` or `unsupported` (never `mocked` for the
# physical target). Intermediate findings go into `progress` and the evidence
# text, never into `status`.
net_status=unsupported
bt_status=unsupported
tel_status=unsupported
net_progress=none
bt_progress=none
tel_progress=none
net_evidence=""
bt_evidence=""
tel_evidence=""
gnss_status=unsupported
nfc_status=unsupported
st54_status=unsupported
uwb_status=unsupported
gnss_progress=none
nfc_progress=none
st54_progress=none
uwb_progress=none
gnss_evidence=""
nfc_evidence=""
st54_evidence=""
uwb_evidence=""
modem_present=0
wifi_dev=""
associated_surface=""
associated_probe_exit=0
associated_tsv=""

associated_field() {
  printf '%s\n' "$associated_surface" | sed -n "s/^$1=//p" | head -n 1
}

associated_count() {
  case "$1" in
    ''|*[!0-9]*) printf '0\n' ;;
    *) printf '%s\n' "$1" ;;
  esac
}

associated_parse() {
  gnss_devices=$(associated_count "$(associated_field gnss_devices)")
  gnss_class_devices=$(associated_count "$(associated_field gnss_class_devices)")
  gnss_kepler_nodes=$(associated_count "$(associated_field gnss_kepler_nodes)")
  gnss_kepler_bound=$(associated_count "$(associated_field gnss_kepler_bound)")
  gnss_kepler_module=$(associated_field gnss_kepler_module)
  gnss_kepler_module=${gnss_kepler_module:-unknown}
  nfc_class_devices=$(associated_count "$(associated_field nfc_class_devices)")
  nfc_st21_nodes=$(associated_count "$(associated_field nfc_st21_nodes)")
  nfc_st21_bound=$(associated_count "$(associated_field nfc_st21_bound)")
  st21_module=$(associated_field st21_module)
  st21_module=${st21_module:-unknown}
  st54_nodes=$(associated_count "$(associated_field st54_nodes)")
  st54_bound=$(associated_count "$(associated_field st54_bound)")
  st54_module=$(associated_field st54_module)
  st54_module=${st54_module:-unknown}
  uwb_class_devices=$(associated_count "$(associated_field uwb_class_devices)")
  uwb_nodes=$(associated_count "$(associated_field uwb_nodes)")
  uwb_reset_nodes=$(associated_count "$(associated_field uwb_reset_nodes)")
}

associated_write_evidence() {
  probe_summary=$(printf '%s\n' "$associated_surface" | tr '\n' ';' | sed 's/;*$//')
  if [ "$associated_probe_exit" -ne 0 ]; then
    gnss_progress=probe-failed
    nfc_progress=probe-failed
    st54_progress=probe-failed
    uwb_progress=probe-failed
    gnss_evidence="read-only probe exit=$associated_probe_exit; no capability promotion"
    nfc_evidence="read-only probe exit=$associated_probe_exit; no capability promotion"
    st54_evidence="read-only probe exit=$associated_probe_exit; no capability promotion"
    uwb_evidence="read-only probe exit=$associated_probe_exit; no capability promotion"
  else
    if [ "$gnss_devices" -gt 0 ] || [ "$gnss_class_devices" -gt 0 ]; then
      if [ "$gnss_kepler_nodes" -gt 0 ] && [ "$gnss_kepler_bound" -gt 0 ]; then
        gnss_progress=kernel-gnss-device-present-location-adapter-unimplemented
        gnss_evidence="samsung,kepler binding and GNSS raw/class device observed; no Ferrix location API or BETP/NMEA parser"
      elif [ "$gnss_kepler_nodes" -gt 0 ] || [ "$gnss_kepler_module" = loaded ]; then
        gnss_progress=kernel-kepler-present-gnss-device-unobserved
        gnss_evidence="GNSS raw/class device observed with unverified samsung,kepler runtime binding; no Ferrix location API"
      elif [ "$gnss_class_devices" -gt 0 ]; then
        gnss_progress=linux-gnss-class-present-binding-unobserved
        gnss_evidence="Linux GNSS class entries are visible without a pinned samsung,kepler binding; exact driver and Ferrix adapter are unverified"
      else
        gnss_progress=no-gnss-kepler-surface
        gnss_evidence="GNSS raw/class device observed but the expected samsung,kepler binding was not observed"
      fi
    else
      gnss_progress=no-gnss-kepler-surface
      gnss_evidence="no GNSS character device, GNSS class entry, or samsung,kepler binding observed; target route remains raw-device-only"
    fi
    if [ "$nfc_class_devices" -gt 0 ] || [ "$nfc_st21_nodes" -gt 0 ] || [ "$st21_module" = loaded ]; then
      nfc_progress=linux-nfc-surface-st21-or-class-present
      nfc_evidence="ST21/NFC surface observed (class=$nfc_class_devices nodes=$nfc_st21_nodes bound=$nfc_st21_bound module=$st21_module); native NFC/ESE route is not selected"
    else
      nfc_progress=no-linux-nfc-st21-surface
      nfc_evidence="no Linux NFC class, ST21 binding, or st21nfc module observed; the radio-first target route does not select an NFC/ESE driver"
    fi
    if [ "$st54_nodes" -gt 0 ] || [ "$st54_module" = loaded ]; then
      st54_progress=st54-surface-secure-element-boundary-unreviewed
      st54_evidence="ST54 surface observed (nodes=$st54_nodes bound=$st54_bound module=$st54_module); secure-element route is not selected"
    else
      st54_progress=no-st54-linux-surface
      st54_evidence="no ST54 device-tree binding or st54spi module observed; the radio-first target route does not select the Android ST54 module"
    fi
    if [ "$uwb_class_devices" -gt 0 ] || [ "$uwb_nodes" -gt 0 ] || [ "$uwb_reset_nodes" -gt 0 ]; then
      uwb_progress=linux-uwb-surface-present-driver-unreviewed
      uwb_evidence="UWB surface observed (class=$uwb_class_devices nodes=$uwb_nodes reset_nodes=$uwb_reset_nodes); no selected native UWB route or Ferrix adapter"
    else
      uwb_progress=no-linux-uwb-surface
      uwb_evidence="no Linux UWB class, compatible node, or AoC reset property observed; target audit selects no UWB route"
    fi
  fi

  associated_tsv="$out/associated-evidence-latest.tsv"
  {
    printf '# Google Pixel 9a associated-radio evidence, %s UTC. evidence label: %s. Probe is read-only; not QEMU, not mock.\n' "$stamp" "$evidence_label"
    printf '# No row below promotes a capability; GNSS, NFC, ST54, and UWB remain unsupported until the target promotion gates pass.\n'
    printf 'subsystem\tstatus\tprogress\tevidence\n'
    printf 'gnss\t%s\t%s\t%s; %s; probe_exit=%s; %s\n' "$gnss_status" "$gnss_progress" "$evidence_label" "$stamp" "$associated_probe_exit" "$gnss_evidence"
    printf 'nfc\t%s\t%s\t%s; %s; probe_exit=%s; %s\n' "$nfc_status" "$nfc_progress" "$evidence_label" "$stamp" "$associated_probe_exit" "$nfc_evidence"
    printf 'st54\t%s\t%s\t%s; %s; probe_exit=%s; %s\n' "$st54_status" "$st54_progress" "$evidence_label" "$stamp" "$associated_probe_exit" "$st54_evidence"
    printf 'uwb\t%s\t%s\t%s; %s; probe_exit=%s; %s\n' "$uwb_status" "$uwb_progress" "$evidence_label" "$stamp" "$associated_probe_exit" "$uwb_evidence"
    printf '# probe_fields=%s\n' "$probe_summary"
  } | redact > "$associated_tsv"
}

collect_associated() {
  capture associated-radios "$associated_radio_probe_cmd"
  associated_surface=$captured
  associated_probe_exit=$captured_status
  associated_parse
  associated_write_evidence
}

inventory() {
  [ "$guest_verified" -eq 1 ] || preflight
  capture rfkill 'rfkill list 2>&1'
  rfkill_out=$captured
  capture nm-general 'nmcli -t general status 2>&1'
  nm_general=$captured
  capture nm-devices 'nmcli -t -f DEVICE,TYPE,STATE device status 2>&1'
  nm_devices=$captured
  wifi_dev=$(printf '%s\n' "$nm_devices" | awk -F: '$2 == "wifi" { print $1; exit }')
  capture nm-radio 'nmcli -t radio all 2>&1'
  wifi_radio=$(printf '%s\n' "$captured" | awk -F: 'NR == 1 { print $2 }')
  wifi_driver=""
  if [ -n "$wifi_dev" ]; then
    capture nm-wifi-device "nmcli -t -f GENERAL.DEVICE,GENERAL.TYPE,GENERAL.DRIVER,GENERAL.DRIVER-VERSION,GENERAL.FIRMWARE-VERSION,GENERAL.STATE,IP4.ADDRESS,IP6.ADDRESS device show $(shquote "$wifi_dev") 2>&1"
    wifi_state=$captured
    wifi_driver=$(printf '%s\n' "$captured" | sed -n 's/^GENERAL\.DRIVER://p' | head -n 1)
  else
    wifi_state=""
  fi
  capture bluetoothctl-show 'timeout 10 bluetoothctl show 2>&1'
  bt_out=$captured
  capture mm-list 'mmcli -L 2>&1'
  mm_list=$captured
  case "$mm_list" in
    *"No modems were found"*|"") modem_present=0 ;;
    */org/freedesktop/ModemManager1/Modem/*) modem_present=1 ;;
    *) modem_present=0 ;;
  esac
  mm_modem=""
  if [ "$modem_present" -eq 1 ]; then
    capture mm-modem 'mmcli -m any 2>&1'
    mm_modem=$captured
  fi
  capture lsmod 'lsmod 2>&1'
  lsmod_out=$captured
  capture dmesg-radios "${priv:+$priv }dmesg 2>&1 | grep -iE 'brcmfmac|bcm4383|bcmdhd|btbcm|cpif|s5300|firmware' | tail -n 80"
  capture firmware-presence "$firmware_integrity_cmd"
  firmware_out=$captured
  fw_total=$(printf '%s\n' "$captured" | grep -c '^firmware=' || true)
  fw_present=$(printf '%s\n' "$captured" | grep -c '^firmware=.*=verified$' || true)
  fw_manifest_verified=$(printf '%s\n' "$captured" | sed -n 's/^firmware_manifest=//p' | head -n 1)
  capture services "$services_cmd"
  services_out=$captured
  capture ferrix-sockets "$sockets_cmd"
  sockets_out=$captured
  capture ferrix-logs "${priv:+$priv }sh -c $(shquote "$logs_cmd")"
  logs_out=$captured
  capture ferrix-core-state "$core_probe"
  core_reply=$captured
  capture ferrix-networkd-state "$networkd_probe"
  networkd_reply=$captured
  capture ferrix-telephonyd-state "$telephonyd_probe"
  telephonyd_reply=$captured
  capture cpif-surface "$cpif_probe_cmd"
  cpif_surface=$captured
  collect_associated

  # --- decisions (status vocabulary matches capabilities.toml) ---
  driver_loaded=0
  if printf '%s\n' "$lsmod_out" | grep -qE "^($wifi_driver_re)"; then driver_loaded=1; fi
  if [ -n "$wifi_driver" ] && printf '%s' "$wifi_driver" | grep -qE "^($wifi_driver_re)"; then driver_loaded=1; fi
  networkd_ready=0
  case "$logs_out" in *networkd-ready*networkmanager*|*networkmanager*networkd-ready*) networkd_ready=1 ;; esac
  # A readiness log alone is not enough: the socket probe must return a real
  # state reply, not an authorization/error envelope. This keeps an installed
  # but unusable adapter from being promoted to working networking.
  case "$networkd_reply" in
    *'"reply":"state"'*)
      case "$networkd_reply" in *'"error"'*) ;; *) networkd_ready=1 ;; esac
      ;;
  esac
  core_network_ready=0
  case "$core_reply" in
    *'"reply":"state"'*'"wifi":"connected"'*) core_network_ready=1 ;;
  esac
  nm_connected=0
  nm_connectivity=$(printf '%s\n' "$nm_general" | awk -F: 'NR == 1 { print $1 }')
  nm_full=$(printf '%s\n' "$nm_general" | awk -F: 'NR == 1 { print $2 }')
  case "$nm_connectivity:$nm_full" in connected:full) nm_connected=1 ;; esac
  wifi_link=0
  if printf '%s\n' "$nm_devices" | awk -F: -v device="$wifi_dev" '$1 == device && $3 == "connected" { found=1 } END { exit(found ? 0 : 1) }'; then
    if printf '%s\n' "$wifi_state" | grep -q '^GENERAL\.STATE:100' &&
      printf '%s\n' "$wifi_state" | grep -q '^IP4\.ADDRESS'; then
      wifi_link=1
    fi
  fi
  wifi_rfkill=$(printf '%s\n' "$rfkill_out" | awk 'tolower($0) ~ /wlan|wifi/ { found = 1 } found && /Soft blocked/ { print $3; exit }')
  wifi_rfkill=${wifi_rfkill:-unknown}

  net_status=unsupported
  if [ -z "$wifi_dev" ]; then
    net_progress=no-wifi-device
    net_evidence="no Wi-Fi device in NetworkManager; driver loaded=$driver_loaded; firmware present $fw_present/$fw_total"
  elif [ "$driver_loaded" -ne 1 ] || [ "$fw_manifest_verified" != verified ] || [ "$fw_present" -ne "$fw_total" ]; then
    net_progress=device-present-driver-or-firmware-missing
    net_evidence="Wi-Fi device $wifi_dev present but driver loaded=$driver_loaded (driver '$wifi_driver', expected $wifi_driver_re) and firmware present $fw_present/$fw_total"
  elif [ "$wifi_radio" != enabled ]; then
    net_progress=radio-disabled
    net_evidence="Wi-Fi device $wifi_dev with driver $wifi_driver but radio is '$wifi_radio' (rfkill soft blocked=$wifi_rfkill); run enable wifi"
  elif [ "$nm_connected" -ne 1 ] || [ "$wifi_link" -ne 1 ]; then
    net_progress=wifi-not-associated-or-no-ip
    net_evidence="Wi-Fi device $wifi_dev (driver $wifi_driver) is not fully connected (NetworkManager connectivity=$nm_connectivity/$nm_full, link=$wifi_link); DHCP/IP evidence is required"
  elif [ "$networkd_ready" -ne 1 ] || [ "$core_network_ready" -ne 1 ]; then
    net_progress=linux-wifi-up-ferrix-adapter-not-observed
    net_evidence="Wi-Fi device $wifi_dev (driver $wifi_driver) has connectivity and an IPv4 address, but ferrix-networkd/core state was not a successful connected reply (networkd_ready=$networkd_ready core_connected=$core_network_ready)"
  else
    net_status=supported
    net_progress=linux-and-ferrix-adapter-observed
    net_evidence="NetworkManager Wi-Fi device $wifi_dev (driver $wifi_driver) radio enabled; firmware present $fw_present/$fw_total; ferrix-networkd networkmanager backend ready and answering the typed protocol"
  fi

  bt_controller=0
  bt_powered=0
  case "$bt_out" in *Controller*) bt_controller=1 ;; esac
  case "$bt_out" in *'Powered: yes'*) bt_powered=1 ;; esac
  # A powered HCI controller is not sufficient evidence for this target: a
  # generic BlueZ controller (or a controller that came up with a stale
  # patchram) could satisfy the same surface. Require the exact factory
  # selected Broadcom HCD row from the verified manifest before promoting
  # Bluetooth. The path is pinned by bluetooth-firmware-source-evidence.md
  # and by firmware-stage-allowlist.tsv; do not accept BTFW_B/BTFW_D aliases.
  bt_firmware_verified=0
  if [ "$fw_manifest_verified" = verified ] &&
    printf '%s\n' "$firmware_out" | grep -q '^firmware=/lib/firmware/brcm/BCM[.]hcd=verified$'; then
    bt_firmware_verified=1
  fi
  core_started=0
  case "$services_out" in *ferrix-core=*started*) core_started=1 ;; esac
  core_bluetooth=0
  case "$core_reply" in
    *'"reply":"state"'*'"present":true'*'"enabled":true'*) core_bluetooth=1 ;;
  esac
  bt_status=unsupported
  if [ "$bt_controller" -ne 1 ]; then
    bt_progress=no-controller
    bt_evidence="bluetoothctl shows no controller (btbcm/firmware not proven)"
  elif [ "$bt_powered" -ne 1 ]; then
    bt_progress=controller-unpowered
    bt_evidence="BlueZ controller present but Powered: no; run enable bluetooth"
  elif [ "$bt_firmware_verified" -ne 1 ]; then
    bt_progress=controller-powered-firmware-unverified
    bt_evidence="BlueZ controller powered, but exact factory Broadcom firmware /lib/firmware/brcm/BCM.hcd is not verified (firmware_manifest=$fw_manifest_verified)"
  elif [ "$core_started" -ne 1 ] || [ "$core_bluetooth" -ne 1 ]; then
    bt_progress=controller-powered-ferrix-adapter-not-observed
    bt_evidence="BlueZ controller powered, but the ferrix-core BlueZ adapter path was not observed (ferrix-core started=$core_started, core.sock bluetooth state reply=$core_bluetooth; install socat on the guest to query)"
  else
    bt_status=supported
    bt_progress=linux-and-ferrix-adapter-observed
    bt_evidence="BlueZ controller powered (bluetoothctl Powered: yes) and ferrix-core reports the typed bluetooth state over /run/ferrix/core.sock; pairing/profiles not exercised"
  fi

  telephonyd_ready=0
  case "$logs_out" in *telephonyd-ready*) telephonyd_ready=1 ;; esac
  cpif_platform=$(printf '%s\n' "$cpif_surface" | sed -n 's/^cpif_platform=//p' | head -n 1)
  cpif_driver=$(printf '%s\n' "$cpif_surface" | sed -n 's/^cpif_driver=//p' | head -n 1)
  cpif_state=$(printf '%s\n' "$cpif_surface" | sed -n 's/^cpif_state=//p' | head -n 1)
  radio_backend=$(printf '%s\n' "$cpif_surface" | sed -n 's/^radio_backend=//p' | head -n 1)
  s5300_modem_module=$(printf '%s\n' "$cpif_surface" | sed -n 's/^s5300_modem_module=//p' | head -n 1)
  s5300_modem_driver=$(printf '%s\n' "$cpif_surface" | sed -n 's/^s5300_modem_driver=//p' | head -n 1)
  wwan_devices=$(printf '%s\n' "$cpif_surface" | sed -n 's/^wwan_devices=//p' | head -n 1)
  fmt_channels=$(printf '%s\n' "$cpif_surface" | sed -n 's/^fmt_channels=//p' | head -n 1)
  sit_ports=$(printf '%s\n' "$cpif_surface" | sed -n 's/^sit_ports=//p' | head -n 1)
  at_ports=$(printf '%s\n' "$cpif_surface" | sed -n 's/^at_ports=//p' | head -n 1)
  rmnet_count=$(printf '%s\n' "$cpif_surface" | sed -n 's/^rmnet_count=//p' | head -n 1)
  radio_backend=${radio_backend:-unknown}
  cpif_platform=${cpif_platform:-unknown}
  cpif_driver=${cpif_driver:-unknown}
  cpif_state=${cpif_state:-unknown}
  s5300_modem_module=${s5300_modem_module:-unknown}
  s5300_modem_driver=${s5300_modem_driver:-unknown}
  wwan_devices=${wwan_devices:-unknown}
  fmt_channels=${fmt_channels:-unknown}
  sit_ports=${sit_ports:-unknown}
  at_ports=${at_ports:-unknown}
  rmnet_count=${rmnet_count:-unknown}
  telephonyd_probe_access=unavailable
  case "$telephonyd_reply" in
    *'"error"'*'"capability":"telephony-state"'*) telephonyd_probe_access=unauthorized ;;
    *'"reply":"state"'*) telephonyd_probe_access=state-observed ;;
  esac
  data_control_boundary='typed connect-data/disconnect-data via the telephony-data grant; an SSH shell must not bypass appd authorization'
  data_apn_boundary='APN value withheld; supply DataProfile.apn or the daemon-only FERRIX_TELEPHONY_APN fallback; malformed/empty Samsung-SIT APNs fail closed'
  data_bearer_evidence="bearer/interface evidence: ModemManager modem=$modem_present rmnet_count=$rmnet_count wwan_devices=$wwan_devices; telephonyd_state_probe=$telephonyd_probe_access"
  tel_status=unsupported
  if [ "$modem_present" -ne 1 ]; then
    if [ "$radio_backend" = wwan-sit ]; then
      tel_progress=wwan-sit-surface-no-modemmanager-modem
      tel_evidence="radio-first s5300_modem WWAN/SIT surface observed but ModemManager lists no modem (s5300_modem_module=$s5300_modem_module s5300_modem_driver=$s5300_modem_driver wwan_devices=$wwan_devices sit_ports=$sit_ports at_ports=$at_ports rmnet_count=$rmnet_count; cpif_platform=$cpif_platform cpif_driver=$cpif_driver cpif_state=$cpif_state; ferrix-telephonyd ready=$telephonyd_ready); $data_control_boundary; $data_apn_boundary; $data_bearer_evidence"
    else
      tel_progress=no-modemmanager-modem
      tel_evidence="$cpif_sentence (radio_backend=$radio_backend; mmcli -L lists no modem; cpif_platform=$cpif_platform cpif_driver=$cpif_driver fmt_channels=$fmt_channels sit_ports=$sit_ports at_ports=$at_ports rmnet_count=$rmnet_count cpif_state=$cpif_state; ferrix-telephonyd ready=$telephonyd_ready); $data_control_boundary; $data_apn_boundary; $data_bearer_evidence"
    fi
  else
    # Enumeration alone is not phone support (linux-plan.md), so the status
    # stays unsupported; the progress column records the enumeration.
    tel_progress=modem-enumerated-only
    modem_state=$(printf '%s\n' "$mm_modem" | sed -n 's/.*state: *//p' | head -n 1 | tr -d "'")
    tel_evidence="ModemManager lists a modem (state '${modem_state:-unknown}'; radio_backend=$radio_backend s5300_modem_module=$s5300_modem_module s5300_modem_driver=$s5300_modem_driver wwan_devices=$wwan_devices); cpif_platform=$cpif_platform cpif_driver=$cpif_driver fmt_channels=$fmt_channels sit_ports=$sit_ports at_ports=$at_ports rmnet_count=$rmnet_count cpif_state=$cpif_state; ferrix-telephonyd ready=$telephonyd_ready; $data_control_boundary; $data_apn_boundary; $data_bearer_evidence; calls, SMS, data, IMS untested so the capability stays unsupported"
  fi

  tsv="$out/evidence-latest.tsv"
  {
    printf '# Google Pixel 9a radio evidence, %s UTC. evidence label: %s. Redacted transcript: %s\n' "$stamp" "$evidence_label" "$(basename "$transcript")"
    printf '# status vocabulary: supported|unsupported (ferrix-hardware-api CapabilityStatus; mocked is never used for the physical target). progress carries the nuance. Not QEMU, not mock.\n'
    printf 'subsystem\tstatus\tprogress\tevidence\n'
    printf 'network\t%s\t%s\t%s; %s; %s\n' "$net_status" "$net_progress" "$evidence_label" "$stamp" "$net_evidence"
    printf 'bluetooth\t%s\t%s\t%s; %s; %s\n' "$bt_status" "$bt_progress" "$evidence_label" "$stamp" "$bt_evidence"
    printf 'telephony\t%s\t%s\t%s; %s; %s\n' "$tel_status" "$tel_progress" "$evidence_label" "$stamp" "$tel_evidence"
    printf 'gnss\t%s\t%s\t%s; %s; %s\n' "$gnss_status" "$gnss_progress" "$evidence_label" "$stamp" "$gnss_evidence"
    printf 'nfc\t%s\t%s\t%s; %s; %s\n' "$nfc_status" "$nfc_progress" "$evidence_label" "$stamp" "$nfc_evidence"
    printf 'secure-element\t%s\t%s\t%s; %s; %s\n' "$st54_status" "$st54_progress" "$evidence_label" "$stamp" "$st54_evidence"
    printf 'uwb\t%s\t%s\t%s; %s; %s\n' "$uwb_status" "$uwb_progress" "$evidence_label" "$stamp" "$uwb_evidence"
  } | redact > "$tsv"
  say "network=$net_status progress=$net_progress"
  say "bluetooth=$bt_status progress=$bt_progress"
  say "telephony=$tel_status progress=$tel_progress"
  say "wifi_device=${wifi_dev:-none}"
  say "modem_present=$modem_present"
  say "ferrix_sockets=$(printf '%s' "$sockets_out" | tr '\n' ' ')"
  printf 'transcript=%s\n' "$transcript"
  printf 'evidence_tsv=%s\n' "$tsv"
}

# The associated command intentionally skips the broad radio inventory.  It is
# useful after a native boot even when NetworkManager/ModemManager are not yet
# configured, and its only guest action is the read-only sysfs/devnode probe.
associated() {
  [ "$guest_verified" -eq 1 ] || preflight
  collect_associated
  say "associated_probe=read-only"
  say "gnss=$gnss_status progress=$gnss_progress"
  say "nfc=$nfc_status progress=$nfc_progress"
  say "secure_element=$st54_status progress=$st54_progress"
  say "uwb=$uwb_status progress=$uwb_progress"
  printf 'transcript=%s\n' "$transcript"
  printf 'associated_evidence_tsv=%s\n' "$associated_tsv"
}

# --- enable ----------------------------------------------------------------

mutate() {
  say "mutation=$1"
  recheck_guest_identity
  capture mutation "$1"
  say "mutation_exit=$captured_status"
  [ "$captured_status" -eq 0 ] ||
    die "radio mutation failed (exit $captured_status): $1"
}

enable_radios() {
  selection=""
  [ "$#" -gt 0 ] || set -- all
  for radio in "$@"; do
    case "$radio" in
      all) selection="wifi bluetooth modem" ;;
      wifi|bluetooth|modem) selection="${selection:+$selection }$radio" ;;
      *) usage ;;
    esac
  done
  preflight
  capture mm-list 'mmcli -L 2>&1'
  case "$captured" in
    */org/freedesktop/ModemManager1/Modem/*) modem_present=1 ;;
    *) modem_present=0 ;;
  esac
  say "enable_selection=$selection"
  phrase="CONFIRM PIXEL 9A ENABLE RADIOS: $selection"
  printf 'This changes radio state on the verified guest (no storage, partition, slot, or bootloader change).\n'
  printf 'Type exactly the following line and press Enter:\n%s\n' "$phrase"
  IFS= read -r answer || die "confirmation was not provided; no mutation performed" 6
  [ "$answer" = "$phrase" ] || die "confirmation did not match; no mutation performed" 6
  say "confirmation=accepted"
  for radio in $selection; do
    case "$radio" in
      wifi)
        mutate "${priv:+$priv }rfkill unblock wifi"
        mutate "${priv:+$priv }nmcli radio wifi on"
        ;;
      bluetooth)
        mutate "${priv:+$priv }rfkill unblock bluetooth"
        mutate "${priv:+$priv }bluetoothctl power on"
        ;;
      modem)
        if [ "$modem_present" -eq 1 ]; then
          mutate "${priv:+$priv }rfkill unblock wwan"
          mutate "${priv:+$priv }mmcli -m any --enable"
        else
          say "modem=skipped; $cpif_sentence"
        fi
        ;;
    esac
  done
  printf 'transcript=%s\n' "$transcript"
}

# --- connect-wifi ----------------------------------------------------------

# Route chosen for the PSK: `nmcli --ask device wifi connect <SSID>` reads the
# password from the guest's stdin, which is fed through the SSH channel from
# fd 3. The PSK therefore never appears in remote argv, remote shell history,
# host argv, or any transcript. The SSID is in remote argv (unavoidable for
# nmcli) but is hashed everywhere this workflow writes.
connect_wifi() {
  [ "$#" -eq 1 ] && [ -n "$1" ] || usage
  ssid=$1
  REDACT_SSID=$ssid
  export REDACT_SSID
  preflight
  psk=${FERRIX_PIXEL9A_WIFI_PSK:-}
  if [ -z "$psk" ]; then
    [ -t 0 ] || die "no PSK: set FERRIX_PIXEL9A_WIFI_PSK or run interactively"
    printf 'Wi-Fi passphrase (not echoed): '
    stty -echo
    IFS= read -r psk || { stty echo; die "no PSK provided"; }
    stty echo
    printf '\n'
  fi
  capture nm-devices 'nmcli -t -f DEVICE,TYPE,STATE device status 2>&1'
  wifi_dev=$(printf '%s\n' "$captured" | awk -F: '$2 == "wifi" { print $1; exit }')
  [ -n "$wifi_dev" ] || die "no Wi-Fi device in NetworkManager; run inventory/enable wifi first"
  ssid_hash=$(sha256_prefix "$ssid")
  say "ssid_sha256=$ssid_hash"
  say "wifi_device=$wifi_dev"
  phrase="CONFIRM PIXEL 9A CONNECT WIFI SSID SHA256 $ssid_hash"
  printf 'This changes network state on the verified guest (no storage, partition, slot, or bootloader change).\n'
  printf 'Type exactly the following line and press Enter:\n%s\n' "$phrase"
  IFS= read -r answer || die "confirmation was not provided; no mutation performed" 6
  [ "$answer" = "$phrase" ] || die "confirmation did not match; no mutation performed" 6
  say "confirmation=accepted"
  cmd="${priv:+$priv }nmcli --ask device wifi connect $(shquote "$ssid") ifname $(shquote "$wifi_dev")"
  say "mutation=$cmd"
  connect_cmd=$cmd
  recheck_guest_identity
  cmd=$connect_cmd
  connect_status=0
  connect_out=$(printf '%s\n' "$psk" | (exec 3<&0; remote "$cmd" 2>&1)) || connect_status=$?
  unset psk
  {
    printf '## connect\n$ %s\n' "$cmd"
    [ -z "$connect_out" ] || printf '%s\n' "$connect_out"
    printf '[exit %s]\n' "$connect_status"
  } | redact >> "$transcript"
  [ "$connect_status" -ne 255 ] || die "guest unreachable over SSH during connect" 3
  say "connect_exit=$connect_status"
  capture nm-wifi-state "nmcli -t -f GENERAL.STATE,IP4.ADDRESS device show $(shquote "$wifi_dev") 2>&1"
  case "$captured" in
    *'GENERAL.STATE:100'*) say "wifi_connected=yes" ;;
    *) say "wifi_connected=no" ;;
  esac
  printf 'transcript=%s\n' "$transcript"
}

# --- evidence --------------------------------------------------------------

evidence() {
  inventory
  cat <<FRAGMENT

# Proposed capabilities.toml fragment for $target_id.
# Review the redacted transcript first, then paste manually; this script does
# not edit capabilities.toml. Evidence origin: $evidence_label (not QEMU, not mock).
# status uses only supported|unsupported (CapabilityStatus); progress is folded
# into the evidence text.
[network]
status = "$net_status"
evidence = "$evidence_label; $stamp; progress=$net_progress; $net_evidence"

[bluetooth]
status = "$bt_status"
evidence = "$evidence_label; $stamp; progress=$bt_progress; $bt_evidence"

[telephony]
status = "$tel_status"
evidence = "$evidence_label; $stamp; progress=$tel_progress; $tel_evidence"

# Associated-radio diagnostics are intentionally not capabilities.toml
# sections: ferrix-hardware-api has no GNSS/NFC/ST54/UWB contract yet, and a
# kernel surface is not a supported Ferrix capability. Review the TSV and
# source audit before proposing a future target-local adapter.
# gnss: status=$gnss_status progress=$gnss_progress; $gnss_evidence
# nfc: status=$nfc_status progress=$nfc_progress; $nfc_evidence
# secure-element: status=$st54_status progress=$st54_progress; $st54_evidence
# uwb: status=$uwb_status progress=$uwb_progress; $uwb_evidence
FRAGMENT
}

# --- dispatch --------------------------------------------------------------

command_name=${1:-}
[ -n "$command_name" ] || usage
shift
case "$command_name" in
  preflight) [ "$#" -eq 0 ] || usage; open_transcript preflight; preflight; printf 'transcript=%s\n' "$transcript" ;;
  inventory) [ "$#" -eq 0 ] || usage; open_transcript inventory; inventory ;;
  associated) [ "$#" -eq 0 ] || usage; open_transcript associated; associated ;;
  enable) open_transcript enable; enable_radios "$@" ;;
  connect-wifi) open_transcript connect-wifi; connect_wifi "$@" ;;
  evidence) [ "$#" -eq 0 ] || usage; open_transcript inventory; evidence ;;
  redact) [ "$#" -eq 0 ] || usage; redact ;;
  *) usage ;;
esac
