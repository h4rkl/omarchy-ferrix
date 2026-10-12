#!/bin/sh
set -eu

# Issue #34 recovery-suppression proof for the four S5300 modem source
# partitions (active-slot modem, efs, persist, modem_userdata) on an ALREADY
# booted Google Pixel 9a (tegu / GXQ96 / MP1.0) Ferrix dev guest, reached over
# the USB CDC-ACM bring-up shell (tools/pixel-9a/acm-probe.py).
#
#   s5300-mount-proof.sh [preflight]   read-only; the default
#   s5300-mount-proof.sh mount ROLE    one gated read-only mount of ROLE
#
# preflight checks the exact device tree, the pinned Kleaf kernel, that
# ferrix-s5300-mounts and exynos-modem are out of every runlevel, nothing
# is mounted under /mnt/vendor, util-linux losetup and an unbound loop device
# whose `ro` reads 0 exist, and, for every role, unique partition
# resolution, the superblock type from util-linux `blkid -p`, the block-device
# read-only flag, queue iostats, holders, existing mounts, and the sysfs `stat`
# counters. It mounts nothing and changes no block-device state.
#
# mount needs the typed confirmation
#   FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM='mount ROLE read-only on google-pixel-9a-tegu-gxq96-mp1-0'
# and, for the per-unit partitions that the pinned factory image does NOT
# rewrite (efs, persist, modem_userdata), the recorded owner decision
#   FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=owner-approved-issue34-per-unit-ro-mount
# It runs a fresh preflight session, prints the bound partition, then sends a
# second session that refuses unless the same partition is still unique,
# unmounted, and of the same type. That session marks the partition's block
# device read-only (BLKROSET), attaches it to a loop device with
# `losetup --read-only`, checks that the loop reads read-only and maps exactly
# the whole partition, mounts the LOOP once with the exact ferrix-s5300-mounts
# options under /run, reads the directory tree, unmounts, detaches the loop,
# and reports counters of both devices, mountinfo, and kernel log lines. The
# partition's read-only flag stays set until the next boot.
#
# The read-only loop is the write barrier: in the pinned 6.1 kernel a loop
# configured read-only fails every write, discard, and write-zeroes request
# before it reaches the backing partition, while BLKROSET on the partition
# only makes the block layer warn. The barrier covers I/O through the loop
# only; other writers of the partition, the whole disk, or UFS passthrough are
# excluded by the preflight checks, not by the kernel. Unchanged partition
# counters are consistent with no write, not proof of absence.
#
# No result ever passes the issue #34 gate. The gate stays `blocked` unless the
# checked-in ferrix-s5300-mounts meets the read-only-loop contract and the
# guest's installed copy is byte-identical to it; then the best outcome is
# `owner-review-required`.
#
# Exit status: 0 completed (mount: no anomaly), 2 usage or missing gate,
# 3 transport or guest failure, 4 not the exact tegu board, 5 malformed probe
# output, 6 refused before any state change, 7 mount session did not finish
# cleanly after the read-only flag was set, 8 mount observation has anomalies.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained. Set
# FERRIX_PIXEL9A_S5300_MOUNT_PROOF_EVIDENCE=mock for stubbed runs. Transcripts
# go to ignored out/pixel-9a/s5300-mount-proof/ and contain no serials,
# UUIDs, labels, port names, file names, or partition bytes. Never commit them.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
probe="$root/tools/pixel-9a/s5300-mount-proof-remote.sh"
transport="$root/tools/pixel-9a/acm-probe.py"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/s5300-mount-proof"
roles='modem_img efs persist modem_userdata'
per_unit_roles='efs persist modem_userdata'
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
pinned_kernel=6.1.124-android14-11-ga4662a84a15f
service_initd="$root/targets/boards/$target_id/pmos/device-google-tegu/ferrix-s5300-mounts.initd"
service_apkbuild="$root/targets/boards/$target_id/pmos/device-google-tegu/APKBUILD"
per_unit_decision=owner-approved-issue34-per-unit-ro-mount
max_output=65536
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-90}

usage() {
	printf 'usage: tools/pixel-9a/s5300-mount-proof.sh [preflight | mount modem_img|efs|persist|modem_userdata]\n' >&2
	printf 'environment: FERRIX_SHELL (CDC-ACM node) FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, default 90)\n' >&2
	printf '             FERRIX_PIXEL9A_S5300_MOUNT_PROOF_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	printf '             mount: FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM, FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION\n' >&2
	exit 2
}

