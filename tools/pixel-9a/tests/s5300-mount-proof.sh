#!/bin/sh
# Offline proof for the issue #34 S5300 source mount proof workflow. The real
# CDC-ACM transport talks to a fake guest (acm-fake-guest.sh): a real
# interactive shell on a pty whose final `sh` is a stub that checks it got the
# exact probe body after the five generated header lines, then answers with a
# synthetic transcript; no device is contacted and nothing is mounted.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
proof="$root/tools/pixel-9a/s5300-mount-proof.sh"
probe="$root/tools/pixel-9a/s5300-mount-proof-remote.sh"
initd="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-s5300-mounts.initd"
fails=0
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for the CDC-ACM transport test' >&2; exit 1; }

check() {
	name=$1
	shift
	if "$@"; then
		printf 'ok   %s\n' "$name"
	else
		printf 'FAIL %s\n' "$name"
		fails=$((fails + 1))
	fi
}

check "static: host POSIX syntax" sh -n "$proof"
check "static: probe POSIX syntax" sh -n "$probe"
# A command is the first word of a line or follows ; | & ( then do else.
verbs='adb|fastboot|mount|umount|blockdev|dd|mkfs|fsck|e2fsck|tune2fs|debugfs|flash|erase|reboot|losetup|ssh|scp|curl'
offending=$(grep -vE '^[[:space:]]*(#|printf )' "$proof" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*($verbs)([[:space:]]|$)" || true)
check "static: the host runs no storage, mount, Android, or network command" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
preflight_part=$(sed -n '1,/^# --- mount mode/p' "$probe" | grep -v '^[[:space:]]*#')
check "static: nothing before the mount-mode boundary mounts or changes a block device" \
	sh -c '! printf "%s\n" "$1" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*(mount|umount|blockdev[[:space:]]+--set|dd|mkfs|fsck)([[:space:]]|$)"' sh "$preflight_part"
check "static: the preflight only asks losetup for its version" \
	sh -c '! printf "%s\n" "$1" | grep -E "losetup[[:space:]]" | grep -vE "losetup -V |command -v losetup "' sh "$preflight_part"
check "static: preflight returns before the mount-mode boundary" \
	sh -c 'printf "%s\n" "$1" | grep -A6 "proof_mode\" = preflight" | grep -q "exit 0"' sh "$preflight_part"
check "static: probe reads only TYPE with uncached low-level blkid" \
	sh -c '[ "$(grep -c "blkid -c /dev/null -p -o value -s TYPE -- \"\$part_link\"" "$1")" -eq 1 ] && [ "$(grep -cE "[^-]blkid .*-s (UUID|LABEL|PARTUUID)" "$1")" -eq 0 ]' sh "$probe"
check "static: probe gates on the exact tegu device tree" \
	sh -c 'grep -Fq "ZUMA PRO TEGU MP on ZUMA PRO" "$1" && grep -Fq "google,ZUMA PRO TEGU" "$1"' sh "$probe"
check "static: exactly one mount call, after BLKROSET, the read-only loop, and their verification" \
	sh -c '[ "$(grep -cE "^[[:space:]]*mount -t" "$1")" -eq 1 ] &&
		awk "/blockdev --setro/ { s = NR } /ro_now\" = 1/ { v = NR } /^[[:space:]]*losetup --read-only/ { l = NR } /loop_ro\" = 1/ { r = NR }
			/^[[:space:]]*mount -t/ { m = NR } END { exit !(s && v && l && r && m && s < v && v < l && l < r && r < m) }" "$1"' sh "$probe"
check "static: the filesystem is mounted only from the loop device" \
	grep -Fq 'mount -t "$proof_fstype" -o "ro,$recovery,nosuid,nodev,noexec" -- "$loop" "$destination"' "$probe"
check "static: every loop attach is read-only" \
	sh -c '[ "$(grep -vE "^[[:space:]]*#" "$1" | grep -oE "losetup [^ ]+" | grep -cvE "^losetup (-V|--read-only|-d|>/dev/null)$")" -eq 0 ] &&
		[ "$(grep -cE "^[[:space:]]*losetup --read-only -- \"\\\$loop\" \"\\\$device\"" "$1")" -eq 1 ]' sh "$probe"
check "static: the barrier requires an unbound loop whose ro reads 0" \
	sh -c 'grep -Fq "[ \"\$(cat \"\$path/ro\" 2>/dev/null)\" = 0 ] || continue" "$1" && grep -Fq "[ \"\$loop_ro_before\" = 0 ]" "$1"' sh "$probe"
check "static: the guest never clears the read-only block-device flag" sh -c '! grep -nE "setrw|BLKROSET.*0" "$1"' sh "$probe"
check "static: probe uses the service options per filesystem" \
	sh -c 'grep -Fq "ext4) recovery=noload" "$1" && grep -Fq "f2fs) recovery=norecovery" "$1" && grep -Fq "\"ro,\$recovery,nosuid,nodev,noexec\"" "$1" &&
		grep -Fq "ext4) printf '"'"'noload" "$2" && grep -Fq "f2fs) printf '"'"'norecovery" "$2" && grep -Fq "\"ro,\$(recovery_option \"\$fstype\"),nosuid,nodev,noexec\"" "$2"' sh "$probe" "$initd"
