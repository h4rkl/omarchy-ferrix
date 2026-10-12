#!/bin/sh
# Offline proof of the target-local google-kleaf-cpif adapter patches for the
# pinned exynos-modem source (issue #1, s5300-cpif-adapter.md). No phone, USB,
# ADB, Fastboot, SSH, or modem node is involved: the patched daemon is built
# and tested on the host (or the pinned Linux Rust container) and its CLI is
# exercised only through --print-config and refusals, which exit before any
# device node is opened.
#
# Static checks always run. The archive/patch/cargo stage needs the pinned
# source archive: it runs when the archive is already cached under
# out/pixel-9a/s5300-cpif-adapter/dl, or when FERRIX_S5300_ADAPTER_FETCH=1
# allows the hash-verified download (crates come from Cargo.lock, --locked).
# FERRIX_S5300_ADAPTER_FULL=1 makes a missing archive a failure instead of a
# skip. Linux hosts run cargo natively; other hosts need Docker and use the
# pinned rust image from targets/qemu-aarch64/scripts/linux-check.sh, because
# sit-proxy is Linux-only.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
self="$root/tools/pixel-9a/tests/exynos-modem-cpif-adapter.sh"
board="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pkg="$board/pmos/exynos-modem"
device="$board/pmos/device-google-tegu"
sources="$device/s5300-userspace-sources.toml"
commit=634e8eb404c3fcf959dce6a0b0d97b33ba0fc346
archive_name="exynos-modem-$commit.tar.gz"
archive_url="https://git.deusch.me/pixel-mainline/exynos-modem/archive/$commit.tar.gz"
archive_sha256=324efde8cdef9c3e108cc92d9248556cdc8c0234e77a8a66218b49b978a9445f
archive_sha512=fa081999130a9775a9140fa16901d4dcc9a2733ebd9af1eb55b03717659a952c9f60d476ccc0da635160ba5ee6aaca39c9920ab061e1272b2b3bcaa89ff4357c
patches="0001-exynos-modem-google-kleaf-cpif-backend.patch 0002-sit-proxy-google-kleaf-cpif-nodes.patch"
rust_image=rust@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f
fails=0

check() {
	name=$1
	shift
	if "$@"; then
		printf 'ok   %s\n' "$name"
	else
		printf 'FAIL %s\n' "$name"
		fails=$((fails + 1))
	fi
}

sha() {
	if command -v sha"$1"sum >/dev/null 2>&1; then
		sha"$1"sum "$2" | awk '{print $1}'
	else
		shasum -a "$1" "$2" | awk '{print $1}'
	fi
}