mode=preflight
role=''
case "$#:${1:-}" in
	0:|1:preflight) ;;
	2:mount)
		mode=mount
		role=$2
		case " $roles " in *" $role "*) ;; *) usage ;; esac
		;;
	*) usage ;;
esac
case "${FERRIX_PIXEL9A_S5300_MOUNT_PROOF_EVIDENCE:-physical}" in
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
	printf 'REFUSING pixel-9a S5300 mount proof: %s\n' "$1" >&2
	printf '{"event":"pixel-9a-s5300-mount-proof","mode":"%s","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
		"$mode" "$evidence" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

# The mount gate is checked before any file is written or the port is opened.
if [ "$mode" = mount ]; then
	expected_confirm="mount $role read-only on $target_id"
	[ "${FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM:-}" = "$expected_confirm" ] ||
		die "set FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM='$expected_confirm' to mount $role once" 2
	case " $per_unit_roles " in
		*" $role "*)
			[ "${FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION:-}" = "$per_unit_decision" ] ||
				die "$role is a per-unit partition outside factory recovery; mounting it needs the recorded owner decision" 2
			;;
	esac
fi

for tool in python3 awk sed; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly"
done
[ -f "$probe" ] && [ ! -L "$probe" ] || die 'guest probe script is missing or linked'
[ -f "$transport" ] && [ ! -L "$transport" ] || die 'CDC-ACM transport is missing or linked'

sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/$mode-$stamp-$$.txt"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-mount-proof.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
{
	printf '# Pixel 9a S5300 source mount proof (%s), %s UTC\n' "$mode" "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# No serials, port names, filesystem UUIDs/labels, file names, or partition contents are collected. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nmode=%s\nsource=cdc-acm\n' "$target_id" "$evidence" "$mode"
	printf 'probe_body_sha256=%s\n' "$(sha256 "$probe")"
} >"$transcript"

emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

# session NAME MODE ROLE DEV DEVNAME FSTYPE: compose the five-line header and
# the exact probe body, send it as the only content of one CDC-ACM session,
# and leave the output in $work/NAME.out. Sets session_status.
session() {
	composed="$work/$1.sh"
	{
		printf 'proof_mode=%s\nproof_role=%s\nproof_dev=%s\nproof_devname=%s\nproof_fstype=%s\n' "$2" "$3" "$4" "$5" "$6"
		cat "$probe"
	} >"$composed"
	printf '%s_probe_sha256=%s\n' "$1" "$(sha256 "$composed")" >>"$transcript"
	session_status=0
	python3 "$transport" --probe "$composed" --timeout "$acm_timeout" --max-bytes "$max_output" --stderr discard \
		>"$work/$1.out" 2>"$work/$1.log" || session_status=$?
	case "$session_status" in
		255)
			reason=$(sed -n 's/^ferrix-acm: .* reason=\([a-z-]*\)$/\1/p' "$work/$1.log" | sed -n '1p')
			die "guest CDC-ACM shell transport failed (${reason:-unknown})" 3
			;;
		4) die 'guest is not the exact tegu board (device-tree gate refused)' 4 ;;
	esac
	[ "$(wc -c <"$work/$1.out" | tr -d ' ')" -le "$max_output" ] || die 'probe output exceeds the reviewed size limit' 5
}

