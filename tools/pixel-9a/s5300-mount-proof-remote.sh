#!/bin/sh
# Guest body of the issue #34 S5300 source mount proof. Sent over the
# CDC-ACM shell (acm-probe.py, piped to the guest `sh`) by
# s5300-mount-proof.sh, which prepends exactly five validated assignments:
#
#   proof_mode=preflight|mount
#   proof_role=            (mount: modem_img|efs|persist|modem_userdata)
#   proof_dev=             (mount: major:minor from the preflight session)
#   proof_devname=         (mount: kernel block name from the preflight)
#   proof_fstype=          (mount: ext4|f2fs from the preflight)
#
# preflight never mounts, never changes block-device state, and never reads
# partition contents: util-linux `blkid -p` reads only the superblock TYPE,
# and the rest comes from sysfs, /proc and the kernel log.
#
# mount (only after the host's typed confirmation) repeats the preflight for
# one role and refuses unless the partition is the one the preflight bound.
# It then marks that partition's block device read-only (BLKROSET), attaches
# it to an unbound loop device with `losetup --read-only`, and checks from
# sysfs that the loop is read-only and maps exactly the whole partition. The
# filesystem is mounted only from that loop device, never from the partition,
# with the exact ferrix-s5300-mounts options at a private /run path; the
# directory tree is read, the filesystem unmounted, and the loop detached.
# Block-layer counters of the partition and the loop, mountinfo, and kernel
# log lines from before, during, and after are reported. The partition's
# read-only flag is deliberately left set until the next boot.
#
# Why the loop: in the pinned 6.1 kernel BLKROSET only makes the block layer
# warn about a write (block/blk-core.c bio_check_ro); it does not fail it. A
# loop device configured read-only fails every write, discard, and
# write-zeroes request in loop_handle_cmd() before it reaches the backing
# file, and reports itself read-only (set_disk_ro) so ext4 and f2fs skip their
# orphan and error-superblock writes. See the board README.
set -u

partlabel_root=/dev/disk/by-partlabel
sys_block_path=/sys/class/block
mountinfo_path=/proc/self/mountinfo
mount_root=/mnt/vendor
proof_root=/run/ferrix-s5300-mount-proof
pinned_kernel=6.1.124-android14-11-ga4662a84a15f
roles='modem_img efs persist modem_userdata'

model=$(tr '\000' '\n' </proc/device-tree/model 2>/dev/null | sed -n '1p')
compatible=$(tr '\000' '\n' </proc/device-tree/compatible 2>/dev/null)
[ "$model" = 'ZUMA PRO TEGU MP on ZUMA PRO' ] || exit 4
printf '%s\n' "$compatible" | grep -Fxq 'google,ZUMA PRO TEGU' || exit 4

case "${proof_mode:-}" in preflight|mount) ;; *) exit 5 ;; esac

kernel_release=$(uname -r | tr -cd 'A-Za-z0-9._+~-')
uid=$(id -u)
printf 'probe=s5300-mount-proof\nmode=%s\n' "$proof_mode"
printf 'dt_model=%s\ndt_compatible=google,ZUMA PRO TEGU\n' "$model"
printf 'kernel_release=%s\nuid=%s\n' "$kernel_release" "$uid"

if ! command -v blkid >/dev/null 2>&1; then
	blkid_impl=missing
elif blkid -V 2>&1 | grep -q 'util-linux'; then
	blkid_impl=util-linux
else
	blkid_impl=other
fi
if ! command -v blockdev >/dev/null 2>&1; then
	blockdev_impl=missing
elif blockdev -V 2>&1 | grep -q 'util-linux'; then
	blockdev_impl=util-linux
else
	blockdev_impl=other
fi
if ! command -v losetup >/dev/null 2>&1; then
	losetup_impl=missing
elif losetup -V 2>&1 | grep -q 'util-linux'; then
	losetup_impl=util-linux
else
	losetup_impl=other
fi
printf 'blkid_impl=%s\nblockdev_impl=%s\nlosetup_impl=%s\n' "$blkid_impl" "$blockdev_impl" "$losetup_impl"