check "static: the proof mounts only under /run, never at the service destination" \
	sh -c 'grep -Fxq "proof_root=/run/ferrix-s5300-mount-proof" "$1" && grep -Fq "destination=\$proof_root/\$proof_role" "$1"' sh "$probe"
check "static: guest and host pin the same Kleaf kernel" \
	sh -c '[ "$(sed -n "s/^pinned_kernel=//p" "$1")" = "$(sed -n "s/^pinned_kernel=//p" "$2")" ]' sh "$probe" "$proof"
check "static: the guest reads the installed service hash before the mount-mode boundary" \
	sh -c 'printf "%s\n" "$1" | grep -Fq "sha256sum /etc/init.d/ferrix-s5300-mounts"' sh "$preflight_part"
check "static: the host never writes a passed gate" sh -c '! grep -nE "gate=passed|safety_gate passed|=passed" "$1"' sh "$proof"
probe_status=0
{ printf 'proof_mode=preflight\nproof_role=\nproof_dev=\nproof_devname=\nproof_fstype=\n'; cat "$probe"; } | sh >/dev/null 2>&1 || probe_status=$?
check "probe: refuses a host that is not the tegu board" [ "$probe_status" -eq 4 ]

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-mount-proof-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; rm -rf "$work"' EXIT
mkdir -p "$work/fx"
acm_guest_init "$root" "$work" "$probe"
unset FERRIX_PIXEL9A_ACM_TIMEOUT FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION 2>/dev/null || true

zero='1 0 8 0 0 0 0 0 0 4 4 0 0 0 0 0 0'
sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }
initd_sha256=$(sha256 "$initd")
loopzero='2 0 16 0 0 0 0 0 0 1 1 0 0 0 0 0 0'
part() {
	printf 'part.%s.label=%s\npart.%s.link_state=block\npart.%s.link_device=%s\n' "$1" "$2" "$1" "$1" "$3"
	printf 'part.%s.partname_count=1\npart.%s.partname_devices=%s\npart.%s.dev=%s\npart.%s.parent=sda\n' "$1" "$1" "$3" "$1" "$4" "$1"
	printf 'part.%s.bdev_ro=0\npart.%s.iostats=1\npart.%s.holders=0\npart.%s.stat=%s\n' "$1" "$1" "$1" "$1" "$zero"
	printf 'part.%s.blkid_exit=0\npart.%s.blkid_type=%s\npart.%s.mounts=0\n' "$1" "$1" "$5" "$1"
}
common() {
	printf 'probe=s5300-mount-proof\nmode=%s\ndt_model=ZUMA PRO TEGU MP on ZUMA PRO\ndt_compatible=google,ZUMA PRO TEGU\n' "$1"
	printf 'kernel_release=6.1.124-android14-11-ga4662a84a15f\nuid=0\nservice_sha256=%s\n' "$initd_sha256"
	printf 'blkid_impl=util-linux\nblockdev_impl=other\n'
	printf 'losetup_impl=util-linux\nloop_free=loop3\n'
	printf 'kernel_fs.ext4=yes\nkernel_fs.f2fs=yes\n'
	printf 'runlevel.ferrix-s5300-mounts=absent\nrunlevel.exynos-modem=absent\n'
	printf 'mountinfo_readable=yes\nvendor_mounts=0\nproof_mounts=0\nkmsg_usable=yes\n'
	printf 'slot_source=bootconfig\nslot_value_count=1\nslot_suffix=_b\n'
}
{
	common preflight
	part modem_img modem_b sda20 8:20 ext4
	part efs efs sda21 8:21 f2fs
	part persist persist sda22 8:22 f2fs
	part modem_userdata modem_userdata sda23 8:23 f2fs
	printf 'probe_complete=yes\n'
} >"$work/fx/preflight"