# --- cargo stage: runs inside the patched tree, natively or in the container.
# Tests every workspace package the recipe tests, then drives the built
# binaries' CLI without a modem node present.
cargo_stage() {
	src=$1
	cd "$src"
	host=$(rustc -vV | sed -n 's/^host: //p')
	[ -n "$host" ] || { echo 'cannot determine the Rust host target' >&2; exit 1; }
	cargo test --frozen --offline --target "$host" -p exynos-modem -p sit-proxy -p samsung-sit-ffi
	cargo build --frozen --offline --target "$host" -p exynos-modem -p sit-proxy
	bin="${CARGO_TARGET_DIR:-$src/target}/$host/debug"
	em="$bin/exynos-modem"
	sp="$bin/sit-proxy"
	stage_fails=0
	expect() {
		label=$1
		want=$2
		shift 2
		status=0
		got=$("$@" 2>&1) || status=$?
		if [ "$status" -eq 0 ] && [ "$got" = "$want" ]; then
			printf 'ok   cli: %s\n' "$label"
		else
			printf 'FAIL cli: %s (exit %s)\n%s\n' "$label" "$status" "$got"
			stage_fails=$((stage_fails + 1))
		fi
	}
	refuse() {
		label=$1
		shift
		status=0
		"$@" >/dev/null 2>&1 || status=$?
		if [ "$status" -eq 2 ]; then
			printf 'ok   cli: refuses %s\n' "$label"
		else
			printf 'FAIL cli: %s exited %s, expected 2\n' "$label" "$status"
			stage_fails=$((stage_fails + 1))
		fi
	}
	nl='
'
	expect 'exynos-modem default stays linux-google-tegu-wwan' \
		"kernel_backend=linux-google-tegu-wwan${nl}boot_dev=/dev/umts_boot0${nl}rfs_dev=/dev/umts_rfs0${nl}oem_gems_dev=/dev/umts_oem1${nl}uecap_dir=/vendor/firmware/uecapconfig${nl}crash_dump_boot=enabled" \
		"$em" --print-config
	expect 'exynos-modem google-kleaf-cpif selects oem_ipc1 and disables dump-boot' \
		"kernel_backend=google-kleaf-cpif${nl}boot_dev=/dev/umts_boot0${nl}rfs_dev=/dev/umts_rfs0${nl}oem_gems_dev=/dev/oem_ipc1${nl}uecap_dir=/vendor/firmware/uecapconfig${nl}crash_dump_boot=disabled" \
		"$em" --kernel-backend google-kleaf-cpif --print-config
	expect 'exynos-modem google-kleaf-cpif accepts its explicit nodes and none' \
		"kernel_backend=google-kleaf-cpif${nl}boot_dev=none${nl}rfs_dev=/dev/umts_rfs0${nl}oem_gems_dev=/dev/oem_ipc1${nl}uecap_dir=/vendor/firmware/uecapconfig${nl}crash_dump_boot=disabled" \
		"$em" --boot-dev none --rfs-dev /dev/umts_rfs0 --oem-gems-dev /dev/oem_ipc1 --kernel-backend google-kleaf-cpif --print-config
	refuse 'the legacy OEM node on google-kleaf-cpif' \
		"$em" --kernel-backend google-kleaf-cpif --oem-gems-dev /dev/umts_oem1 --print-config
	refuse 'a WWAN RFS node on google-kleaf-cpif' \
		"$em" --kernel-backend google-kleaf-cpif --rfs-dev /dev/wwan0rfs0 --print-config
	refuse 'an unknown exynos-modem backend' "$em" --kernel-backend cpif --print-config
	refuse 'a repeated exynos-modem backend' \
		"$em" --kernel-backend google-kleaf-cpif --kernel-backend linux-google-tegu-wwan --print-config
	# Without --print-config a refused configuration must still exit before
	# the channel threads start (they would retry a missing node forever).
	refuse 'a bad CPIF path in run mode' "$em" --kernel-backend google-kleaf-cpif --boot-dev /dev/null
	expect 'sit-proxy default stays on the WWAN SIT nodes' \
		"kernel_backend=linux-google-tegu-wwan${nl}dev.0=/dev/wwan0sit0${nl}dev.1=/dev/wwan0sit1${nl}socket_dir=/run/sit-proxy" \
		"$sp" --print-config
	expect 'sit-proxy google-kleaf-cpif selects umts_ipc0/1' \
		"kernel_backend=google-kleaf-cpif${nl}dev.0=/dev/umts_ipc0${nl}dev.1=/dev/umts_ipc1${nl}socket_dir=/run/sit-proxy" \
		"$sp" --kernel-backend google-kleaf-cpif --print-config
	refuse 'a WWAN SIT node on google-kleaf-cpif' \
		"$sp" --kernel-backend google-kleaf-cpif --dev /dev/wwan0sit0 --print-config
	refuse 'a repeated CPIF SIT node' \
		"$sp" --kernel-backend google-kleaf-cpif --dev /dev/umts_ipc0 --dev /dev/umts_ipc0 --print-config
	refuse 'an unknown sit-proxy backend' "$sp" --kernel-backend wwan --print-config
	refuse 'a bad CPIF SIT path in run mode' "$sp" --kernel-backend google-kleaf-cpif --dev /dev/null
	[ "$stage_fails" -eq 0 ]
}

