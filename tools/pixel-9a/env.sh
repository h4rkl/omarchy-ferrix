#!/bin/sh

# Host-side environment loader for the exact Pixel 9a target.
#
# Source this file from the repository root to load the canonical profile:
#   . tools/pixel-9a/env.sh
# Or use its read-only command interface:
#   tools/pixel-9a/env.sh validate|print|check [build|recovery|inspection|firmware|all]
#
# The profile is intentionally separate from QEMU's TOML mock profile and from
# the pmOS guest environment. No command here talks to a device or creates an
# output directory.

ferrix_pixel9a_env_die() {
  printf 'pixel-9a-env: %s\n' "$*" >&2
  return 1
}

ferrix_pixel9a_env_repo_root() {
  ferrix_pixel9a_env_candidate=''
  if [ -n "${FERRIX_PIXEL9A_REPO_ROOT:-}" ]; then
    ferrix_pixel9a_env_candidate=$FERRIX_PIXEL9A_REPO_ROOT
  elif [ -f "$PWD/Cargo.toml" ] && [ -d "$PWD/targets" ]; then
    ferrix_pixel9a_env_candidate=$PWD
  else
    # When executed, $0 identifies this file. A sourced POSIX shell has no
    # portable source-file variable, so callers outside the repository must
    # set FERRIX_PIXEL9A_REPO_ROOT explicitly.
    if [ "${0##*/}" = env.sh ]; then
      ferrix_pixel9a_env_candidate=$(CDPATH= cd -- "$(dirname -- "$0")/../.." 2>/dev/null && pwd -P) || ferrix_pixel9a_env_candidate=''
    fi
  fi

  [ -f "$ferrix_pixel9a_env_candidate/Cargo.toml" ] &&
    [ -d "$ferrix_pixel9a_env_candidate/targets" ] || {
      ferrix_pixel9a_env_die "run from the repository root or set FERRIX_PIXEL9A_REPO_ROOT"
      return 1
    }
  ferrix_pixel9a_env_absolute_root=$(CDPATH= cd -P -- "$ferrix_pixel9a_env_candidate" 2>/dev/null && pwd -P) || {
    ferrix_pixel9a_env_die "repository root is not accessible: $ferrix_pixel9a_env_candidate"
    return 1
  }
  printf '%s\n' "$ferrix_pixel9a_env_absolute_root"
}

