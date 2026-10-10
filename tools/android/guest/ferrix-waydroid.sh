#!/bin/sh
# Guest-side Waydroid bootstrap for the OPT-IN Android compatibility runtime
# (ADR-005, ADR-007). One implementation shared by every target that stages
# the runtime; targets differ only in the environment they pass:
#
#   ferrix-waydroid.sh prepare    root, before the compositor: offline
#                                 `waydroid init`, renderer properties,
#                                 container start, APK staging record.
#   ferrix-waydroid.sh provision  root, after the compositor's Wayland socket
#                                 exists: session bus, pulse socket, session
#                                 start, idempotent Fulguris install, default
#                                 browser role.
#   ferrix-waydroid.sh run        prepare, wait for the Wayland socket, then
#                                 provision (for a backgrounded service).
#
# Installed as /usr/lib/ferrix/ferrix-waydroid.sh by
# targets/qemu-aarch64/pmos/finalize-in-container.sh (QEMU, run from local.d
# as root with a root session) and tools/pixel-9a/dev-compose.py (Pixel 9a,
# run from the ferrix-android OpenRC service with the session owned by
# `user`). Every record carries FERRIX_ANDROID_EVIDENCE; nothing here claims
# a rendered frame. ferrix-appd only launches apps; it never installs.
#
# Environment (defaults reproduce the QEMU route):
#   FERRIX_ANDROID_EVIDENCE        evidence label (QEMU)
#   FERRIX_ANDROID_SESSION_USER    account owning the Waydroid session; empty
#                                  runs session commands as the caller
#   FERRIX_ANDROID_XDG_RUNTIME_DIR session runtime dir ($XDG_RUNTIME_DIR, /run/user/0)
#   FERRIX_ANDROID_WAYLAND_DISPLAY compositor socket name (wayland-0)
#   FERRIX_ANDROID_SESSION_BUS     session D-Bus address (unix:path=<xdg>/ferrix-android-bus)
#   FERRIX_ANDROID_PULSE           system | pipewire (falls back to null) | null | none (system)
#   FERRIX_ANDROID_PROPS, _IMAGES, _CONFIG, _BASE_PROPS, _FULGURIS_APK,
#   _CONSOLE, _RUN_DIR, _LXC_PATH, _BOOT_TICKS, _WAYLAND_TICKS  paths/bounds
#   FERRIX_ANDROID_RENDERER        swiftshader (default) | mali; else read from
#                                  /etc/ferrix/waydroid-renderer
#   FERRIX_ANDROID_MALI_DIR        extracted stock Mali overlay
#                                  (tools/pixel-9a/waydroid-mali-overlay.sh):
#                                  overlay/vendor/... and waydroid-mali.prop
#   FERRIX_ANDROID_OVERLAY         Waydroid vendor overlay (/var/lib/waydroid/overlay/vendor)
set -u
evidence=${FERRIX_ANDROID_EVIDENCE:-QEMU}
props=${FERRIX_ANDROID_PROPS:-/etc/ferrix/waydroid-properties.ini}
images=${FERRIX_ANDROID_IMAGES:-/etc/waydroid-extra/images}
cfg=${FERRIX_ANDROID_CONFIG:-/var/lib/waydroid/waydroid.cfg}
base_props=${FERRIX_ANDROID_BASE_PROPS:-${cfg%/*}/waydroid_base.prop}
fulguris_apk=${FERRIX_ANDROID_FULGURIS_APK:-/usr/share/ferrix/android/fulguris.apk}
package=${FERRIX_ANDROID_PACKAGE:-net.slions.fulguris.full.fdroid}
console=${FERRIX_ANDROID_CONSOLE:-/dev/console}
run_dir=${FERRIX_ANDROID_RUN_DIR:-/run/ferrix}
session_user=${FERRIX_ANDROID_SESSION_USER:-}
xdg=${FERRIX_ANDROID_XDG_RUNTIME_DIR:-${XDG_RUNTIME_DIR:-/run/user/0}}
wayland=${FERRIX_ANDROID_WAYLAND_DISPLAY:-wayland-0}
bus=${FERRIX_ANDROID_SESSION_BUS:-unix:path=$xdg/ferrix-android-bus}
pulse=${FERRIX_ANDROID_PULSE:-system}
lxc_path=${FERRIX_ANDROID_LXC_PATH:-/var/lib/waydroid/lxc}
boot_ticks=${FERRIX_ANDROID_BOOT_TICKS:-1800}
wayland_ticks=${FERRIX_ANDROID_WAYLAND_TICKS:-3000}
# The Android density is a per-target staging choice carried in the
# properties file; the default matches the QEMU route.
density_dpi=$(sed -n 's/^ro\.sf\.lcd_density=\([0-9][0-9]*\)$/\1/p' "$props" 2>/dev/null | head -n 1)
[ -n "$density_dpi" ] || density_dpi=400
mali_dir=${FERRIX_ANDROID_MALI_DIR:-/usr/share/ferrix/android/mali}
vendor_overlay=${FERRIX_ANDROID_OVERLAY:-/var/lib/waydroid/overlay/vendor}
renderer=${FERRIX_ANDROID_RENDERER:-$(cat /etc/ferrix/waydroid-renderer 2>/dev/null || echo swiftshader)}

# select_renderer: for `mali`, install the stock Mali overlay and layer its
# properties over the target's; otherwise remove a previously installed
# overlay. Falls back to the software renderer whenever the overlay is absent.
select_renderer() {
  installed="$vendor_overlay/.ferrix-mali-files"
  if [ "$renderer" = mali ] && [ -d "$mali_dir/overlay/vendor" ] && [ -f "$mali_dir/waydroid-mali.prop" ]; then
    mkdir -p "$vendor_overlay"
    (cd "$mali_dir/overlay/vendor" && find . -type f) > "$installed.new"
    cp -a "$mali_dir/overlay/vendor/." "$vendor_overlay/"
    mv "$installed.new" "$installed"
    effective="$run_dir/waydroid-properties.effective.ini"
    awk -v over="$mali_dir/waydroid-mali.prop" '
      BEGIN { while ((getline line < over) > 0) if (line ~ /=/) { k = line; sub(/=.*/, "", k); o[k] = 1 } }
      /=/ { k = $0; sub(/=.*/, "", k); if (k in o) next } { print }
    ' "$props" > "$effective"
    grep '=' "$mali_dir/waydroid-mali.prop" >> "$effective"
    props=$effective
  else
    if [ -f "$installed" ]; then
      while IFS= read -r f; do rm -f "$vendor_overlay/${f#./}"; done < "$installed"
      rm -f "$installed"
      # Drop Mali-only keys the software properties do not override.
      if [ -f "$cfg" ] && [ -f "$mali_dir/waydroid-mali.prop" ]; then
        awk -v over="$mali_dir/waydroid-mali.prop" -v keep="$props" '
          BEGIN { while ((getline l < over) > 0) if (l ~ /=/) { k = l; sub(/=.*/, "", k); o[k] = 1 }
                  while ((getline l < keep) > 0) if (l ~ /=/) { k = l; sub(/=.*/, "", k); delete o[k] } }
          { k = $0; sub(/[ \t]*=.*/, "", k); if (!($0 ~ /=/ && k in o)) print }
        ' "$cfg" > "$cfg.ferrix" && mv "$cfg.ferrix" "$cfg"
      fi
    fi
    renderer=swiftshader
  fi
  egl=$(sed -n 's/^ro\.hardware\.egl=//p' "$props" | head -n 1)
  gralloc=$(sed -n 's/^ro\.hardware\.gralloc=//p' "$props" | head -n 1)
  ev waydroid-renderer ",\"renderer\":\"$renderer\",\"egl\":\"${egl:-unset}\",\"gralloc\":\"${gralloc:-unset}\""
}