# validate FILE MODE [ROLE]: every line is a known, unique key with a value in
# the reviewed character set, and every required key is present.
validate() {
	awk -v mode="$2" -v roles="${3:-$roles}" '
		BEGIN {
			split("probe mode dt_model dt_compatible kernel_release uid service_sha256 blkid_impl blockdev_impl losetup_impl loop_free kernel_fs.ext4 kernel_fs.f2fs runlevel.ferrix-s5300-mounts runlevel.exynos-modem mountinfo_readable vendor_mounts proof_mounts kmsg_usable slot_source slot_value_count slot_suffix probe_complete", g, " ")
			for (i in g) required[g[i]] = 1
			split(roles, r, " ")
			split("label link_state link_device partname_count partname_devices dev parent bdev_ro iostats holders stat blkid_exit blkid_type mounts", f, " ")
			for (i in r) for (j in f) required["part." r[i] "." f[j]] = 1
			if (mode == "mount") {
				split("mount_options state_changed", m, " ")
				for (i in m) required[m[i]] = 1
				split("refusal loop_name stat.initial loop_stat.initial setro_exit bdev_ro.before loop_ro.before losetup_exit loop_dev loop_ro loop_backing_match loop_geometry loop_iostats loop_type stat.before loop_stat.before mountinfo mount_exit entries stat.mounted stat.settled loop_stat.mounted loop_stat.settled loop_stat.after umount_exit still_mounted detach_exit still_bound stat.after bdev_ro.after", o, " ")
				for (i in o) optional[o[i]] = 1
			}
		}
		{
			eq = index($0, "=")
			if (eq < 2) { bad = 1; next }
			key = substr($0, 1, eq - 1)
			value = substr($0, eq + 1)
			if (mode == "mount" && key ~ /^kmsg\.[0-9][0-9]$/) {
				if (value !~ /^[A-Za-z0-9 ._:,+~()\/=-]*$/) bad = 1
				if (key in seen) bad = 1
				seen[key] = 1
				next
			}
			if (!(key in required) && !(key in optional)) bad = 1
			if (key in seen) bad = 1
			if (value !~ /^[A-Za-z0-9 ._:,+~\/=-]*$/) bad = 1
			seen[key] = 1
		}
		END { if (bad) exit 1; for (k in required) if (!(k in seen)) exit 1 }
	' "$1"
}

get() {
	awk -v key="$2=" 'index($0, key) == 1 { print substr($0, length(key) + 1); exit }' "$1"
}

expected_type() {
	case "$1" in modem_img) printf 'ext4\n' ;; *) printf 'f2fs\n' ;; esac
}

# The issue #34 gate covers the production service only if the service mounts
# through the same protected path as this proof, and only if the guest runs
# that exact service. The first part is a static check of the checked-in
# service against the contract that tools/pixel-9a/tests/s5300-mounts-service.sh
# exercises: its only mount call mounts the attached loop, it sets BLKROSET
# and attaches with `losetup --read-only`, both a new and an existing mount
# go through verify_loop, and the package ships exactly this file with the
# util-linux tools it needs. The second part compares the guest's installed
# /etc/init.d/ferrix-s5300-mounts with the checked-in file. Both can only
# block the gate, never pass it.
service_contract() {
	[ -f "$service_initd" ] && [ -f "$service_apkbuild" ] || return 1
	code=$(grep -v '^[[:space:]]*#' "$service_initd")
	[ "$(printf '%s\n' "$code" | grep -cE '(^|[;&|{(]|then|do)[[:space:]]*mount[[:space:]]')" -eq 1 ] || return 1
	printf '%s\n' "$code" | grep -Fq 'mount -t "$fstype" -o "ro,$(recovery_option "$fstype"),nosuid,nodev,noexec" -- "$dev_root/$attached" "$destination"' || return 1
	printf '%s\n' "$code" | grep -Eq '^[[:space:]]*blockdev --setro "\$device"' || return 1
	printf '%s\n' "$code" | grep -Fq 'losetup --read-only -- "$dev_root/$attached" "$device"' || return 1
	printf '%s\n' "$code" | grep -Fq 'verify_loop "$attached" "$device" "$fstype"' || return 1
	printf '%s\n' "$code" | grep -Fq 'verify_loop "$loop_name" "$device" "$fstype"' || return 1
	! printf '%s\n' "$code" | grep -Eq 'setrw|[l]osetup -[a-z]*f|--find' || return 1
	for package in blkid blockdev losetup; do
		grep -Eq "^[[:space:]]*$package[[:space:]]*\$" "$service_apkbuild" || return 1
	done
	sha512=$( (shasum -a 512 "$service_initd" 2>/dev/null || sha512sum "$service_initd") | cut -c1-128)
	[ "$(awk '$2 == "ferrix-s5300-mounts.initd" { print $1 }' "$service_apkbuild")" = "$sha512" ]
}
service_path=unprotected
service_contract && service_path=read-only-loop
service_sha256=missing
[ ! -f "$service_initd" ] || service_sha256=$(sha256 "$service_initd")
service_installed=unknown
gate_blockers=''
gate_for() {
	if [ "$evidence" = mock ]; then printf 'not-evaluated-mock\n'
	elif [ -n "$1" ] || [ -n "$gate_blockers" ]; then printf 'blocked\n'
	else printf 'owner-review-required\n'
	fi
}

