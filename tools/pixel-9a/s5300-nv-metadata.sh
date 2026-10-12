#!/bin/sh
set -eu

# Issue #79 metadata-only S5300 NV preflight on an ALREADY booted Google Pixel
# 9a (tegu / GXQ96 / MP1.0) Ferrix dev guest, reached over the USB CDC-ACM
# bring-up shell (tools/pixel-9a/acm-probe.py). Before a manual CP boot trial
# it answers one question: are `nv_normal.bin` and `nv_protected.bin` on the
# per-unit `efs` partition present, regular files, and at least the 0x80000
# bytes that the CP3A TOC sizes their sections at (s5300-cpif-e0.md row T1)?
#
#   s5300-nv-metadata.sh [preflight]   read-only; the default
#   s5300-nv-metadata.sh mount         one gated read-only mount of efs
#
# Both modes first refuse, before the port is opened, unless the checked-in
# guest body, the issue #34 probe whose barrier it copies, and the checked-in
# ferrix-s5300-mounts service are the reviewed files pinned below.
#
# preflight checks the exact device tree and pinned Kleaf kernel, that the
# installed ferrix-s5300-mounts is the accepted file, that it and exynos-modem
# are out of every runlevel, that nothing is mounted under /mnt/vendor or
# either proof root, that util-linux blkid/losetup, blockdev, `stat -c` and
# `timeout` exist with an unbound loop whose `ro` reads 0, and, for efs only:
# unique partition resolution, the util-linux `blkid -p` type (f2fs), the
# block-device read-only flag, queue iostats, holders, existing mounts, and
# the sysfs `stat` counters. It mounts nothing and changes no state.
#
# mount needs the typed confirmation
#   FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM='stat efs NV metadata through one read-only loop mount on google-pixel-9a-tegu-gxq96-mp1-0'
# and the recorded owner decision for per-unit read-only mounts
#   FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=owner-approved-issue34-per-unit-ro-mount
# for every run. It runs a fresh preflight, prints the bound partition, then
# sends a second session that refuses before any state change unless efs is
# still the same unique, unmounted, unheld f2fs partition and the installed
# service is still the accepted file. That session applies the issue #34
# contract unchanged: BLKROSET on the partition, `losetup --read-only` onto a
# verified unbound loop that must map exactly the whole partition, one mount
# of the LOOP with `ro,norecovery,nosuid,nodev,noexec` under /run, then
# unmount and detach. While mounted it only looks up the two allowlisted names
# and reports their type and lstat size. The host then checks the barrier,
# counters, mountinfo, kernel log, teardown and a post-check exactly like the
# #34 proof, and classifies the NV files last so that a missing or short file
# never hides a teardown problem.
#
# The barrier covers I/O through the loop only; other writers of the
# partition, the whole disk, or UFS passthrough are excluded by the preflight
# checks, not by the kernel. Unchanged counters are consistent with no write,
# not proof of absence. A size of at least 0x80000 does not show that the CP
# accepts the file. `persist` `modem/cpsha` and the device-tree identity values
# are deliberately not inspected here (board README).
#
# Exit status: 0 completed (mount: both NV files meet the minimum and no
# anomaly), 2 usage or missing gate, 3 transport or guest failure, 4 not the
# exact tegu board, 5 malformed probe output, 6 refused before any state
# change, 7 mount session did not finish cleanly after the read-only flag was
# set, 8 mount observation has anomalies, 9 clean observation but an NV file
# is absent, not a regular file, unreadable by stat, or short.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained. Set
# FERRIX_PIXEL9A_S5300_NV_METADATA_EVIDENCE=mock for stubbed runs. Transcripts
# go, mode 0600, to ignored out/pixel-9a/s5300-nv-metadata/ and contain no
# serials, UUIDs, labels, port names, file names other than the two
# allowlisted ones, or partition bytes. Never commit them.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
probe="$root/tools/pixel-9a/s5300-nv-metadata-remote.sh"
issue34_probe="$root/tools/pixel-9a/s5300-mount-proof-remote.sh"
transport="$root/tools/pixel-9a/acm-probe.py"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/s5300-nv-metadata"
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
pinned_kernel=6.1.124-android14-11-ga4662a84a15f
service_initd="$root/targets/boards/$target_id/pmos/device-google-tegu/ferrix-s5300-mounts.initd"
service_apkbuild="$root/targets/boards/$target_id/pmos/device-google-tegu/APKBUILD"
per_unit_decision=owner-approved-issue34-per-unit-ro-mount
# Reviewed inputs. The service is the file the owner accepted for the issue
# #34 mount gate on 2026-10-12; the #34 probe is the one whose barrier blocks
# the guest body copies; the guest body is the reviewed issue #79 probe.
# Changing any of them needs a new review and a new pin.
accepted_service_sha256=b90d3b41ac175c93e5da5db0858327157da414bc9ca06d403c598108db22f74b
issue34_probe_sha256=94f4af135f34c4d0cb446181946313b8a085912f480c840fc790528ab7530003
reviewed_probe_sha256=2eaadbaeb5d6e277f12d236b9a6bdf63a400703488db0574bd8166ff00c0f73a
toc_min=524288
max_output=65536
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-90}
nv_names='nv_normal.bin nv_protected.bin'

