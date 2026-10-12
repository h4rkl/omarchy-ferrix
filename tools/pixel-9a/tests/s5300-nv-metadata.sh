#!/bin/sh
# Offline proof for the issue #79 S5300 NV metadata workflow. Three layers,
# none of which contacts a device or mounts anything:
#
#   static   the guest body copies the issue #34 barrier blocks verbatim, runs
#            only metadata lookups on two allowlisted names, and the host
#            pins match the checked-in files;
#   guest    the real guest body runs with its /dev, /sys, /proc, /etc, and
#            /run paths redirected into a fixture, and blkid, blockdev,
#            losetup, mount, umount, stat, dmesg, timeout, uname, and id as
#            PATH stubs that model the sysfs state the kernel would show;
#   host     the real CDC-ACM transport talks to a fake guest
#            (acm-fake-guest.sh) that answers with the guest-layer outputs or
#            derived fixtures, so the host's gates and classification run
#            end to end.
#
# Passing this is not physical evidence.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
host="$root/tools/pixel-9a/s5300-nv-metadata.sh"
probe="$root/tools/pixel-9a/s5300-nv-metadata-remote.sh"
issue34_probe="$root/tools/pixel-9a/s5300-mount-proof-remote.sh"
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
sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }

# --- static ---------------------------------------------------------------------

check "static: host POSIX syntax" sh -n "$host"
check "static: probe POSIX syntax" sh -n "$probe"
verbs='adb|fastboot|mount|umount|blockdev|dd|mkfs|fsck|e2fsck|tune2fs|debugfs|flash|erase|reboot|losetup|ssh|scp|curl'
offending=$(grep -vE '^[[:space:]]*(#|printf )' "$host" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*($verbs)([[:space:]]|$)" || true)
check "static: the host runs no storage, mount, Android, or network command" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: host pins the checked-in guest body" \
	sh -c '[ "$(sed -n "s/^reviewed_probe_sha256=//p" "$1")" = "$2" ]' sh "$host" "$(sha256 "$probe")"
check "static: host pins the checked-in issue #34 probe" \
	sh -c '[ "$(sed -n "s/^issue34_probe_sha256=//p" "$1")" = "$2" ]' sh "$host" "$(sha256 "$issue34_probe")"
check "static: host pins the checked-in ferrix-s5300-mounts" \
	sh -c '[ "$(sed -n "s/^accepted_service_sha256=//p" "$1")" = "$2" ]' sh "$host" "$(sha256 "$initd")"

# block FILE START END: the first block from a line matching START through the
# next line matching END.
block() {
	START=$2 END=$3 awk 'on { print; if ($0 ~ ENVIRON["END"]) exit; next } $0 ~ ENVIRON["START"] { on = 1; print }' "$1"
}
same_block() {
	a=$(block "$probe" "$2" "$3")
	b=$(block "$issue34_probe" "$2" "$3")
	check "static: $1 is the issue #34 block verbatim" sh -c '[ -n "$1" ] && [ "$1" = "$2" ]' sh "$a" "$b"
}
same_block 'free_loop (unbound loop with ro 0)' '^free_loop\(\) \{$' '^}$'
same_block 'under_count' '^under_count\(\) \{$' '^}$'
same_block 'read_stat' '^read_stat\(\) \{$' '^}$'
same_block 'read_flag' '^read_flag\(\) \{$' '^}$'
same_block 'probe_part (partition resolution)' '^probe_part\(\) \{$' '^}$'
same_block 'cleanup (unmount, then detach)' '^cleanup\(\) \{$' '^}$'
same_block 'BLKROSET check and read-only attach' '^if \[ "\$setro_exit" -eq 0 \] && \[ "\$ro_now" = 1 \]; then$' '^fi$'
same_block 'loop verification' '^if \[ "\$bound" = yes \]; then$' '^fi$'
same_block 'verified-loop mount gate and mount call' '^# Mount only from a loop that sysfs' '^	mount_exit=\$\?$'
same_block 'BLKROSET call' '^blockdev --setro' '^printf .setro_exit'
same_block 'final teardown exit' '^\[ "\$mounted" = no \] [|][|] exit 7$' '^exit 0$'
code=$(grep -v '^[[:space:]]*#' "$probe")
check "static: exactly one mount call, from the loop" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -cE "(^|[;&|{(]|then|do)[[:space:]]*mount[[:space:]]")" -eq 1 ] &&
		printf "%s\n" "$1" | grep -Fq "mount -t \"\$proof_fstype\" -o \"ro,\$recovery,nosuid,nodev,noexec\" -- \"\$loop\" \"\$destination\""' sh "$code"
check "static: mount mode is efs and f2fs only" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq "[ \"\${proof_role:-}\" = efs ] || exit 5" && printf "%s\n" "$1" | grep -Fxq "[ \"\${proof_fstype:-}\" = f2fs ] || exit 5" &&
		printf "%s\n" "$1" | grep -Fxq "recovery=norecovery"' sh "$code"
check "static: the guest never clears the read-only flag or lets losetup pick a loop" \
	sh -c '! printf "%s\n" "$1" | grep -nE "setrw|BLKROSET.*0|losetup -[a-z]*f|--find|autoclear"' sh "$code"
preflight_part=$(sed -n '1,/^# --- mount mode/p' "$probe" | grep -v '^[[:space:]]*#')
check "static: nothing before the mount-mode boundary mounts or changes a block device" \
	sh -c '! printf "%s\n" "$1" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*(mount|umount|blockdev[[:space:]]+--set|losetup[[:space:]]+-[-a-z]*[^V ]|dd|mkfs|fsck)([[:space:]]|$)"' sh "$preflight_part"
check "static: preflight probes only efs and returns before the boundary" \
	sh -c 'printf "%s\n" "$1" | grep -A3 "proof_mode\" = preflight ]; then" | grep -Fq "probe_part efs efs" &&
		printf "%s\n" "$1" | grep -A3 "proof_mode\" = preflight ]; then" | grep -q "exit 0"' sh "$preflight_part"
# Every use of an NV path is an lstat-class test or `stat -c %s` without -L,
# and nothing on the mounted filesystem is listed, opened, or walked.
printf '%s\n' 'if [ -L "$nv_path" ]; then' 'elif [ ! -e "$nv_path" ]; then' 'elif [ -d "$nv_path" ]; then' \
	'elif [ -f "$nv_path" ]; then' "nv_size=\$(timeout 10 stat -c %s \"\$nv_path\" 2>/dev/null) || nv_size=''" >"${TMPDIR:-/tmp}/nv-allow.$$"