for fs in ext4 f2fs; do
	if awk -v fs="$fs" '$NF == fs { found = 1 } END { exit !found }' /proc/filesystems 2>/dev/null; then
		printf 'kernel_fs.%s=yes\n' "$fs"
	else
		printf 'kernel_fs.%s=no\n' "$fs"
	fi
done

# An unbound loop device that has never been left read-only: its sysfs `ro`
# must read 0 before it is attached, so that 1 afterwards can only come from
# the read-only loop configuration (the kernel leaves the old value in place
# when a loop is detached, drivers/block/loop.c __loop_clr_fd).
free_loop() {
	for path in "$sys_block_path"/loop*; do
		name=${path##*/}
		case "$name" in loop[0-9]|loop[0-9][0-9]|loop[0-9][0-9][0-9]) ;; *) continue ;; esac
		[ ! -e "$path/loop/backing_file" ] || continue
		[ -b "/dev/$name" ] || continue
		[ "$(cat "$path/ro" 2>/dev/null)" = 0 ] || continue
		[ "$(cat "$path/size" 2>/dev/null)" = 0 ] || continue
		printf '%s\n' "$name"
		return 0
	done
	return 1
}
loop_free=none
loop_free=$(free_loop) || loop_free=none
printf 'loop_free=%s\n' "$loop_free"