# --- preflight session (always first; read-only) ------------------------------

session preflight preflight '' '' '' ''
[ "$session_status" -eq 0 ] || die "guest preflight probe failed (exit $session_status)" 3
pre="$work/preflight.out"
validate "$pre" preflight || die 'preflight output has a malformed, unknown, duplicate, or missing field' 5
[ "$(get "$pre" probe)" = s5300-mount-proof ] && [ "$(get "$pre" mode)" = preflight ] && [ "$(get "$pre" probe_complete)" = yes ] ||
	die 'preflight probe did not complete' 5
[ "$(get "$pre" dt_model)" = "$dt_model" ] && [ "$(get "$pre" dt_compatible)" = "$dt_compatible" ] ||
	die 'probe reported a device tree other than the exact tegu board' 4

problems=''
problem() { problems="$problems${problems:+,}$1"; }
role_problems=''
# Joined with "+" so the global list stays comma-separated.
role_problem() { role_problems="$role_problems${role_problems:++}$1"; }

kernel_release=$(get "$pre" kernel_release)
slot=$(get "$pre" slot_suffix)
emit kernel_release "$kernel_release"
emit kernel_pinned "$([ "$kernel_release" = "$pinned_kernel" ] && echo yes || echo no)"
emit guest_uid "$(get "$pre" uid)"
emit blkid_impl "$(get "$pre" blkid_impl)"
emit blockdev_impl "$(get "$pre" blockdev_impl)"
emit losetup_impl "$(get "$pre" losetup_impl)"
emit loop_free "$(get "$pre" loop_free)"
emit slot_suffix "$slot"
for key in kernel_fs.ext4 kernel_fs.f2fs runlevel.ferrix-s5300-mounts runlevel.exynos-modem mountinfo_readable vendor_mounts proof_mounts kmsg_usable; do
	emit "$key" "$(get "$pre" "$key")"
done
[ "$kernel_release" = "$pinned_kernel" ] || problem kernel-not-pinned
[ "$(get "$pre" uid)" = 0 ] || problem not-root
[ "$(get "$pre" blkid_impl)" = util-linux ] || problem blkid-not-util-linux
[ "$(get "$pre" blockdev_impl)" != missing ] || problem blockdev-missing
[ "$(get "$pre" losetup_impl)" = util-linux ] || problem losetup-not-util-linux
case "$(get "$pre" loop_free)" in
	none) problem no-unbound-loop-with-ro-0 ;;
	loop[0-9]|loop[0-9][0-9]|loop[0-9][0-9][0-9]) ;;
	*) die 'probe reported a malformed loop device name' 5 ;;
esac
[ "$(get "$pre" kernel_fs.ext4)" = yes ] || problem ext4-not-in-kernel
[ "$(get "$pre" kernel_fs.f2fs)" = yes ] || problem f2fs-not-in-kernel
[ "$(get "$pre" runlevel.ferrix-s5300-mounts)" = absent ] || problem ferrix-s5300-mounts-in-runlevel
[ "$(get "$pre" runlevel.exynos-modem)" = absent ] || problem exynos-modem-in-runlevel
[ "$(get "$pre" mountinfo_readable)" = yes ] || problem mountinfo-unreadable
[ "$(get "$pre" vendor_mounts)" = 0 ] || problem vendor-mount-present
[ "$(get "$pre" proof_mounts)" = 0 ] || problem proof-mount-present
[ "$(get "$pre" kmsg_usable)" = yes ] || problem kmsg-unusable
case "$slot" in _a|_b) ;; *) problem "slot-$slot" ;; esac

