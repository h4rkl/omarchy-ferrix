#!/bin/sh
# Offline contract tests for the exact Pixel 9a S5300 service integration
# (issue #88, s5300-cpif-integration.md): explicit per-backend CLI in
# exynos-modem.initd and sit-proxy.initd, the manual-only google-kleaf-cpif
# preflight/provision/arm/start path in ferrix-s5300-cpif.sh, the package hash
# pins, the runlevel policy, ModemManager ordering, and the Kleaf composer's
# backend pairing.
#
# The scripts are sourced with OpenRC helpers stubbed and their /proc, /sys,
# /dev, /run, and installed-file roots redirected into a fixture. CPIF nodes
# are FIFOs standing in for character devices (the one `-c` node test is
# rewritten to `-p` in the fixture copy only), so any open() of a node blocks
# and the bounded runner fails the case. No device, modem process, or CP is
# involved; passing this is not physical evidence.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
board="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pkg="$board/pmos/device-google-tegu"
mm="$board/pmos/modemmanager-samsung-sit"
lib="$pkg/ferrix-s5300-cpif.sh"
pins="$pkg/s5300-cpif-services.sha256"
accepted_mounts_sha256=b90d3b41ac175c93e5da5db0858327157da414bc9ca06d403c598108db22f74b
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-s5300-cpif.XXXXXX")
trap 'chmod -R u+rwx "$work" 2>/dev/null; rm -rf "$work"' EXIT HUP INT TERM
fails=0

fail() {
	printf 'FAIL %s\n' "$*"
	fails=$((fails + 1))
}

ok() {
	printf 'ok   %s\n' "$*"
}

sha() {
	if command -v sha"$1"sum >/dev/null 2>&1; then
		sha"$1"sum "$2" | awk '{print $1}'
	else
		shasum -a "$1" "$2" | awk '{print $1}'
	fi
}

cpif_modem_args='--model 5300 --kernel-backend google-kleaf-cpif --uecap-dir /vendor/firmware/uecapconfig --boot-dev /dev/umts_boot0 --rfs-dev /dev/umts_rfs0 --oem-gems-dev /dev/oem_ipc1'
wwan_modem_args='--model 5300 --uecap-dir /vendor/firmware/uecapconfig --boot-dev /dev/umts_boot0 --rfs-dev /dev/umts_rfs0 --oem-gems-dev /dev/umts_oem1'
cpif_sit_args='--kernel-backend google-kleaf-cpif --dev /dev/umts_ipc0 --dev /dev/umts_ipc1 --socket-dir /run/sit-proxy'
wwan_sit_args='--dev /dev/wwan0sit0 --dev /dev/wwan0sit1 --socket-dir /run/sit-proxy'
# What the patched binaries print for the CPIF lines above
# (exynos-modem-cpif-adapter.sh checks the real binaries).
nl='
'
cpif_modem_config="kernel_backend=google-kleaf-cpif${nl}boot_dev=/dev/umts_boot0${nl}rfs_dev=/dev/umts_rfs0${nl}oem_gems_dev=/dev/oem_ipc1${nl}uecap_dir=/vendor/firmware/uecapconfig${nl}scratch_dir=/run/exynos-modem${nl}crash_dump_boot=disabled"
cpif_sit_config="kernel_backend=google-kleaf-cpif${nl}dev.0=/dev/umts_ipc0${nl}dev.1=/dev/umts_ipc1${nl}socket_dir=/run/sit-proxy"
nodes='umts_boot0 umts_ipc0 umts_ipc1 umts_rfs0 oem_ipc0 oem_ipc1 oem_ipc2 oem_ipc3 oem_ipc4 oem_ipc5 oem_ipc6 oem_ipc7'

# --- static contract -----------------------------------------------------------
for script in "$lib" "$pkg/exynos-modem.initd" "$pkg/sit-proxy.initd" "$pkg/ferrix-s5300-mounts.initd" "$mm/modemmanager.initd"; do
	sh -n "$script" || fail "syntax: $script"
done

# The accepted #34 source-mount barrier is byte-for-byte unchanged, and the
# library re-checks exactly that file before reusing its verify path.
[ "$(sha 256 "$pkg/ferrix-s5300-mounts.initd")" = "$accepted_mounts_sha256" ] &&
	ok 'static: ferrix-s5300-mounts is the service accepted for the #34 gate' ||
	fail 'static: ferrix-s5300-mounts changed; the #34 gate must be re-reviewed'
grep -qx "cpif_mounts_sha256=$accepted_mounts_sha256" "$lib" &&
	grep -q "^accepted_service_sha256=$accepted_mounts_sha256\$" "$root/tools/pixel-9a/s5300-nv-metadata.sh" &&
	ok 'static: library and NV workflow pin the same accepted mounts service' ||
	fail 'static: the accepted mounts hash differs between library and NV workflow'

# Pins: each installed path appears exactly once with the checked-in file's
# SHA-256, and the library's required list is exactly the pinned set.
pin_source() {
	case "$1" in
		/etc/init.d/exynos-modem) printf '%s\n' "$pkg/exynos-modem.initd" ;;
		/etc/init.d/sit-proxy) printf '%s\n' "$pkg/sit-proxy.initd" ;;
		/etc/init.d/ferrix-s5300-mounts) printf '%s\n' "$pkg/ferrix-s5300-mounts.initd" ;;
		/etc/init.d/modemmanager) printf '%s\n' "$mm/modemmanager.initd" ;;
		/usr/lib/ferrix/s5300-cpif.sh) printf '%s\n' "$lib" ;;
		/usr/share/ferrix/pixel-9a/s5300-userspace-sources.toml) printf '%s\n' "$pkg/s5300-userspace-sources.toml" ;;
		/usr/lib/udev/rules.d/99-google-tegu-sit.rules) printf '%s\n' "$pkg/99-google-tegu-sit.rules" ;;
		*) return 1 ;;
	esac
}
pinned=$(grep -v '^#' "$pins" | awk '{print $2}' | sort | tr '\n' ' ')
required=$(sed -n "s/^cpif_pinned_paths='\(.*\)'\$/\1/p" "$lib" | tr ' ' '\n' | sort | tr '\n' ' ')
[ -n "$required" ] && [ "$pinned" = "$required" ] &&
	[ "$(grep -vc '^#' "$pins")" -eq "$(printf '%s' "$required" | wc -w | tr -d ' ')" ] &&
	ok 'static: pins name exactly the library-required paths, once each' ||
	fail 'static: pins and the library-required paths differ'
grep -v '^#' "$pins" | while read -r hash path; do
	source=$(pin_source "$path") || { printf 'FAIL static: unexpected pin %s\n' "$path"; exit 1; }
	[ "$(sha 256 "$source")" = "$hash" ] || { printf 'FAIL static: stale pin for %s\n' "$path"; exit 1; }
done && ok 'static: every pin matches its checked-in file' || fail 'static: stale package pins'

# Package recipes: every source checksum matches, the library and pins are
# installed where the library looks, and the modem services are in no runlevel.
for recipe in "$pkg/APKBUILD" "$mm/APKBUILD"; do
	dir=${recipe%/APKBUILD}
	sed -n '/^sha512sums="/,/^"/p' "$recipe" | grep -E '^[0-9a-f]{128}  ' | while read -r hash name; do
		[ -f "$dir/$name" ] || continue # external archives
		[ "$(sha 512 "$dir/$name")" = "$hash" ] || { printf 'FAIL static: stale sha512 for %s\n' "$name"; exit 1; }
	done && ok "static: ${dir##*/} sha512sums match" || fail "static: ${dir##*/} sha512sums are stale"
