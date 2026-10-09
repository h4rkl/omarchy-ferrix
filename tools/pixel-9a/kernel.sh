#!/bin/sh
set -eu

# Safe host-side orchestration around Google's repo and Kleaf/Bazel tools.
# This file deliberately contains no device transport or provisioning path.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pins=${FERRIX_PIXEL9A_KERNEL_PINS:-"$target/kernel-sources.toml"}
cache_root=${FERRIX_CACHE_DIR:-"$root/.cache/ferrix"}
source_root=${FERRIX_PIXEL9A_KERNEL_SOURCE:-"$cache_root/pixel-9a/kernel/source"}
bazel_cache=${FERRIX_PIXEL9A_KERNEL_BAZEL_CACHE:-"$cache_root/pixel-9a/kernel/bazel"}
out_root=${FERRIX_OUT_DIR:-"$root/out"}
kernel_out=${FERRIX_PIXEL9A_KERNEL_OUT:-"$out_root/pixel-9a/kernel/tegu"}
# Bazel runs from the source root, so a relative output must be anchored to the
# repository before it reaches --destdir.
case "$kernel_out" in /*) ;; *) kernel_out="$root/$kernel_out" ;; esac
dist="$kernel_out/dist"
manifest="$kernel_out/build-manifest.tsv"
module_list="$target/pmos/device-google-tegu/modules-initfs"
config_fragment="$target/kernel-config.tegu"
gki_fragment="$target/kernel-config.ferrix-gki"
minimum_kib=${FERRIX_PIXEL9A_KERNEL_MIN_FREE_KIB:-83886080}
sync_confirmation=I-understand-this-downloads-large-kernel-sources

die() {
  printf 'FAIL pixel-9a-kernel: %s\n' "$*" >&2
  exit 1
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    die "missing sha256sum or shasum for kernel config provenance"
  fi
}

pin_top() {
  key=$1
  awk -v key="$key" '
    $0 ~ "^" key "[[:space:]]*=" {
      value=$0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      print value
      found=1
      exit
    }
    END { if (!found) exit 1 }
  ' "$pins"
}

source_field() {
  wanted=$1
  field=$2
  awk -v wanted="$wanted" -v field="$field" '
    /^\[\[source\]\]/ { active=0 }
    /^name[[:space:]]*=/ {
      value=$0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      active=(value == wanted)
    }
    active && $0 ~ "^" field "[[:space:]]*=" {
      value=$0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      print value
      found=1
      exit
    }
    END { if (!found) exit 1 }
  ' "$pins"
}

source_path() {
  case "$1" in
    gki-common) printf '%s\n' aosp ;;
    tegu-board) printf '%s\n' private/devices/google/tegu ;;
    zuma-board) printf '%s\n' private/devices/google/zuma ;;
    zumapro-soc) printf '%s\n' private/devices/google/zumapro ;;
    soc-gs) printf '%s\n' private/google-modules/soc/gs ;;
    # The pinned kernel manifest checks out BCM4383 at this exact path (the
    # repository name contains bcmdhd, but the manifest project path does
    # not).  Keep the checkout path separate from the upstream URL/name.
    wifi-bcm4383) printf '%s\n' private/google-modules/wlan/bcm4383 ;;
    bluetooth-broadcom) printf '%s\n' private/google-modules/bluetooth/broadcom ;;
    modem-s5300) printf '%s\n' private/google-modules/radio/samsung/s5300 ;;
    display-common) printf '%s\n' private/google-modules/display/common ;;
    display-tegu-panels) printf '%s\n' private/google-modules/display/panels/tegu ;;
    audio-amplifiers) printf '%s\n' private/google-modules/amplifiers ;;
    audio-aoc) printf '%s\n' private/google-modules/aoc ;;
    audio-aoc-ipc) printf '%s\n' private/google-modules/aoc_ipc ;;
    battery-management) printf '%s\n' private/google-modules/bms ;;
    touch-common) printf '%s\n' private/google-modules/touch/common ;;
    touch-synaptics) printf '%s\n' private/google-modules/touch/synaptics ;;
    *) die "kernel-sources.toml contains an unmapped source name: $1" ;;
  esac
}

component_names() {
  awk '
    /^name[[:space:]]*=/ {
      value=$0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      if (value != "manifest" && value != "superproject") print value
    }
  ' "$pins"
}

existing_parent() {
  candidate=$1
  while [ ! -d "$candidate" ]; do
    parent=$(dirname -- "$candidate")
    [ "$parent" != "$candidate" ] || die "cannot resolve an existing parent for $1"
    candidate=$parent
  done
  printf '%s\n' "$candidate"
}

print_resolution() {
  branch=$(pin_top branch) || die "missing branch in $pins"
  manifest_revision=$(source_field manifest revision) || die "missing manifest revision"
  manifest_url=$(source_field manifest url) || die "missing manifest URL"
  printf '%s\n' "target: google-pixel-9a-tegu-gxq96-mp1-0"
  printf '%s\n' "pins: $pins"
  printf '%s\n' "branch: $branch"
  printf '%s\n' "manifest revision: $manifest_revision"
  printf '%s\n' "source: $source_root"
  printf '%s\n' "bazel cache: $bazel_cache"
  printf '%s\n' "dist: $dist"
  printf '%s\n' "manifest output: $manifest"
  printf '%s\n' "target config fragment: $config_fragment"
  printf '%s\n' "sync command: repo init -u $manifest_url -b $manifest_revision -m default.xml"
  printf '%s\n' "sync command: repo sync -c --no-tags --fail-fast"
  printf '%s\n' "build command: tools/bazel --output_user_root=$bazel_cache run --kernel_package=@//aosp --config=stamp --config=tegu //private/devices/google/tegu:zumapro_tegu_dist -- --destdir=$dist"
}

preflight() {
  print_resolution
  [ -f "$pins" ] || die "pin file does not exist: $pins"
  os=$(uname -s)
  arch=$(uname -m)
  [ "$os" = Linux ] || die "Google Kleaf Pixel builds require a native Linux host; detected $os/$arch"
  [ "$arch" = x86_64 ] || die "the pinned Google host prebuilts require Linux x86_64; detected $arch"
  for command_name in git repo bash awk find sort sha256sum stat; do
    command -v "$command_name" >/dev/null 2>&1 ||
      die "missing prerequisite '$command_name'; install it explicitly before retrying"
  done
  repo version >/dev/null 2>&1 || die "repo is present but did not run successfully"
  disk_parent=$(existing_parent "$cache_root")
  free_kib=$(df -Pk "$disk_parent" | awk 'NR == 2 { print $4 }')
  case "$free_kib" in
    ''|*[!0-9]*) die "could not determine free space below $disk_parent" ;;
  esac
  [ "$free_kib" -ge "$minimum_kib" ] ||
    die "insufficient free space: ${free_kib} KiB available, ${minimum_kib} KiB required"
  printf '%s\n' "PASS pixel-9a-kernel host: Linux/x86_64, prerequisites present, ${free_kib} KiB free"
}

verify_git_revision() {
  label=$1
  directory=$2
  expected=$3
  [ -d "$directory" ] || die "$label checkout is missing: $directory"
  actual=$(git -C "$directory" rev-parse HEAD 2>/dev/null) || die "$label is not a readable Git checkout"
  [ "$actual" = "$expected" ] || die "$label revision mismatch: expected $expected, found $actual"
  git -C "$directory" diff --quiet --ignore-submodules -- || die "$label checkout has unstaged changes"
  git -C "$directory" diff --cached --quiet --ignore-submodules -- || die "$label checkout has staged changes"
  # Untracked source files can change a Kleaf build just as surely as a
  # tracked edit.  `git diff` does not report them, so reject them explicitly
  # before treating this checkout as the pinned source baseline.
  if git -C "$directory" status --porcelain --untracked-files=all | grep -q .; then
    die "$label checkout has untracked changes"
  fi
  printf '%s\t%s\t%s\n' source "$label" "$actual"
}

verify_sources() {
  [ -f "$config_fragment" ] || die "target kernel config fragment is missing: $config_fragment"
  [ -d "$source_root/.repo" ] || die "kernel source is not initialized: run just pixel-9a-kernel-sync"
  expected=$(source_field manifest revision) || die "missing manifest revision"
  verify_git_revision manifest "$source_root/.repo/manifests" "$expected"
  expected=$(source_field superproject revision) || die "missing superproject revision"
  verify_git_revision superproject "$source_root/.repo/ferrix-superproject" "$expected"
  count=0
  for name in $(component_names); do
    path=$(source_path "$name")
    expected=$(source_field "$name" revision) || die "missing revision for $name"
    verify_git_revision "$name" "$source_root/$path" "$expected"
    count=$((count + 1))
  done
  [ "$count" -eq 16 ] || die "expected 16 pinned build components, found $count"
  [ -x "$source_root/tools/bazel" ] || die "Kleaf launcher is missing: $source_root/tools/bazel"
  [ -f "$source_root/private/devices/google/tegu/BUILD.bazel" ] || die "tegu Kleaf target is missing"
  source_config="$source_root/private/devices/google/tegu/tegu_defconfig"
  [ -f "$source_config" ] || die "tegu Kleaf config fragment is missing: $source_config"
  cmp -s "$config_fragment" "$source_config" ||
    die "tegu Kleaf config fragment differs from the checked-in pinned fragment"
  printf '%s\t%s\t%s\n' config tegu_defconfig "$(sha256_of "$config_fragment")"
  printf '%s\n' "PASS pixel-9a-kernel sources: manifest, superproject, and $count components verified"
}

builtin_module() {
  [ -f "$2" ] || return 1
  tr '-' '_' <"$2" | grep -q "/$(printf '%s' "$1" | tr '-' '_')\\.ko$"
}

verify_required_modules() {
  [ -f "$module_list" ] || die "target initfs module list is missing: $module_list"
  while IFS= read -r module; do
    case "$module" in
      ''|'#'*) continue ;;
    esac
    find "$dist" -type f -name "$module.ko" -print -quit | grep -q . && continue
    # GKI compiles core storage/filesystem drivers (dm, ext4, UFS) into the
    # kernel; Kleaf lists those in modules.builtin instead of emitting a .ko.
    builtin_module "$module" "$dist/modules.builtin" ||
      die "Kleaf dist is missing required tegu module: $module.ko"
  done < "$module_list"
}

sync_sources() {
  preflight
  [ "${FERRIX_PIXEL9A_KERNEL_SYNC:-}" = "$sync_confirmation" ] ||
    die "large source sync is opt-in; set FERRIX_PIXEL9A_KERNEL_SYNC=$sync_confirmation"
  if [ -e "$source_root" ] && [ ! -d "$source_root/.repo" ]; then
    die "source path exists but is not a repo checkout: $source_root"
  fi
  mkdir -p "$source_root" "$bazel_cache"
  manifest_url=$(source_field manifest url)
  manifest_revision=$(source_field manifest revision)
  superproject_url=$(source_field superproject url)
  superproject_revision=$(source_field superproject revision)
  printf '%s\n' "+ cd $source_root"
  printf '%s\n' "+ repo init -u $manifest_url -b $manifest_revision -m default.xml"
  (cd "$source_root" && repo init -u "$manifest_url" -b "$manifest_revision" -m default.xml)
  actual=$(git -C "$source_root/.repo/manifests" rev-parse HEAD)
  [ "$actual" = "$manifest_revision" ] ||
    die "repo initialized unexpected manifest revision: $actual"
  super="$source_root/.repo/ferrix-superproject"
  if [ ! -d "$super/.git" ]; then
    mkdir -p "$super"
    git -C "$super" init
    git -C "$super" remote add origin "$superproject_url"
  fi
  actual_url=$(git -C "$super" remote get-url origin)
  [ "$actual_url" = "$superproject_url" ] || die "superproject remote mismatch: $actual_url"
  printf '%s\n' "+ git fetch pinned superproject $superproject_revision"
  git -C "$super" fetch --depth=1 origin "$superproject_revision"
  git -C "$super" checkout --detach FETCH_HEAD
  printf '%s\n' "+ repo sync -c --no-tags --fail-fast"
  (cd "$source_root" && repo sync -c --no-tags --fail-fast)
  verify_sources
}

write_manifest() {
  tmp="$manifest.tmp.$$"
  trap 'rm -f "$tmp"' EXIT HUP INT TERM
  {
    printf 'schema_version\t1\n'
    printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
    printf 'branch\t%s\n' "$(pin_top branch)"
    printf 'build_system\tgoogle-kleaf-bazel\n'
    printf 'kernel_package\t@//aosp\n'
    printf 'dist\t%s\n' "$dist"
    printf 'source\tmanifest\t%s\n' "$(git -C "$source_root/.repo/manifests" rev-parse HEAD)"
    printf 'source\tsuperproject\t%s\n' "$(git -C "$source_root/.repo/ferrix-superproject" rev-parse HEAD)"
    printf 'config\ttegu_defconfig\t%s\n' "$(sha256_of "$config_fragment")"
    printf 'config\tferrix_gki_defconfig\t%s\n' "$(sha256_of "$gki_fragment")"
    for name in $(component_names); do
      path=$(source_path "$name")
      printf 'source\t%s\t%s\n' "$name" "$(git -C "$source_root/$path" rev-parse HEAD)"
    done
    find "$dist" -type f -print | LC_ALL=C sort | while IFS= read -r artifact; do
      relative=${artifact#"$dist"/}
      size=$(stat -c %s "$artifact")
      hash=$(sha256sum "$artifact" | awk '{ print $1 }')
      printf 'artifact\t%s\t%s\t%s\n' "$relative" "$size" "$hash"
    done
  } >"$tmp"
  mv "$tmp" "$manifest"
  trap - EXIT HUP INT TERM
}

build_kernel() {
  preflight
  verify_sources
  [ "${FERRIX_PIXEL9A_KERNEL_BUILD:-}" = I-understand-this-runs-a-large-kleaf-build ] ||
    die "kernel build is opt-in; set FERRIX_PIXEL9A_KERNEL_BUILD=I-understand-this-runs-a-large-kleaf-build"
  if [ -d "$dist" ] && find "$dist" -mindepth 1 -print -quit | grep -q .; then
    die "dist path is not empty; choose a fresh FERRIX_PIXEL9A_KERNEL_OUT: $dist"
  fi
  mkdir -p "$dist" "$bazel_cache"
  # Ferrix-owned Bazel package for the GKI config fragment; Google's tree is
  # left untouched.
  mkdir -p "$source_root/ferrix"
  cp "$gki_fragment" "$source_root/ferrix/ferrix_gki_defconfig"
  printf '%s\n' 'exports_files(["ferrix_gki_defconfig"])' >"$source_root/ferrix/BUILD.bazel"
  # Overrides follow the --config expansions (device.bazelrc sets
  # --use_prebuilt_gki); Bazel keeps the last value. Use the negated form:
  # Kleaf's wrapper reads --use_prebuilt_gki=VALUE as a build number to
  # download, so =false would still fetch a prebuilt.
  set -- --output_user_root="$bazel_cache" run \
    --kernel_package=@//aosp \
    --config=stamp \
    --config=tegu \
    --nouse_prebuilt_gki \
    --nouse_signed_prebuilts \
    --defconfig_fragment=//ferrix:ferrix_gki_defconfig \
    //private/devices/google/tegu:zumapro_tegu_dist \
    -- --destdir="$dist"
  printf '%s\n' "+ cd $source_root"
  printf '%s\n' "+ tools/bazel $*"
  (
    cd "$source_root"
    tools/bazel "$@"
  )
  find "$dist" -type f -print -quit | grep -q . || die "Kleaf completed without dist artifacts"
  verify_required_modules
  write_manifest
  printf '%s\n' "PASS pixel-9a-kernel build: artifacts and hashes recorded in $manifest"
}

usage() {
  printf '%s\n' "usage: tools/pixel-9a/kernel.sh preflight|sync|verify|build" >&2
  exit 2
}

case "${1:-}" in
  preflight) [ "$#" -eq 1 ] || usage; preflight ;;
  sync) [ "$#" -eq 1 ] || usage; sync_sources ;;
  verify) [ "$#" -eq 1 ] || usage; print_resolution; verify_sources ;;
  build) [ "$#" -eq 1 ] || usage; build_kernel ;;
  *) usage ;;
esac