# props_applied: every managed property appears in the config exactly once
# with its value, ignoring Waydroid's `key = value` spacing.
props_applied() {
  grep -v '^\[' "$props" | grep '=' | while IFS= read -r line; do
    key=${line%%=*}; value=${line#*=}
    [ "$(grep -Ec "^${key}[[:space:]]*=" "$cfg")" = 1 ] &&
      grep -Eq "^${key}[[:space:]]*=[[:space:]]*${value}[[:space:]]*$" "$cfg" ||
      { echo missing; break; }
  done | grep -q missing && return 1
  return 0
}
rec() { printf '%s\n' "$1" >> "$console"; logger -t ferrix "$1" 2>/dev/null || true; }
# ev EVENT [,"k":v...]  -> one record with the configured evidence label.
ev() { rec "{\"evidence\":\"$evidence\",\"event\":\"$1\"${2:-}}"; }
dev_state() { if [ -e "$1" ]; then ls -l "$1" 2>/dev/null | awk '{print $1" "$5" "$6}' | tr ',' ' '; else printf 'absent'; fi; }
present() { if [ -e "$1" ]; then printf present; else printf absent; fi; }

# Session-side commands run as the session owner so the Wayland/pulse
# sockets and Android user data belong to the account the compositor uses.
# Arguments are fixed paths and names chosen by this script (no spaces).
pulse_dir=$xdg/pulse
[ "$pulse" != system ] || pulse_dir=/run/pulse
session_env="XDG_RUNTIME_DIR=$xdg WAYLAND_DISPLAY=$wayland DBUS_SESSION_BUS_ADDRESS=$bus PULSE_RUNTIME_PATH=$pulse_dir"
as_session() {
  if [ -n "$session_user" ] && [ "$(id -un 2>/dev/null)" != "$session_user" ]; then
    su -s /bin/sh "$session_user" -c "$session_env $1"
  else
    sh -c "$session_env $1"
  fi
}
# session_log NAME -> a log path the session owner can write.
session_log() {
  : > "$run_dir/$1"
  [ -z "$session_user" ] || chown "$session_user" "$run_dir/$1" 2>/dev/null || true
  printf '%s' "$run_dir/$1"
}

# ensure_lxc_extras: Android 14+ init and aconfigd write flag storage under
# /metadata (`mkdir /metadata/aconfig`), which is read-only in the Waydroid
# rootfs; without it every aconfig flag reads false and system_server dies
# on flag-gated permissions (android.permission.RANGING). Waydroid 1.6
# mounts nothing there, and rewrites `config` on init/upgrade, so keep the
# mount in a Ferrix-owned include and re-add the include before each start.
# `optional` keeps images without a /metadata mountpoint (LineageOS 20)
# starting.
ensure_lxc_extras() {
  conf_dir="$lxc_path/waydroid"
  [ -f "$conf_dir/config" ] || return 0
  extras="$conf_dir/config_ferrix"
  printf '%s\n' '# Managed by ferrix-waydroid.sh' \
    'lxc.mount.entry = tmpfs metadata tmpfs nodev,nosuid,mode=0755,optional 0 0' >"$extras"
  grep -qxF "lxc.include = $extras" "$conf_dir/config" ||
    printf 'lxc.include = %s\n' "$extras" >>"$conf_dir/config"
  ev waydroid-lxc-extras ",\"status\":\"ok\",\"metadata\":\"tmpfs\",\"path\":\"$extras\""
}

prepare() {
  mkdir -p "$run_dir"
  binderfs=absent
  grep -qs binder /proc/filesystems && binderfs=present
  ev android-binder-devices ",\"binder\":\"$(dev_state /dev/binder)\",\"hwbinder\":\"$(dev_state /dev/hwbinder)\",\"vndbinder\":\"$(dev_state /dev/vndbinder)\",\"binderfs\":\"$binderfs\",\"pid_ns\":\"$(present /proc/self/ns/pid)\",\"psi\":\"$( [ -r /proc/pressure/cpu ] && echo present || echo absent)\""
  if ! command -v waydroid >/dev/null 2>&1; then
    ev waydroid-init ',"status":"failed","reason":"waydroid-not-installed"'
    return 0
  fi
  rc-service dbus start >/dev/null 2>&1 || true
  rc-service cgroups start >/dev/null 2>&1 || true
  if [ -f "$cfg" ] && grep -q '^images_path' "$cfg"; then
    ev waydroid-init ',"status":"already-initialized"'
  elif [ -f "$images/system.img" ] && [ -f "$images/vendor.img" ]; then
    if timeout 300 waydroid init -f -i "$images" >"$run_dir/waydroid-init.log" 2>&1; then
      ev waydroid-init ",\"status\":\"ok\",\"images\":\"$images\",\"source\":\"offline\""
    else
      ev waydroid-init ",\"status\":\"failed\",\"reason\":\"waydroid-init-exit-$?\",\"log\":\"$run_dir/waydroid-init.log\""
      tail -n 20 "$run_dir/waydroid-init.log" >> "$console" 2>/dev/null || true
    fi
  else
    ev waydroid-init ',"status":"failed","reason":"images-missing"'
  fi
  select_renderer
  # Renderer properties. waydroid init rewrites the file without
  # them, and Waydroid itself rewrites it as `key = value`; merge by key so a
  # rerun never leaves a duplicate option (configparser refuses those).
  if [ -f "$cfg" ] && [ -f "$props" ] && ! props_applied; then
    merged="$cfg.ferrix"
    awk -v props="$props" '
      BEGIN { while ((getline line < props) > 0) if (line ~ /=/ && line !~ /^\[/) {
                key = line; sub(/[ \t]*=.*/, "", key); managed[key] = 1 } }
      { key = $0; sub(/[ \t]*=.*/, "", key); if (!($0 ~ /=/ && key in managed)) print }
    ' "$cfg" > "$merged"
    { grep -q '^\[properties\]' "$merged" || printf '\n[properties]\n'; grep -v '^\[' "$props"; } >> "$merged"
    mv "$merged" "$cfg"
    ev waydroid-properties ",\"status\":\"applied\",\"density_dpi\":${density_dpi},\"path\":\"$cfg\""
  fi
  # `waydroid init` creates waydroid_base.prop before the overrides above
  # exist. A plain container start reuses that file and may select a GPU
  # renderer the target cannot provide; regenerate the offline container
  # configuration whenever its effective renderer differs.
  if [ -f "$cfg" ] && { ! grep -q "^ro.hardware.gralloc=${gralloc:-default}$" "$base_props" 2>/dev/null \
    || ! grep -q "^ro.hardware.egl=${egl:-swiftshader}$" "$base_props" 2>/dev/null \
    || ! grep -q "^ro.sf.lcd_density=${density_dpi}$" "$base_props" 2>/dev/null; }; then
    if timeout 300 waydroid upgrade -o >"$run_dir/waydroid-upgrade.log" 2>&1; then
      ev waydroid-properties ",\"status\":\"materialized\",\"renderer\":\"$renderer\",\"density_dpi\":${density_dpi}"
    else
      ev waydroid-properties ",\"status\":\"failed\",\"reason\":\"offline-upgrade-exit-$?\",\"log\":\"$run_dir/waydroid-upgrade.log\""
    fi
  fi
  ensure_lxc_extras
  if [ -f "$cfg" ] && rc-service waydroid-container start >"$run_dir/waydroid-container.log" 2>&1; then
    ev waydroid-container ',"status":"started"'
  else
    ev waydroid-container ",\"status\":\"failed\",\"log\":\"$run_dir/waydroid-container.log\""
    tail -n 20 "$run_dir/waydroid-container.log" >> "$console" 2>/dev/null || true
  fi
  [ ! -f "$fulguris_apk" ] || ev android-app-staged ",\"app\":\"org.ferrix.browser\",\"path\":\"$fulguris_apk\""
  return 0
}

start_session_bus() {
  case $bus in unix:path=*) sock=${bus#unix:path=} ;; *) return 0 ;; esac
  [ ! -S "$sock" ] || return 0
  rm -f "$sock"
  log=$(session_log android-dbus.log)
  as_session "dbus-daemon --session --address=$bus --fork --nopidfile >>$log 2>&1" || true
}

# Waydroid always bind-mounts <xdg>/pulse/native into the container and LXC
# refuses to start without it. A socket-only PulseAudio with a null sink
# satisfies it (Android plays silently); it loads no ALSA module, so PipeWire
# keeps the sound card.
start_null_pulse() {
  [ ! -S "$pulse_dir/native" ] || return 0
  command -v pulseaudio >/dev/null 2>&1 || return 0
  log=$(session_log android-pulse.log)
  as_session "pulseaudio -n --daemonize=yes --exit-idle-time=-1 --use-pid-file=no -L module-native-protocol-unix -L module-null-sink -L module-always-sink >>$log 2>&1" || true
  for _ in $(seq 1 50); do [ -S "$pulse_dir/native" ] && break; sleep 0.1; done
}

start_pulse() {
  case $pulse in
    system)
      # Alpine's system PulseAudio owns /run/pulse/native, while Waydroid
      # mounts the session's pulse path. Expose the same socket there.
      pulseaudio --system --daemonize=yes --exit-idle-time=-1 >"$run_dir/android-pulse.log" 2>&1 || true
      for _ in $(seq 1 50); do [ -S /run/pulse/native ] && break; sleep 0.1; done
      if [ -S /run/pulse/native ]; then
        mkdir -p "$xdg/pulse"
        ln -sf /run/pulse/native "$xdg/pulse/native"
      fi
      ;;
    pipewire)
      # pipewire-pulse serves <xdg>/pulse/native for the PipeWire session
      # ferrix-audio already runs, so Android audio reaches the speaker.
      if [ ! -S "$xdg/pulse/native" ] && command -v pipewire-pulse >/dev/null 2>&1; then
        log=$(session_log android-pulse.log)
        as_session "pipewire-pulse >>$log 2>&1 &" || true
        for _ in $(seq 1 50); do [ -S "$xdg/pulse/native" ] && break; sleep 0.1; done
      fi
      # Without it the container could not start at all; fall back to null.
      [ -S "$pulse_dir/native" ] || start_null_pulse
      ;;
    null)
      start_null_pulse
      ;;
  esac
  ev android-pulse ",\"mode\":\"$pulse\",\"socket\":\"$(present "$pulse_dir/native")\""
}

