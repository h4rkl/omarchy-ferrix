#!/bin/sh
# usage: deploy-product.sh -- hot-swap ferrix-* binaries on a running phone, no reflash.
# Builds the product on the builder, pushes a tarball over the bring-up
# shell, checks its hash, replaces /usr/bin/ferrix-*, restarts services.
# Changes to OpenRC scripts, the rootfs or the kernel still need build-image.sh + reflash.sh.
. "$(dirname -- "$0")/env.sh"
need FERRIX_CAPACITY_TSV
cd "$FERRIX_ROOT" || exit 1
tgz=out/dev-loop/ferrix-product.tgz
remote "
set -e
a=\$(date -u +%Y%m%dT%H%M%SZ)
sh tools/pixel-9a/attempt-env.sh \$a '$FERRIX_CAPACITY_TSV' >/dev/null
. tools/pixel-9a/env.sh >/dev/null 2>&1 || true
. out/pixel-9a/attempts/\$a/env.sh
FERRIX_PIXEL9A_PRODUCT_BUILD=I-understand-this-builds-the-pixel-9a-product \
  tools/pixel-9a/native-builder.sh run product build 2>&1 | grep -E '^error|-->|PASS|FAIL' >&2
cd out/pixel-9a/product/tegu-\$a && tar -czf /tmp/ferrix-product.tgz \$(ls | grep -E '^ferrix-')
" || exit 1
scp -q "$FERRIX_REMOTE:/tmp/ferrix-product.tgz" "$tgz" || exit 1
want=$(shasum -a 256 "$tgz" | cut -c1-64)
python3 "$DEVTEST/shell-put.py" "$tgz" /tmp/ferrix-product.tgz >/dev/null
got=$(phone 'sha256sum /tmp/ferrix-product.tgz' 10 | grep -o '^[0-9a-f]\{64\}' | head -1)
[ "$want" = "$got" ] || { echo "STOP: transfer hash mismatch"; exit 1; }
phone 'export PATH=/usr/bin:/usr/sbin:/bin:/sbin
rc-service ferrix-product stop >/dev/null 2>&1
rm -rf /tmp/newbin && mkdir /tmp/newbin && cd /tmp/newbin && tar -xzf /tmp/ferrix-product.tgz
for f in ferrix-*; do cat $f > /usr/bin/$f.new && chmod 755 /usr/bin/$f.new && mv /usr/bin/$f.new /usr/bin/$f; done
cd /
rc-service ferrix-networkd restart >/dev/null 2>&1
rc-service ferrix-core restart >/dev/null 2>&1
rc-service ferrix-product start >/dev/null 2>&1
rc-status default | grep ferrix' 90
echo "deployed $want"
