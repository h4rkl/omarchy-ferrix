#!/bin/sh
set -eu

# Issue #92 S2 PREFLIGHT ONLY, for the exact Google Pixel 9a
# google-pixel-9a-tegu-gxq96-mp1-0 (tegu / GXQ96 / MP1.0) Ferrix dev guest,
# reached over the USB CDC-ACM bring-up shell (tools/pixel-9a/acm-probe.py).
# It collects the bounded, redacted facts a reviewer needs before a separate,
# owner-approved one-shot S2 manual CP boot, and reports GO/NO-GO for those
# facts only. It never boots the CP and implements no active S2 step.
#
#   s5300-s2-preflight.sh            read-only; there is no other mode
#
# Host side (no device contact): the checked-in ferrix-s5300-cpif.sh must
# match its package pin, the checked-in ferrix-s5300-mounts must be the
# service accepted for the issue #34 gate, and the recovery host artifacts
# are checked: target manifest gates, the pinned factory archive's presence,
# size and SHA-256. Those are reported as host-artifact evidence, never as a
# device fact.
#
# Guest side: one CDC-ACM session runs the checked-in library followed by
# s5300-s2-preflight-remote.sh. That reads the exact device tree, kernel,
# slot, hardware revision, backend, installed package files and versions,
# the CPIF kernel surface (CP must be OFFLINE), every CPIF node, a complete
# alias-aware opener scan, OpenRC service state, runlevel links and
# conf.d/rc.conf/local.d/udev overrides, the arm token, pidfiles, /run and
# the scratch directory, and source mount state from mountinfo and sysfs.
# It opens no CPIF node, writes nothing, starts, stops, mounts, provisions
# or arms nothing (s5300-s2-preflight.md).
#
# Result: preflight_result=go|no-go|incomplete and preflight_phase=
# pre-mount|post-mount. `go` only means that no read-only fact blocks the
# next manual step: in pre-mount, the owner-approved manual start of the
# accepted source mounts; in post-mount, review of an S2 run. It never
# authorises a CP boot (cp_boot_authorized=no). Mock runs report
# preflight_gate=not-evidence-mock.
#
# Exit status: 0 go, 2 usage, 3 transport or guest failure, 4 not the exact
# tegu board, 5 malformed probe output, 6 host input refused before the port
# is opened, 8 no-go, 9 incomplete.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained. Set
# FERRIX_PIXEL9A_S5300_S2_EVIDENCE=mock for stubbed runs. The transcript and
# JSON summary go, mode 0600, to ignored out/pixel-9a/s5300-s2-preflight/ and
# contain no serials, port names, labels, device names, file names, pids,
# kernel log, or per-unit bytes. Never commit them.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
board="$root/targets/boards/$target_id"
pkg="$board/pmos/device-google-tegu"
probe="$root/tools/pixel-9a/s5300-s2-preflight-remote.sh"
library="$pkg/ferrix-s5300-cpif.sh"
pins="$pkg/s5300-cpif-services.sha256"
mounts_initd="$pkg/ferrix-s5300-mounts.initd"
transport="$root/tools/pixel-9a/acm-probe.py"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/s5300-s2-preflight"
target_manifest="$board/manifest.toml"
recovery_pins=${FERRIX_PIXEL9A_PINS:-"$board/recovery.pins"}
recovery_dir=${FERRIX_PIXEL9A_RECOVERY_DIR:-"$board/recovery"}
accepted_mounts_sha256=b90d3b41ac175c93e5da5db0858327157da414bc9ca06d403c598108db22f74b
pinned_kernel=6.1.124-android14-11-ga4662a84a15f
hw_revision=MP1.0
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
nodes='umts_boot0 umts_ipc0 umts_ipc1 umts_rfs0 oem_ipc0 oem_ipc1 oem_ipc2 oem_ipc3 oem_ipc4 oem_ipc5 oem_ipc6 oem_ipc7'
modules='cpif shm_ipc cpif_page'
pinned_files='exynos-modem sit-proxy ferrix-s5300-mounts modemmanager s5300-cpif.sh s5300-userspace-sources.toml 99-google-tegu-sit.rules'
max_output=65536
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-180}

