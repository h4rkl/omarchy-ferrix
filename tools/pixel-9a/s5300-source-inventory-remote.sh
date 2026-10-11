#!/bin/sh
# Sent over SSH by s5300-source-inventory.sh. Read-only probe of the four
# S5300 modem source partitions on the exact tegu guest. It never mounts,
# writes, or reads partition contents: util-linux `blkid -p` reads only the
# superblock signature and prints only TYPE (never UUID or LABEL). The paths
# and resolution rules mirror pmos/device-google-tegu/ferrix-s5300-mounts.initd.
set -u

partlabel_root=/dev/disk/by-partlabel
sys_block_path=/sys/class/block
mountinfo_path=/proc/self/mountinfo
mount_root=/mnt/vendor

model=$(tr '\000' '\n' </proc/device-tree/model 2>/dev/null | sed -n '1p')
compatible=$(tr '\000' '\n' </proc/device-tree/compatible 2>/dev/null)
[ "$model" = 'ZUMA PRO TEGU MP on ZUMA PRO' ] || exit 4
printf '%s\n' "$compatible" | grep -Fxq 'google,ZUMA PRO TEGU' || exit 4

printf 'probe=s5300-source-inventory\n'
printf 'dt_model=%s\n' "$model"
printf 'dt_compatible=google,ZUMA PRO TEGU\n'
printf 'kernel_release=%s\n' "$(uname -r | tr -cd 'A-Za-z0-9._+~-')"
uid=$(id -u)
printf 'uid=%s\n' "$uid"

# blkid needs read access to the block node. Use a non-interactive
# privilege prefix only; never prompt or escalate any other command.
if [ "$uid" = 0 ]; then
	priv=''
	printf 'privilege=root\n'
elif command -v doas >/dev/null 2>&1 && doas -n true >/dev/null 2>&1; then
	priv='doas -n'
	printf 'privilege=doas\n'
elif command -v sudo >/dev/null 2>&1 && sudo -n true >/dev/null 2>&1; then
	priv='sudo -n'
	printf 'privilege=sudo\n'
else
	priv=none
	printf 'privilege=none\n'
fi

if ! command -v blkid >/dev/null 2>&1; then
	blkid_impl=missing
elif blkid -V 2>&1 | grep -q 'util-linux'; then
	blkid_impl=util-linux
else
	blkid_impl=other
fi
printf 'blkid_impl=%s\n' "$blkid_impl"

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

mountinfo_readable=no
[ -r "$mountinfo_path" ] && mountinfo_readable=yes
printf 'mountinfo_readable=%s\n' "$mountinfo_readable"

probe_part() {
	role=$1
	label=$2
	key="part.$role"
	printf '%s.label=%s\n' "$key" "$label"
	link=''
	link_state=missing
	if [ "$label" != unresolved ]; then
		link=$(readlink -f "$partlabel_root/$label" 2>/dev/null) || link=''
		if [ -n "$link" ] && [ -b "$link" ]; then
			link_state=block
		elif [ -n "$link" ] && [ -e "$link" ]; then
			link_state=not-block
		else
			link=''
		fi
	fi
	link_device=${link##*/}
	printf '%s.link_state=%s\n%s.link_device=%s\n' "$key" "$link_state" "$key" "$link_device"

	owners=''
	[ "$label" = unresolved ] || owners=$(grep -Flx "PARTNAME=$label" "$sys_block_path"/*/uevent 2>/dev/null)
	owner_count=$(printf '%s\n' "$owners" | awk 'NF { count++ } END { print count + 0 }')
	owner_names=''
	for owner in $owners; do
		name=$(awk -F= '$1 == "DEVNAME" { print $2; exit }' "$owner" 2>/dev/null)
		owner_names="$owner_names${owner_names:+ }${name:-unnamed}"
	done
	printf '%s.partname_count=%s\n%s.partname_devices=%s\n' "$key" "$owner_count" "$key" "$owner_names"

	dev=''
	[ "$link_state" != block ] || dev=$(cat "$sys_block_path/$link_device/dev" 2>/dev/null) || dev=''
	printf '%s.dev=%s\n' "$key" "$dev"

	# Probe only an unambiguous partition, as the service does.
	blkid_exit=not-run
	blkid_type=''
	if [ "$link_state" = block ] && [ "$owner_count" -eq 1 ] && [ "$owner_names" = "$link_device" ]; then
		if [ "$blkid_impl" != util-linux ]; then
			blkid_exit=unavailable-blkid
		elif [ "$priv" = none ]; then
			blkid_exit=unavailable-privilege
		else
			blkid_exit=0
			blkid_type=$($priv blkid -c /dev/null -p -o value -s TYPE -- "$link" 2>/dev/null) || blkid_exit=$?
			blkid_type=$(printf '%s' "$blkid_type" | tr -cd 'A-Za-z0-9._-' | cut -c1-32)
		fi
	fi
	printf '%s.blkid_exit=%s\n%s.blkid_type=%s\n' "$key" "$blkid_exit" "$key" "$blkid_type"

	# Every mount of this device and every mount at its service destination.
	[ "$mountinfo_readable" = yes ] || return 0
	awk -v key="$key.mount=" -v dev="$dev" -v dest="$mount_root/$role" '
		(dev != "" && $3 == dev) || $5 == dest { print key $0 }
	' "$mountinfo_path" 2>/dev/null
}

modem_label=unresolved
case "$slot" in _a|_b) modem_label="modem$slot" ;; esac
probe_part modem_img "$modem_label"
probe_part efs efs
probe_part persist persist
probe_part modem_userdata modem_userdata
printf 'probe_complete=yes\n'