nv_uses=$(printf '%s\n' "$code" | grep -F '$nv_path' | sed 's/^[[:space:]]*//' | grep -vxF 'nv_path=$destination/$1' || true)
nv_other=$(printf '%s\n' "$nv_uses" | grep -vxFf "${TMPDIR:-/tmp}/nv-allow.$$" || true)
rm -f "${TMPDIR:-/tmp}/nv-allow.$$"
check "static: NV paths are only lstat-class tests and one stat -c %s without -L" \
	sh -c '[ "$(printf "%s\n" "$1" | wc -l | tr -d " ")" -eq 5 ] && [ -z "$2" ]' sh "$nv_uses" "$nv_other"
[ -z "$nv_other" ] || printf '%s\n' "$nv_other"
check "static: symlink test precedes every following test" \
	awk '/nv_lookup\(\) \{/ { on = 1 } on && /-L "\$nv_path"/ { l = NR } on && /-[edf] "\$nv_path"/ { if (!l) bad = 1 } on && /^}/ { on = 0 } END { exit bad || !l }' "$probe"
check "static: no read, hash, walk, or listing of the mounted filesystem" \
	sh -c '! printf "%s\n" "$1" | grep -E "\\\$destination|\\\$nv_path" | grep -E "(^|[^a-z_])(cat|head|tail|dd|od|xxd|hexdump|sha[0-9]*sum|md5sum|wc|find|ls|readlink|stat -L|cp|tar|strings|grep)[[:space:]]"' sh "$code"
check "static: only the two allowlisted names are looked up" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq "nv_names='"'"'nv_normal.bin nv_protected.bin'"'"'" &&
		[ "$(printf "%s\n" "$1" | grep -c "nv_lookup \"")" -eq 1 ] && printf "%s\n" "$1" | grep -Fq "for nv_name in \$nv_names; do nv_lookup \"\$nv_name\"; done"' sh "$code"
check "static: lookups happen only on the session's own loop mount" \
	sh -c 'printf "%s\n" "$1" | grep -Fq "*\" \$loop_dev / \$destination \"*)"' sh "$code"
check "static: guest and host pin the same Kleaf kernel" \
	sh -c '[ "$(sed -n "s/^pinned_kernel=//p" "$1")" = "$(sed -n "s/^pinned_kernel=//p" "$2")" ]' sh "$probe" "$host"
check "static: host keeps cpif services and CP boot out of scope" \
	sh -c '! grep -nE "rc-update|rc-service|exynos-modem (start|--)|umts_boot|/dev/umts|modem_state" "$1"' sh "$host"
probe_status=0
{ printf 'proof_mode=preflight\nproof_role=\nproof_dev=\nproof_devname=\nproof_fstype=\nproof_service_sha256=\n'; cat "$probe"; } | sh >/dev/null 2>&1 || probe_status=$?
check "probe: refuses a host that is not the tegu board" [ "$probe_status" -eq 4 ]

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-nv-metadata-test.XXXXXX")
work=$(CDPATH= cd -- "$work" && pwd -P)
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; [ -n "${KEEP:-}" ] || { chmod -R u+rwx "$work" 2>/dev/null; rm -rf "$work"; }' EXIT

# --- guest: the real body against a stubbed kernel ------------------------------

fx="$work/gfx"
bin="$work/gbin"
body="$work/guest-body.sh"
mkdir -p "$bin"
sed -e "s#^partlabel_root=.*#partlabel_root=$fx/dev/disk/by-partlabel#" \
	-e "s#^sys_block_path=.*#sys_block_path=$fx/sys#" \
	-e "s#^mountinfo_path=.*#mountinfo_path=$fx/mountinfo#" \
	-e "s#^mount_root=.*#mount_root=$fx/mnt/vendor#" \
	-e "s#^proof_root=.*#proof_root=$fx/run/ferrix-s5300-nv-metadata#" \
	-e "s#^issue34_root=.*#issue34_root=$fx/run/ferrix-s5300-mount-proof#" \
	-e "s#^dt_root=.*#dt_root=$fx/dt#" \
	-e "s#^filesystems_path=.*#filesystems_path=$fx/filesystems#" \
	-e "s#^kmsg_path=.*#kmsg_path=$fx/kmsg#" \
	-e "s#^service_file=.*#service_file=$fx/etc/init.d/ferrix-s5300-mounts#" \
	-e "s#^runlevels_root=.*#runlevels_root=$fx/etc/runlevels#" \
	-e "s#^uuid_path=.*#uuid_path=$fx/uuid#" \
	-e "s#\"/dev/\\\$#\"$fx/dev/\$#g" \
	-e "s#^loop=/dev/#loop=$fx/dev/#" \
	-e 's#\[ -b #[ -e #g' \
	"$probe" >"$body"
# Twelve path variables, three /dev literals, two block-device tests.
check "guest: harness redirected every guest path" sh -c '[ "$(grep -c "$1" "$2")" -eq 15 ] && ! grep -q "\[ -b " "$2"' sh "$fx" "$body"

