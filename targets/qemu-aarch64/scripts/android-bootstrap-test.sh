#!/bin/sh
# Deterministic host test for the shared Android guest bootstrap
# (tools/android/guest/ferrix-waydroid.sh). It proves, against stub tools:
#   - QEMU prepare via 00-ferrix-android.start: offline initialization,
#     renderer materialization, container startup, APK staging (idempotent);
#   - QEMU provision (root-owned session): install, browser role;
#   - Pixel-style provision (session owned by `user`, PipeWire pulse):
#     session-side commands run through su as that user, root-only commands
#     (waydroid shell, lxc-attach) do not, records carry the physical label;
#   - run without a compositor socket fails with a typed record;
#   - the Mali renderer round-trip and the /metadata tmpfs LXC include.
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
cat > "$work/bin/pulseaudio" <<'EOF'
#!/bin/sh
printf 'pulseaudio user=%s args=%s\n' "${FAKE_USER:-root}" "$*" >> "$FAKE_STATE/calls"
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
    # Like Waydroid: the renderer comes from waydroid.cfg [properties].
    v() { sed -n "s/^$1[ ]*=[ ]*//p" "${FERRIX_ANDROID_CONFIG:?}" | tail -n 1; }
    printf 'ro.hardware.gralloc=%s\nro.hardware.egl=%s\nro.sf.lcd_density=400\n' \
      "$(v ro.hardware.gralloc)" "$(v ro.hardware.egl)" > "${FERRIX_ANDROID_BASE_PROPS:?}"
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

# Waydroid rewrites its config as `key = value`; a rerun with a stale value
# must replace, not duplicate, the managed options.
cfg="$work/var/lib/waydroid/waydroid.cfg"
sed -i.bak -e 's/^ro.hardware.gralloc=default$/ro.hardware.gralloc = default/' \
  -e 's/^ro.sf.lcd_density=.*/ro.sf.lcd_density = 320/' "$cfg"
common_env sh "$helper" prepare
[ "$(grep -c '^ro.hardware.gralloc' "$cfg")" = 1 ] || { echo 'FAIL android-bootstrap-test: duplicate gralloc option' >&2; exit 1; }
[ "$(grep -c '^ro.sf.lcd_density' "$cfg")" = 1 ] || { echo 'FAIL android-bootstrap-test: duplicate density option' >&2; exit 1; }
check '^ro.sf.lcd_density=400$' "$cfg"

# A config already holding a duplicate (an older helper's rerun) is repaired.
printf 'ro.hardware.gralloc=default\n' >> "$cfg"
common_env sh "$helper" prepare
[ "$(grep -c '^ro.hardware.gralloc' "$cfg")" = 1 ] || { echo 'FAIL android-bootstrap-test: duplicate not repaired' >&2; exit 1; }

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

# Null pulse: a socket-only PulseAudio for Waydroid's mandatory bind mount,
# never an ALSA module (PipeWire keeps the card).
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_SESSION_USER=user FERRIX_ANDROID_PULSE=null sh "$helper" provision
check '^pulseaudio user=user args=-n .*module-native-protocol-unix.*module-null-sink' "$work/state/calls"
refute '^pulseaudio .*alsa' "$work/state/calls"
check '"event":"android-pulse","mode":"null"' "$work/console"

# run without a compositor socket: typed failure, no session start.
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_SESSION_USER=user sh "$helper" run
check '"event":"android-app-provision".*"reason":"no-wayland-socket"' "$work/console"
refute '^session-start' "$work/state/calls"

# Opt-in stock Mali renderer (Pixel 9a route): the extracted overlay lands in
# Waydroid's vendor overlay, its properties override the software ones and
# are materialized; switching back removes exactly those files and keys.
mali="$work/mali"
mkdir -p "$mali/overlay/vendor/lib64/egl" "$work/overlay/vendor/lib64"
: > "$mali/overlay/vendor/lib64/egl/libGLES_mali.so"
: > "$work/overlay/vendor/lib64/keep.so"
printf '[properties]\nro.hardware.egl=mali\nro.hardware.gralloc=pixel\nvendor.mali.base_protected_tls_max=67108864\n' > "$mali/waydroid-mali.prop"
: > "$work/console"
common_env FERRIX_ANDROID_RENDERER=mali FERRIX_ANDROID_MALI_DIR="$mali" \
  FERRIX_ANDROID_OVERLAY="$work/overlay/vendor" sh "$helper" prepare
check '"event":"waydroid-renderer","renderer":"mali","egl":"mali","gralloc":"pixel"' "$work/console"
check '"event":"waydroid-properties".*"status":"materialized".*"renderer":"mali"' "$work/console"
[ -f "$work/overlay/vendor/lib64/egl/libGLES_mali.so" ] || { echo 'FAIL android-bootstrap-test: mali overlay not installed' >&2; exit 1; }
check '^ro.hardware.egl=mali$' "$cfg"
check '^vendor.mali.base_protected_tls_max=67108864$' "$cfg"
check '^ro.hardware.egl=mali$' "$work/var/lib/waydroid/waydroid_base.prop"
: > "$work/console"
common_env FERRIX_ANDROID_RENDERER=swiftshader FERRIX_ANDROID_MALI_DIR="$mali" \
  FERRIX_ANDROID_OVERLAY="$work/overlay/vendor" sh "$helper" prepare
check '"event":"waydroid-renderer","renderer":"swiftshader","egl":"swiftshader","gralloc":"default"' "$work/console"
[ ! -e "$work/overlay/vendor/lib64/egl/libGLES_mali.so" ] || { echo 'FAIL android-bootstrap-test: mali overlay not removed' >&2; exit 1; }
[ -f "$work/overlay/vendor/lib64/keep.so" ] || { echo 'FAIL android-bootstrap-test: unrelated overlay file removed' >&2; exit 1; }
refute '^vendor.mali' "$cfg"
check '^ro.hardware.egl=swiftshader$' "$work/var/lib/waydroid/waydroid_base.prop"
# A requested Mali renderer without an extracted overlay stays in software.
: > "$work/console"
common_env FERRIX_ANDROID_RENDERER=mali FERRIX_ANDROID_MALI_DIR="$work/absent" \
  FERRIX_ANDROID_OVERLAY="$work/overlay/vendor" sh "$helper" prepare
check '"event":"waydroid-renderer","renderer":"swiftshader"' "$work/console"

# Android 14+ needs a writable /metadata for aconfig storage: prepare adds a
# Ferrix-owned LXC include with an optional tmpfs there, exactly once.
lxc="$work/lxc/waydroid"
mkdir -p "$lxc"
printf 'lxc.rootfs.path = /var/lib/waydroid/rootfs\n' > "$lxc/config"
for _ in 1 2; do
  : > "$work/console"
  common_env FERRIX_ANDROID_LXC_PATH="$work/lxc" sh "$helper" prepare
done
check '"event":"waydroid-lxc-extras".*"metadata":"tmpfs"' "$work/console"
check '^lxc.mount.entry = tmpfs metadata tmpfs .*optional 0 0$' "$lxc/config_ferrix"
[ "$(grep -cxF "lxc.include = $lxc/config_ferrix" "$lxc/config")" = 1 ] ||
  { echo 'FAIL android-bootstrap-test: config_ferrix include not present exactly once' >&2; cat "$lxc/config" >&2; exit 1; }
passed=$((passed + 1))

printf '{"evidence":"host","event":"android-bootstrap-tests","passed":%s,"failed":0}\n' "$passed"
