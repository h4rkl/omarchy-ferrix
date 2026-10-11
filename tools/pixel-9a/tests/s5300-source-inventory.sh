#!/bin/sh
# Offline proof for the read-only S5300 source partition inventory. The real
# CDC-ACM transport talks to a fake guest (acm-fake-guest.sh): a real
# interactive shell on a pty whose final `sh` is a stub that checks it got the
# exact probe bytes, then answers with a synthetic probe transcript; no device
# is contacted.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
inventory="$root/tools/pixel-9a/s5300-source-inventory.sh"
probe="$root/tools/pixel-9a/s5300-source-inventory-remote.sh"
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

check "static: host POSIX syntax" sh -n "$inventory"
check "static: probe POSIX syntax" sh -n "$probe"
for script in "$inventory" "$probe"; do
	# Field names and status words contain "mount"; a command never does.
	offending=$(grep -nwE 'adb|fastboot|mount|umount|dd|mkfs|fsck|e2fsck|tune2fs|debugfs|flash|erase|reboot|losetup' "$script" |
		grep -vE '^[0-9]+:[[:space:]]*#' | grep -vE 'mount_|mountinfo|\.mount|-mount|mount-|not-mounted' || true)
	check "static: no storage, mount, or Android verbs in ${script##*/}" [ -z "$offending" ]
	[ -z "$offending" ] || printf '%s\n' "$offending"
done
check "static: probe reads only TYPE with uncached low-level blkid" \
	sh -c '[ "$(grep -c "blkid -c /dev/null -p -o value -s TYPE -- \"\$link\"" "$1")" -eq 1 ] && [ "$(grep -cE "[^-]blkid .*-s (UUID|LABEL|PARTUUID)" "$1")" -eq 0 ]' sh "$probe"
check "static: probe gates on the exact tegu device tree" \
	sh -c 'grep -Fq "ZUMA PRO TEGU MP on ZUMA PRO" "$1" && grep -Fq "google,ZUMA PRO TEGU" "$1"' sh "$probe"
check "static: exactly one guest session, through the CDC-ACM transport" \
	sh -c '[ "$(grep -v "^[[:space:]]*#" "$1" | grep -cE "python3 \"\\\$transport\" --probe \"\\\$probe\" ")" -eq 1 ]' sh "$inventory"
check "static: no SSH or other network transport" \
	sh -c '! grep -v "^[[:space:]]*#" "$1" | grep -nwE "ssh|scp|nc|telnet|curl"' sh "$inventory"
check "static: the host never writes a passed gate" sh -c '! grep -nE "gate=passed|safety_gate passed" "$1"' sh "$inventory"
# Off the target the probe must refuse before reading any partition state.
probe_status=0
sh "$probe" >/dev/null 2>&1 </dev/null || probe_status=$?
check "probe: refuses a host that is not the tegu board" [ "$probe_status" -eq 4 ]

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-inventory-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; rm -rf "$work"' EXIT
mkdir -p "$work/fx"
acm_guest_init "$root" "$work" "$probe"
unset FERRIX_PIXEL9A_ACM_TIMEOUT 2>/dev/null || true

part() {
	printf 'part.%s.label=%s\npart.%s.link_state=block\npart.%s.link_device=%s\n' "$1" "$2" "$1" "$1" "$3"
	printf 'part.%s.partname_count=1\npart.%s.partname_devices=%s\npart.%s.dev=%s\n' "$1" "$1" "$3" "$1" "$4"
	printf 'part.%s.blkid_exit=0\npart.%s.blkid_type=%s\n' "$1" "$1" "$5"
}
{
	printf 'probe=s5300-source-inventory\ndt_model=ZUMA PRO TEGU MP on ZUMA PRO\ndt_compatible=google,ZUMA PRO TEGU\n'
	printf 'kernel_release=6.1.0-test\nuid=1000\nprivilege=doas\nblkid_impl=util-linux\n'
	printf 'slot_source=bootconfig\nslot_value_count=1\nslot_suffix=_b\nmountinfo_readable=yes\n'
	part modem_img modem_b sda20 8:20 ext4
	part efs efs sda21 8:21 ext4
	part persist persist sda22 8:22 ext4
	part modem_userdata modem_userdata sda23 8:23 ext4
	printf 'probe_complete=yes\n'
} >"$work/fx/ext4"