cat >"$bin/uname" <<'EOF'
#!/bin/sh
printf '%s\n' "${FAKE_KERNEL-6.1.124-android14-11-ga4662a84a15f}"
EOF
cat >"$bin/id" <<'EOF'
#!/bin/sh
printf '%s\n' "${FAKE_UID-0}"
EOF
# Busybox provides sha256sum on the guest; some hosts only have shasum.
command -v sha256sum >/dev/null 2>&1 || printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' >"$bin/sha256sum"
cat >"$bin/timeout" <<'EOF'
#!/bin/sh
shift
exec "$@"
EOF
cat >"$bin/sync" <<'EOF'
#!/bin/sh
EOF
cat >"$bin/sleep" <<'EOF'
#!/bin/sh
EOF
cat >"$bin/dmesg" <<'EOF'
#!/bin/sh
cat "$FAKE_KMSG"
EOF
cat >"$bin/stat" <<'EOF'
#!/bin/sh
# Only `stat -c %s PATH`, answered from the directory entry (ls -ln) so the
# fixture can make the NV files unreadable.
printf 'stat %s\n' "$*" >>"$FAKE_LOG"
[ "$#" -eq 3 ] && [ "$1" = -c ] && [ "$2" = %s ] || exit 64
[ -z "${FAKE_STAT_FAIL-}" ] || [ "$3" = / ] || exit 1
ls -lnd "$3" | awk '{ print $5 }'
EOF
cat >"$bin/blkid" <<'EOF'
#!/bin/sh
[ "$1" = -V ] && { printf '%s\n' "${FAKE_BLKID_VERSION-blkid from util-linux 2.40.2  (libblkid 2.40.2)}"; exit 0; }
printf 'blkid %s\n' "$*" >>"$FAKE_LOG"
for device; do :; done
name=${device##*/}
case "$name" in
	loop*)
		backing=$(cat "$FAKE_SYS/$name/loop/backing_file" 2>/dev/null) || exit 2
		name=${backing##*/}
		;;
esac
eval "type=\${FAKE_TYPE_$name-f2fs}"
[ -n "$type" ] || exit 2
printf '%s\n' "$type"
EOF
cat >"$bin/blockdev" <<'EOF'
#!/bin/sh
[ "$*" = -V ] && { echo 'blockdev from util-linux 2.40.2'; exit 0; }
printf 'blockdev %s\n' "$*" >>"$FAKE_LOG"
[ "$#" -eq 2 ] && [ "$1" = --setro ] || exit 64
[ -z "${FAKE_SETRO_FAIL-}" ] || exit 1
echo 1 >"$FAKE_SYS/${2##*/}/ro"
EOF
cat >"$bin/losetup" <<'EOF'
#!/bin/sh
[ "$*" = -V ] && { printf '%s\n' "${FAKE_LOSETUP_VERSION-losetup from util-linux 2.40.2}"; exit 0; }
printf 'losetup %s\n' "$*" >>"$FAKE_LOG"
case "$#:$1" in
	2:-d)
		[ -z "${FAKE_DETACH_FAIL-}" ] || exit 1
		name=${2##*/}
		rm -rf "$FAKE_SYS/$name/loop"
		echo 0 >"$FAKE_SYS/$name/size"
		;;
	4:--read-only)
		[ "$2" = -- ] || exit 64
		name=${3##*/}
		part=${4##*/}
		[ ! -e "$FAKE_SYS/$name/loop" ] || exit 1
		mkdir "$FAKE_SYS/$name/loop"
		echo "$4" >"$FAKE_SYS/$name/loop/backing_file"
		echo "${FAKE_LOOP_OFFSET-0}" >"$FAKE_SYS/$name/loop/offset"
		echo 0 >"$FAKE_SYS/$name/loop/sizelimit"
		cat "$FAKE_SYS/$part/size" >"$FAKE_SYS/$name/size"
		echo "${FAKE_LOOP_RO-1}" >"$FAKE_SYS/$name/ro"
		;;
	*) exit 64 ;;
esac
EOF
cat >"$bin/mount" <<'EOF'
#!/bin/sh
# Mounting the loop makes the fixture efs tree appear at the destination.
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
dev=$(cat "$FAKE_SYS/${1##*/}/dev")
rmdir "$2" && ln -s "$FAKE_EFS" "$2" || exit 32
printf '99 1 %s / %s ro,nosuid,nodev,noexec,relatime - %s %s ro,lazytime,background_gc=on,norecovery,active_logs=6\n' \
	"$dev" "$2" "$type" "$1" >>"$FAKE_MOUNTINFO"
printf 'F2FS-fs (%s): Mounted with checkpoint version = 1a2b\n' "${1##*/}" >>"$FAKE_KMSG"
EOF
cat >"$bin/umount" <<'EOF'
#!/bin/sh
printf 'umount %s\n' "$*" >>"$FAKE_LOG"
[ -z "${FAKE_UMOUNT_FAIL-}" ] || exit 32
for target; do :; done
awk -v target="$target" '$5 != target' "$FAKE_MOUNTINFO" >"$FAKE_MOUNTINFO.next"
mv "$FAKE_MOUNTINFO.next" "$FAKE_MOUNTINFO"
rm "$target" && mkdir "$target"
EOF
chmod 0755 "$bin"/*

add_partition() {
	# label devname minor
	: >"$fx/dev/$2"
	mkdir -p "$fx/devices/sda/$2/holders"
	ln -s "../devices/sda/$2" "$fx/sys/$2"
	printf 'MAJOR=8\nMINOR=%s\nDEVNAME=%s\nDEVTYPE=partition\nPARTNAME=%s\n' "$3" "$2" "$1" >"$fx/sys/$2/uevent"
	echo "8:$3" >"$fx/sys/$2/dev"
	echo "$(($3 * 1024))" >"$fx/sys/$2/size"
	echo 0 >"$fx/sys/$2/ro"
	echo "$3" >"$fx/sys/$2/partition"
	echo '  1 0 8 0 0 0 0 0 0 4 4 0 0 0 0 0 0' >"$fx/sys/$2/stat"
}
add_loop() {
	: >"$fx/dev/loop$1"
	mkdir -p "$fx/sys/loop$1/queue"
	echo "7:$1" >"$fx/sys/loop$1/dev"
	echo 0 >"$fx/sys/loop$1/size"
	echo "${2:-0}" >"$fx/sys/loop$1/ro"
	echo 1 >"$fx/sys/loop$1/queue/iostats"
	echo '2 0 16 0 0 0 0 0 0 1 1 0 0 0 0 0 0' >"$fx/sys/loop$1/stat"
}
# nv_file NAME BYTES: a regular file of that size that cannot be read.
nv_file() {
	head -c "$2" /dev/zero >"$fx/efs/$1"
	chmod 000 "$fx/efs/$1"
}
fixture() {
	chmod -R u+rwx "$fx" 2>/dev/null || :
	rm -rf "$fx"
	mkdir -p "$fx/dev/disk/by-partlabel" "$fx/sys" "$fx/devices/sda/queue" "$fx/dt" "$fx/mnt/vendor" \
		"$fx/run" "$fx/etc/init.d" "$fx/etc/runlevels/default" "$fx/efs/.private-dir"
	ln -s ../devices/sda "$fx/sys/sda"
	echo 1 >"$fx/devices/sda/queue/iostats"
	printf 'DEVNAME=sda\nDEVTYPE=disk\n' >"$fx/devices/sda/uevent"
	add_partition efs sda12 12
	add_partition persist sda13 13
	ln -s ../../sda12 "$fx/dev/disk/by-partlabel/efs"
	ln -s ../../sda13 "$fx/dev/disk/by-partlabel/persist"
	add_loop 0 1
	add_loop 1
	add_loop 2
	printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fx/dt/model"
	printf 'google,ZUMA PRO TEGU\000google,zumapro\000' >"$fx/dt/compatible"
	printf 'nodev\tproc\n\text4\n\tf2fs\n' >"$fx/filesystems"
	printf '1 0 8:1 / / rw - ext4 /dev/root rw\n' >"$fx/mountinfo"
	: >"$fx/kmsg"
	echo 0123abcd-4567-89ab-cdef-0123456789ab >"$fx/uuid"
	cp "$initd" "$fx/etc/init.d/ferrix-s5300-mounts"
	nv_file nv_normal.bin 524288
	nv_file nv_protected.bin 1048576
	# Names and data that must never be reported.
	printf 'IMEI 356938035643809\n' >"$fx/efs/factory.prop"
	ln -s /nowhere/secret-target "$fx/efs/link-SN0123"
	: >"$fx/log"
}
accepted=$(sha256 "$initd")
# guest MODE [DEV DEVNAME SERVICE]: run the redirected body; output in
# $fx/out, exit status in guest_status.
guest() {
	guest_status=0
	{
		if [ "$1" = preflight ]; then
			printf 'proof_mode=preflight\nproof_role=\nproof_dev=\nproof_devname=\nproof_fstype=\nproof_service_sha256=\n'
		else
			printf 'proof_mode=mount\nproof_role=efs\nproof_dev=%s\nproof_devname=%s\nproof_fstype=f2fs\nproof_service_sha256=%s\n' \
				"${2:-8:12}" "${3:-sda12}" "${4:-$accepted}"
		fi
		cat "$body"
	} | env PATH="$bin:$PATH" FAKE_LOG="$fx/log" FAKE_SYS="$fx/sys" FAKE_MOUNTINFO="$fx/mountinfo" \
		FAKE_KMSG="$fx/kmsg" FAKE_EFS="$fx/efs" sh >"$fx/out" 2>"$fx/err" || guest_status=$?
}
gout() { grep -Fxq "$1" "$fx/out"; }
gcount() { grep -c -- "$1" "$fx/log" || true; }
no_state_change() { [ "$(gcount '^blockdev ')" -eq 0 ] && [ "$(gcount '^losetup ')" -eq 0 ] && [ "$(gcount '^mount ')" -eq 0 ]; }
torn_down() {
	[ "$(wc -l <"$fx/mountinfo" | tr -d ' ')" -eq 1 ] && [ ! -e "$fx/sys/loop1/loop" ] && [ ! -L "$fx/run/ferrix-s5300-nv-metadata/efs" ]
}
private() {
	! grep -Eq 'IMEI|356938035643809|factory\.prop|SN0123|secret-target|private-dir|0123abcd' "$fx/out"
}

fixture
guest preflight
check "guest preflight: completes" [ "$guest_status" -eq 0 ]
check "guest preflight: skips the stale read-only loop" gout 'loop_free=loop1'
check "guest preflight: resolves efs uniquely as f2fs on sda" \
	sh -c 'grep -Fxq part.efs.partname_count=1 "$1" && grep -Fxq part.efs.blkid_type=f2fs "$1" && grep -Fxq part.efs.parent=sda "$1" && grep -Fxq part.efs.iostats=1 "$1"' sh "$fx/out"
check "guest preflight: reports the accepted service hash" gout "service_sha256=$accepted"
check "guest preflight: probes efs only" sh -c '! grep -q "^part\.persist\." "$1"' sh "$fx/out"
check "guest preflight: changes no state" no_state_change
check "guest preflight: reveals nothing from the filesystem" private
cp "$fx/out" "$work/g-preflight"

fixture
guest mount
check "guest mount: completes" [ "$guest_status" -eq 0 ]
check "guest mount: BLKROSET, read-only attach, verify, mount the loop, unmount, detach in order" \
	awk -v fx="$fx" '$0 == "blockdev --setro " fx "/dev/sda12" { s = NR } $0 == "losetup --read-only -- " fx "/dev/loop1 " fx "/dev/sda12" { l = NR }
		$0 == "blkid -c /dev/null -p -o value -s TYPE -- " fx "/dev/loop1" { b = NR }
		$0 == "mount -t f2fs -o ro,norecovery,nosuid,nodev,noexec -- " fx "/dev/loop1 " fx "/run/ferrix-s5300-nv-metadata/efs" { m = NR }
		/^stat -c %s .*nv_normal\.bin$/ { n = NR } /^umount -- / { u = NR } $0 == "losetup -d " fx "/dev/loop1" { d = NR }
		END { exit !(s && l && b && m && n && u && d && s < l && l < b && b < m && m < n && n < u && u < d) }' "$fx/log"
check "guest mount: never mounts the partition itself" sh -c '! grep -Eq "^mount .* -- $1/dev/sda[0-9]+ " "$2"' sh "$fx" "$fx/log"
check "guest mount: stats only the two allowlisted names" \
	sh -c '[ "$(grep -c "^stat -c %s $1/" "$2")" -eq 2 ] && grep -Fxq "stat -c %s $1/run/ferrix-s5300-nv-metadata/efs/nv_normal.bin" "$2" &&
		grep -Fxq "stat -c %s $1/run/ferrix-s5300-nv-metadata/efs/nv_protected.bin" "$2"' sh "$fx" "$fx/log"
check "guest mount: sizes of unreadable files from lstat" \
	sh -c 'grep -Fxq nv.nv_normal.bin.type=regular "$1" && grep -Fxq nv.nv_normal.bin.size=524288 "$1" && grep -Fxq nv.nv_protected.bin.size=1048576 "$1"' sh "$fx/out"
check "guest mount: lookups ran on its own loop mount" gout 'nv_root=mounted-loop'
efs_ro() { [ "$(cat "$fx/sys/sda12/ro")" = 1 ]; }
check "guest mount: torn down" torn_down
check "guest mount: partition left read-only" efs_ro
check "guest mount: clean post-check" sh -c 'grep -Fxq post.vendor_mounts=0 "$1" && grep -Fxq post.proof_mounts=0 "$1" && grep -Fxq "service_sha256.after=$2" "$1"' sh "$fx/out" "$accepted"
check "guest mount: reveals nothing else from the filesystem" private
check "guest mount: mountinfo source redacted" sh -c 'grep -q "^mountinfo=.* redacted-source " "$1" && ! grep -q "^mountinfo=.*/dev/loop" "$1"' sh "$fx/out"
cp "$fx/out" "$work/g-mount"

fixture
chmod u+w "$fx/efs/nv_protected.bin"
head -c 4096 /dev/zero >"$fx/efs/nv_protected.bin"
chmod 000 "$fx/efs/nv_protected.bin"
chmod 000 "$fx/efs/nv_normal.bin"
mv "$fx/efs/nv_normal.bin" "$fx/efs/nv_normal.moved"
guest mount
check "guest mount short/missing: still completes" [ "$guest_status" -eq 0 ]
check "guest mount short/missing: tears down" torn_down
check "guest mount short/missing: absent and short reported" \
	sh -c 'grep -Fxq nv.nv_normal.bin.type=absent "$1" && grep -Fxq nv.nv_normal.bin.size=none "$1" && grep -Fxq nv.nv_protected.bin.size=4096 "$1"' sh "$fx/out"
cp "$fx/out" "$work/g-short"

fixture
chmod u+w "$fx/efs/nv_normal.bin" && rm "$fx/efs/nv_normal.bin"
ln -s nv_protected.bin "$fx/efs/nv_normal.bin"
chmod u+w "$fx/efs/nv_protected.bin" && rm "$fx/efs/nv_protected.bin"
mkdir "$fx/efs/nv_protected.bin"
guest mount
check "guest mount symlink/dir: never followed, never sized" \
	sh -c 'grep -Fxq nv.nv_normal.bin.type=symlink "$1" && grep -Fxq nv.nv_protected.bin.type=directory "$1" && ! grep -q "^stat -c %s .*nv_" "$2"' sh "$fx/out" "$fx/log"
check "guest mount symlink/dir: torn down" torn_down
check "guest mount symlink/dir: link target never printed" private
cp "$fx/out" "$work/g-symlink"

# Refusals before any state change.
refused() {
	check "guest refusal ($1): exit 6 with reason, no state change" \
		sh -c '[ "$1" -eq 6 ] && grep -Fxq "refusal=$2" "$3" && grep -Fxq state_changed=no "$3"' sh "$guest_status" "$2" "$fx/out"
	check "guest refusal ($1): nothing set, attached, or mounted" no_state_change
}
fixture
add_partition efs sda30 30
guest mount
refused 'two PARTNAME=efs partitions' partition-not-unique
fixture
export FAKE_TYPE_sda12=ext4
guest mount
unset FAKE_TYPE_sda12
refused 'efs no longer f2fs' type-changed-since-preflight
fixture
printf '60 1 8:12 / /data rw - f2fs /dev/sda12 rw\n' >>"$fx/mountinfo"
guest mount
refused 'efs mounted elsewhere' partition-already-mounted
fixture
: >"$fx/sys/sda12/holders/dm-0"
guest mount
refused 'efs held by another device' partition-has-holders
fixture
printf '# changed\n' >>"$fx/etc/init.d/ferrix-s5300-mounts"
guest mount
refused 'installed service not the accepted file' installed-service-not-accepted
fixture
ln -s ../../init.d/ferrix-s5300-mounts "$fx/etc/runlevels/default/ferrix-s5300-mounts"
guest mount
refused 'service in a runlevel' ferrix-s5300-mounts-in-runlevel
fixture
printf '61 1 7:9 / %s/run/ferrix-s5300-mount-proof/efs ro - f2fs /dev/loop9 ro\n' "$fx" >>"$fx/mountinfo"
guest mount
refused 'issue #34 proof mount left behind' issue34-mount-present
fixture
guest mount 8:13 sda13
refused 'bound partition changed since preflight' partition-changed-since-preflight
fixture
echo 1 >"$fx/sys/loop1/ro"
echo 1 >"$fx/sys/loop2/ro"
guest mount
refused 'no clean loop' no-unbound-loop-with-ro-0
fixture
export FAKE_LOSETUP_VERSION='losetup from BusyBox'
guest mount
unset FAKE_LOSETUP_VERSION
refused 'BusyBox losetup' losetup-not-util-linux
fixture
export FAKE_KERNEL=6.1.162-android14-11-gdeadbeef
guest mount
unset FAKE_KERNEL
refused 'unpinned kernel' kernel-not-pinned
cp "$fx/out" "$work/g-refused"
fixture
printf 'QEMU Virtual Machine\000' >"$fx/dt/model"
guest preflight
check "guest identity: a non-tegu device tree exits 4 with no output" sh -c '[ "$1" -eq 4 ] && [ ! -s "$2" ]' sh "$guest_status" "$fx/out"
fixture
guest mount 8:12 sda12 0123
check "guest header: malformed service hash exits 5" [ "$guest_status" -eq 5 ]
check "guest header: malformed service hash changes nothing" no_state_change

# Barrier and teardown failures after BLKROSET.
fixture
export FAKE_LOOP_RO=0
guest mount
unset FAKE_LOOP_RO
check "guest barrier: a loop that is not read-only is never mounted" \
	sh -c '[ "$1" -eq 7 ] && grep -Fxq loop_ro=0 "$2" && grep -Fxq mount_exit=not-run "$2" && grep -Fxq nv_root=not-run "$2" &&
		! grep -q "^mount " "$3"' sh "$guest_status" "$fx/out" "$fx/log"
check "guest barrier: the unverified loop is detached" torn_down
cp "$fx/out" "$work/g-loop-rw"
fixture
export FAKE_LOOP_OFFSET=4096
guest mount
unset FAKE_LOOP_OFFSET
check "guest barrier: a loop not mapping the whole partition is never mounted" \
	sh -c '[ "$1" -eq 7 ] && grep -Fxq loop_geometry=mismatch "$2" && ! grep -q "^mount " "$3"' sh "$guest_status" "$fx/out" "$fx/log"
check "guest barrier: the mismatched loop is detached" torn_down
fixture
export FAKE_SETRO_FAIL=1
guest mount
unset FAKE_SETRO_FAIL
check "guest barrier: BLKROSET failure attaches and mounts nothing" \
	sh -c '[ "$1" -eq 7 ] && grep -Fxq losetup_exit=not-run "$2" && ! grep -q "^losetup " "$3" && ! grep -q "^mount " "$3"' sh "$guest_status" "$fx/out" "$fx/log"
fixture
export FAKE_UMOUNT_FAIL=1
guest mount
unset FAKE_UMOUNT_FAIL
check "guest teardown: unmount failure keeps the loop, exits 7, still reports NV" \
	sh -c '[ "$1" -eq 7 ] && grep -Fxq still_mounted=yes "$2" && grep -Fxq still_bound=yes "$2" && grep -Fxq detach_exit=not-run "$2" &&
		grep -Fxq nv.nv_normal.bin.size=524288 "$2" && ! grep -q "^losetup -d " "$3"' sh "$guest_status" "$fx/out" "$fx/log"
cp "$fx/out" "$work/g-umount"
fixture
export FAKE_DETACH_FAIL=1
guest mount
unset FAKE_DETACH_FAIL
check "guest teardown: detach failure exits 7 with the loop reported bound" \
	sh -c '[ "$1" -eq 7 ] && grep -Fxq still_mounted=no "$2" && grep -Fxq detach_exit=1 "$2" && grep -Fxq still_bound=yes "$2"' sh "$guest_status" "$fx/out"
cp "$fx/out" "$work/g-detach"
fixture
export FAKE_STAT_FAIL=1
guest mount
unset FAKE_STAT_FAIL
check "guest stat failure: regular file with unavailable size, still torn down" \
	sh -c '[ "$1" -eq 0 ] && grep -Fxq nv.nv_normal.bin.size=unavailable "$2"' sh "$guest_status" "$fx/out"
check "guest stat failure: still torn down" torn_down
cp "$fx/out" "$work/g-stat-fail"

# --- host: the workflow through the real transport ------------------------------

mkdir -p "$work/fx"
acm_guest_init "$root" "$work" "$probe"
unset FERRIX_PIXEL9A_ACM_TIMEOUT FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION 2>/dev/null || true
derive() {
	src=$1
	name=$2
	shift 2
	sed "$@" "$src" >"$work/fx/$name"
}
derive "$work/g-preflight" preflight -e ''
derive "$work/g-preflight" pre-qemu -e 's/^dt_model=.*/dt_model=QEMU Virtual Machine/'
derive "$work/g-preflight" pre-service-differs -e "s/^service_sha256=.*/service_sha256=$(printf '%064d' 0)/"
derive "$work/g-preflight" pre-mounted -e 's/^part\.efs\.mounts=0$/part.efs.mounts=1/'
derive "$work/g-preflight" pre-holders -e 's/^part\.efs\.holders=0$/part.efs.holders=1/'
derive "$work/g-preflight" pre-ambiguous -e 's/^part\.efs\.partname_count=1$/part.efs.partname_count=2/' \
	-e 's/^part\.efs\.partname_devices=sda12$/part.efs.partname_devices=sda12 sda30/'
derive "$work/g-preflight" pre-ext4 -e 's/^part\.efs\.blkid_type=f2fs$/part.efs.blkid_type=ext4/'
derive "$work/g-preflight" pre-unknown-key -e '/^probe_complete=/i\
serial=ABC123'
derive "$work/g-preflight" pre-no-stat -e 's/^stat_usable=yes$/stat_usable=no/'
derive "$work/g-preflight" pre-issue34 -e 's/^issue34_mounts=0$/issue34_mounts=1/'
derive "$work/g-mount" mount -e ''
derive "$work/g-short" mount-short -e ''
derive "$work/g-symlink" mount-symlink -e ''
derive "$work/g-refused" mount-refused -e ''
derive "$work/g-loop-rw" mount-loop-rw -e ''
derive "$work/g-umount" mount-umount -e ''
derive "$work/g-detach" mount-detach -e ''
derive "$work/g-stat-fail" mount-stat-fail -e ''
derive "$work/g-mount" mount-write -e 's/^stat\.after=.*/stat.after=1 0 8 0 1 0 8 1 0 5 5 0 0 0 0 0 0/'
derive "$work/g-mount" mount-loopwarn -e '/^probe_complete=/i\
kmsg.09=Trying to write to read-only block-device loop1'
derive "$work/g-short" mount-short-write -e 's/^stat\.after=.*/stat.after=1 0 8 0 1 0 8 1 0 5 5 0 0 0 0 0 0/'
derive "$work/g-mount" mount-noflag -e 's/,norecovery,active_logs=6$/,active_logs=6/'
derive "$work/g-mount" mount-leftover -e 's/^post\.proof_mounts=0$/post.proof_mounts=1/'
derive "$work/g-mount" mount-service-after -e "s/^service_sha256\.after=.*/service_sha256.after=$(printf '%064d' 1)/"
derive "$work/g-mount" mount-not-loop -e 's/^nv_root=mounted-loop$/nv_root=not-the-loop-mount/' -e '/^nv\./d'
derive "$work/g-mount" mount-bad-type -e 's/^nv\.nv_normal\.bin\.type=regular$/nv.nv_normal.bin.type=fifo/'
derive "$work/g-mount" mount-extra-name -e '/^probe_complete=/i\
nv.factory.prop.type=regular'
derive "$work/g-mount" mount-imei -e '/^probe_complete=/i\
imei=356938035643809'

status=0
out=''
runs=0
confirm='stat efs NV metadata through one read-only loop mount on google-pixel-9a-tegu-gxq96-mp1-0'
decision=owner-approved-issue34-per-unit-ro-mount
# run CASE PREFLIGHT_FIXTURE MOUNT_FIXTURE MOUNT_EXIT -- ARGS...
run() {
	case_name=$1
	pre_fixture=$2
	mount_fx=$3
	mount_status=$4
	shift 4
	case_dir="$work/run-$case_name"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 6
	acm_guest_ctl fixture.1 "$work/fx/$pre_fixture"
	acm_guest_ctl exit.1 0
	if [ -n "$mount_fx" ]; then
		acm_guest_ctl fixture.2 "$work/fx/$mount_fx"
		acm_guest_ctl exit.2 "$mount_status"
	fi
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_S5300_NV_METADATA_EVIDENCE="${EVIDENCE:-mock}" sh "$host" "$@" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	ctl=$ACM_GUEST_CTL
	results="$case_dir/pixel-9a/s5300-nv-metadata"
	EVIDENCE=mock
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }

run preflight preflight '' 0
check "host preflight: completes in one session" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
check "host preflight: the guest received the exact body after the generated header" cmp -s "$ctl/body.1" "$probe"
check "host preflight: header binds nothing" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=preflight\nproof_role=\nproof_dev=\nproof_devname=\nproof_fstype=\nproof_service_sha256=")" ]' sh "$ctl/header.1"
check "host preflight: ready with the accepted service" sh -c 'printf "%s\n" "$1" | grep -Fxq preflight_status=ready && printf "%s\n" "$1" | grep -Fxq service_installed=accepted' sh "$out"
check "host preflight: transcript is private to the owner" sh -c '[ "$(ls -l "$1" | cut -c1-10)" = "-rw-------" ]' sh "$results/preflight-latest.txt"
EVIDENCE=physical run preflight-physical preflight '' 0
check "host preflight: physical transcript names the exact target" grep -Fxq 'evidence=physical:google-pixel-9a-tegu-gxq96-mp1-0' "$results/preflight-latest.txt"
run pre-service-differs pre-service-differs '' 0
check "host preflight: another installed service blocks" has 'preflight_problems=installed-service-differs-from-accepted'
run pre-ambiguous pre-ambiguous '' 0
check "host preflight: an ambiguous partition blocks" has 'efs.preflight=partname-count-2'
run pre-ext4 pre-ext4 '' 0
check "host preflight: a wrong filesystem type blocks" has 'efs.preflight=type-ext4-expected-f2fs'
run pre-issue34 pre-issue34 '' 0
check "host preflight: a left-over issue #34 proof mount blocks" has 'preflight_problems=issue34-mount-present'
run pre-qemu pre-qemu '' 0
check "host identity: a non-tegu device tree exits 4" [ "$status" -eq 4 ]
run pre-unknown-key pre-unknown-key '' 0
check "host malformed: unknown field refused, value not echoed or kept" \
	sh -c '[ "$1" -eq 5 ] && ! printf "%s\n" "$2" | grep -Fq ABC123 && ! grep -rFq ABC123 "$3"' sh "$status" "$out" "$work/run-pre-unknown-key"

