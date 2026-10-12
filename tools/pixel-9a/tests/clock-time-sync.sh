#!/bin/sh
# Device-free test for Pixel 9a time sync (#8): the chrony NetworkManager
# dispatcher hook, its QEMU mirror, and how the device package wires it.
# chronyc is a recorder; the host clock and any host chronyd are untouched.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
pkg="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
hook="$pkg/chrony-onoffline.nm-dispatcher"
qemu_hook="$root/targets/qemu-aarch64/pmos/nm-dispatcher/60-ferrix-chrony-onoffline"

fail() {
	echo "clock-time-sync: $*" >&2
	exit 1
}

for f in "$hook" "$qemu_hook"; do
	[ -f "$f" ] || fail "missing $f"
	sh -n "$f"
done

# --- Packaging ---------------------------------------------------------------
apkbuild="$pkg/APKBUILD"
# chronyd and its runlevel link come from postmarketos-base-ui; repeating
# them would only hide a substrate change.
if grep -Eq '^[[:space:]]*chrony(-openrc)?[[:space:]]*$' "$apkbuild"; then
	fail 'device package depends on chrony, which postmarketos-base-ui already provides'
fi
if grep -Eq 'for service in .*[[:space:]](chronyd|swclock|hwclock)[[:space:];]' "$apkbuild"; then
	fail 'device package links a clock service owned by the pmOS base'
fi
grep -Fq 'install -Dm755 "$srcdir"/chrony-onoffline.nm-dispatcher' "$apkbuild" ||
	fail 'device package does not install the dispatcher hook executable'
grep -Fq '"$pkgdir"/etc/NetworkManager/dispatcher.d/60-ferrix-chrony-onoffline' "$apkbuild" ||
	fail 'device package installs the dispatcher hook at the wrong path'
grep -Fq '"/etc/NetworkManager/dispatcher.d/60-ferrix-chrony-onoffline", 0o755' "$root/tools/pixel-9a/dev-compose.py" ||
	fail 'dev-compose does not stage the dispatcher hook like the package'
cmp -s "$hook" "$qemu_hook" || fail 'QEMU dispatcher hook differs from the device package copy'
[ -x "$qemu_hook" ] || fail 'QEMU dispatcher hook is not executable'
grep -Fq 'nm-dispatcher/60-ferrix-chrony-onoffline' "$root/targets/qemu-aarch64/pmos/finalize-in-container.sh" ||
	fail 'QEMU finalize step does not install the dispatcher hook'

# --- Dispatcher hook ---------------------------------------------------------
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-clock-time-sync.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin"
cat >"$work/bin/chronyc" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$work/chronyc.log"
EOF
chmod 755 "$work/bin/chronyc"
# No chronyd socket on this host: every event exits 0 without calling chronyc.
for event in up down connectivity-change pre-up hostname; do
	PATH="$work/bin:$PATH" sh "$hook" wlan0 "$event" || fail "hook failed for $event without chronyd"
done
[ ! -e "$work/chronyc.log" ] || fail 'hook called chronyc without a chronyd socket'
grep -Fq 'socket=/run/chrony/chronyd.sock' "$hook" || fail 'hook does not use the chronyd Unix socket'
# The same hook with the socket path redirected to a real Unix socket.
if command -v python3 >/dev/null 2>&1; then
	sock="$work/chronyd.sock"
	python3 -c 'import socket,sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$sock"
	sed "s#^socket=.*#socket=$sock#" "$hook" >"$work/hook"
	for event in pre-up hostname vpn-up; do
		PATH="$work/bin:$PATH" sh "$work/hook" wlan0 "$event"
	done
	[ ! -e "$work/chronyc.log" ] || fail 'hook reacted to an event that does not change routes'
	for event in up connectivity-change down dhcp4-change dhcp6-change; do
		PATH="$work/bin:$PATH" sh "$work/hook" wlan0 "$event"
	done
	[ "$(grep -c "^-h $sock onoffline\$" "$work/chronyc.log")" = 5 ] ||
		fail "hook did not run chronyc onoffline once per route event: $(cat "$work/chronyc.log")"
else
	echo 'skip clock-time-sync: python3 not on PATH, socket-path hook stage skipped'
fi

echo 'PASS clock-time-sync'