usage() {
	printf 'usage: tools/pixel-9a/s5300-nv-metadata.sh [preflight | mount]\n' >&2
	printf 'environment: FERRIX_SHELL (CDC-ACM node) FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, default 90)\n' >&2
	printf '             FERRIX_PIXEL9A_S5300_NV_METADATA_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	printf '             mount: FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM, FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION\n' >&2
	exit 2
}

mode=preflight
case "$#:${1:-}" in
	0:|1:preflight) ;;
	1:mount) mode=mount ;;
	*) usage ;;
esac
case "${FERRIX_PIXEL9A_S5300_NV_METADATA_EVIDENCE:-physical}" in
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
	printf 'REFUSING pixel-9a S5300 NV metadata: %s\n' "$1" >&2
	printf '{"event":"pixel-9a-s5300-nv-metadata","mode":"%s","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
		"$mode" "$evidence" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

# The mount gate is checked before any file is written or the port is opened.
if [ "$mode" = mount ]; then
	expected_confirm="stat efs NV metadata through one read-only loop mount on $target_id"
	[ "${FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM:-}" = "$expected_confirm" ] ||
		die "set FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM='$expected_confirm' to mount efs once" 2
	[ "${FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION:-}" = "$per_unit_decision" ] ||
		die 'efs is a per-unit partition outside factory recovery; mounting it needs the recorded owner decision' 2
fi

for tool in python3 awk sed; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly"
done
for file in "$probe" "$issue34_probe" "$transport" "$service_initd" "$service_apkbuild"; do
	[ -f "$file" ] && [ ! -L "$file" ] || die "reviewed input ${file#"$root"/} is missing or linked" 6
done

sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }

# Source and service hash gate: every reviewed input is the pinned file.
[ "$(sha256 "$probe")" = "$reviewed_probe_sha256" ] || die 'the guest NV body is not the reviewed file' 6
[ "$(sha256 "$issue34_probe")" = "$issue34_probe_sha256" ] ||
	die 'the issue #34 mount proof probe changed; the copied barrier needs a new review' 6
[ "$(sha256 "$service_initd")" = "$accepted_service_sha256" ] ||
	die 'the checked-in ferrix-s5300-mounts is not the service accepted for the #34 gate' 6
sha512=$( (shasum -a 512 "$service_initd" 2>/dev/null || sha512sum "$service_initd") | cut -c1-128)
[ "$(awk '$2 == "ferrix-s5300-mounts.initd" { print $1 }' "$service_apkbuild")" = "$sha512" ] ||
	die 'the device package does not ship the checked-in ferrix-s5300-mounts' 6

umask 077
mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/$mode-$stamp-$$.txt"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-nv-metadata.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
{
	printf '# Pixel 9a S5300 NV metadata (%s), %s UTC\n' "$mode" "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# Presence, type, and size of two allowlisted names only. No serials, port names, UUIDs, labels, other file names, or contents. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nmode=%s\nsource=cdc-acm\n' "$target_id" "$evidence" "$mode"
	printf 'probe_body_sha256=%s\naccepted_service_sha256=%s\nissue34_probe_sha256=%s\n' \
		"$reviewed_probe_sha256" "$accepted_service_sha256" "$issue34_probe_sha256"
} >"$transcript"

emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