# Gates: nothing reaches the guest without the typed phrase and the decision.
mkdir -p "$work/run-gate"
for args in 'mount efs' 'mount extra' 'unmount' 'stat'; do
	status=0
	# shellcheck disable=SC2086
	FERRIX_OUT_DIR="$work/run-gate" sh "$host" $args >/dev/null 2>&1 || status=$?
	check "host usage: '$args' refused" [ "$status" -eq 2 ]
done
status=0
out=$(FERRIX_OUT_DIR="$work/run-gate" sh "$host" mount 2>&1) || status=$?
check "host gate: missing confirmation refused, phrase named" sh -c '[ "$1" -eq 2 ] && printf "%s\n" "$2" | grep -Fq "$3"' sh "$status" "$out" "$confirm"
status=0
FERRIX_OUT_DIR="$work/run-gate" FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM="mount efs read-only on google-pixel-9a-tegu-gxq96-mp1-0" \
	FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=$decision sh "$host" mount >/dev/null 2>&1 || status=$?
check "host gate: the issue #34 mount phrase is not accepted" [ "$status" -eq 2 ]
status=0
out=$(FERRIX_OUT_DIR="$work/run-gate" FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM="$confirm" sh "$host" mount 2>&1) || status=$?
check "host gate: the per-unit owner decision is required" \
	sh -c '[ "$1" -eq 2 ] && printf "%s\n" "$2" | grep -Fq "per-unit partition outside factory recovery"' sh "$status" "$out"
