#!/bin/sh
set -eu

# Device-free product artifact workflow for Google Pixel 9a tegu / GXQ96 /
# MP1.0. Product compilation is performed directly on a provisioned native
# Linux x86_64 host. Alternate host environments and cross-compilation
# substitutes are not accepted for this target-local product path.
#
# This script does not download sources, contact hardware, or assemble a guest
# image. It creates a reproducible product handoff directory for the target's
# postmarketOS package workflow.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_dir="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
profile_path="$target_dir/dev/profile.toml"
apps_dir="$root/apps"
colors_path="$root/themes/ferrix/colors.toml"
target_id=google-pixel-9a-tegu-gxq96-mp1-0
codename=tegu
hardware_revision=MP1.0
evidence="physical:$target_id"
product_target=${FERRIX_PIXEL9A_PRODUCT_TARGET:-${FERRIX_PIXEL9A_RUST_TARGET:-aarch64-unknown-linux-musl}}
case "$product_target" in
  aarch64-unknown-linux-musl)
    product_linker=${FERRIX_PIXEL9A_RUST_LINKER:-aarch64-linux-musl-gcc}
    ;;
  *)
    product_linker=${FERRIX_PIXEL9A_RUST_LINKER:-}
    ;;
esac
production_features='ferrix-networkd/production,ferrix-telephonyd/production'
build_confirmation=I-understand-this-builds-the-pixel-9a-product
product_bins='ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files'

absolute_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$root" "$1" ;;
  esac
}

out_root=$(absolute_path "${FERRIX_OUT_DIR:-$root/out}")
product_out=$(absolute_path "${FERRIX_PIXEL9A_PRODUCT_OUT:-$out_root/pixel-9a/product/tegu}")
cache_root=$(absolute_path "${FERRIX_PIXEL9A_CACHE_DIR:-${FERRIX_CACHE_DIR:-$root/.cache/ferrix}/pixel-9a}")
cargo_target_dir=$(absolute_path "${FERRIX_PIXEL9A_CARGO_TARGET_DIR:-$cache_root/product-target}")
manifest_override=${FERRIX_PIXEL9A_PRODUCT_MANIFEST:-}
if [ -n "$manifest_override" ]; then
  manifest_path=$(absolute_path "$manifest_override")
else
  manifest_path="$product_out/manifest.tsv"
fi

die() {
  printf 'FAIL pixel-9a-product: %s\n' "$*" >&2
  exit 1
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

toolchain_channel() {
  sed -n 's/^channel = "\([^"]*\)"$/\1/p' "$root/rust-toolchain.toml" | head -1
}

file_size() {
  wc -c <"$1" | tr -d '[:space:]'
}

check_host() {
  host_os=$(uname -s)
  host_arch=$(uname -m)
  [ "$host_os" = Linux ] || die "native Linux host required; detected $host_os/$host_arch"
  [ "$host_arch" = x86_64 ] || die "native Linux x86_64 host required; detected $host_os/$host_arch"
}

check_target() {
  case "$product_target" in
    ''|*[!A-Za-z0-9._-]*) die "unsafe Rust target override: $product_target" ;;
  esac
  [ "$product_target" = aarch64-unknown-linux-musl ] ||
    die "physical Pixel product requires the pinned Rust target aarch64-unknown-linux-musl (got $product_target)"
}

check_tools() {
  for command_name in awk chmod cp dirname find grep mkdir mktemp mv rustc sed tr uname wc; do
    command -v "$command_name" >/dev/null 2>&1 ||
      die "missing prerequisite '$command_name'; install it explicitly before retrying"
  done
  command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 ||
    die 'missing sha256sum or shasum'
  command -v cargo >/dev/null 2>&1 || die "missing prerequisite 'cargo'; install it explicitly before retrying"
}

check_rust_target() {
  target_libdir=$(rustc --print target-libdir --target "$product_target" 2>/dev/null) ||
    die "rustc cannot resolve target $product_target; install the pinned Rust target explicitly"
  [ -d "$target_libdir" ] ||
    die "Rust target standard library is not installed: $product_target"
  find "$target_libdir" -maxdepth 1 -type f -name 'libstd-*.rlib' -print -quit 2>/dev/null | grep -q . ||
    die "Rust target standard library is not installed: $product_target"
  if [ -n "$product_linker" ]; then
    command -v "$product_linker" >/dev/null 2>&1 ||
      die "missing target Rust linker '$product_linker'; provision a musl cross-linker or set FERRIX_PIXEL9A_RUST_LINKER"
  fi
}

