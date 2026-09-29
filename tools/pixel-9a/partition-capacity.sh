#!/bin/sh
set -eu

# Read-only exact-target capacity handoff for the experimental radio split
# route. In ADB mode static partitions are resolved through /dev/block/by-name
# and dynamic partitions through /dev/block/mapper; the block sysfs `size`
# file is defined in 512-byte sectors. An owner-entered Fastbootd mode can
# provide the same ten capacities through read-only `partition-size:*` getvars.
# No block node is opened or written.
#
# This is deliberately separate from partition-inventory.sh. A capacity
# handoff can be complete for the ten named targets while the broader Android
# inventory remains partial because /proc/partitions or /proc/cmdline is
# hidden. The handoff records that distinction and never authorizes a boot or
# storage operation.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/pixel-9a
expected_product=tegu
expected_revision=MP1.0
expected_sku=GXQ96
expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0
tab=$(printf '\t')

die() {
	printf 'REFUSING pixel-9a partition capacity handoff: %s\n' "$*" >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || die "missing $1; install Android Platform Tools"
}

sha256_prefix() {
	if command -v shasum >/dev/null 2>&1; then
		printf '%s' "$1" | shasum -a 256 | cut -c1-12
	else
		printf '%s' "$1" | sha256sum | cut -c1-12
	fi
}

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/partition-capacity-$stamp.txt"
handoff="$out/partition-capacities-$stamp.tsv"
latest="$out/partition-capacities-latest.tsv"
handoff_tmp="$out/.partition-capacities-latest.$$"
serial=''
cleanup() {
	rm -f "$handoff_tmp"
}
trap cleanup EXIT HUP INT TERM

{
	printf '# Pixel 9a exact-target read-only partition capacity handoff, %s UTC\n' "$stamp"
	printf '# Serial numbers and unique device identifiers are never stored here.\n'
} >"$transcript"

emit() {
	key=$1
	value=$2
	if [ -n "$serial" ]; then
		case "$value" in
			*"$serial"*) value=$(printf '%s\n' "$value" | sed "s/$serial/serial-sha256:$(sha256_prefix "$serial")/g") ;;
		esac
	fi
	printf '%s=%s\n' "$key" "$value" >>"$transcript"
}

count_rows() {
	printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

adb_prop() {
	adb -s "$serial" shell getprop "$1" </dev/null | tr -d '\r'
}

remote_read() {
	# Keep the adb exit status separate from CR stripping.  On Android, a
	# denied shell read can otherwise be hidden by the succeeding `tr` in a
	# pipeline and its diagnostic text can be mistaken for a value.
	if ! value=$(adb -s "$serial" shell "$@" </dev/null 2>/dev/null); then
		return 1
	fi
	value=$(printf '%s\n' "$value" | tr -d '\r')
	[ -n "$value" ] || return 1
	printf '%s\n' "$value"
}

valid_block_name() {
	case "$1" in
		''|*[!A-Za-z0-9._-]*) return 1 ;;
		*) return 0 ;;
	esac
}

# Resolve a symlink without accepting a path outside the Android block
# namespace. readlink is a read-only lookup; ls is the toybox fallback.
resolve_link() {
	path=$1
	link=''
	if link=$(remote_read readlink -f "$path" 2>/dev/null | awk 'NF { print $1; exit }'); then
		:
	else
		link=''
	fi
	case "$link" in
		/dev/block/*) ;;
		*)
			link=$(remote_read ls -l "$path" 2>/dev/null | awk '/ -> / { print $NF; exit }' || true)
			case "$link" in
				/dev/block/*) ;;
				*) return 1 ;;
			esac
			;;
	esac
	block=${link##*/}
	valid_block_name "$block" || return 1
	printf '%s\n' "$block"
}

capacity_for_block() {
	block=$1
	label=$2
	raw_sectors=$(remote_read cat "/sys/class/block/$block/size") ||
		die "$label capacity is not readable from /sys/class/block/$block/size"
	sectors=$(printf '%s\n' "$raw_sectors" | awk 'NF { print $1; exit }')
	case "$sectors" in
		''|*[!0-9]*) die "$label has a malformed sysfs sector count" ;;
	esac
	[ "$sectors" -gt 0 ] || die "$label has a zero sysfs sector count"
	bytes=$(awk -v sectors="$sectors" 'BEGIN { printf "%.0f\n", sectors * 512 }')
	case "$bytes" in
		''|*[!0-9]*) die "$label has a malformed byte capacity" ;;
	esac
	[ "$bytes" -gt 0 ] || die "$label has a zero byte capacity"
	[ $((bytes % 4096)) -eq 0 ] || die "$label capacity is not 4096-byte aligned"
	logical=$(remote_read cat "/sys/class/block/$block/queue/logical_block_size" 2>/dev/null | awk 'NF { print $1; exit }' || true)
	case "$logical" in
		''|*[!0-9]*) logical=unavailable ;;
	esac
	capacity_block=$block
	capacity_sectors=$sectors
	capacity_bytes=$bytes
	capacity_logical_block=$logical
}