check "host gate: refused runs write no transcript" [ ! -d "$work/run-gate/pixel-9a" ]

export FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM="$confirm" FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=$decision
EVIDENCE=physical run mount preflight mount 0 mount
check "host mount: completes, exit 0" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 2 ]' sh "$status" "$runs"
check "host mount: both sessions carry the exact body" sh -c 'cmp -s "$1/body.1" "$2" && cmp -s "$1/body.2" "$2"' sh "$ctl" "$probe"
check "host mount: header binds the preflight partition and the accepted service" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=mount\nproof_role=efs\nproof_dev=8:12\nproof_devname=sda12\nproof_fstype=f2fs\nproof_service_sha256=%s" "$2")" ]' sh "$ctl/header.2" "$accepted"
check "host mount: the bound partition is printed before the mount" \
	sh -c 'printf "%s\n" "$1" | grep -Fq "partition efs, device sda12 (8:12), f2fs"' sh "$out"
check "host mount: barrier verified, contract met, clean post-check" \
	sh -c 'for k in write_barrier=verified mount_contract=contract-met post_check=clean mount_observation=no-write-observed anomalies=none; do
		printf "%s\n" "$1" | grep -Fxq "$k" || exit 1; done' sh "$out"
check "host mount: both NV files meet the TOC minimum, with sizes" \
	sh -c 'for k in nv_normal.bin=meets-toc-minimum nv_normal.bin.size=524288 nv_protected.bin=meets-toc-minimum nv_protected.bin.size=1048576 nv_metadata=meets-toc-minimum nv_toc_minimum_bytes=524288; do
		printf "%s\n" "$1" | grep -Fxq "$k" || exit 1; done' sh "$out"
