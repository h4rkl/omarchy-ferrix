#!/bin/sh
# Build the checked-in exynos-modem and modemmanager-samsung-sit recipes with
# abuild on Alpine 3.24 aarch64 (matching the device's postmarketOS v26.06).
# /pkg is the read-only pmos/ directory; /out receives the artifacts.
set -eu
# abuild calls doas for dependency installs; root needs no elevation.
apk add --no-cache alpine-sdk >/tmp/apk.log 2>&1 || { tail -20 /tmp/apk.log; exit 1; }
apk add --no-cache doas >/dev/null 2>&1 && mkdir -p /etc/doas.d && echo "permit nopass root" >/etc/doas.d/root.conf
abuild-keygen -a -n >/tmp/keygen.log 2>&1 || { cat /tmp/keygen.log; exit 1; }
cp /root/.abuild/*.rsa.pub /etc/apk/keys/
mkdir -p /build/ferrix
cp -R /pkg/exynos-modem /pkg/modemmanager-samsung-sit /build/ferrix/
repo=/root/packages/ferrix
echo "$repo" >>/etc/apk/repositories
apk update >/dev/null 2>&1 || true

cd /build/ferrix/exynos-modem
abuild -F -r >/tmp/exynos-modem.log 2>&1 || { tail -40 /tmp/exynos-modem.log; exit 1; }
echo "exynos-modem built"

cd /build/ferrix/modemmanager-samsung-sit
abuild -F -r >/tmp/mm.log 2>&1 || { tail -60 /tmp/mm.log; exit 1; }
echo "modemmanager-samsung-sit built"
grep -E "^ *[0-9]+/[0-9]+ .*(FAIL|OK)|^Ok:|^Fail:" /tmp/mm.log | grep -E "FAIL|samsung-sit|Ok:|Fail:" || true
grep -E ">>> .*(Applying|patch)" /tmp/mm.log | head -10 || true

rm -rf /out/apk /out/files
mkdir -p /out/apk /out/files
cp "$repo"/aarch64/modemmanager-samsung-sit-*.apk "$repo"/aarch64/libmm-glib-*.apk /out/apk/
cp /root/.abuild/*.rsa.pub /out/apk/
cp /tmp/mm.log /out/abuild-modemmanager.log
cp /tmp/exynos-modem.log /out/abuild-exynos-modem.log
# The two files a hot swap replaces, extracted from the main package.
main=$(ls /out/apk/modemmanager-samsung-sit-[0-9]*.apk)
mkdir -p /tmp/x && tar -xzf "$main" -C /tmp/x 2>/dev/null
mkdir -p /out/files/usr/sbin /out/files/usr/lib/ModemManager
cp /tmp/x/usr/sbin/ModemManager /out/files/usr/sbin/
cp /tmp/x/usr/lib/ModemManager/*.so /out/files/usr/lib/ModemManager/
ls /tmp/x/usr/lib/ModemManager/ | head -80 > /out/plugin-list.txt
cd /out && find apk files -type f | LC_ALL=C sort | xargs sha256sum > SHA256SUMS
apk info -v alpine-sdk abuild gcc meson glib-dev 2>/dev/null > /out/build-toolchain.txt || true
cat /etc/alpine-release >> /out/build-toolchain.txt
echo ABUILD-OK