done
for want in '"$srcdir"/ferrix-s5300-cpif.sh \' '"$pkgdir"/usr/lib/ferrix/s5300-cpif.sh' \
	'"$srcdir"/s5300-cpif-services.sha256 \' '"$pkgdir"/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256'; do
	grep -Fq "$want" "$pkg/APKBUILD" || fail "static: APKBUILD does not install $want"
done
runlevel=$(grep -E '^[[:space:]]*for service in .*; do$' "$pkg/APKBUILD")
case " $runlevel " in
	*' ferrix-s5300-mounts '*|*' exynos-modem '*|*' sit-proxy '*) fail 'static: a modem service is in the default runlevel' ;;
	*' modemmanager '*) ok 'static: no S5300 service is linked into a runlevel (ModemManager stays)' ;;
	*) fail 'static: runlevel list unreadable' ;;
esac
grep -Eq 'rc-update|runlevels' "$board/pmos/exynos-modem/APKBUILD" && fail 'static: exynos-modem recipe links a service' || true
code=$(grep -v '^[[:space:]]*#' "$mm/modemmanager.initd")
printf '%s\n' "$code" | grep -Eq '^[[:space:]]*need .*(ferrix-s5300-mounts|exynos-modem|sit-proxy)' &&
	fail 'static: ModemManager needs (and would start) the S5300 chain' ||
	ok 'static: ModemManager only orders after the S5300 chain'
printf '%s\n' "$code" | grep -qx '	use ferrix-s5300-mounts exynos-modem sit-proxy' ||
	fail 'static: ModemManager lost its S5300 ordering'
# The udev rule still tags only WWAN SIT ports; no CPIF node is handed to
# ModemManager until a reviewed S3 plugin change.
grep -Eq 'cpif|umts_|oem_ipc' "$pkg/99-google-tegu-sit.rules" && fail 'static: udev rule tags CPIF nodes for ModemManager' || true

# Version and kernel pins in the library follow the recipe and the workflows.
modem_ver=$(sed -n 's/^pkgver=//p' "$board/pmos/exynos-modem/APKBUILD")-r$(sed -n 's/^pkgrel=//p' "$board/pmos/exynos-modem/APKBUILD")
grep -qx "cpif_modem_version=$modem_ver" "$lib" && ok "static: library expects the recipe's $modem_ver" ||
	fail "static: library modem version is not $modem_ver"
grep -qx "cpif_kernel=$(sed -n 's/^pinned_kernel=//p' "$root/tools/pixel-9a/s5300-mount-proof.sh")" "$lib" ||
	fail 'static: library kernel pin differs from the mount proof'

# Init scripts: explicit CLI per backend, expected --print-config text, and
# CPIF nodes appear only in command_args and the config text.
for pair in "exynos-modem.initd|$wwan_modem_args" "exynos-modem.initd|$cpif_modem_args" \
	"sit-proxy.initd|$wwan_sit_args" "sit-proxy.initd|$cpif_sit_args"; do
	grep -Fqx "		command_args=\"${pair#*|}\"" "$pkg/${pair%%|*}" || fail "static: ${pair%%|*} lacks: ${pair#*|}"
done
extract_config() {
	awk -v var="$2" -v q="'" 'index($0, var "=" q) == 1 { on = 1; $0 = substr($0, length(var) + 3) }
		on { line = $0; if (substr(line, length(line)) == q) { print substr(line, 1, length(line) - 1); exit } print line }' "$1"
}
[ "$(extract_config "$pkg/exynos-modem.initd" cpif_modem_config)" = "$cpif_modem_config" ] &&
	[ "$(extract_config "$pkg/sit-proxy.initd" cpif_sit_config)" = "$cpif_sit_config" ] &&
	ok 'static: init scripts expect the reviewed --print-config output' ||
	fail 'static: init script --print-config expectations differ from the adapter contract'
for initd in exynos-modem.initd sit-proxy.initd; do
	grep -v '^[[:space:]]*#' "$pkg/$initd" | grep -E 'umts_ipc|oem_ipc' |
		grep -vE '^		command_args=|^(dev\.[01]|oem_gems_dev)=' && fail "static: $initd names a CPIF node outside its CLI" || true
done
# The library touches /dev only through the node path built in
# cpif_check_node, and that path only through file tests and stat.
grep -n 'cpif_dev' "$lib" | grep -vE '^[0-9]+:cpif_dev=/dev$|path="\$cpif_dev/\$node"|^[0-9]+:cpif_control_dev="\$cpif_dev/null"$|^[0-9]+:		devs=\$\(find "\$cpif_dev" ! -type d ! -type l 2>/dev/null\) \|\| echo unreadable$' &&
	fail 'static: library uses /dev outside cpif_check_node, the control, and the /dev walk' || true
# Issue #107: the positive control is the only device the library opens. It
# is opened read-only on fd 7, once, only after the stat(2) check, and never
# written; nothing else names it.
[ "$(grep -c 'cpif_control_dev' "$lib")" -eq 3 ] &&
	grep -qx '		scan=$(cpif_scan_body 7<"$cpif_control_dev") || scan=' "$lib" &&
	grep -Fqx "			[ \"\$(stat -c '%t:%T|%F' -- \"\$cpif_control_dev\" 2>/dev/null | tr 'A-F' 'a-f')\" = \"\$control_key|character special file\" ] &&" "$lib" &&
	[ "$(grep -cE '[0-9]<[^&(]|<>' "$lib")" -eq 1 ] &&
	ok 'static: the positive control is the only opened device, read-only, after its stat check' ||
	fail 'static: the positive control is opened other than once, read-only, after its stat check'
grep -nE 'exec [0-9]|[0-9]>[^&]' "$lib" | grep -v '2>/dev/null' && fail 'static: library opens or writes a numbered descriptor' || true
grep -n '"\$path"' "$lib" | grep -vE '\[ ! -L "\$path" \] && \[ -c "\$path" \]|stat -c .%t %T. "\$path"|eerror ' &&
	fail 'static: library uses a node path other than by file test or stat' || true
grep -Eqn '(<|>|exec|dd |cat |head |od |hexdump).*\$(path|cpif_dev)' "$lib" && fail 'static: library may open a node' || true

# --- stub harness --------------------------------------------------------------
bin="$work/bin"
mkdir -p "$bin"
cat >"$bin/stat" <<'EOF'
#!/usr/bin/env python3
# stat [-L] -c FMT [--] FILE...: FIFOs stand in for CPIF character devices,
# with the device number looked up by the name the path resolves to. Like
# stat(1), it prints every file it can and exits 1 if any failed.
import os, stat, sys
args = sys.argv[1:]
follow = args[:1] == ["-L"]
args = args[1:] if follow else args
if len(args) < 3 or args[0] != "-c":
    sys.exit(64)
fmt, paths = args[1], args[2:]
if paths[:1] == ["--"]:
    paths = paths[1:]