usage() {
	printf 'usage: tools/pixel-9a/s5300-s2-preflight.sh\n' >&2
	printf 'environment: FERRIX_SHELL (CDC-ACM node) FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, default 180)\n' >&2
	printf '             FERRIX_PIXEL9A_S5300_S2_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	exit 2
}

[ "$#" -eq 0 ] || usage
case "${FERRIX_PIXEL9A_S5300_S2_EVIDENCE:-physical}" in
	physical) evidence="physical:$target_id" ;;
	mock) evidence=mock ;;
	*) usage ;;
esac
case "$acm_timeout" in ''|*[!0-9]*) usage ;; esac
[ "$acm_timeout" -ge 1 ] && [ "$acm_timeout" -le 3600 ] || usage

transcript=''
die() {
	code=${2:-1}
	[ -z "$transcript" ] || printf 'result=refused\nreason=%s\n' "$1" >>"$transcript"
	printf 'REFUSING pixel-9a S5300 S2 preflight: %s\n' "$1" >&2
	printf '{"event":"pixel-9a-s5300-s2-preflight","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
		"$evidence" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

for tool in python3 awk sed; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly"
done
for file in "$probe" "$library" "$pins" "$mounts_initd" "$transport" "$target_manifest" "$pkg/APKBUILD" \
	"$board/pmos/modemmanager-samsung-sit/APKBUILD"; do
	[ -f "$file" ] && [ ! -L "$file" ] || die "reviewed input ${file#"$root"/} is missing or linked" 6
done

sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }

# --- host inputs, before any file is written or the port is opened ------------

# The library sent to the guest is exactly the pinned, alias-aware one.
library_sha256=$(sha256 "$library")
[ "$(awk '$2 == "/usr/lib/ferrix/s5300-cpif.sh" { n++; h = $1 } END { if (n == 1) print h }' "$pins")" = "$library_sha256" ] ||
	die 'checked-in ferrix-s5300-cpif.sh does not match its package pin' 6
[ "$(sha256 "$mounts_initd")" = "$accepted_mounts_sha256" ] ||
	die 'checked-in ferrix-s5300-mounts is not the service accepted for the #34 gate' 6
grep -qx "cpif_mounts_sha256=$accepted_mounts_sha256" "$library" ||
	die 'the library does not pin the accepted ferrix-s5300-mounts' 6

# Checked-in SHA-256 of each pinned installed file, by basename, from the pin
# list (s5300-cpif-services.test checks it against the sources).
expected_file_sha() {
	awk -v b="$1" '{ n = split($2, p, "/"); if (p[n] == b) { c++; h = $1 } } END { if (c == 1) print h }' "$pins"
}
for name in $pinned_files; do
	hash=$(expected_file_sha "$name")
	case "$hash" in ''|*[!0-9a-f]*) die "the checked-in pin list does not name $name exactly once" 6 ;; esac
	[ "${#hash}" -eq 64 ] || die "the checked-in pin list does not name $name exactly once" 6
done
expected_device_pkg=$(sed -n 's/^pkgver=//p' "$pkg/APKBUILD")-r$(sed -n 's/^pkgrel=//p' "$pkg/APKBUILD")
expected_mm_pkg=$(sed -n 's/^pkgver=//p' "$board/pmos/modemmanager-samsung-sit/APKBUILD")-r$(sed -n 's/^pkgrel=//p' "$board/pmos/modemmanager-samsung-sit/APKBUILD")
expected_modem_pkg=$(sed -n 's/^cpif_modem_version=//p' "$library")
case "$expected_device_pkg:$expected_mm_pkg:$expected_modem_pkg" in
	*-r[0-9]*:*-r[0-9]*:*-r[0-9]*) ;;
	*) die 'cannot read the expected package releases from the checked-in recipes' 6 ;;
esac