check "host mount: firmware-acceptance caveat recorded" has 'nv_caveat=size-at-least-the-toc-section-does-not-show-cp-firmware-acceptance'
check "host mount: transcript private, no identifiers, names, or link targets" \
	sh -c '[ "$(ls -l "$1" | cut -c1-10)" = "-rw-------" ] && ! grep -Eq "IMEI|356938035643809|factory\.prop|SN0123|secret-target|private-dir|0123abcd" "$1"' sh "$results/mount-latest.txt"
check "host mount: physical transcript names the exact target" grep -Fxq 'evidence=physical:google-pixel-9a-tegu-gxq96-mp1-0' "$results/mount-latest.txt"
check "host mount: never claims a CP boot" sh -c '! printf "%s\n" "$1" | grep -qiE "cp_boot=|telephony=supported|authori[sz]ed=yes"' sh "$out"

run mount-short preflight mount-short 0 mount
check "host NV: absent and short files exit 9 after a clean teardown" \
	sh -c '[ "$1" -eq 9 ] && printf "%s\n" "$2" | grep -Fxq nv_metadata=nv_normal.bin:absent,nv_protected.bin:short &&
		printf "%s\n" "$2" | grep -Fxq nv_protected.bin.size=4096 && printf "%s\n" "$2" | grep -Fxq post_check=clean' sh "$status" "$out"