vanish = os.environ.get("FAKE_VANISH", "")
# FAKE_STAT_FLAVOR: gnu (coreutils) and busybox print the same %t/%T/%F/%n
# tokens and differ only in diagnostics; upper, decimal, and padded are
# hostile formats that the opener scan must never read as clean.
flavor = os.environ.get("FAKE_STAT_FLAVOR", "gnu")
status = 0
for path in paths:
    if vanish and path == vanish:
        # The fd is closed between the directory glob and the stat.
        os.unlink(path)
    try:
        st = os.stat(path) if follow else os.lstat(path)
    except OSError as e:
        if flavor == "busybox":
            sys.stderr.write("stat: can't stat '%s': %s\n" % (path, e.strerror))
        else:
            sys.stderr.write("stat: cannot stat '%s': %s\n" % (path, e.strerror))
        status = 1
        continue
    rdev, kind = "", "regular empty file" if st.st_size == 0 else "regular file"
    if stat.S_ISREG(st.st_mode):
        # A regular file named in $FAKE_RDEV/regular stands in for a
        # character device that the scanner may open (the positive control).
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
    if rdev and flavor == "upper":
        major, minor = major.upper(), minor.upper()
    elif rdev and flavor == "decimal":
        major, minor = str(int(major, 16)), str(int(minor, 16))
    elif rdev and flavor == "padded":
        major, minor = major.zfill(4), minor.zfill(4)
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
fx_lib="$work/ferrix-s5300-cpif.sh"

# Re-pin the fixture's installed files, as the package would.
repin() {
	grep -v '^#' "$pins" | while read -r _ path; do
		printf '%s  %s\n' "$(sha 256 "$fx$path")" "$path"
	done >"$fx/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256"
}

# A fresh, passing google-kleaf-cpif fixture at $fx.
build_fixture() {
	[ ! -d "$work/fx" ] || chmod -R u+rwx "$work/fx"
	rm -rf "$work/fx"
	fx="$work/fx"
	mkdir -p "$fx"
	fx=$(CDPATH= cd -- "$fx" && pwd -P)
	chmod 755 "$fx"
	export FAKE_LOG="$fx/log" FAKE_RDEV="$fx/rdev" FAKE_UNAME=6.1.124-android14-11-ga4662a84a15f
	: >"$FAKE_LOG"
	mkdir -p "$fx/etc/ferrix" "$fx/etc/init.d" "$fx/usr/lib/ferrix" "$fx/usr/share/ferrix/pixel-9a" \
		"$fx/usr/lib/udev/rules.d" "$fx/usr/sbin" "$fx/lib/apk/db" "$fx/proc/device-tree" "$fx/proc/self" \
		"$fx/proc/100/task/100/fd" "$fx/proc/100/task/101/fd" "$fx/proc/self/ns" "$fx/sys/bus/platform/devices" "$fx/sys/bus/platform/drivers/cp_interface" \
		"$fx/sys/class" "$fx/dev" "$fx/run" "$fx/rdev" "$fx/mnt/vendor/modem_img/images/default" \
		"$fx/mnt/vendor/efs" "$fx/mnt/vendor/persist/modem" "$fx/vendor/firmware/uecapconfig" \
		"$fx/vendor/firmware/carrierconfig"
	sed 's/^FERRIX_S5300_KERNEL_BACKEND=.*/FERRIX_S5300_KERNEL_BACKEND=google-kleaf-cpif/' \
		"$pkg/ferrix-backends.env" >"$fx/etc/ferrix/backends.env"
	# The accepted mounts service is exercised by s5300-mounts-service.sh; here
	# its verify_own_mount is a stand-in whose hash the fixture library pins.
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
		"$lib" >"$fx_lib"
	[ "$(grep -c "$fx" "$fx_lib")" -eq 5 ] && grep -q '\[ -p "\$path" \]' "$fx_lib" ||
		{ echo 'harness could not redirect the library' >&2; exit 1; }
	cp "$fx_lib" "$fx/usr/lib/ferrix/s5300-cpif.sh"
	for svc in exynos-modem sit-proxy; do
		sed -e "s#/etc/ferrix/backends.env#$fx/etc/ferrix/backends.env#g" \
			-e "s#/usr/lib/ferrix/s5300-cpif.sh#$fx/usr/lib/ferrix/s5300-cpif.sh#g" \
			"$pkg/$svc.initd" >"$work/$svc.initd"
		[ "$(grep -c "$fx" "$work/$svc.initd")" -eq 4 ] || { echo "harness could not redirect $svc" >&2; exit 1; }
		cp "$pkg/$svc.initd" "$fx/etc/init.d/$svc"
	done
	cp "$mm/modemmanager.initd" "$fx/etc/init.d/modemmanager"
	cp "$pkg/s5300-userspace-sources.toml" "$fx/usr/share/ferrix/pixel-9a/"
	cp "$pkg/99-google-tegu-sit.rules" "$fx/usr/lib/udev/rules.d/"
	repin
	printf 'C:Q1\nP:exynos-modem\nV:0.1.0_git20260731-r3\n\nC:Q2\nP:exynos-modem-dev\nV:0.1.0_git20260731-r3\n\n' \
		>"$fx/lib/apk/db/installed"
	for svc in exynos-modem sit-proxy; do
		cat >"$fx/usr/sbin/$svc" <<EOF
#!/bin/sh
printf '$svc %s\n' "\$*" >>"\$FAKE_LOG"
[ "\${FAKE_STALE_BINARY-}" = 1 ] && { echo 'kernel_backend=linux-google-tegu-wwan'; exit 0; }
cat "$fx/$svc.config"
EOF
		chmod 755 "$fx/usr/sbin/$svc"
	done
	printf '%s\n' "$cpif_modem_config" >"$fx/exynos-modem.config"
	printf '%s\n' "$cpif_sit_config" >"$fx/sit-proxy.config"
	printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fx/proc/device-tree/model"
	printf 'google,ZUMA PRO TEGU\000google,zumapro\000' >"$fx/proc/device-tree/compatible"
	printf 'cpif 1 0 - Live 0x0\nshm_ipc 1 0 - Live 0x0\ncpif_page 1 0 - Live 0x0\nbcmdhd 1 0 - Live 0x0\n' \
		>"$fx/proc/modules"
	printf '22 1 0:20 / /run rw,nosuid,nodev - tmpfs tmpfs rw,mode=755\n' >"$fx/proc/self/mountinfo"
	ln -s /dev/null "$fx/proc/100/task/100/fd/0"
	ln -s /dev/null "$fx/proc/100/task/101/fd/0"
	ln -s 'pid:[4026531836]' "$fx/proc/self/ns/pid"
	# Issue #107 positive control: /dev/null as the sysfs mem class names it,
	# held on fd 7 by the scanning shell, which is pid 4242 here.
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
	mkdir -p "$fx/sys/bus/platform/devices/other/of_node"
	printf 'google,other\000' >"$fx/sys/bus/platform/devices/other/of_node/compatible"
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
	chmod 755 "$fx/run"
	mkdir -m 700 "$fx/run/exynos-modem"
	printf fake >"$fx/mnt/vendor/modem_img/images/default/modem.bin"
	printf fake >"$fx/mnt/vendor/persist/modem/cpsha"
	for nv in nv_normal.bin nv_protected.bin; do
		python3 -c 'import sys; open(sys.argv[1], "wb").truncate(524288)' "$fx/mnt/vendor/efs/$nv"
	done
	export FAKE_STARTED='ferrix-s5300-mounts ferrix-radio-firmware'
	unset FAKE_MOUNT_BAD FAKE_STALE_BINARY FAKE_STAT_FLAVOR FAKE_VANISH
}