mkdir -p "$out"
umask 077
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/preflight-$stamp-$$.txt"
summary="$out/summary-$stamp-$$.json"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-s2.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
composed="$work/probe.sh"
cat "$library" "$probe" >"$composed"
{
	printf '# Pixel 9a S5300 S2 preflight (issue #92, read-only), %s UTC\n' "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# No serials, port names, labels, device names, pids, kernel log, or per-unit bytes are collected. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nsource=cdc-acm-read-only\ncp_nodes_opened=no\nstate_changes=none\n' "$target_id" "$evidence"
	printf 'library_sha256=%s\nprobe_body_sha256=%s\ncomposed_probe_sha256=%s\n' "$library_sha256" "$(sha256 "$probe")" "$(sha256 "$composed")"
} >"$transcript"

emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

blockers=''
skew=''
unknowns=''
host_blockers=''
add() { eval "$1=\"\${$1}\${$1:+,}\$2\""; }
block() { add blockers "$1"; }
image_skew() { add skew "$1"; add blockers "image-skew-$1"; }
unknown() { add unknowns "$1"; }

# --- host recovery artifact evidence (never a device fact) --------------------

manifest_gate() {
	awk -v wanted="$1" '
		/^\[gates\][[:space:]]*$/ { g = 1; next } /^\[/ { g = 0 }
		g && $0 ~ "^[[:space:]]*" wanted "[[:space:]]*=" { v = $0; sub(/^[^=]*=[[:space:]]*/, "", v); gsub(/[[:space:]"]/, "", v); print v; exit }
	' "$target_manifest"
}
gate_identity=$(manifest_gate identity)
gate_recovery=$(manifest_gate recovery_procedure)
[ "$gate_identity" = passed ] || add host_blockers host-identity-gate
case "$gate_recovery" in tested|passed|verified) ;; *) add host_blockers host-recovery-procedure ;; esac
host_recovery=missing
if [ -f "$recovery_pins" ] && [ ! -L "$recovery_pins" ]; then
	factory_name=$(awk -F '\t' '$1 == "factory" { n++; f = $4 } END { if (n == 1) print f }' "$recovery_pins")
	factory_hash=$(awk -F '\t' '$1 == "factory" { n++; h = $6 } END { if (n == 1) print h }' "$recovery_pins")
	factory_bytes=$(awk -F '\t' '$1 == "factory" { n++; b = $7 } END { if (n == 1) print b }' "$recovery_pins")
	case "$factory_name:$factory_hash:$factory_bytes" in
		*/*|:*|*::*|*:) host_recovery=pin-malformed ;;
		*)
			archive="$recovery_dir/$factory_name"
			if [ -L "$archive" ] || { [ -e "$archive" ] && [ ! -f "$archive" ]; }; then host_recovery=not-regular
			elif [ ! -f "$archive" ]; then host_recovery=missing
			elif [ "$(wc -c <"$archive" | tr -d ' ')" != "$factory_bytes" ]; then host_recovery=size-mismatch
			elif [ "$(sha256 "$archive")" != "$factory_hash" ]; then host_recovery=hash-mismatch
			else host_recovery=verified
			fi
			;;
	esac
else
	host_recovery=pin-missing
fi
[ "$host_recovery" = verified ] || add host_blockers "host-recovery-$host_recovery"
{
	printf '## host artifact evidence (this host; not a device fact)\n'
	printf 'host.manifest_identity=%s\nhost.manifest_recovery_procedure=%s\nhost.recovery_factory_archive=%s\n' \
		"${gate_identity:-missing}" "${gate_recovery:-missing}" "$host_recovery"
	printf 'host.expected.device-google-tegu=%s\nhost.expected.exynos-modem=%s\nhost.expected.modemmanager-samsung-sit=%s\n' \
		"$expected_device_pkg" "$expected_modem_pkg" "$expected_mm_pkg"
} >>"$transcript"

# --- the only guest session ---------------------------------------------------

session_status=0
python3 "$transport" --probe "$composed" --timeout "$acm_timeout" --max-bytes "$max_output" --stderr discard \
	>"$work/probe.out" 2>"$work/transport.log" || session_status=$?
case "$session_status" in
	0) ;;
	255)
		reason=$(sed -n 's/^ferrix-acm: .* reason=\([a-z-]*\)$/\1/p' "$work/transport.log" | sed -n '1p')
		die "guest CDC-ACM shell transport failed (${reason:-unknown})" 3
		;;
	4) die 'guest is not the exact tegu board (device-tree gate refused)' 4 ;;
	5) die 'guest body ran without the checked-in library' 5 ;;
	*) die "guest probe failed (exit $session_status)" 3 ;;
esac
raw="$work/probe.out"
[ "$(wc -c <"$raw" | tr -d ' ')" -le "$max_output" ] || die 'probe output exceeds the reviewed size limit' 5

# --- validate the probe output ------------------------------------------------

awk -v nodes="$nodes" -v modules="$modules" -v files="$pinned_files" '
	# Every enumerated or numeric field is checked here, once, so the
	# classification below never sees a value outside its reviewed set.
	function oneof(v, list,    n, a, i) { n = split(list, a, "|"); for (i = 1; i <= n; i++) if (v == a[i]) return 1; return 0 }
	function valid(k, v, count, fields, i) {
		if (k == "scratch_entries") return v == "unavailable" || (v ~ /^[0-9]+$/ && length(v) <= 6)
		if (k ~ /^(uid|slot_value_count|cp_platform_count|wwan_devices|class_node_count|class_extra_nodes|opener_holder_count|init_instances|rc_conf_mentions|confd_instances|local_d_mentions|udev_overrides|sit_socket_dir|vendor_mounts)$/ ||
			k ~ /^(proc|rc_daemon|runlevel|pidfile)\./ || k ~ /^part\.[a-z_]+\.(count|loops|direct_mounts|holders)$/ || k ~ /^dest\.[a-z_]+\.mounts$/)
			return v ~ /^[0-9]+$/ && length(v) <= 6
		if (k ~ /^file\..*\.sha256$/) return oneof(v, "missing|symlink|unreadable") || (v ~ /^[0-9a-f]+$/ && length(v) == 64)
		if (k ~ /^file\..*\.pin$/) return oneof(v, "match|mismatch|unpinned")
		if (k == "check.cli") return oneof(v, "pass|fail|skipped")
		if (k == "check.sources") return oneof(v, "pass|fail|deferred")
		if (k ~ /^(check|node)\./) return oneof(v, "pass|fail")
		if (k ~ /^service\./) return v == "stopped" || v == "unknown" || v ~ /^(started|starting|stopping|inactive|wasinactive|failed)(\+(started|starting|stopping|inactive|wasinactive|failed))*$/
		if (k ~ /^confd\./) return oneof(v, "absent|empty|expected|override|symlink|other")
		if (k ~ /^fw\./) return oneof(v, "present|missing")
		if (k ~ /^part\..*\.bdev_ro$/) return oneof(v, "0|1|unknown")
		if (k ~ /^dest\..*\.source$/) return oneof(v, "loop|other|none")
		if (k ~ /^dest\..*\.ro$/) return oneof(v, "yes|no|none")
		if (k == "slot_source") return oneof(v, "bootconfig|cmdline|none")
		if (k == "slot_suffix") return oneof(v, "_a|_b|other|none")
		if (k == "pidns") return oneof(v, "initial|other|unreadable")
		if (k == "env_file" || k == "pins_file") return oneof(v, "present|missing|symlink")
		if (k == "env.target") return oneof(v, "exact|other|missing|duplicate")
		if (k == "env.backend") return oneof(v, "google-kleaf-cpif|linux-google-tegu-wwan|missing|duplicate|empty|other")
		if (k == "opener_scan") return oneof(v, "clean|held|incomplete|not-run")
		if (k == "opener_held_nodes") {
			if (v == "none") return 1
			if (v == "") return 0
			count = split(v, fields, ",")
			if (count > 13) return 0
			for (i = 1; i <= count; i++) if (!(fields[i] in known_node) && fields[i] != "other") return 0
			return 1
		}
		if (k == "opener_holder_comms") {
			if (v == "none") return 1
			if (v == "") return 0
			count = split(v, fields, ",")
			if (count > 4) return 0
			for (i = 1; i <= count; i++) if (!oneof(fields[i], "ModemManager|exynos-modem|sit-proxy|other")) return 0
			return 1
		}
		if (k == "openrc_state_dir") return oneof(v, "present|missing")
		if (k == "arm_token") return oneof(v, "absent|present|symlink")
		if (k == "scratch") return oneof(v, "absent|ok|symlink|not-directory|wrong-owner-mode")
		return 1
	}
	BEGIN {
		split("probe dt_model dt_compatible kernel_release uid hw_revision slot_source slot_value_count slot_suffix pidns env_file env.target env.backend check.target pins_file check.package pkg.device-google-tegu pkg.exynos-modem pkg.modemmanager-samsung-sit check.cli cp_platform_count cp_driver modem_state wwan_devices check.surface class_node_count class_extra_nodes opener_scan opener_holder_count opener_held_nodes opener_holder_comms proc.s5300_daemons proc.modemmanager openrc_state_dir service.ferrix-s5300-mounts service.exynos-modem service.sit-proxy service.modemmanager service.ferrix-radio-firmware rc_daemon.exynos-modem rc_daemon.sit-proxy runlevel.ferrix-s5300-mounts runlevel.exynos-modem runlevel.sit-proxy init_instances confd.ferrix-s5300-mounts confd.exynos-modem confd.sit-proxy confd.modemmanager rc_conf_mentions confd_instances local_d_mentions udev_overrides arm_token pidfile.exynos-modem pidfile.sit-proxy sit_socket_dir check.run scratch scratch_entries fw.uecapconfig fw.carrierconfig vendor_mounts check.sources probe_complete", g, " ")
		for (i in g) required[g[i]] = 1
		split(nodes, n, " "); for (i in n) { required["node." n[i]] = 1; known_node[n[i]] = 1 }
		split(modules, m, " "); for (i in m) required["module." m[i]] = 1
		split(files, f, " "); for (i in f) { required["file." f[i] ".sha256"] = 1; required["file." f[i] ".pin"] = 1 }
		split("modem_img efs persist modem_userdata", d, " "); split("mounts source ro fstype", df, " ")
		for (i in d) for (j in df) required["dest." d[i] "." df[j]] = 1
		split("modem efs persist modem_userdata", r, " "); split("count bdev_ro loops direct_mounts holders", rf, " ")
		for (i in r) for (j in rf) required["part." r[i] "." rf[j]] = 1
	}
	{
		eq = index($0, "=")
		if (eq < 2) { bad = 1; next }
		key = substr($0, 1, eq - 1)
		value = substr($0, eq + 1)
		if (!(key in required) || (key in seen)) bad = 1
		if (value !~ /^[A-Za-z0-9 ._:,+~-]*$/) bad = 1
		if (!valid(key, value)) bad = 1
		seen[key] = 1
	}
	END { if (bad) exit 1; for (k in required) if (!(k in seen)) exit 1 }
' "$raw" || die 'probe output has a malformed, unknown, duplicate, or missing field' 5

get() {
	awk -v key="$1=" 'index($0, key) == 1 { print substr($0, length(key) + 1); exit }' "$raw"
}
# The validator above already constrained every enumerated and numeric
# field, so these are plain lookups (a refusal inside $(...) would only exit
# the subshell).
num() { get "$1"; }
oneof() { get "$1"; }
passfail() { get "$1"; }

[ "$(get probe)" = s5300-s2-preflight ] && [ "$(get probe_complete)" = yes ] || die 'probe did not complete' 5
[ "$(get dt_model)" = "$dt_model" ] && [ "$(get dt_compatible)" = "$dt_compatible" ] ||
	die 'probe reported a device tree other than the exact tegu board' 4

# --- classify: live device facts ---------------------------------------------

kernel=$(get kernel_release)
emit kernel_release "$kernel"
[ "$kernel" = "$pinned_kernel" ] || block kernel-not-pinned
uid=$(num uid)
[ "$uid" -eq 0 ] || block probe-not-root
revision=$(get hw_revision)
emit hw_revision "$revision"
case "$revision" in
	"$hw_revision") ;;
	absent) unknown hw-revision-not-in-bootconfig ;;
	*) block hw-revision-not-mp1.0 ;;
esac
slot=$(oneof slot_suffix _a _b other none)
emit slot_suffix "$slot"
emit slot_source "$(oneof slot_source bootconfig cmdline none)"
case "$slot:$(num slot_value_count)" in _a:1|_b:1) ;; *) block slot-not-exactly-one ;; esac
pidns=$(oneof pidns initial other unreadable)
emit pidns "$pidns"
[ "$pidns" = initial ] || block not-initial-pid-namespace
env_target=$(oneof env.target exact other missing duplicate)
env_backend=$(get env.backend)
emit env_file "$(oneof env_file present missing symlink)"
emit backend "$env_backend"
[ "$env_target" = exact ] || block "target-env-$env_target"
[ "$env_backend" = google-kleaf-cpif ] || block backend-not-google-kleaf-cpif
[ "$(passfail check.target)" = pass ] || block target-check

# Image skew: the installed files and releases against the checked-in ones.
# These cannot pass until a rebuilt image is flashed.
emit pins_file "$(oneof pins_file present missing symlink)"
for name in $pinned_files; do
	installed=$(get "file.$name.sha256")
	if [ "$installed" = "$(expected_file_sha "$name")" ]; then state=current
	else
		case "$installed" in missing|symlink|unreadable) state=$installed ;; *) state=differs ;; esac
		image_skew "file-$name"
	fi
	emit "installed.$name" "$state"
	emit "installed.$name.on_device_pin" "$(oneof "file.$name.pin" match mismatch unpinned)"
done
[ "$(get file.ferrix-s5300-mounts.sha256)" = "$accepted_mounts_sha256" ] || block mounts-service-not-accepted-34
for pair in "device-google-tegu|$expected_device_pkg" "exynos-modem|$expected_modem_pkg" "modemmanager-samsung-sit|$expected_mm_pkg"; do
	name=${pair%%|*}
	installed=$(get "pkg.$name")
	emit "pkg.$name" "$installed"
	[ "$installed" = "${pair#*|}" ] || image_skew "pkg-$name"
done
[ "$(passfail check.package)" = pass ] || block package-check
cli=$(oneof check.cli pass fail skipped)
emit check.cli "$cli"
[ "$cli" = pass ] || block "cli-$cli"

# Kernel surface, nodes, and openers.
for name in $modules; do
	state=$(get "module.$name")
	emit "module.$name" "$state"
	[ "$state" = Live ] || block "module-$name-not-live"
done
cp_count=$(num cp_platform_count)
cp_driver=$(get cp_driver)
modem_state=$(get modem_state)
emit cp_platform_count "$cp_count"
emit cp_driver "$cp_driver"
emit modem_state "$modem_state"
[ "$cp_count" -eq 1 ] || block cp-platform-not-exactly-one
[ "$cp_driver" = cp_interface ] || block cp-driver-not-cp_interface
case "$modem_state" in
	OFFLINE) ;;
	unreadable) unknown modem-state ;;
	*) block cp-not-offline ;;
esac
wwan=$(num wwan_devices)
emit wwan_devices "$wwan"
[ "$wwan" -eq 0 ] || block wwan-surface-present
[ "$(passfail check.surface)" = pass ] || block surface-check
failed_nodes=''
for name in $nodes; do
	[ "$(passfail "node.$name")" = pass ] || failed_nodes="$failed_nodes${failed_nodes:+,}$name"
done
emit nodes_failed "${failed_nodes:-none}"
[ -z "$failed_nodes" ] || block node-check
emit class_node_count "$(num class_node_count)"
emit class_extra_nodes "$(num class_extra_nodes)"
scan=$(oneof opener_scan clean held incomplete not-run)
emit opener_scan "$scan"
emit opener_holder_count "$(num opener_holder_count)"
emit opener_held_nodes "$(get opener_held_nodes)"
emit opener_holder_comms "$(get opener_holder_comms)"
case "$scan" in
	clean) ;;
	held) block cpif-node-held ;;
	incomplete) block opener-scan-incomplete ;;
	*) block opener-scan-not-run ;;
esac
daemons=$(num proc.s5300_daemons)
emit s5300_daemon_processes "$daemons"
[ "$daemons" -eq 0 ] || block s5300-daemon-running
emit modemmanager_processes "$(num proc.modemmanager)"

# OpenRC: services, runlevels, overrides.
emit openrc_state_dir "$(oneof openrc_state_dir present missing)"
[ "$(get openrc_state_dir)" = present ] || unknown openrc-state
mounts_state=$(get service.ferrix-s5300-mounts)
for name in ferrix-s5300-mounts exynos-modem sit-proxy modemmanager ferrix-radio-firmware; do
	emit "service.$name" "$(get "service.$name")"
done
for name in exynos-modem sit-proxy; do
	state=$(get "service.$name")
	case "$state" in stopped) ;; *) block "service-$name-$state" ;; esac
	[ "$(num "rc_daemon.$name")" -eq 0 ] || block "stale-openrc-daemon-record-$name"
	[ "$(num "pidfile.$name")" -eq 0 ] || block "stale-pidfile-$name"
done
case "$mounts_state" in
	stopped) phase=pre-mount ;;
	started) phase=post-mount ;;
	unknown) phase=indeterminate ;;
	*) phase=indeterminate; block "service-ferrix-s5300-mounts-$mounts_state" ;;
esac
for name in ferrix-s5300-mounts exynos-modem sit-proxy; do
	links=$(num "runlevel.$name")
	emit "runlevel.$name" "$links"
	[ "$links" -eq 0 ] || block "runlevel-link-$name"
done
for key in init_instances confd_instances rc_conf_mentions local_d_mentions udev_overrides; do
	count=$(num "$key")
	emit "$key" "$count"
	[ "$count" -eq 0 ] || block "openrc-override-$key"
done
for name in ferrix-s5300-mounts exynos-modem sit-proxy modemmanager; do
	state=$(oneof "confd.$name" absent empty expected override symlink other)
	emit "confd.$name" "$state"
	case "$name:$state" in
		*:absent|*:empty|modemmanager:expected) ;;
		*) block "openrc-override-confd-$name" ;;
	esac
done

# Manual-start state.
token=$(oneof arm_token absent present symlink)
emit arm_token "$token"
[ "$token" = absent ] || block arm-token-present
emit sit_socket_dir "$(num sit_socket_dir)"
[ "$(passfail check.run)" = pass ] || block run-not-root-tmpfs
scratch=$(oneof scratch absent ok symlink not-directory wrong-owner-mode)
scratch_entries=$(get scratch_entries)
emit scratch "$scratch"
emit scratch_entries "$scratch_entries"
case "$scratch" in
	ok) case "$scratch_entries" in 0) ;; unavailable) block scratch-list-incomplete ;; *) block scratch-not-empty ;; esac ;;
	absent) ;;
	*) block "scratch-$scratch" ;;
esac
for name in uecapconfig carrierconfig; do
	state=$(oneof "fw.$name" present missing)
	emit "fw.$name" "$state"
	[ "$state" = present ] || block "firmware-dir-$name-missing"
done

# Source mounts: in pre-mount nothing may be mounted, attached, or held; in
# post-mount all four must be single read-only loop mounts and pass the
# accepted verify path. Anything in between is a partial or stale state.
vendor_mounts=$(num vendor_mounts)
emit vendor_mounts "$vendor_mounts"
prior_mount=no
for role in modem efs persist modem_userdata; do
	count=$(num "part.$role.count")
	ro=$(oneof "part.$role.bdev_ro" 0 1 unknown)
	loops=$(num "part.$role.loops")
	direct=$(num "part.$role.direct_mounts")
	holders=$(num "part.$role.holders")
	emit "part.$role" "count=$count bdev_ro=$ro loops=$loops direct_mounts=$direct holders=$holders"
	[ "$count" -eq 1 ] || block "partition-$role-not-unique"
	[ "$direct" -eq 0 ] || block "partition-$role-mounted-directly"
	[ "$ro" != 1 ] || prior_mount=yes
	case "$phase" in
		pre-mount)
			[ "$loops" -eq 0 ] || block "stale-loop-$role"
			[ "$holders" -eq 0 ] || block "partition-$role-held"
			;;
		post-mount)
			[ "$loops" -eq 1 ] || block "partial-mount-loop-$role"
			[ "$ro" = 1 ] || block "partition-$role-not-blkroset"
			;;
	esac
done
for dest in modem_img efs persist modem_userdata; do
	mounts=$(num "dest.$dest.mounts")
	source=$(oneof "dest.$dest.source" loop other none)
	ro=$(oneof "dest.$dest.ro" yes no none)
	emit "dest.$dest" "mounts=$mounts source=$source ro=$ro fstype=$(get "dest.$dest.fstype")"
	case "$phase" in
		pre-mount) [ "$mounts" -eq 0 ] || block "stale-mount-$dest" ;;
		post-mount) [ "$mounts" -eq 1 ] && [ "$source" = loop ] && [ "$ro" = yes ] || block "partial-mount-$dest" ;;
	esac
done
[ "$phase" != pre-mount ] || [ "$vendor_mounts" -eq 0 ] || block stale-vendor-mount
emit partition_ro_flag_since_boot "$prior_mount"
sources=$(oneof check.sources pass fail deferred)
emit check.sources "$sources"
case "$phase:$sources" in
	pre-mount:deferred) ;;
	post-mount:pass) ;;
	post-mount:*) block sources-check ;;
	*) block sources-state-inconsistent ;;
esac

# --- result ------------------------------------------------------------------

deferred=''
case "$phase" in
	pre-mount) deferred='sources-verify-own-mount,modem-image-metadata,nv-size-metadata,cpsha-metadata,post-mount-partial-detection' ;;
esac
deferred_active='scratch-provision,arm,token-consumption,start-pre-preflight,cp-boot-observation,daemon-and-dmesg-capture,online-10-min,teardown'
all_blockers="$blockers${blockers:+${host_blockers:+,}}$host_blockers"
if [ -n "$all_blockers" ]; then result=no-go
elif [ -n "$unknowns" ]; then result=incomplete
else result=go
fi
emit preflight_phase "$phase"
emit device_blockers "${blockers:-none}"
emit image_skew "${skew:-none}"
emit host_blockers "${host_blockers:-none}"
emit host_recovery_factory_archive "$host_recovery"
emit unknown_facts "${unknowns:-none}"
emit deferred_until_mounts "${deferred:-none}"
emit deferred_to_active_workflow "$deferred_active"
emit preflight_result "$result"
if [ "$evidence" = mock ]; then gate=not-evidence-mock; else gate=$result; fi
emit preflight_gate "$gate"
emit cp_boot_authorized no
emit active_s2_workflow not-implemented
emit capability_change none
{
	printf '## raw probe output\n'
	cat "$raw"
} >>"$transcript"

json_list() {
	printf '%s' "$1" | awk -F, '{ for (i = 1; i <= NF; i++) printf "%s\"%s\"", (i > 1 ? "," : ""), $i }'
}
{
	printf '{"event":"pixel-9a-s5300-s2-preflight","target_id":"%s","evidence":"%s",' "$target_id" "$evidence"
	printf '"preflight_result":"%s","preflight_gate":"%s","preflight_phase":"%s",' "$result" "$gate" "$phase"
	printf '"device_blockers":[%s],"image_skew":[%s],"host_blockers":[%s],"unknown_facts":[%s],' \
		"$(json_list "$blockers")" "$(json_list "$skew")" "$(json_list "$host_blockers")" "$(json_list "$unknowns")"
	printf '"deferred_until_mounts":[%s],"deferred_to_active_workflow":[%s],' "$(json_list "$deferred")" "$(json_list "$deferred_active")"
	printf '"host_recovery_factory_archive":"%s","modem_state":"%s","opener_scan":"%s",' "$host_recovery" "$modem_state" "$scan"
	printf '"cp_nodes_opened":false,"state_changes":"none","cp_boot_authorized":false,"active_s2_workflow":"not-implemented",'
	printf '"transcript":"%s"}\n' "${transcript##*/}"
} >"$summary"
cp "$transcript" "$out/preflight-latest.txt"
cp "$summary" "$out/summary-latest.json"
printf 'transcript=%s\nsummary=%s\n' "$transcript" "$summary"
printf 'This is a read-only preflight. It does not boot the CP or authorise S2.\n'
case "$result" in
	go) exit 0 ;;
	incomplete) exit 9 ;;
	*) exit 8 ;;
esac
