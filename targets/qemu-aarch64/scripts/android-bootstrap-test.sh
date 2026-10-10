#!/bin/sh
# Deterministic host test for the shared Android guest bootstrap
# (tools/android/guest/ferrix-waydroid.sh). It proves, against stub tools:
#   - QEMU prepare via 00-ferrix-android.start: offline initialization,
#     renderer materialization, container startup (idempotent);
#   - QEMU provision (root-owned session): the built-in Browser (Jelly) is
#     verified present and never installed, browser role, setup wizard and
#     Android home launcher disabled;
#   - Pixel-style provision (session owned by `user`, PipeWire pulse):
#     session-side commands run through su as that user, root-only commands
#     (waydroid shell, lxc-attach) do not, records carry the physical label;
#   - run without a compositor socket fails with a typed record;
#   - the Mali renderer round-trip and the /metadata tmpfs LXC include;
#   - compositor restarts: the extra-socket directory bind in the LXC include
#     and the in-container link that replaces Waydroid's socket-file bind
#     (run for real against a fake container root);
#   - the hardware Codec2 overlay on top of Mali (never without it), its
#     properties, removal without touching shared files, and its device binds.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script=$root/targets/qemu-aarch64/pmos/local.d/00-ferrix-android.start
helper=$root/tools/android/guest/ferrix-waydroid.sh
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-android-bootstrap.XXXXXX")
# Fake container root for the Wayland link; short, for the sun_path limit.
croot=$(mktemp -d /tmp/fxa.XXXXXX)
trap 'rm -rf "$work" "$croot"' EXIT INT TERM
mkdir -p "$work/bin" "$work/images" "$work/run" "$work/xdg" "$work/state" "$work/var/lib/waydroid"
: > "$work/images/system.img"
: > "$work/images/vendor.img"
# Packages the fake Android image ships: the Browser and both home launchers.
: > "$work/state/pkg-org.lineageos.jelly"
: > "$work/state/pkg-com.android.launcher3"
: > "$work/state/pkg-org.lineageos.trebuchet"
: > "$work/console"
cat > "$work/properties.ini" <<'PROPS'
[properties]
ro.hardware.gralloc=default
ro.hardware.egl=swiftshader
persist.waydroid.multi_windows=true
persist.waydroid.task_streams=true
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
# lxc-attach: the browser role is recorded; the Wayland link script is run
# for real with the container's /run mapped into $FAKE_CROOT (no umount
# there, so the loop just stops).
cat > "$work/bin/lxc-attach" <<'EOF'
#!/bin/sh
printf 'lxc-attach user=%s %s\n' "${FAKE_USER:-root}" "$*" >> "$FAKE_STATE/calls"
case "$*" in
  *add-role-holder*) : > "$FAKE_STATE/role" ;;
  *ferrix-wayland*)
    for last; do :; done
    script=$(printf '%s\n' "$last" | sed "s#/run/#$FAKE_CROOT/run/#g")
    PATH=$FAKE_CROOT/bin:$PATH sh -c "$script" ;;
