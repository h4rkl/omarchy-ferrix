#!/bin/sh
# Sent over the CDC-ACM shell (acm-probe.py, piped to the guest `sh`) by
# s5300-cpif-inventory.sh. Issue #1 E1: read-only inventory of the Google
# Kleaf CPIF surface on the exact tegu guest before any CP boot.
#
# It NEVER opens a CPIF character device. Opening /dev/umts_boot0 or
# /dev/umts_ipc* runs the link's init_comm hook (ipc_io_device.c), so every
# /dev path below is touched only by node_meta(): shell file tests and
# stat(2) metadata. All other reads are /proc/modules, /proc/device-tree and
# sysfs attributes (of_node/compatible, driver link, modem_state, class dev
# and subsystem). No dmesg, node contents, serials, SIM or IMEI data are read.
set -u

# The offline test rewrites only these three roots to point at a fixture.
proc_root=/proc
sys_root=/sys
dev_root=/dev

model=$(tr '\000' '\n' <"$proc_root/device-tree/model" 2>/dev/null | sed -n '1p')
compatible=$(tr '\000' '\n' <"$proc_root/device-tree/compatible" 2>/dev/null)
[ "$model" = 'ZUMA PRO TEGU MP on ZUMA PRO' ] || exit 4
printf '%s\n' "$compatible" | grep -Fxq 'google,ZUMA PRO TEGU' || exit 4

clean() { tr -cd 'A-Za-z0-9._+~-' | cut -c1-48; }

printf 'probe=s5300-cpif-inventory\n'
printf 'dt_model=%s\n' "$model"
printf 'dt_compatible=google,ZUMA PRO TEGU\n'
printf 'kernel_release=%s\n' "$(uname -r | clean)"
printf 'uid=%s\n' "$(id -u)"

# Modules: /proc/modules state column (Live/Loading/Unloading), its line
# count (duplicates are malformed), and whether /sys/module has the name
# without a /proc/modules line (built in).
modules_readable=no
[ -r "$proc_root/modules" ] && modules_readable=yes
printf 'modules_readable=%s\n' "$modules_readable"
for name in cpif shm_ipc cpif_page; do
	count=0
	state=''
	if [ "$modules_readable" = yes ]; then
		count=$(awk -v m="$name" '$1 == m { n++ } END { print n + 0 }' "$proc_root/modules" 2>/dev/null)
		state=$(awk -v m="$name" '$1 == m { print $5; exit }' "$proc_root/modules" 2>/dev/null | clean)
	fi
	sysmod=no
	[ -d "$sys_root/module/$name" ] && sysmod=yes
	printf 'module.%s.count=%s\nmodule.%s.state=%s\nmodule.%s.sysfs=%s\n' \
		"$name" "${count:-0}" "$name" "$state" "$name" "$sysmod"
done

# The exact samsung,exynos-cp platform device(s), as radios.sh and
# ferrix_s5300_cpif::discover_at find them.
platform_dir="$sys_root/bus/platform/devices"
platform_readable=no
[ -d "$platform_dir" ] && platform_readable=yes
printf 'platform_readable=%s\n' "$platform_readable"
cp_count=0
cp_entry=''
for entry in "$platform_dir"/*; do
	[ -f "$entry/of_node/compatible" ] || continue
	if tr '\000' '\n' <"$entry/of_node/compatible" 2>/dev/null | grep -Fxq 'samsung,exynos-cp'; then
		cp_count=$((cp_count + 1))
		cp_entry=$entry
	fi
done
printf 'cp_platform_count=%s\n' "$cp_count"
cp_driver=''
cp_state_file=missing
cp_state=''
if [ "$cp_count" -eq 1 ]; then
	if [ -L "$cp_entry/driver" ]; then
		cp_driver=$(readlink "$cp_entry/driver" 2>/dev/null | sed 's#/*$##; s#.*/##' | clean)
		[ -n "$cp_driver" ] || cp_driver=unnamed
	fi
	if [ -f "$cp_entry/modem_state" ]; then
		cp_state_file=unreadable
		if cp_state=$(head -c 64 "$cp_entry/modem_state" 2>/dev/null); then
			cp_state_file=present
			cp_state=$(printf '%s' "$cp_state" | tr -cd 'A-Za-z0-9_' | cut -c1-32)
		else
			cp_state=''
		fi
	fi