package_present() {
  waydroid shell -- pm path "$package" >"$run_dir/waydroid-app-list.log" 2>&1 || true
  grep -q '^package:' "$run_dir/waydroid-app-list.log" 2>/dev/null
}

# Android's first-run wizard must never show: Ferrix owns setup, and until
# user_setup_complete is set ActivityTaskManager skips app launches ("user
# setup not complete"). Mark setup done and disable any wizard package.
complete_android_setup() {
  : > "$run_dir/waydroid-setup.log"
  waydroid shell -- settings put global device_provisioned 1 >>"$run_dir/waydroid-setup.log" 2>&1 || true
  waydroid shell -- settings put secure user_setup_complete 1 >>"$run_dir/waydroid-setup.log" 2>&1 || true
  for wizard in org.lineageos.setupwizard com.google.android.setupwizard com.android.provision; do
    waydroid shell -- pm path "$wizard" 2>/dev/null | grep -q '^package:' || continue
    waydroid shell -- pm disable "$wizard" >>"$run_dir/waydroid-setup.log" 2>&1 || true
  done
  # Ferrix owns display power. Android never sees the shell's user activity,
  # so its own screen timeout puts it to sleep; asleep, a newly launched task
  # never draws and (in task-streams mode) never gets a window.
  waydroid shell -- settings put system screen_off_timeout 2147483647 >>"$run_dir/waydroid-setup.log" 2>&1 || true
  waydroid shell -- svc power stayon true >>"$run_dir/waydroid-setup.log" 2>&1 || true
  done_flag=$(waydroid shell -- settings get secure user_setup_complete 2>/dev/null | tr -d '\r')
  if [ "$done_flag" = 1 ]; then
    ev android-setup ',"status":"complete"'
  else
    ev android-setup ",\"status\":\"failed\",\"log\":\"$run_dir/waydroid-setup.log\""
  fi
}