esac
EOF
cat > "$work/bin/waydroid" <<'EOF'
#!/bin/sh
set -eu
case "$*" in
  'init -f'*) printf '[waydroid]\nimages_path = %s\n' "$4" > "${FERRIX_ANDROID_CONFIG:?}" ;;
  'upgrade -o')
    # Like Waydroid: waydroid_base.prop gets every waydroid.cfg [properties]
    # entry as key=value (the container reads only this file).
    printf 'upgrade\n' >> "$FAKE_STATE/calls"
    awk '/^\[/ { in_props = ($0 == "[properties]"); next }
         in_props && /=/ { k = $0; sub(/[ \t]*=.*/, "", k); v = $0; sub(/^[^=]*=[ \t]*/, "", v); print k "=" v }' \
      "${FERRIX_ANDROID_CONFIG:?}" > "${FERRIX_ANDROID_BASE_PROPS:?}"
    ;;
  'session start')
    printf 'session-start user=%s bus=%s wayland=%s\n' "${FAKE_USER:-root}" "$DBUS_SESSION_BUS_ADDRESS" "$WAYLAND_DISPLAY" >> "$FAKE_STATE/calls" ;;
  'shell -- getprop sys.boot_completed')
    [ "${FAKE_USER:-root}" = root ] || exit 7
    echo 1 ;;
  'shell -- pm path '*)
    if [ -f "$FAKE_STATE/pkg-$5" ]; then echo "package:/product/app/$5/base.apk"; else exit 1; fi ;;
  'app install '*)
    printf 'app-install user=%s\n' "${FAKE_USER:-root}" >> "$FAKE_STATE/calls" ;;
  'container unfreeze'|status) ;;
  'shell -- settings put '*)
    printf 'settings-put %s\n' "$5 $6 $7" >> "$FAKE_STATE/calls"
    [ "$6" != user_setup_complete ] || : > "$FAKE_STATE/setup" ;;
  'shell -- settings get secure user_setup_complete')
    if [ -f "$FAKE_STATE/setup" ]; then echo 1; else echo null; fi ;;
  'shell -- svc power stayon true')
    printf 'stayon\n' >> "$FAKE_STATE/calls" ;;
  'shell -- pm disable '*)
    printf 'pm-disable %s\n' "$5" >> "$FAKE_STATE/calls" ;;
  'shell -- cmd package resolve-activity'*)
    if [ -f "$FAKE_STATE/role" ]; then
      echo "org.lineageos.jelly/org.lineageos.jelly.MainActivity"
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
    FERRIX_ANDROID_CONSOLE="$work/console" \
    FERRIX_ANDROID_RUN_DIR="$work/run" \
    FERRIX_ANDROID_XDG_RUNTIME_DIR="$work/xdg" \
    FERRIX_ANDROID_BOOT_TICKS=3 FERRIX_ANDROID_WAYLAND_TICKS=2 \
    FAKE_CROOT="$croot" \
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
refute '"event":"android-app-staged"' "$work/console"
check '"event":"android-binder-devices".*"binderfs"' "$work/console"
check '^ro.sf.lcd_density=400$' "$work/var/lib/waydroid/waydroid_base.prop"
# Android apps run as per-task windows: both properties reach the file the
# container reads, not only waydroid.cfg.
check '^persist.waydroid.multi_windows=true$' "$work/var/lib/waydroid/waydroid_base.prop"
check '^persist.waydroid.task_streams=true$' "$work/var/lib/waydroid/waydroid_base.prop"
common_env sh "$script"
check '"event":"waydroid-init".*"status":"already-initialized"' "$work/console"

# QEMU provision: root-owned session, system pulse.
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_EVIDENCE=QEMU FERRIX_ANDROID_PULSE=system sh "$helper" provision
check '"evidence":"QEMU","event":"android-app-provision","app":"org.ferrix.browser","status":"present","package":"org.lineageos.jelly","browser_role":"default"' "$work/console"
refute '^app-install' "$work/state/calls"
check '"event":"android-setup","status":"complete","launcher":"disabled"' "$work/console"
check '^pm-disable com.android.launcher3$' "$work/state/calls"
check '^pm-disable org.lineageos.trebuchet$' "$work/state/calls"
check "^lxc-attach user=root .*add-role-holder android.app.role.BROWSER org.lineageos.jelly\$" "$work/state/calls"
check "^session-start user=root bus=unix:path=$work/xdg/ferrix-android-bus wayland=wayland-0\$" "$work/state/calls"
refute '^su ' "$work/state/calls"
# No compositor extra socket in the container: Waydroid's bind stays and the
# record says so. The directory the container binds now exists.
check '"event":"android-wayland-link","status":"unavailable"' "$work/console"
[ -d "$work/xdg/ferrix-android-wayland" ] || { echo 'FAIL android-bootstrap-test: wayland dir not created' >&2; exit 1; }

# Compositor restarts (Pixel 9a, 2026-10-11): Waydroid binds the socket
# *file*; a restarted ferrix-shell's new socket never reached Android. The
# link replaces that bind with a symlink into the directory bind, once.
mkdir -p "$croot/run/xdg" "$croot/run/ferrix-wayland" "$croot/bin"
printf '#!/bin/sh\nexit 1\n' > "$croot/bin/umount"
chmod +x "$croot/bin/umount"
: > "$croot/run/xdg/wayland-0"
python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$croot/run/ferrix-wayland/wayland-0"
for expect in linked already; do
  : > "$work/console"
  common_env sh "$helper" link-wayland
  check "\"event\":\"android-wayland-link\",\"status\":\"$expect\",\"host_dir\":\"$work/xdg/ferrix-android-wayland\",\"socket\":\"/run/xdg/wayland-0\",\"target\":\"/run/ferrix-wayland/wayland-0\"" "$work/console"