derive() {
	src=$1
	name=$2
	shift 2
	sed "$@" "$work/fx/$src" >"$work/fx/$name"
}
derive preflight pre-kernel -e 's/^kernel_release=.*/kernel_release=6.1.162-android14-11-gdeadbeef/'
derive preflight pre-runlevel -e 's/^runlevel.ferrix-s5300-mounts=absent$/runlevel.ferrix-s5300-mounts=present/'
derive preflight pre-efs-mounted -e 's/^part\.efs\.mounts=0$/part.efs.mounts=1/'
derive preflight pre-efs-ext4 -e 's/^part\.efs\.blkid_type=f2fs$/part.efs.blkid_type=ext4/'
derive preflight pre-iostats -e 's/^part\.persist\.iostats=1$/part.persist.iostats=0/' -e 's/^part\.persist\.holders=0$/part.persist.holders=1/'
derive preflight pre-qemu -e 's/^dt_model=.*/dt_model=QEMU Virtual Machine/'
derive preflight pre-unknown-key -e '/^probe_complete=/i\
serial=ABC123'
derive preflight pre-no-loop -e 's/^loop_free=loop3$/loop_free=none/'
derive preflight pre-busybox-losetup -e 's/^losetup_impl=util-linux$/losetup_impl=other/'
derive preflight pre-service-differs -e "s/^service_sha256=.*/service_sha256=$(printf '%064d' 0)/"
derive preflight pre-service-missing -e 's/^service_sha256=.*/service_sha256=missing/'
derive preflight pre-service-malformed -e 's/^service_sha256=.*/service_sha256=0123/'
derive preflight pre-wrong-label -e 's/^part\.modem_img\.label=modem_b$/part.modem_img.label=modem_a/'

# mount ROLE LABEL DEVNAME DEV FSTYPE: a clean mount-session transcript.
mount_fixture() {
	common mount
	part "$1" "$2" "$3" "$4" "$5"
	case "$5" in ext4) opt=noload ;; *) opt=norecovery ;; esac
	printf 'mount_options=ro,%s,nosuid,nodev,noexec\nstate_changed=bdev-ro\nloop_name=loop3\n' "$opt"
	printf 'stat.initial=%s\nloop_stat.initial=%s\nsetro_exit=0\nbdev_ro.before=1\n' "$zero" "$loopzero"
	printf 'loop_ro.before=0\nlosetup_exit=0\nloop_dev=7:3\nloop_ro=1\nloop_backing_match=yes\nloop_geometry=whole-partition\n'
	printf 'loop_iostats=1\nloop_type=%s\nstat.before=%s\nloop_stat.before=%s\n' "$5" "$zero" "$loopzero"
	if [ "$5" = ext4 ]; then
		printf 'mountinfo=50 30 7:3 / /run/ferrix-s5300-mount-proof/%s ro,nosuid,nodev,noexec,relatime - ext4 redacted-source ro,norecovery\n' "$1"
	else
		printf 'mountinfo=50 30 7:3 / /run/ferrix-s5300-mount-proof/%s ro,nosuid,nodev,noexec,relatime - f2fs redacted-source ro,lazytime,background_gc=on,norecovery,active_logs=6\n' "$1"
	fi
	printf 'mount_exit=0\nentries=17\n'
	printf 'stat.mounted=40 0 900 3 0 0 0 0 0 20 20 0 0 0 0 0 0\nstat.settled=44 0 960 3 0 0 0 0 0 24 24 0 0 0 0 0 0\n'
	printf 'loop_stat.mounted=30 0 800 2 0 0 0 0 0 10 10 0 0 0 0 0 0\nloop_stat.settled=33 0 850 2 0 0 0 0 0 12 12 0 0 0 0 0 0\n'
	printf 'loop_stat.after=33 0 850 2 0 0 0 0 0 12 12 0 0 0 0 0 0\n'
	printf 'umount_exit=0\nstill_mounted=no\ndetach_exit=0\nstill_bound=no\n'
	printf 'stat.after=44 0 960 3 0 0 0 0 0 24 24 0 0 0 0 0 0\nbdev_ro.after=1\n'
	printf 'kmsg.01=F2FS-fs (loop3): Mounted with checkpoint version = 1a2b\n'
	printf 'probe_complete=yes\n'
}
mount_fixture efs efs sda21 8:21 f2fs >"$work/fx/mount-efs"
mount_fixture modem_img modem_b sda20 8:20 ext4 | sed 's/^kmsg.01=.*/kmsg.01=EXT4-fs (loop3): mounted filesystem without journal. Quota mode: none./' >"$work/fx/mount-modem"
derive mount-efs mount-write -e 's/^stat.after=.*/stat.after=44 0 960 3 1 0 8 1 0 25 25 0 0 0 0 0 0/'
derive mount-efs mount-flush -e 's/^stat.settled=.*/stat.settled=44 0 960 3 0 0 0 0 0 24 24 0 0 0 0 1 0/'
derive mount-efs mount-rowarn -e '/^probe_complete=/i\
kmsg.02=Trying to write to read-only block-device sda21'
derive mount-efs mount-loopwarn -e '/^probe_complete=/i\
kmsg.02=Trying to write to read-only block-device loop3'
derive mount-efs mount-loopwrite -e 's/^loop_stat.settled=.*/loop_stat.settled=33 0 850 2 1 0 8 0 0 12 12 0 0 0 0 0 0/' \
	-e 's/^loop_stat.after=.*/loop_stat.after=33 0 850 2 1 0 8 0 0 12 12 0 0 0 0 0 0/'
