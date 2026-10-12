#!/bin/sh
# Offline proof for the issue #92 S2 read-only preflight
# (s5300-s2-preflight.md). No device is contacted. Three layers:
#   1. static checks that the guest body only reads, never calls a
#      state-changing helper, and carries the exact init-script CLI;
#   2. the real composed probe (checked-in library + body) with only its
#      roots rewritten into a fixture whose CPIF "nodes" are FIFOs: open(2)
#      on a FIFO with no peer blocks, so any accidental open hangs and fails
#      the bounded run. Every fixture file is snapshotted before and after to
#      prove the probe wrote nothing;
#   3. the host workflow through the real CDC-ACM transport and a fake guest
#      (acm-fake-guest.sh) answering with probe transcripts derived from
#      layer 2.
# Passing this is not physical evidence and does not authorise a CP boot.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
board="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pkg="$board/pmos/device-google-tegu"
mm="$board/pmos/modemmanager-samsung-sit"
lib="$pkg/ferrix-s5300-cpif.sh"
pins="$pkg/s5300-cpif-services.sha256"
host="$root/tools/pixel-9a/s5300-s2-preflight.sh"
probe="$root/tools/pixel-9a/s5300-s2-preflight-remote.sh"
fails=0
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required' >&2; exit 1; }

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
sha() {
	if command -v sha"$1"sum >/dev/null 2>&1; then sha"$1"sum "$2" | awk '{print $1}'; else shasum -a "$1" "$2" | awk '{print $1}'; fi
}
nodes='umts_boot0 umts_ipc0 umts_ipc1 umts_rfs0 oem_ipc0 oem_ipc1 oem_ipc2 oem_ipc3 oem_ipc4 oem_ipc5 oem_ipc6 oem_ipc7'

# --- 1. static ------------------------------------------------------------------