# run_case NAME want(ok|fail) PATTERN SERVICE ACTION [MUTATION]
# Builds a fresh fixture, applies MUTATION, sources SERVICE's fixture copy
# with OpenRC helpers stubbed, and runs ACTION within a 15 s bound. A FIFO
# node that gets opened blocks, so a timeout fails the case. For `fail`,
# PATTERN must appear in the refusal.
run_case() {
	name=$1 want=$2 pattern=$3 service=$4 action=$5 mutation=${6:-}
	build_fixture
	[ -z "$mutation" ] || eval "$mutation"
	out="$work/case.out"
	(
		PATH="$bin:$fx/usr/sbin:$PATH"
		eerror() { printf 'ERR %s\n' "$*"; }
		einfo() { :; }
		ebegin() { :; }
		eend() { return "${1:-0}"; }
		service_started() { case " $FAKE_STARTED " in *" $1 "*) return 0 ;; esac; return 1; }
		checkpath() {
			printf 'checkpath %s\n' "$*" >>"$FAKE_LOG"
			for target; do :; done
			case "$target" in "$fx"/*) mkdir -p "$target" && chmod "$3" "$target" ;; esac
		}
		. "$work/$service.initd"
		eval "$action"
	) >"$out" 2>&1 &
	pid=$!
	ticks=0
	while kill -0 "$pid" 2>/dev/null; do
		ticks=$((ticks + 1))
		[ "$ticks" -le 150 ] || { kill "$pid" 2>/dev/null; break; }
		sleep 0.1
	done
	status=0
	wait "$pid" || status=$?
	if [ "$ticks" -gt 150 ]; then
		fail "$name: timed out (a CPIF node was opened?)"
		return
	fi
	if grep -E '^(exynos-modem|sit-proxy) ' "$FAKE_LOG" | grep -qv -- '--print-config$'; then
		fail "$name: a modem binary ran without --print-config"
		return
	fi
	case "$want" in
		ok) [ "$status" -eq 0 ] && ok "$name" || { fail "$name (exit $status)"; sed 's/^/     /' "$out"; } ;;
		fail)
			if [ "$status" -ne 0 ] && grep -Fq -- "$pattern" "$out"; then
				ok "$name"
			else
				fail "$name (exit $status, expected refusal '$pattern')"
				sed 's/^/     /' "$out"
			fi
			;;
	esac
}

# --- backend selection -----------------------------------------------------
set_backend() { sed -i.bak "s/^FERRIX_S5300_KERNEL_BACKEND=.*/FERRIX_S5300_KERNEL_BACKEND=$1/" "$fx/etc/ferrix/backends.env"; }
run_case 'wwan: exynos-modem keeps the exact radio-first CLI under supervise-daemon' ok '' exynos-modem \
	'[ "$command_args" = "$wwan_modem_args" ] && [ "$supervisor" = supervise-daemon ] && [ "$respawn_delay" = 2 ] && [ -z "${pidfile-}" ]' \
	'set_backend linux-google-tegu-wwan'
run_case 'wwan: sit-proxy keeps the WWAN SIT nodes under supervise-daemon' ok '' sit-proxy \
	'[ "$command_args" = "$wwan_sit_args" ] && [ "$supervisor" = supervise-daemon ]' 'set_backend linux-google-tegu-wwan'
run_case 'wwan: the CPIF preflight refuses the WWAN backend' fail 'only for FERRIX_S5300_KERNEL_BACKEND=google-kleaf-cpif' \
	exynos-modem preflight 'set_backend linux-google-tegu-wwan'
run_case 'wwan: arm refuses the WWAN backend' fail 'only for FERRIX_S5300_KERNEL_BACKEND' exynos-modem arm \
	'set_backend linux-google-tegu-wwan'
run_case 'cpif: exynos-modem selects the explicit CPIF CLI without a respawning supervisor' ok '' exynos-modem \
	'[ "$command_args" = "$cpif_modem_args" ] && [ -z "${supervisor-}" ] && [ "$command_background" = yes ] && [ "$pidfile" = /run/exynos-modem.pid ]'
run_case 'cpif: sit-proxy selects umts_ipc0/1 without a respawning supervisor' ok '' sit-proxy \
	'[ "$command_args" = "$cpif_sit_args" ] && [ -z "${supervisor-}" ] && [ "$pidfile" = /run/sit-proxy.pid ]'
for backend in cpif '' linux-google-tegu-wwan-x; do
	run_case "refuses backend '$backend' in exynos-modem start" fail "refuses kernel backend '$backend'" exynos-modem start_pre \
		"set_backend '$backend'"
	run_case "refuses backend '$backend' in sit-proxy start" fail "refuses kernel backend '$backend'" sit-proxy start_pre \
		"set_backend '$backend'"
done

# --- OpenRC dependencies -------------------------------------------------------
# depend() is executed with OpenRC's gendepends keywords stubbed to record
# each keyword's services. A `need` starts its target before start_pre, so on
# anything but the legacy WWAN route neither service may `need` the source
# mounts or the CP daemon; they may only `use`/`after` them.
deps='deps() {
	for kw in need use after before config; do eval "$kw() { printf \"%s %s\\n\" $kw \"\$*\" >>\"\$fx/deps\"; }"; done
	: >"$fx/deps"; depend
}
kw() { awk -v k="$1" '"'"'$1 == k { for (i = 2; i <= NF; i++) print $i }'"'"' "$fx/deps" | sort -u | tr "\n" " "; }
has() { case " $(kw "$1") " in *" $2 "*) return 0 ;; esac; return 1; }'
run_case 'deps wwan: exynos-modem keeps need on the source mounts and firmware gate' ok '' exynos-modem \
	"$deps"'; deps && has need ferrix-s5300-mounts && has need ferrix-radio-firmware && has after udev && has before sit-proxy && has config "$fx/etc/ferrix/backends.env"' \
	'set_backend linux-google-tegu-wwan'
run_case 'deps wwan: sit-proxy keeps need on the mounts, exynos-modem, and firmware gate' ok '' sit-proxy \
	"$deps"'; deps && has need ferrix-s5300-mounts && has need exynos-modem && has need ferrix-radio-firmware && has after exynos-modem' \
	'set_backend linux-google-tegu-wwan'
for backend in google-kleaf-cpif '' cpif; do
	run_case "deps '$backend': exynos-modem only uses and orders after the source mounts" ok '' exynos-modem \
		"$deps"'; deps && [ "$(kw need)" = "ferrix-radio-firmware " ] && has use ferrix-s5300-mounts && has after ferrix-s5300-mounts && has before sit-proxy && has config "$fx/etc/ferrix/backends.env"' \
		"set_backend '$backend'"
	run_case "deps '$backend': sit-proxy only uses and orders after the mounts and exynos-modem" ok '' sit-proxy \
		"$deps"'; deps && [ "$(kw need)" = "ferrix-radio-firmware " ] && has use ferrix-s5300-mounts && has use exynos-modem && has after ferrix-s5300-mounts && has after exynos-modem' \
		"set_backend '$backend'"