ferrix_pixel9a_env_load() {
  FERRIX_PIXEL9A_REPO_ROOT=$(ferrix_pixel9a_env_repo_root) || return 1
  FERRIX_PIXEL9A_PROFILE_FILE="$FERRIX_PIXEL9A_REPO_ROOT/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.env"
  FERRIX_PIXEL9A_RUNTIME_PROFILE="$FERRIX_PIXEL9A_REPO_ROOT/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml"
  [ -f "$FERRIX_PIXEL9A_PROFILE_FILE" ] ||
    ferrix_pixel9a_env_die "canonical profile is missing: $FERRIX_PIXEL9A_PROFILE_FILE" || return 1
  [ -f "$FERRIX_PIXEL9A_RUNTIME_PROFILE" ] ||
    ferrix_pixel9a_env_die "target runtime profile is missing: $FERRIX_PIXEL9A_RUNTIME_PROFILE" || return 1

  # The profile is repository-owned data, not an arbitrary user path. Export
  # its fields so a sourced shell and child workflow see the same values.
  set -a
  . "$FERRIX_PIXEL9A_PROFILE_FILE" || {
    ferrix_pixel9a_env_die "could not load canonical profile: $FERRIX_PIXEL9A_PROFILE_FILE"
    set +a
    return 1
  }
  set +a

  FERRIX_PIXEL9A_CACHE_DIR="$FERRIX_PIXEL9A_REPO_ROOT/$FERRIX_PIXEL9A_CACHE_RELATIVE"
  FERRIX_PIXEL9A_OUT_DIR="$FERRIX_PIXEL9A_REPO_ROOT/$FERRIX_PIXEL9A_OUT_RELATIVE"
  FERRIX_PIXEL9A_KERNEL_CACHE_DIR="$FERRIX_PIXEL9A_REPO_ROOT/$FERRIX_PIXEL9A_KERNEL_CACHE_RELATIVE"
  FERRIX_PIXEL9A_KERNEL_OUT="$FERRIX_PIXEL9A_REPO_ROOT/$FERRIX_PIXEL9A_KERNEL_OUT_RELATIVE"
  FERRIX_PIXEL9A_ROOTFS_OUT="$FERRIX_PIXEL9A_REPO_ROOT/$FERRIX_PIXEL9A_ROOTFS_OUT_RELATIVE"
  FERRIX_PIXEL9A_FIRMWARE_OUT="$FERRIX_PIXEL9A_REPO_ROOT/$FERRIX_PIXEL9A_FIRMWARE_OUT_RELATIVE"
  FERRIX_PIXEL9A_IMAGE_OUT="$FERRIX_PIXEL9A_REPO_ROOT/$FERRIX_PIXEL9A_IMAGE_OUT_RELATIVE"

  # These are the explicit interfaces of the product -> rootfs -> boot ->
  # artifact pipeline. Keeping them here prevents each workflow from deriving
  # a subtly different target root when the profile is sourced.
  FERRIX_PIXEL9A_PRODUCT_OUT="$FERRIX_PIXEL9A_OUT_DIR/product/tegu"
  FERRIX_PIXEL9A_PRODUCT_DIR="$FERRIX_PIXEL9A_PRODUCT_OUT"
  FERRIX_PIXEL9A_CARGO_TARGET_DIR="$FERRIX_PIXEL9A_CACHE_DIR/product-target"
  FERRIX_PIXEL9A_KERNEL_SOURCE="$FERRIX_PIXEL9A_KERNEL_CACHE_DIR/source"
  FERRIX_PIXEL9A_KERNEL_BAZEL_CACHE="$FERRIX_PIXEL9A_KERNEL_CACHE_DIR/bazel"
  FERRIX_PIXEL9A_KERNEL_DIST="$FERRIX_PIXEL9A_KERNEL_OUT/dist"
  FERRIX_PIXEL9A_KERNEL_MANIFEST="$FERRIX_PIXEL9A_KERNEL_OUT/build-manifest.tsv"
  FERRIX_PIXEL9A_RADIO_KERNEL_CACHE_DIR="$FERRIX_PIXEL9A_CACHE_DIR/kernel-radio"
  FERRIX_PIXEL9A_RADIO_KERNEL_OUT="$FERRIX_PIXEL9A_OUT_DIR/kernel-radio/tegu-wwan"
  FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$FERRIX_PIXEL9A_RADIO_KERNEL_OUT/dist"
  FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$FERRIX_PIXEL9A_RADIO_KERNEL_OUT/build-manifest.tsv"
  FERRIX_PIXEL9A_ROOTFS_MANIFEST="$FERRIX_PIXEL9A_ROOTFS_OUT/manifest.tsv"
  FERRIX_PIXEL9A_FIRMWARE_DIR="$FERRIX_PIXEL9A_FIRMWARE_OUT/staged"
  FERRIX_PIXEL9A_BOOT_OUT="$FERRIX_PIXEL9A_IMAGE_OUT"
  FERRIX_PIXEL9A_BOOT_MANIFEST="$FERRIX_PIXEL9A_IMAGE_OUT/boot-manifest.tsv"
  FERRIX_PIXEL9A_RADIO_BOOT_OUT="$FERRIX_PIXEL9A_OUT_DIR/images/tegu-wwan"
  FERRIX_PIXEL9A_RADIO_BOOT_MANIFEST="$FERRIX_PIXEL9A_RADIO_BOOT_OUT/boot-manifest.tsv"
  FERRIX_PIXEL9A_PROFILE="$FERRIX_PIXEL9A_RUNTIME_PROFILE"

  # Existing host workflows use these repository-level roots and append their
  # own target-specific subdirectory. The explicit interfaces above take
  # precedence for the native product pipeline; these compatibility roots keep
  # discovery, recovery, and QEMU in their existing namespaces.
  FERRIX_CACHE_DIR="$FERRIX_PIXEL9A_REPO_ROOT/.cache/ferrix"
  FERRIX_OUT_DIR="$FERRIX_PIXEL9A_REPO_ROOT/out"
  FERRIX_PIXEL9A_ENV_LOADED=1
  export FERRIX_PIXEL9A_REPO_ROOT FERRIX_PIXEL9A_PROFILE_FILE FERRIX_PIXEL9A_RUNTIME_PROFILE \
    FERRIX_PIXEL9A_CACHE_DIR FERRIX_PIXEL9A_OUT_DIR \
    FERRIX_PIXEL9A_KERNEL_CACHE_DIR FERRIX_PIXEL9A_KERNEL_OUT \
    FERRIX_PIXEL9A_ROOTFS_OUT FERRIX_PIXEL9A_FIRMWARE_OUT \
    FERRIX_PIXEL9A_IMAGE_OUT FERRIX_PIXEL9A_PRODUCT_OUT \
    FERRIX_PIXEL9A_PRODUCT_DIR \
    FERRIX_PIXEL9A_CARGO_TARGET_DIR FERRIX_PIXEL9A_KERNEL_SOURCE \
    FERRIX_PIXEL9A_KERNEL_BAZEL_CACHE FERRIX_PIXEL9A_KERNEL_DIST \
    FERRIX_PIXEL9A_KERNEL_MANIFEST FERRIX_PIXEL9A_RADIO_KERNEL_CACHE_DIR \
    FERRIX_PIXEL9A_RADIO_KERNEL_OUT FERRIX_PIXEL9A_RADIO_KERNEL_DIST \
    FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST \
    FERRIX_PIXEL9A_ROOTFS_MANIFEST \
    FERRIX_PIXEL9A_FIRMWARE_DIR FERRIX_PIXEL9A_BOOT_OUT \
    FERRIX_PIXEL9A_BOOT_MANIFEST FERRIX_PIXEL9A_RADIO_BOOT_OUT \
    FERRIX_PIXEL9A_RADIO_BOOT_MANIFEST FERRIX_PIXEL9A_PROFILE FERRIX_CACHE_DIR FERRIX_OUT_DIR \
    FERRIX_PIXEL9A_ENV_LOADED
}