check_static_inputs() {
  [ -f "$root/Cargo.toml" ] || die "workspace manifest is missing: $root/Cargo.toml"
  [ -f "$root/Cargo.lock" ] || die "locked dependency file is missing: $root/Cargo.lock"
  [ -f "$root/rust-toolchain.toml" ] || die "pinned Rust toolchain file is missing: $root/rust-toolchain.toml"
  [ -n "$(toolchain_channel)" ] || die "pinned Rust toolchain file has no channel: $root/rust-toolchain.toml"
  [ -f "$profile_path" ] || die "target development profile is missing: $profile_path"
  [ -f "$colors_path" ] || die "Ferrix colors file is missing: $colors_path"
  [ -d "$apps_dir" ] || die "physical app catalog is missing: $apps_dir"
  [ -f "$apps_dir/grants.json" ] || die "physical grants policy is missing: $apps_dir/grants.json"
  [ -f "$root/crates/ferrix-networkd/Cargo.toml" ] || die 'ferrix-networkd manifest is missing'
  [ -f "$root/crates/ferrix-telephonyd/Cargo.toml" ] || die 'ferrix-telephonyd manifest is missing'

  grep -q '^schema_version = 1$' "$profile_path" || die 'target profile schema is not 1'
  grep -q '^name = "pixel-9a-native-linux"$' "$profile_path" ||
    die 'target development profile is not pixel-9a-native-linux'
  grep -q '^schema_version = 1$' "$colors_path" || die 'Ferrix colors schema is not 1'
  grep -Eq '^production[[:space:]]*=' "$root/crates/ferrix-networkd/Cargo.toml" ||
    die 'ferrix-networkd production feature is missing'
  grep -Eq '^production[[:space:]]*=' "$root/crates/ferrix-telephonyd/Cargo.toml" ||
    die 'ferrix-telephonyd production feature is missing'

  app_count=0
  for descriptor in "$apps_dir"/*.json; do
    [ -f "$descriptor" ] || continue
    app_count=$((app_count + 1))
  done
  [ "$app_count" -gt 0 ] || die "physical app catalog has no JSON descriptors: $apps_dir"
}

check_output_path() {
  [ "$product_out" != / ] || die 'product output may not be the filesystem root'
  [ "$product_out" != "$root" ] || die 'product output may not be the repository root'
  if [ -e "$product_out" ] || [ -L "$product_out" ]; then
    die "product output already exists; choose a fresh FERRIX_PIXEL9A_PRODUCT_OUT: $product_out"
  fi
}

print_resolution() {
  printf '%s\n' "target: $target_id"
  printf '%s\n' "codename: $codename"
  printf '%s\n' "hardware revision: $hardware_revision"
  printf '%s\n' "evidence: $evidence"
  printf '%s\n' 'host: native Linux x86_64'
  printf '%s\n' "rust target: $product_target (physical Pixel product contract)"
  [ -n "$product_linker" ] && printf '%s\n' "rust linker: $product_linker (override FERRIX_PIXEL9A_RUST_LINKER)"
  printf '%s\n' "product output: $product_out"
  printf '%s\n' "Cargo target directory: $cargo_target_dir"
  printf '%s\n' "profile: $profile_path"
  printf '%s\n' "app catalog: $apps_dir"
  printf '%s\n' "colors: $colors_path"
  printf '%s\n' "manifest: $manifest_path"
  printf '%s\n' "production features: $production_features"
  printf '%s\n' 'product packages: ferrix-core ferrix-appd ferrix-runtime-native ferrix-basic-screen ferrix-terminal ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files'
  printf '%s\n' 'product binaries: ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files'
}

preflight() {
  check_host
  check_target
  check_tools
  check_rust_target
  check_static_inputs
  print_resolution
  printf '%s\n' 'PASS pixel-9a-product preflight: native Linux host, target inputs, and production features verified'
}

run_cargo_builds() {
  if [ "$product_target" = aarch64-unknown-linux-musl ]; then
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$product_linker"
  fi
  release_dir="$cargo_target_dir/$product_target/release"
  printf '%s\n' "+ cargo build --release --locked --target $product_target --features $production_features"
  (
    cd "$root"
    CARGO_TARGET_DIR="$cargo_target_dir" cargo build --release --locked --target "$product_target" \
      -p ferrix-core -p ferrix-appd -p ferrix-runtime-native -p ferrix-basic-screen \
      -p ferrix-terminal -p ferrix-telephonyd -p ferrix-networkd -p ferrix-contactsd \
      -p ferrix-phone -p ferrix-messages -p ferrix-contacts -p ferrix-settings -p ferrix-files \
      --features "$production_features"
  )

  shell_rustflags=${FERRIX_PIXEL9A_PRODUCT_SHELL_RUSTFLAGS:-"-C target-feature=-crt-static"}
  printf '%s\n' "+ cargo build --release --locked --target $product_target -p ferrix-shell --no-default-features --features udev"
  (
    cd "$root"
    RUSTFLAGS="$shell_rustflags" CARGO_TARGET_DIR="$cargo_target_dir" cargo build --release --locked \
      --target "$product_target" -p ferrix-shell --no-default-features --features udev
  )

  release_dir="$cargo_target_dir/$product_target/release"
  for binary in $product_bins; do
    [ -x "$release_dir/$binary" ] || die "Cargo build did not produce executable: $release_dir/$binary"
  done
}

stage_file() {
  source=$1
  destination=$2
  cp "$source" "$destination"
}

stage_product() {
  stage=$1
  release_dir="$cargo_target_dir/$product_target/release"
  mkdir -p "$stage/apps"
  for binary in $product_bins; do
    stage_file "$release_dir/$binary" "$stage/$binary"
    chmod 0755 "$stage/$binary"
  done
  stage_file "$profile_path" "$stage/profile.toml"
  stage_file "$colors_path" "$stage/colors.toml"
  chmod 0644 "$stage/profile.toml" "$stage/colors.toml"
  for descriptor in "$apps_dir"/*.json; do
    [ -f "$descriptor" ] || continue
    stage_file "$descriptor" "$stage/apps/${descriptor##*/}"
    chmod 0644 "$stage/apps/${descriptor##*/}"
  done
  [ -f "$stage/apps/grants.json" ] || die 'staged grants policy is missing'
}

