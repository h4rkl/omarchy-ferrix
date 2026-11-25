#!/bin/sh
# Emulated S3 run of the built ModemManager apks (mock, not device evidence).
# A fake sysfs tree and udev database present umts_ipc0/1 (cpif) and rmnet0
# (net) with the exact ID_MM_* properties observed on the device; libudev
# reads them without udevd. /run/sit-proxy/0 and /1 are silent listeners:
# they accept connections and never answer, so no SIT protocol is emulated.
set -eu
apk update >/dev/null 2>&1
apk add --no-cache --allow-untrusted /apk/modemmanager-samsung-sit-[0-9]*.apk \
	/apk/libmm-glib-[0-9]*.apk dbus python3 iproute2 >/tmp/apk.log 2>&1 || { tail -20 /tmp/apk.log; exit 1; }

ip link add rmnet0 type dummy
ifindex=$(cat /sys/class/net/rmnet0/ifindex)

# Fake sysfs (overmounts the real one in this privileged container only).
mount -t tmpfs sysfs-fake /sys
mkdir -p /sys/class/cpif /sys/class/net /sys/devices/virtual/cpif /sys/devices/virtual/net /sys/dev/char
minor=0
for n in umts_ipc0 umts_ipc1; do
	d=/sys/devices/virtual/cpif/$n
	mkdir -p "$d"
	printf 'MAJOR=227\nMINOR=%d\nDEVNAME=%s\n' "$minor" "$n" >"$d/uevent"
	printf '227:%d\n' "$minor" >"$d/dev"
	ln -s ../../../../class/cpif "$d/subsystem"
	ln -s "../../devices/virtual/cpif/$n" "/sys/class/cpif/$n"
	ln -s "../../devices/virtual/cpif/$n" "/sys/dev/char/227:$minor"
	minor=$((minor + 1))
done
d=/sys/devices/virtual/net/rmnet0
mkdir -p "$d"
printf 'INTERFACE=rmnet0\nIFINDEX=%s\n' "$ifindex" >"$d/uevent"
echo "$ifindex" >"$d/ifindex"
echo 1 >"$d/type"
ln -s ../../../../class/net "$d/subsystem"
ln -s ../../devices/virtual/net/rmnet0 /sys/class/net/rmnet0

# udev database: the properties udevadm info showed on the device.
mkdir -p /run/udev/data
sit='E:ID_MM_CANDIDATE=1
E:ID_MM_DEVICE_PROCESS=1
E:ID_MM_PHYSDEV_UID=google-tegu-s5300-cpif
E:ID_MM_KERNEL_DRIVER=cp_interface
E:ID_MM_PORT_TYPE_SIT=1
E:ID_MM_SIT_PROXY=1
E:ID_MM_SIT_PROXY_REQUIRED=1
E:ID_MM_SIT_CARRIER_CONFIG_DIRECTORY=/vendor/firmware/carrierconfig'
for m in 0 1; do printf 'I:1\n%s\n' "$sit" >"/run/udev/data/c227:$m"; done
printf 'I:1\nE:ID_MM_CANDIDATE=1\nE:ID_MM_DEVICE_PROCESS=1\nE:ID_MM_PHYSDEV_UID=google-tegu-s5300-cpif\nE:ID_MM_KERNEL_DRIVER=cp_interface\n' \
	>"/run/udev/data/n$ifindex"

# Silent stand-ins for the sit-proxy sockets.
mkdir -p /run/sit-proxy
python3 -c '
import socket, sys, time
socks = []
for p in sys.argv[1:]:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(p); s.listen(8); s.setblocking(False); socks.append(s)
held = []
while True:
    for s in socks:
        try:
            c, _ = s.accept(); held.append(c)
            print("sim: client connected to", s.getsockname(), flush=True)
        except BlockingIOError:
            pass
    time.sleep(0.05)
' /run/sit-proxy/0 /run/sit-proxy/1 >/tmp/proxy.log 2>&1 &

mkdir -p /run/dbus
dbus-daemon --system --fork
ModemManager --debug >/out/mm-sim.log 2>&1 &
mm=$!
sleep "${SIM_SECONDS:-150}"
mmcli -L >/out/mmcli-L.txt 2>&1 || true
mmcli -m 0 >/out/mmcli-m0.txt 2>&1 || true
cp /tmp/proxy.log /out/sim-proxy.log
kill "$mm" 2>/dev/null || true
sleep 2
echo SIM-DONE
