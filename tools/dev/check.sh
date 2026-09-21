#!/bin/sh
# Read-only validation of the host tools needed by the default Rust/QEMU path.
set -u

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' "$root/rust-toolchain.toml")
target=aarch64-unknown-linux-gnu
failures=0

pass() {
  printf 'PASS %s\n' "$1"
}

fail() {
  printf 'FAIL %s\n' "$1" >&2
  failures=$((failures + 1))
}

version_line() {
  "$@" 2>/dev/null | sed -n '1p'
}

check_command() {
  label=$1
  command_name=$2
  shift 2
  if command -v "$command_name" >/dev/null 2>&1; then
    detail=$(version_line "$@")
    pass "$label${detail:+ — $detail}"
  else
    fail "$label — missing command: $command_name"
  fi
}

case "$(uname -s):$(uname -m)" in
  Darwin:arm64) pass "host — Apple Silicon macOS" ;;
  Linux:x86_64|Linux:aarch64|Linux:arm64) pass "host — $(uname -s) $(uname -m)" ;;
  *) fail "unsupported host — $(uname -s) $(uname -m)" ;;
esac

[ -n "$toolchain" ] || fail "Rust channel — rust-toolchain.toml has no channel"

check_command "git" git git --version
check_command "Python" python3 python3 --version
check_command "cpio" cpio cpio --version
check_command "SHA-256 tool" shasum shasum --version
check_command "QEMU AArch64" qemu-system-aarch64 qemu-system-aarch64 --version
check_command "QEMU image tool" qemu-img qemu-img --version
check_command "Docker CLI" docker docker --version
check_command "rustup" rustup rustup --version
check_command "just" just just --version

if command -v docker >/dev/null 2>&1; then
  if docker info >/dev/null 2>&1; then
    pass "Docker engine — reachable by the current user"
  else
    fail "Docker engine — not running or not reachable by the current user"
  fi
fi

if command -v rustup >/dev/null 2>&1 && [ -n "$toolchain" ]; then
  if rust_version=$(rustup run "$toolchain" rustc --version 2>/dev/null) && \
      cargo_version=$(rustup run "$toolchain" cargo --version 2>/dev/null); then
    pass "Rust toolchain — $toolchain"
    pass "rustc — $rust_version"
    pass "cargo — $cargo_version"
  else
    fail "Rust toolchain — $toolchain is not installed"
  fi

  if rustup component list --toolchain "$toolchain" --installed 2>/dev/null | grep -q '^rustfmt-'; then
    pass "Rust component — rustfmt"
  else
    fail "Rust component — rustfmt is not installed for $toolchain"
  fi

  if rustup component list --toolchain "$toolchain" --installed 2>/dev/null | grep -q '^clippy-'; then
    pass "Rust component — clippy"
  else
    fail "Rust component — clippy is not installed for $toolchain"
  fi

  if rustup target list --toolchain "$toolchain" --installed 2>/dev/null | grep -qx "$target"; then
    pass "Rust target — $target"
  else
    fail "Rust target — $target is not installed for $toolchain"
  fi
fi

if [ "$(uname -s)" = Linux ]; then
  check_command "pkg-config" pkg-config pkg-config --version
  if command -v pkg-config >/dev/null 2>&1; then
    for library in libdrm gbm libinput pixman-1 libseat libudev wayland-client xkbcommon; do
      if pkg-config --exists "$library"; then
        pass "Linux development library — $library"
      else
        fail "Linux development library — $library is unavailable to pkg-config"
      fi
    done
  fi
fi

if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
  pass "optional local AArch64 linker — aarch64-linux-gnu-gcc"
else
  printf 'WARN optional local AArch64 linker unavailable; pinned containers provide it\n'
fi

printf 'resolved repository: %s\n' "$root"
printf 'resolved output: %s\n' "${FERRIX_OUT_DIR:-$root/out}/qemu-aarch64"

if [ "$failures" -ne 0 ]; then
  printf '%s host requirement(s) failed\n' "$failures" >&2
  exit 1
fi

printf 'All required host tools are ready.\n'