for each in $roles; do
	role_problems=''
	label=$(get "$pre" "part.$each.label")
	expected_label=$each
	[ "$each" != modem_img ] || case "$slot" in _a|_b) expected_label="modem$slot" ;; *) expected_label=unresolved ;; esac
	[ "$label" = "$expected_label" ] || die "probe reported label '$label' for $each (expected $expected_label)" 5
	link_state=$(get "$pre" "part.$each.link_state")
	link_device=$(get "$pre" "part.$each.link_device")
	count=$(get "$pre" "part.$each.partname_count")
	devices=$(get "$pre" "part.$each.partname_devices")
	dev=$(get "$pre" "part.$each.dev")
	blkid_exit=$(get "$pre" "part.$each.blkid_exit")
	blkid_type=$(get "$pre" "part.$each.blkid_type")
	stat=$(get "$pre" "part.$each.stat")
	case "$count" in ''|*[!0-9]*) die "probe reported a malformed PARTNAME count for $each" 5 ;; esac
	if [ "$label" = unresolved ]; then role_problem slot-unresolved
	elif [ "$link_state" != block ]; then role_problem "partlabel-$link_state"
	elif [ "$count" -ne 1 ]; then role_problem "partname-count-$count"
	elif [ "$devices" != "$link_device" ]; then role_problem device-mismatch
	fi
	case "$link_device" in *[!a-z0-9]*) die "probe reported a malformed device name for $each" 5 ;; esac
	if [ -z "$role_problems" ]; then
		case "$dev" in [0-9]*:[0-9]*) ;; *) die "probe reported a malformed device number for $each" 5 ;; esac
		[ "$blkid_exit" = 0 ] && [ "$blkid_type" = "$(expected_type "$each")" ] ||
			role_problem "type-${blkid_type:-exit-$blkid_exit}-expected-$(expected_type "$each")"
		[ "$(get "$pre" "part.$each.mounts")" = 0 ] || role_problem already-mounted
		[ "$(get "$pre" "part.$each.holders")" = 0 ] || role_problem has-holders
		[ "$(get "$pre" "part.$each.iostats")" = 1 ] || role_problem iostats-not-enabled
		[ "$stat" != unavailable ] || role_problem stat-unavailable
		case "$stat" in unavailable) ;; *[!0-9\ ]*) die "probe reported malformed counters for $each" 5 ;; esac
	fi
	emit "$each.label" "$label"
	emit "$each.device" "${link_device:-none}"
	emit "$each.dev" "${dev:-none}"
	emit "$each.parent" "$(get "$pre" "part.$each.parent")"
	emit "$each.type" "${blkid_type:-none}"
	emit "$each.bdev_ro" "$(get "$pre" "part.$each.bdev_ro")"
	emit "$each.iostats" "$(get "$pre" "part.$each.iostats")"
	emit "$each.holders" "$(get "$pre" "part.$each.holders")"
	emit "$each.mounts" "$(get "$pre" "part.$each.mounts")"
	emit "$each.stat" "$stat"
	emit "$each.factory_rewritten" "$([ "$each" = modem_img ] && echo yes || echo no)"
	emit "$each.preflight" "${role_problems:-ready}"
	[ -z "$role_problems" ] || problem "$each:$role_problems"
	if [ "$each" = "$role" ]; then
		bound_ready=${role_problems:-ready}
		bound_device=$link_device
		bound_dev=$dev
		bound_type=$blkid_type
		bound_label=$label
	fi
done
emit preflight_problems "${problems:-none}"
guest_service=$(get "$pre" service_sha256)
case "$guest_service" in
	missing|unavailable) service_installed=$guest_service ;;
	"$service_sha256") service_installed=matches-checkout ;;
	*)
		printf '%s\n' "$guest_service" | grep -Eqx '[0-9a-f]{64}' || die 'probe reported a malformed service hash' 5
		service_installed=differs-from-checkout
		;;
esac
emit service_mount_path "$service_path"
emit service_installed "$service_installed"
[ "$service_path" = read-only-loop ] || gate_blockers=service-mounts-without-read-only-loop
[ "$service_installed" = matches-checkout ] ||
	gate_blockers="$gate_blockers${gate_blockers:+,}installed-service-not-the-checked-in-service"
[ -z "$gate_blockers" ] || emit issue34_gate_blocker "$gate_blockers"

if [ "$mode" = preflight ]; then
	gate=$(gate_for "$problems")
	emit preflight_status "$([ -z "$problems" ] && echo ready || echo blocked)"
	emit issue34_safety_gate "$gate"
	emit result preflight-only
	{ printf '## raw preflight output\n'; cat "$pre"; } >>"$transcript"
	cp "$transcript" "$out/preflight-latest.txt"
	printf 'transcript=%s\n' "$transcript"
	printf 'Preflight never mounts. A mount is a separate gated run: s5300-mount-proof.sh mount ROLE.\n'
	exit 0