derive mount-efs mount-orphan -e '/^probe_complete=/i\
kmsg.02=F2FS-fs (loop3): write access unavailable, skipping orphan cleanup'
derive mount-efs mount-loop-rw -e 's/^loop_ro=1$/loop_ro=0/'
derive mount-efs mount-loop-stale -e 's/^loop_ro.before=0$/loop_ro.before=1/'
derive mount-efs mount-loop-offset -e 's/^loop_geometry=whole-partition$/loop_geometry=mismatch/'
derive mount-efs mount-loop-backing -e 's/^loop_backing_match=yes$/loop_backing_match=no/'
derive mount-efs mount-loop-bound -e 's/^detach_exit=0$/detach_exit=1/' -e 's/^still_bound=no$/still_bound=yes/'
derive mount-efs mount-partition-source -e 's/^mountinfo=50 30 7:3 /mountinfo=50 30 8:21 /'
derive mount-efs mount-noflag -e 's/,norecovery,active_logs=6$/,active_logs=6/'
derive mount-efs mount-rwsuper -e 's/ - f2fs redacted-source ro,/ - f2fs redacted-source rw,/'
derive mount-efs mount-foreign -e 's/^mountinfo=50 30 7:3 /mountinfo=50 30 8:99 /'
{
	common mount
	part efs efs sda21 8:21 f2fs | sed 's/^part\.efs\.mounts=0$/part.efs.mounts=1/'
	printf 'mount_options=ro,norecovery,nosuid,nodev,noexec\nrefusal=partition-already-mounted\nstate_changed=no\nprobe_complete=yes\n'
} >"$work/fx/mount-refused"
{
	common mount
	part efs efs sda21 8:21 f2fs
	printf 'mount_options=ro,norecovery,nosuid,nodev,noexec\nstate_changed=bdev-ro\nloop_name=loop3\n'
	printf 'stat.initial=%s\nloop_stat.initial=%s\nsetro_exit=0\nbdev_ro.before=1\n' "$zero" "$loopzero"
	printf 'loop_ro.before=0\nlosetup_exit=0\nloop_dev=7:3\nloop_ro=1\nloop_backing_match=yes\nloop_geometry=whole-partition\n'
	printf 'loop_iostats=1\nloop_type=f2fs\nstat.before=%s\nloop_stat.before=%s\nmountinfo=\nmount_exit=32\nentries=not-run\n' "$zero" "$loopzero"
	printf 'stat.mounted=not-run\nstat.settled=not-run\nloop_stat.mounted=not-run\nloop_stat.settled=not-run\nloop_stat.after=%s\n' "$loopzero"
	printf 'umount_exit=not-run\nstill_mounted=no\ndetach_exit=0\nstill_bound=no\nstat.after=%s\nbdev_ro.after=1\n' "$zero"
	printf 'kmsg.01=F2FS-fs (loop3): Magic Mismatch\nprobe_complete=yes\n'
} >"$work/fx/mount-failed"
derive mount-efs mount-umount -e 's/^umount_exit=0$/umount_exit=32/' -e 's/^still_mounted=no$/still_mounted=yes/'
{
	common mount
	part efs efs sda21 8:21 f2fs
	printf 'mount_options=ro,norecovery,nosuid,nodev,noexec\nstate_changed=bdev-ro\nloop_name=loop3\n'
	printf 'stat.initial=%s\nloop_stat.initial=%s\nsetro_exit=1\nbdev_ro.before=0\n' "$zero" "$loopzero"
	printf 'loop_ro.before=not-run\nlosetup_exit=not-run\nloop_dev=\nloop_ro=not-run\nloop_backing_match=not-run\nloop_geometry=not-run\n'
	printf 'loop_iostats=not-run\nloop_type=\nstat.before=not-run\nloop_stat.before=not-run\nmountinfo=\nmount_exit=not-run\nentries=not-run\n'
	printf 'stat.mounted=not-run\nstat.settled=not-run\nloop_stat.mounted=not-run\nloop_stat.settled=not-run\nloop_stat.after=%s\n' "$loopzero"
	printf 'umount_exit=not-run\nstill_mounted=no\ndetach_exit=not-run\nstill_bound=no\nstat.after=%s\nbdev_ro.after=0\nprobe_complete=yes\n' "$zero"
} >"$work/fx/mount-setro"
derive mount-efs mount-efs-differs -e "s/^service_sha256=.*/service_sha256=$(printf '%064d' 0)/"
derive mount-efs mount-service-changed -e "s/^service_sha256=.*/service_sha256=$(printf '%064d' 1)/"
derive mount-efs mount-unknown-key -e '/^probe_complete=/i\
imei=123456789012345'