ferrix_pixel9a_env_validate() {
  ferrix_pixel9a_env_load || return 1

  [ "$FERRIX_PIXEL9A_PROFILE_SCHEMA" = 1 ] ||
    ferrix_pixel9a_env_die "unsupported profile schema: $FERRIX_PIXEL9A_PROFILE_SCHEMA" || return 1
  [ "$FERRIX_PIXEL9A_PROFILE_KIND" = physical-host-dev ] ||
    ferrix_pixel9a_env_die "profile is not the physical host profile" || return 1
  ferrix_pixel9a_board_id=$(basename -- "$(dirname -- "$(dirname -- "$FERRIX_PIXEL9A_PROFILE_FILE")")")
  [ "$FERRIX_TARGET" = "$ferrix_pixel9a_board_id" ] ||
    ferrix_pixel9a_env_die "profile target does not match its board directory: $FERRIX_TARGET" || return 1
  [ -n "$FERRIX_TARGET" ] || ferrix_pixel9a_env_die "FERRIX_TARGET is empty" || return 1
  [ "$FERRIX_TARGET_CODENAME" = tegu ] || ferrix_pixel9a_env_die "target codename is not tegu" || return 1
  [ "$FERRIX_TARGET_MODEL" = GXQ96 ] || ferrix_pixel9a_env_die "target model is not GXQ96" || return 1
  [ "$FERRIX_TARGET_HARDWARE_REVISION" = MP1.0 ] || ferrix_pixel9a_env_die "target hardware revision is not MP1.0" || return 1
  [ "$FERRIX_EVIDENCE" = "physical:$FERRIX_TARGET" ] ||
    ferrix_pixel9a_env_die "evidence must identify the exact physical target: $FERRIX_EVIDENCE" || return 1
  [ "$FERRIX_TARGET_KIND" = physical ] || ferrix_pixel9a_env_die "target kind is not physical" || return 1
  [ "$FERRIX_BACKEND_MODE" = native-linux ] || ferrix_pixel9a_env_die "backend mode is not native-linux" || return 1
  [ "$FERRIX_MOCK_BACKEND" = disabled ] || ferrix_pixel9a_env_die "mock backend is enabled" || return 1
  [ "$FERRIX_PIXEL9A_SUBSTRATE" = postmarketos ] || ferrix_pixel9a_env_die "unexpected substrate" || return 1
  [ "$FERRIX_PIXEL9A_ANDROID_FRAMEWORK" = disabled ] || ferrix_pixel9a_env_die "Android framework is enabled" || return 1
  [ "$FERRIX_NETWORK_BACKEND" = networkmanager ] || ferrix_pixel9a_env_die "network backend is not NetworkManager" || return 1
  [ "$FERRIX_TELEPHONY_BACKEND" = modemmanager ] || ferrix_pixel9a_env_die "telephony backend is not ModemManager" || return 1
  [ "$FERRIX_BLUETOOTH_BACKEND" = bluez ] || ferrix_pixel9a_env_die "Bluetooth backend is not BlueZ" || return 1
  [ "$FERRIX_AUDIO_BACKEND" = wireplumber ] || ferrix_pixel9a_env_die "audio backend is not WirePlumber" || return 1
  [ "$FERRIX_POWER_BACKEND" = upower ] || ferrix_pixel9a_env_die "power backend is not UPower" || return 1
  [ "$FERRIX_PIXEL9A_HOST_OS" = Linux ] || ferrix_pixel9a_env_die "host OS requirement changed" || return 1
  [ "$FERRIX_PIXEL9A_HOST_ARCH" = x86_64 ] || ferrix_pixel9a_env_die "host architecture requirement changed" || return 1
  case "${FERRIX_PIXEL9A_RUST_TARGET:-}" in
    ''|*[!A-Za-z0-9._-]*) ferrix_pixel9a_env_die "invalid default Rust target: ${FERRIX_PIXEL9A_RUST_TARGET:-}" || return 1 ;;
  esac
  case "${FERRIX_PIXEL9A_RUST_LINKER:-}" in
    ''|*[!A-Za-z0-9._/+:-]*) ferrix_pixel9a_env_die "invalid default Rust linker: ${FERRIX_PIXEL9A_RUST_LINKER:-}" || return 1 ;;
  esac

  for ferrix_pixel9a_path in \
    "$FERRIX_PIXEL9A_CACHE_RELATIVE" \
    "$FERRIX_PIXEL9A_OUT_RELATIVE" \
    "$FERRIX_PIXEL9A_KERNEL_CACHE_RELATIVE" \
    "$FERRIX_PIXEL9A_KERNEL_OUT_RELATIVE" \
    "$FERRIX_PIXEL9A_ROOTFS_OUT_RELATIVE" \
    "$FERRIX_PIXEL9A_FIRMWARE_OUT_RELATIVE" \
    "$FERRIX_PIXEL9A_IMAGE_OUT_RELATIVE"; do
    case "$ferrix_pixel9a_path" in
      ''|/*|*..*|*' '*) ferrix_pixel9a_env_die "unsafe relative path in profile: $ferrix_pixel9a_path" || return 1 ;;
    esac
  done

  [ "$FERRIX_PIXEL9A_CACHE_DIR" != "$FERRIX_PIXEL9A_OUT_DIR" ] ||
    ferrix_pixel9a_env_die 'cache and output roots must be distinct' || return 1
  [ "$FERRIX_PIXEL9A_KERNEL_CACHE_DIR" != "$FERRIX_PIXEL9A_KERNEL_OUT" ] ||
    ferrix_pixel9a_env_die 'kernel cache and kernel output roots must be distinct' || return 1
  [ "$FERRIX_PIXEL9A_PRODUCT_OUT" != "$FERRIX_PIXEL9A_ROOTFS_OUT" ] ||
    ferrix_pixel9a_env_die 'product and rootfs output roots must be distinct' || return 1
  [ "$FERRIX_PIXEL9A_ROOTFS_OUT" != "$FERRIX_PIXEL9A_IMAGE_OUT" ] ||
    ferrix_pixel9a_env_die 'rootfs and image output roots must be distinct' || return 1
  case "$FERRIX_PIXEL9A_KERNEL_CACHE_DIR" in
    "$FERRIX_PIXEL9A_CACHE_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'kernel cache is outside the Pixel cache root' || return 1 ;;
  esac
  case "$FERRIX_PIXEL9A_RADIO_KERNEL_CACHE_DIR" in
    "$FERRIX_PIXEL9A_CACHE_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'radio kernel cache is outside the Pixel cache root' || return 1 ;;
  esac
  case "$FERRIX_PIXEL9A_PRODUCT_OUT" in
    "$FERRIX_PIXEL9A_OUT_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'product output is outside the Pixel output root' || return 1 ;;
  esac
  case "$FERRIX_PIXEL9A_KERNEL_OUT" in
    "$FERRIX_PIXEL9A_OUT_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'kernel output is outside the Pixel output root' || return 1 ;;
  esac
  case "$FERRIX_PIXEL9A_RADIO_KERNEL_OUT" in
    "$FERRIX_PIXEL9A_OUT_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'radio kernel output is outside the Pixel output root' || return 1 ;;
  esac
  case "$FERRIX_PIXEL9A_ROOTFS_OUT" in
    "$FERRIX_PIXEL9A_OUT_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'rootfs output is outside the Pixel output root' || return 1 ;;
  esac
  case "$FERRIX_PIXEL9A_IMAGE_OUT" in
    "$FERRIX_PIXEL9A_OUT_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'image output is outside the Pixel output root' || return 1 ;;
  esac
  case "$FERRIX_PIXEL9A_RADIO_BOOT_OUT" in
    "$FERRIX_PIXEL9A_OUT_DIR"/*) ;;
    *) ferrix_pixel9a_env_die 'radio boot output is outside the Pixel output root' || return 1 ;;
  esac
  [ "$FERRIX_PIXEL9A_RADIO_KERNEL_OUT" != "$FERRIX_PIXEL9A_KERNEL_OUT" ] ||
    ferrix_pixel9a_env_die 'radio and Google kernel output roots must be distinct' || return 1
  [ "$FERRIX_PIXEL9A_RADIO_BOOT_OUT" != "$FERRIX_PIXEL9A_IMAGE_OUT" ] ||
    ferrix_pixel9a_env_die 'radio and Google boot output roots must be distinct' || return 1
}

ferrix_pixel9a_env_tools_for() {
  case "${1:-all}" in
    build)
      printf '%s\n' "$FERRIX_PIXEL9A_HOST_TOOLS_BUILD"
      ;;
    recovery) printf '%s\n' "$FERRIX_PIXEL9A_HOST_TOOLS_RECOVERY" ;;
    inspection) printf '%s\n' "$FERRIX_PIXEL9A_HOST_TOOLS_INSPECTION" ;;
    firmware) printf '%s\n' "$FERRIX_PIXEL9A_HOST_TOOLS_FIRMWARE" ;;
    all)
      printf '%s %s %s %s\n' \
        "$(ferrix_pixel9a_env_tools_for build)" \
        "$FERRIX_PIXEL9A_HOST_TOOLS_RECOVERY" \
        "$FERRIX_PIXEL9A_HOST_TOOLS_INSPECTION" \
        "$FERRIX_PIXEL9A_HOST_TOOLS_FIRMWARE"
      ;;
    *) ferrix_pixel9a_env_die "unknown tool group: $1" || return 1 ;;
  esac
}

ferrix_pixel9a_env_check() {
  ferrix_pixel9a_env_validate || return 1
  ferrix_pixel9a_env_os=$(uname -s)
  ferrix_pixel9a_env_arch=$(uname -m)
  [ "$ferrix_pixel9a_env_os" = "$FERRIX_PIXEL9A_HOST_OS" ] ||
    ferrix_pixel9a_env_die "native Linux host required; detected $ferrix_pixel9a_env_os" || return 1
  [ "$ferrix_pixel9a_env_arch" = "$FERRIX_PIXEL9A_HOST_ARCH" ] ||
    ferrix_pixel9a_env_die "x86_64 host required; detected $ferrix_pixel9a_env_arch" || return 1

  ferrix_pixel9a_env_group=${1:-all}
  ferrix_pixel9a_env_missing=''
  for ferrix_pixel9a_tool in $(ferrix_pixel9a_env_tools_for "$ferrix_pixel9a_env_group"); do
    command -v "$ferrix_pixel9a_tool" >/dev/null 2>&1 ||
      ferrix_pixel9a_env_missing="$ferrix_pixel9a_env_missing $ferrix_pixel9a_tool"
  done
  [ -z "$ferrix_pixel9a_env_missing" ] ||
    ferrix_pixel9a_env_die "missing $ferrix_pixel9a_env_group host tools:$ferrix_pixel9a_env_missing" || return 1

  if [ "$ferrix_pixel9a_env_group" = build ] || [ "$ferrix_pixel9a_env_group" = all ]; then
    ferrix_pixel9a_env_rust_target=${FERRIX_PIXEL9A_PRODUCT_TARGET:-$FERRIX_PIXEL9A_RUST_TARGET}
    case "$ferrix_pixel9a_env_rust_target" in
      ''|*[!A-Za-z0-9._-]*) ferrix_pixel9a_env_die "invalid Rust target: $ferrix_pixel9a_env_rust_target" || return 1 ;;
    esac
    [ "$ferrix_pixel9a_env_rust_target" = aarch64-unknown-linux-musl ] ||
      ferrix_pixel9a_env_die "physical Pixel product requires the pinned Rust target aarch64-unknown-linux-musl (got $ferrix_pixel9a_env_rust_target)" || return 1
    ferrix_pixel9a_env_target_libdir=$(rustc --print target-libdir --target "$ferrix_pixel9a_env_rust_target" 2>/dev/null) ||
      ferrix_pixel9a_env_die "rustc cannot resolve target $ferrix_pixel9a_env_rust_target" || return 1
    [ -d "$ferrix_pixel9a_env_target_libdir" ] ||
      ferrix_pixel9a_env_die "Rust target standard library is not installed: $ferrix_pixel9a_env_rust_target" || return 1
    find "$ferrix_pixel9a_env_target_libdir" -maxdepth 1 -type f -name 'libstd-*.rlib' -print -quit 2>/dev/null | grep -q . ||
      ferrix_pixel9a_env_die "Rust target standard library is not installed: $ferrix_pixel9a_env_rust_target" || return 1
    ferrix_pixel9a_env_linker=${FERRIX_PIXEL9A_RUST_LINKER:-}
    [ -n "$ferrix_pixel9a_env_linker" ] ||
      ferrix_pixel9a_env_die 'target Rust linker is not configured' || return 1
    command -v "$ferrix_pixel9a_env_linker" >/dev/null 2>&1 ||
      ferrix_pixel9a_env_die "target Rust linker is not installed: $ferrix_pixel9a_env_linker" || return 1
  fi
  printf '%s\n' "PASS pixel-9a-env: native Linux $ferrix_pixel9a_env_arch host and $ferrix_pixel9a_env_group tools available"
}

ferrix_pixel9a_env_print() {
  ferrix_pixel9a_env_validate || return 1
  printf 'profile_file=%s\n' "$FERRIX_PIXEL9A_PROFILE_FILE"
  printf 'runtime_profile=%s\n' "$FERRIX_PIXEL9A_RUNTIME_PROFILE"
  printf 'profile_kind=%s\n' "$FERRIX_PIXEL9A_PROFILE_KIND"
  printf 'target=%s\n' "$FERRIX_TARGET"
  printf 'evidence=%s\n' "$FERRIX_EVIDENCE"
  printf 'backend_mode=%s\n' "$FERRIX_BACKEND_MODE"
  printf 'substrate=%s\n' "$FERRIX_PIXEL9A_SUBSTRATE"
  printf 'cache_dir=%s\n' "$FERRIX_PIXEL9A_CACHE_DIR"
  printf 'out_dir=%s\n' "$FERRIX_PIXEL9A_OUT_DIR"
  printf 'kernel_cache_dir=%s\n' "$FERRIX_PIXEL9A_KERNEL_CACHE_DIR"
  printf 'kernel_out=%s\n' "$FERRIX_PIXEL9A_KERNEL_OUT"
  printf 'rootfs_out=%s\n' "$FERRIX_PIXEL9A_ROOTFS_OUT"
  printf 'firmware_out=%s\n' "$FERRIX_PIXEL9A_FIRMWARE_OUT"
  printf 'image_out=%s\n' "$FERRIX_PIXEL9A_IMAGE_OUT"
  printf 'product_out=%s\n' "$FERRIX_PIXEL9A_PRODUCT_OUT"
  printf 'cargo_target_dir=%s\n' "$FERRIX_PIXEL9A_CARGO_TARGET_DIR"
  printf 'kernel_source=%s\n' "$FERRIX_PIXEL9A_KERNEL_SOURCE"
  printf 'kernel_bazel_cache=%s\n' "$FERRIX_PIXEL9A_KERNEL_BAZEL_CACHE"
  printf 'kernel_dist=%s\n' "$FERRIX_PIXEL9A_KERNEL_DIST"
  printf 'radio_kernel_cache_dir=%s\n' "$FERRIX_PIXEL9A_RADIO_KERNEL_CACHE_DIR"
  printf 'radio_kernel_out=%s\n' "$FERRIX_PIXEL9A_RADIO_KERNEL_OUT"
  printf 'radio_kernel_dist=%s\n' "$FERRIX_PIXEL9A_RADIO_KERNEL_DIST"
  printf 'radio_kernel_manifest=%s\n' "$FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST"
  printf 'rootfs_manifest=%s\n' "$FERRIX_PIXEL9A_ROOTFS_MANIFEST"
  printf 'firmware_dir=%s\n' "$FERRIX_PIXEL9A_FIRMWARE_DIR"
  printf 'boot_out=%s\n' "$FERRIX_PIXEL9A_BOOT_OUT"
  printf 'boot_manifest=%s\n' "$FERRIX_PIXEL9A_BOOT_MANIFEST"
  printf 'radio_boot_out=%s\n' "$FERRIX_PIXEL9A_RADIO_BOOT_OUT"
  printf 'radio_boot_manifest=%s\n' "$FERRIX_PIXEL9A_RADIO_BOOT_MANIFEST"
  printf 'rust_target=%s\n' "${FERRIX_PIXEL9A_PRODUCT_TARGET:-$FERRIX_PIXEL9A_RUST_TARGET}"
  printf 'rust_linker=%s\n' "$FERRIX_PIXEL9A_RUST_LINKER"
  printf 'host_os=%s\n' "$FERRIX_PIXEL9A_HOST_OS"
  printf 'host_arch=%s\n' "$FERRIX_PIXEL9A_HOST_ARCH"
}

ferrix_pixel9a_env_main() {
  ferrix_pixel9a_env_command=${1:-}
  case "$ferrix_pixel9a_env_command" in
    validate) [ "$#" -eq 1 ] || return 2; ferrix_pixel9a_env_validate ;;
    print) [ "$#" -eq 1 ] || return 2; ferrix_pixel9a_env_print ;;
    check) [ "$#" -le 2 ] || return 2; ferrix_pixel9a_env_check "${2:-all}" ;;
    '')
      printf '%s\n' 'usage: tools/pixel-9a/env.sh validate|print|check [build|recovery|inspection|firmware|all]' >&2
      return 2
      ;;
    *) printf '%s\n' 'usage: tools/pixel-9a/env.sh validate|print|check [build|recovery|inspection|firmware|all]' >&2; return 2 ;;
  esac
}

if ! ferrix_pixel9a_env_load; then
  case "${0##*/}" in
    env.sh) exit 1 ;;
    *) return 1 2>/dev/null || exit 1 ;;
  esac
fi

# A sourced POSIX shell keeps its caller's $0, so this guard avoids running the
# command interface during `. tools/pixel-9a/env.sh`.
if [ "${0##*/}" = env.sh ]; then
  ferrix_pixel9a_env_main "$@"
fi
