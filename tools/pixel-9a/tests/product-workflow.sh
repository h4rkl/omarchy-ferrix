#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/product.sh"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-product-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
tab=$(printf '\t')

fail() {
  printf 'product workflow test failed: %s\n' "$*" >&2
  exit 1
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

[ -x "$workflow" ] || fail 'workflow is not executable'
sh -n "$workflow" || fail 'shell syntax'

# Device access, alternate guest profiles, and test-only control channels are
# outside this target-local product boundary.
if grep -Eiq '(^|[^[:alnum:]_-])(adb|fastboot)([^[:alnum:]_-]|$)|targets/qemu-aarch64/profile\.toml|FERRIX_QEMU_PROFILE|--mock-control|ferrix-mock-control' "$workflow"; then
  fail 'workflow contains a device or non-physical runtime path'
fi
grep -q 'dev/profile.toml' "$workflow" || fail 'target development profile is not selected'
grep -q 'themes/ferrix/colors.toml' "$workflow" || fail 'physical colors source is not selected'
grep -q 'apps' "$workflow" || fail 'physical app catalog is not selected'
grep -q 'aarch64-unknown-linux-musl' "$workflow" || fail 'default Rust target is not musl aarch64'
grep -q 'aarch64-linux-musl-gcc' "$workflow" || fail 'default musl linker is not explicit'
grep -q 'ferrix-networkd/production' "$workflow" || fail 'network production feature is not enabled'
grep -q 'ferrix-telephonyd/production' "$workflow" || fail 'telephony production feature is not enabled'
if grep -Eiq 'docker|container|PRODUCT_BUILDER|product_builder' "$workflow"; then
  fail 'product workflow must not use alternate builder tooling'
fi

mkdir -p "$work/bin" "$work/out" "$work/cache" "$work/cargo-target"
mkdir -p "$work/rust-target-libdir"
touch "$work/rust-target-libdir/libstd-test.rlib"

printf '%s\n' '#!/bin/sh' \
  'case "${1:-}" in' \
  '  -s) printf "%s\n" Darwin ;;' \
  '  -m) printf "%s\n" arm64 ;;' \
  '  *) exit 2 ;;' \
  'esac' >"$work/bin/uname"
chmod 0755 "$work/bin/uname"

printf '%s\n' '#!/bin/sh' \
  'case "$1" in' \
  '  --print) printf "%s\n" "$PRODUCT_TEST_RUST_TARGET_LIBDIR" ;;' \
  '  *) exit 2 ;;' \
  'esac' >"$work/bin/rustc"
chmod 0755 "$work/bin/rustc"

printf '%s\n' '#!/bin/sh' 'exit 0' >"$work/bin/aarch64-linux-musl-gcc"
chmod 0755 "$work/bin/aarch64-linux-musl-gcc"

if PATH="$work/bin:$PATH" FERRIX_OUT_DIR="$work/out" FERRIX_PIXEL9A_CACHE_DIR="$work/cache" \
  "$workflow" build >"$work/darwin.out" 2>"$work/darwin.err"; then
  fail 'non-Linux host was accepted'
fi
grep -q 'native Linux host required' "$work/darwin.err" || fail 'non-Linux refusal reason'
[ ! -e "$work/out/pixel-9a" ] || fail 'host refusal created output'

# Make the test host look like the required native build host. The fake Cargo
# executable materializes one executable for every expected product binary and
# records each invocation, so no cross compiler or external build is needed.
printf '%s\n' '#!/bin/sh' \
  'case "${1:-}" in' \
  '  -s) printf "%s\n" Linux ;;' \
  '  -m) printf "%s\n" x86_64 ;;' \
  '  *) exit 2 ;;' \
  'esac' >"$work/bin/uname"
chmod 0755 "$work/bin/uname"

printf '%s\n' '#!/bin/sh' \
  'set -eu' \
  'printf "[linker=%s][%s]\n" "${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER:-}" "$*" >>"$PRODUCT_TEST_CARGO_LOG"' \
  'target=' \
  'for arg in "$@"; do' \
  '  if [ "$arg" = --target ]; then' \
  '    next=1' \
  '  elif [ "${next:-0}" = 1 ]; then' \
  '    target=$arg' \
  '    next=0' \
  '  fi' \
  'done' \
  '[ -n "$target" ]' \
  'release="$CARGO_TARGET_DIR/$target/release"' \
  'mkdir -p "$release"' \
  'for binary in ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files; do' \
  '  printf "#!/bin/sh\n" >"$release/$binary"' \
  '  chmod 0755 "$release/$binary"' \
  'done' >"$work/bin/cargo"
chmod 0755 "$work/bin/cargo"

if PATH="$work/bin:$PATH" PRODUCT_TEST_RUST_TARGET_LIBDIR="$work/rust-target-libdir" \
  FERRIX_OUT_DIR="$work/out" FERRIX_PIXEL9A_CACHE_DIR="$work/cache" \
  FERRIX_PIXEL9A_CARGO_TARGET_DIR="$work/cargo-target" \
  "$workflow" build >"$work/confirm.out" 2>"$work/confirm.err"; then
  fail 'build accepted missing explicit confirmation'
fi
grep -q 'product build is opt-in' "$work/confirm.err" || fail 'confirmation refusal reason'
[ ! -e "$work/out/pixel-9a/product" ] || fail 'confirmation refusal created product output'
[ ! -e "$work/cargo.log" ] || fail 'confirmation refusal invoked Cargo'

# The target linker must be present before the opt-in gate can reach Cargo.
mv "$work/bin/aarch64-linux-musl-gcc" "$work/bin/aarch64-linux-musl-gcc.saved"
if PATH="$work/bin:$PATH" PRODUCT_TEST_RUST_TARGET_LIBDIR="$work/rust-target-libdir" \
  FERRIX_OUT_DIR="$work/out" FERRIX_PIXEL9A_CACHE_DIR="$work/cache" \
  FERRIX_PIXEL9A_CARGO_TARGET_DIR="$work/cargo-target" \
  "$workflow" build >"$work/linker.out" 2>"$work/linker.err"; then
  fail 'build accepted a missing target linker'
fi
grep -q 'missing target Rust linker' "$work/linker.err" || fail 'linker refusal reason'
mv "$work/bin/aarch64-linux-musl-gcc.saved" "$work/bin/aarch64-linux-musl-gcc"

if PATH="$work/bin:$PATH" PRODUCT_TEST_RUST_TARGET_LIBDIR="$work/rust-target-libdir" \
  FERRIX_PIXEL9A_PRODUCT_TARGET=aarch64-unknown-linux-gnu \
  FERRIX_OUT_DIR="$work/out" FERRIX_PIXEL9A_CACHE_DIR="$work/cache" \
  FERRIX_PIXEL9A_CARGO_TARGET_DIR="$work/cargo-target" \
  "$workflow" build >"$work/target.out" 2>"$work/target.err"; then
  fail 'build accepted a non-musl physical target'
fi
grep -q 'physical Pixel product requires' "$work/target.err" || fail 'physical target refusal reason'

cargo_log="$work/cargo.log"
confirmation=I-understand-this-builds-the-pixel-9a-product
PATH="$work/bin:$PATH" \
  PRODUCT_TEST_RUST_TARGET_LIBDIR="$work/rust-target-libdir" \
  PRODUCT_TEST_CARGO_LOG="$cargo_log" \
  FERRIX_OUT_DIR="$work/out" \
  FERRIX_PIXEL9A_CACHE_DIR="$work/cache" \
  FERRIX_PIXEL9A_CARGO_TARGET_DIR="$work/cargo-target" \
  FERRIX_PIXEL9A_PRODUCT_BUILD="$confirmation" \
  "$workflow" build >"$work/build.out"

product="$work/out/pixel-9a/product/tegu"
[ -d "$product" ] || fail 'product output directory is missing'
[ -f "$product/manifest.tsv" ] || fail 'product manifest is missing'

product_bins='ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files'
for binary in $product_bins; do
  [ -x "$product/$binary" ] || fail "staged binary is not executable: $binary"
done

cmp -s "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" "$product/profile.toml" ||
  fail 'target development profile was not staged byte-for-byte'
cmp -s "$root/themes/ferrix/colors.toml" "$product/colors.toml" ||
  fail 'Ferrix colors were not staged byte-for-byte'
for descriptor in "$root/apps"/*.json; do
  name=${descriptor##*/}
  cmp -s "$descriptor" "$product/apps/$name" || fail "app catalog entry was not staged: $name"