fi

# --- gated mount session --------------------------------------------------------

# Only problems that concern the global state or the bound role block it.
blocking=''
old_ifs=$IFS
IFS=,
for item in $problems; do
	case "$item" in
		*:*) [ "${item%%:*}" != "$role" ] || blocking="$blocking${blocking:+,}$item" ;;
		*) blocking="$blocking${blocking:+,}$item" ;;
	esac
done
IFS=$old_ifs
[ -z "$blocking" ] || die "preflight refused the mount ($blocking)" 6
[ "$bound_ready" = ready ] || die "preflight refused the mount ($bound_ready)" 6

case "$bound_type" in ext4) recovery=noload ;; f2fs) recovery=norecovery ;; *) die 'unreachable fstype' 6 ;; esac
printf 'About to mount, read-only and once, on %s (%s):\n' "$target_id" "$evidence"
printf '  kernel %s, slot %s\n' "$kernel_release" "$slot"
printf '  role %s, partition %s, device %s (%s), %s\n' "$role" "$bound_label" "$bound_device" "$bound_dev" "$bound_type"
printf '  BLKROSET on %s, then losetup --read-only onto an unbound loop with ro 0 (preflight found %s); only the loop is mounted\n' "$bound_device" "$(get "$pre" loop_free)"
printf '  options ro,%s,nosuid,nodev,noexec; destination /run/ferrix-s5300-mount-proof/%s\n' "$recovery" "$role"
emit bound_role "$role"
emit bound_device "$bound_device"
emit bound_dev "$bound_dev"
emit bound_type "$bound_type"

session mount mount "$role" "$bound_dev" "$bound_device" "$bound_type"
m="$work/mount.out"
case "$session_status" in 0|6|7) ;; *) die "guest mount probe failed (exit $session_status)" 3 ;; esac
validate "$m" mount "$role" || die 'mount output has a malformed, unknown, duplicate, or missing field' 5
[ "$(get "$m" probe)" = s5300-mount-proof ] && [ "$(get "$m" mode)" = mount ] && [ "$(get "$m" probe_complete)" = yes ] ||
	die 'mount probe did not complete' 5
{ printf '## raw mount-session output\n'; cat "$m"; } >>"$transcript"

if [ "$session_status" -eq 6 ]; then
	emit state_changed "$(get "$m" state_changed)"
	die "guest refused before changing any state ($(get "$m" refusal))" 6
fi

# field STAT N: the Nth counter of a stat line.
field() { printf '%s\n' "$1" | awk -v n="$2" '{ print $n }'; }