# Fastboot reports partition-size values in bytes, normally as hexadecimal
# strings.  Some bootloader/fastbootd implementations emit decimal instead,
# so accept both forms but reject signs, exponents, separators, overflow, and
# zero.  The 2^53 bound keeps the POSIX awk conversion exact; Pixel partition
# sizes are far below it and the handoff must never silently round a capacity.
parse_fastboot_capacity() {
	raw=$1
	case "$raw" in
		0[xX]*)
			digits=${raw#??}
			printf '%s\n' "$digits" | awk '
				/^[0-9A-Fa-f]+$/ {
					value = 0
					for (i = 1; i <= length($0); i++) {
						c = substr($0, i, 1)
						if (c >= "0" && c <= "9") n = c + 0
						else if (c >= "a" && c <= "f") n = index("abcdef", c) + 9
						else n = index("ABCDEF", c) + 9
						value = value * 16 + n
						if (value > 9007199254740991) exit 1
					}
					if (value > 0) { printf "%.0f\n", value; ok = 1 }
				}
				END { if (!ok) exit 1 }
			'
			;;
		''|*[!0-9]*) return 1 ;;
		*)
			printf '%s\n' "$raw" | awk '
				/^[0-9]+$/ {
					value = $0 + 0
					if (value > 0 && value <= 9007199254740991) {
						printf "%.0f\n", value
						ok = 1
					}
				}
				END { if (!ok) exit 1 }
			'
			;;
	esac
}

# Fastboot writes getvar responses to stderr on common platform-tools
# versions.  Match only a response whose key starts at the beginning of the
# normalized line, so `partition-size:foo` cannot accidentally match
# `partition-size:foobar`.  Callers pass only the fixed allow-listed variable
# names below; this function is read-only and never invokes a mutation verb.
fastboot_getvar() {
	variable=$1
	output=$(fastboot -s "$serial" getvar "$variable" 2>&1) || return 1
	printf '%s\n' "$output" | awk -v key="$variable" '
		{
			line = $0
			sub(/^[[:space:]]*/, "", line)
			sub(/^\(bootloader\)[[:space:]]*/, "", line)
			prefix = key ":"
			if (index(line, prefix) == 1) {
				value = substr(line, length(prefix) + 1)
				sub(/^[[:space:]]*/, "", value)
				sub(/[[:space:]]*$/, "", value)
				if (value != "") { print value; found = 1; exit }
			}
		}
		END { if (!found) exit 1 }
	'
}

fastboot_capacity_for_partition() {
	partition=$1
	active_name="partition-size:${partition}${active_suffix}"
	inactive_name="partition-size:${partition}${inactive_suffix}"
	active_raw=$(fastboot_getvar "$active_name") ||
		die "$active_name is not reported by Fastboot; dynamic partitions require fastbootd and independent A/B getvar rows"
	inactive_raw=$(fastboot_getvar "$inactive_name") ||
		die "$inactive_name is not reported by Fastboot; dynamic partitions require fastbootd and independent A/B getvar rows"
	active_bytes=$(parse_fastboot_capacity "$active_raw") ||
		die "$active_name has an invalid hex/decimal byte capacity: $active_raw"
	inactive_bytes=$(parse_fastboot_capacity "$inactive_raw") ||
		die "$inactive_name has an invalid hex/decimal byte capacity: $inactive_raw"
	[ "$active_bytes" = "$inactive_bytes" ] ||
		die "$partition A/B capacities differ ($active_bytes vs $inactive_bytes)"
	[ $((active_bytes % 4096)) -eq 0 ] ||
		die "$partition capacity is not 4096-byte aligned: $active_bytes"
	fastboot_active_raw=$active_raw
	fastboot_inactive_raw=$inactive_raw
	fastboot_capacity_bytes=$active_bytes
}

resolve_dynamic_block() {
	partition=$1
	suffix=$2
	for candidate in "/dev/block/mapper/$partition$suffix" "/dev/block/mapper/$partition"; do
		# The unsuffixed mapper alias, when present, describes the active
		# logical partition only; never use it for the inactive suffix.
		if [ "$candidate" = "/dev/block/mapper/$partition" ] && [ "$suffix" != "$active_suffix" ]; then
			continue
		fi
		if block=$(resolve_link "$candidate"); then
			printf '%s\n' "$block"
			return 0
		fi
	done
	return 1
}

write_partition_row() {
	partition=$1
	bytes=$2
	printf 'partition%s%s%s%s\n' "$tab" "$partition" "$tab" "$bytes" >>"$handoff_tmp"
}