done
[ -f "$product/apps/grants.json" ] || fail 'grants policy was not staged'
[ ! -e "$product/targets/qemu-aarch64/profile.toml" ] || fail 'alternate guest profile was staged'

grep -F "$product" "$work/build.out" >/dev/null || fail 'build output path was not reported'
grep -F 'manifest:' "$work/build.out" >/dev/null || fail 'manifest path was not reported'
grep -F -- 'ferrix-networkd/production' "$cargo_log" >/dev/null || fail 'network production feature was not passed to Cargo'
grep -F -- 'ferrix-telephonyd/production' "$cargo_log" >/dev/null || fail 'telephony production feature was not passed to Cargo'
grep -F -- '[linker=aarch64-linux-musl-gcc]' "$cargo_log" >/dev/null || fail 'musl linker was not passed to Cargo'
grep -F -- '--target aarch64-unknown-linux-musl' "$cargo_log" >/dev/null || fail 'Cargo target was not aarch64 musl'
grep -F -- '--no-default-features --features udev' "$cargo_log" >/dev/null || fail 'shell build did not select the target software backend'

awk -F "$tab" '$1 == "target_id" && $2 == "google-pixel-9a-tegu-gxq96-mp1-0" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv"
awk -F "$tab" '$1 == "evidence" && $2 == "physical:google-pixel-9a-tegu-gxq96-mp1-0" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv"
awk -F "$tab" '$1 == "rust_target" && $2 == "aarch64-unknown-linux-musl" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv"
awk -F "$tab" '$1 == "rust_linker" && $2 == "aarch64-linux-musl-gcc" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv"
awk -F "$tab" '$1 == "backend_mode" && $2 == "native-linux" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv"
awk -F "$tab" '$1 == "network_backend" && $2 == "networkmanager" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv"
awk -F "$tab" '$1 == "telephony_backend" && $2 == "modemmanager" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv"
awk -F "$tab" '$1 == "toolchain_channel" && $2 == "1.98.1" { found=1 } END { exit(found ? 0 : 1) }' "$product/manifest.tsv" ||
  fail 'product manifest does not record the pinned Rust channel'