if [ "${1:-}" = --cargo-stage ]; then
	cargo_stage "$2"
	exit $?
fi

# --- static checks (no network, no build).
check "static: POSIX syntax" sh -n "$self"
recipe_patches=$(sed -n '/^source="/,/"$/p' "$pkg/APKBUILD" | grep -o '[0-9][0-9][0-9][0-9]-[A-Za-z0-9.-]*\.patch' | tr '\n' ' ' | sed 's/ $//')
check "static: recipe lists exactly the ordered target-local patches" [ "$recipe_patches" = "$patches" ]
check "static: recipe keeps the pinned archive hash" grep -q "^$archive_sha512  $archive_name\$" "$pkg/APKBUILD"
check "static: source manifest keeps the pinned archive" \
	sh -c 'grep -q "^commit = \"$1\"$" "$3" && grep -q "^sha256 = \"$2\"$" "$3"' sh "$commit" "$archive_sha256" "$sources"
patch_count=$(find "$pkg" -name '*.patch' | wc -l | tr -d ' ')
check "static: no unlisted patch in the package" [ "$patch_count" -eq 2 ]
for p in $patches; do
	[ -f "$pkg/$p" ] || { check "static: $p exists" false; continue; }
	check "static: recipe sha512 pins $p" grep -q "^$(sha 512 "$pkg/$p")  $p\$" "$pkg/APKBUILD"
	check "static: source manifest sha256 pins $p" \
		grep -q "file = \"$p\", sha256 = \"$(sha 256 "$pkg/$p")\"" "$sources"
	touched=$(grep '^+++ b/' "$pkg/$p" | sed 's|^+++ b/||' | grep -v '^crates/\(exynos-modem\|sit-proxy\)/src/[a-z0-9_/]*\.rs$' || true)
	check "static: $p touches only exynos-modem/sit-proxy Rust sources" [ -z "$touched" ]
	check "static: $p adds no dependency, build script, or cargo config" \
		sh -c '! grep -E "^\+\+\+ b/.*(Cargo\.(toml|lock)|build\.rs|\.cargo/)" "$1"' sh "$pkg/$p"
	check "static: $p adds no process spawning or network code" \
		sh -c '! grep -E "^\+.*(std::process::Command|TcpStream|UdpSocket|std::net::)" "$1"' sh "$pkg/$p"
done
# The patches only make the backend selectable; nothing in the checked-in
# integration selects it, so CP boot on Kleaf stays off until a reviewed
# change (and its physical gates) enables it.
check "static: backend environment still pairs the radio-first kernel" \
	grep -q '^FERRIX_S5300_KERNEL_BACKEND=linux-google-tegu-wwan$' "$device/ferrix-backends.env"
for initd in exynos-modem.initd sit-proxy.initd; do
	check "static: $initd still refuses every backend but linux-google-tegu-wwan" \
		grep -q '\[ "${FERRIX_S5300_KERNEL_BACKEND:-}" = "linux-google-tegu-wwan" \] ||' "$device/$initd"
	check "static: $initd does not select google-kleaf-cpif" \
		sh -c '! grep -v "^[[:space:]]*#" "$1" | grep -nE "google-kleaf-cpif|--kernel-backend|oem_ipc|umts_ipc"' sh "$device/$initd"
done
check "static: exynos-modem recipe installs no service or runlevel" \
	sh -c '! grep -nE "rc-update|openrc|initd|install_if" "$1"' sh "$pkg/APKBUILD"