derive() {
	name=$1
	shift
	sed "$@" "$work/fx/ext4" >"$work/fx/$name"
}
safe_mount='part.efs.mount=40 30 8:21 / /mnt/vendor/efs ro,nosuid,nodev,noexec,relatime - f2fs /dev/sda21 ro,lazytime,norecovery,background_gc=on'
derive f2fs -e 's/^part\.efs\.blkid_type=ext4$/part.efs.blkid_type=f2fs/' -e "/^probe_complete=/i\\
$safe_mount"
derive f2fs-rw -e 's/^part\.efs\.blkid_type=ext4$/part.efs.blkid_type=f2fs/' -e '/^probe_complete=/i\
part.efs.mount=40 30 8:21 / /mnt/vendor/efs ro,relatime - f2fs /dev/sda21 rw,lazytime'
derive ext4-elsewhere -e '/^probe_complete=/i\
part.persist.mount=41 30 8:22 / /mnt/other ro,relatime - ext4 /dev/sda22 ro,noload'
derive unknown -e 's/^part\.persist\.blkid_exit=0$/part.persist.blkid_exit=2/' -e 's/^part\.persist\.blkid_type=ext4$/part.persist.blkid_type=/' \
	-e 's/^part\.modem_userdata\.blkid_exit=0$/part.modem_userdata.blkid_exit=8/' -e 's/^part\.modem_userdata\.blkid_type=ext4$/part.modem_userdata.blkid_type=/' \
	-e 's/^part\.modem_img\.blkid_type=ext4$/part.modem_img.blkid_type=vfat/'
derive privilege -e 's/^privilege=doas$/privilege=none/' -e 's/^part\.efs\.blkid_exit=0$/part.efs.blkid_exit=unavailable-privilege/' -e 's/^part\.efs\.blkid_type=ext4$/part.efs.blkid_type=/'
derive duplicate -e 's/^part\.efs\.partname_count=1$/part.efs.partname_count=2/' -e 's/^part\.efs\.partname_devices=sda21$/part.efs.partname_devices=sda21 sdb5/' \
	-e 's/^part\.efs\.blkid_exit=0$/part.efs.blkid_exit=not-run/' -e 's/^part\.efs\.blkid_type=ext4$/part.efs.blkid_type=/'
derive mismatch -e 's/^part\.persist\.partname_devices=sda22$/part.persist.partname_devices=sda30/' \
	-e 's/^part\.persist\.blkid_exit=0$/part.persist.blkid_exit=not-run/' -e 's/^part\.persist\.blkid_type=ext4$/part.persist.blkid_type=/'
derive bad-mountinfo -e '/^probe_complete=/i\
part.efs.mount=40 30 8:21 / /mnt/vendor/efs ro ext4 /dev/sda21'
derive foreign -e '/^probe_complete=/i\
part.modem_userdata.mount=42 30 0:44 / /mnt/vendor/modem_userdata rw,relatime - tmpfs tmpfs rw'
derive slot -e 's/^slot_suffix=_b$/slot_suffix=unresolved/' -e 's/^slot_value_count=1$/slot_value_count=0/' \
	-e 's/^part\.modem_img\.label=modem_b$/part.modem_img.label=unresolved/' -e 's/^part\.modem_img\.link_state=block$/part.modem_img.link_state=missing/' \
	-e 's/^part\.modem_img\.link_device=sda20$/part.modem_img.link_device=/' -e 's/^part\.modem_img\.partname_count=1$/part.modem_img.partname_count=0/' \
	-e 's/^part\.modem_img\.partname_devices=sda20$/part.modem_img.partname_devices=/' -e 's/^part\.modem_img\.dev=8:20$/part.modem_img.dev=/' \
	-e 's/^part\.modem_img\.blkid_exit=0$/part.modem_img.blkid_exit=not-run/' -e 's/^part\.modem_img\.blkid_type=ext4$/part.modem_img.blkid_type=/'
derive qemu -e 's/^dt_model=.*/dt_model=QEMU Virtual Machine/'
derive unknown-key -e '/^probe_complete=/i\
serial=ABC123'
derive duplicate-key -e '/^probe_complete=/i\
slot_suffix=_a'
derive wrong-label -e 's/^part\.modem_img\.label=modem_b$/part.modem_img.label=modem_a/'