# The service and its consumer must not be able to start behind the proof.
for service in ferrix-s5300-mounts exynos-modem; do
	if ls /etc/runlevels/*/"$service" >/dev/null 2>&1; then
		printf 'runlevel.%s=present\n' "$service"
	else
		printf 'runlevel.%s=absent\n' "$service"
	fi
done

mountinfo_readable=no
[ -r "$mountinfo_path" ] && mountinfo_readable=yes
printf 'mountinfo_readable=%s\n' "$mountinfo_readable"
under_count() {
	awk -v prefix="$1/" 'index($5, prefix) == 1 || $5 == substr(prefix, 1, length(prefix) - 1) { n++ } END { print n + 0 }' \
		"$mountinfo_path" 2>/dev/null || printf 'unknown\n'
}
printf 'vendor_mounts=%s\nproof_mounts=%s\n' "$(under_count "$mount_root")" "$(under_count "$proof_root")"

kmsg_state=no
[ -w /dev/kmsg ] && dmesg >/dev/null 2>&1 && kmsg_state=yes
printf 'kmsg_usable=%s\n' "$kmsg_state"

# Same slot rule as the mount service: bootconfig first, then the command
# line, and exactly one _a/_b value.
slot_source=bootconfig
slot_values=$(sed -n 's/^[[:space:]]*androidboot\.slot_suffix[[:space:]]*=[[:space:]]*"*\([^"]*\)"*[[:space:]]*$/\1/p' /proc/bootconfig 2>/dev/null)
if [ -z "$slot_values" ]; then
	slot_source=cmdline
	slot_values=$(awk '{
		for (i = 1; i <= NF; i++) {
			if ($i ~ /^androidboot\.slot_suffix=/) {
				value = $i
				sub(/^androidboot\.slot_suffix=/, "", value)
				print value
			}
		}
	}' /proc/cmdline 2>/dev/null)
fi
slot_count=$(printf '%s\n' "$slot_values" | awk 'NF { count++ } END { print count + 0 }')
slot=unresolved
if [ "$slot_count" -eq 1 ]; then
	case "$slot_values" in
		_a|_b) slot=$slot_values ;;
		*) slot=unsupported ;;
	esac
fi
printf 'slot_source=%s\nslot_value_count=%s\nslot_suffix=%s\n' "$slot_source" "$slot_count" "$slot"

# Block-layer counters of one partition: the 17 fields of
# Documentation/ABI/stable/sysfs-block "stat", or `unavailable`.
read_stat() {
	value=$(tr -s ' \t' '  ' <"$sys_block_path/$1/stat" 2>/dev/null | sed 's/^ //; s/ $//')
	case "$value" in
		*[!0-9\ ]*|'') printf 'unavailable\n' ;;
		*) printf '%s\n' "$value" ;;
	esac
}

read_flag() {
	value=$(cat "$1" 2>/dev/null) || value=unknown
	case "$value" in 0|1) printf '%s\n' "$value" ;; *) printf 'unknown\n' ;; esac
}

# Resolve and describe one role exactly as the mount service does, plus the
# block-device state the proof needs. Sets the part_* globals.
probe_part() {
	role=$1
	label=$2
	key="part.$role"
	printf '%s.label=%s\n' "$key" "$label"
	part_link=''
	link_state=missing
	if [ "$label" != unresolved ]; then
		part_link=$(readlink -f "$partlabel_root/$label" 2>/dev/null) || part_link=''
		if [ -n "$part_link" ] && [ -b "$part_link" ]; then
			link_state=block
		elif [ -n "$part_link" ] && [ -e "$part_link" ]; then
			link_state=not-block
		else
			part_link=''
		fi
	fi
	part_devname=${part_link##*/}
	part_link_state=$link_state
	printf '%s.link_state=%s\n%s.link_device=%s\n' "$key" "$link_state" "$key" "$part_devname"

	owners=''
	[ "$label" = unresolved ] || owners=$(grep -Flx "PARTNAME=$label" "$sys_block_path"/*/uevent 2>/dev/null)
	part_owner_count=$(printf '%s\n' "$owners" | awk 'NF { count++ } END { print count + 0 }')
	part_owner_names=''
	for owner in $owners; do
		name=$(awk -F= '$1 == "DEVNAME" { print $2; exit }' "$owner" 2>/dev/null)
		part_owner_names="$part_owner_names${part_owner_names:+ }${name:-unnamed}"
	done
	printf '%s.partname_count=%s\n%s.partname_devices=%s\n' "$key" "$part_owner_count" "$key" "$part_owner_names"

	part_dev=''
	part_parent=''
	part_ro=unknown
	part_iostats=unknown
	part_holders=unknown
	part_stat=unavailable
	part_unique=no
	if [ "$link_state" = block ] && [ "$part_owner_count" -eq 1 ] && [ "$part_owner_names" = "$part_devname" ]; then
		part_unique=yes
		part_dev=$(cat "$sys_block_path/$part_devname/dev" 2>/dev/null) || part_dev=''
		parent_path=$(readlink -f "$sys_block_path/$part_devname/.." 2>/dev/null) || parent_path=''
		[ -f "$sys_block_path/$part_devname/partition" ] && part_parent=${parent_path##*/}
		part_ro=$(read_flag "$sys_block_path/$part_devname/ro")
		[ -z "$part_parent" ] || part_iostats=$(read_flag "$sys_block_path/$part_parent/queue/iostats")
		part_holders=$(ls -A "$sys_block_path/$part_devname/holders" 2>/dev/null | awk 'END { print NR }')
		part_stat=$(read_stat "$part_devname")
	fi
	printf '%s.dev=%s\n%s.parent=%s\n' "$key" "$part_dev" "$key" "$part_parent"
	printf '%s.bdev_ro=%s\n%s.iostats=%s\n%s.holders=%s\n' "$key" "$part_ro" "$key" "$part_iostats" "$key" "$part_holders"
	printf '%s.stat=%s\n' "$key" "$part_stat"

	part_blkid_exit=not-run
	part_type=''
	if [ "$part_unique" = yes ]; then
		if [ "$blkid_impl" != util-linux ]; then
			part_blkid_exit=unavailable-blkid
		elif [ "$uid" != 0 ]; then
			part_blkid_exit=unavailable-privilege
		else
			part_blkid_exit=0
			part_type=$(blkid -c /dev/null -p -o value -s TYPE -- "$part_link" 2>/dev/null) || part_blkid_exit=$?
			part_type=$(printf '%s' "$part_type" | tr -cd 'A-Za-z0-9._-' | cut -c1-32)
		fi
	fi
	printf '%s.blkid_exit=%s\n%s.blkid_type=%s\n' "$key" "$part_blkid_exit" "$key" "$part_type"

	part_mounts=unknown
	if [ "$mountinfo_readable" = yes ]; then
		part_mounts=$(awk -v dev="$part_dev" -v dest="$mount_root/$role" -v own="$proof_root/$role" '
			(dev != "" && $3 == dev) || $5 == dest || $5 == own { n++ } END { print n + 0 }' "$mountinfo_path" 2>/dev/null)
	fi
	printf '%s.mounts=%s\n' "$key" "$part_mounts"
}