status=0
out=''
confirm_efs='mount efs read-only on google-pixel-9a-tegu-gxq96-mp1-0'
decision=owner-approved-issue34-per-unit-ro-mount
# run CASE PREFLIGHT_FIXTURE [MOUNT_FIXTURE [MOUNT_EXIT]] -- ARGS...
run() {
	case_name=$1
	pre_fixture=$2
	mount_fx=${3:-}
	mount_status=${4:-0}
	shift 4
	case_dir="$work/run-$case_name"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 5
	acm_guest_ctl fixture.1 "$work/fx/$pre_fixture"
	acm_guest_ctl exit.1 0
	if [ -n "$mount_fx" ]; then
		acm_guest_ctl fixture.2 "$work/fx/$mount_fx"
		acm_guest_ctl exit.2 "$mount_status"
	fi
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_S5300_MOUNT_PROOF_EVIDENCE="${EVIDENCE:-mock}" sh "$proof" "$@" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	ctl=$ACM_GUEST_CTL
	results="$case_dir/pixel-9a/s5300-mount-proof"
	EVIDENCE=mock
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }

run preflight preflight '' 0
check "preflight: completes" [ "$status" -eq 0 ]
check "preflight: exactly one guest session" [ "$runs" -eq 1 ]
check "preflight: the guest received the exact body after the generated header" cmp -s "$ctl/body.1" "$probe"
check "preflight: header selects preflight and binds nothing" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=preflight\nproof_role=\nproof_dev=\nproof_devname=\nproof_fstype=")" ]' sh "$ctl/header.1"
check "preflight: ready" has 'preflight_status=ready'
check "preflight: each role ready" sh -c '[ "$(printf "%s\n" "$1" | grep -cE "^[a-z_]+\.preflight=ready$")" -eq 4 ]' sh "$out"
check "preflight: modem image is the only factory-rewritten source" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq modem_img.factory_rewritten=yes && [ "$(printf "%s\n" "$1" | grep -c "factory_rewritten=no$")" -eq 3 ]' sh "$out"
check "preflight: mock never evaluates the gate" has 'issue34_safety_gate=not-evaluated-mock'
check "preflight: transcript keeps hashes and raw output" \
	sh -c 'grep -Eq "^probe_body_sha256=[0-9a-f]{64}$" "$1" && grep -Eq "^preflight_probe_sha256=[0-9a-f]{64}$" "$1" && grep -Fxq "part.efs.blkid_type=f2fs" "$1"' sh "$results/preflight-latest.txt"
run preflight-arg preflight '' 0 preflight
check "preflight: explicit argument is the same read-only run" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
EVIDENCE=physical run preflight-physical preflight '' 0
check "preflight: clean physical run with the checked-in protected service needs owner review, never passes" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq issue34_safety_gate=owner-review-required && printf "%s\n" "$1" | grep -Fxq service_mount_path=read-only-loop &&
		printf "%s\n" "$1" | grep -Fxq service_installed=matches-checkout && ! printf "%s\n" "$1" | grep -q "^issue34_gate_blocker="' sh "$out"
check "preflight: physical transcript names the exact target" \
	grep -Fxq 'evidence=physical:google-pixel-9a-tegu-gxq96-mp1-0' "$results/preflight-latest.txt"
EVIDENCE=physical run pre-kernel pre-kernel '' 0
check "preflight: another kernel blocks" sh -c 'printf "%s\n" "$1" | grep -Fxq kernel_pinned=no && printf "%s\n" "$1" | grep -Fxq issue34_safety_gate=blocked' sh "$out"
EVIDENCE=physical run pre-service-differs pre-service-differs '' 0
check "preflight: an installed service other than the checked-in one blocks the gate" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq service_installed=differs-from-checkout &&
		printf "%s\n" "$2" | grep -Fxq issue34_gate_blocker=installed-service-not-the-checked-in-service &&
		printf "%s\n" "$2" | grep -Fxq issue34_safety_gate=blocked' sh "$status" "$out"
EVIDENCE=physical run pre-service-missing pre-service-missing '' 0
check "preflight: a guest without the service blocks the gate" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq service_installed=missing && printf "%s\n" "$1" | grep -Fxq issue34_safety_gate=blocked' sh "$out"
run pre-service-malformed pre-service-malformed '' 0
check "malformed: a service hash that is not sha256 is refused" [ "$status" -eq 5 ]
run pre-runlevel pre-runlevel '' 0
check "preflight: service in a runlevel blocks" has 'preflight_problems=ferrix-s5300-mounts-in-runlevel'
run pre-efs-ext4 pre-efs-ext4 '' 0
check "preflight: type other than the physical inventory blocks the role" has 'efs.preflight=type-ext4-expected-f2fs'
run pre-iostats pre-iostats '' 0
check "preflight: role problems join with + inside one comma item" has 'persist.preflight=has-holders+iostats-not-enabled'
run pre-qemu pre-qemu '' 0
check "identity: QEMU device tree refused" [ "$status" -eq 4 ]
run pre-unknown-key pre-unknown-key '' 0
check "malformed: unknown preflight field refused, value not echoed" \
	sh -c '[ "$1" -eq 5 ] && ! printf "%s\n" "$2" | grep -Fq ABC123' sh "$status" "$out"
