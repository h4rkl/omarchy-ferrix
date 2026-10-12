#!/bin/sh
# Offline contract tests for the exact Pixel 9a ferrix-s5300-mounts service
# (issue #34). The service is sourced with OpenRC helpers stubbed and its /dev,
# /sys, /proc, and mount-root paths redirected into a fixture; blkid, blockdev,
# losetup, mount, and umount are PATH stubs that model the sysfs state the
# kernel would show. No device, loop, or mount is touched. Passing this is not
# physical evidence: the read-only loop barrier on the real tegu kernel still
# needs the physical preflight and an owner-reviewed mount proof run.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
package_dir="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
initd="$package_dir/ferrix-s5300-mounts.initd"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-s5300-mounts.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

sha512_file() {
	if command -v sha512sum >/dev/null 2>&1; then
		sha512sum "$1" | awk '{print $1}'
	else
		shasum -a 512 "$1" | awk '{print $1}'
	fi
}

sh -n "$initd"

# --- static contract -----------------------------------------------------------
code=$(grep -v '^[[:space:]]*#' "$initd")
for package in blkid util-linux-misc losetup; do
	grep -Eq "^[[:space:]]*$package[[:space:]]*$" "$package_dir/APKBUILD" ||
		fail "device package must depend on $package (util-linux blkid, blockdev, losetup) for the S5300 mounts"
done
# Alpine has no `blockdev` package; /sbin/blockdev ships in util-linux-misc.
if grep -Eq '^[[:space:]]*blockdev[[:space:]]*$' "$package_dir/APKBUILD"; then
	fail 'device package depends on a non-existent blockdev package'
fi
[ "$(awk '$2 == "ferrix-s5300-mounts.initd" { print $1 }' "$package_dir/APKBUILD")" = "$(sha512_file "$initd")" ] ||
	fail 'APKBUILD sha512 for ferrix-s5300-mounts.initd is stale'
if printf '%s\n' "$code" | grep -Eq 'mountpoint -q|/proc/mounts|-o "?ro,nosuid'; then
	fail 'S5300 mounts still use plain ro or /proc/mounts read-only detection'
fi
[ "$(printf '%s\n' "$code" | grep -cE '(^|[;&|{(]|then|do)[[:space:]]*mount[[:space:]]')" -eq 1 ] ||
	fail 'S5300 mounts must issue exactly one mount call'
printf '%s\n' "$code" | grep -Fq 'mount -t "$fstype" -o "ro,$(recovery_option "$fstype"),nosuid,nodev,noexec" -- "$dev_root/$attached" "$destination"' ||
	fail 'the only S5300 mount call must mount the attached loop, never the partition'
[ "$(printf '%s\n' "$code" | grep -oE '(^|[;&|{(]|then|do)[[:space:]]*losetup [^ ]+' | sed 's/.*losetup /losetup /' |
	grep -cvE '^losetup (-V|--read-only|-d)$')" -eq 0 ] ||
	fail 'S5300 mounts call losetup with something other than -V, --read-only, or -d'
printf '%s\n' "$code" | grep -Eq '^[[:space:]]*blockdev --setro "\$device"' ||
	fail 'S5300 mounts must set BLKROSET on the source partition'
if printf '%s\n' "$code" | grep -Eq 'setrw|--setro[^"]*0|losetup -[a-z]*f|--find|autoclear'; then
	fail 'S5300 mounts clear BLKROSET or let losetup pick an unverified loop'
fi
awk '/^stop\(\)/ { inside = 1 } inside && /verify_own_mount "\$destination"/ { if (!v) v = NR } inside && /umount --/ { u = NR }
	inside && /detach_loop/ { d = NR } inside && /^}/ { inside = 0 }
	END { exit !(v && u && d && v < u && u < d) }' "$initd" ||
	fail 'S5300 mounts stop must verify its own loop mount before any unmount or detach'
awk '/^mount_one\(\)/ { inside = 1 } inside && /blockdev --setro/ { s = NR } inside && /sys_value "\$part_name\/ro"\)" = 1/ { r = NR }
	inside && /losetup --read-only/ { l = NR } inside && /verify_loop "\$attached"/ { v = NR } inside && /^[[:space:]]*mount -t/ { m = NR }
	inside && /^}/ { inside = 0 }
	END { exit !(s && r && l && v && m && s < r && r < l && l < v && v < m) }' "$initd" ||
	fail 'S5300 mounts must order BLKROSET, its check, the read-only attach, loop verification, then the mount'