role_label() {
	case "$1" in
		modem_img) case "$slot" in _a|_b) printf 'modem%s\n' "$slot" ;; *) printf 'unresolved\n' ;; esac ;;
		*) printf '%s\n' "$1" ;;
	esac
}

if [ "$proof_mode" = preflight ]; then
	for role in $roles; do
		probe_part "$role" "$(role_label "$role")"
	done
	printf 'probe_complete=yes\n'
	exit 0
fi

# --- mount mode ---------------------------------------------------------------

refuse() {
	printf 'refusal=%s\nstate_changed=no\nprobe_complete=yes\n' "$1"
	exit 6
}

case "$proof_role" in modem_img|efs|persist|modem_userdata) ;; *) exit 5 ;; esac
case "$proof_fstype" in ext4) recovery=noload ;; f2fs) recovery=norecovery ;; *) exit 5 ;; esac
case "$proof_dev" in *[!0-9:]*|'') exit 5 ;; esac
case "$proof_devname" in *[!a-z0-9]*|'') exit 5 ;; esac

probe_part "$proof_role" "$(role_label "$proof_role")"
printf 'mount_options=ro,%s,nosuid,nodev,noexec\n' "$recovery"

[ "$kernel_release" = "$pinned_kernel" ] || refuse kernel-not-pinned
[ "$uid" = 0 ] || refuse not-root
[ "$blkid_impl" = util-linux ] || refuse blkid-not-util-linux
[ "$blockdev_impl" != missing ] || refuse blockdev-missing
[ "$losetup_impl" = util-linux ] || refuse losetup-not-util-linux
[ "$loop_free" != none ] || refuse no-unbound-loop-with-ro-0
[ "$mountinfo_readable" = yes ] || refuse mountinfo-unreadable
[ "$kmsg_state" = yes ] || refuse kmsg-unusable
awk -v fs="$proof_fstype" '$NF == fs { found = 1 } END { exit !found }' /proc/filesystems 2>/dev/null ||
	refuse filesystem-not-in-kernel
