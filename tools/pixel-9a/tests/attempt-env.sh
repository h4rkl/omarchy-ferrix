#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/attempt-env.sh"
work=$(mktemp -d)
attempt=$(date -u +%Y%m%dT%H%M%SZ)
capacity="out/pixel-9a/partition-capacities-static-attempt-env-test-$attempt-$$.tsv"
attempt_dir="$root/out/pixel-9a/attempts/$attempt"
[ ! -e "$attempt_dir" ] && [ ! -e "$root/$capacity" ] || {
	echo 'attempt-env test would overlap an existing attempt or capacity file' >&2
	rm -rf "$work"
	exit 1
}
trap 'rm -rf "$work" "$attempt_dir"; rm -f "$root/$capacity"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$root/out/pixel-9a"
printf 'schema_version\t2\n' > "$root/$capacity"

cat > "$work/bin/git" <<'EOF'
#!/bin/sh
case "$*" in
	*'rev-parse --verify HEAD') printf '%s\n' "${FAKE_COMMIT:-0123456789abcdef0123456789abcdef01234567}" ;;
	*'status --porcelain --untracked-files=normal') [ "${FAKE_DIRTY:-0}" = 0 ] || printf '%s\n' ' M source.rs' ;;
	*) exit 99 ;;
esac
EOF
chmod 0755 "$work/bin/git"
export PATH="$work/bin:$PATH"
export FERRIX_PIXEL9A_REPO_ROOT="$root"

if FAKE_DIRTY=1 sh "$script" "$attempt" "$capacity" > "$work/dirty.out" 2>&1; then
	echo 'attempt-env accepted a dirty source checkout' >&2
	exit 1
fi
[ ! -e "$attempt_dir" ] || { echo 'dirty refusal created an attempt' >&2; exit 1; }
for invalid in 'tomorrow' '20990101T000000Z;touch' ; do
	if sh "$script" "$invalid" "$capacity" > "$work/invalid.out" 2>&1; then
		echo "attempt-env accepted unsafe attempt ID: $invalid" >&2
		exit 1
	fi
done
if sh "$script" "$attempt" 'out/pixel-9a/partition-capacities-static-$(touch x).tsv' > "$work/invalid.out" 2>&1; then
	echo 'attempt-env accepted shell syntax in a capacity path' >&2
	exit 1
fi

sh "$script" "$attempt" "$capacity" > "$work/prepared.out"
env_file="$attempt_dir/env.sh"
[ -f "$env_file" ] || { echo 'attempt-env did not write env.sh' >&2; exit 1; }
sh -n "$env_file"
(
	. "$env_file"
	[ "$FERRIX_PIXEL9A_ATTEMPT_COMMIT" = 0123456789abcdef0123456789abcdef01234567 ]
	[ "${#FERRIX_PIXEL9A_ATTEMPT_CAPACITY_SHA256}" -eq 64 ]
	[ "$FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST" = "out/pixel-9a/kernel-radio/tegu-wwan-$attempt/build-manifest.tsv" ]
	[ "$FERRIX_PIXEL9A_PRODUCT_DIR" = "$FERRIX_PIXEL9A_PRODUCT_OUT" ]
	[ "$FERRIX_PIXEL9A_PMAPORTS_DIR" = .cache/ferrix/pixel-9a/pmaports ]
	[ "$FERRIX_PIXEL9A_BOOT_INPUTS_OUT" = "$root/out/pixel-9a/boot-inputs/tegu-wwan-$attempt" ]
	[ "$FERRIX_PIXEL9A_BOOT_INPUTS_MANIFEST" = "$FERRIX_PIXEL9A_BOOT_INPUTS_OUT/manifest.tsv" ]
	[ "$FERRIX_PIXEL9A_STATIC_BOOT_OUT" = "out/pixel-9a/images/tegu-wwan-static-$attempt" ]
	[ "$FERRIX_PIXEL9A_STATIC_PARTITION_SIZES" = "$capacity" ]
) || { echo 'attempt-env outputs disagree across build stages' >&2; exit 1; }

cat > "$work/bin/docker" <<'EOF'
#!/bin/sh
case "$1" in
	info) printf '%s\n' linux/amd64 ;;
	run) printf '%s\n' "$*" >> "$FAKE_DOCKER_LOG" ;;
	*) exit 99 ;;
esac
EOF
chmod 0755 "$work/bin/docker"
export FAKE_DOCKER_LOG="$work/docker.log"
(
	. "$env_file"
	sh "$root/tools/pixel-9a/native-builder.sh" run product preflight >/dev/null
) || { echo 'native builder refused the prepared clean attempt' >&2; exit 1; }
grep -Fq 'tools/pixel-9a/product.sh preflight' "$FAKE_DOCKER_LOG" || {
	echo 'native builder did not dispatch the prepared attempt' >&2
	exit 1
}
if (
	. "$env_file"
	FAKE_COMMIT=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa sh "$root/tools/pixel-9a/native-builder.sh" run product preflight
) > "$work/revision.out" 2>&1; then
	echo 'native builder accepted a changed source commit' >&2
	exit 1
fi
grep -Fq 'source commit changed during this build attempt' "$work/revision.out"
printf 'changed\n' >> "$root/$capacity"
if (
	. "$env_file"
	sh "$root/tools/pixel-9a/native-builder.sh" run product preflight
) > "$work/changed.out" 2>&1; then
	echo 'native builder accepted a changed capacity handoff' >&2
	exit 1
fi
grep -Fq 'attempt capacity handoff changed' "$work/changed.out"
if sh "$script" "$attempt" "$capacity" > "$work/repeat.out" 2>&1; then
	echo 'attempt-env reused an existing attempt ID' >&2
	exit 1
fi
printf '%s\n' '{"event":"pixel-9a-attempt-env-test","status":"passed"}'
