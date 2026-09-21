#!/bin/sh
# Install the host tools needed by the default Rust/QEMU development path.
# This script is explicitly opt-in; `tools/dev/check.sh` remains read-only.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' "$root/rust-toolchain.toml")
target=aarch64-unknown-linux-gnu
just_version=1.58.0
temporary_dir=
docker_access_pending=0

usage() {
  printf '%s\n' \
    'Usage: sh tools/dev/install.sh [--check]' \
    '' \
    'Without arguments, installs the default developer environment.' \
    '--check performs the same read-only validation as tools/dev/check.sh.'
}

case "${1:-}" in
  '') ;;
  --check) exec sh "$root/tools/dev/check.sh" ;;
  -h|--help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { usage >&2; exit 2; }
[ -n "$toolchain" ] || { echo "cannot resolve the pinned Rust channel" >&2; exit 2; }

cleanup() {
  if [ -n "$temporary_dir" ] && [ -d "$temporary_dir" ]; then
    rm -rf "$temporary_dir"
  fi
}
trap cleanup EXIT HUP INT TERM

make_temporary_dir() {
  if [ -z "$temporary_dir" ]; then
    temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-dev-install.XXXXXX")
  fi
}

as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    echo "root privileges are required, but sudo is not installed" >&2
    exit 2
  fi
}

install_macos() {
  [ "$(uname -m)" = arm64 ] || {
    echo "unsupported macOS architecture: $(uname -m); Apple Silicon is required" >&2
    exit 2
  }

  if ! xcode-select -p >/dev/null 2>&1; then
    xcode-select --install >/dev/null 2>&1 || true
    echo "Xcode Command Line Tools installation was requested." >&2
    echo "Complete the macOS dialog, then run this installer again." >&2
    exit 2
  fi

  if ! command -v brew >/dev/null 2>&1; then
    command -v curl >/dev/null 2>&1 || { echo "curl is required to install Homebrew" >&2; exit 2; }
    make_temporary_dir
    brew_installer="$temporary_dir/homebrew-install.sh"
    curl --proto '=https' --tlsv1.2 -fsSL \
      https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh \
      -o "$brew_installer"
    NONINTERACTIVE=1 /bin/bash "$brew_installer"
    if [ -x /opt/homebrew/bin/brew ]; then
      PATH="/opt/homebrew/bin:$PATH"
      export PATH
    fi
  fi

  brew install qemu just cpio python
  if ! docker info >/dev/null 2>&1; then
    if [ ! -d /Applications/Docker.app ] && [ ! -d "$HOME/Applications/Docker.app" ]; then
      brew install --cask docker
    fi
    open -gja Docker >/dev/null 2>&1 || true
    attempts=30
    while [ "$attempts" -gt 0 ]; do
      docker info >/dev/null 2>&1 && break
      sleep 1
      attempts=$((attempts - 1))
    done
  fi
}

install_linux() {
  [ -r /etc/os-release ] || { echo "Linux host has no /etc/os-release" >&2; exit 2; }
  # shellcheck disable=SC1091
  . /etc/os-release
  case "${ID:-}" in
    debian|ubuntu) ;;
    *)
      echo "unsupported Linux distribution: ${ID:-unknown}; use Debian or Ubuntu" >&2
      exit 2
      ;;
  esac
  case "$(uname -m)" in
    x86_64|aarch64|arm64) ;;
    *) echo "unsupported Linux architecture: $(uname -m)" >&2; exit 2 ;;
  esac

  as_root env DEBIAN_FRONTEND=noninteractive apt-get update
  as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y \
    build-essential ca-certificates cpio curl file git gzip \
    libdigest-sha-perl pkg-config python3 qemu-system-arm qemu-user-static \
    qemu-utils xz-utils \
    libdrm-dev libgbm-dev libinput-dev libpixman-1-dev libseat-dev \
    libudev-dev libwayland-dev libxkbcommon-dev

  if ! command -v docker >/dev/null 2>&1; then
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y docker.io
  fi

  if command -v systemctl >/dev/null 2>&1; then
    as_root systemctl enable --now docker >/dev/null 2>&1 || true
  elif command -v service >/dev/null 2>&1; then
    as_root service docker start >/dev/null 2>&1 || true
  fi

  if [ "$(id -u)" -ne 0 ] && ! docker info >/dev/null 2>&1; then
    if as_root docker info >/dev/null 2>&1; then
      as_root usermod -aG docker "$(id -un)"
      docker_access_pending=1
    fi
  fi
}

case "$(uname -s)" in
  Darwin) install_macos ;;
  Linux) install_linux ;;
  *) echo "unsupported operating system: $(uname -s)" >&2; exit 2 ;;
esac

if ! command -v rustup >/dev/null 2>&1; then
  command -v curl >/dev/null 2>&1 || { echo "curl is required to install rustup" >&2; exit 2; }
  make_temporary_dir
  rustup_installer="$temporary_dir/rustup-init.sh"
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$rustup_installer"
  sh "$rustup_installer" -y --profile minimal --default-toolchain none --no-modify-path
  cargo_bin=${CARGO_HOME:-"$HOME/.cargo"}/bin
  PATH="$cargo_bin:$PATH"
  export PATH
fi

rustup toolchain install "$toolchain" --profile minimal \
  --component rustfmt --component clippy --target "$target"

if ! command -v just >/dev/null 2>&1; then
  cargo +"$toolchain" install just --locked --version "$just_version"
fi

if [ "$docker_access_pending" -eq 1 ]; then
  printf '%s\n' \
    '' \
    "Docker was installed and $(id -un) was added to the docker group." \
    'Start a new login shell, then run:' \
    '  sh tools/dev/install.sh --check' >&2
  exit 2
fi

if ! docker info >/dev/null 2>&1; then
  case "$(uname -s)" in
    Darwin)
      printf '%s\n' \
        '' \
        'Docker Desktop is installed but is not ready.' \
        'Open Docker, complete its first-run license/configuration, then run:' \
        '  sh tools/dev/install.sh --check' >&2
      ;;
    *)
      printf '%s\n' \
        '' \
        'Docker is installed but its engine is not reachable.' \
        'Start the Docker service, then run:' \
        '  sh tools/dev/install.sh --check' >&2
      ;;
  esac
  exit 2
fi

sh "$root/tools/dev/check.sh"
