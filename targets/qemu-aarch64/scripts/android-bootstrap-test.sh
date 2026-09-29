#!/bin/sh
# Deterministic host test for the pre-compositor Android guest bootstrap. It
# proves offline initialization, container startup, and APK staging. Package
# installation belongs to the post-compositor product bootstrap.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script=$root/targets/qemu-aarch64/pmos/local.d/00-ferrix-android.start
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-android-bootstrap.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
mkdir -p "$work/bin" "$work/images" "$work/run" "$work/var/lib/waydroid" "$work/usr/share/ferrix/android"
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

cat > "$work/bin/rc-service" <<'EOF'
#!/bin/sh
exit 0
EOF
cat > "$work/bin/timeout" <<'EOF'
#!/bin/sh
shift
exec "$@"
EOF
cat > "$work/bin/logger" <<'EOF'
#!/bin/sh
exit 0
EOF
cat > "$work/bin/waydroid" <<'EOF'
#!/bin/sh
set -eu
case "$1 ${2:-}" in
  'init -f') : > "${FERRIX_ANDROID_CONFIG:?}" ;;
  'upgrade -o')
    cat > "${FERRIX_ANDROID_BASE_PROPS:?}" <<'PROPS'
ro.hardware.gralloc=default
ro.hardware.egl=swiftshader
ro.sf.lcd_density=400
PROPS
    ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$work/bin"/*

run_bootstrap() {
  PATH="$work/bin:$PATH" \
  FERRIX_ANDROID_PROPS="$work/properties.ini" \
  FERRIX_ANDROID_IMAGES="$work/images" \
  FERRIX_ANDROID_CONFIG="$work/var/lib/waydroid/waydroid.cfg" \
  FERRIX_ANDROID_BASE_PROPS="$work/var/lib/waydroid/waydroid_base.prop" \
  FERRIX_ANDROID_FULGURIS_APK="$work/usr/share/ferrix/android/fulguris.apk" \
  FERRIX_ANDROID_CONSOLE="$work/console" \
  FERRIX_ANDROID_RUN_DIR="$work/run" \
  sh "$script"
}

run_bootstrap
grep -q '"event":"waydroid-init".*"status":"ok"' "$work/console"
grep -q '"event":"waydroid-properties".*"status":"materialized".*"density_dpi":400' "$work/console"
grep -q '"event":"waydroid-container".*"status":"started"' "$work/console"
grep -q '"event":"android-app-staged"' "$work/console"
grep -q '^ro.sf.lcd_density=400$' "$work/var/lib/waydroid/waydroid_base.prop"
run_bootstrap
printf '%s\n' '{"evidence":"host","event":"android-bootstrap-tests","passed":2,"failed":0}'