grep -Eq '^RADIO_ROUTE_SERVICES = \("ferrix-s5300-mounts",' "$root/tools/pixel-9a/dev-compose.py" ||
	fail 'the dev composer must keep ferrix-s5300-mounts out of the dev runlevel'

# --- stub harness --------------------------------------------------------------
fx="$work/fx"
bin="$work/bin"
script="$work/ferrix-s5300-mounts.initd"
mkdir -p "$fx" "$bin"
fx=$(CDPATH= cd -- "$fx" && pwd -P)
sed -e "s#^mount_root=.*#mount_root=$fx/mnt/vendor#" \
	-e "s#^dev_root=.*#dev_root=$fx/dev#" \
	-e "s#^partlabel_root=.*#partlabel_root=$fx/dev/disk/by-partlabel#" \
	-e "s#^sys_block_path=.*#sys_block_path=$fx/sys#" \
	-e "s#^mountinfo_path=.*#mountinfo_path=$fx/mountinfo#" \
	-e "s#^bootconfig_path=.*#bootconfig_path=$fx/bootconfig#" \
	-e "s#^cmdline_path=.*#cmdline_path=$fx/cmdline#" \
	"$initd" >"$script"
[ "$(grep -c "$fx" "$script")" -eq 7 ] || fail 'S5300 mount harness could not redirect every host path'

cat >"$bin/blkid" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = -V ] && { printf '%s\n' "${FAKE_BLKID_VERSION-blkid from util-linux 2.40.2  (libblkid 2.40.2)}"; exit 0; }
printf 'blkid %s\n' "$*" >>"$FAKE_LOG"
for device; do :; done
name=${device##*/}
case "$name" in
	loop*)
		eval "override=\${FAKE_TYPE_$name+set}"
		if [ -z "$override" ]; then
			backing=$(cat "$FAKE_SYS/$name/loop/backing_file" 2>/dev/null) || exit 2
			name=${backing##*/}
		fi
		;;
esac
eval "type=\${FAKE_TYPE_$name-ext4}"
[ "$type" = ambivalent ] && exit 8
[ -n "$type" ] || exit 2
printf '%s\n' "$type"
EOF
cat >"$bin/blockdev" <<'EOF'
#!/bin/sh
set -eu
printf 'blockdev %s\n' "$*" >>"$FAKE_LOG"
[ "$#" -eq 2 ] && [ "$1" = --setro ] || exit 64
[ -z "${FAKE_SETRO_FAIL-}" ] || exit 1
[ -n "${FAKE_SETRO_NOOP-}" ] || echo 1 >"$FAKE_SYS/${2##*/}/ro"
if [ -n "${FAKE_STEAL_LOOP-}" ]; then
	mkdir -p "$FAKE_SYS/$FAKE_STEAL_LOOP/loop"
	echo /dev/other >"$FAKE_SYS/$FAKE_STEAL_LOOP/loop/backing_file"
	echo 8 >"$FAKE_SYS/$FAKE_STEAL_LOOP/size"
fi
EOF
cat >"$bin/losetup" <<'EOF'
#!/bin/sh
set -eu
[ "$*" = -V ] && { printf '%s\n' "${FAKE_LOSETUP_VERSION-losetup from util-linux 2.40.2}"; exit 0; }
printf 'losetup %s\n' "$*" >>"$FAKE_LOG"
case "$#:$1" in
	2:-d)
		[ -z "${FAKE_DETACH_FAIL-}" ] || exit 1
		name=${2##*/}
		rm -rf "$FAKE_SYS/$name/loop"
		echo 0 >"$FAKE_SYS/$name/size"
		# Like the kernel, detaching leaves the disk read-only flag alone.
		;;
	4:--read-only)
		[ "$2" = -- ] || exit 64
		name=${3##*/}
		part=${4##*/}
		[ -z "${FAKE_LOSETUP_FAIL-}" ] || exit 1
		[ ! -e "$FAKE_SYS/$name/loop" ] || exit 1
		mkdir "$FAKE_SYS/$name/loop"
		echo "${FAKE_LOOP_BACKING-$4}" >"$FAKE_SYS/$name/loop/backing_file"
		echo "${FAKE_LOOP_OFFSET-0}" >"$FAKE_SYS/$name/loop/offset"
		echo 0 >"$FAKE_SYS/$name/loop/sizelimit"
		echo "${FAKE_LOOP_SIZE-$(cat "$FAKE_SYS/$part/size")}" >"$FAKE_SYS/$name/size"
		echo "${FAKE_LOOP_RO-1}" >"$FAKE_SYS/$name/ro"
		[ -z "${FAKE_LOOP_DEV-}" ] || echo "$FAKE_LOOP_DEV" >"$FAKE_SYS/$name/dev"
		;;
	*) exit 64 ;;
esac
EOF
cat >"$bin/mount" <<'EOF'
#!/bin/sh
set -eu
printf 'mount %s\n' "$*" >>"$FAKE_LOG"
[ -z "${FAKE_MOUNT_FAIL-}" ] || exit 32
type= options=
while [ "$#" -gt 0 ]; do
	case "$1" in
		-t) type=$2; shift 2 ;;
		-o) options=$2; shift 2 ;;
		--) shift; break ;;
		*) break ;;
	esac