done
[ "$(readlink "$croot/run/xdg/wayland-0")" = "$croot/run/ferrix-wayland/wayland-0" ] ||
  { echo 'FAIL android-bootstrap-test: wayland socket not linked' >&2; ls -l "$croot/run/xdg" >&2; exit 1; }
passed=$((passed + 1))
# A failing link (read-only /run/xdg) is a typed failure, not a silent pass.
rm -f "$croot/run/xdg/wayland-0"
: > "$croot/run/xdg/wayland-0"
chmod 0555 "$croot/run/xdg"
: > "$work/console"
common_env sh "$helper" link-wayland
chmod 0755 "$croot/run/xdg"
if [ "$(id -u)" != 0 ]; then
  check '"event":"android-wayland-link","status":"failed"' "$work/console"
fi
rm -f "$croot/run/xdg/wayland-0"

# Waydroid rewrites its config as `key = value`; a rerun with a stale value
# must replace, not duplicate, the managed options.
cfg="$work/var/lib/waydroid/waydroid.cfg"
sed -i.bak -e 's/^ro.hardware.gralloc=default$/ro.hardware.gralloc = default/' \
  -e 's/^ro.sf.lcd_density=.*/ro.sf.lcd_density = 320/' "$cfg"
common_env sh "$helper" prepare
[ "$(grep -c '^ro.hardware.gralloc' "$cfg")" = 1 ] || { echo 'FAIL android-bootstrap-test: duplicate gralloc option' >&2; exit 1; }
[ "$(grep -c '^ro.sf.lcd_density' "$cfg")" = 1 ] || { echo 'FAIL android-bootstrap-test: duplicate density option' >&2; exit 1; }
check '^ro.sf.lcd_density=400$' "$cfg"

# Device regression (2026-10-10): waydroid.cfg already carried
# multi_windows=true but waydroid_base.prop still said false, because the
# offline upgrade only ran for renderer/density drift. A stale managed
# property in the base file must trigger `waydroid upgrade -o`.
base="$work/var/lib/waydroid/waydroid_base.prop"
sed -i.bak 's/^persist.waydroid.multi_windows=true$/persist.waydroid.multi_windows=false/' "$base"
: > "$work/console"
: > "$work/state/calls"
common_env sh "$helper" prepare
check '^upgrade$' "$work/state/calls"
check '"event":"waydroid-properties".*"status":"materialized".*"trigger":"stale"' "$work/console"
check '^persist.waydroid.multi_windows=true$' "$base"
# Changing a property in the target file applies it to waydroid.cfg and
# always regenerates the base file in the same run.
sed -i.bak 's/^persist.waydroid.task_streams=true$/persist.waydroid.task_streams=false/' "$work/properties.ini"
: > "$work/console"
: > "$work/state/calls"
common_env sh "$helper" prepare
check '"event":"waydroid-properties".*"status":"applied"' "$work/console"
check '"event":"waydroid-properties".*"status":"materialized".*"trigger":"properties-applied"' "$work/console"
check '^persist.waydroid.task_streams=false$' "$base"
sed -i.bak 's/^persist.waydroid.task_streams=false$/persist.waydroid.task_streams=true/' "$work/properties.ini"
common_env sh "$helper" prepare
check '^persist.waydroid.task_streams=true$' "$base"
# Up to date: no upgrade.
: > "$work/state/calls"
common_env sh "$helper" prepare
refute '^upgrade$' "$work/state/calls"

# A config already holding a duplicate (an older helper's rerun) is repaired.
printf 'ro.hardware.gralloc=default\n' >> "$cfg"
common_env sh "$helper" prepare
[ "$(grep -c '^ro.hardware.gralloc' "$cfg")" = 1 ] || { echo 'FAIL android-bootstrap-test: duplicate not repaired' >&2; exit 1; }

# Pixel-style provision: session owned by `user`, PipeWire pulse.
rm -f "$work/state/role" "$work/state/pkg-org.lineageos.trebuchet"
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_EVIDENCE=physical:google-pixel-9a-tegu-gxq96-mp1-0 \
  FERRIX_ANDROID_SESSION_USER=user FERRIX_ANDROID_PULSE=pipewire \
  FERRIX_ANDROID_SESSION_BUS="unix:path=$work/xdg/bus" sh "$helper" provision