status=0
out=''
# run CASE FIXTURE [EXIT [EVIDENCE]]: one fresh fake guest per case.
run() {
	case_dir="$work/run-$1"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl fixture "$work/fx/$2"
	acm_guest_ctl exit "${3:-0}"
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE="${4:-mock}" \
		sh "$inventory" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	transcript="$case_dir/pixel-9a/s5300-source-inventory/inventory-latest.txt"
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }

run ext4 ext4
check "ext4: completes" [ "$status" -eq 0 ]
check "ext4: the probe ran exactly once over CDC-ACM" [ "$runs" -eq 1 ]
check "ext4: the guest received the exact checked-in probe bytes" cmp -s "$ACM_GUEST_CTL/stdin" "$probe"
check "ext4: transcript records the transport and probe hash" \
	sh -c 'grep -Fxq source=cdc-acm-read-only "$1" && grep -Eq "^probe_sha256=[0-9a-f]{64}$" "$1"' sh "$transcript"
check "ext4: every source is unique ext4" sh -c '[ "$(printf "%s\n" "$1" | grep -cE "^[a-z_]+\.type_status=ext4$")" -eq 4 ]' sh "$out"
check "ext4: service would accept each source" sh -c '[ "$(printf "%s\n" "$1" | grep -cE "^[a-z_]+\.service_would_accept=yes$")" -eq 4 ]' sh "$out"
check "ext4: active slot modem label" has 'modem_img.label=modem_b'
check "ext4: unmounted flag display stays unknown" has 'ext4_recovery_flag_in_mountinfo=unknown'
check "ext4: on-device suppression stays unknown" has 'journal_replay_suppression_on_device=unknown'
check "ext4: mock never evaluates the gate" has 'issue34_safety_gate=not-evaluated-mock'
check "ext4: transcript is labelled mock" grep -Fxq 'evidence=mock' "$transcript"
check "ext4: transcript keeps raw probe output" grep -Fxq 'part.efs.blkid_type=ext4' "$transcript"

run physical ext4 0 physical
check "physical label: clean run still needs owner review" has 'issue34_safety_gate=owner-review-required'
check "physical label: transcript names the exact target" grep -Fxq 'evidence=physical:google-pixel-9a-tegu-gxq96-mp1-0' "$transcript"

run f2fs f2fs
check "f2fs: safe ro,norecovery mount meets the contract" has 'efs.mount_status=contract-met'
check "f2fs: recovery flag observed" has 'f2fs_recovery_flag_in_mountinfo=observed'
check "f2fs: raw type reported" has 'efs.raw_type=f2fs'
check "privacy: mountinfo source is redacted in retained transcript" \
	sh -c 'grep -Fq "<redacted-source>" "$1" && ! grep -Fq "/dev/sda21" "$1"' sh "$transcript"

run f2fs-rw f2fs-rw
check "f2fs rw: writable superblock violates the contract" has 'efs.mount_status=contract-violated:superblock-not-ro,recovery-not-suppressed'
check "f2fs rw: service would refuse" has 'efs.service_would_accept=no'
run physical-rw f2fs-rw 0 physical
check "f2fs rw: physical gate blocked" has 'issue34_safety_gate=blocked'

run elsewhere ext4-elsewhere
check "mount outside destination is reported" has 'persist.mount_status=contract-violated:mounted-outside-destination'

run unknown unknown 0 physical
check "unknown: no signature" has 'persist.type_status=no-signature'
check "unknown: ambivalent" has 'modem_userdata.type_status=ambivalent'
check "unknown: unsupported raw type kept" sh -c 'printf "%s\n" "$1" | grep -Fxq modem_img.type_status=unsupported && printf "%s\n" "$1" | grep -Fxq modem_img.raw_type=vfat' sh "$out"
check "unknown: physical gate blocked" has 'issue34_safety_gate=blocked'

run privilege privilege
check "privilege: unavailable blkid read is unknown" has 'efs.type_status=unknown-privilege-unavailable'

run duplicate duplicate
check "duplicate: refused label" has 'efs.resolution=duplicate-partname'
check "duplicate: both kernel devices kept" has 'efs.kernel_partname_devices=sda21 sdb5'
check "duplicate: not probed" has 'efs.type_status=not-probed'