run mount-symlink preflight mount-symlink 0 mount
check "host NV: a symlink or directory fails closed, exit 9" \
	sh -c '[ "$1" -eq 9 ] && printf "%s\n" "$2" | grep -Fxq nv_metadata=nv_normal.bin:symlink,nv_protected.bin:directory' sh "$status" "$out"
run mount-stat-fail preflight mount-stat-fail 0 mount
check "host NV: an unavailable size fails closed, exit 9" has 'nv_metadata=nv_normal.bin:size-unavailable,nv_protected.bin:size-unavailable'
run mount-short-write preflight mount-short-write 0 mount
check "host NV: a write anomaly outranks a short file (exit 8), both reported" \
	sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fq "anomalies=partition-counter-5-changed-by-after" && printf "%s\n" "$2" | grep -Fxq nv_protected.bin=short' sh "$status" "$out"
run mount-umount preflight mount-umount 7 mount
check "host teardown: unmount failure exits 7, NV still classified" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fq "still-mounted" && printf "%s\n" "$2" | grep -Fxq nv_normal.bin=meets-toc-minimum' sh "$status" "$out"
run mount-detach preflight mount-detach 7 mount
check "host teardown: detach failure exits 7" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fq "loop-still-bound,loop-detach-failed"' sh "$status" "$out"
run mount-loop-rw preflight mount-loop-rw 7 mount
check "host barrier: a writable loop is unverified, NV not observed, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq write_barrier=unverified && printf "%s\n" "$2" | grep -Fxq nv_metadata=not-observed' sh "$status" "$out"
run mount-write preflight mount-write 0 mount
check "host barrier: a partition write counter change exits 8" sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq mount_observation=anomaly' sh "$status" "$out"
run mount-loopwarn preflight mount-loopwarn 0 mount
check "host barrier: a rejected write on the loop still stops, exit 8" has 'anomalies=kmsg-write-attempt-on-read-only-loop'
run mount-noflag preflight mount-noflag 0 mount
check "host barrier: mountinfo without norecovery violates the contract" has 'mount_contract=contract-violated:recovery-flag-not-shown'
run mount-leftover preflight mount-leftover 0 mount
check "host post-check: a mount left under the proof root exits 8" sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq post_check=dirty' sh "$status" "$out"
run mount-service-after preflight mount-service-after 0 mount
check "host post-check: a service changed during the session exits 8" has 'anomalies=installed-service-changed-during-session'
run mount-not-loop preflight mount-not-loop 0 mount
check "host NV: lookups skipped off the loop mount are not observed, exit 9" \
	sh -c '[ "$1" -eq 9 ] && printf "%s\n" "$2" | grep -Fxq nv_metadata=not-observed' sh "$status" "$out"