# session NAME MODE DEV DEVNAME FSTYPE SERVICE: compose the six-line header and
# the exact probe body, send it as the only content of one CDC-ACM session,
# and leave the output in $work/NAME.out. Sets session_status.
session() {
	composed="$work/$1.sh"
	role=''
	[ "$2" = preflight ] || role=efs
	{
		printf 'proof_mode=%s\nproof_role=%s\nproof_dev=%s\nproof_devname=%s\nproof_fstype=%s\nproof_service_sha256=%s\n' \
			"$2" "$role" "$3" "$4" "$5" "$6"
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

# validate FILE MODE: every line is a known, unique key with a value in the
# reviewed character set, and every required key is present. Raw output is
# retained only after this passes.
validate() {
	awk -v mode="$2" '
		BEGIN {
			split("probe mode dt_model dt_compatible kernel_release uid service_sha256 blkid_impl blockdev_impl losetup_impl stat_usable timeout_usable kernel_fs.f2fs loop_free runlevel.ferrix-s5300-mounts runlevel.exynos-modem mountinfo_readable vendor_mounts proof_mounts issue34_mounts kmsg_usable probe_complete", g, " ")
			for (i in g) required[g[i]] = 1
			split("label link_state link_device partname_count partname_devices dev parent bdev_ro iostats holders stat blkid_exit blkid_type mounts", f, " ")
			for (j in f) required["part.efs." f[j]] = 1
			if (mode == "mount") {
				split("mount_options state_changed", m, " ")
				for (i in m) required[m[i]] = 1
				split("refusal loop_name stat.initial loop_stat.initial setro_exit bdev_ro.before loop_ro.before losetup_exit loop_dev loop_ro loop_backing_match loop_geometry loop_iostats loop_type stat.before loop_stat.before mountinfo mount_exit nv_root stat.mounted stat.settled loop_stat.mounted loop_stat.settled loop_stat.after umount_exit still_mounted detach_exit still_bound stat.after bdev_ro.after service_sha256.after post.vendor_mounts post.proof_mounts post.issue34_mounts", o, " ")
				for (i in o) optional[o[i]] = 1
				split("nv_normal.bin nv_protected.bin", n, " ")
				for (i in n) { optional["nv." n[i] ".type"] = 1; optional["nv." n[i] ".size"] = 1 }
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
			if (key ~ /^nv\..*\.type$/ && value !~ /^(regular|absent|symlink|directory|other)$/) bad = 1
			if (key ~ /^nv\..*\.size$/ && value !~ /^([0-9]+|none|unavailable)$/) bad = 1
			seen[key] = 1
		}
		END { if (bad) exit 1; for (k in required) if (!(k in seen)) exit 1 }
	' "$1"
}

get() {
	awk -v key="$2=" 'index($0, key) == 1 { print substr($0, length(key) + 1); exit }' "$1"
}

# --- preflight session (always first; read-only) ------------------------------

session preflight preflight '' '' '' ''
[ "$session_status" -eq 0 ] || die "guest preflight probe failed (exit $session_status)" 3
pre="$work/preflight.out"
validate "$pre" preflight || die 'preflight output has a malformed, unknown, duplicate, or missing field' 5
[ "$(get "$pre" probe)" = s5300-nv-metadata ] && [ "$(get "$pre" mode)" = preflight ] && [ "$(get "$pre" probe_complete)" = yes ] ||
	die 'preflight probe did not complete' 5
[ "$(get "$pre" dt_model)" = "$dt_model" ] && [ "$(get "$pre" dt_compatible)" = "$dt_compatible" ] ||
	die 'probe reported a device tree other than the exact tegu board' 4

problems=''
problem() { problems="$problems${problems:+,}$1"; }

kernel_release=$(get "$pre" kernel_release)
emit kernel_release "$kernel_release"
emit kernel_pinned "$([ "$kernel_release" = "$pinned_kernel" ] && echo yes || echo no)"
emit guest_uid "$(get "$pre" uid)"
for key in blkid_impl blockdev_impl losetup_impl stat_usable timeout_usable loop_free kernel_fs.f2fs \
	runlevel.ferrix-s5300-mounts runlevel.exynos-modem mountinfo_readable vendor_mounts proof_mounts issue34_mounts kmsg_usable; do
	emit "$key" "$(get "$pre" "$key")"
done
[ "$kernel_release" = "$pinned_kernel" ] || problem kernel-not-pinned
[ "$(get "$pre" uid)" = 0 ] || problem not-root
[ "$(get "$pre" blkid_impl)" = util-linux ] || problem blkid-not-util-linux
[ "$(get "$pre" blockdev_impl)" != missing ] || problem blockdev-missing
[ "$(get "$pre" losetup_impl)" = util-linux ] || problem losetup-not-util-linux
[ "$(get "$pre" stat_usable)" = yes ] || problem stat-unusable
[ "$(get "$pre" timeout_usable)" = yes ] || problem timeout-missing
case "$(get "$pre" loop_free)" in
	none) problem no-unbound-loop-with-ro-0 ;;
	loop[0-9]|loop[0-9][0-9]|loop[0-9][0-9][0-9]) ;;
	*) die 'probe reported a malformed loop device name' 5 ;;