done
# The stub can tell a reintroduced `need` apart: the same harness flags a CPIF
# depend() that needs the mounts again.
run_case 'deps: a reintroduced CPIF need on the source mounts is detected' fail 'reintroduced' exynos-modem \
	"$deps"'; sed -i.bak "s/^			use ferrix-s5300-mounts\$/			need ferrix-s5300-mounts/" "$work/exynos-modem.initd"; . "$work/exynos-modem.initd"; deps; [ "$(kw need)" = "ferrix-radio-firmware " ] || { echo reintroduced; exit 1; }'

# --- manual-only start ------------------------------------------------------
run_case 'cpif: preflight passes on the exact surface without opening a node' ok '' exynos-modem \
	'preflight && [ ! -e "$fx/run/ferrix-s5300-cpif.armed" ]'
run_case 'cpif: start refuses without arm' fail "run 'rc-service exynos-modem arm' first" exynos-modem start_pre
run_case 'cpif: arm then start consumes the one-shot token' ok '' exynos-modem \
	'arm && [ "$(stat -c "%u %a %s" "$fx/run/ferrix-s5300-cpif.armed")" = "$(id -u) 600 0" ] && start_pre && [ ! -e "$fx/run/ferrix-s5300-cpif.armed" ] && ! start_pre'
run_case 'cpif: arm refuses an existing token' fail 'already exists' exynos-modem 'arm && arm'
run_case 'cpif: a failed preflight does not arm' fail 'not OFFLINE' exynos-modem \
	'arm; status=$?; [ ! -e "$fx/run/ferrix-s5300-cpif.armed" ] && exit $status' \
	'echo ONLINE >"$fx/sys/bus/platform/devices/cp/modem_state"'
run_case 'cpif: start refuses a token not written by arm' fail 'was not written by' exynos-modem start_pre \
	'printf x >"$fx/run/ferrix-s5300-cpif.armed"; chmod 600 "$fx/run/ferrix-s5300-cpif.armed"'
run_case 'cpif: start refuses a symlinked token' fail "arm' first" exynos-modem start_pre \
	': >"$fx/elsewhere"; ln -s "$fx/elsewhere" "$fx/run/ferrix-s5300-cpif.armed"'
run_case 'cpif: a consumed token is gone even when the preflight then fails' fail 'not OFFLINE' exynos-modem \
	'arm && echo BOOTING >"$fx/sys/bus/platform/devices/cp/modem_state" && { start_pre; status=$?; [ ! -e "$fx/run/ferrix-s5300-cpif.armed" ] && exit $status; }'

# --- target, package, and binary refusals -------------------------------------
pf() { run_case "cpif refuses: $1" fail "$2" exynos-modem preflight "$3"; }
pf 'another target' 'refuses FERRIX_TARGET' \
	'sed -i.bak "s/^FERRIX_TARGET=.*/FERRIX_TARGET=qemu-aarch64/" "$fx/etc/ferrix/backends.env"'
pf 'another device tree' 'not the exact tegu MP board' 'printf "ZUMA PRO TEGU EVT\000" >"$fx/proc/device-tree/model"'
pf 'another kernel' 'not the pinned Kleaf' 'export FAKE_UNAME=6.1.162-android14-11'
pf 'a stale installed init script' 'is not the pinned package file' 'echo "# local edit" >>"$fx/etc/init.d/exynos-modem"'
pf 'a modified accepted mounts service' 'is not the pinned package file' 'echo "# edit" >>"$fx/etc/init.d/ferrix-s5300-mounts"'
pf 'a stale ModemManager service' 'is not the pinned package file' 'echo "need sit-proxy" >>"$fx/etc/init.d/modemmanager"'
pf 'a symlinked pinned file' 'is not the pinned package file' \
	'mv "$fx/usr/lib/udev/rules.d/99-google-tegu-sit.rules" "$fx/r"; ln -s "$fx/r" "$fx/usr/lib/udev/rules.d/99-google-tegu-sit.rules"'
pf 'a pin list missing a path' 'exactly once' \
	'grep -v /etc/init.d/sit-proxy "$fx/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256" >"$fx/p"; mv "$fx/p" "$fx/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256"'
pf 'a pin list naming a path twice' 'exactly once' \
	'grep /etc/init.d/sit-proxy "$fx/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256" >"$fx/p"; cat "$fx/p" >>"$fx/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256"'
pf 'a missing pin list' 'service pins are missing' 'rm "$fx/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256"'
pf 'a stale exynos-modem package' "not 0.1.0_git20260731-r3" 'sed -i.bak "3s/-r3/-r2/" "$fx/lib/apk/db/installed"'
pf 'no exynos-modem package' "is 'missing'" 'printf "P:other\nV:1\n" >"$fx/lib/apk/db/installed"'
pf 'a binary without the reviewed CPIF configuration' 'does not resolve the reviewed' 'export FAKE_STALE_BINARY=1'

# --- kernel surface refusals ------------------------------------------------
pf 'a missing cpif module' 'cpif is not loaded' 'sed -i.bak "/^cpif /d" "$fx/proc/modules"'
pf 'a module that is not Live' 'shm_ipc is not loaded' 'sed -i.bak "s/^shm_ipc 1 0 - Live/shm_ipc 1 0 - Loading/" "$fx/proc/modules"'
pf 'a duplicated module line' 'cpif_page is not loaded' 'echo "cpif_page 1 0 - Live 0x0" >>"$fx/proc/modules"'
pf 'two CP platform devices' 'exactly one samsung,exynos-cp' \
	'cp -R "$fx/sys/bus/platform/devices/cp" "$fx/sys/bus/platform/devices/cp2"'
pf 'no CP platform device' 'found 0' 'rm -rf "$fx/sys/bus/platform/devices/cp"'
pf 'an unbound CP device' "bound to 'nothing'" 'rm "$fx/sys/bus/platform/devices/cp/driver"'
pf 'another CP driver' "bound to 's5300_modem'" \
	'rm "$fx/sys/bus/platform/devices/cp/driver"; ln -s ../../drivers/s5300_modem "$fx/sys/bus/platform/devices/cp/driver"'
pf 'an ONLINE CP' "'ONLINE', not OFFLINE" 'echo ONLINE >"$fx/sys/bus/platform/devices/cp/modem_state"'
pf 'an unreadable modem_state' "'unreadable', not OFFLINE" 'rm "$fx/sys/bus/platform/devices/cp/modem_state"'
pf 'a WWAN device' 'a WWAN device exists' 'mkdir -p "$fx/sys/class/wwan/wwan0"'
pf 'a missing node' 'umts_rfs0 is missing' 'rm "$fx/dev/umts_rfs0"'
pf 'a symlinked node' 'oem_ipc1 is missing, a symlink' 'rm "$fx/dev/oem_ipc1"; ln -s umts_ipc0 "$fx/dev/oem_ipc1"'
pf 'a regular-file node' 'umts_boot0 is missing, a symlink, or not a character device' \
	'rm "$fx/dev/umts_boot0"; : >"$fx/dev/umts_boot0"'
pf 'a node in another class' "in class 'misc'" \
	'mv "$fx/sys/class/cpif/umts_ipc1" "$fx/m"; mkdir -p "$fx/sys/class/misc"; mv "$fx/m" "$fx/sys/class/misc/umts_ipc1"'
pf 'an ambiguous node (two classes)' 'exactly one sysfs class entry (found 2)' \
	'mkdir -p "$fx/sys/class/misc/umts_boot0"'