check '"evidence":"physical:google-pixel-9a-tegu-gxq96-mp1-0","event":"android-app-provision".*"status":"present".*"browser_role":"default"' "$work/console"
check '"event":"android-boot".*"boot_completed":true.*"session_user":"user"' "$work/console"
check '"event":"android-setup","status":"complete","launcher":"disabled"' "$work/console"
# Only the launchers the image reports are disabled.
check '^pm-disable com.android.launcher3$' "$work/state/calls"
refute '^pm-disable org.lineageos.trebuchet$' "$work/state/calls"
refute '^app-install' "$work/state/calls"
check '^settings-put global device_provisioned 1$' "$work/state/calls"
check '^settings-put secure user_setup_complete 1$' "$work/state/calls"
check '^settings-put system screen_off_timeout 2147483647$' "$work/state/calls"
check '^stayon$' "$work/state/calls"
check '^dbus-daemon user=user$' "$work/state/calls"
# pipewire-pulse is backgrounded by design; allow it a moment to record.
for _ in 1 2 3 4 5 6 7 8 9 10; do
  grep -q '^pipewire-pulse' "$work/state/calls" && break
  /bin/sleep 0.2
done
check "^pipewire-pulse user=user xdg=$work/xdg\$" "$work/state/calls"
check "^session-start user=user bus=unix:path=$work/xdg/bus wayland=wayland-0\$" "$work/state/calls"
check '^lxc-attach user=root ' "$work/state/calls"
# An image without the Browser package: typed failure, nothing installed,
# and no launcher left to disable.
rm -f "$work/state/pkg-org.lineageos.jelly" "$work/state/pkg-com.android.launcher3"
: > "$work/console"
: > "$work/state/calls"
common_env FERRIX_ANDROID_SESSION_USER=user FERRIX_ANDROID_PULSE=none sh "$helper" provision
check '"event":"android-app-provision","app":"org.ferrix.browser","status":"failed","reason":"browser-package-missing","package":"org.lineageos.jelly"' "$work/console"
check '"event":"android-setup","status":"complete","launcher":"absent"' "$work/console"
refute '^app-install' "$work/state/calls"
refute 'add-role-holder' "$work/state/calls"
: > "$work/state/pkg-org.lineageos.jelly"

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

# Opt-in stock Codec2 overlay (hardware codecs) layers over the Mali one:
# installed with its properties only together with Mali, removed on its own
# without touching a file the Mali overlay also ships.
codec2="$work/codec2"
mkdir -p "$codec2/overlay/vendor/lib64/ferrix-codec2" "$codec2/overlay/vendor/etc" "$mali/overlay/vendor/lib64"
: > "$codec2/overlay/vendor/lib64/ferrix-codec2/libcodec2_vndk.so"
: > "$codec2/overlay/vendor/etc/media_codecs_c2.xml"
: > "$codec2/overlay/vendor/lib64/shared.so"
: > "$mali/overlay/vendor/lib64/shared.so"
printf '[properties]\nmedia.c2.hal.selection=aidl\ndebug.ffmpeg-codec2.rank.video=272\n' > "$codec2/waydroid-codec2.prop"
codec_env() { common_env FERRIX_ANDROID_MALI_DIR="$mali" FERRIX_ANDROID_CODEC2_DIR="$codec2" \
  FERRIX_ANDROID_OVERLAY="$work/overlay/vendor" "$@"; }
: > "$work/console"
codec_env FERRIX_ANDROID_RENDERER=mali FERRIX_ANDROID_CODECS=hardware sh "$helper" prepare
check '"event":"waydroid-renderer","renderer":"mali"' "$work/console"
check '"event":"waydroid-codecs","codecs":"hardware","requested":"hardware"' "$work/console"
check '"event":"waydroid-properties".*"status":"materialized".*"codecs":"hardware"' "$work/console"
[ -f "$work/overlay/vendor/lib64/ferrix-codec2/libcodec2_vndk.so" ] && [ -f "$work/overlay/vendor/etc/media_codecs_c2.xml" ] ||
  { echo 'FAIL android-bootstrap-test: codec2 overlay not installed' >&2; exit 1; }
