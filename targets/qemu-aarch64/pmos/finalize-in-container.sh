#!/bin/sh
# Runs as root inside containers/guest-builder (via sudo from
# build-in-container.sh). Mounts the pmbootstrap-generated disk image through
# a loop device, installs the project readiness marker into the root
# filesystem, exports the boot files QEMU needs, records image facts and
# copies the image to the output directory.
set -eu
img=$1
project=${FERRIX_PMOS_PROJECT:-/project}
out=${FERRIX_PMOS_OUT:-/out}
marker_src="$project/local.d/ferrix-boot-health.start"
marker_dst=/etc/local.d/ferrix-boot-health.start
product_src="$project/local.d/00-ferrix-product.start"
product_dst=/etc/local.d/00-ferrix-product.start

log() { printf '[pmos-finalize] %s\n' "$*"; }
fail() {
  printf '{"evidence":"QEMU","event":"pmos-build-failed","reason":"%s","detail":"%s"}\n' "$1" "$2" >&2
  exit 1
}

mnt=$(mktemp -d)
loop=""
boot_mounted=0
root_mounted=0
cleanup() {
  [ "$boot_mounted" = 1 ] && umount "$mnt/boot" 2>/dev/null || true
  [ "$root_mounted" = 1 ] && umount "$mnt" 2>/dev/null || true
  [ -n "$loop" ] && losetup -d "$loop" 2>/dev/null || true
  rmdir "$mnt" 2>/dev/null || true
}
trap cleanup EXIT

loop=$(losetup --find --show --partscan "$img")
loopname=${loop#/dev/}
# No udev in the container: create partition nodes from sysfs when missing.
for sys in /sys/block/"$loopname"/"$loopname"p*; do
  [ -d "$sys" ] || continue
  name=${sys##*/}
  if [ ! -e "/dev/$name" ]; then
    major=$(cut -d: -f1 "$sys/dev")
    minor=$(cut -d: -f2 "$sys/dev")
    mknod "/dev/$name" b "$major" "$minor"
  fi
done
rootdev=""
bootdev=""
partitions=""
for dev in /dev/"$loopname"p*; do
  [ -e "$dev" ] || continue
  label=$(blkid -o value -s LABEL "$dev" 2>/dev/null || true)
  fstype=$(blkid -o value -s TYPE "$dev" 2>/dev/null || true)
  partitions="$partitions${partitions:+,}{\"index\":${dev##*p},\"label\":\"$label\",\"fstype\":\"$fstype\"}"
  case "$label" in
    pmOS_root) rootdev=$dev ;;
    pmOS_boot) bootdev=$dev ;;
  esac
done
[ -n "$rootdev" ] || fail "no-root-partition" "no partition labelled pmOS_root in $img"
[ -n "$bootdev" ] || fail "no-boot-partition" "no partition labelled pmOS_boot in $img"
log "root=$rootdev boot=$bootdev"

mount "$rootdev" "$mnt"
root_mounted=1
mkdir -p "$mnt/boot"
mount -o ro "$bootdev" "$mnt/boot"
boot_mounted=1

# Readiness marker (OpenRC local.d, default runlevel).
install -Dm755 "$marker_src" "$mnt$marker_dst"
install -Dm755 "$product_src" "$mnt$product_dst"
# Product binaries: the same set qemu-build.sh stages for the smoke guest,
# including the service daemons and the Phone/Messages/Contacts/Settings apps.
product_bins="ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings"
for bin in $product_bins; do
  [ -f "/product/$bin" ] || fail "missing-product-binary" "$bin not in /product; run just qemu-build first"
  install -Dm755 "/product/$bin" "$mnt/usr/bin/$bin"