# --- archive, patch application, and cargo stage.
out=${FERRIX_OUT_DIR:-"$root/out"}/pixel-9a/s5300-cpif-adapter
archive="$out/dl/$archive_name"
if [ ! -f "$archive" ] && [ "${FERRIX_S5300_ADAPTER_FETCH:-0}" = 1 ]; then
	command -v curl >/dev/null 2>&1 || { echo 'curl is required to fetch the pinned archive' >&2; exit 1; }
	mkdir -p "$out/dl"
	curl -fsSL --retry 3 -o "$archive.part" "$archive_url"
	mv "$archive.part" "$archive"
fi
if [ ! -f "$archive" ]; then
	if [ "${FERRIX_S5300_ADAPTER_FULL:-0}" = 1 ]; then
		check "archive: pinned source archive present" false
	else
		printf 'skip archive/patch/cargo stage: %s is not cached (set FERRIX_S5300_ADAPTER_FETCH=1)\n' "$archive"
	fi
	[ "$fails" -eq 0 ] || { printf '%s check(s) failed\n' "$fails"; exit 1; }
	exit 0
fi

# Hashes are verified before the archive is extracted.
check "archive: sha256 matches the pin" [ "$(sha 256 "$archive")" = "$archive_sha256" ]
check "archive: sha512 matches the recipe" [ "$(sha 512 "$archive")" = "$archive_sha512" ]
[ "$fails" -eq 0 ] || { printf '%s check(s) failed; not extracting\n' "$fails"; exit 1; }

mkdir -p "$out"
work=$(mktemp -d "$out/work.XXXXXX")
trap 'rm -rf "$work"' EXIT
tar -xzf "$archive" -C "$work"
src="$work/exynos-modem"
check "archive: extracts to the recipe builddir name" [ -f "$src/Cargo.lock" ]
command -v patch >/dev/null 2>&1 || { echo 'patch is required' >&2; exit 1; }
for p in $patches; do
	# Same invocation shape as abuild's default_prepare.
	check "patch: $p applies cleanly with -p1" sh -c 'cd "$1" && patch -p1 -N -s -i "$2"' sh "$src" "$pkg/$p"
done
leftovers=$(find "$src" \( -name '*.rej' -o -name '*.orig' \) | wc -l | tr -d ' ')
check "patch: no rejects or backup files" [ "$leftovers" -eq 0 ]
check "patch: workspace lockfile is unchanged" sh -c 'tar -xzOf "$1" exynos-modem/Cargo.lock | cmp -s - "$2/Cargo.lock"' sh "$archive" "$src"
[ "$fails" -eq 0 ] || { printf '%s check(s) failed; not building\n' "$fails"; exit 1; }

cargo_home="$out/cargo-home"
mkdir -p "$cargo_home"
command -v cargo >/dev/null 2>&1 || { echo 'cargo is required for the adapter cargo stage' >&2; exit 1; }
# The recipe's own fetch: locked crate versions, all targets.
(cd "$src" && CARGO_HOME="$cargo_home" cargo fetch --locked) || { check "cargo: locked fetch" false; exit 1; }
case $(uname -s) in
	Linux)
		status=0
		CARGO_HOME="$cargo_home" CARGO_TARGET_DIR="$out/target-linux" sh "$self" --cargo-stage "$src" || status=$?
		;;
	*)
		command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1 || {
			echo 'non-Linux host: Docker is required for the Linux-only sit-proxy tests' >&2
			exit 1
		}
		status=0
		docker run --rm --network none \
			-v "$root:/repo:ro" -v "$src:/src" -v "$cargo_home:/cargo-home" -v "$out/target-container:/target" \
			-e CARGO_HOME=/cargo-home -e CARGO_TARGET_DIR=/target \
			"$rust_image" sh /repo/tools/pixel-9a/tests/exynos-modem-cpif-adapter.sh --cargo-stage /src || status=$?
		;;
esac
check "cargo: patched tests and CLI contract pass" [ "$status" -eq 0 ]

[ "$fails" -eq 0 ] || { printf '%s check(s) failed\n' "$fails"; exit 1; }
echo 'exynos-modem google-kleaf-cpif adapter: all checks passed'