anomalies=''
anomaly() { anomalies="$anomalies${anomalies:+,}$1"; }
loop_name=$(get "$m" loop_name)
loop_dev=$(get "$m" loop_dev)
case "$loop_name" in loop[0-9]|loop[0-9][0-9]|loop[0-9][0-9][0-9]) ;; *) die 'mount probe reported a malformed loop device name' 5 ;; esac
emit state_changed "$(get "$m" state_changed)"
# The gate stands for the service the preflight hashed; a change between the
# two sessions is a stop.
[ "$(get "$m" service_sha256)" = "$guest_service" ] || anomaly installed-service-changed-between-sessions
emit setro_exit "$(get "$m" setro_exit)"
emit bdev_ro_before_mount "$(get "$m" bdev_ro.before)"
emit loop_name "$loop_name"
emit loop_ro_before_attach "$(get "$m" loop_ro.before)"
emit losetup_exit "$(get "$m" losetup_exit)"
emit loop_dev "${loop_dev:-none}"
emit loop_ro "$(get "$m" loop_ro)"
emit loop_backing_match "$(get "$m" loop_backing_match)"
emit loop_geometry "$(get "$m" loop_geometry)"
emit loop_type "$(get "$m" loop_type)"
emit loop_iostats "$(get "$m" loop_iostats)"
emit mount_exit "$(get "$m" mount_exit)"
emit umount_exit "$(get "$m" umount_exit)"
emit still_mounted "$(get "$m" still_mounted)"
emit detach_exit "$(get "$m" detach_exit)"
emit still_bound "$(get "$m" still_bound)"
emit bdev_ro_after "$(get "$m" bdev_ro.after)"
emit entries_read "$(get "$m" entries)"
[ "$(get "$m" setro_exit)" = 0 ] && [ "$(get "$m" bdev_ro.before)" = 1 ] || anomaly bdev-not-read-only
# The barrier: a loop whose ro read 0 unbound and 1 once attached read-only
# (so the flag came from the read-only loop configuration), on the loop major,
# mapping the whole bound partition with the same superblock type.
barrier=verified
[ "$(get "$m" loop_ro.before)" = 0 ] || barrier=unverified
[ "$(get "$m" losetup_exit)" = 0 ] || barrier=unverified
[ "$(get "$m" loop_ro)" = 1 ] || barrier=unverified
case "$loop_dev" in 7:[0-9]*) ;; *) barrier=unverified ;; esac
[ "$(get "$m" loop_backing_match)" = yes ] || barrier=unverified
[ "$(get "$m" loop_geometry)" = whole-partition ] || barrier=unverified
[ "$(get "$m" loop_type)" = "$bound_type" ] || barrier=unverified
emit write_barrier "$barrier"
[ "$barrier" = verified ] || anomaly read-only-loop-not-verified
if [ "$barrier" != verified ] && [ "$(get "$m" mount_exit)" != not-run ]; then
	anomaly mounted-without-verified-barrier
fi
[ "$(get "$m" mount_exit)" = 0 ] || anomaly mount-failed
[ "$(get "$m" still_mounted)" = no ] || anomaly still-mounted
case "$(get "$m" mount_exit):$(get "$m" umount_exit)" in 0:0|[!0]*:not-run|not-run:not-run) ;; *) anomaly umount-failed ;; esac
[ "$(get "$m" still_bound)" = no ] || anomaly loop-still-bound
case "$(get "$m" losetup_exit):$(get "$m" detach_exit)" in 0:0|[!0]*:*|not-run:not-run) ;; *) anomaly loop-detach-failed ;; esac
[ "$(get "$m" bdev_ro.after)" = 1 ] || anomaly bdev-ro-lost

# The mount must show what ferrix-s5300-mounts verify_mount() requires, on
# the loop device rather than the partition.
info=$(get "$m" mountinfo)
mount_contract=not-mounted
if [ -n "$info" ]; then
	mount_contract=$(printf '%s\n' "$info" | awk -v dev="$loop_dev" -v fstype="$bound_type" '
		function has(list, option) { return index("," list ",", "," option ",") > 0 }
		function add(r) { reasons = reasons (reasons == "" ? "" : ",") r }
		{
			for (sep = 7; sep <= NF && $sep != "-"; sep++) ;
			if (sep + 3 > NF) { print "malformed"; exit }
			if (dev == "" || $3 != dev) add("not-the-read-only-loop")
			if ($4 != "/") add("not-whole-filesystem")
			if ($(sep + 1) != fstype) add("type-mismatch")
			if (!has($6, "ro")) add("mount-not-ro")
			if (!has($(sep + 3), "ro")) add("superblock-not-ro")
			super = $(sep + 3)
			if (!((fstype == "ext4" && (has(super, "noload") || has(super, "norecovery"))) ||
			      (fstype == "f2fs" && has(super, "norecovery")))) add("recovery-flag-not-shown")
			print (reasons == "" ? "contract-met" : "contract-violated:" reasons)
		}')
fi
emit mountinfo_redacted "${info:-none}"
emit mount_contract "$mount_contract"
[ "$mount_contract" = contract-met ] || anomaly "mountinfo-$mount_contract"

# sysfs-block stat fields: 5 write ios, 7 write sectors, 9 in flight,
# 12 discard ios, 14 discard sectors, 16 flush ios. A change on the
# partition is a write that reached it. A change on the loop is a write the
# filesystem attempted; the read-only loop fails such requests, but an attempt
# still stops the run for review.
valid_stat() {
	case "$1" in *[!0-9\ ]*) die "malformed $2 counters" 5 ;; esac
	[ "$(printf '%s\n' "$1" | awk '{ print NF }')" -ge 17 ] || die "short $2 counters" 5
}
for phase in initial before mounted settled after; do
	value=$(get "$m" "stat.$phase")
	emit "stat_$phase" "${value:-none}"
