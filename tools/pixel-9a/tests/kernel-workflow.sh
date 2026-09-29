#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/kernel.sh"
config="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-config.tegu"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-kernel-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

fail() {
  printf 'kernel workflow test failed: %s\n' "$*" >&2
  exit 1
}

[ -x "$workflow" ] || fail "workflow is not executable"
[ -f "$config" ] || fail "checked-in tegu kernel config fragment is missing"
for symbol in CONFIG_EXYNOS_MODEM_IF=m CONFIG_SEC_MODEM_S5100=m \
  CONFIG_SHM_IPC=m CONFIG_CP_PKTPROC=m CONFIG_LINK_DEVICE_PCIE_IOMMU=y \
  CONFIG_CP_THERMAL=m; do
  grep -qx "$symbol" "$config" || fail "tegu config fragment lacks $symbol"
done
grep -q '^kernel_config_fragment = "kernel-config.tegu"$' \
  "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/manifest.toml" ||
  fail "target manifest does not name tegu config fragment"
grep -q 'wifi-bcm4383).*private/google-modules/wlan/bcm4383' "$workflow" ||
  fail "BCM4383 checkout path does not match the pinned kernel manifest"
grep -q -- '--untracked-files=all' "$workflow" ||
  fail "kernel provenance check does not reject untracked source files"
sh -n "$workflow" || fail "shell syntax"

# The source/build workflow must remain completely separate from every
# physical-device executable, including strings that could become commands.
if grep -Eiq '(^|[^[:alnum:]_-])(adb|fastboot)([^[:alnum:]_-]|$)' "$workflow"; then
  fail "device-facing tool name appears in kernel workflow"
fi
if grep -Eq '(flash|erase|format|reboot|sideload|unlock)[[:space:]]' "$workflow"; then
  fail "device mutation verb appears as a command"
fi

mkdir -p "$work/bin" "$work/cache" "$work/out"
cat >"$work/bin/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) printf '%s\n' Darwin ;;
  -m) printf '%s\n' arm64 ;;
  *) exit 2 ;;
esac
EOF
cat >"$work/bin/repo" <<EOF
#!/bin/sh
printf '%s\n' invoked >>"$work/repo-invoked"
exit 99
EOF
chmod +x "$work/bin/uname" "$work/bin/repo"

if PATH="$work/bin:$PATH" FERRIX_CACHE_DIR="$work/cache" FERRIX_OUT_DIR="$work/out" \
  "$workflow" preflight >"$work/macos.out" 2>"$work/macos.err"; then
  fail "macOS preflight unexpectedly passed"
fi
grep -q 'require a native Linux host' "$work/macos.err" || fail "macOS refusal reason"
[ ! -e "$work/repo-invoked" ] || fail "macOS refusal invoked repo"
[ -z "$(find "$work/cache" "$work/out" -mindepth 1 -print -quit)" ] ||
  fail "preflight wrote cache or output"

# A verify request without a sync must fail before running any external build.
if FERRIX_CACHE_DIR="$work/cache" FERRIX_OUT_DIR="$work/out" \
  "$workflow" verify >"$work/verify.out" 2>"$work/verify.err"; then
  fail "verify without source unexpectedly passed"
fi
grep -q 'kernel source is not initialized' "$work/verify.err" || fail "missing-source refusal"

grep -q 'repo init -u https://android.googlesource.com/kernel/manifest' "$work/macos.out" ||
  fail "resolved repo command was not printed"
grep -q 'kernel_package=@//aosp' "$work/macos.out" ||
  fail "local GKI Kleaf build command was not printed"

printf '%s\n' 'pixel-9a kernel workflow tests passed'