need awk
need cut
need date
need mkdir
need rm
need sed
need tr
if command -v shasum >/dev/null 2>&1; then :; else need sha256sum; fi

if command -v adb >/dev/null 2>&1; then
	adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
else
	adb_rows=''
fi
if command -v fastboot >/dev/null 2>&1; then
	fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
else
	fastboot_rows=''
fi
adb_count=$(count_rows "$adb_rows")
fastboot_count=$(count_rows "$fastboot_rows")

if [ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ]; then
	# Fastbootd exposes logical A/B partition metadata even when Android's
	# inactive mapper nodes are absent.  This branch remains entirely read-only:
	# it never reboots into fastbootd, changes slots, or opens a block device.
	need fastboot
	serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
	product=$(fastboot_getvar product) || die 'Fastboot product getvar is unavailable'
	revision=$(fastboot_getvar hw-revision) || die 'Fastboot hw-revision getvar is unavailable'
	sku=$(fastboot_getvar variant) || die 'Fastboot variant getvar is unavailable'
	slot=$(fastboot_getvar current-slot) || die 'Fastboot current-slot getvar is unavailable'
	slot_count=$(fastboot_getvar slot-count) || die 'Fastboot slot-count getvar is unavailable'
	userspace=$(fastboot_getvar is-userspace) || die 'Fastboot is-userspace getvar is unavailable'
	[ "$product" = "$expected_product" ] || die "product mismatch: $product"
	[ "$revision" = "$expected_revision" ] || die "hardware revision mismatch: $revision"
	[ "$sku" = "$expected_sku" ] || die "SKU/variant mismatch: $sku"
	[ "$slot_count" = 2 ] || die "unexpected slot count: $slot_count"
	case "$slot" in
		a|_a) active_suffix=_a; inactive_suffix=_b ;;
		b|_b) active_suffix=_b; inactive_suffix=_a ;;
		*) die "unexpected current slot: $slot" ;;
	esac
	[ "$userspace" = yes ] ||
		die 'Fastboot capacity handoff requires fastbootd (is-userspace=yes) so both dynamic A/B logical partitions are independently observable; no reboot was issued'

	emit target_id "$expected_target_id"
	emit product "$product"
	emit hardware_revision "$revision"
	emit sku "$sku"
	emit transport fastboot
	emit current_slot "$slot"
	emit slot_count "$slot_count"
	emit fastboot_userspace "$userspace"
	emit slot_scope both-a-and-b
	emit inventory_completeness partial
	emit capacity_source fastboot-getvar-partition-size
	emit capacity_unit bytes
	emit handoff_status exact-capacities-observed

	{
		printf '# source=Fastboot getvar partition-size:<partition>_<slot>; values are bytes (hex or decimal)\n'
		printf '# inventory_completeness=partial (Fastboot exposes no guest /proc inventory; this handoff never upgrades that gate)\n'
		printf '# exact-target=%s product=%s revision=%s sku=%s current_slot=%s is_userspace=%s\n' "$expected_target_id" "$product" "$revision" "$sku" "$slot" "$userspace"
		printf 'schema_version%s1\n' "$tab"
		printf 'target_id%s%s\n' "$tab" "$expected_target_id"
	} >"$handoff_tmp"

	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta vbmeta_vendor vbmeta_system vendor_dlkm system_dlkm; do
		fastboot_capacity_for_partition "$partition"
		write_partition_row "$partition" "$fastboot_capacity_bytes"
		emit "${partition}_active_capacity_bytes" "$fastboot_capacity_bytes"
		emit "${partition}_active_raw" "$fastboot_active_raw"
		emit "${partition}_inactive_raw" "$fastboot_inactive_raw"
	done

	mv "$handoff_tmp" "$handoff"
	cp "$handoff" "$latest"
	trap - EXIT HUP INT TERM
	printf 'capacity_handoff=%s\n' "$handoff"
	printf 'capacity_handoff_latest=%s\n' "$latest"
	printf 'inventory_completeness=partial\n'
	printf 'transcript=%s\n' "$transcript" >&2
	exit 0
fi

[ "$adb_count" -eq 1 ] && [ "$fastboot_count" -eq 0 ] ||
	die "capacity handoff requires exactly one ADB or one Fastboot device (adb=$adb_count fastboot=$fastboot_count)"

need adb

serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
[ "$state" = device ] || die "ADB state is '$state', not device"
product=$(adb_prop ro.product.device)
revision=$(adb_prop ro.boot.hardware.revision)
sku=$(adb_prop ro.boot.hardware.sku)
slot=$(adb_prop ro.boot.slot_suffix)
[ "$product" = "$expected_product" ] || die "product mismatch: $product"
[ "$revision" = "$expected_revision" ] || die "hardware revision mismatch: $revision"
[ "$sku" = "$expected_sku" ] || die "SKU mismatch: $sku"
case "$slot" in
	_a) active_suffix=_a; inactive_suffix=_b ;;
	_b) active_suffix=_b; inactive_suffix=_a ;;
	*) die "unexpected current slot: $slot" ;;