check '^media.c2.hal.selection=aidl$' "$cfg"
check '^ro.hardware.egl=mali$' "$cfg"
check '^media.c2.hal.selection=aidl$' "$work/var/lib/waydroid/waydroid_base.prop"
check '^debug.ffmpeg-codec2.rank.video=272$' "$work/var/lib/waydroid/waydroid_base.prop"
# Idempotent: a rerun neither duplicates keys nor re-materializes.
: > "$work/console"
codec_env FERRIX_ANDROID_RENDERER=mali FERRIX_ANDROID_CODECS=hardware sh "$helper" prepare
[ "$(grep -c '^media.c2.hal.selection' "$cfg")" = 1 ] || { echo 'FAIL android-bootstrap-test: duplicate codec2 key' >&2; exit 1; }
refute '"event":"waydroid-properties"' "$work/console"
# Codecs back to software, Mali kept: codec2-only files and keys go, the
# shared file and the Mali overlay stay.
: > "$work/console"
codec_env FERRIX_ANDROID_RENDERER=mali FERRIX_ANDROID_CODECS=software sh "$helper" prepare
check '"event":"waydroid-codecs","codecs":"software","requested":"software"' "$work/console"
[ ! -e "$work/overlay/vendor/lib64/ferrix-codec2/libcodec2_vndk.so" ] || { echo 'FAIL android-bootstrap-test: codec2 overlay not removed' >&2; exit 1; }
[ -f "$work/overlay/vendor/lib64/shared.so" ] && [ -f "$work/overlay/vendor/lib64/egl/libGLES_mali.so" ] ||
  { echo 'FAIL android-bootstrap-test: Mali overlay damaged by codec2 removal' >&2; exit 1; }
refute '^media.c2' "$cfg"
refute '^media.c2' "$work/var/lib/waydroid/waydroid_base.prop"
check '^ro.hardware.egl=mali$' "$cfg"
# Hardware codecs without the Mali renderer stay in software.
: > "$work/console"
codec_env FERRIX_ANDROID_RENDERER=swiftshader FERRIX_ANDROID_CODECS=hardware sh "$helper" prepare
check '"event":"waydroid-codecs","codecs":"software","requested":"hardware"' "$work/console"
[ ! -e "$work/overlay/vendor/lib64/ferrix-codec2/libcodec2_vndk.so" ] && [ ! -e "$work/overlay/vendor/lib64/shared.so" ] ||
  { echo 'FAIL android-bootstrap-test: overlay files left without Mali' >&2; exit 1; }
refute '^media.c2' "$cfg"
refute '^ro.hardware.egl=mali' "$cfg"

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
# The compositor's extra-socket directory, not a socket file.
check "^lxc.mount.entry = $work/xdg/ferrix-android-wayland run/ferrix-wayland none bind,create=dir,optional 0 0\$" "$lxc/config_ferrix"
[ "$(grep -cxF "lxc.include = $lxc/config_ferrix" "$lxc/config")" = 1 ] ||
  { echo 'FAIL android-bootstrap-test: config_ferrix include not present exactly once' >&2; cat "$lxc/config" >&2; exit 1; }
passed=$((passed + 1))
refute 'dev/bigwave' "$lxc/config_ferrix"
# Hardware codecs also bind the extra stock Codec2 nodes the host has.
mkdir -p "$work/devroot/dev"
: > "$work/devroot/dev/bigwave"
: > "$work/devroot/dev/g2d"
chmod 0600 "$work/devroot/dev/g2d"
: > "$work/console"
codec_env FERRIX_ANDROID_LXC_PATH="$work/lxc" FERRIX_ANDROID_DEV_ROOT="$work/devroot" \
  FERRIX_ANDROID_RENDERER=mali FERRIX_ANDROID_CODECS=hardware sh "$helper" prepare
check '"event":"waydroid-lxc-extras".*"codec2_nodes":\["bigwave","g2d"\]' "$work/console"
check "^lxc.mount.entry = $work/devroot/dev/bigwave dev/bigwave none bind,create=file,optional 0 0\$" "$lxc/config_ferrix"
check "^lxc.mount.entry = $work/devroot/dev/g2d dev/g2d none bind,create=file,optional 0 0\$" "$lxc/config_ferrix"
refute 'fimg2d' "$lxc/config_ferrix"
[ "$(ls -l "$work/devroot/dev/g2d" | cut -c1-10)" = '-rw-rw----' ] || { echo 'FAIL android-bootstrap-test: codec2 node mode' >&2; exit 1; }
check '^lxc.mount.entry = tmpfs metadata' "$lxc/config_ferrix"

printf '{"evidence":"host","event":"android-bootstrap-tests","passed":%s,"failed":0}\n' "$passed"