esac
[ "$(get "$pre" kernel_fs.f2fs)" = yes ] || problem f2fs-not-in-kernel
[ "$(get "$pre" runlevel.ferrix-s5300-mounts)" = absent ] || problem ferrix-s5300-mounts-in-runlevel
[ "$(get "$pre" runlevel.exynos-modem)" = absent ] || problem exynos-modem-in-runlevel
[ "$(get "$pre" mountinfo_readable)" = yes ] || problem mountinfo-unreadable
[ "$(get "$pre" vendor_mounts)" = 0 ] || problem vendor-mount-present
[ "$(get "$pre" proof_mounts)" = 0 ] || problem proof-mount-present
[ "$(get "$pre" issue34_mounts)" = 0 ] || problem issue34-mount-present
[ "$(get "$pre" kmsg_usable)" = yes ] || problem kmsg-unusable

guest_service=$(get "$pre" service_sha256)
case "$guest_service" in
	missing|unavailable) service_installed=$guest_service ;;
	"$accepted_service_sha256") service_installed=accepted ;;
	*)
		printf '%s\n' "$guest_service" | grep -Eqx '[0-9a-f]{64}' || die 'probe reported a malformed service hash' 5
		service_installed=differs-from-accepted
		;;
esac
emit service_installed "$service_installed"
[ "$service_installed" = accepted ] || problem "installed-service-$service_installed"

[ "$(get "$pre" part.efs.label)" = efs ] || die 'probe reported a label other than efs' 5
link_state=$(get "$pre" part.efs.link_state)
link_device=$(get "$pre" part.efs.link_device)
count=$(get "$pre" part.efs.partname_count)
devices=$(get "$pre" part.efs.partname_devices)
dev=$(get "$pre" part.efs.dev)
blkid_exit=$(get "$pre" part.efs.blkid_exit)
blkid_type=$(get "$pre" part.efs.blkid_type)
stat=$(get "$pre" part.efs.stat)
case "$count" in ''|*[!0-9]*) die 'probe reported a malformed PARTNAME count for efs' 5 ;; esac
case "$link_device" in *[!a-z0-9]*) die 'probe reported a malformed device name for efs' 5 ;; esac
efs_problems=''
efs_problem() { efs_problems="$efs_problems${efs_problems:++}$1"; }
if [ "$link_state" != block ]; then efs_problem "partlabel-$link_state"
elif [ "$count" -ne 1 ]; then efs_problem "partname-count-$count"
elif [ "$devices" != "$link_device" ]; then efs_problem device-mismatch
fi
if [ -z "$efs_problems" ]; then
	case "$dev" in [0-9]*:[0-9]*) ;; *) die 'probe reported a malformed device number for efs' 5 ;; esac
	[ "$blkid_exit" = 0 ] && [ "$blkid_type" = f2fs ] || efs_problem "type-${blkid_type:-exit-$blkid_exit}-expected-f2fs"
	[ "$(get "$pre" part.efs.mounts)" = 0 ] || efs_problem already-mounted
	[ "$(get "$pre" part.efs.holders)" = 0 ] || efs_problem has-holders
	[ "$(get "$pre" part.efs.iostats)" = 1 ] || efs_problem iostats-not-enabled
	[ "$stat" != unavailable ] || efs_problem stat-unavailable
	case "$stat" in unavailable) ;; *[!0-9\ ]*) die 'probe reported malformed counters for efs' 5 ;; esac
fi
emit efs.device "${link_device:-none}"
emit efs.dev "${dev:-none}"
emit efs.partname_count "$count"
emit efs.type "${blkid_type:-none}"
for key in parent bdev_ro iostats holders mounts; do
	emit "efs.$key" "$(get "$pre" "part.efs.$key")"
done
emit efs.stat "$stat"
emit efs.factory_rewritten no
emit efs.preflight "${efs_problems:-ready}"
[ -z "$efs_problems" ] || problem "efs:$efs_problems"
emit preflight_problems "${problems:-none}"

