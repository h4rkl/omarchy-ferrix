#!/bin/sh
# usage: boot-health.sh -- one-shot summary of a running phone: services, radios, recent errors.
. "$(dirname -- "$0")/env.sh"
phone 'export PATH=/usr/bin:/usr/sbin:/bin:/sbin
uptime
echo "--- services not started"; rc-status default | grep -v " started"
echo "--- sockets"; ls -l /run/ferrix/ 2>&1
echo "--- nmcli"; nmcli -t -f DEVICE,TYPE,STATE device 2>&1 | head
echo "--- bluetooth"; bluetoothctl show 2>&1 | grep -E "Powered|Discover" 
echo "--- kernel errors"; dmesg 2>/dev/null | grep -iE "error|fail|oops|panic" | tail -15
echo "--- ferrix logs"; tail -n 5 /var/log/ferrix/*.log 2>/dev/null' 20