check 'static: host POSIX syntax' sh -n "$host"
check 'static: probe POSIX syntax' sh -n "$probe"
body=$(grep -vE '^[[:space:]]*#' "$probe")
# Code with single-quoted text (awk programs, the CLI and config strings,
# messages) removed, so the scans below see only shell words.
code=$(printf '%s\n' "$body" | python3 -c "import re, sys; sys.stdout.write(re.sub(\"'[^']*'\", lambda m: chr(10) * m.group().count(chr(10)), sys.stdin.read()))")
offending=$(printf '%s\n' "$code" | grep -nwE 'adb|fastboot|ssh|scp|nc|telnet|curl|dmesg|modprobe|insmod|rmmod|mmcli|rc-service|rc-update|rc-status|openrc-run|start-stop-daemon|supervise-daemon|ioctl|mknod|mount|umount|losetup|blockdev|blkid|dd|reboot|flash|erase|rm|mv|cp|touch|mkdir|chmod|chown|ln|kill|tee|sync' || true)
check 'static: the guest body names no device, service, mount, or write verb' [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
redirects=$(printf '%s\n' "$code" | grep -nE '>' | grep -vE '2>/dev/null|>&2' || true)
check 'static: the guest body never redirects into a file' [ -z "$redirects" ]
[ -z "$redirects" ] || printf '%s\n' "$redirects"
# Every filesystem path is built from a library root (cpif_fs, cpif_proc,
# cpif_sys, cpif_run, ...) or a probe variable. The only literal system paths
# left are mount points compared against mountinfo and the /dev name compared
# against a loop's backing_file; neither is opened.
literal_paths=$(printf '%s\n' "$code" | sed -E 's#"?\$\{?(cpif|s2)_[a-z_]+\}?"?/[^ ;|)]*##g' |
	grep -nE '(^|[^A-Za-z0-9_])/(proc|sys|etc|run|lib|usr|mnt|dev|vendor)([^A-Za-z0-9_]|$)' |
	grep -vE '/dev/null|"/mnt/vendor/\$s2_dest"|"/dev/\$s2_devname"' || true)
check 'static: no new roots; the body reaches the system only through the library roots' [ -z "$literal_paths" ]
check 'static: the body never names the /dev root; only library node checks reach /dev' \
	sh -c '! printf "%s\n" "$1" | grep -n "cpif_dev"' sh "$code"
[ -z "$literal_paths" ] || printf '%s\n' "$literal_paths"
calls=$(printf '%s\n' "$code" | grep -nE '(^|[;&|({[:space:]])(cpif_write_arm|cpif_consume_arm|cpif_provision_scratch|checkpath|cpif_modem_preflight|cpif_sit_preflight)([[:space:]]|;|$)' |
	grep -vE '^[0-9]+:(cpif_write_arm|cpif_consume_arm|cpif_provision_scratch|checkpath)\(\) \{ return 1; \}$' || true)
check 'static: the body calls no arm, token, provision, or full-preflight helper' [ -z "$calls" ]
[ -z "$calls" ] || printf '%s\n' "$calls"
check 'static: the body replaces every state-changing library helper with a refusal' \
	sh -c 'for f in cpif_write_arm cpif_consume_arm cpif_provision_scratch checkpath; do grep -qx "$f() { return 1; }" "$1" || exit 1; done' sh "$probe"
check 'static: the body refuses to run without the library' grep -qx '\[ "${cpif_target:-}" = google-pixel-9a-tegu-gxq96-mp1-0 \] || exit 5' "$probe"
dev_paths=$(printf '%s\n' "$body" | grep -nE '/dev/(umts|oem|wwan)' |
	grep -vE "^[0-9]+:s2_modem_args='|^[0-9]+:(boot_dev|rfs_dev|oem_gems_dev)=/dev/" || true)
check 'static: CPIF /dev paths appear only in the init-script CLI and config text' [ -z "$dev_paths" ]
[ -z "$dev_paths" ] || printf '%s\n' "$dev_paths"
extract_config() {
	awk -v var="$2" -v q="'" 'index($0, var "=" q) == 1 { on = 1; $0 = substr($0, length(var) + 3) }
		on { line = $0; if (substr(line, length(line)) == q) { print substr(line, 1, length(line) - 1); exit } print line }' "$1"
}
initd_args=$(sed -n '/google-kleaf-cpif)/,/;;/s/^		command_args="\(.*\)"$/\1/p' "$pkg/exynos-modem.initd")
check 'static: the body CLI is exactly the exynos-modem.initd CPIF command_args' \
	[ "$(sed -n "s/^s2_modem_args='\\(.*\\)'\$/\\1/p" "$probe")" = "$initd_args" ]
check 'static: the body --print-config text is exactly the init script expectation' \
	[ "$(extract_config "$probe" s2_modem_config)" = "$(extract_config "$pkg/exynos-modem.initd" cpif_modem_config)" ]
check 'static: the binary runs only behind the pinned release check' \
	grep -Fq 'if [ "$(s2_apk_version exynos-modem)" = "$cpif_modem_version" ]; then' "$probe"
check 'static: source checks run only if the mounts service is already started' \
	sh -c 'grep -B1 -F "cpif_check_sources)" "$1" | grep -Fq "if service_started ferrix-s5300-mounts; then"' sh "$probe"
check 'static: exactly one guest session, through the CDC-ACM transport' \
	sh -c '[ "$(grep -v "^[[:space:]]*#" "$1" | grep -cE "python3 \"\\\$transport\" --probe \"\\\$composed\" ")" -eq 1 ]' sh "$host"
check 'static: the composed probe is exactly the library followed by the body' \
	grep -Fqx 'cat "$library" "$probe" >"$composed"' "$host"
check 'static: the host never edits capabilities or status' \
	sh -c '! grep -v "^[[:space:]]*#" "$1" | grep -nE "capabilities\.toml|docs/status|rc-service|fastboot|adb "' sh "$host"
check 'static: the host pins the #34-accepted mounts service like the library' \
	sh -c 'h=$(sed -n "s/^accepted_mounts_sha256=//p" "$1"); grep -qx "cpif_mounts_sha256=$h" "$2" && [ "$h" = "$(sed -n "s/^cpif_mounts_sha256=//p" "$2")" ]' sh "$host" "$lib"
check 'static: the host kernel pin follows the library' \
	[ "$(sed -n 's/^pinned_kernel=//p' "$host")" = "$(sed -n 's/^cpif_kernel=//p' "$lib")" ]
check 'static: the host and body name the same pinned files as the library' \
	sh -c 'want=$(sed -n "s/^cpif_pinned_paths='"'"'\(.*\)'"'"'\$/\1/p" "$2" | tr " " "\n" | sed "s#.*/##" | sort | tr "\n" " "); got=$(sed -n "s/^pinned_files='"'"'\(.*\)'"'"'\$/\1/p" "$1" | tr " " "\n" | sort | tr "\n" " "); [ -n "$want" ] && [ "$want" = "$got" ]' sh "$host" "$lib"
check 'static: just recipes run the workflow and this test' \
	sh -c 'grep -Fqx "    @sh \"{{root}}/tools/pixel-9a/s5300-s2-preflight.sh\"" "$1" && grep -Fqx "    @sh \"{{root}}/tools/pixel-9a/tests/s5300-s2-preflight.sh\"" "$1"' sh "$root/justfile"
check 'static: ferrix-s5300-mounts.initd is unchanged (#34)' \
	[ "$(sha 256 "$pkg/ferrix-s5300-mounts.initd")" = "$(sed -n 's/^cpif_mounts_sha256=//p' "$lib")" ]

probe_status=0
cat "$lib" "$probe" | sh >/dev/null 2>&1 || probe_status=$?
check 'probe: refuses a host that is not the tegu board' [ "$probe_status" -eq 4 ]
probe_status=0
sh "$probe" >/dev/null 2>&1 </dev/null || probe_status=$?
check 'probe: refuses to run without the checked-in library' [ "$probe_status" -eq 5 ]

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-s2-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; chmod -R u+rwx "$work" 2>/dev/null; rm -rf "$work"' EXIT HUP INT TERM

# --- 2. the real composed probe against a FIFO fixture ------------------------------

bin="$work/bin"
mkdir -p "$bin"
cat >"$bin/stat" <<'EOF'
#!/usr/bin/env python3
# stat [-L] -c FMT [--] FILE...: FIFOs stand in for CPIF character devices,
# with the device number looked up by the name the path resolves to.
import os, stat, sys
args = sys.argv[1:]
follow = args[:1] == ["-L"]
args = args[1:] if follow else args
if len(args) < 3 or args[0] != "-c":
    sys.exit(64)
fmt, paths = args[1], args[2:]
if paths[:1] == ["--"]:
    paths = paths[1:]
status = 0
for path in paths:
    try:
        st = os.stat(path) if follow else os.lstat(path)
    except OSError:
        status = 1
        continue
    rdev, kind = "", "regular empty file" if st.st_size == 0 else "regular file"
    if stat.S_ISREG(st.st_mode):
        # A regular file named in $FAKE_RDEV/regular stands in for the
        # character device the opener scan holds as its positive control.
        name = os.path.basename(os.path.realpath(path) if follow else path)
        try:
            with open(os.path.join(os.environ["FAKE_RDEV"], "regular", name)) as f:
                rdev = f.read().strip()
            kind = "character special file"
        except OSError:
            pass
    elif stat.S_ISDIR(st.st_mode):
        kind = "directory"
    elif stat.S_ISLNK(st.st_mode):
        kind = "symbolic link"
    elif stat.S_ISCHR(st.st_mode):
        kind = "character special file"
        rdev = "%x %x" % (os.major(st.st_rdev), os.minor(st.st_rdev))
    elif stat.S_ISFIFO(st.st_mode):
        name = os.path.basename(os.path.realpath(path) if follow else path)
        try:
            with open(os.path.join(os.environ["FAKE_RDEV"], name)) as f:
                rdev = f.read().strip()
            kind = "character special file"
        except OSError:
            kind = "fifo"
    major, minor = (rdev.split() + ["", ""])[:2]
    values = {"t": major, "T": minor, "u": str(st.st_uid), "g": str(st.st_gid),
              "a": "%o" % stat.S_IMODE(st.st_mode), "h": str(st.st_nlink), "s": str(st.st_size),
              "n": path, "F": kind}
    out, i = "", 0
    while i < len(fmt):
        if fmt[i] == "%" and i + 1 < len(fmt):
            out += values[fmt[i + 1]]
            i += 2
        else:
            out += fmt[i]
            i += 1
    print(out)
sys.exit(status)
EOF
cat >"$bin/uname" <<'EOF'
#!/bin/sh
[ "$*" = -r ] && { printf '%s\n' "$FAKE_UNAME"; exit 0; }
exit 64
EOF
if ! command -v sha256sum >/dev/null 2>&1; then
	printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' >"$bin/sha256sum"
fi
chmod 755 "$bin"/*
uid=$(id -u)
log="$work/log"

repin() {
	grep -v '^#' "$pins" | while read -r _ path; do
		printf '%s  %s\n' "$(sha 256 "$fx$path")" "$path"
	done >"$fx/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256"
}

# A fresh exact-target fixture at $fx: CP OFFLINE, services stopped, nothing
# mounted, scratch not yet provisioned. The fixture library is the checked-in
# one with only its roots, uid/gid, accepted-mounts pin and the one `-c` node
# test rewritten, and it is also installed as the guest's library.
build_fixture() {
	[ ! -d "$work/fx" ] || chmod -R u+rwx "$work/fx"
	rm -rf "$work/fx"
	mkdir -p "$work/fx"
	fx=$(CDPATH= cd -- "$work/fx" && pwd -P)
	chmod 755 "$fx"
	export FAKE_RDEV="$fx/rdev" FAKE_UNAME=6.1.124-android14-11-ga4662a84a15f FAKE_LOG="$log"
	: >"$log"
	mkdir -p "$fx/etc/ferrix" "$fx/etc/init.d" "$fx/etc/conf.d" "$fx/etc/runlevels/default" "$fx/etc/local.d" \
		"$fx/etc/udev/rules.d" "$fx/usr/lib/ferrix" "$fx/usr/share/ferrix/pixel-9a" "$fx/usr/lib/udev/rules.d" \
		"$fx/usr/sbin" "$fx/lib/apk/db" "$fx/proc/device-tree" "$fx/proc/self/ns" "$fx/proc/100/task/100/fd" \
		"$fx/proc/100/task/101/fd" "$fx/sys/bus/platform/devices" "$fx/sys/bus/platform/drivers/cp_interface" \
		"$fx/sys/class/block" "$fx/dev" "$fx/run/openrc/started" "$fx/run/openrc/daemons" "$fx/rdev" \
		"$fx/mnt/vendor" "$fx/vendor/firmware/uecapconfig" "$fx/vendor/firmware/carrierconfig"
	sed 's/^FERRIX_S5300_KERNEL_BACKEND=.*/FERRIX_S5300_KERNEL_BACKEND=google-kleaf-cpif/' \
		"$pkg/ferrix-backends.env" >"$fx/etc/ferrix/backends.env"
	cat >"$fx/etc/init.d/ferrix-s5300-mounts" <<'EOF'
#!/sbin/openrc-run
mount_root=/mnt/vendor
verify_own_mount() {
	printf 'verify_own_mount %s\n' "$1" >>"$FAKE_LOG"
	[ "$1" != "${FAKE_MOUNT_BAD-}" ]
}
EOF
	sed -e "s#^cpif_fs=.*#cpif_fs=$fx#" -e "s#^cpif_proc=.*#cpif_proc=$fx/proc#" \
		-e "s#^cpif_sys=.*#cpif_sys=$fx/sys#" -e "s#^cpif_dev=.*#cpif_dev=$fx/dev#" \
		-e "s#^cpif_run=.*#cpif_run=$fx/run#" -e "s#^cpif_uid=.*#cpif_uid=$uid#" -e "s#^cpif_gid=.*#cpif_gid=$("$bin/stat" -c %g "$fx")#" \
		-e "s#^cpif_mounts_sha256=.*#cpif_mounts_sha256=$(sha 256 "$fx/etc/init.d/ferrix-s5300-mounts")#" \
		-e 's#\[ ! -L "\$path" \] && \[ -c "\$path" \]#[ ! -L "$path" ] \&\& [ -p "$path" ]#' \
		"$lib" >"$work/lib-fixture.sh"
	cp "$work/lib-fixture.sh" "$fx/usr/lib/ferrix/s5300-cpif.sh"
	cp "$pkg/exynos-modem.initd" "$fx/etc/init.d/exynos-modem"
	cp "$pkg/sit-proxy.initd" "$fx/etc/init.d/sit-proxy"
	cp "$mm/modemmanager.initd" "$fx/etc/init.d/modemmanager"
	cp "$pkg/s5300-userspace-sources.toml" "$fx/usr/share/ferrix/pixel-9a/"
	cp "$pkg/99-google-tegu-sit.rules" "$fx/usr/lib/udev/rules.d/"
	repin
	printf 'P:device-google-tegu\nV:1-r4\n\nP:exynos-modem\nV:0.1.0_git20260731-r3\n\nP:modemmanager-samsung-sit\nV:1.25.95_git20260731-r3\n\n' \
		>"$fx/lib/apk/db/installed"
	cat >"$fx/usr/sbin/exynos-modem" <<EOF
#!/bin/sh
printf 'exynos-modem %s\n' "\$*" >>"\$FAKE_LOG"
cat "$work/exynos-modem.config"
EOF
	chmod 755 "$fx/usr/sbin/exynos-modem"
	extract_config "$pkg/exynos-modem.initd" cpif_modem_config >"$work/exynos-modem.config"
	printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fx/proc/device-tree/model"
	printf 'google,ZUMA PRO TEGU\000google,zumapro\000' >"$fx/proc/device-tree/compatible"
	printf 'androidboot.serialno = "SECRETSERIAL1"\nandroidboot.slot_suffix = "_b"\nandroidboot.revision = "MP1.0"\n' >"$fx/proc/bootconfig"
	printf 'console=ttyGS1 androidboot.serialno=SECRETSERIAL1\n' >"$fx/proc/cmdline"
	printf 'cpif 1 0 - Live 0x0\nshm_ipc 1 0 - Live 0x0\ncpif_page 1 0 - Live 0x0\n' >"$fx/proc/modules"
	printf '22 1 0:20 / /run rw,nosuid,nodev - tmpfs tmpfs rw,mode=755\n' >"$fx/proc/self/mountinfo"
	ln -s 'pid:[4026531836]' "$fx/proc/self/ns/pid"
	ln -s /dev/null "$fx/proc/100/task/100/fd/0"
	ln -s /dev/null "$fx/proc/100/task/101/fd/0"
	printf 'ModemManager\n' >"$fx/proc/100/comm"
	# Issue #107 positive control: /dev/null as sysfs names it, held on fd 7
	# by the scanning shell (pid 4242 here).
	mkdir -p "$fx/rdev/regular" "$fx/sys/class/mem/null" "$fx/sys/dev/char" "$fx/proc/4242/task/4242/fd"
	: >"$fx/dev/null"
	echo '1 3' >"$fx/rdev/regular/null"
	echo 1:3 >"$fx/sys/class/mem/null/dev"
	ln -s ../../class/mem/null "$fx/sys/dev/char/1:3"
	printf '4242 (sh) S 1 4242 4242 0 -1 4194304\n' >"$fx/proc/self/stat"
	ln -s "$fx/dev/null" "$fx/proc/4242/task/4242/fd/7"
	cp_dev="$fx/sys/bus/platform/devices/cp"
	mkdir -p "$cp_dev/of_node"
	printf 'samsung,exynos-cp\000' >"$cp_dev/of_node/compatible"
	ln -s ../../drivers/cp_interface "$cp_dev/driver"
	echo OFFLINE >"$cp_dev/modem_state"
	minor=0
	for node in $nodes; do
		mkfifo "$fx/dev/$node"
		printf 'e3 %x\n' "$minor" >"$fx/rdev/$node"
		mkdir -p "$fx/sys/class/cpif/$node"
		printf '227:%d\n' "$minor" >"$fx/sys/class/cpif/$node/dev"
		ln -s ../../../class/cpif "$fx/sys/class/cpif/$node/subsystem"
		ln -s "../../class/cpif/$node" "$fx/sys/dev/char/227:$minor"
		minor=$((minor + 1))
	done
	for spec in modem_a:sda10:10 modem_b:sda11:11 efs:sda12:12 persist:sda13:13 modem_userdata:sda14:14; do
		label=${spec%%:*} rest=${spec#*:}
		dev=${rest%%:*}
		mkdir -p "$fx/sys/class/block/$dev/holders"
		printf 'MAJOR=8\nMINOR=%s\nDEVNAME=%s\nDEVTYPE=partition\nPARTNAME=%s\n' "${rest#*:}" "$dev" "$label" >"$fx/sys/class/block/$dev/uevent"
		echo "8:${rest#*:}" >"$fx/sys/class/block/$dev/dev"
		echo 0 >"$fx/sys/class/block/$dev/ro"
	done
	for n in 0 1 2 3; do
		mkdir -p "$fx/sys/class/block/loop$n"
		echo "7:$n" >"$fx/sys/class/block/loop$n/dev"
		echo 0 >"$fx/sys/class/block/loop$n/ro"
	done
	ln -s /etc/init.d/modemmanager "$fx/run/openrc/started/modemmanager"
	ln -s /etc/init.d/ferrix-radio-firmware "$fx/run/openrc/started/ferrix-radio-firmware"
	ln -s /etc/init.d/modemmanager "$fx/etc/runlevels/default/modemmanager"
	printf '# postmarketOS\nmodemmanager_opts="--test-quick-suspend-resume"\n' >"$fx/etc/conf.d/modemmanager"
	printf '# OpenRC\nrc_parallel="NO"\n' >"$fx/etc/rc.conf"
	printf 'SUBSYSTEM=="net", GOTO="end"\n' >"$fx/etc/udev/rules.d/10-net.rules"
	chmod 755 "$fx/run"
	unset FAKE_MOUNT_BAD
}

# The four accepted read-only loop mounts, as a manual ferrix-s5300-mounts
# start would leave them, with the service marked started.
mount_sources() {
	n=0
	for pair in modem_img:sda11:ext4:noload efs:sda12:f2fs:norecovery persist:sda13:f2fs:norecovery modem_userdata:sda14:f2fs:norecovery; do
		dest=${pair%%:*} rest=${pair#*:}
		part=${rest%%:*} rest=${rest#*:}
		mount_loop "$n" "$dest" "$part" "${rest%%:*}" "${rest#*:}"
		n=$((n + 1))
	done
	ln -s /etc/init.d/ferrix-s5300-mounts "$fx/run/openrc/started/ferrix-s5300-mounts"
}
mount_loop() {
	mkdir -p "$fx/sys/class/block/loop$1/loop"
	echo "/dev/$3" >"$fx/sys/class/block/loop$1/loop/backing_file"
	echo 1 >"$fx/sys/class/block/loop$1/ro"
	echo 1 >"$fx/sys/class/block/$3/ro"
	printf '%s 1 7:%s / /mnt/vendor/%s ro,nosuid,nodev,noexec - %s /dev/loop%s ro,%s\n' "$((40 + $1))" "$1" "$2" "$4" "$1" "$5" \
		>>"$fx/proc/self/mountinfo"
}

snapshot() {
	python3 - "$fx" <<'EOF'
import os, sys
root = sys.argv[1]
for base, dirs, files in os.walk(root):
    for name in sorted(dirs + files):
        p = os.path.join(base, name)
        st = os.lstat(p)
        print(p, st.st_mode, st.st_size, st.st_mtime_ns, st.st_uid)
EOF
}

# probe_case NAME [MUTATION]: fresh fixture, mutation, then the composed
# fixture probe within a 15 s bound. Leaves the output in $pout.
pout="$work/probe.out"
probe_case() {
	build_fixture
	[ -z "${2:-}" ] || eval "$2"
	cat "$work/lib-fixture.sh" "$probe" >"$work/composed-fixture.sh"
	snapshot | sort >"$work/before"
	(PATH="$bin:$PATH" sh "$work/composed-fixture.sh") >"$pout" 2>"$work/probe.err" </dev/null &
	pid=$!
	ticks=0
	while kill -0 "$pid" 2>/dev/null && [ "$ticks" -lt 150 ]; do
		sleep 0.1
		ticks=$((ticks + 1))
	done
	hung=no
	if kill -0 "$pid" 2>/dev/null; then
		hung=yes
		kill "$pid" 2>/dev/null || :
		# Give any reader still blocked on a FIFO node an EOF so it exits.
		python3 -c 'import os, sys
for p in sys.argv[1:]:
    try:
        os.close(os.open(p, os.O_WRONLY | os.O_NONBLOCK))
    except OSError:
        pass' "$fx"/dev/* 2>/dev/null || :
	fi
	pstatus=0
	wait "$pid" 2>/dev/null || pstatus=$?
	snapshot | sort >"$work/after"
	check "$1: finishes without opening any FIFO node" [ "$hung" = no ]
	check "$1: exit 0 and complete" sh -c '[ "$1" -eq 0 ] && grep -Fxq probe_complete=yes "$2"' sh "$pstatus" "$pout"
	check "$1: no file in the fixture was created, changed, or removed" cmp -s "$work/before" "$work/after"
	check "$1: the binary ran only with --print-config" sh -c '! grep "^exynos-modem " "$1" | grep -qv -- "--print-config$"' sh "$log"
	check "$1: no serial or command-line value is printed" sh -c '! grep -q SECRETSERIAL1 "$1"' sh "$pout"
}
po() { grep -Fxq "$1" "$pout"; }

build_fixture
cat "$work/lib-fixture.sh" "$probe" >"$work/composed-fixture.sh"
cat "$lib" "$probe" >"$work/composed-checked-in.sh"
check 'fixture probe: only the roots, ids, mounts pin and the one node test differ from the checked-in composition' \
	sh -c '[ "$(diff "$1" "$2" | grep -c "^>")" -eq 9 ]' sh "$work/composed-checked-in.sh" "$work/composed-fixture.sh"

probe_case 'clean pre-mount'
check 'clean: identity, slot, revision, and PID namespace' sh -c 'grep -Fxq slot_suffix=_b "$1" && grep -Fxq slot_source=bootconfig "$1" && grep -Fxq hw_revision=MP1.0 "$1" && grep -Fxq pidns=initial "$1" && grep -Fxq env.target=exact "$1" && grep -Fxq env.backend=google-kleaf-cpif "$1"' sh "$pout"
check 'clean: target, package, CLI, surface, run checks pass' sh -c 'for k in target package cli surface run; do grep -Fxq "check.$k=pass" "$1" || exit 1; done' sh "$pout"
check 'clean: every node passes and the scan is clean' sh -c '[ "$(grep -c "^node\..*=pass$" "$1")" -eq 12 ] && grep -Fxq opener_scan=clean "$1" && grep -Fxq class_node_count=12 "$1"' sh "$pout"
check 'clean: the positive and format controls pass and no foreign device exists' sh -c 'grep -Fxq opener_positive_control=detected "$1" && grep -Fxq opener_format_control=matched "$1" && grep -Fxq opener_major_foreign=0 "$1" && grep -Fxq opener_vanished=0 "$1"' sh "$pout"
check 'clean: on-device pins match and the CP is OFFLINE' sh -c '[ "$(grep -c "^file\..*\.pin=match$" "$1")" -eq 7 ] && grep -Fxq modem_state=OFFLINE "$1" && grep -Fxq cp_driver=cp_interface "$1"' sh "$pout"
check 'clean: services stopped, ModemManager started, no links or overrides' sh -c 'grep -Fxq service.exynos-modem=stopped "$1" && grep -Fxq service.ferrix-s5300-mounts=stopped "$1" && grep -Fxq service.modemmanager=started "$1" && grep -Fxq runlevel.exynos-modem=0 "$1" && grep -Fxq confd.modemmanager=expected "$1" && grep -Fxq rc_conf_mentions=0 "$1" && grep -Fxq udev_overrides=0 "$1"' sh "$pout"
check 'clean: no token, scratch not provisioned, nothing mounted, sources deferred' sh -c 'grep -Fxq arm_token=absent "$1" && grep -Fxq scratch=absent "$1" && grep -Fxq vendor_mounts=0 "$1" && grep -Fxq part.efs.loops=0 "$1" && grep -Fxq part.modem.count=1 "$1" && grep -Fxq check.sources=deferred "$1"' sh "$pout"
check 'clean: the source verify path did not run' sh -c '! grep -q verify_own_mount "$1"' sh "$log"
cp "$pout" "$work/clean.out"

probe_case 'held through an alias' 'mkdir -p "$fx/elsewhere"; mkfifo "$fx/elsewhere/alias"; cp "$fx/rdev/umts_boot0" "$fx/rdev/alias"; ln -s "$fx/elsewhere/alias" "$fx/proc/100/task/100/fd/9"'
check 'alias: held, node and holder name reported, no pid' sh -c 'grep -Fxq opener_scan=held "$1" && grep -Fxq opener_held_nodes=umts_boot0 "$1" && grep -Fxq opener_holder_comms=ModemManager "$1" && grep -Fxq opener_holder_count=1 "$1" && ! grep -q "=100$" "$1"' sh "$pout"
probe_case 'holder comm is private' 'printf "SECRETIMEI1\n" >"$fx/proc/100/comm"; ln -s "$fx/dev/umts_boot0" "$fx/proc/100/task/100/fd/9"'
check 'holder comm: only other leaves the guest' sh -c 'grep -Fxq opener_holder_comms=other "$1" && ! grep -Fq SECRETIMEI1 "$1"' sh "$pout"
probe_case 'held by a non-leader thread' 'ln -s "$fx/dev/umts_rfs0" "$fx/proc/100/task/101/fd/4"'
check 'thread: held' po opener_held_nodes=umts_rfs0
probe_case 'held extra class node' 'mkfifo "$fx/umts_router"; echo "e3 40" >"$fx/rdev/umts_router"; mkdir -p "$fx/sys/class/cpif/umts_router"; echo 227:64 >"$fx/sys/class/cpif/umts_router/dev"; ln -s ../../../class/cpif "$fx/sys/class/cpif/umts_router/subsystem"; ln -s ../../class/cpif/umts_router "$fx/sys/dev/char/227:64"; ln -s "$fx/umts_router" "$fx/proc/100/task/100/fd/5"'
check 'extra class node: counted and held without retaining its name' sh -c 'grep -Fxq class_extra_nodes=1 "$1" && grep -Fxq opener_held_nodes=other "$1" && ! grep -Fq umts_router "$1"' sh "$pout"
probe_case 'unlistable fd directory' 'chmod 000 "$fx/proc/100/task/101/fd"'
check 'unlistable: the scan is incomplete, not clean' po opener_scan=incomplete
probe_case 'positive control not seen' 'rm "$fx/proc/4242/task/4242/fd/7"'
check 'control missing: reported and the scan is incomplete, not clean' sh -c 'grep -Fxq opener_positive_control=missing "$1" && grep -Fxq opener_scan=incomplete "$1"' sh "$pout"
probe_case 'unlistable task directory' 'mkdir -p "$fx/proc/300/task"; chmod 000 "$fx/proc/300/task"'
check 'unlistable task directory: the scan is incomplete' po opener_scan=incomplete
probe_case 'vanished pid' 'ln -s "$fx/gone" "$fx/proc/303"'
check 'vanished pid: skipped and counted, scan still clean' sh -c 'grep -Fxq opener_vanished=1 "$1" && grep -Fxq opener_scan=clean "$1"' sh "$pout"
probe_case 'foreign device under a CPIF major' 'mkfifo "$fx/dev/secretname"; echo "e3 20" >"$fx/rdev/secretname"; mkdir -p "$fx/sys/devices/virtual/misc/secretname"; ln -s ../../../../class/misc "$fx/sys/devices/virtual/misc/secretname/subsystem"; ln -s ../../devices/virtual/misc/secretname "$fx/sys/dev/char/227:32"'
check 'foreign: counted, never named' sh -c 'grep -Fxq opener_major_foreign=2 "$1" && ! grep -q secretname "$1"' sh "$pout"
probe_case 'other PID namespace' 'rm "$fx/proc/self/ns/pid"; ln -s "pid:[4026532001]" "$fx/proc/self/ns/pid"'
check 'pidns: reported and the scan is incomplete' sh -c 'grep -Fxq pidns=other "$1" && grep -Fxq opener_scan=incomplete "$1"' sh "$pout"
probe_case 'online CP' 'echo ONLINE >"$fx/sys/bus/platform/devices/cp/modem_state"'
check 'online: state reported and surface fails' sh -c 'grep -Fxq modem_state=ONLINE "$1" && grep -Fxq check.surface=fail "$1"' sh "$pout"
probe_case 'stale previous run' 'printf "" >"$fx/run/ferrix-s5300-cpif.armed"; echo 1 >"$fx/run/exynos-modem.pid"; mkdir -m 700 "$fx/run/exynos-modem"; : >"$fx/run/exynos-modem/crash"; ln -s /etc/init.d/exynos-modem "$fx/run/openrc/started/exynos-modem"; mkdir -p "$fx/proc/300"; printf "exynos-modem\n" >"$fx/proc/300/comm"'
check 'stale run: token, pidfile, scratch entry, service, and daemon reported' sh -c 'grep -Fxq arm_token=present "$1" && grep -Fxq pidfile.exynos-modem=1 "$1" && grep -Fxq scratch=ok "$1" && grep -Fxq scratch_entries=1 "$1" && grep -Fxq service.exynos-modem=started "$1" && grep -Fxq proc.s5300_daemons=1 "$1"' sh "$pout"
probe_case 'OpenRC overrides' 'printf "rc_need=\"ferrix-s5300-mounts\"\n" >"$fx/etc/conf.d/exynos-modem"; printf "modemmanager_opts=\"\"\nrc_need=\"sit-proxy\"\n" >"$fx/etc/conf.d/modemmanager"; ln -s /etc/init.d/sit-proxy "$fx/etc/runlevels/default/sit-proxy"; ln -s exynos-modem "$fx/etc/init.d/exynos-modem.x"; printf "rc_exynos_modem_need=x\n" >>"$fx/etc/rc.conf"; printf "rc-service exynos-modem start\n" >"$fx/etc/local.d/s5300.start"; printf "KERNEL==\"umts_*\", MODE=\"0666\"\n" >"$fx/etc/udev/rules.d/90-cpif.rules"'
check 'overrides: conf.d, runlevel, instance, rc.conf, local.d, udev all reported' sh -c 'grep -Fxq confd.exynos-modem=override "$1" && grep -Fxq confd.modemmanager=override "$1" && grep -Fxq runlevel.sit-proxy=1 "$1" && grep -Fxq init_instances=1 "$1" && grep -Fxq rc_conf_mentions=1 "$1" && grep -Fxq local_d_mentions=1 "$1" && grep -Fxq udev_overrides=1 "$1"' sh "$pout"
probe_case 'partial mount after a failed start' 'mount_loop 0 modem_img sda11 ext4 noload; mount_loop 1 efs sda12 f2fs norecovery'
check 'partial: stopped service with two loop mounts and BLKROSET reported' sh -c 'grep -Fxq service.ferrix-s5300-mounts=stopped "$1" && grep -Fxq vendor_mounts=2 "$1" && grep -Fxq dest.modem_img.mounts=1 "$1" && grep -Fxq dest.modem_img.source=loop "$1" && grep -Fxq part.modem.loops=1 "$1" && grep -Fxq part.efs.bdev_ro=1 "$1" && grep -Fxq dest.persist.mounts=0 "$1" && grep -Fxq check.sources=deferred "$1"' sh "$pout"
probe_case 'direct partition mount' 'printf "60 1 8:13 / /mnt/vendor/persist rw - f2fs /dev/sda13 rw\n" >>"$fx/proc/self/mountinfo"'
check 'direct: reported as a direct, writable, non-loop mount' sh -c 'grep -Fxq part.persist.direct_mounts=1 "$1" && grep -Fxq dest.persist.source=other "$1" && grep -Fxq dest.persist.ro=no "$1"' sh "$pout"
probe_case 'post-mount' 'mount_sources; mkdir -p "$fx/mnt/vendor/modem_img/images/default" "$fx/mnt/vendor/efs" "$fx/mnt/vendor/persist/modem"; printf fake >"$fx/mnt/vendor/modem_img/images/default/modem.bin"; printf fake >"$fx/mnt/vendor/persist/modem/cpsha"; for nv in nv_normal.bin nv_protected.bin; do python3 -c "import sys; open(sys.argv[1], \"wb\").truncate(524288)" "$fx/mnt/vendor/efs/$nv"; done'
check 'post-mount: all four loops and the accepted verify path pass' sh -c 'grep -Fxq service.ferrix-s5300-mounts=started "$1" && grep -Fxq vendor_mounts=4 "$1" && grep -Fxq check.sources=pass "$1" && [ "$(grep -c "^dest\..*\.source=loop$" "$1")" -eq 4 ] && [ "$(grep -c "^part\..*\.loops=1$" "$1")" -eq 4 ]' sh "$pout"
check 'post-mount: verify_own_mount ran for all four destinations' sh -c '[ "$(grep -c "^verify_own_mount /mnt/vendor/" "$1")" -eq 4 ]' sh "$log"
cp "$pout" "$work/post.out"
probe_case 'post-mount with a short NV file' 'mount_sources; mkdir -p "$fx/mnt/vendor/modem_img/images/default" "$fx/mnt/vendor/efs" "$fx/mnt/vendor/persist/modem"; printf fake >"$fx/mnt/vendor/modem_img/images/default/modem.bin"; printf fake >"$fx/mnt/vendor/persist/modem/cpsha"; : >"$fx/mnt/vendor/efs/nv_normal.bin"; : >"$fx/mnt/vendor/efs/nv_protected.bin"'
check 'post-mount short NV: the sources check fails' po check.sources=fail
probe_case 'installed 1-r2 image' 'printf "# 1-r2 library\n" >>"$fx/usr/lib/ferrix/s5300-cpif.sh"; repin; sed -i.bak "s/^V:1-r4$/V:1-r2/" "$fx/lib/apk/db/installed"; rm "$fx/lib/apk/db/installed.bak"'
check '1-r2: self-consistent on the device, but the installed library differs' sh -c 'grep -Fxq file.s5300-cpif.sh.pin=match "$1" && grep -Fxq check.package=pass "$1" && grep -Fxq pkg.device-google-tegu=1-r2 "$1" && ! grep -Fxq "file.s5300-cpif.sh.sha256=$2" "$1"' sh "$pout" "$(sha 256 "$work/lib-fixture.sh")"
probe_case 'stale exynos-modem release' 'sed -i.bak "s/^V:0.1.0_git20260731-r3$/V:0.1.0_git20260731-r2/" "$fx/lib/apk/db/installed"; rm "$fx/lib/apk/db/installed.bak"'
check 'stale release: the binary is not run' sh -c 'grep -Fxq check.cli=skipped "$1" && ! grep -q "^exynos-modem " "$2"' sh "$pout" "$log"
probe_case 'WWAN backend and no slot' 'sed -i.bak "s/=google-kleaf-cpif$/=linux-google-tegu-wwan/" "$fx/etc/ferrix/backends.env"; rm "$fx/etc/ferrix/backends.env.bak"; printf "androidboot.revision = \"MP1.0\"\n" >"$fx/proc/bootconfig"'
check 'wwan/no slot: reported, target check fails, modem partition not resolved' sh -c 'grep -Fxq env.backend=linux-google-tegu-wwan "$1" && grep -Fxq check.target=fail "$1" && grep -Fxq slot_suffix=none "$1" && grep -Fxq part.modem.count=0 "$1"' sh "$pout"

# --- 3. host workflow through the fake CDC-ACM guest -----------------------------------

expected_sha() { awk -v b="$1" '{ n = split($2, p, "/"); if (p[n] == b) print $1 }' "$pins"; }
# The fake guest answers with layer-2 output, normalised to the checked-in
# package hashes and root.
normalise() {
	sed -e 's/^uid=.*/uid=0/' "$1" >"$2"
	for name in exynos-modem sit-proxy ferrix-s5300-mounts modemmanager s5300-cpif.sh s5300-userspace-sources.toml 99-google-tegu-sit.rules; do
		sed -i.bak "s/^file\\.$name\\.sha256=.*/file.$name.sha256=$(expected_sha "$name")/" "$2"
	done
	rm -f "$2.bak"
}
mkdir -p "$work/fx3"
normalise "$work/clean.out" "$work/fx3/go"
normalise "$work/post.out" "$work/fx3/post"
derive() {
	name=$1
	shift
	sed "$@" "$work/fx3/go" >"$work/fx3/$name"
}
derive skew -e "s/^file\\.s5300-cpif\\.sh\\.sha256=.*/file.s5300-cpif.sh.sha256=$(printf '%064d' 1)/" -e 's/^pkg\.device-google-tegu=.*/pkg.device-google-tegu=1-r2/'
derive online -e 's/^modem_state=OFFLINE$/modem_state=ONLINE/' -e 's/^check\.surface=pass$/check.surface=fail/'
derive held -e 's/^opener_scan=clean$/opener_scan=held/' -e 's/^opener_holder_count=0$/opener_holder_count=1/' -e 's/^opener_held_nodes=none$/opener_held_nodes=umts_boot0/'
derive incomplete-scan -e 's/^opener_scan=clean$/opener_scan=incomplete/'
derive control-missing -e 's/^opener_positive_control=detected$/opener_positive_control=missing/'
derive format-mismatch -e 's/^opener_format_control=matched$/opener_format_control=mismatch/'
derive foreign -e 's/^opener_major_foreign=0$/opener_major_foreign=1/'
derive foreign-unknown -e 's/^opener_major_foreign=0$/opener_major_foreign=unknown/'
derive bad-control -e 's/^opener_positive_control=detected$/opener_positive_control=assumed/'
derive missing-control -e '/^opener_positive_control=/d'
derive token -e 's/^arm_token=absent$/arm_token=present/'
derive runlevel -e 's/^runlevel\.exynos-modem=0$/runlevel.exynos-modem=1/'
derive confd -e 's/^confd\.sit-proxy=absent$/confd.sit-proxy=override/'
derive rcconf -e 's/^rc_conf_mentions=0$/rc_conf_mentions=1/'
derive stale-mount -e 's/^vendor_mounts=0$/vendor_mounts=1/' -e 's/^dest\.modem_img\.mounts=0$/dest.modem_img.mounts=1/' -e 's/^part\.modem\.loops=0$/part.modem.loops=1/'
derive started -e 's/^service\.exynos-modem=stopped$/service.exynos-modem=started/'
derive unknown-exynos -e 's/^service\.exynos-modem=stopped$/service.exynos-modem=unknown/'
derive unknown-sit -e 's/^service\.sit-proxy=stopped$/service.sit-proxy=unknown/'
derive pidfile -e 's/^pidfile\.sit-proxy=0$/pidfile.sit-proxy=1/'
derive scratch -e 's/^scratch=absent$/scratch=ok/' -e 's/^scratch_entries=0$/scratch_entries=2/'
derive scratch-list-incomplete -e 's/^scratch=absent$/scratch=ok/' -e 's/^scratch_entries=0$/scratch_entries=unavailable/'
derive scratch-mode -e 's/^scratch=absent$/scratch=wrong-owner-mode/'
derive pidns -e 's/^pidns=initial$/pidns=other/'
derive slot -e 's/^slot_value_count=1$/slot_value_count=2/' -e 's/^slot_suffix=_b$/slot_suffix=none/'
derive kernel -e 's/^kernel_release=.*/kernel_release=6.1.162-android14-11/'
derive backend -e 's/^env\.backend=.*/env.backend=linux-google-tegu-wwan/'
derive revision-absent -e 's/^hw_revision=.*/hw_revision=absent/'
derive revision-other -e 's/^hw_revision=.*/hw_revision=EVT1.0/'
derive failing-mounts -e 's/^service\.ferrix-s5300-mounts=stopped$/service.ferrix-s5300-mounts=failed/'
sed -e 's/^dest\.persist\.mounts=1$/dest.persist.mounts=0/' -e 's/^dest\.persist\.source=loop$/dest.persist.source=none/' \
	-e 's/^dest\.persist\.ro=yes$/dest.persist.ro=none/' -e 's/^vendor_mounts=4$/vendor_mounts=3/' "$work/fx3/post" >"$work/fx3/post-partial"
derive unknown-key -e '/^probe_complete=/i\
imei=SECRETIMEI1'
derive duplicate-key -e '/^probe_complete=/i\
arm_token=absent'
derive missing-key -e '/^part\.efs\.holders=/d'
derive bad-enum -e 's/^service\.sit-proxy=stopped$/service.sit-proxy=crashed/'
derive bad-count -e 's/^vendor_mounts=0$/vendor_mounts=many/'
derive bad-sha -e 's/^file\.sit-proxy\.sha256=.*/file.sit-proxy.sha256=abc/'
derive bad-value -e 's/^cp_driver=.*/cp_driver=..\/cp/'
derive bad-holder-comm -e 's/^opener_holder_comms=.*/opener_holder_comms=SECRETIMEI1/'
derive bad-held-node -e 's/^opener_held_nodes=.*/opener_held_nodes=SECRETIMEI1/'
derive not-complete -e 's/^probe_complete=yes$/probe_complete=no/'
derive qemu -e 's/^dt_model=.*/dt_model=QEMU Virtual Machine/'

recovery="$work/recovery"
mkdir -p "$recovery"
printf 'stock factory archive stand-in\n' >"$recovery/factory.zip"
printf 'factory\tB\t1\tfactory.zip\thttps://example.invalid/f.zip\t%s\t%s\tpage\n' \
	"$(sha 256 "$recovery/factory.zip")" "$(wc -c <"$recovery/factory.zip" | tr -d ' ')" >"$work/recovery.pins"
export FERRIX_PIXEL9A_PINS="$work/recovery.pins" FERRIX_PIXEL9A_RECOVERY_DIR="$recovery"

cat "$lib" "$probe" >"$work/composed-expected.sh"
acm_guest_init "$root" "$work" "$work/composed-expected.sh"
unset FERRIX_PIXEL9A_ACM_TIMEOUT 2>/dev/null || true

status=0
out=''
# run CASE FIXTURE [EXIT [EVIDENCE]]: one fresh fake guest per case.
run() {
	case_dir="$work/run-$1"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl fixture "$work/fx3/$2"
	acm_guest_ctl exit "${3:-0}"
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_S5300_S2_EVIDENCE="${4:-mock}" sh "$host" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	port=$FERRIX_SHELL
	acm_guest_stop
	transcript="$case_dir/pixel-9a/s5300-s2-preflight/preflight-latest.txt"
	summary="$case_dir/pixel-9a/s5300-s2-preflight/summary-latest.json"
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }
reports() { printf '%s\n' "$out" | grep -E "^($1)=" | grep -Fq "$2"; }

run go go
check 'go: exit 0, pre-mount go' sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq preflight_result=go && printf "%s\n" "$2" | grep -Fxq preflight_phase=pre-mount' sh "$status" "$out"
check 'go: exactly one session with the exact composed bytes' sh -c '[ "$1" -eq 1 ] && cmp -s "$2" "$3"' sh "$runs" "$ACM_GUEST_CTL/stdin" "$work/composed-expected.sh"
check 'go: mock never reports a physical gate' has 'preflight_gate=not-evidence-mock'
check 'go: never authorises a CP boot or claims an active workflow' sh -c 'printf "%s\n" "$1" | grep -Fxq cp_boot_authorized=no && printf "%s\n" "$1" | grep -Fxq active_s2_workflow=not-implemented' sh "$out"
check 'go: source checks are deferred until the manual mount start' has 'deferred_until_mounts=sources-verify-own-mount,modem-image-metadata,nv-size-metadata,cpsha-metadata,post-mount-partial-detection'
check 'go: active-workflow steps are listed as deferred' sh -c 'printf "%s\n" "$1" | grep -q "^deferred_to_active_workflow=scratch-provision,arm,"' sh "$out"
check 'go: host recovery evidence is separate and verified' sh -c 'printf "%s\n" "$1" | grep -Fxq host_recovery_factory_archive=verified && grep -Fxq "## host artifact evidence (this host; not a device fact)" "$2"' sh "$out" "$transcript"
check 'go: no blockers, skew, or unknowns' sh -c 'printf "%s\n" "$1" | grep -Fxq device_blockers=none && printf "%s\n" "$1" | grep -Fxq image_skew=none && printf "%s\n" "$1" | grep -Fxq unknown_facts=none' sh "$out"
check 'go: JSON summary parses and records the decision' python3 -c 'import json, sys; d = json.load(open(sys.argv[1])); assert d["preflight_result"] == "go" and d["opener_positive_control"] == "detected" and d["cp_boot_authorized"] is False and d["state_changes"] == "none" and d["evidence"] == "mock" and d["device_blockers"] == [] and "arm" in d["deferred_to_active_workflow"]' "$summary"
check 'go: transcript and summary are private (0600)' sh -c '[ "$(stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1")" = 600 ] && [ "$(stat -c %a "$2" 2>/dev/null || stat -f %Lp "$2")" = 600 ]' sh "$transcript" "$summary"
check 'privacy: port path is neither printed nor retained' sh -c '! printf "%s\n" "$1" | grep -Fq "$2" && ! grep -rFq "$2" "$3"' sh "$out" "$port" "$work/run-go"

run physical go 0 physical
check 'physical: gate is go and labelled with the exact target' sh -c 'printf "%s\n" "$1" | grep -Fxq preflight_gate=go && grep -Fxq evidence=physical:google-pixel-9a-tegu-gxq96-mp1-0 "$2"' sh "$out" "$transcript"

run post post
check 'post-mount: exit 0, phase post-mount, sources pass, nothing deferred to mounts' sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq preflight_phase=post-mount && printf "%s\n" "$2" | grep -Fxq check.sources=pass && printf "%s\n" "$2" | grep -Fxq deferred_until_mounts=none' sh "$status" "$out"

# nogo CASE FIXTURE FRAGMENT: completed run, NO-GO (exit 8) naming FRAGMENT.
nogo() {
	run "$1" "$2"
	check "$1: no-go (exit 8)" sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq preflight_result=no-go' sh "$status" "$out"
	check "$1: names $3" reports 'device_blockers|host_blockers|image_skew' "$3"
}
nogo skew skew image-skew-file-s5300-cpif.sh
check 'skew: image skew lists the library and the device package' has 'image_skew=file-s5300-cpif.sh,pkg-device-google-tegu'
nogo online online cp-not-offline
nogo held held cpif-node-held
nogo incomplete-scan incomplete-scan opener-scan-incomplete
nogo control-missing control-missing opener-positive-control-missing
nogo format-mismatch format-mismatch opener-format-control-mismatch
nogo foreign foreign cpif-major-foreign-device
nogo foreign-unknown foreign-unknown opener-major-enumeration-incomplete
nogo token token arm-token-present
nogo runlevel runlevel runlevel-link-exynos-modem
nogo confd confd openrc-override-confd-sit-proxy
nogo rcconf rcconf openrc-override-rc_conf_mentions
nogo stale-mount stale-mount stale-mount-modem_img
check 'stale-mount: the stale loop is named too' reports device_blockers stale-loop-modem
nogo started started service-exynos-modem-started
nogo unknown-exynos unknown-exynos service-exynos-modem-unknown
nogo unknown-sit unknown-sit service-sit-proxy-unknown
nogo pidfile pidfile stale-pidfile-sit-proxy
nogo scratch scratch scratch-not-empty
nogo scratch-list-incomplete scratch-list-incomplete scratch-list-incomplete
nogo scratch-mode scratch-mode scratch-wrong-owner-mode
nogo pidns pidns not-initial-pid-namespace
nogo slot slot slot-not-exactly-one
nogo kernel kernel kernel-not-pinned
nogo backend backend backend-not-google-kleaf-cpif
nogo revision-other revision-other hw-revision-not-mp1.0
nogo failing-mounts failing-mounts service-ferrix-s5300-mounts-failed
nogo post-partial post-partial partial-mount-persist

run revision-absent revision-absent
check 'revision absent: incomplete (exit 9), not go' sh -c '[ "$1" -eq 9 ] && printf "%s\n" "$2" | grep -Fxq preflight_result=incomplete && printf "%s\n" "$2" | grep -Fxq unknown_facts=hw-revision-not-in-bootconfig' sh "$status" "$out"

mv "$recovery/factory.zip" "$recovery/factory.zip.away"
nogo recovery-missing go host-recovery-missing
check 'recovery missing: device facts stay clean; only the host blocks' has 'device_blockers=none'
printf 'tampered\n' >"$recovery/factory.zip"
nogo recovery-tampered go host-recovery-size-mismatch
mv "$recovery/factory.zip.away" "$recovery/factory.zip"

# Fail closed: malformed, ambiguous, or foreign evidence is refused with no result.
refuse() {
	run "$1" "$2" "${4:-0}"
	check "$1: refused with exit $3" [ "$status" -eq "$3" ]
	check "$1: no result" sh -c '! printf "%s\n" "$1" | grep -q "^preflight_result="' sh "$out"
}
refuse unknown-key unknown-key 5
check 'unknown-key: value neither echoed nor retained' sh -c '! printf "%s\n" "$1" | grep -Fq SECRETIMEI1 && ! grep -rFq SECRETIMEI1 "$2"' sh "$out" "$work/run-unknown-key"
refuse duplicate-key duplicate-key 5
refuse missing-key missing-key 5
refuse bad-enum bad-enum 5
refuse bad-count bad-count 5
refuse bad-sha bad-sha 5
refuse bad-value bad-value 5
refuse bad-holder-comm bad-holder-comm 5
refuse bad-held-node bad-held-node 5
refuse not-complete not-complete 5
refuse bad-control bad-control 5
refuse missing-control missing-control 5
refuse qemu qemu 4
refuse identity go 4 4
refuse no-library go 5 5
refuse guest-error go 3 1
check 'guest-error: refusal recorded in the transcript' sh -c 'grep -Fxq result=refused "$1"/pixel-9a/s5300-s2-preflight/preflight-*.txt' sh "$work/run-guest-error"

mkdir -p "$work/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$work/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=3 FERRIX_PIXEL9A_S5300_S2_EVIDENCE=mock sh "$host" 2>&1) || status=$?
acm_guest_stop
check 'unreachable: exit 3 with a sanitised reason' sh -c '[ "$1" -eq 3 ] && printf "%s\n" "$2" | grep -Fxq "REFUSING pixel-9a S5300 S2 preflight: guest CDC-ACM shell transport failed (no-shell-response)"' sh "$status" "$out"

status=0
FERRIX_OUT_DIR="$work/run-usage" sh "$host" mount >/dev/null 2>&1 || status=$?
check 'usage: there is no active mode' sh -c '[ "$1" -eq 2 ] && [ ! -d "$2/pixel-9a" ]' sh "$status" "$work/run-usage"
status=0
FERRIX_OUT_DIR="$work/run-usage" FERRIX_PIXEL9A_S5300_S2_EVIDENCE=qemu sh "$host" >/dev/null 2>&1 || status=$?
check 'usage: unknown evidence label is refused before any session' [ "$status" -eq 2 ]

# Host input gates, in a copied root so the checked-in files stay untouched.
copy_root() {
	rm -rf "$work/root"
	r="$work/root"
	mkdir -p "$r/tools/pixel-9a" "$r/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu" \
		"$r/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/modemmanager-samsung-sit"
	cp "$host" "$probe" "$root/tools/pixel-9a/acm-probe.py" "$r/tools/pixel-9a/"
	cp "$board/manifest.toml" "$r/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/"
	cp "$pkg/APKBUILD" "$lib" "$pins" "$pkg/ferrix-s5300-mounts.initd" "$r/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/"
	cp "$mm/APKBUILD" "$r/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/modemmanager-samsung-sit/"
}
host_refuses() {
	status=0
	out=$(FERRIX_OUT_DIR="$work/run-host" FERRIX_SHELL=/nonexistent FERRIX_PIXEL9A_S5300_S2_EVIDENCE=mock \
		sh "$work/root/tools/pixel-9a/s5300-s2-preflight.sh" 2>&1) || status=$?
	check "host gate: $1 (exit 6, before any file or session)" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fq "$3" && [ ! -d "$4/pixel-9a" ]' \
		sh "$status" "$out" "$2" "$work/run-host"
}
copy_root
printf '# edit\n' >>"$work/root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-s5300-cpif.sh"
host_refuses 'an edited library' 'does not match its package pin'
copy_root
printf '# edit\n' >>"$work/root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-s5300-mounts.initd"
host_refuses 'an edited mounts service' 'not the service accepted for the #34 gate'
copy_root
rm "$work/root/tools/pixel-9a/s5300-s2-preflight-remote.sh"
host_refuses 'a missing guest body' 'is missing or linked'

mock_gates=''
for file in "$work"/run-*/pixel-9a/s5300-s2-preflight/preflight-*.txt; do
	[ -f "$file" ] || continue
	if grep -Fxq 'evidence=mock' "$file" && grep -Eq '^preflight_gate=(go|no-go|incomplete)$' "$file"; then
		mock_gates=yes
		break
	fi
done
check 'no mock run ever reports a physical gate' [ -z "$mock_gates" ]

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-s5300-s2-preflight-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-s5300-s2-preflight-test","status":"passed"}\n'
