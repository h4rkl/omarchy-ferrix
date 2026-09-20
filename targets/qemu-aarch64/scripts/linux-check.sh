#!/bin/sh
# Runs a cargo command for the Linux guest architecture inside the same pinned
# arm64 Rust builder image that qemu-build.sh uses, with the compositor's
# native development libraries installed. This is how Linux-only code
# (Smithay backends, the application sandbox) is type-checked and unit-tested
# from a macOS host. It is a host convenience, never QEMU evidence.
#
#   just linux-check                       # cargo test --workspace
#   just linux-check clippy --workspace --all-targets -- -D warnings
#
# The container runs privileged so the sandbox tests can create Linux
# namespaces; nothing outside the mounted repository and cache is touched.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
cache=${FERRIX_CACHE_DIR:-"$root/.cache/ferrix"}
rust_image=rust@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f
rust_toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' "$root/rust-toolchain.toml")
[ -n "$rust_toolchain" ] || { echo "cannot resolve pinned Rust channel" >&2; exit 2; }
command -v docker >/dev/null || { echo "missing docker; start Docker Desktop (see just host-check)" >&2; exit 2; }
docker info >/dev/null 2>&1 || { echo "docker daemon is not running; start Docker Desktop" >&2; exit 2; }
mkdir -p "$cache/cargo-registry" "$cache/linux-check-target"

if [ "$#" -eq 0 ]; then
  set -- test --workspace
fi
printf 'linux-check (aarch64 musl, pinned rust builder): cargo %s\n' "$*"
docker run --rm --privileged --platform linux/arm64 \
  -v "$root":/src \
  -v "$cache/cargo-registry":/usr/local/cargo/registry \
  -v "$cache/linux-check-target":/build \
  -w /src -e CARGO_TARGET_DIR=/build -e RUSTFLAGS='-C target-feature=-crt-static' -e "RUSTUP_TOOLCHAIN=$rust_toolchain" \
  -e FERRIX_LINUX_CHECK=1 \
  "$rust_image" sh -ec 'apk add --no-cache gcc musl-dev pkgconf pixman-dev libinput-dev eudev-dev libseat-dev libdrm-dev libxkbcommon-dev mesa-dev >/dev/null
# The pinned image ships without clippy; add it only when asked for.
case "$1" in clippy) rustup component add clippy >/dev/null 2>&1 ;; esac
exec cargo "$@"' sh "$@"
