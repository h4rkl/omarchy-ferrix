#!/bin/sh
# Deterministic host test for the shared Android guest bootstrap
# (tools/android/guest/ferrix-waydroid.sh). It proves, against stub tools:
#   - QEMU prepare via 00-ferrix-android.start: offline initialization,
#     renderer materialization, container startup, APK staging (idempotent);
#   - QEMU provision (root-owned session): install, browser role;
#   - Pixel-style provision (session owned by `user`, PipeWire pulse):
#     session-side commands run through su as that user, root-only commands
#     (waydroid shell, lxc-attach) do not, records carry the physical label;
#   - run without a compositor socket fails with a typed record.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script=$root/targets/qemu-aarch64/pmos/local.d/00-ferrix-android.start
helper=$root/tools/android/guest/ferrix-waydroid.sh
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-android-bootstrap.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
mkdir -p "$work/bin" "$work/images" "$work/run" "$work/xdg" "$work/state" "$work/var/lib/waydroid" "$work/usr/share/ferrix/android"
: > "$work/images/system.img"
: > "$work/images/vendor.img"
: > "$work/usr/share/ferrix/android/fulguris.apk"
: > "$work/console"
cat > "$work/properties.ini" <<'PROPS'
[properties]
ro.hardware.gralloc=default
ro.hardware.egl=swiftshader
persist.waydroid.multi_windows=false
ro.sf.lcd_density=400
PROPS

for tool in rc-service logger sleep chown pulseaudio; do
  printf '#!/bin/sh\nexit 0\n' > "$work/bin/$tool"
done
cat > "$work/bin/timeout" <<'EOF'
#!/bin/sh
shift
exec "$@"
EOF
cat > "$work/bin/id" <<'EOF'
#!/bin/sh
printf '%s\n' "${FAKE_USER:-root}"
EOF
# su -s /bin/sh USER -c CMD
cat > "$work/bin/su" <<'EOF'
#!/bin/sh
[ "$1" = -s ] && [ "$4" = -c ] || exit 9
printf 'su %s\n' "$3" >> "$FAKE_STATE/calls"
FAKE_USER=$3 exec sh -c "$5"
EOF
cat > "$work/bin/dbus-daemon" <<'EOF'
#!/bin/sh
printf 'dbus-daemon user=%s\n' "${FAKE_USER:-root}" >> "$FAKE_STATE/calls"
EOF
cat > "$work/bin/pipewire-pulse" <<'EOF'
#!/bin/sh
printf 'pipewire-pulse user=%s xdg=%s\n' "${FAKE_USER:-root}" "$XDG_RUNTIME_DIR" >> "$FAKE_STATE/calls"
EOF
cat > "$work/bin/lxc-attach" <<'EOF'
#!/bin/sh
printf 'lxc-attach user=%s %s\n' "${FAKE_USER:-root}" "$*" >> "$FAKE_STATE/calls"
: > "$FAKE_STATE/role"
EOF
cat > "$work/bin/waydroid" <<'EOF'
#!/bin/sh
set -eu
case "$*" in
  'init -f'*) printf '[waydroid]\nimages_path = %s\n' "$4" > "${FERRIX_ANDROID_CONFIG:?}" ;;
  'upgrade -o')
    cat > "${FERRIX_ANDROID_BASE_PROPS:?}" <<'PROPS'
ro.hardware.gralloc=default
ro.hardware.egl=swiftshader
ro.sf.lcd_density=400
PROPS
    ;;
  'session start')
    printf 'session-start user=%s bus=%s wayland=%s\n' "${FAKE_USER:-root}" "$DBUS_SESSION_BUS_ADDRESS" "$WAYLAND_DISPLAY" >> "$FAKE_STATE/calls" ;;
  'shell -- getprop sys.boot_completed')
    [ "${FAKE_USER:-root}" = root ] || exit 7
    echo 1 ;;
  'shell -- pm path '*)
    if [ -f "$FAKE_STATE/installed" ]; then echo "package:/data/app/base.apk"; else exit 1; fi ;;
  'app install '*)
    printf 'app-install user=%s\n' "${FAKE_USER:-root}" >> "$FAKE_STATE/calls"
    : > "$FAKE_STATE/installed" ;;
  'container unfreeze'|status) ;;
  'shell -- cmd package resolve-activity'*)
    if [ -f "$FAKE_STATE/role" ]; then
      echo "net.slions.fulguris.full.fdroid/acr.browser.lightning.MainActivity"
    else
      echo "android/com.android.internal.app.ResolverActivity"
    fi ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$work/bin"/*

