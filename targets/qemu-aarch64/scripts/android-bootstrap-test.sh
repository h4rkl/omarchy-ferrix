#!/bin/sh
# Deterministic host test for the Android guest bootstrap script.  A fake
# Waydroid CLI proves a staged APK is installed once, then recognized as
# present on the next boot.  It never starts a container or touches a device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script=$root/targets/qemu-aarch64/pmos/local.d/00-ferrix-android.start
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-android-bootstrap.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
mkdir -p "$work/bin" "$work/images" "$work/run" "$work/var/lib/waydroid" "$work/usr/share/ferrix/android"
: > "$work/images/system.img"
: > "$work/images/vendor.img"
: > "$work/usr/share/ferrix/android/brave.apk"
: > "$work/console"

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
state=${FERRIX_ANDROID_TEST_STATE:?}
case "$1 ${2:-}" in
  'init -f') : > "${FERRIX_ANDROID_CONFIG:?}" ;;
  'app list')
    if [ -f "$state" ]; then
      printf 'Name: Brave\npackageName: com.brave.browser\n'
    fi
    ;;
  'app install') printf x >> "$state" ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$work/bin"/*

run_bootstrap() {
  PATH="$work/bin:$PATH" \
  FERRIX_ANDROID_PROPS="$work/properties.ini" \
  FERRIX_ANDROID_IMAGES="$work/images" \
  FERRIX_ANDROID_CONFIG="$work/var/lib/waydroid/waydroid.cfg" \
  FERRIX_ANDROID_BRAVE_APK="$work/usr/share/ferrix/android/brave.apk" \
  FERRIX_ANDROID_CONSOLE="$work/console" \
  FERRIX_ANDROID_RUN_DIR="$work/run" \
  FERRIX_ANDROID_TEST_STATE="$work/brave-installed" \
  sh "$script"
}

run_bootstrap
grep -q '"event":"android-app-install".*"status":"installed"' "$work/console"
[ -f "$work/brave-installed" ]
installs=$(wc -l < "$work/console")
run_bootstrap
grep -q '"event":"android-app-install".*"status":"present"' "$work/console"
[ "$(wc -l < "$work/console")" -gt "$installs" ]
[ "$(wc -c < "$work/brave-installed" | tr -d ' ')" = 1 ]
printf '%s\n' '{"evidence":"host","event":"android-bootstrap-tests","passed":2,"failed":0}'