done
install -Dm644 /product/profile.toml "$mnt/etc/ferrix/profile.toml"
install -Dm644 /product/colors.toml "$mnt/etc/ferrix/colors.toml"
mkdir -p "$mnt/usr/share/ferrix/apps" "$mnt/var/lib/ferrix/telephony" "$mnt/var/lib/ferrix/contacts"
chmod 0700 "$mnt/var/lib/ferrix/telephony" "$mnt/var/lib/ferrix/contacts"
# Catalog plus grants.json (ADR-004); ferrix-appd reads <app dir>/grants.json.
for descriptor in /product/apps/*.json; do
  install -Dm644 "$descriptor" "$mnt/usr/share/ferrix/apps/$(basename "$descriptor")"
done
[ -f "$mnt/usr/share/ferrix/apps/grants.json" ] || fail "missing-grants" "/product/apps/grants.json not staged"
contacts_fixtures_sha=""
if [ -f /product/contacts/mock-contacts.json ]; then
  install -Dm644 /product/contacts/mock-contacts.json "$mnt/usr/share/ferrix/contacts/mock-contacts.json"
  contacts_fixtures_sha=$(sha256sum "$mnt/usr/share/ferrix/contacts/mock-contacts.json" | cut -d' ' -f1)
fi
# --- OPT-IN Android compatibility runtime (Waydroid) -------------------------
# Only when FERRIX_PMOS_ANDROID=1: stage the pinned container images, the
# software-rendering properties, the nftables rule, the first-boot init
# script (00-ferrix-android.start, before 00-ferrix-product.start), and the
# verified Fulguris APK when pmos-build mounted one. `waydroid init` itself runs
# at first guest boot (offline, from /etc/waydroid-extra/images): it needs
# the guest's dbus/lxc environment, not a loop-mounted rootfs.
android_facts='"enabled": false'
if [ "${FERRIX_PMOS_ANDROID:-0}" = 1 ]; then
  # Pixel-class Android viewport for the generic QEMU phone: 400 dpi makes
  # the 1080-pixel panel approximately 432 dp wide. Keep this inside the
  # QEMU Android image branch; it must not enter the shared Ferrix profile or
  # any physical-target package.
  android_density_dpi=400
  for f in system.img vendor.img images.json; do
    [ -f "/android/images/$f" ] || fail "android-images-missing" "/android/images/$f not mounted"
  done
  install -Dm644 /android/images/system.img "$mnt/etc/waydroid-extra/images/system.img"
  install -Dm644 /android/images/vendor.img "$mnt/etc/waydroid-extra/images/vendor.img"
  install -Dm644 /android/images/images.json "$mnt/etc/waydroid-extra/images.json"
  cat > "$mnt/etc/ferrix/waydroid-properties.ini" <<'INI'
[properties]
ro.hardware.gralloc=default
ro.hardware.egl=swiftshader
persist.waydroid.multi_windows=false
ro.sf.lcd_density=400
INI
  install -Dm755 "$project/local.d/00-ferrix-android.start" "$mnt/etc/local.d/00-ferrix-android.start"
  mkdir -p "$mnt/var/lib/waydroid" "$mnt/etc/nftables.d"
  nft_rule=false
  if [ -f "$mnt/usr/share/nftables.avail/51_waydroid.nft" ]; then
    ln -sf /usr/share/nftables.avail/51_waydroid.nft "$mnt/etc/nftables.d/51_waydroid.nft"
    nft_rule=true
  fi
  # cgroups and dbus in the default runlevel (waydroid-container is started
  # by the first-boot script after init, never before).
  for svc in cgroups dbus; do
    if [ -x "$mnt/etc/init.d/$svc" ] && [ ! -L "$mnt/etc/runlevels/default/$svc" ] && [ ! -L "$mnt/etc/runlevels/boot/$svc" ]; then
      ln -s "/etc/init.d/$svc" "$mnt/etc/runlevels/default/$svc"
    fi
  done
  fulguris_sha=""
  if [ -f /android/fulguris.apk ]; then
    install -Dm644 /android/fulguris.apk "$mnt/usr/share/ferrix/android/fulguris.apk"
    fulguris_sha=$(sha256sum "$mnt/usr/share/ferrix/android/fulguris.apk" | cut -d' ' -f1)
  fi
  waydroid_pkg=$(awk '/^P:waydroid$/ { getline; sub(/^V:/, ""); print; exit }' "$mnt/lib/apk/db/installed")
  [ -n "$waydroid_pkg" ] || fail "waydroid-not-installed" "package waydroid missing from the rootfs apk database"
  android_facts="\"enabled\": true, \"waydroid_version\": \"$waydroid_pkg\", \"images_json\": $(cat /android/images/images.json), \"system_img_sha256\": \"$(sha256sum "$mnt/etc/waydroid-extra/images/system.img" | cut -d' ' -f1)\", \"vendor_img_sha256\": \"$(sha256sum "$mnt/etc/waydroid-extra/images/vendor.img" | cut -d' ' -f1)\", \"properties_sha256\": \"$(sha256sum "$mnt/etc/ferrix/waydroid-properties.ini" | cut -d' ' -f1)\", \"init_script_sha256\": \"$(sha256sum "$mnt/etc/local.d/00-ferrix-android.start" | cut -d' ' -f1)\", \"nftables_rule_linked\": $nft_rule, \"fulguris_apk_sha256\": \"$fulguris_sha\", \"init_mode\": \"first-boot\""
  android_facts="\"density_dpi\": $android_density_dpi, $android_facts"
  log "android runtime staged (waydroid $waydroid_pkg, density ${android_density_dpi}dpi)"
fi

local_enabled=true
# -L as well as -e: the runlevel entry is a symlink to /etc/init.d/local that
# dangles when resolved against the container's own root.
if [ ! -L "$mnt/etc/runlevels/default/local" ] && [ ! -e "$mnt/etc/runlevels/default/local" ]; then
  local_enabled=false
  [ -x "$mnt/etc/init.d/local" ] || fail "no-openrc-local" "OpenRC local service missing in rootfs"
  ln -s /etc/init.d/local "$mnt/etc/runlevels/default/local"
  log "enabled OpenRC local service in default runlevel"
fi

# Boot files: boot-deploy places them on the boot partition.
mkdir -p "$out"
exported=""
copy_out() {
  cp -f "$1" "$out/$2"
  exported="$exported${exported:+,}\"$2\""
}
kernel=""
for candidate in "$mnt/boot/vmlinuz" "$mnt/boot"/vmlinuz-*; do
  [ -f "$candidate" ] && { kernel=$candidate; break; }
done
[ -n "$kernel" ] || fail "no-kernel" "no vmlinuz on the boot partition"
copy_out "$kernel" vmlinuz
initramfs=""
for candidate in "$mnt/boot/initramfs" "$mnt/boot"/initramfs-*; do
  case "$candidate" in *-extra*) continue ;; esac
  [ -f "$candidate" ] && { initramfs=$candidate; break; }
done
[ -n "$initramfs" ] || fail "no-initramfs" "no initramfs on the boot partition"
copy_out "$initramfs" initramfs
[ -f "$mnt/boot/initramfs-extra" ] && copy_out "$mnt/boot/initramfs-extra" initramfs-extra

# Kernel command line: the systemd-boot loader entry written by boot-deploy is
# authoritative when present; otherwise compose it exactly as boot-deploy
# does, from /usr/lib/kernel-cmdline.d and /etc/kernel-cmdline.d.
cmdline=""
cmdline_source=""
for entry in "$mnt"/boot/loader/entries/*.conf; do
  [ -f "$entry" ] || continue
  cmdline=$(sed -n 's/^options[[:space:]]*//p' "$entry" | head -1)
  [ -n "$cmdline" ] && { cmdline_source="boot/loader/entries/${entry##*/}"; break; }