cargo_lock_bytes=$(wc -c <"$root/Cargo.lock" | tr -d '[:space:]')
cargo_lock_hash=$(hash_file "$root/Cargo.lock")
awk -F "$tab" -v bytes="$cargo_lock_bytes" -v hash="$cargo_lock_hash" \
  '$1 == "input" && $2 == "cargo_lock" && $3 == bytes && $4 == hash { found=1 } END { exit(found ? 0 : 1) }' \
  "$product/manifest.tsv" || fail 'product manifest does not hash Cargo.lock'
toolchain_bytes=$(wc -c <"$root/rust-toolchain.toml" | tr -d '[:space:]')
toolchain_hash=$(hash_file "$root/rust-toolchain.toml")
awk -F "$tab" -v bytes="$toolchain_bytes" -v hash="$toolchain_hash" \
  '$1 == "input" && $2 == "rust_toolchain" && $3 == bytes && $4 == hash { found=1 } END { exit(found ? 0 : 1) }' \
  "$product/manifest.tsv" || fail 'product manifest does not hash rust-toolchain.toml'
for relative in $product_bins profile.toml colors.toml; do
  file="$product/$relative"
  bytes=$(wc -c <"$file" | tr -d '[:space:]')
  hash=$(hash_file "$file")
  awk -F "$tab" -v name="$relative" -v bytes="$bytes" -v hash="$hash" \
    '$1 == "artifact" && $2 == name && $3 == bytes && $4 == hash { found=1 } END { exit(found ? 0 : 1) }' \
    "$product/manifest.tsv" || fail "manifest hash row is missing or incorrect: $relative"
done
for descriptor in "$product/apps"/*.json; do
  name="apps/${descriptor##*/}"
  bytes=$(wc -c <"$descriptor" | tr -d '[:space:]')
  hash=$(hash_file "$descriptor")
  awk -F "$tab" -v name="$name" -v bytes="$bytes" -v hash="$hash" \
    '$1 == "artifact" && $2 == name && $3 == bytes && $4 == hash { found=1 } END { exit(found ? 0 : 1) }' \
    "$product/manifest.tsv" || fail "manifest hash row is missing or incorrect: $name"
done

printf '%s\n' 'pixel-9a product workflow tests passed'