fi
printf 'cp_platform_driver=%s\ncp_modem_state_file=%s\ncp_modem_state=%s\n' "$cp_driver" "$cp_state_file" "$cp_state"

stat_impl=missing
command -v stat >/dev/null 2>&1 && stat_impl=present
printf 'stat_impl=%s\n' "$stat_impl"

# The ONLY function that names a CPIF /dev path. File tests and stat(2) read
# inode metadata; neither calls the driver's open().
node_meta() {
	node=$1
	path="$dev_root/$node"
	link=no
	[ -L "$path" ] && link=yes
	if [ -c "$path" ]; then kind=char
	elif [ -b "$path" ]; then kind=block
	elif [ -p "$path" ]; then kind=fifo
	elif [ -e "$path" ] || [ -L "$path" ]; then kind=other
	else kind=missing
	fi
	rdev=''
	mode=''
	owner=''
	if [ "$kind" != missing ] && [ "$stat_impl" = present ]; then
		meta=$(stat -L -c '%t %T %a %u %g' "$path" 2>/dev/null) || meta=''
		set -- $meta
		if [ "$#" -eq 5 ]; then
			case "$1$2" in *[!0-9a-fA-F]*) ;; *) [ "$kind" != char ] || rdev=$(printf '%d:%d' "0x$1" "0x$2") ;; esac
			mode=$(printf '%s' "$3" | tr -cd '0-7')
			owner=$(printf '%s:%s' "$4" "$5" | tr -cd '0-9:')
		fi
	fi
	printf 'node.%s.kind=%s\nnode.%s.link=%s\nnode.%s.rdev=%s\nnode.%s.mode=%s\nnode.%s.owner=%s\n' \
		"$node" "$kind" "$node" "$link" "$node" "$rdev" "$node" "$mode" "$node" "$owner"
}

# Sysfs class entries named like the node: /sys/class/<class>/<node>/dev
# and its subsystem link. These are kernel attributes, not the device.
class_meta() {
	node=$1
	classes=''
	class_path=''
	for entry in "$sys_root"/class/*/"$node"; do
		[ -e "$entry" ] || [ -L "$entry" ] || continue
		class=${entry%/*}
		class=$(printf '%s' "${class##*/}" | clean)
		classes="$classes${classes:+ }$class"
		class_path=$entry
	done
	sysfs_dev=''
	subsystem=''
	case "$classes" in
		''|*' '*) ;;
		*)
			sysfs_dev=$(head -c 32 "$class_path/dev" 2>/dev/null | tr -cd '0-9:')
			subsystem=$(readlink "$class_path/subsystem" 2>/dev/null | sed 's#/*$##; s#.*/##' | clean)
			;;
	esac
	printf 'node.%s.classes=%s\nnode.%s.sysfs_dev=%s\nnode.%s.subsystem=%s\n' \
		"$node" "$classes" "$node" "$sysfs_dev" "$node" "$subsystem"
}

# Pinned DTS names (s5300-channels.toml): boot 241, FMT 245/246, RFS 41,
# OEM 129-136.
for node in umts_boot0 umts_ipc0 umts_ipc1 umts_rfs0 \
	oem_ipc0 oem_ipc1 oem_ipc2 oem_ipc3 oem_ipc4 oem_ipc5 oem_ipc6 oem_ipc7; do
	node_meta "$node"
	class_meta "$node"
done

# Names only (glob expansion lists the directory; nothing is opened).
oem_total=0
for entry in "$dev_root"/oem_ipc*; do
	[ -e "$entry" ] || [ -L "$entry" ] || continue
	oem_total=$((oem_total + 1))
done
printf 'oem_ipc_name_count=%s\n' "$oem_total"
wwan_devices=0
for entry in "$sys_root"/class/wwan/*; do
	[ -e "$entry" ] || [ -L "$entry" ] || continue
	wwan_devices=$((wwan_devices + 1))
done
printf 'wwan_devices=%s\n' "$wwan_devices"
rmnet_count=0
for entry in "$sys_root"/class/net/rmnet*; do
	[ -e "$entry" ] || [ -L "$entry" ] || continue
	rmnet_count=$((rmnet_count + 1))
done
printf 'rmnet_count=%s\n' "$rmnet_count"
printf 'probe_complete=yes\n'