done
if [ -z "$cmdline" ]; then
  cmdline=$(for f in "$mnt"/usr/lib/kernel-cmdline.d/*.conf "$mnt"/etc/kernel-cmdline.d/*.conf; do
    [ -f "$f" ] && cat "$f"
  done | grep -v '^[[:space:]]*#' | tr '\n' ' ' | tr -s ' ')
  cmdline_source="usr/lib/kernel-cmdline.d + etc/kernel-cmdline.d"
fi
cmdline=$(printf '%s' "$cmdline" | sed 's/^ *//; s/ *$//')
[ -n "$cmdline" ] || fail "no-cmdline" "could not determine the kernel command line"
printf '%s\n' "$cmdline" > "$out/cmdline.txt"
exported="$exported,\"cmdline.txt\""

# Facts from the installed rootfs.
os_release=$(sed -n 's/^PRETTY_NAME="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' "$mnt/etc/os-release" | head -1)
os_version=$(sed -n 's/^VERSION_ID="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' "$mnt/etc/os-release" | head -1)
packages=$(awk -v want=" device-qemu-aarch64 device-qemu-aarch64-kernel-stable linux-postmarketos-stable postmarketos-base postmarketos-base-openrc postmarketos-mkinitfs boot-deploy systemd-boot openrc busybox musl alpine-base networkmanager modemmanager waydroid waydroid-openrc waydroid-nftrules lxc nftables dnsmasq pulseaudio dbus " '
  /^P:/ { name = substr($0, 3) }
  /^V:/ { if (index(want, " " name " ")) printf "%s\"%s\":\"%s\"", (n++ ? "," : ""), name, substr($0, 3) }
' "$mnt/lib/apk/db/installed")
marker_sha=$(sha256sum "$mnt$marker_dst" | cut -d' ' -f1)
product_marker_sha=$(sha256sum "$mnt$product_dst" | cut -d' ' -f1)
core_sha=$(sha256sum "$mnt/usr/bin/ferrix-core" | cut -d' ' -f1)
shell_sha=$(sha256sum "$mnt/usr/bin/ferrix-shell" | cut -d' ' -f1)
screen_sha=$(sha256sum "$mnt/usr/bin/ferrix-basic-screen" | cut -d' ' -f1)
appd_sha=$(sha256sum "$mnt/usr/bin/ferrix-appd" | cut -d' ' -f1)
sandbox_sha=$(sha256sum "$mnt/usr/bin/ferrix-sandbox" | cut -d' ' -f1)
terminal_sha=$(sha256sum "$mnt/usr/bin/ferrix-terminal" | cut -d' ' -f1)
colors_sha=$(sha256sum "$mnt/etc/ferrix/colors.toml" | cut -d' ' -f1)
grants_sha=$(sha256sum "$mnt/usr/share/ferrix/apps/grants.json" | cut -d' ' -f1)
service_shas=""
for bin in ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings; do
  key=$(printf '%s' "$bin" | tr '-' '_')
  service_shas="$service_shas, \"${key}_sha256\": \"$(sha256sum "$mnt/usr/bin/$bin" | cut -d' ' -f1)\""
done
# Production backends are selected at boot by 00-ferrix-product.start; record
# whether the D-Bus services they need are even installed in this image.
nm_installed=false
mm_installed=false
grep -q '^P:networkmanager$' "$mnt/lib/apk/db/installed" && nm_installed=true
grep -q '^P:modemmanager$' "$mnt/lib/apk/db/installed" && mm_installed=true
cat > "$out/image-facts.json" <<JSON
{
  "os_release": {"pretty_name": "$os_release", "version_id": "$os_version"},
  "packages": {$packages},
  "kernel_cmdline": "$cmdline",
  "kernel_cmdline_source": "$cmdline_source",
  "partitions": [$partitions],
  "readiness_marker": {"path": "$marker_dst", "sha256": "$marker_sha", "service": "openrc local", "local_service_was_enabled": $local_enabled},
  "product": {"startup_path": "$product_dst", "startup_sha256": "$product_marker_sha", "ferrix_core_sha256": "$core_sha", "ferrix_shell_sha256": "$shell_sha", "ferrix_basic_screen_sha256": "$screen_sha", "ferrix_appd_sha256": "$appd_sha", "ferrix_sandbox_sha256": "$sandbox_sha", "ferrix_terminal_sha256": "$terminal_sha"$service_shas, "grants_sha256": "$grants_sha", "contacts_fixtures_sha256": "$contacts_fixtures_sha", "colors_path": "/etc/ferrix/colors.toml", "colors_sha256": "$colors_sha", "telephony_backend_default": "mock", "network_backend_default": "mock", "networkmanager_installed": $nm_installed, "modemmanager_installed": $mm_installed},
  "android": {$android_facts},
  "exported": [$exported,"qemu-aarch64.img"]
}
JSON

sync
umount "$mnt/boot"; boot_mounted=0
umount "$mnt"; root_mounted=0
losetup -d "$loop"; loop=""

log "copying disk image to $out/qemu-aarch64.img"
cp --sparse=always "$img" "$out/qemu-aarch64.img"
log "exported: vmlinuz initramfs cmdline.txt qemu-aarch64.img"