manifest_artifact() {
  relative=$1
  file="$stage/$relative"
  [ -f "$file" ] || die "manifest artifact is missing: $file"
  printf 'artifact\t%s\t%s\t%s\n' "$relative" "$(file_size "$file")" "$(hash_file "$file")" >>"$manifest_tmp"
}

write_manifest() {
  manifest_tmp="$stage/manifest.tsv.tmp.$$"
  cargo_lock_bytes=$(file_size "$root/Cargo.lock")
  cargo_lock_hash=$(hash_file "$root/Cargo.lock")
  toolchain_bytes=$(file_size "$root/rust-toolchain.toml")
  toolchain_hash=$(hash_file "$root/rust-toolchain.toml")
  {
    printf 'schema_version\t1\n'
    printf 'target_id\t%s\n' "$target_id"
    printf 'codename\t%s\n' "$codename"
    printf 'hardware_revision\t%s\n' "$hardware_revision"
    printf 'evidence\t%s\n' "$evidence"
    printf 'substrate\tpostmarketos\n'
    printf 'backend_mode\tnative-linux\n'
    printf 'network_backend\tnetworkmanager\n'
    printf 'telephony_backend\tmodemmanager\n'
    printf 'rust_target\t%s\n' "$product_target"
    printf 'rust_linker\t%s\n' "$product_linker"
    printf 'toolchain_channel\t%s\n' "$(toolchain_channel)"
    printf 'input\tcargo_lock\t%s\t%s\n' "$cargo_lock_bytes" "$cargo_lock_hash"
    printf 'input\trust_toolchain\t%s\t%s\n' "$toolchain_bytes" "$toolchain_hash"
    printf 'production_features\t%s\n' "$production_features"
    printf 'status\tbuilt\n'
    printf 'bootable\tfalse\n'
    printf 'source\tprofile\ttargets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml\n'
    printf 'source\tapp_catalog\tapps\n'
    printf 'source\tcolors\tthemes/ferrix/colors.toml\n'
  } >"$manifest_tmp"
  for binary in $product_bins; do manifest_artifact "$binary"; done
  manifest_artifact profile.toml
  manifest_artifact colors.toml
  for descriptor in "$stage/apps"/*.json; do
    [ -f "$descriptor" ] || continue
    manifest_artifact "apps/${descriptor##*/}"
  done
  mv "$manifest_tmp" "$stage/manifest.tsv"
}

build_product() {
  preflight >/dev/null
  [ "${FERRIX_PIXEL9A_PRODUCT_BUILD:-}" = "$build_confirmation" ] ||
    die "product build is opt-in; set FERRIX_PIXEL9A_PRODUCT_BUILD=$build_confirmation"
  check_output_path
  product_parent=$(dirname "$product_out")
  mkdir -p "$product_parent" "$cargo_target_dir"
  stage=''
  cleanup() {
    if [ -n "$stage" ] && [ -d "$stage" ]; then rm -rf "$stage"; fi
  }
  trap cleanup EXIT HUP INT TERM
  stage=$(mktemp -d "$product_parent/.product-build.XXXXXX")
  run_cargo_builds
  stage_product "$stage"
  write_manifest
  mv "$stage" "$product_out"
  stage=''
  if [ -n "$manifest_override" ]; then
    mkdir -p "$(dirname "$manifest_path")"
    cp "$product_out/manifest.tsv" "$manifest_path"
  fi
  printf '%s\n' "PASS pixel-9a-product build: $product_out"
  printf '%s\n' "manifest: $manifest_path"
}

usage() {
  printf '%s\n' 'usage: tools/pixel-9a/product.sh preflight|build' >&2
  printf '%s\n' "build confirmation: FERRIX_PIXEL9A_PRODUCT_BUILD=$build_confirmation" >&2
  printf '%s\n' 'target: aarch64-unknown-linux-musl (physical Pixel product contract)' >&2
  exit 2
}

case "${1:-}" in
  preflight) [ "$#" -eq 1 ] || usage; preflight ;;
  build) [ "$#" -eq 1 ] || usage; build_product ;;
  *) usage ;;
esac