for service in ferrix-s5300-mounts exynos-modem; do
	! ls /etc/runlevels/*/"$service" >/dev/null 2>&1 || refuse "$service-in-runlevel"
done
[ "$(under_count "$mount_root")" = 0 ] || refuse vendor-mount-present
[ "$(under_count "$proof_root")" = 0 ] || refuse proof-mount-present
[ "$part_unique" = yes ] || refuse partition-not-unique
[ "$part_dev" = "$proof_dev" ] && [ "$part_devname" = "$proof_devname" ] || refuse partition-changed-since-preflight
[ "$part_blkid_exit" = 0 ] && [ "$part_type" = "$proof_fstype" ] || refuse type-changed-since-preflight
[ "$part_mounts" = 0 ] || refuse partition-already-mounted
[ "$part_holders" = 0 ] || refuse partition-has-holders
[ "$part_iostats" = 1 ] || refuse iostats-disabled
[ "$part_stat" != unavailable ] || refuse stat-unavailable

device=$partlabel_root/$(role_label "$proof_role")
device=$(readlink -f "$device")
[ "$device" = "/dev/$proof_devname" ] || refuse device-node-not-in-dev
part_size=$(cat "$sys_block_path/$proof_devname/size" 2>/dev/null) || part_size=''
case "$part_size" in ''|*[!0-9]*|0) refuse partition-size-unavailable ;; esac
loop_name=$loop_free
loop=/dev/$loop_name
destination=$proof_root/$proof_role
marker=$(tr -cd '0-9a-f' </proc/sys/kernel/random/uuid 2>/dev/null)
[ -n "$marker" ] || refuse no-marker
printf 'ferrix-s5300-mount-proof: begin %s\n' "$marker" >/dev/kmsg 2>/dev/null || refuse kmsg-write-failed

# From here block-device state changes; every exit reports it.
printf 'state_changed=bdev-ro\n'
printf 'loop_name=%s\n' "$loop_name"
printf 'stat.initial=%s\n' "$(read_stat "$proof_devname")"
printf 'loop_stat.initial=%s\n' "$(read_stat "$loop_name")"
blockdev --setro "$device" >/dev/null 2>&1
setro_exit=$?
ro_now=$(read_flag "$sys_block_path/$proof_devname/ro")
printf 'setro_exit=%s\nbdev_ro.before=%s\n' "$setro_exit" "$ro_now"

bound=no
mounted=no
losetup_exit=not-run
detach_exit=not-run
mount_exit=not-run
umount_exit=not-run
loop_ro_before=not-run
loop_ro=not-run
loop_dev=''
loop_backing_match=not-run
loop_geometry=not-run
loop_iostats=not-run
loop_type=''
mountinfo_line=''
entries=not-run
stat_before=not-run
stat_mounted=not-run
stat_settled=not-run
loop_stat_before=not-run
loop_stat_mounted=not-run
loop_stat_settled=not-run
cleanup() {
	if [ "$mounted" = yes ]; then
		umount -- "$destination" >/dev/null 2>&1
		umount_exit=$?
		[ "$umount_exit" -ne 0 ] || mounted=no
	fi
	if [ "$bound" = yes ] && [ "$mounted" = no ]; then
		losetup -d "$loop" >/dev/null 2>&1
		detach_exit=$?
		[ -e "$sys_block_path/$loop_name/loop/backing_file" ] || bound=no
	fi
}
# A transport timeout sends Ctrl-C; unmount, detach, report nothing more.
trap 'cleanup; exit 7' HUP INT TERM

if [ "$setro_exit" -eq 0 ] && [ "$ro_now" = 1 ]; then
	# Re-check the chosen loop immediately before attaching it.
	loop_ro_before=$(read_flag "$sys_block_path/$loop_name/ro")
	if [ "$loop_ro_before" = 0 ] && [ ! -e "$sys_block_path/$loop_name/loop/backing_file" ]; then
		losetup --read-only -- "$loop" "$device" >/dev/null 2>&1
		losetup_exit=$?
		# Only a successful attach is ours: if another process took this
		# loop first, losetup fails and its binding is left alone.
		[ "$losetup_exit" -ne 0 ] || [ ! -e "$sys_block_path/$loop_name/loop/backing_file" ] || bound=yes
	fi
fi
if [ "$bound" = yes ]; then
	loop_ro=$(read_flag "$sys_block_path/$loop_name/ro")
	loop_dev=$(cat "$sys_block_path/$loop_name/dev" 2>/dev/null) || loop_dev=''
	backing=$(cat "$sys_block_path/$loop_name/loop/backing_file" 2>/dev/null) || backing=''
	loop_backing_match=no
	[ "$backing" = "$device" ] && loop_backing_match=yes
	offset=$(cat "$sys_block_path/$loop_name/loop/offset" 2>/dev/null) || offset=''
	sizelimit=$(cat "$sys_block_path/$loop_name/loop/sizelimit" 2>/dev/null) || sizelimit=''
	loop_size=$(cat "$sys_block_path/$loop_name/size" 2>/dev/null) || loop_size=''
	loop_geometry=mismatch
	[ "$offset" = 0 ] && [ "$sizelimit" = 0 ] && [ "$loop_size" = "$part_size" ] && loop_geometry=whole-partition
	loop_iostats=$(read_flag "$sys_block_path/$loop_name/queue/iostats")
	loop_type=$(blkid -c /dev/null -p -o value -s TYPE -- "$loop" 2>/dev/null) || loop_type=''
	loop_type=$(printf '%s' "$loop_type" | tr -cd 'A-Za-z0-9._-' | cut -c1-32)
fi
# Mount only from a loop that sysfs shows read-only, mapping exactly the
# whole bound partition, with the same superblock type.
case "$loop_dev" in 7:[0-9]*) loop_major=7 ;; *) loop_major=other ;; esac
if [ "$bound" = yes ] && [ "$loop_ro" = 1 ] && [ "$loop_major" = 7 ] && [ "$loop_backing_match" = yes ] &&
	[ "$loop_geometry" = whole-partition ] && [ "$loop_type" = "$proof_fstype" ]; then
	sync
	sleep 1
	stat_before=$(read_stat "$proof_devname")
	loop_stat_before=$(read_stat "$loop_name")
	mkdir -p "$destination"
	mount -t "$proof_fstype" -o "ro,$recovery,nosuid,nodev,noexec" -- "$loop" "$destination" >/dev/null 2>&1
	mount_exit=$?
	if [ "$mount_exit" -eq 0 ]; then
		mounted=yes
		mountinfo_line=$(awk -v dest="$destination" '$5 == dest { line = $0 } END { print line }' "$mountinfo_path")
		stat_mounted=$(read_stat "$proof_devname")
		loop_stat_mounted=$(read_stat "$loop_name")
		# Metadata reads only; names are counted, never printed.
		entries=$(timeout 30 find "$destination" -xdev 2>/dev/null | awk 'END { print NR }')
		sleep 5
		stat_settled=$(read_stat "$proof_devname")
		loop_stat_settled=$(read_stat "$loop_name")
		umount -- "$destination" >/dev/null 2>&1
		umount_exit=$?
		[ "$umount_exit" -ne 0 ] || mounted=no
	fi
	rmdir "$destination" 2>/dev/null
fi
loop_stat_after=$(read_stat "$loop_name")
cleanup
sync
sleep 1
trap - HUP INT TERM

printf 'loop_ro.before=%s\nlosetup_exit=%s\nloop_dev=%s\nloop_ro=%s\n' "$loop_ro_before" "$losetup_exit" "$loop_dev" "$loop_ro"
printf 'loop_backing_match=%s\nloop_geometry=%s\nloop_iostats=%s\nloop_type=%s\n' "$loop_backing_match" "$loop_geometry" "$loop_iostats" "$loop_type"
printf 'stat.before=%s\nloop_stat.before=%s\n' "$stat_before" "$loop_stat_before"
# Keep the separator and superblock fields; replace the source field.
printf 'mountinfo=%s\n' "$(printf '%s\n' "$mountinfo_line" | awk '
	NF { for (i = 7; i + 2 <= NF; i++) if ($i == "-") { $(i + 2) = "redacted-source"; break }; print }')"
printf 'mount_exit=%s\nentries=%s\n' "$mount_exit" "$entries"
printf 'stat.mounted=%s\nstat.settled=%s\n' "$stat_mounted" "$stat_settled"
printf 'loop_stat.mounted=%s\nloop_stat.settled=%s\nloop_stat.after=%s\n' "$loop_stat_mounted" "$loop_stat_settled" "$loop_stat_after"
printf 'umount_exit=%s\nstill_mounted=%s\n' "$umount_exit" "$mounted"
printf 'detach_exit=%s\nstill_bound=%s\n' "$detach_exit" "$bound"
printf 'stat.after=%s\n' "$(read_stat "$proof_devname")"
printf 'bdev_ro.after=%s\n' "$(read_flag "$sys_block_path/$proof_devname/ro")"

# Kernel log after the marker that names this partition or the loop, a
# filesystem, or the block layer's read-only write warning. UUIDs are masked;
# at most 40 lines.
dmesg 2>/dev/null | awk -v marker="ferrix-s5300-mount-proof: begin $marker" -v dev="$proof_devname" -v lo="$loop_name" '
	seen && (index($0, dev) || index($0, lo) || /EXT4-fs|F2FS-fs|read-only block-device|blk_update_request|I\/O error|loop: Write error/) { print }
	index($0, marker) { seen = 1 }
' | sed -e 's/[0-9a-fA-F]\{8\}-[0-9a-fA-F]\{4\}-[0-9a-fA-F]\{4\}-[0-9a-fA-F]\{4\}-[0-9a-fA-F]\{12\}/uuid/g' \
	-e 's/^\[[ 0-9.]*\] *//' -e 's/^<[0-9]*>//' | tr -cd 'A-Za-z0-9 ._:,+~()/=\n-' | cut -c1-160 | sed -n '1,40p' |
	awk '{ printf "kmsg.%02d=%s\n", NR, $0 }'
printf 'probe_complete=yes\n'
[ "$mounted" = no ] || exit 7
[ "$bound" = no ] || exit 7
[ "$mount_exit" = 0 ] || exit 7
exit 0