pf 'a node with no class entry' 'exactly one sysfs class entry (found 0)' 'rm -rf "$fx/sys/class/cpif/oem_ipc7"'
pf 'a subsystem mismatch' "subsystem 'misc'" \
	'rm "$fx/sys/class/cpif/umts_rfs0/subsystem"; ln -s ../../../class/misc "$fx/sys/class/cpif/umts_rfs0/subsystem"'
pf 'a device-number mismatch' 'is not the device that sysfs names umts_boot0' 'echo 227:99 >"$fx/sys/class/cpif/umts_boot0/dev"'
pf 'an unreadable device number' 'cannot read the device number' 'rm "$fx/rdev/umts_ipc0"'
pf 'a node already held by a process' '/dev/oem_ipc1 is already open (pid 100)' 'ln -s "$fx/dev/oem_ipc1" "$fx/proc/100/task/100/fd/7"'
# Issue #92 opener-scan hardening: match by device number, read every
# thread's fd table, and fail closed on anything the scan cannot read.
pf 'a node held through another path to the same device' '/dev/umts_boot0 is already open (pid 100)' \
	'mkdir -p "$fx/elsewhere"; mkfifo "$fx/elsewhere/alias"; cp "$fx/rdev/umts_boot0" "$fx/rdev/alias"; ln -s "$fx/elsewhere/alias" "$fx/proc/100/task/100/fd/9"'
pf 'a node held only by a non-leader thread' '/dev/umts_rfs0 is already open (pid 100)' \
	'ln -s "$fx/dev/umts_rfs0" "$fx/proc/100/task/101/fd/4"'
pf 'a held CPIF device outside the twelve E1 names' '/dev/umts_router is already open (pid 100)' \
	'mkfifo "$fx/umts_router"; echo "e3 40" >"$fx/rdev/umts_router"; mkdir -p "$fx/sys/class/cpif/umts_router"; echo 227:64 >"$fx/sys/class/cpif/umts_router/dev"; ln -s ../../../class/cpif "$fx/sys/class/cpif/umts_router/subsystem"; ln -s ../../class/cpif/umts_router "$fx/sys/dev/char/227:64"; ln -s "$fx/umts_router" "$fx/proc/100/task/100/fd/5"'
pf 'an fd directory that cannot be listed' 'could not read every open file' 'chmod 000 "$fx/proc/100/task/101/fd"'
pf 'a live fd that cannot be stated' 'could not read every open file' 'ln -s "$fx/gone" "$fx/proc/100/task/100/fd/8"'
pf 'a scan outside the initial PID namespace' 'initial PID namespace' \
	'rm "$fx/proc/self/ns/pid"; ln -s "pid:[4026532001]" "$fx/proc/self/ns/pid"'
pf 'an unreadable PID namespace' 'initial PID namespace' 'rm "$fx/proc/self/ns/pid"'
run_case 'cpif: an fd closed during the scan is churn, not a refusal' ok '' exynos-modem preflight \
	'ln -s "$fx/gone" "$fx/proc/100/task/100/fd/8"; export FAKE_VANISH="$fx/proc/100/task/100/fd/8"'
run_case 'cpif: a task that exits during the scan is churn, not a refusal' ok '' exynos-modem preflight \
	'mkdir -p "$fx/proc/200/task"; ln -s "$fx/gone" "$fx/proc/200/task/200"'
run_case 'cpif: a held non-CPIF character device is not a holder' ok '' exynos-modem preflight \
	'ln -s /dev/null "$fx/proc/100/task/100/fd/3"; mkfifo "$fx/other"; echo "e4 63" >"$fx/rdev/other"; ln -s "$fx/other" "$fx/proc/100/task/100/fd/6"'

# Issue #107: the positive control, task-directory failures, and foreign
# devices under the CPIF majors. `scan` runs the opener scan over the whole
# cpif class and prints its output; `fact KEY=VALUE` checks one fact.
scan='load_cpif_checks; scan_out=$(cpif_opener_scan $(cpif_class_nodes)) || :; printf "%s\n" "$scan_out"
fact() { printf "%s\n" "$scan_out" | grep -Fqx "fact $1"; }'
run_case 'cpif #107: a clean scan detects its own positive control and matches the CPIF format' ok '' exynos-modem \
	"$scan"'; fact opener_positive_control=detected && fact opener_format_control=matched && fact opener_major_foreign=0 && fact opener_vanished=0 && fact opener_scan=clean && ! printf "%s\n" "$scan_out" | grep -q "^holder "'
run_case 'cpif #107: the control is reported only for the scanning shell, not as a CPIF holder' ok '' exynos-modem \
	"$scan"'; fact opener_scan=clean && [ "$(printf "%s\n" "$scan_out" | grep -c "^fact ")" -eq 5 ]' \
	'ln -s "$fx/dev/null" "$fx/proc/100/task/100/fd/7"'
run_case 'cpif #107: the control descriptor is closed after the scan' ok '' exynos-modem \
	'load_cpif_checks && cpif_holders $(cpif_class_nodes) >/dev/null && ! ( : <&7 ) 2>/dev/null'
pf '#107 a positive control the scan does not see' 'positive control was not detected' 'rm "$fx/proc/4242/task/4242/fd/7"'
pf '#107 a positive control seen only under another pid' 'positive control was not detected' \
	'rm "$fx/proc/4242/task/4242/fd/7"; ln -s "$fx/dev/null" "$fx/proc/100/task/100/fd/7"'
pf '#107 an unreadable /proc/self/stat (own pid unknown)' 'positive control was not detected' 'rm "$fx/proc/self/stat"'
pf '#107 a control device that cannot be opened' 'could not read every open file' 'chmod 000 "$fx/dev/null"'
pf '#107 a symlinked control device (never opened)' 'is not the mem-class null device' \
	'rm "$fx/dev/null"; ln -s umts_boot0 "$fx/dev/null"'
pf '#107 a control device that is a CPIF node (never opened)' 'is not the mem-class null device' \
	'rm "$fx/dev/null"; mkfifo "$fx/dev/null"; echo "e3 0" >"$fx/rdev/null"'
pf '#107 a control under a CPIF major in sysfs (never opened)' 'is not the mem-class null device' \
	'echo 227:3 >"$fx/sys/class/mem/null/dev"'
pf '#107 no sysfs mem-class null device' 'is not the mem-class null device' 'rm "$fx/sys/class/mem/null/dev"'
run_case 'cpif #107: a missing control is never clean' ok '' exynos-modem \
	"$scan"'; fact opener_positive_control=missing && fact opener_scan=incomplete' 'rm "$fx/proc/4242/task/4242/fd/7"'
pf '#107 a pid whose task directory cannot be listed' 'could not read every open file' \
	'mkdir -p "$fx/proc/300/task"; chmod 000 "$fx/proc/300/task"'
pf '#107 a live pid with no task directory' 'could not read every open file' 'mkdir -p "$fx/proc/301"'
pf '#107 a live pid with an empty task directory' 'could not read every open file' 'mkdir -p "$fx/proc/302/task"'
run_case 'cpif #107: a pid that vanished mid-scan is skipped and counted' ok '' exynos-modem \
	"$scan"'; fact opener_scan=clean && fact opener_vanished=2' \
	'ln -s "$fx/gone" "$fx/proc/303"; mkdir -p "$fx/proc/304/task"; ln -s "$fx/gone" "$fx/proc/304/task/304"'