run pre-no-loop pre-no-loop '' 0
check "preflight: no unbound loop with ro 0 blocks" has 'preflight_problems=no-unbound-loop-with-ro-0'
run pre-busybox-losetup pre-busybox-losetup '' 0
check "preflight: a non-util-linux losetup blocks" has 'preflight_problems=losetup-not-util-linux'
run pre-wrong-label pre-wrong-label '' 0
check "malformed: wrong-slot modem label refused" [ "$status" -eq 5 ]

# --- mount gating: nothing reaches the guest without the typed phrases -------
mkdir -p "$work/run-gate"
for args in 'mount' 'mount efs extra' 'mount userdata' 'mount ../efs' 'unmount efs'; do
	status=0
	# shellcheck disable=SC2086
	FERRIX_OUT_DIR="$work/run-gate" sh "$proof" $args >/dev/null 2>&1 || status=$?
	check "usage: '$args' refused before any session" [ "$status" -eq 2 ]
done
status=0
out=$(FERRIX_OUT_DIR="$work/run-gate" sh "$proof" mount efs 2>&1) || status=$?
check "gate: missing confirmation refused with exit 2" [ "$status" -eq 2 ]
check "gate: refusal names the exact phrase" sh -c 'printf "%s\n" "$1" | grep -Fq "$2"' sh "$out" "$confirm_efs"
status=0
FERRIX_OUT_DIR="$work/run-gate" FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM="mount persist read-only on google-pixel-9a-tegu-gxq96-mp1-0" \
	FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=$decision sh "$proof" mount efs >/dev/null 2>&1 || status=$?
check "gate: a phrase for another role is refused" [ "$status" -eq 2 ]
status=0
out=$(FERRIX_OUT_DIR="$work/run-gate" FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM="$confirm_efs" sh "$proof" mount efs 2>&1) || status=$?
check "gate: per-unit partition needs the owner decision" \
	sh -c '[ "$1" -eq 2 ] && printf "%s\n" "$2" | grep -Fq "per-unit partition outside factory recovery"' sh "$status" "$out"
check "gate: refused runs write no transcript" [ ! -d "$work/run-gate/pixel-9a" ]

export FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM="$confirm_efs" FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=$decision
run mount-efs preflight mount-efs 0 mount efs
check "mount efs: completes" [ "$status" -eq 0 ]
check "mount efs: preflight session then mount session" [ "$runs" -eq 2 ]
check "mount efs: both sessions carry the exact body" sh -c 'cmp -s "$1/body.1" "$2" && cmp -s "$1/body.2" "$2"' sh "$ctl" "$probe"
check "mount efs: mount header binds the preflight partition" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=mount\nproof_role=efs\nproof_dev=8:21\nproof_devname=sda21\nproof_fstype=f2fs")" ]' sh "$ctl/header.2"
check "mount efs: the bound partition is printed before the mount" \
	sh -c 'printf "%s\n" "$1" | grep -Fq "role efs, partition efs, device sda21 (8:21), f2fs"' sh "$out"
check "mount efs: the guest is told to use only the loop" \
	sh -c 'printf "%s\n" "$1" | grep -Fq "BLKROSET on sda21, then losetup --read-only onto an unbound loop with ro 0 (preflight found loop3); only the loop is mounted"' sh "$out"
check "mount efs: read-only loop barrier verified" has 'write_barrier=verified'
check "mount efs: service contract met on the loop" has 'mount_contract=contract-met'
check "mount efs: loop counters compared" has 'loop_write_attempt_counters=compared'
check "mount efs: barrier scope recorded" has 'barrier_scope=read-only-loop-fails-writes-through-the-loop-only-not-other-writers-of-the-partition-or-disk'
check "mount efs: counters compared" has 'write_counters=compared'
check "mount efs: no write observed" has 'mount_observation=no-write-observed'
check "mount efs: counter caveat recorded" has 'counters_caveat=unchanged-counters-are-consistent-with-no-write-not-proof-of-absence'
check "mount efs: mock never evaluates the gate" has 'issue34_safety_gate=not-evaluated-mock'
check "mount efs: mountinfo source stays redacted" \
	sh -c 'grep -Fq "redacted-source" "$1" && ! grep -Fq "/dev/sda21" "$1"' sh "$results/mount-efs-latest.txt"
EVIDENCE=physical run mount-efs-physical preflight mount-efs 0 mount efs
check "mount efs physical: a clean run with the protected service needs owner review, never passes" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq mount_observation=no-write-observed && printf "%s\n" "$2" | grep -Fxq issue34_safety_gate=owner-review-required' sh "$status" "$out"
EVIDENCE=physical run mount-efs-differs pre-service-differs mount-efs-differs 0 mount efs
check "mount efs physical: a clean run on a guest with another service stays blocked" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq issue34_safety_gate=blocked' sh "$status" "$out"