if [ "$mode" = preflight ]; then
	emit preflight_status "$([ -z "$problems" ] && echo ready || echo blocked)"
	emit result preflight-only
	{ printf '## raw preflight output\n'; cat "$pre"; } >>"$transcript"
	cp "$transcript" "$out/preflight-latest.txt"
	printf 'transcript=%s\n' "$transcript"
	printf 'Preflight never mounts. The NV lookup is a separate gated run: s5300-nv-metadata.sh mount.\n'
	exit 0
fi

# --- gated mount session --------------------------------------------------------

[ -z "$problems" ] || die "preflight refused the mount ($problems)" 6

printf 'About to mount efs, read-only and once, on %s (%s), to stat two allowlisted NV names:\n' "$target_id" "$evidence"
printf '  kernel %s; installed ferrix-s5300-mounts is the accepted file\n' "$kernel_release"
printf '  partition efs, device %s (%s), f2fs\n' "$link_device" "$dev"
printf '  BLKROSET on %s, then losetup --read-only onto an unbound loop with ro 0 (preflight found %s); only the loop is mounted\n' "$link_device" "$(get "$pre" loop_free)"
printf '  options ro,norecovery,nosuid,nodev,noexec; destination /run/ferrix-s5300-nv-metadata/efs\n'
printf '  while mounted: type and lstat size of %s only; nothing is opened or listed\n' "$nv_names"
emit bound_device "$link_device"
emit bound_dev "$dev"
emit bound_type f2fs

session mount mount "$dev" "$link_device" f2fs "$accepted_service_sha256"
m="$work/mount.out"
case "$session_status" in 0|6|7) ;; *) die "guest mount probe failed (exit $session_status)" 3 ;; esac
validate "$m" mount || die 'mount output has a malformed, unknown, duplicate, or missing field' 5
[ "$(get "$m" probe)" = s5300-nv-metadata ] && [ "$(get "$m" mode)" = mount ] && [ "$(get "$m" probe_complete)" = yes ] ||
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
[ "$(get "$m" service_sha256)" = "$accepted_service_sha256" ] || anomaly installed-service-changed-between-sessions
for key in setro_exit loop_name losetup_exit loop_ro loop_backing_match loop_geometry loop_type loop_iostats \
	mount_exit nv_root umount_exit still_mounted detach_exit still_bound; do
	emit "$key" "$(get "$m" "$key")"
done
emit bdev_ro_before_mount "$(get "$m" bdev_ro.before)"
emit loop_ro_before_attach "$(get "$m" loop_ro.before)"
emit loop_dev "${loop_dev:-none}"
emit bdev_ro_after "$(get "$m" bdev_ro.after)"
[ "$(get "$m" setro_exit)" = 0 ] && [ "$(get "$m" bdev_ro.before)" = 1 ] || anomaly bdev-not-read-only
# The barrier, exactly as in the #34 proof: a loop whose ro read 0 unbound
# and 1 once attached read-only, on the loop major, mapping the whole bound
# partition with the same superblock type.
barrier=verified
[ "$(get "$m" loop_ro.before)" = 0 ] || barrier=unverified
[ "$(get "$m" losetup_exit)" = 0 ] || barrier=unverified
[ "$(get "$m" loop_ro)" = 1 ] || barrier=unverified
case "$loop_dev" in 7:[0-9]*) ;; *) barrier=unverified ;; esac
[ "$(get "$m" loop_backing_match)" = yes ] || barrier=unverified
[ "$(get "$m" loop_geometry)" = whole-partition ] || barrier=unverified
[ "$(get "$m" loop_type)" = f2fs ] || barrier=unverified
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

info=$(get "$m" mountinfo)
mount_contract=not-mounted
if [ -n "$info" ]; then
	mount_contract=$(printf '%s\n' "$info" | awk -v dev="$loop_dev" '
		function has(list, option) { return index("," list ",", "," option ",") > 0 }
		function add(r) { reasons = reasons (reasons == "" ? "" : ",") r }
		{
			for (sep = 7; sep <= NF && $sep != "-"; sep++) ;
			if (sep + 3 > NF) { print "malformed"; exit }
			if (dev == "" || $3 != dev) add("not-the-read-only-loop")
			if ($4 != "/") add("not-whole-filesystem")
			if ($(sep + 1) != "f2fs") add("type-mismatch")
			if (!has($6, "ro")) add("mount-not-ro")
			if (!has($(sep + 3), "ro")) add("superblock-not-ro")
			if (!has($(sep + 3), "norecovery")) add("recovery-flag-not-shown")
			print (reasons == "" ? "contract-met" : "contract-violated:" reasons)
		}')
fi
emit mountinfo_redacted "${info:-none}"
emit mount_contract "$mount_contract"
[ "$mount_contract" = contract-met ] || anomaly "mountinfo-$mount_contract"

valid_stat() {
	case "$1" in *[!0-9\ ]*) die "malformed $2 counters" 5 ;; esac
	[ "$(printf '%s\n' "$1" | awk '{ print NF }')" -ge 17 ] || die "short $2 counters" 5
}
for phase in initial before mounted settled after; do
	emit "stat_$phase" "$(get "$m" "stat.$phase")"
	emit "loop_stat_$phase" "$(get "$m" "loop_stat.$phase")"
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