run_case 'cpif #107: a preflight with a vanished pid still passes' ok '' exynos-modem preflight 'ln -s "$fx/gone" "$fx/proc/303"'
pf '#107 a foreign sysfs device under a CPIF major' 'is not the cpif class entry of that name' \
	'mkdir -p "$fx/sys/devices/virtual/misc/secretname"; ln -s ../../../../class/misc "$fx/sys/devices/virtual/misc/secretname/subsystem"; ln -s ../../devices/virtual/misc/secretname "$fx/sys/dev/char/227:32"'
pf '#107 a foreign /dev name under a CPIF major' 'is not the cpif class entry of that name' \
	'mkfifo "$fx/dev/secretname"; echo "e3 20" >"$fx/rdev/secretname"'
pf '#107 a /dev alias of a CPIF device under another name' 'is not the cpif class entry of that name' \
	'mkdir -p "$fx/dev/sub"; mkfifo "$fx/dev/sub/secretname"; cp "$fx/rdev/umts_boot0" "$fx/rdev/secretname"'
pf '#107 a cpif class entry missing from /sys/dev/char' 'could not read every open file' 'rm "$fx/sys/dev/char/227:3"'
run_case 'cpif #107: a foreign device is counted, never named' ok '' exynos-modem \
	"$scan"'; fact opener_major_foreign=2 && fact opener_scan=clean && ! printf "%s\n" "$scan_out" | grep -q secretname' \
	'mkfifo "$fx/dev/secretname"; echo "e3 20" >"$fx/rdev/secretname"; mkdir -p "$fx/sys/devices/virtual/misc/secretname"; ln -s ../../../../class/misc "$fx/sys/devices/virtual/misc/secretname/subsystem"; ln -s ../../devices/virtual/misc/secretname "$fx/sys/dev/char/227:32"'
pf '#107 a held device under a CPIF major outside the cpif class' 'outside the cpif class is already open (pid 100)' \
	'mkfifo "$fx/other"; echo "e3 63" >"$fx/rdev/other"; ln -s "$fx/other" "$fx/proc/100/task/100/fd/6"'
# stat(1) output formats: GNU and BusyBox print the same tokens; a hostile
# format (decimal or zero-padded numbers) makes the scan incomplete, never
# clean, even when it would hide a holder.
for flavor in gnu busybox upper; do
	run_case "cpif #107 stat $flavor: clean with the control detected" ok '' exynos-modem \
		"$scan"'; fact opener_positive_control=detected && fact opener_format_control=matched && fact opener_scan=clean' \
		"export FAKE_STAT_FLAVOR=$flavor"
	pf "#107 stat $flavor: a held node is still detected" '/dev/umts_ipc0 is already open (pid 100)' \
		"export FAKE_STAT_FLAVOR=$flavor; ln -s \"\$fx/dev/umts_ipc0\" \"\$fx/proc/100/task/101/fd/9\""
done
run_case 'cpif #107 stat decimal: the format control fails and a holder is not missed as clean' ok '' exynos-modem \
	"$scan"'; fact opener_format_control=mismatch && fact opener_scan=incomplete && ! fact opener_scan=clean' \
	'export FAKE_STAT_FLAVOR=decimal; ln -s "$fx/dev/umts_ipc0" "$fx/proc/100/task/101/fd/9"'
run_case 'cpif #107 stat padded: the control is not opened and the scan is incomplete' ok '' exynos-modem \
	"$scan"'; fact opener_positive_control=missing && fact opener_format_control=mismatch && fact opener_scan=incomplete' \
	'export FAKE_STAT_FLAVOR=padded'
run_case 'cpif #107 stat decimal: the opener check refuses on its own' fail 'could not read every open file' exynos-modem \
	'load_cpif_checks && cpif_check_unheld $(cpif_class_nodes)' 'export FAKE_STAT_FLAVOR=decimal'
# awk implementations: the same cases with mawk and gawk where installed.
for impl in mawk gawk busybox; do
	if [ "$impl" = busybox ]; then
		command -v busybox >/dev/null 2>&1 && busybox awk 'BEGIN { exit 0 }' 2>/dev/null || { printf 'skip cpif #107 awk %s: not installed\n' "$impl"; continue; }
		printf '#!/bin/sh\nexec busybox awk "$@"\n' >"$bin/awk"
	else
		command -v "$impl" >/dev/null 2>&1 || { printf 'skip cpif #107 awk %s: not installed\n' "$impl"; continue; }
		printf '#!/bin/sh\nexec %s "$@"\n' "$(command -v "$impl")" >"$bin/awk"
	fi
	chmod 755 "$bin/awk"
	run_case "cpif #107 awk $impl: clean with the control detected" ok '' exynos-modem \
		"$scan"'; fact opener_positive_control=detected && fact opener_scan=clean'
	pf "#107 awk $impl: a held node through an alias" '/dev/umts_boot0 is already open (pid 100)' \
		'mkdir -p "$fx/elsewhere"; mkfifo "$fx/elsewhere/alias"; cp "$fx/rdev/umts_boot0" "$fx/rdev/alias"; ln -s "$fx/elsewhere/alias" "$fx/proc/100/task/100/fd/9"'
	pf "#107 awk $impl: a missing control" 'positive control was not detected' 'rm "$fx/proc/4242/task/4242/fd/7"'
	pf "#107 awk $impl: a foreign device" 'is not the cpif class entry of that name' 'mkfifo "$fx/dev/secretname"; echo "e3 20" >"$fx/rdev/secretname"'
	rm -f "$bin/awk"
done

# --- source mounts and inputs -----------------------------------------------
pf 'unstarted source mounts' 'start ferrix-s5300-mounts manually' 'export FAKE_STARTED=ferrix-radio-firmware'
pf 'a source mount that fails the accepted verify path' 'not a verified read-only loop mount' \
	'export FAKE_MOUNT_BAD=/mnt/vendor/persist'
pf 'a mounts service that is not the accepted file' 'not the service accepted for the #34 gate' \
	'sed -i.bak "s/^cpif_mounts_sha256=.*/cpif_mounts_sha256=0000/" "$fx/usr/lib/ferrix/s5300-cpif.sh"; repin'
pf 'a missing modem image' 'images/default/modem.bin' 'rm "$fx/mnt/vendor/modem_img/images/default/modem.bin"'
pf 'a symlinked NV file' 'efs/nv_normal.bin' \
	'mv "$fx/mnt/vendor/efs/nv_normal.bin" "$fx/nv"; ln -s "$fx/nv" "$fx/mnt/vendor/efs/nv_normal.bin"'
pf 'a short NV file' 'nv_protected.bin is smaller than 524288' \
	'python3 -c "import sys; open(sys.argv[1], \"wb\").truncate(524287)" "$fx/mnt/vendor/efs/nv_protected.bin"'
pf 'a missing cpsha' 'persist/modem/cpsha' 'rm "$fx/mnt/vendor/persist/modem/cpsha"'
pf 'a missing carrier configuration' '/vendor/firmware/carrierconfig' 'rmdir "$fx/vendor/firmware/carrierconfig"'
run_case 'cpif: the accepted verify path ran for all four destinations' ok '' exynos-modem \
	'preflight && [ "$(grep -c "^verify_own_mount /mnt/vendor/" "$FAKE_LOG")" -eq 4 ]'