provision() {
  [ -f "$fulguris_apk" ] || return 0
  if ! command -v waydroid >/dev/null 2>&1; then
    ev android-app-provision ',"app":"org.ferrix.browser","status":"failed","reason":"waydroid-not-installed"'
    return 0
  fi
  mkdir -p "$run_dir"
  start_session_bus
  start_pulse
  session=$(session_log waydroid-session.log)
  as_session "waydroid session start >>$session 2>&1 &"
  # RUNNING only means the LXC/session processes exist. Package operations
  # require Android's framework to have completed boot as well.
  booted=false
  for _ in $(seq 1 "$boot_ticks"); do
    if [ "$(waydroid shell -- getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; then
      booted=true
      break
    fi
    sleep 0.1
  done
  ev android-boot ",\"boot_completed\":$booted,\"session_user\":\"${session_user:-$(id -un 2>/dev/null)}\""
  [ "$booted" != true ] || complete_android_setup
  provision_status=present
  if ! package_present; then
    provision_status=installed
    install_log=$(session_log waydroid-fulguris-install.log)
    as_session "waydroid app install $fulguris_apk >>$install_log 2>&1" || true
  fi
  for _ in $(seq 1 600); do
    package_present && break
    sleep 0.1
  done
  if package_present; then
    # RoleManager's shell command authorizes Android's shell UID (2000), not
    # the UID 0 used by `waydroid shell`. Run it in the existing container as
    # that UID, then verify the effective HTTPS resolver rather than trusting
    # the command's asynchronous, output-free success response.
    : > "$run_dir/waydroid-fulguris-role.log"
    : > "$run_dir/waydroid-browser-resolver.log"
    waydroid container unfreeze >>"$run_dir/waydroid-fulguris-role.log" 2>&1 || true
    lxc-attach -P "$lxc_path" -n waydroid --uid 2000 -- \
      /system/bin/cmd role add-role-holder android.app.role.BROWSER "$package" \
      >>"$run_dir/waydroid-fulguris-role.log" 2>&1 || true
    for _ in $(seq 1 100); do
      waydroid shell -- cmd package resolve-activity --brief \
        -a android.intent.action.VIEW -c android.intent.category.BROWSABLE \
        -d https://example.com >"$run_dir/waydroid-browser-resolver.log" 2>&1 || true
      grep -q "^$package/" "$run_dir/waydroid-browser-resolver.log" 2>/dev/null && break
      sleep 0.1
    done
    if grep -q "^$package/" "$run_dir/waydroid-browser-resolver.log" 2>/dev/null; then
      ev android-app-provision ",\"app\":\"org.ferrix.browser\",\"status\":\"$provision_status\",\"package\":\"$package\",\"browser_role\":\"default\""
    else
      ev android-app-provision ",\"app\":\"org.ferrix.browser\",\"status\":\"failed\",\"reason\":\"default-browser-role\",\"role_log\":\"$run_dir/waydroid-fulguris-role.log\",\"resolver_log\":\"$run_dir/waydroid-browser-resolver.log\""
      cat "$run_dir/waydroid-fulguris-role.log" "$run_dir/waydroid-browser-resolver.log" >> "$console" 2>/dev/null || true
    fi
  else
    ev android-app-provision ",\"app\":\"org.ferrix.browser\",\"status\":\"failed\",\"session_log\":\"$session\",\"install_log\":\"$run_dir/waydroid-fulguris-install.log\""
    printf '%s\n' 'android provisioning diagnostics:' >> "$console"
    waydroid status >> "$console" 2>&1 || true
    for log in "$session" "$run_dir/waydroid-fulguris-install.log" "$run_dir/android-pulse.log" "$run_dir/android-dbus.log"; do
      [ ! -s "$log" ] || { printf '%s\n' "--- $log" >> "$console"; tail -n 40 "$log" >> "$console"; }
    done
  fi
  return 0
}

wait_for_wayland() {
  for _ in $(seq 1 "$wayland_ticks"); do
    [ -S "$xdg/$wayland" ] && return 0
    sleep 0.1
  done
  ev android-app-provision ",\"app\":\"org.ferrix.browser\",\"status\":\"failed\",\"reason\":\"no-wayland-socket\",\"socket\":\"$xdg/$wayland\""
  return 1
}

case ${1:-} in
  prepare) prepare ;;
  provision) provision ;;
  run) prepare; wait_for_wayland && provision ;;
  *) printf 'usage: %s prepare|provision|run\n' "$0" >&2; exit 2 ;;
esac
exit 0