kmsg="$work/kmsg"
awk 'index($0, "kmsg.") == 1 { print substr($0, index($0, "=") + 1) }' "$m" >"$kmsg"
emit kmsg_lines "$(awk 'END { print NR }' "$kmsg")"
! grep -i 'read-only block-device' "$kmsg" | grep -qv "block-device $loop_name\$" || anomaly kmsg-write-to-read-only-partition
! grep -qi "read-only block-device $loop_name\$" "$kmsg" || anomaly kmsg-write-attempt-on-read-only-loop
! grep -qiE 'I/O error|blk_update_request|loop: Write error' "$kmsg" || anomaly kmsg-io-error
! grep -qiE 'EXT4-fs error|F2FS-fs .*(error|inconsistent|need fsck|NEED_FSCK)' "$kmsg" || anomaly kmsg-fs-error
! grep -qiE 'orphan cleanup|orphan inode|recover(ed|ing)? (fsync|orphan)|Try to recover|recovery complete|journal recovery' "$kmsg" ||
	anomaly kmsg-recovery-or-orphan-message

# Post-check after teardown: the service is unchanged and nothing is left
# mounted under the service destinations or either proof root.
post=clean
[ "$(get "$m" service_sha256.after)" = "$accepted_service_sha256" ] || { post=dirty; anomaly installed-service-changed-during-session; }
for key in post.vendor_mounts post.proof_mounts post.issue34_mounts; do
	[ "$(get "$m" "$key")" = 0 ] || { post=dirty; anomaly "${key#post.}-left-after-teardown"; }
done
emit post_check "$post"

emit anomalies "${anomalies:-none}"
emit mount_observation "$([ -z "$anomalies" ] && echo no-write-observed || echo anomaly)"
emit counters_caveat 'unchanged-counters-are-consistent-with-no-write-not-proof-of-absence'
emit barrier_scope 'read-only-loop-fails-writes-through-the-loop-only-not-other-writers-of-the-partition-or-disk'

# NV classification comes last and never replaces a teardown or barrier
# result: both are always reported, and the exit status prefers them.
nv_problems=''
if [ "$(get "$m" nv_root)" != mounted-loop ]; then
	nv_problems=not-observed
	for name in $nv_names; do emit "$name" not-observed; done
else
	for name in $nv_names; do
		type=$(get "$m" "nv.$name.type")
		size=$(get "$m" "nv.$name.size")
		[ -n "$type" ] && [ -n "$size" ] || die "mount probe did not report $name" 5
		case "$type:$size" in
			regular:[0-9]*)
				if [ "$size" -ge "$toc_min" ]; then class=meets-toc-minimum
				else class=short
				fi
				emit "$name.size" "$size"
				;;
			regular:*) class=size-unavailable ;;
			*) class=$type ;;
		esac
		emit "$name" "$class"
		[ "$class" = meets-toc-minimum ] || nv_problems="$nv_problems${nv_problems:+,}$name:$class"
	done
fi
emit nv_toc_minimum_bytes "$toc_min"
emit nv_metadata "$([ -z "$nv_problems" ] && echo meets-toc-minimum || echo "$nv_problems")"
emit nv_caveat 'size-at-least-the-toc-section-does-not-show-cp-firmware-acceptance'
emit result nv-metadata-observation
cp "$transcript" "$out/mount-latest.txt"
printf 'transcript=%s\n' "$transcript"
printf 'The efs block device stays read-only until the next boot. No CP boot is attempted or authorised by this run.\n'
[ "$session_status" -eq 0 ] || exit 7
[ -z "$anomalies" ] || exit 8
[ -z "$nv_problems" ] || exit 9
exit 0
