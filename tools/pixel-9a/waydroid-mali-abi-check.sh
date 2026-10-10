#!/bin/sh
# Offline ABI check for the stock Mali overlay
# (tools/pixel-9a/waydroid-mali-overlay.sh) against a Waydroid vendor image
# (tools/android/waydroid-lineage-build.sh). Never touches a device.
#
#   sh tools/pixel-9a/waydroid-mali-abi-check.sh VENDOR_IMG OVERLAY_VENDOR_DIR [SYSTEM_LIB_DIRS]
#
#   VENDOR_IMG          Waydroid vendor.img (ext4)
#   OVERLAY_VENDOR_DIR  <overlay output>/overlay/vendor
#   SYSTEM_LIB_DIRS     optional colon-separated unpacked system library
#                       dirs (LL-NDK), e.g. the build's
#                       out/target/product/<product>/system/lib64 and
#                       .../apex/com.android.runtime/lib64/bionic; without
#                       them, libraries not in the overlay or the vendor
#                       image are assumed to come from the system and their
#                       symbols are not resolved.
#
# Fails when
#   shadow:   an overlay library replaces a Waydroid vendor library and a
#             Waydroid vendor binary uses a symbol only the Waydroid copy
#             defines (the overlay would break that binary);
#   missing:  a stock blob NEEDs a library found nowhere;
#   unresolved: a stock blob's strong undefined symbol is defined by no
#             library in its NEEDED tree (only with SYSTEM_LIB_DIRS).
# Needs debugfs and readelf (reported, never installed).
set -eu
die() { printf 'FAIL waydroid-mali-abi-check: %s\n' "$*" >&2; exit 1; }
[ $# -ge 2 ] || { sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
vendor_img=$1
overlay=$2
system_lib=${3:-}
for t in debugfs readelf awk sort comm; do
  command -v "$t" >/dev/null 2>&1 || die "missing prerequisite '$t'; install it explicitly"
done
[ -f "$vendor_img" ] || die "no vendor image: $vendor_img"
[ -d "$overlay/lib64" ] || die "no overlay lib64 under $overlay"
old_ifs=$IFS; IFS=:
for d in $system_lib; do [ -d "$d" ] || die "no system library dir: $d"; done
IFS=$old_ifs

work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-mali-abi.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/vendor"
debugfs -R "rdump /lib64 /bin $work/vendor" "$vendor_img" >/dev/null 2>&1 ||
  die "cannot read /lib64 and /bin from $vendor_img"
# Search order: overlay first (it shadows), then the Waydroid vendor (with
# its sphal hw/egl dirs), then the system dirs.
search="$overlay/lib64:$overlay/lib64/hw:$overlay/lib64/egl:$work/vendor/lib64:$work/vendor/lib64/hw:$work/vendor/lib64/egl${system_lib:+:$system_lib}"

# Fields are read from the end: IFUNC types print as "<OS specific>: 10".
defs() { readelf -W --dyn-syms "$1" 2>/dev/null | awk '$1 ~ /^[0-9]+:$/ && NF >= 8 && $(NF-1) != "UND" { n = $NF; sub(/@.*/, "", n); print n }' | sort -u; }
strong_undefs() { readelf -W --dyn-syms "$1" 2>/dev/null | awk '$1 ~ /^[0-9]+:$/ && NF >= 8 && $(NF-1) == "UND" && $(NF-3) != "WEAK" { n = $NF; sub(/@.*/, "", n); print n }' | sort -u; }
needed() { readelf -d "$1" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'; }
elf() { readelf -h "$1" >/dev/null 2>&1; }
locate() {
  old_ifs=$IFS; IFS=:
  for d in $search; do
    [ -f "$d/$1" ] && { IFS=$old_ifs; printf '%s\n' "$d/$1"; return 0; }
  done
  IFS=$old_ifs
  return 1
}

failures=0

# shadow
for lib in $(cd "$overlay/lib64" && ls); do
  [ -f "$overlay/lib64/$lib" ] && [ -f "$work/vendor/lib64/$lib" ] || continue
  defs "$overlay/lib64/$lib" >"$work/overlay.defs"
  defs "$work/vendor/lib64/$lib" >"$work/vendor.defs"
  find "$work/vendor" -type f | while IFS= read -r user; do
    needed "$user" | grep -qxF "$lib" || continue
    lost=$(strong_undefs "$user" | comm -12 - "$work/vendor.defs" | comm -23 - "$work/overlay.defs" | wc -l | tr -d ' ')
    [ "$lost" = 0 ] || printf 'shadow %s breaks %s (%s symbols)\n' "$lib" "${user#"$work"/}" "$lost"
  done >>"$work/shadow"
done
[ ! -s "$work/shadow" ] || { cat "$work/shadow"; failures=$((failures + $(wc -l <"$work/shadow"))); }

# missing / unresolved
find "$overlay" -type f | while IFS= read -r blob; do
  elf "$blob" || continue
  : >"$work/tree"; queue=$(needed "$blob"); seen=" "
  while [ -n "$queue" ]; do
    set -- $queue; lib=$1; shift; queue="$*"
    case "$seen" in *" $lib "*) continue ;; esac
    seen="$seen$lib "
    if path=$(locate "$lib"); then
      printf '%s\n' "$path" >>"$work/tree"
      queue="$queue $(needed "$path" | tr '\n' ' ')"
    elif [ -n "$system_lib" ]; then
      printf 'missing %s needed by %s\n' "$lib" "${blob#"$overlay"/}"
    fi
  done
  [ -n "$system_lib" ] || continue
  while IFS= read -r path; do defs "$path"; done <"$work/tree" | sort -u >"$work/tree.defs"
  n=$(strong_undefs "$blob" | comm -23 - "$work/tree.defs" | wc -l | tr -d ' ')
  [ "$n" = 0 ] || printf 'unresolved %s: %s symbols (e.g. %s)\n' "${blob#"$overlay"/}" "$n" \
    "$(strong_undefs "$blob" | comm -23 - "$work/tree.defs" | head -n 3 | tr '\n' ' ')"
done >"$work/resolve"
[ ! -s "$work/resolve" ] || { cat "$work/resolve"; failures=$((failures + $(wc -l <"$work/resolve"))); }

[ "$failures" = 0 ] || die "$failures finding(s)"
printf 'PASS waydroid-mali-abi-check: %s against %s%s\n' "$overlay" "$vendor_img" \
  "$([ -n "$system_lib" ] && printf ' (symbols resolved with %s)' "$system_lib" || printf ' (system symbols not resolved)')"
