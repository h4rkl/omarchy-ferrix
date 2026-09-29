#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
env_script="$root/tools/pixel-9a/env.sh"
profile="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.env"
runtime_profile="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml"

[ -f "$profile" ]
[ -f "$runtime_profile" ]
sh -n "$profile"
sh -n "$env_script"

validate_output=$(cd "$root" && "$env_script" validate)
[ -z "$validate_output" ]

print_output=$(cd "$root" && "$env_script" print)
profile_target=$(sed -n 's/^FERRIX_TARGET=//p' "$profile")
profile_evidence=$(sed -n 's/^FERRIX_EVIDENCE=//p' "$profile")
profile_network=$(sed -n 's/^FERRIX_NETWORK_BACKEND=//p' "$profile")
profile_mock=$(sed -n 's/^FERRIX_MOCK_BACKEND=//p' "$profile")
printf '%s\n' "$print_output" | grep -Fx "target=$profile_target" >/dev/null
printf '%s\n' "$print_output" | grep -Fx "evidence=$profile_evidence" >/dev/null
printf '%s\n' "$print_output" | grep -Fx "runtime_profile=$runtime_profile" >/dev/null
printf '%s\n' "$print_output" | grep -Fx 'backend_mode=native-linux' >/dev/null
printf '%s\n' "$print_output" | grep -Fx 'substrate=postmarketos' >/dev/null
printf '%s\n' "$print_output" | grep -Fx 'rust_target=aarch64-unknown-linux-musl' >/dev/null
printf '%s\n' "$print_output" | grep -Fx 'rust_linker=aarch64-linux-musl-gcc' >/dev/null

# The build gate must include every host-side composition tool used by the
# radio-first split route. In particular, mkdtboimg.py and cpio are easy to
# miss because the product and kernel stages do not invoke them directly.
build_tools=$(sed -n "s/^FERRIX_PIXEL9A_HOST_TOOLS_BUILD='\(.*\)'$/\1/p" "$profile")
printf '%s\n' "$build_tools" | tr ' ' '\n' | grep -Fx 'mkdtboimg.py' >/dev/null
printf '%s\n' "$build_tools" | tr ' ' '\n' | grep -Fx 'cpio' >/dev/null

printf '%s\n' "$print_output" | grep -Fx "cache_dir=$root/.cache/ferrix/pixel-9a" >/dev/null
printf '%s\n' "$print_output" | grep -Fx "out_dir=$root/out/pixel-9a" >/dev/null
printf '%s\n' "$print_output" | grep -Fx "radio_kernel_cache_dir=$root/.cache/ferrix/pixel-9a/kernel-radio" >/dev/null
printf '%s\n' "$print_output" | grep -Fx "radio_kernel_out=$root/out/pixel-9a/kernel-radio/tegu-wwan" >/dev/null
printf '%s\n' "$print_output" | grep -Fx "radio_kernel_dist=$root/out/pixel-9a/kernel-radio/tegu-wwan/dist" >/dev/null
printf '%s\n' "$print_output" | grep -Fx "radio_boot_out=$root/out/pixel-9a/images/tegu-wwan" >/dev/null

# Loading the profile must replace accidental caller values for identity and
# backend selection; those values are safety-sensitive and not overrides.
loaded=$(cd "$root" && FERRIX_TARGET=wrong FERRIX_EVIDENCE=QEMU sh -c '. "$1"; printf "%s\n%s\n%s\n%s\n" "$FERRIX_TARGET" "$FERRIX_EVIDENCE" "$FERRIX_NETWORK_BACKEND" "$FERRIX_MOCK_BACKEND"' sh "$env_script")
expected=$(printf '%s\n%s\n%s\n%s\n' "$profile_target" "$profile_evidence" "$profile_network" "$profile_mock")
[ "$loaded" = "$expected" ]

# The runtime profile is hardware-shaped and must not contain QEMU/mock
# sections or synthetic network/telephony fixtures.
grep -Fx 'width = 1080' "$runtime_profile" >/dev/null
grep -Fx 'height = 2424' "$runtime_profile" >/dev/null
grep -Fx 'orientation = "portrait"' "$runtime_profile" >/dev/null
grep -Fx 'touch = true' "$runtime_profile" >/dev/null
if grep -Eq '^\[telephony\]|^\[network\]|Ferrix Mock|ferrix-lab|mock-passphrase' "$runtime_profile"; then
  printf '%s\n' 'runtime profile contains synthetic QEMU/mock state' >&2
  exit 1
fi

printf '%s\n' 'pixel-9a environment profile tests passed'