# --- private scratch directory ----------------------------------------------
pf '/run not on tmpfs' "/run is 'ext4'" 'sed -i.bak "s/ - tmpfs tmpfs / - ext4 \/dev\/root /" "$fx/proc/self/mountinfo"'
pf 'a group-writable /run' 'group/other-writable' 'chmod 775 "$fx/run"'
pf 'a missing scratch directory' "run 'rc-service exynos-modem provision'" 'rmdir "$fx/run/exynos-modem"'
pf 'a 0755 scratch directory' 'is not a root-owned 0700 directory' 'chmod 755 "$fx/run/exynos-modem"'
pf 'a symlinked scratch directory' 'is not a root-owned 0700 directory' \
	'rmdir "$fx/run/exynos-modem"; mkdir -m 700 "$fx/x"; ln -s "$fx/x" "$fx/run/exynos-modem"'
run_case 'cpif: provision creates only a 0700 scratch directory' ok '' exynos-modem \
	'provision && [ "$(stat -c "%a" "$fx/run/exynos-modem")" = 700 ] && grep -qx "checkpath -d -m 0700 -o root:root $fx/run/exynos-modem" "$FAKE_LOG" && ! grep -E "^(exynos-modem|sit-proxy) " "$FAKE_LOG"' \
	'rmdir "$fx/run/exynos-modem"'
run_case 'cpif: provision refuses a symlink and leaves its target alone' fail 'exists and is not a directory' exynos-modem \
	'provision; status=$?; [ "$(stat -c "%a" "$fx/x")" = 755 ] && ! grep -q checkpath "$FAKE_LOG" && exit $status' \
	'rmdir "$fx/run/exynos-modem"; mkdir -m 755 "$fx/x"; ln -s "$fx/x" "$fx/run/exynos-modem"'
run_case 'cpif: provision refuses a non-tmpfs /run' fail "not tmpfs" exynos-modem provision \
	'rmdir "$fx/run/exynos-modem"; : >"$fx/proc/self/mountinfo"'
run_case 'cpif: provision refuses another target' fail 'refuses FERRIX_TARGET' exynos-modem provision \
	'rmdir "$fx/run/exynos-modem"; sed -i.bak "s/^FERRIX_TARGET=.*/FERRIX_TARGET=x/" "$fx/etc/ferrix/backends.env"'

# --- sit-proxy ----------------------------------------------------------------
online='echo ONLINE >"$fx/sys/bus/platform/devices/cp/modem_state"; export FAKE_STARTED="ferrix-s5300-mounts ferrix-radio-firmware exynos-modem"; ln -s "$fx/dev/umts_boot0" "$fx/proc/100/task/100/fd/3"; ln -s "$fx/dev/umts_rfs0" "$fx/proc/100/task/100/fd/4"; ln -s "$fx/dev/oem_ipc1" "$fx/proc/100/task/100/fd/5"'
run_case 'cpif: sit-proxy starts after exynos-modem has the CP ONLINE' ok '' sit-proxy \
	'preflight && start_pre && grep -qx "checkpath -d -m 0755 /run/sit-proxy" "$FAKE_LOG"' "$online"
run_case 'cpif: sit-proxy refuses an OFFLINE CP' fail "'OFFLINE', not ONLINE" sit-proxy start_pre \
	'export FAKE_STARTED="ferrix-s5300-mounts ferrix-radio-firmware exynos-modem"'
run_case 'cpif: sit-proxy refuses without a running exynos-modem' fail 'needs a running exynos-modem' sit-proxy start_pre \
	'echo ONLINE >"$fx/sys/bus/platform/devices/cp/modem_state"'
run_case 'cpif: sit-proxy refuses an FMT node someone else holds' fail '/dev/umts_ipc1 is already open' sit-proxy start_pre \
	"$online; ln -s \"\$fx/dev/umts_ipc1\" \"\$fx/proc/100/task/101/fd/6\""
run_case 'cpif: sit-proxy refuses a missing FMT node' fail 'umts_ipc0 is missing' sit-proxy start_pre \
	"$online; rm \"\$fx/dev/umts_ipc0\""
run_case 'cpif: sit-proxy refuses another kernel' fail 'not the pinned Kleaf' sit-proxy preflight \
	"$online; export FAKE_UNAME=6.1.99"
run_case 'cpif: sit-proxy refuses a stale binary' fail 'does not resolve the reviewed' sit-proxy start_pre \
	"$online; export FAKE_STALE_BINARY=1"

# --- Kleaf composer: backend pairing and runlevel policy -----------------------
PYTHONDONTWRITEBYTECODE=1 python3 - "$root" 2>"$work/compose.err" <<'EOF' && ok 'compose: Kleaf pairs google-kleaf-cpif and strips every S5300 runlevel link' || { fail 'compose: backend pairing or runlevel policy'; cat "$work/compose.err"; }
import importlib.util, sys
from pathlib import Path
root = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("dev_compose", root / "tools/pixel-9a/dev-compose.py")
compose = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compose)
pkg = root / "targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
mm = pkg.parent / "modemmanager-samsung-sit/modemmanager.initd"

def refused(call, *args):
    try:
        call(*args)
    except SystemExit as exit_:
        return exit_.code != 0
    return False

patches = compose.gki_rootfs_patches()
backends = patches["/etc/ferrix/backends.env"]
env = (pkg / "ferrix-backends.env").read_text()
paired = backends(env)
changed = [(a, b) for a, b in zip(env.splitlines(), paired.splitlines()) if a != b]
assert changed == [("FERRIX_S5300_KERNEL_BACKEND=linux-google-tegu-wwan",
                    "FERRIX_S5300_KERNEL_BACKEND=google-kleaf-cpif")], changed
assert len(env.splitlines()) == len(paired.splitlines())
assert backends(paired) == paired
for bad in ("", env.replace("FERRIX_S5300_KERNEL_BACKEND=linux-google-tegu-wwan\n", ""),
            env + "FERRIX_S5300_KERNEL_BACKEND=google-kleaf-cpif\n",
            env.replace("=linux-google-tegu-wwan", "=cpif")):
    assert refused(backends, bad)
# The current ModemManager service is left alone; an older one that `need`s
# the chain loses that line.
current = mm.read_text()
assert patches["/etc/init.d/modemmanager"](current) == current
old = current.replace("\tuse ferrix-s5300-mounts", "\tneed ferrix-s5300-mounts")
assert "need ferrix-s5300-mounts" not in patches["/etc/init.d/modemmanager"](old)
services = compose.apkbuild_default_services(pkg / "APKBUILD")
assert "modemmanager" in services
assert not set(services) & set(compose.RADIO_ROUTE_SERVICES), services
assert set(compose.RADIO_ROUTE_SERVICES) == {"ferrix-s5300-mounts", "exynos-modem", "sit-proxy"}
source = (root / "tools/pixel-9a/dev-compose.py").read_text()
assert "for service in RADIO_ROUTE_SERVICES:\n        link = f\"/etc/runlevels/default/{service}\"\n        editor.remove(link)" in source
assert 'die("the device package links a manual-only S5300 service into a runlevel")' in source
EOF

if [ "$fails" -ne 0 ]; then
	printf '%s check(s) failed\n' "$fails"
	exit 1
fi
printf 'S5300 CPIF service integration: all checks passed\n'