run mount-refused preflight mount-refused 6 mount
check "host refusal: guest refusal exits 6 before any state change" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq state_changed=no && printf "%s\n" "$2" | grep -Fq "(kernel-not-pinned)"' sh "$status" "$out"
for fixture_name in pre-mounted pre-holders pre-ambiguous pre-ext4 pre-service-differs pre-no-stat; do
	run "refuse-$fixture_name" "$fixture_name" '' 0 mount
	check "host refusal: preflight $fixture_name never gets a mount session" sh -c '[ "$1" -eq 6 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
done
for fixture_name in mount-bad-type mount-extra-name mount-imei; do
	run "$fixture_name" preflight "$fixture_name" 0 mount
	check "host malformed: $fixture_name refused, nothing retained" \
		sh -c '[ "$1" -eq 5 ] && ! printf "%s\n" "$2" | grep -Eq "factory\.prop|356938035643809|fifo" && ! grep -rEq "factory\.prop|356938035643809" "$3"' \
		sh "$status" "$out" "$work/run-$fixture_name"
done

# Source and service hash gate, on copies of the workflow.
alt="$work/alt"
alt_pkg="$alt/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
pkg="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
alt_reset() {
	rm -rf "$alt"
	mkdir -p "$alt/tools/pixel-9a" "$alt_pkg"
	cp "$host" "$probe" "$issue34_probe" "$root/tools/pixel-9a/acm-probe.py" "$alt/tools/pixel-9a/"
	cp "$initd" "$pkg/APKBUILD" "$alt_pkg/"
}
alt_run() {
	status=0
	out=$(FERRIX_OUT_DIR="$work/run-alt" FERRIX_PIXEL9A_S5300_NV_METADATA_EVIDENCE=mock sh "$alt/tools/pixel-9a/s5300-nv-metadata.sh" 2>&1) || status=$?
}
alt_reset
printf '# changed\n' >>"$alt/tools/pixel-9a/s5300-nv-metadata-remote.sh"
alt_run
check "hash gate: an unreviewed guest body is refused before the port" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fq "not the reviewed file" && [ ! -d "$3/pixel-9a" ]' sh "$status" "$out" "$work/run-alt"
alt_reset
printf '# changed\n' >>"$alt/tools/pixel-9a/s5300-mount-proof-remote.sh"
alt_run
check "hash gate: a changed issue #34 probe needs a new review" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fq "copied barrier needs a new review"' sh "$status" "$out"
alt_reset
printf '# changed\n' >>"$alt_pkg/ferrix-s5300-mounts.initd"
alt_run
check "hash gate: a changed checked-in service is refused" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fq "not the service accepted"' sh "$status" "$out"
alt_reset
awk '$2 == "ferrix-s5300-mounts.initd" { $1 = "00" } { print }' "$pkg/APKBUILD" >"$alt_pkg/APKBUILD"
alt_run
check "hash gate: a package that does not ship the service is refused" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fq "does not ship"' sh "$status" "$out"
check "hash gate: refused runs write no transcript" [ ! -d "$work/run-alt/pixel-9a" ]

mkdir -p "$work/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$work/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=3 FERRIX_PIXEL9A_S5300_NV_METADATA_EVIDENCE=mock \
	sh "$host" 2>&1) || status=$?
silent_port=$FERRIX_SHELL
acm_guest_stop
check "unreachable: exit 3 with sanitised reason" \
	sh -c '[ "$1" -eq 3 ] && printf "%s\n" "$2" | grep -Fq "transport failed (no-shell-response)"' sh "$status" "$out"
check "privacy: port path is neither printed nor retained" \
	sh -c '! printf "%s\n" "$1" | grep -Fq "$2" && ! grep -rFq "$2" "$3"' sh "$out" "$silent_port" "$work/run-unreachable"

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-s5300-nv-metadata-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-s5300-nv-metadata-test","status":"passed"}\n'