esac

partition_names=$(remote_read ls -1 /dev/block/by-name || true)
printf '%s\n' "$partition_names" | awk '$0 ~ /(_a|\.a)$/ { a=1 } $0 ~ /(_b|\.b)$/ { b=1 } END { if (!a || !b) exit 1 }' ||
	die 'read-only /dev/block/by-name map does not expose both A/B suffixes'

# Retain the broader inventory status independently. A denied proc file is
# expected on stock Android and must not be converted into a complete claim.
proc_partitions=$(remote_read cat /proc/partitions || true)
proc_mounts=$(remote_read cat /proc/mounts || true)
proc_cmdline=$(remote_read cat /proc/cmdline || true)
inventory_completeness=complete
[ -n "$proc_partitions" ] && [ -n "$proc_mounts" ] && [ -n "$proc_cmdline" ] || inventory_completeness=partial

emit target_id "$expected_target_id"
emit product "$product"
emit hardware_revision "$revision"
emit sku "$sku"
emit transport adb
emit current_slot "$slot"
emit slot_scope both-a-and-b
emit inventory_completeness "$inventory_completeness"
emit capacity_source 'sysfs-size-512-byte-sectors'
emit capacity_unit bytes
emit handoff_status exact-capacities-observed

{
	printf '# source=sysfs:/sys/class/block/*/size; each value is 512-byte sectors\n'
	printf '# inventory_completeness=%s (this handoff never upgrades the broader inventory gate)\n' "$inventory_completeness"
	printf '# exact-target=%s product=%s revision=%s sku=%s current_slot=%s\n' "$expected_target_id" "$product" "$revision" "$sku" "$slot"
	printf 'schema_version%s1\n' "$tab"
	printf 'target_id%s%s\n' "$tab" "$expected_target_id"
} >"$handoff_tmp"

for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta vbmeta_vendor vbmeta_system; do
	active_block=$(resolve_link "/dev/block/by-name/${partition}${active_suffix}") ||
		die "$partition${active_suffix} is not resolvable through /dev/block/by-name"
	inactive_block=$(resolve_link "/dev/block/by-name/${partition}${inactive_suffix}") ||
		die "$partition${inactive_suffix} is not resolvable through /dev/block/by-name"
	capacity_for_block "$active_block" "$partition${active_suffix}"
	active_bytes=$capacity_bytes
	active_sectors=$capacity_sectors
	active_logical=$capacity_logical_block
	capacity_for_block "$inactive_block" "$partition${inactive_suffix}"
	[ "$active_bytes" = "$capacity_bytes" ] ||
		die "$partition A/B capacities differ ($active_bytes vs $capacity_bytes)"
	write_partition_row "$partition" "$active_bytes"
	emit "${partition}_active_block" "$active_block"
	emit "${partition}_inactive_block" "$inactive_block"
	emit "${partition}_sectors_512" "$active_sectors"
	emit "${partition}_logical_block_size" "$active_logical"
done

# Dynamic partitions are not listed in /dev/block/by-name on this device.
# Require a mapper node for both slots; an active mount alone is insufficient
# evidence for a handoff that may be used against the inactive slot.
for partition in vendor_dlkm system_dlkm; do
	active_block=$(resolve_dynamic_block "$partition" "$active_suffix") ||
		die "$partition${active_suffix} is not resolvable through /dev/block/mapper"
	inactive_block=$(resolve_dynamic_block "$partition" "$inactive_suffix") ||
		die "$partition${inactive_suffix} is not independently resolvable through /dev/block/mapper"
	capacity_for_block "$active_block" "$partition${active_suffix}"
	active_bytes=$capacity_bytes
	active_sectors=$capacity_sectors
	active_logical=$capacity_logical_block
	capacity_for_block "$inactive_block" "$partition${inactive_suffix}"
	[ "$active_bytes" = "$capacity_bytes" ] ||
		die "$partition A/B capacities differ ($active_bytes vs $capacity_bytes)"
	write_partition_row "$partition" "$active_bytes"
	emit "${partition}_active_block" "$active_block"
	emit "${partition}_inactive_block" "$inactive_block"
	emit "${partition}_sectors_512" "$active_sectors"
	emit "${partition}_logical_block_size" "$active_logical"
done

mv "$handoff_tmp" "$handoff"
cp "$handoff" "$latest"
trap - EXIT HUP INT TERM
printf 'capacity_handoff=%s\n' "$handoff"
printf 'capacity_handoff_latest=%s\n' "$latest"
printf 'inventory_completeness=%s\n' "$inventory_completeness"
printf 'transcript=%s\n' "$transcript" >&2