unset FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION
FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM='mount modem_img read-only on google-pixel-9a-tegu-gxq96-mp1-0'
run mount-modem preflight mount-modem 0 mount modem_img
check "mount modem_img: factory-rewritten role needs no per-unit decision" [ "$status" -eq 0 ]
check "mount modem_img: ext4 binds noload" \
	sh -c 'printf "%s\n" "$1" | grep -Fq "options ro,noload,nosuid,nodev,noexec;"' sh "$out"
check "mount modem_img: ext4 norecovery display meets the contract" has 'mount_contract=contract-met'
export FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM="$confirm_efs" FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=$decision

run mount-write preflight mount-write 0 mount efs
check "partition write counter change: anomaly, exit 8" sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq "anomalies=partition-counter-5-changed-by-after,partition-counter-7-changed-by-after"' sh "$status" "$out"
EVIDENCE=physical run mount-write-physical preflight mount-write 0 mount efs
check "write counter change: physical gate blocked" has 'issue34_safety_gate=blocked'
run mount-flush preflight mount-flush 0 mount efs
check "partition flush counter change: anomaly" has 'anomalies=partition-counter-16-changed-by-settled'
run mount-rowarn preflight mount-rowarn 0 mount efs
check "read-only warning naming the partition: anomaly" has 'anomalies=kmsg-write-to-read-only-partition'
run mount-loopwarn preflight mount-loopwarn 0 mount efs
check "read-only warning naming the loop: rejected attempt still stops the run" \
	sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq anomalies=kmsg-write-attempt-on-read-only-loop' sh "$status" "$out"
run mount-loopwrite preflight mount-loopwrite 0 mount efs
check "loop write counter change: attempt anomaly, partition untouched" \
	has 'anomalies=loop-write-attempt-counter-5-by-settled,loop-write-attempt-counter-7-by-settled,loop-write-attempt-counter-5-by-after,loop-write-attempt-counter-7-by-after'
for case_name in loop-rw loop-stale loop-offset loop-backing; do
	run "mount-$case_name" preflight "mount-$case_name" 0 mount efs
	check "unverified loop ($case_name): barrier unverified and mount flagged" \
		sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq write_barrier=unverified &&
			printf "%s\n" "$2" | grep -Fxq anomalies=read-only-loop-not-verified,mounted-without-verified-barrier' sh "$status" "$out"
done
run mount-loop-bound preflight mount-loop-bound 7 mount efs
check "loop left bound: exit 7" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=loop-still-bound,loop-detach-failed' sh "$status" "$out"
run mount-partition-source preflight mount-partition-source 0 mount efs
check "partition mounted directly instead of the loop: contract violated" has 'mount_contract=contract-violated:not-the-read-only-loop'
run mount-orphan preflight mount-orphan 0 mount efs
check "orphan cleanup message: anomaly for review" has 'anomalies=kmsg-recovery-or-orphan-message'
run mount-noflag preflight mount-noflag 0 mount efs
check "mountinfo without norecovery: contract violated" has 'mount_contract=contract-violated:recovery-flag-not-shown'
run mount-rwsuper preflight mount-rwsuper 0 mount efs
check "writable superblock: contract violated" has 'mount_contract=contract-violated:superblock-not-ro'
run mount-foreign preflight mount-foreign 0 mount efs
check "foreign device at destination: contract violated" has 'mount_contract=contract-violated:not-the-read-only-loop'
run mount-failed preflight mount-failed 7 mount efs
check "mount failure: exit 7 with anomalies" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fq "anomalies=mount-failed,mountinfo-not-mounted"' sh "$status" "$out"
check "mount failure: the read-only flag is still reported" has 'bdev_ro_after=1'
run mount-umount preflight mount-umount 7 mount efs
check "umount failure: exit 7, still mounted reported" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fq "anomalies=still-mounted,umount-failed"' sh "$status" "$out"
run mount-setro preflight mount-setro 7 mount efs
check "BLKROSET failure: no loop, never mounted, anomaly" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq mount_exit=not-run && printf "%s\n" "$2" | grep -Fxq losetup_exit=not-run &&
		printf "%s\n" "$2" | grep -Fxq "anomalies=bdev-not-read-only,read-only-loop-not-verified,mount-failed,bdev-ro-lost,mountinfo-not-mounted"' sh "$status" "$out"
run mount-refused preflight mount-refused 6 mount efs
check "guest refusal: exit 6 before any state change" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq state_changed=no && printf "%s\n" "$2" | grep -Fq "(partition-already-mounted)"' sh "$status" "$out"
EVIDENCE=physical run mount-service-changed preflight mount-service-changed 0 mount efs
check "installed service changed between sessions: anomaly, exit 8, gate blocked" \
	sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq anomalies=installed-service-changed-between-sessions &&
		printf "%s\n" "$2" | grep -Fxq issue34_safety_gate=blocked' sh "$status" "$out"