common_env() {
  env PATH="$work/bin:$PATH" FAKE_STATE="$work/state" \
    FERRIX_ANDROID_HELPER="$helper" \
    FERRIX_ANDROID_PROPS="$work/properties.ini" \
    FERRIX_ANDROID_IMAGES="$work/images" \
    FERRIX_ANDROID_CONFIG="$work/var/lib/waydroid/waydroid.cfg" \
    FERRIX_ANDROID_BASE_PROPS="$work/var/lib/waydroid/waydroid_base.prop" \
    FERRIX_ANDROID_FULGURIS_APK="$work/usr/share/ferrix/android/fulguris.apk" \
    FERRIX_ANDROID_CONSOLE="$work/console" \
    FERRIX_ANDROID_RUN_DIR="$work/run" \
    FERRIX_ANDROID_XDG_RUNTIME_DIR="$work/xdg" \
    FERRIX_ANDROID_BOOT_TICKS=3 FERRIX_ANDROID_WAYLAND_TICKS=2 \
    "$@"
}
passed=0
check() {
  grep -q "$1" "$2" || {
    printf 'FAIL android-bootstrap-test: missing %s in %s\n' "$1" "$2" >&2
    cat "$2" >&2
    exit 1
  }
  passed=$((passed + 1))
}
refute() {
  if grep -q "$1" "$2"; then
    printf 'FAIL android-bootstrap-test: unexpected %s in %s\n' "$1" "$2" >&2
    exit 1
  fi
  passed=$((passed + 1))
}

# QEMU prepare through the local.d wrapper, twice (idempotent).
common_env sh "$script"
check '"evidence":"QEMU","event":"waydroid-init".*"status":"ok"' "$work/console"
check '"event":"waydroid-properties".*"status":"materialized".*"density_dpi":400' "$work/console"
check '"event":"waydroid-container".*"status":"started"' "$work/console"
check '"event":"android-app-staged"' "$work/console"
check '"event":"android-binder-devices".*"binderfs"' "$work/console"
check '^ro.sf.lcd_density=400$' "$work/var/lib/waydroid/waydroid_base.prop"
common_env sh "$script"
check '"event":"waydroid-init".*"status":"already-initialized"' "$work/console"

# QEMU provision: root-owned session, system pulse.
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_EVIDENCE=QEMU FERRIX_ANDROID_PULSE=system sh "$helper" provision
check '"evidence":"QEMU","event":"android-app-provision".*"status":"installed".*"browser_role":"default"' "$work/console"
check '^app-install user=root$' "$work/state/calls"
check "^session-start user=root bus=unix:path=$work/xdg/ferrix-android-bus wayland=wayland-0\$" "$work/state/calls"
refute '^su ' "$work/state/calls"

# Pixel-style provision: session owned by `user`, PipeWire pulse.
rm -f "$work/state/installed" "$work/state/role"
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_EVIDENCE=physical:google-pixel-9a-tegu-gxq96-mp1-0 \
  FERRIX_ANDROID_SESSION_USER=user FERRIX_ANDROID_PULSE=pipewire \
  FERRIX_ANDROID_SESSION_BUS="unix:path=$work/xdg/bus" sh "$helper" provision
check '"evidence":"physical:google-pixel-9a-tegu-gxq96-mp1-0","event":"android-app-provision".*"status":"installed".*"browser_role":"default"' "$work/console"
check '"event":"android-boot".*"boot_completed":true.*"session_user":"user"' "$work/console"
check '^app-install user=user$' "$work/state/calls"
check '^dbus-daemon user=user$' "$work/state/calls"
# pipewire-pulse is backgrounded by design; allow it a moment to record.
for _ in 1 2 3 4 5 6 7 8 9 10; do
  grep -q '^pipewire-pulse' "$work/state/calls" && break
  /bin/sleep 0.2
done
check "^pipewire-pulse user=user xdg=$work/xdg\$" "$work/state/calls"
check "^session-start user=user bus=unix:path=$work/xdg/bus wayland=wayland-0\$" "$work/state/calls"
check '^lxc-attach user=root ' "$work/state/calls"
# Already installed: not reinstalled.
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_SESSION_USER=user FERRIX_ANDROID_PULSE=none sh "$helper" provision
check '"event":"android-app-provision".*"status":"present"' "$work/console"
refute '^app-install' "$work/state/calls"

# run without a compositor socket: typed failure, no session start.
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_SESSION_USER=user sh "$helper" run
check '"event":"android-app-provision".*"reason":"no-wayland-socket"' "$work/console"
refute '^session-start' "$work/state/calls"

printf '{"evidence":"host","event":"android-bootstrap-tests","passed":%s,"failed":0}\n' "$passed"
