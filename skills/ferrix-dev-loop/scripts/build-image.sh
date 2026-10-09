#!/bin/sh
# usage: [KERNEL=1] [ANDROID=0|1] build-image.sh
# On the builder: optional Kleaf kernel build (about 11 minutes), product
# build, dev-compose. Then fetch the image dir, verify every manifest hash,
# and print the local image dir. Run builder-sync.sh first.
. "$(dirname -- "$0")/env.sh"
need FERRIX_CAPACITY_TSV FERRIX_BASE_ROOTFS
cd "$FERRIX_ROOT" || exit 1
remote "
set -e
a=\$(date -u +%Y%m%dT%H%M%SZ)
sh tools/pixel-9a/attempt-env.sh \$a '$FERRIX_CAPACITY_TSV' >/dev/null
. tools/pixel-9a/env.sh >/dev/null 2>&1 || true
. out/pixel-9a/attempts/\$a/env.sh
if [ '${KERNEL:-0}' = 1 ]; then
  export FERRIX_PIXEL9A_KERNEL_OUT=out/pixel-9a/kernel/tegu-\$a
  FERRIX_PIXEL9A_KERNEL_BUILD=I-understand-this-runs-a-large-kleaf-build \
    tools/pixel-9a/native-builder.sh run kernel build > out/pixel-9a/attempts/\$a/kernel-build.log 2>&1 ||
    { tail -20 out/pixel-9a/attempts/\$a/kernel-build.log >&2; exit 1; }
  echo \$a > /tmp/ferrix-attempt
fi
k=\$(cat /tmp/ferrix-attempt)
FERRIX_PIXEL9A_PRODUCT_BUILD=I-understand-this-builds-the-pixel-9a-product \
  tools/pixel-9a/native-builder.sh run product build 2>&1 | grep -E '^error|^warning|PASS|FAIL' >&2
export FERRIX_PIXEL9A_KERNEL_OUT=out/pixel-9a/kernel/tegu-\$k
export FERRIX_PIXEL9A_DEV_BASE_ROOTFS='$FERRIX_BASE_ROOTFS'
export FERRIX_PIXEL9A_DEV_PRODUCT_DIR=out/pixel-9a/product/tegu-\$a
export FERRIX_PIXEL9A_DEV_MODULES_SKIP=\${FERRIX_PIXEL9A_DEV_MODULES_SKIP:-mali_kbase,mali_pixel}
export FERRIX_PIXEL9A_DEV_OUT=out/pixel-9a/images/tegu-kleaf-dev-\$a
export FERRIX_PIXEL9A_DEV_ANDROID=${ANDROID:-1}
tools/pixel-9a/native-builder.sh run dev-compose build 2>&1 | grep -E 'PASS|FAIL|rror' | tail -3 >&2
echo \$a > /tmp/ferrix-dev-attempt
" || exit 1
a=$(remote 'cat /tmp/ferrix-dev-attempt')
d=out/pixel-9a/images/tegu-kleaf-dev-$a
ok=0
for _ in 1 2 3 4 5; do
	rsync -a --partial -z "$FERRIX_REMOTE:$FERRIX_REMOTE_REPO/$d" out/pixel-9a/images/ && { ok=1; break; }
	sleep 10
done
[ $ok = 1 ] || { echo "STOP: fetch failed" >&2; exit 1; }
(cd "$d" && awk -F'\t' '$1=="artifact"{print $5"  "$3}' manifest.tsv | shasum -a 256 -c --quiet) ||
	{ echo "STOP: image hash mismatch" >&2; exit 1; }
echo "$d"