done
for phase in initial before mounted settled after; do
	value=$(get "$m" "loop_stat.$phase")
	emit "loop_stat_$phase" "${value:-none}"
done
counters_status=compared
initial=$(get "$m" stat.initial)
after=$(get "$m" stat.after)
case "$initial:$after" in
	*unavailable*|*not-run*|:*|*:) counters_status=unavailable; anomaly counters-unavailable ;;
esac
if [ "$counters_status" = compared ]; then
	valid_stat "$initial" initial
	for phase in before mounted settled after; do
		value=$(get "$m" "stat.$phase")
		case "$value" in ''|unavailable|not-run) continue ;; esac
		valid_stat "$value" "$phase"
		for n in 5 7 12 14 16; do
			[ "$(field "$value" "$n")" = "$(field "$initial" "$n")" ] || anomaly "partition-counter-$n-changed-by-$phase"
		done
	done
	[ "$(field "$after" 9)" = 0 ] || anomaly io-in-flight-after
fi
emit write_counters "$counters_status"
loop_counters=compared
loop_before=$(get "$m" loop_stat.before)
case "$loop_before" in ''|unavailable|not-run) loop_counters=not-mounted ;; esac
[ "$(get "$m" loop_iostats)" = 1 ] || { [ "$loop_counters" = not-mounted ] || { loop_counters=iostats-off; anomaly loop-iostats-not-enabled; }; }
if [ "$loop_counters" = compared ]; then
	valid_stat "$loop_before" loop-before
	for phase in mounted settled after; do
		value=$(get "$m" "loop_stat.$phase")
		case "$value" in ''|unavailable|not-run) continue ;; esac
		valid_stat "$value" "loop-$phase"
		for n in 5 7 12 14 16; do
			[ "$(field "$value" "$n")" = "$(field "$loop_before" "$n")" ] || anomaly "loop-write-attempt-counter-$n-by-$phase"
		done
	done
fi
emit loop_write_attempt_counters "$loop_counters"

# Kernel log after the session's marker: a write to a read-only block device
# (the partition: a write reached it; the loop: an attempt the loop failed),
# an I/O or filesystem error, orphan cleanup, or recovery is a stop.
kmsg="$work/kmsg"
awk 'index($0, "kmsg.") == 1 { print substr($0, index($0, "=") + 1) }' "$m" >"$kmsg"
kmsg_count=$(awk 'END { print NR }' "$kmsg")
emit kmsg_lines "$kmsg_count"
! grep -i 'read-only block-device' "$kmsg" | grep -qv "block-device $loop_name\$" || anomaly kmsg-write-to-read-only-partition
! grep -qi "read-only block-device $loop_name\$" "$kmsg" || anomaly kmsg-write-attempt-on-read-only-loop
! grep -qiE 'I/O error|blk_update_request|loop: Write error' "$kmsg" || anomaly kmsg-io-error
! grep -qiE 'EXT4-fs error|F2FS-fs .*(error|inconsistent|need fsck|NEED_FSCK)' "$kmsg" || anomaly kmsg-fs-error
! grep -qiE 'orphan cleanup|orphan inode|recover(ed|ing)? (fsync|orphan)|Try to recover|recovery complete|journal recovery' "$kmsg" ||
	anomaly kmsg-recovery-or-orphan-message

emit anomalies "${anomalies:-none}"
if [ -z "$anomalies" ]; then
	observation=no-write-observed
else
	observation=anomaly
fi
emit mount_observation "$observation"
emit counters_caveat 'unchanged-counters-are-consistent-with-no-write-not-proof-of-absence'
emit barrier_scope 'read-only-loop-fails-writes-through-the-loop-only-not-other-writers-of-the-partition-or-disk'
emit issue34_safety_gate "$(gate_for "$anomalies")"
emit result mount-observation
cp "$transcript" "$out/mount-$role-latest.txt"
printf 'transcript=%s\n' "$transcript"
printf 'The partition block device stays read-only until the next boot. A mount observation never passes the #34 gate.\n'
[ "$session_status" -eq 0 ] || exit 7
[ -z "$anomalies" ] || exit 8
exit 0