run mismatch mismatch
check "mismatch: link and kernel device differ" has 'persist.resolution=device-mismatch'

run bad-mountinfo bad-mountinfo
check "bad mountinfo: reported malformed" has 'efs.mount_status=malformed-mountinfo'
check "bad mountinfo: service would refuse" has 'efs.service_would_accept=no'

run foreign foreign
check "foreign: other device at destination" has 'modem_userdata.mount_status=contract-violated:foreign-mount-at-destination'

run slot slot
check "slot: unresolved slot reported" has 'slot_status=unresolved'
check "slot: modem image not guessed" has 'modem_img.resolution=slot-unresolved'
check "slot: other sources still inventoried" has 'efs.type_status=ext4'

# Transport failures: an unresponsive port, a wedged probe, and an overrun.
mkdir -p "$work/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$work/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=3 FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE=mock \
	sh "$inventory" 2>&1) || status=$?
silent_port=$FERRIX_SHELL
acm_guest_stop
check "unreachable: exit 3" [ "$status" -eq 3 ]
check "unreachable: sanitised transport reason" has 'REFUSING pixel-9a S5300 source inventory: guest CDC-ACM shell transport failed (no-shell-response)'
check "unreachable: refusal recorded" sh -c 'grep -Fxq result=refused "$1"/pixel-9a/s5300-source-inventory/inventory-*.txt' sh "$work/run-unreachable"
check "privacy: port path is neither printed nor retained" \
	sh -c '! printf "%s\n" "$1" | grep -Fq "$2" && ! grep -rFq "$2" "$3"' sh "$out" "$silent_port" "$work/run-unreachable"

mkdir -p "$work/run-timeout"
acm_guest_start
acm_guest_ctl fixture "$work/fx/ext4"
acm_guest_ctl sleep 30
status=0
out=$(FERRIX_OUT_DIR="$work/run-timeout" FERRIX_PIXEL9A_ACM_TIMEOUT=3 FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE=mock \
	sh "$inventory" 2>&1) || status=$?
acm_guest_stop
check "timeout: wedged probe exits 3" [ "$status" -eq 3 ]
check "timeout: reason is timeout" has 'REFUSING pixel-9a S5300 source inventory: guest CDC-ACM shell transport failed (timeout)'
check "timeout: no inventory is reported" sh -c '! printf "%s\n" "$1" | grep -q "^efs\."' sh "$out"

mkdir -p "$work/run-overrun"
acm_guest_start
acm_guest_ctl fixture "$work/fx/ext4"
acm_guest_ctl flood 70000
status=0
out=$(FERRIX_OUT_DIR="$work/run-overrun" FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE=mock sh "$inventory" 2>&1) || status=$?
acm_guest_stop
check "truncation: output over 64 KiB exits 3" [ "$status" -eq 3 ]
check "truncation: reason is output-limit" has 'REFUSING pixel-9a S5300 source inventory: guest CDC-ACM shell transport failed (output-limit)'

mkdir -p "$work/run-bad-timeout-env"
status=0
FERRIX_OUT_DIR="$work/run-bad-timeout-env" FERRIX_PIXEL9A_ACM_TIMEOUT='5;id' sh "$inventory" >/dev/null 2>&1 || status=$?
check "usage: malformed transport timeout is refused before any session" \
	sh -c '[ "$1" -eq 2 ] && [ ! -d "$2/pixel-9a" ]' sh "$status" "$work/run-bad-timeout-env"

run identity ext4 4
check "identity mismatch: exit 4" [ "$status" -eq 4 ]
run qemu qemu
check "identity: QEMU device tree refused" [ "$status" -eq 4 ]
run unknown-key unknown-key
check "malformed: unknown field refused" [ "$status" -eq 5 ]
check "malformed: unknown value not echoed" sh -c '! printf "%s\n" "$1" | grep -Fq ABC123' sh "$out"
run duplicate-key duplicate-key
check "malformed: duplicate field refused" [ "$status" -eq 5 ]
run wrong-label wrong-label
check "malformed: wrong-slot modem label refused" [ "$status" -eq 5 ]

check "no run ever reports a passed gate" sh -c '! grep -rq "issue34_safety_gate=passed" "$1"' sh "$work"

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-s5300-source-inventory-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-s5300-source-inventory-test","status":"passed"}\n'