done
case ",$options," in *,ro,*) ro=ro ;; *) ro=rw ;; esac
super=$ro
if [ -z "${FAKE_HIDE_RECOVERY-}" ]; then
	case ",$options," in
		*,noload,*) super=$ro,noload ;;
		*,norecovery,*) super=$ro,norecovery ;;
	esac
fi
dev=$(cat "$FAKE_SYS/${1##*/}/dev")
printf '99 1 %s / %s %s,nosuid,nodev,noexec - %s %s %s\n' "$dev" "$2" "$ro" "$type" "$1" "$super" >>"$FAKE_MOUNTINFO"
EOF
cat >"$bin/umount" <<'EOF'
#!/bin/sh
set -eu
printf 'umount %s\n' "$*" >>"$FAKE_LOG"
[ -z "${FAKE_UMOUNT_FAIL-}" ] || exit 32
for target; do :; done
awk -v target="$target" '$5 != target' "$FAKE_MOUNTINFO" >"$FAKE_MOUNTINFO.next"
mv "$FAKE_MOUNTINFO.next" "$FAKE_MOUNTINFO"
EOF
chmod 0755 "$bin/blkid" "$bin/blockdev" "$bin/losetup" "$bin/mount" "$bin/umount"

add_partition() {
	# label devname minor
	: >"$fx/dev/$2"
	mkdir -p "$fx/sys/$2"
	printf 'MAJOR=8\nMINOR=%s\nDEVNAME=%s\nDEVTYPE=partition\nPARTNAME=%s\n' "$3" "$2" "$1" >"$fx/sys/$2/uevent"
	echo "8:$3" >"$fx/sys/$2/dev"
	echo "$(($3 * 1024))" >"$fx/sys/$2/size"
	echo 0 >"$fx/sys/$2/ro"
}
add_loop() {
	: >"$fx/dev/loop$1"
	mkdir -p "$fx/sys/loop$1"
	echo "7:$1" >"$fx/sys/loop$1/dev"
	echo 0 >"$fx/sys/loop$1/size"
	echo 0 >"$fx/sys/loop$1/ro"
}
fixture() {
	rm -rf "$fx"
	mkdir -p "$fx/dev/disk/by-partlabel" "$fx/mnt/vendor" "$fx/sys"
	for spec in modem_a:sda10:10 modem_b:sda11:11 efs:sda12:12 persist:sda13:13 modem_userdata:sda14:14; do
		label=${spec%%:*}
		rest=${spec#*:}
		add_partition "$label" "${rest%%:*}" "${rest#*:}"
		ln -s "../../${rest%%:*}" "$fx/dev/disk/by-partlabel/$label"
	done
	for n in 0 1 2 3 4; do add_loop "$n"; done
	printf 'androidboot.slot_suffix = "_b"\n' >"$fx/bootconfig"
	: >"$fx/cmdline"
	printf '1 0 8:0 / / rw - ext4 /dev/root rw\n' >"$fx/mountinfo"
	: >"$fx/log"
}
# Bind loopN read-only over a partition and mount it at a destination, as a
# previous start would have: loop devname destination fstype mount-opts super-opts
premount_loop() {
	mkdir -p "$fx/sys/loop$1/loop"
	echo "$fx/dev/$2" >"$fx/sys/loop$1/loop/backing_file"
	echo 0 >"$fx/sys/loop$1/loop/offset"
	echo 0 >"$fx/sys/loop$1/loop/sizelimit"
	cat "$fx/sys/$2/size" >"$fx/sys/loop$1/size"
	echo 1 >"$fx/sys/loop$1/ro"
	echo 1 >"$fx/sys/$2/ro"
	printf '50 1 7:%s / %s %s - %s %s %s\n' "$1" "$fx/mnt/vendor/$3" "$5" "$4" "$fx/dev/loop$1" "$6" >>"$fx/mountinfo"
}
# A direct partition mount: destination devname fstype mount-opts super-opts [root]
premount_partition() {
	printf '50 1 %s %s %s %s - %s %s %s\n' "$(cat "$fx/sys/$2/dev")" "${6:-/}" "$1" "$4" "$3" "$fx/dev/$2" "$5" >>"$fx/mountinfo"
}
run() {
	(
		set +eu
		FAKE_LOG="$fx/log"
		FAKE_MOUNTINFO="$fx/mountinfo"
		FAKE_SYS="$fx/sys"
		export FAKE_LOG FAKE_MOUNTINFO FAKE_SYS
		PATH="$bin:$PATH"
		. "$script"
		ebegin() { :; }
		eend() { return "${1:-0}"; }
		eerror() { printf '%s\n' "$*" >&2; }
		checkpath() { mkdir -p "$4"; }
		require_block_device() { [ -f "$1" ]; }
		"$1"
	) >"$fx/out" 2>"$fx/err"
}
refuses() {
	if run start; then
		fail "S5300 mounts accepted $1"
	fi
	grep -q -- "$2" "$fx/err" || { cat "$fx/err" >&2; fail "S5300 mounts refusal reason: $1"; }
	no_partition_mount
}
count() {
	grep -c -- "$1" "$fx/log" || true
}
# The core contract: no partition node is ever handed to mount(8).
no_partition_mount() {
	if grep -Eq "^mount .* -- $fx/dev/sda[0-9]+ " "$fx/log"; then
		fail 'S5300 mounts mounted a partition directly instead of its read-only loop'
	fi
}
not_mounted() {
	if grep -q "^mount .* $fx/mnt/vendor/$1\$" "$fx/log"; then
		fail "S5300 mounts issued a mount at $1 after refusing it"
	fi
}
loop_bound() {
	[ -e "$fx/sys/$1/loop/backing_file" ]
}
no_state_change() {
	[ "$(count '^blockdev ')" -eq 0 ] && [ "$(count '^losetup ')" -eq 0 ] && [ "$(count '^mount ')" -eq 0 ] ||
		fail "S5300 mounts changed block-device state after refusing $1"
}

# --- clean start and stop ------------------------------------------------------
fixture
run start || { cat "$fx/err" >&2; fail 'S5300 mounts refused four clean ext4 partitions'; }
n=0
for pair in modem_img:sda11 efs:sda12 persist:sda13 modem_userdata:sda14; do
	dest=${pair%%:*} part=${pair#*:} loop=loop$n
	grep -Fqx "blkid -c /dev/null -p -o value -s TYPE -- $fx/dev/$part" "$fx/log" ||
		fail "S5300 mounts did not low-level probe $dest before mounting"
	s=$(grep -Fnx "blockdev --setro $fx/dev/$part" "$fx/log" | cut -d: -f1)
	l=$(grep -Fnx "losetup --read-only -- $fx/dev/$loop $fx/dev/$part" "$fx/log" | cut -d: -f1)
	b=$(grep -Fnx "blkid -c /dev/null -p -o value -s TYPE -- $fx/dev/$loop" "$fx/log" | head -n1 | cut -d: -f1)
	m=$(grep -Fnx "mount -t ext4 -o ro,noload,nosuid,nodev,noexec -- $fx/dev/$loop $fx/mnt/vendor/$dest" "$fx/log" | cut -d: -f1)
	[ -n "$s" ] && [ -n "$l" ] && [ -n "$b" ] && [ -n "$m" ] && [ "$s" -lt "$l" ] && [ "$l" -lt "$b" ] && [ "$b" -lt "$m" ] ||
		fail "S5300 mounts did not set BLKROSET, attach $loop read-only, verify it, then mount it for $dest"
	[ "$(cat "$fx/sys/$part/ro")" = 1 ] || fail "S5300 mounts left $part writable"
	grep -Eq "^99 1 7:$n / $fx/mnt/vendor/$dest " "$fx/mountinfo" || fail "S5300 mounts did not mount $dest from $loop"
	n=$((n + 1))
done
[ "$(count '^mount ')" -eq 4 ] || fail 'S5300 mounts issued an unexpected mount'
[ "$(count '^losetup --read-only ')" -eq 4 ] || fail 'S5300 mounts attached an unexpected loop'
[ "$(count '^losetup -d ')" -eq 0 ] || fail 'S5300 mounts detached a loop during a clean start'
[ "$(cat "$fx/sys/sda10/ro")" = 0 ] || fail 'S5300 mounts touched the inactive-slot modem partition'
if grep -q "sda10" "$fx/log"; then fail 'S5300 mounts used the inactive-slot modem partition'; fi
no_partition_mount
run start || { cat "$fx/err" >&2; fail 'S5300 mounts refused its own verified mounts on a second start'; }
[ "$(count '^mount ')" -eq 4 ] && [ "$(count '^losetup --read-only ')" -eq 4 ] ||
	fail 'S5300 mounts re-mounted or re-attached already verified sources'
run stop || { cat "$fx/err" >&2; fail 'S5300 mounts could not stop'; }
[ "$(count '^umount -- ')" -eq 4 ] || fail 'S5300 mounts stop did not unmount exactly four sources'
for n in 0 1 2 3; do
	grep -Fqx "losetup -d $fx/dev/loop$n" "$fx/log" || fail "S5300 mounts stop did not detach loop$n"
	! loop_bound "loop$n" || fail "S5300 mounts stop left loop$n bound"
done
[ "$(wc -l <"$fx/mountinfo" | tr -d '[:space:]')" -eq 1 ] || fail 'S5300 mounts stop left a mount behind'
for part in sda11 sda12 sda13 sda14; do
	[ "$(cat "$fx/sys/$part/ro")" = 1 ] || fail "S5300 mounts stop cleared BLKROSET on $part"
done
# Detached loops keep ro=1, so a restart must not reuse them: only loop4 is
# left, the modem image takes it, and efs fails closed.
: >"$fx/log"
refuses 'a restart with every remaining loop left read-only' 'no unbound loop device with ro=0 is available for efs'
grep -Fqx "losetup --read-only -- $fx/dev/loop4 $fx/dev/sda11" "$fx/log" || fail 'S5300 mounts restart did not use the one clean loop'
[ "$(count '^losetup --read-only ')" -eq 1 ] || fail 'S5300 mounts reused a stale read-only loop'

fixture
export FAKE_TYPE_sda12=f2fs
run start || { cat "$fx/err" >&2; fail 'S5300 mounts refused an f2fs efs partition'; }
unset FAKE_TYPE_sda12
grep -Fqx "mount -t f2fs -o ro,norecovery,nosuid,nodev,noexec -- $fx/dev/loop1 $fx/mnt/vendor/efs" "$fx/log" ||
	fail 'S5300 mounts did not mount the f2fs loop with ro,norecovery'
no_partition_mount

fixture
rm "$fx/bootconfig"
printf 'console=ttySAC0 androidboot.slot_suffix=_a\n' >"$fx/cmdline"
run start || { cat "$fx/err" >&2; fail 'S5300 mounts refused a command-line slot'; }
grep -Fqx "losetup --read-only -- $fx/dev/loop0 $fx/dev/sda10" "$fx/log" || fail 'S5300 mounts ignored the command-line slot'

# --- refusals before any block-device state change -------------------------------
fixture
export FAKE_TYPE_sda13=vfat
refuses 'an unrecognised filesystem type' "refusing unrecognised filesystem 'vfat'"
unset FAKE_TYPE_sda13
not_mounted persist
if grep sda13 "$fx/log" | grep -vq '^blkid '; then fail 'S5300 mounts changed an unrecognised partition'; fi
[ "$(cat "$fx/sys/sda13/ro")" = 0 ] || fail 'S5300 mounts set BLKROSET on a refused partition'
fixture
export FAKE_TYPE_sda13=ambivalent
refuses 'an ambivalent superblock probe' "refusing unrecognised filesystem 'none'"
unset FAKE_TYPE_sda13
not_mounted persist
fixture
export FAKE_TYPE_sda11=
refuses 'a modem image without a filesystem signature' "refusing unrecognised filesystem 'none'"
unset FAKE_TYPE_sda11
no_state_change 'a modem image without a signature'

fixture
export FAKE_BLKID_VERSION='BusyBox v1.37.0 multi-call binary.'
refuses 'a BusyBox blkid probe' 'util-linux blkid is required'
unset FAKE_BLKID_VERSION
no_state_change 'a BusyBox blkid'
fixture
export FAKE_LOSETUP_VERSION='BusyBox v1.37.0 multi-call binary.'
refuses 'a BusyBox losetup' 'util-linux losetup is required'
unset FAKE_LOSETUP_VERSION
no_state_change 'a BusyBox losetup'

fixture
for n in 0 1 2 3 4; do echo 1 >"$fx/sys/loop$n/ro"; done
refuses 'only loops left read-only by an earlier user' 'no unbound loop device with ro=0'
no_state_change 'stale read-only loops'
fixture
for n in 0 1 2 3 4; do mkdir "$fx/sys/loop$n/loop"; : >"$fx/sys/loop$n/loop/backing_file"; done
refuses 'only bound loops' 'no unbound loop device with ro=0'
no_state_change 'bound loops'

fixture
add_partition efs sdb3 3
refuses 'a partition label carried by two partitions' 'partition label efs must name exactly one partition (found 2)'
not_mounted efs
fixture
rm "$fx/dev/disk/by-partlabel/persist"
ln -s ../../sda14 "$fx/dev/disk/by-partlabel/persist"
refuses 'a by-partlabel link to another partition' 'the kernel names sda13'
not_mounted persist
fixture
rm "$fx/bootconfig"
refuses 'a missing boot slot' 'exactly one androidboot.slot_suffix is required'
no_state_change 'a missing boot slot'
fixture
printf 'androidboot.slot_suffix = "_b"\nandroidboot.slot_suffix = "_a"\n' >"$fx/bootconfig"
refuses 'two boot slots' 'exactly one androidboot.slot_suffix is required (found 2)'
no_state_change 'two boot slots'
fixture
printf 'androidboot.slot_suffix = "_c"\n' >"$fx/bootconfig"
refuses 'an unknown boot slot' 'unsupported androidboot.slot_suffix'

fixture
premount_partition /elsewhere sda11 ext4 ro ro,noload
refuses 'a modem partition already mounted directly elsewhere' 'is already mounted directly'
no_state_change 'a directly mounted partition'
fixture
rm "$fx/sys/sda11/dev"
refuses 'an unreadable partition device number' 'cannot read the device number'
no_state_change 'an unreadable device number'

# --- refusals after BLKROSET, before or after the read-only attach ---------------
fixture
export FAKE_SETRO_FAIL=1
refuses 'a failed BLKROSET' 'could not mark .* read-only'
unset FAKE_SETRO_FAIL
[ "$(count '^losetup ')" -eq 0 ] && [ "$(count '^mount ')" -eq 0 ] || fail 'S5300 mounts attached or mounted after a failed BLKROSET'
fixture
export FAKE_SETRO_NOOP=1
refuses 'a BLKROSET the kernel does not report' 'does not report read-only after BLKROSET'
unset FAKE_SETRO_NOOP
[ "$(count '^losetup ')" -eq 0 ] && [ "$(count '^mount ')" -eq 0 ] || fail 'S5300 mounts attached or mounted on a writable partition'

fixture
export FAKE_STEAL_LOOP=loop0
refuses 'a loop taken between selection and attach' 'loop0 was taken before modem_b could be attached'
unset FAKE_STEAL_LOOP
[ "$(count '^losetup ')" -eq 0 ] || fail 'S5300 mounts attached or detached a loop it did not own'
loop_bound loop0 || fail 'S5300 mounts released a loop another process owns'
fixture
export FAKE_LOSETUP_FAIL=1
refuses 'a failed read-only attach' 'could not attach .* to loop0 read-only'
unset FAKE_LOSETUP_FAIL
[ "$(count '^losetup -d ')" -eq 0 ] && [ "$(count '^mount ')" -eq 0 ] ||
	fail 'S5300 mounts detached or mounted after a failed attach'

loop_refuses() {
	# description reason
	refuses "$1" "$2"
	grep -Fqx "losetup -d $fx/dev/loop0" "$fx/log" || fail "S5300 mounts did not detach its loop after $1"
	! loop_bound loop0 || fail "S5300 mounts left its loop bound after $1"
	[ "$(count '^mount ')" -eq 0 ] || fail "S5300 mounts mounted after $1"
}
fixture
export FAKE_LOOP_RO=0
loop_refuses 'a loop that does not read read-only' 'the loop is not read-only'
unset FAKE_LOOP_RO
fixture
export FAKE_LOOP_BACKING=/dev/sda12
loop_refuses 'a loop backed by another partition' 'it is not backed by'
unset FAKE_LOOP_BACKING
fixture
export FAKE_LOOP_OFFSET=4096
loop_refuses 'a loop with an offset' 'it maps only part of'
unset FAKE_LOOP_OFFSET
fixture
export FAKE_LOOP_SIZE=8
loop_refuses 'a loop smaller than the partition' 'its size differs from'
unset FAKE_LOOP_SIZE
fixture
export FAKE_LOOP_DEV=259:0
loop_refuses 'a node that is not a loop block device' 'not a loop block device (major 7)'
unset FAKE_LOOP_DEV
fixture
export FAKE_TYPE_loop0=f2fs
loop_refuses 'a loop whose superblock type differs from the partition' "it carries 'f2fs', not the ext4 superblock"
unset FAKE_TYPE_loop0
fixture
export FAKE_TYPE_loop0=
loop_refuses 'a loop without a filesystem signature' "it carries 'none'"
unset FAKE_TYPE_loop0

fixture
export FAKE_MOUNT_FAIL=1
refuses 'a failed mount' 'could not mount modem_b read-only'
unset FAKE_MOUNT_FAIL
grep -Fqx "losetup -d $fx/dev/loop0" "$fx/log" && ! loop_bound loop0 || fail 'S5300 mounts left its loop bound after a failed mount'
[ "$(count '^mount ')" -eq 1 ] || fail 'S5300 mounts continued after a failed mount'

fixture
export FAKE_HIDE_RECOVERY=1
refuses 'a kernel that does not report noload' 'cannot confirm noload'
unset FAKE_HIDE_RECOVERY
grep -Fqx "umount -- $fx/mnt/vendor/modem_img" "$fx/log" || fail 'S5300 mounts left an unconfirmed mount in place'
grep -Fqx "losetup -d $fx/dev/loop0" "$fx/log" || fail 'S5300 mounts left the loop of an unconfirmed mount attached'
[ "$(count '^mount ')" -eq 1 ] || fail 'S5300 mounts continued after an unconfirmed mount'
fixture
export FAKE_HIDE_RECOVERY=1 FAKE_UMOUNT_FAIL=1
refuses 'an unconfirmed mount that cannot be unmounted' 'could not unmount unconfirmed'
unset FAKE_HIDE_RECOVERY FAKE_UMOUNT_FAIL
[ "$(count '^losetup -d ')" -eq 0 ] || fail 'S5300 mounts detached a loop that is still mounted'
loop_bound loop0 || fail 'S5300 mounts released the loop under a live mount'
fixture
export FAKE_LOOP_RO=0 FAKE_DETACH_FAIL=1
refuses 'an unverified loop that cannot be detached' 'could not detach loop0'
unset FAKE_LOOP_RO FAKE_DETACH_FAIL
[ "$(count '^mount ')" -eq 0 ] || fail 'S5300 mounts mounted an unverified loop'

# --- existing mounts at a destination --------------------------------------------
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,noload
run start || { cat "$fx/err" >&2; fail 'S5300 mounts refused an existing verified read-only loop mount'; }
not_mounted efs
if grep sda12 "$fx/log" | grep -vq '^blkid '; then fail 'S5300 mounts re-attached an existing verified efs mount'; fi
[ "$(count '^losetup --read-only ')" -eq 3 ] || fail 'S5300 mounts attached a second loop over a verified source'
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,norecovery
run start || fail 'S5300 mounts refused the norecovery spelling of the ext4 noload flag'
fixture
premount_partition "$fx/mnt/vendor/efs" sda12 ext4 ro,nosuid,nodev,noexec ro,noload
refuses 'an existing direct partition mount with every option' 'sda12 itself is mounted'
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,noload
echo 0 >"$fx/sys/sda12/ro"
refuses 'an existing loop mount over a writable partition' 'is not marked read-only'
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,noload
echo 0 >"$fx/sys/loop3/ro"
refuses 'an existing mount of a writable loop' 'the loop is not read-only'
fixture
premount_loop 3 sda13 efs ext4 ro,nosuid,nodev,noexec ro,noload
refuses 'a loop over a foreign partition at the efs mount point' 'it is not backed by'
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,noload
echo 4096 >"$fx/sys/loop3/loop/offset"
refuses 'an existing loop with an offset' 'it maps only part of'
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,noload
sed -i.bak 's#^50 1 7:3 / #50 1 7:3 /nested #' "$fx/mountinfo"
refuses 'a bind of a loop subdirectory' 'it is not all of a read-only loop'
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,noload
sed -i.bak 's#^50 1 7:3 #50 1 7:4 #' "$fx/mountinfo"
refuses 'a mountinfo device number that is not the named loop' 'it is not all of a read-only loop'
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec ro,data=ordered
refuses 'an existing ext4 ro mount without noload' 'cannot confirm noload on the ext4 superblock'
fixture
export FAKE_TYPE_sda13=f2fs
premount_loop 3 sda13 persist f2fs ro,nosuid,nodev,noexec ro,disable_roll_forward
refuses 'an existing f2fs ro mount without norecovery' 'cannot confirm norecovery on the f2fs superblock'
unset FAKE_TYPE_sda13
fixture
premount_loop 3 sda12 efs ext4 ro,nosuid,nodev,noexec rw,noload
refuses 'a read-only bind of a writable superblock' 'non-read-only mount or superblock'
fixture
premount_loop 3 sda12 efs f2fs ro,nosuid,nodev,noexec ro,norecovery
refuses 'a mount type that differs from the superblock' 'refusing a f2fs mount'

# --- stop ------------------------------------------------------------------------
fixture
run start || fail 'S5300 mounts refused a clean start before the stop checks'
export FAKE_UMOUNT_FAIL=1
if run stop; then fail 'S5300 mounts stop accepted a busy mount'; fi
unset FAKE_UMOUNT_FAIL
grep -q 'could not unmount' "$fx/err" || fail 'S5300 mounts stop refusal reason'
[ "$(count '^losetup -d ')" -eq 0 ] || fail 'S5300 mounts stop detached a loop under a busy mount'
if grep -q 'umount .*-[fl]' "$fx/log"; then fail 'S5300 mounts stop forced or lazily detached a mount'; fi
fixture
run start || fail 'S5300 mounts refused a clean start before the shared-loop stop check'
# Another mount of the same loop elsewhere keeps the loop attached.
grep -E "^99 1 7:3 " "$fx/mountinfo" | sed "s#$fx/mnt/vendor/modem_userdata#/elsewhere#" >>"$fx/mountinfo"
if run stop; then fail 'S5300 mounts stop detached a loop that is still mounted elsewhere'; fi
grep -q 'leaving loop3 attached: it is still mounted' "$fx/err" || fail 'S5300 mounts stop shared-loop refusal reason'
loop_bound loop3 || fail 'S5300 mounts stop released a loop that is still mounted'

stop_refuses() {
	# description reason
	if run stop; then fail "S5300 mounts stop accepted $1"; fi
	grep -q -- "$2" "$fx/err" || { cat "$fx/err" >&2; fail "S5300 mounts stop refusal reason: $1"; }
	[ "$(count '^umount ')" -eq 0 ] && [ "$(count '^losetup -d ')" -eq 0 ] ||
		fail "S5300 mounts stop unmounted or detached after refusing $1"
}
stop_after_start() {
	fixture
	run start || { cat "$fx/err" >&2; fail 'S5300 mounts refused a clean start before a stop refusal check'; }
	: >"$fx/log"
}
fixture
run stop || { cat "$fx/err" >&2; fail 'S5300 mounts stop refused when nothing was mounted'; }
[ "$(count '^umount ')" -eq 0 ] && [ "$(count '^losetup ')" -eq 0 ] || fail 'S5300 mounts stop acted with nothing mounted'
fixture
premount_partition "$fx/mnt/vendor/efs" sda12 ext4 ro,nosuid,nodev,noexec ro,noload
stop_refuses 'a direct partition mount at efs' 'sda12 itself is mounted'
grep -q "refusing to stop: $fx/mnt/vendor/efs" "$fx/err" || fail 'S5300 mounts stop did not name the refused destination'
fixture
premount_loop 3 sda13 efs ext4 ro,nosuid,nodev,noexec ro,noload
stop_refuses 'a loop over a foreign partition at efs' 'it is not backed by'
loop_bound loop3 || fail 'S5300 mounts stop released a foreign loop'
stop_after_start
printf '60 1 0:30 / %s rw - tmpfs tmpfs rw\n' "$fx/mnt/vendor/persist" >>"$fx/mountinfo"
stop_refuses 'an unknown mount stacked on persist' 'it is not all of a read-only loop'
for n in 0 1 2 3; do loop_bound "loop$n" || fail "S5300 mounts stop released loop$n after refusing"; done
stop_after_start
echo 0 >"$fx/sys/loop1/ro"
stop_refuses 'a writable loop at efs' 'the loop is not read-only'
stop_after_start
rm "$fx/bootconfig"
# modem_userdata, persist, and efs verify, but modem_img cannot be tied to a
# slot, so nothing at all is unmounted.
stop_refuses 'a modem image mount whose slot cannot be resolved' 'exactly one androidboot.slot_suffix is required'
[ "$(grep -c "^99 1 7:" "$fx/mountinfo")" -eq 4 ] || fail 'S5300 mounts stop removed a mount before refusing another'
stop_after_start
export FAKE_BLKID_VERSION='BusyBox v1.37.0 multi-call binary.'
stop_refuses 'a stop without util-linux blkid' 'util-linux blkid is required'
unset FAKE_BLKID_VERSION

echo 'pixel-9a s5300 mounts service tests passed'