run mount-unknown-key preflight mount-unknown-key 0 mount efs
check "malformed: unknown mount field refused, value not echoed" \
	sh -c '[ "$1" -eq 5 ] && ! printf "%s\n" "$2" | grep -Fq 123456789012345' sh "$status" "$out"
run mount-pre-mounted pre-efs-mounted '' 0 mount efs
check "preflight refusal: already mounted role never gets a mount session" \
	sh -c '[ "$1" -eq 6 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
run mount-pre-kernel pre-kernel '' 0 mount efs
check "preflight refusal: unpinned kernel never gets a mount session" sh -c '[ "$1" -eq 6 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
run mount-other-role pre-iostats mount-efs 0 mount efs
check "preflight: a problem on another role does not block this one" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 2 ]' sh "$status" "$runs"

# Copies of the workflow next to services that do not meet the read-only-loop
# contract: the previous direct partition mount, a service whose APKBUILD
# checksum is stale, and one whose package lacks losetup. The guest is told it
# runs the copy, so only the static contract can block the gate.
alt="$work/alt"
alt_pkg="$alt/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
alt_initd="$alt_pkg/ferrix-s5300-mounts.initd"
mkdir -p "$alt/tools/pixel-9a" "$alt_pkg"
cp "$proof" "$probe" "$root/tools/pixel-9a/acm-probe.py" "$alt/tools/pixel-9a/"
proof_orig=$proof
proof="$alt/tools/pixel-9a/s5300-mount-proof.sh"
alt_case() {
	# NAME: run a clean physical preflight against the current alt copy.
	derive preflight "pre-$1" -e "s/^service_sha256=.*/service_sha256=$(sha256 "$alt_initd")/"
	EVIDENCE=physical run "$1" "pre-$1" '' 0
}
pkg="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
sed 's#-- "$dev_root/$attached" "$destination"#-- "$device" "$destination"#' "$initd" >"$alt_initd"
awk -v sum="$( (shasum -a 512 "$alt_initd" 2>/dev/null || sha512sum "$alt_initd") | cut -c1-128)" \
	'$2 == "ferrix-s5300-mounts.initd" { $1 = sum; print; next } { print }' "$pkg/APKBUILD" >"$alt_pkg/APKBUILD"
alt_case alt-direct
check "unprotected service: a direct partition mount blocks a clean physical run" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq service_mount_path=unprotected && printf "%s\n" "$1" | grep -Fxq service_installed=matches-checkout &&
		printf "%s\n" "$1" | grep -Fxq issue34_gate_blocker=service-mounts-without-read-only-loop &&
		printf "%s\n" "$1" | grep -Fxq issue34_safety_gate=blocked' sh "$out"
{ cat "$initd"; printf '# changed\n'; } >"$alt_initd"
cp "$pkg/APKBUILD" "$alt_pkg/APKBUILD"
alt_case alt-stale
check "unprotected service: a stale APKBUILD checksum blocks" has 'issue34_gate_blocker=service-mounts-without-read-only-loop'
cp "$initd" "$alt_initd"
grep -vE '^[[:space:]]*losetup[[:space:]]*$' "$pkg/APKBUILD" >"$alt_pkg/APKBUILD"
alt_case alt-no-losetup
check "unprotected service: a package without util-linux losetup blocks" has 'issue34_gate_blocker=service-mounts-without-read-only-loop'
cp "$pkg/APKBUILD" "$alt_pkg/APKBUILD"
alt_case alt-same
check "control: the alt copy of the checked-in service meets the contract" has 'service_mount_path=read-only-loop'
EVIDENCE=physical run mount-loopwrite-protected preflight mount-loopwrite 0 mount efs
check "protected service: a rejected write attempt still blocks" has 'issue34_safety_gate=blocked'
proof=$proof_orig

mkdir -p "$work/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$work/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=3 FERRIX_PIXEL9A_S5300_MOUNT_PROOF_EVIDENCE=mock \
	sh "$proof" 2>&1) || status=$?
silent_port=$FERRIX_SHELL
acm_guest_stop
check "unreachable: exit 3 with sanitised reason" \
	sh -c '[ "$1" -eq 3 ] && printf "%s\n" "$2" | grep -Fq "transport failed (no-shell-response)"' sh "$status" "$out"
check "privacy: port path is neither printed nor retained" \
	sh -c '! printf "%s\n" "$1" | grep -Fq "$2" && ! grep -rFq "$2" "$3"' sh "$out" "$silent_port" "$work/run-unreachable"

check "no run ever reports a passed gate" sh -c '! grep -rq "issue34_safety_gate=passed" "$1"' sh "$work"

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-s5300-mount-proof-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-s5300-mount-proof-test","status":"passed"}\n'
