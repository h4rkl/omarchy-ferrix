#!/bin/sh
# Offline proof for the Pixel 9a radios workflow. `ssh` is replaced by a stub
# that logs every remote command and answers from synthetic fixture files, so
# this test never contacts a device. It checks the static allow-list (one ssh
# call inside remote(), no Android tooling), exact-target verification, the
# QEMU refusal, evidence statuses, confirmation gating, mutation order,
# PSK/SSID handling, and the redaction filter.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
radios="$root/tools/pixel-9a/radios.sh"
fixtures="$root/tools/pixel-9a/tests/fixtures/radios"
target_id=google-pixel-9a-tegu-gxq96-mp1-0
cpif_sentence='S5300 CPIF exposes no ModemManager modem; CPIF backend not implemented'
cpif_evidence="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-source-evidence.md"
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

[ -x "$radios" ] || { echo 'radios workflow is not executable' >&2; exit 1; }
check "static: POSIX syntax" sh -n "$radios"

# Every executable `ssh` must be the single line inside remote().
ssh_lines=$(grep -nE '^[[:space:]]*[^#]*(^|[^A-Za-z0-9_./-])ssh([[:space:]]|$)' "$radios" \
  | grep -vE '^[0-9]+:[[:space:]]*#' \
  | grep -vE '^[0-9]+:[[:space:]]*need ssh$' || true)
ssh_count=$(printf '%s\n' "$ssh_lines" | awk 'NF { count++ } END { print count + 0 }')
check "static: exactly one ssh invocation" [ "$ssh_count" -eq 1 ]
check "static: the ssh invocation is the remote() helper" \
  sh -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+:[[:space:]]+ssh -o BatchMode=yes -o ConnectTimeout=10 "' sh "$ssh_lines"
check "static: ssh line lives inside remote()" \
  sh -c 'awk "/^remote\\(\\) \\{/{inside=1} inside && /^}/{inside=0} inside && /^[[:space:]]+ssh /{found=1} END{exit !found}" "$1"' sh "$radios"
offending=$(grep -nwE 'adb|fastboot|flash|erase|format|reboot|sideload|unlock|dd|mkfs' "$radios" || true)
check "static: no Android tooling or storage/boot verbs anywhere" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: no partial/mocked status vocabulary" \
  sh -c '! grep -nE "status=(partial|mocked)|_status=partial|_status=mocked" "$1"' sh "$radios"

# The channel metadata is source-derived and must remain a complete catalog;
# this check never opens a device node or contacts the network.
channels="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/s5300-channels.toml"
check "static: complete S5300 DTS I/O-device catalog" \
  sh -c 'test "$(grep -c "^\[\[io_device\]\]$" "$1")" -eq 17' sh "$channels"
for io_name in umts_ipc umts_rfs0 umts_router umts_dm0 umts_loopback rmnet umts_dummy multipdp umts_boot0 umts_rcs0 umts_rcs1 umts_wfc0 umts_wfc1 oem_ipc oem_test gnss_boot gnss_dump; do
  check "static: S5300 catalog contains $io_name" grep -q "^name = \"$io_name\"$" "$channels"
done
check "static: FMT channels expand to two names" grep -q '^runtime_names = \["umts_ipc0", "umts_ipc1"\]$' "$channels"
check "static: raw-IP channels expand to thirty names" grep -q '^channel_count = 30$' "$channels"
check "static: OEM channels expand to eight names" grep -q '^runtime_names = \["oem_ipc0"' "$channels"
check "static: SIT constants source is pinned" grep -q '^source_constants_revision = "204078758737776959ecc85c527a00fd2cabb62f"$' "$channels"
check "static: FMT0 uses SIT channel 245" sh -c 'grep -A4 "^device_node = \"/dev/umts_ipc0\"$" "$1" | grep -q "^channel_value = 245$"' sh "$channels"
check "static: FMT0 symbolic constant is exact" sh -c 'grep -A3 "^device_node = \"/dev/umts_ipc0\"$" "$1" | grep -q "^channel_id = \"EXYNOS_CH_ID_FMT_0\"$"' sh "$channels"
check "static: FMT1 uses SIT channel 246" sh -c 'grep -A4 "^device_node = \"/dev/umts_ipc1\"$" "$1" | grep -q "^channel_value = 246$"' sh "$channels"
check "static: FMT1 symbolic constant is exact" sh -c 'grep -A3 "^device_node = \"/dev/umts_ipc1\"$" "$1" | grep -q "^channel_id = \"EXYNOS_CH_ID_FMT_1\"$"' sh "$channels"
check "static: rmnet range is SIT 181 through 210" sh -c 'grep -A4 "^name = \"rmnet\"$" "$1" | grep -q "^channel_value = 181$" && grep -A5 "^name = \"rmnet\"$" "$1" | grep -q "^channel_end = 210$"' sh "$channels"
check "static: OEM range is SIT 129 through 136" sh -c 'grep -A4 "^name = \"oem_ipc\"$" "$1" | grep -q "^channel_value = 129$" && grep -A5 "^name = \"oem_ipc\"$" "$1" | grep -q "^channel_end = 136$"' sh "$channels"
check "static: CPIF source records ModemManager control-port boundary" \
  grep -q 'data-only network interface is explicitly not enough' "$cpif_evidence"
check "static: CPIF source records mutable boot ioctls" \
  sh -c 'grep -q "IOCTL_START_CP_BOOTLOADER" "$1" && grep -q "IOCTL_LOAD_CP_IMAGE" "$1"' sh "$cpif_evidence"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-radios-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
calls="$work/calls.log"
: > "$calls"

cat > "$work/bin/ssh" <<'STUB'
#!/bin/sh
# Stub: log the remote command (the last argument) and answer from fixtures.
[ "$1" = -o ] && [ "$2" = BatchMode=yes ] || { echo "stub: ssh without BatchMode" >&2; exit 98; }
for arg; do cmd=$arg; done
printf '%s\n' "$cmd" >> "$STUB_CALLS"
[ "${STUB_UNREACHABLE:-0}" = 1 ] && { echo "ssh: connect to host 172.16.42.1 port 22: No route to host" >&2; exit 255; }
fx=$STUB_FIXTURES
case "$cmd" in
  *'/proc/device-tree/compatible'*)
    identity_count_file="$STUB_CALLS.identity"
    identity_count=0
    [ -f "$identity_count_file" ] && identity_count=$(cat "$identity_count_file")
    identity_count=$((identity_count + 1))
    printf '%s\n' "$identity_count" > "$identity_count_file"
    if [ "$identity_count" -eq 1 ]; then
      identity_name=${STUB_IDENTITY:-tegu}
    else
      identity_name=${STUB_IDENTITY_RECHECK:-${STUB_IDENTITY:-tegu}}
    fi
    cat "$fx/identity-$identity_name.txt"
    ;;
  *'rfkill unblock'*|*'nmcli radio wifi on'*|*'bluetoothctl power on'*|*'mmcli -m any --enable'*)
    if [ "${STUB_MUTATION_FAIL:-}" = "$cmd" ]; then
      printf 'synthetic mutation failure\n' >&2
      exit 42
    fi
    printf 'mutated\n' >> "$STUB_CALLS.mutations"; echo ok ;;
  *'nmcli --ask device wifi connect'*)
    IFS= read -r psk || psk=""
    printf 'psk-bytes=%s\n' "${#psk}"
    echo "Device 'wlan0' successfully activated with 'synthetic-uuid'." ;;
  *'rfkill list'*) cat "$fx/rfkill.txt" ;;
  *'nmcli -t general status'*) cat "$fx/nmcli-general.txt" ;;
  *'nmcli -t -f DEVICE,TYPE,STATE device status'*) cat "$fx/nmcli-devices.txt" ;;
  *'nmcli -t radio all'*) cat "$fx/nmcli-radio.txt" ;;
  *'nmcli -t -f GENERAL.DEVICE,'*) cat "$fx/nmcli-wifi-device.txt" ;;
  *'nmcli -t -f GENERAL.STATE,IP4.ADDRESS device show'*) cat "$fx/nmcli-wifi-state.txt" ;;
  *'bluetoothctl show'*) cat "$fx/bluetoothctl-show.txt" ;;
  *'mmcli -L'*) cat "$fx/mmcli-list-none.txt" ;;
  *'mmcli -m any'*) echo "stub: modem detail requested without a modem" >&2; exit 97 ;;
  *'lsmod'*) cat "$fx/lsmod.txt" ;;
  *'dmesg'*) cat "$fx/dmesg.txt" ;;
  *'firmware-manifest.tsv'*) cat "$fx/firmware.txt" ;;
  *'rc-service'*) cat "$fx/services.txt" ;;
  *'socket=/run/ferrix/'*) cat "$fx/sockets.txt" ;;
  *'ferrix-networkd.log'*) cat "$fx/logs.txt" ;;
  *'UNIX-CONNECT:/run/ferrix/core.sock'*) cat "$fx/core-probe.txt" ;;
  *'UNIX-CONNECT:/run/ferrix/networkd.sock'*) cat "$fx/networkd-probe.txt" ;;
  *'UNIX-CONNECT:/run/ferrix/telephonyd.sock'*) cat "$fx/telephonyd-probe.txt" ;;
  *'probe=read-only'*) cat "$fx/associated-radios.txt" ;;
  *'cpif_platform=absent'*) cat "$fx/cpif-surface.txt" ;;
  *) echo "stub: unexpected remote command: $cmd" >&2; exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/ssh"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_FIXTURES="$fixtures"
unset FERRIX_PIXEL9A_SSH_HOST FERRIX_PIXEL9A_SSH_USER FERRIX_PIXEL9A_SSH_PORT FERRIX_PIXEL9A_SSH_OPTS 2>/dev/null || true

run_radios() {
  case_name=$1
  shift
  mkdir -p "$work/$case_name"
  export FERRIX_OUT_DIR="$work/$case_name"
  : > "$calls"
  rm -f "$calls.mutations"
  rm -f "$calls.identity"
  status=0
  "$radios" "$@" > "$work/$case_name/stdout" 2> "$work/$case_name/stderr" || status=$?
}

# --- preflight ---
export STUB_IDENTITY=tegu
run_radios preflight preflight
check "preflight: verified exact target succeeds" [ "$status" -eq 0 ]
check "preflight: prints guest_identity=verified" grep -q '^guest_identity=verified$' "$work/preflight/stdout"
check "preflight: prints device-tree compatible" grep -q '^dt_compatible=.*google,ZUMA PRO TEGU' "$work/preflight/stdout"
check "preflight: prints kernel and os fields" sh -c 'grep -q "^kernel=" "$1" && grep -q "^os_release=" "$1" && grep -q "^hostname=" "$1"' sh "$work/preflight/stdout"
check "preflight: ssh target uses pmOS defaults" grep -q '^ssh_target=user@172.16.42.1:22$' "$work/preflight/stdout"
check "preflight: every remote call used BatchMode" sh -c '! grep -q "stub: ssh without BatchMode" "$1"' sh "$work/preflight/stderr"

export STUB_IDENTITY=wrong-model
run_radios wrong_model preflight
check "preflight: near-match device-tree model is refused" [ "$status" -eq 4 ]
check "preflight: model refusal names the exact pinned model" grep -Fq 'ZUMA PRO TEGU MP on ZUMA PRO' "$work/wrong_model/stderr"
export STUB_IDENTITY=tegu

# --- QEMU refusal ---
export STUB_IDENTITY=qemu
run_radios qemu inventory
check "qemu: linux,dummy-virt guest is refused with exit 5" [ "$status" -eq 5 ]
check "qemu: refusal is recorded" grep -q '^guest_identity=refused-qemu$' "$work/qemu/stdout"
check "qemu: refusal emits a machine-readable failure" grep -q '"event":"pixel-9a-radios".*"status":"failed".*"exit":5' "$work/qemu/stderr"
check "qemu: no inventory command ran after refusal" [ "$(wc -l < "$calls" | tr -d ' ')" -eq 1 ]
export STUB_IDENTITY=tegu

# --- unreachable guest ---
export STUB_UNREACHABLE=1
run_radios unreachable preflight
check "unreachable: SSH transport failure exits 3" [ "$status" -eq 3 ]
unset STUB_UNREACHABLE

# --- inventory ---
run_radios inventory inventory
check "inventory: succeeds" [ "$status" -eq 0 ]
tsv="$work/inventory/pixel-9a/radios/evidence-latest.tsv"
check "inventory: writes evidence-latest.tsv" [ -f "$tsv" ]
check "inventory: header has subsystem/status/progress/evidence" grep -q "^subsystem	status	progress	evidence$" "$tsv"
check "inventory: network supported" grep -q "^network	supported	linux-and-ferrix-adapter-observed	" "$tsv"
check "inventory: bluetooth supported" grep -q "^bluetooth	supported	linux-and-ferrix-adapter-observed	" "$tsv"
check "inventory: telephony unsupported with the CPIF sentence" \
  sh -c 'grep "^telephony	unsupported	no-modemmanager-modem	" "$1" | grep -Fq "$2"' sh "$tsv" "$cpif_sentence"
check "inventory: CPIF surface fields are carried into evidence" \
  sh -c 'grep "^telephony	" "$1" | grep -Fq "cpif_platform=present cpif_driver=bound fmt_channels=2 sit_ports=2 at_ports=1 rmnet_count=30"' sh "$tsv"
check "inventory: SIT and AT WWAN ports are carried into evidence" \
  sh -c 'grep "^telephony	" "$1" | grep -Fq "sit_ports=2 at_ports=1"' sh "$tsv"
check "inventory: kernel CPIF state is carried into evidence" \
  sh -c 'grep "^telephony	" "$1" | grep -Fq "cpif_state=ONLINE"' sh "$tsv"
check "inventory: cellular data evidence names typed requests and grant" \
  sh -c 'grep "^telephony	" "$1" | grep -Fq "connect-data/disconnect-data via the telephony-data grant"' sh "$tsv"
check "inventory: cellular data evidence withholds APN and records bearer surface" \
  sh -c 'grep "^telephony	" "$1" | grep -Fq "APN value withheld" && grep "^telephony	" "$1" | grep -Fq "bearer/interface evidence" && grep "^telephony	" "$1" | grep -Fq "rmnet_count=30"' sh "$tsv"
check "inventory: status column is only supported|unsupported" \
  awk -F '\t' '!/^#/ && $1 != "subsystem" && $2 != "supported" && $2 != "unsupported" { bad = 1 } END { exit bad }' "$tsv"
check "inventory: every evidence row names the physical target" \
  sh -c 'test "$(grep -vc "^#" "$1")" -eq 8 && test "$(grep -c "	physical: $2; " "$1")" -eq 7' sh "$tsv" "$target_id"
check "inventory: evidence never claims QEMU or mock" sh -c '! grep -Eiq "	(qemu|mock)[;:]" "$1"' sh "$tsv"
transcript=$(find "$work/inventory/pixel-9a/radios" -name 'inventory-*.txt' -type f | head -1)
check "inventory: transcript exists" [ -n "$transcript" ]
check "inventory: all staged firmware rows are hash-verified" \
  sh -c 'grep -q "firmware present 14/14" "$1" && grep -q "firmware_manifest=verified" "$2"' sh "$tsv" "$transcript"
check "inventory: transcript queried Linux services and Ferrix adapters" \
  sh -c 'grep -q "^## nm-devices" "$1" && grep -q "^## bluetoothctl-show" "$1" && grep -q "^## mm-list" "$1" && grep -q "^## ferrix-logs" "$1" && grep -q "^## ferrix-core-state" "$1" && grep -q "^## ferrix-networkd-state" "$1" && grep -q "^## cpif-surface" "$1" && grep -q "^## associated-radios" "$1"' sh "$transcript"
check "privacy: inventory transcript has no plaintext MAC" \
  sh -c '! grep -Eq "[0-9A-Fa-f]{2}(:[0-9A-Fa-f]{2}){5}" "$1"' sh "$transcript"
check "privacy: inventory transcript hashes MACs" grep -q 'Controller mac:[0-9a-f]\{12\}' "$transcript"
check "privacy: inventory transcript has no global IPv6" sh -c '! grep -q "2001:db8" "$1"' sh "$transcript"
check "inventory: transcript labels the physical target" grep -q "^# evidence: physical: $target_id" "$transcript"
check "inventory: read-only (no mutation reached the stub)" [ ! -f "$calls.mutations" ]
check "inventory: privileged reads use the detected doas prefix" grep -q '^doas dmesg' "$calls"

associated_inventory_tsv="$work/inventory/pixel-9a/radios/associated-evidence-latest.tsv"
check "inventory: associated evidence is separate from capabilities TSV" [ -f "$associated_inventory_tsv" ]
check "inventory: associated evidence has four unsupported rows" \
  sh -c 'test "$(grep -vc "^#" "$1")" -eq 5 && test "$(grep -c "	unsupported	" "$1")" -eq 4' sh "$associated_inventory_tsv"
check "inventory: GNSS associated evidence records raw-device-only boundary" \
  sh -c 'grep "^gnss	unsupported	no-gnss-kepler-surface	" "$1" | grep -Fq "raw-device-only"' sh "$associated_inventory_tsv"
check "inventory: NFC associated evidence records unselected native route" \
  sh -c 'grep "^nfc	unsupported	no-linux-nfc-st21-surface	" "$1" | grep -Fq "NFC/ESE driver"' sh "$associated_inventory_tsv"
check "inventory: ST54 associated evidence remains unselected" \
  sh -c 'grep "^st54	unsupported	no-st54-linux-surface	" "$1" | grep -Fq "Android ST54 module"' sh "$associated_inventory_tsv"
check "inventory: UWB associated evidence remains unselected" \
  sh -c 'grep "^uwb	unsupported	no-linux-uwb-surface	" "$1" | grep -Fq "target audit selects no UWB route"' sh "$associated_inventory_tsv"

# The focused command performs the same exact-target preflight and emits only
# the associated-radio TSV; it never changes guest state.
run_radios associated associated
associated_command_tsv="$work/associated/pixel-9a/radios/associated-evidence-latest.tsv"
check "associated: focused probe succeeds" [ "$status" -eq 0 ]
check "associated: focused probe writes separate TSV" [ -f "$associated_command_tsv" ]
check "associated: focused probe declares read-only" grep -q '^associated_probe=read-only$' "$work/associated/stdout"
check "associated: focused probe does not mutate" [ ! -f "$calls.mutations" ]

# A visible kernel surface must still remain unsupported: the probe is
# diagnostic evidence only and no associated-radio Ferrix adapter exists.
associated_present="$work/associated-present-fixtures"
cp -R "$fixtures" "$associated_present"
cat > "$associated_present/associated-radios.txt" <<'EOF'
gnss_devices=1
gnss_class_devices=1
gnss_kepler_nodes=1
gnss_kepler_bound=1
gnss_kepler_module=absent
nfc_class_devices=1
nfc_st21_nodes=1
nfc_st21_bound=1
st21_module=loaded
st54_nodes=1
st54_bound=1
st54_module=loaded
uwb_class_devices=1
uwb_nodes=1
uwb_reset_nodes=1
EOF
export STUB_FIXTURES="$associated_present"
run_radios associated_present inventory
associated_tsv="$work/associated_present/pixel-9a/radios/associated-evidence-latest.tsv"
check "inventory: visible KEPLER surface remains unsupported" \
  sh -c 'grep "^gnss	unsupported	kernel-gnss-device-present-location-adapter-unimplemented	" "$1" | grep -Fq "GNSS raw/class device observed"' sh "$associated_tsv"
check "inventory: visible ST21 surface remains unsupported" \
  sh -c 'grep "^nfc	unsupported	linux-nfc-surface-st21-or-class-present	" "$1" | grep -Fq "native NFC/ESE route is not selected"' sh "$associated_tsv"
check "inventory: visible ST54 surface remains unsupported" \
  sh -c 'grep "^st54	unsupported	st54-surface-secure-element-boundary-unreviewed	" "$1" | grep -Fq "secure-element route is not selected"' sh "$associated_tsv"
check "inventory: visible UWB surface remains unsupported" \
  sh -c 'grep "^uwb	unsupported	linux-uwb-surface-present-driver-unreviewed	" "$1" | grep -Fq "no selected native UWB route"' sh "$associated_tsv"
export STUB_FIXTURES="$fixtures"

# The radio-first kernel is intentionally a separate ABI from Google's CPIF
# route. Its read-only inventory must not describe a bound s5300_modem WWAN /
# SIT surface as if it were the legacy `/dev/umts_ipc*` path.
wwan_sit_fixtures="$work/wwan-sit-fixtures"
cp -R "$fixtures" "$wwan_sit_fixtures"
cat > "$wwan_sit_fixtures/cpif-surface.txt" <<'EOF'
radio_backend=wwan-sit
cpif_platform=absent
cpif_driver=unbound
cpif_state=unknown
s5300_modem_module=loaded
s5300_modem_driver=bound
wwan_devices=1
fmt_channels=0
sit_ports=2
at_ports=1
rmnet_count=30
EOF
export STUB_FIXTURES="$wwan_sit_fixtures"
run_radios wwan_sit inventory
wwan_sit_tsv="$work/wwan_sit/pixel-9a/radios/evidence-latest.tsv"
check "inventory: radio-first WWAN/SIT route stays unsupported without ModemManager" \
  sh -c 'awk -F "\t" '\''$1 == "telephony" && $2 == "unsupported" && $3 == "wwan-sit-surface-no-modemmanager-modem" { found=1 } END { exit !found }'\'' "$1"' sh "$wwan_sit_tsv"
check "inventory: radio-first evidence identifies s5300_modem separately from CPIF" \
  sh -c 'grep "^telephony\t" "$1" | grep -Fq "radio-first s5300_modem WWAN/SIT surface observed" && grep -Fq "s5300_modem_module=loaded s5300_modem_driver=bound wwan_devices=1" "$1"' sh "$wwan_sit_tsv"
export STUB_FIXTURES="$fixtures"

# A manifest mismatch must never be promoted to working networking, even when
# NetworkManager and the kernel driver otherwise report a connected device.
firmware_bad="$work/firmware-bad-fixtures"
cp -R "$fixtures" "$firmware_bad"
sed -i.bak 's#firmware=/lib/firmware/bcmdhd.cal=verified#firmware=/lib/firmware/bcmdhd.cal=mismatch#; s#^firmware_manifest=verified$#firmware_manifest=invalid#' "$firmware_bad/firmware.txt"
rm -f "$firmware_bad/firmware.txt.bak"
export STUB_FIXTURES="$firmware_bad"
run_radios firmware_mismatch inventory
firmware_bad_tsv="$work/firmware_mismatch/pixel-9a/radios/evidence-latest.tsv"
check "inventory: firmware mismatch stays unsupported" \
  sh -c 'awk -F "\t" '\''$1 == "network" && $2 == "unsupported" && $3 == "device-present-driver-or-firmware-missing" { found=1 } END { exit !found }'\'' "$1"' sh "$firmware_bad_tsv"
export STUB_FIXTURES="$fixtures"

# A powered controller without the exact factory-selected BCM.hcd must not be
# promoted to working Bluetooth. This catches a stale/generic HCI surface even
# when BlueZ and ferrix-core both report an enabled adapter.
firmware_bt_bad="$work/firmware-bt-bad-fixtures"
cp -R "$fixtures" "$firmware_bt_bad"
sed -i.bak 's#^firmware=/lib/firmware/brcm/BCM.hcd=verified$#firmware=/lib/firmware/brcm/BCM.hcd=mismatch#; s#^firmware_manifest=verified$#firmware_manifest=invalid#' "$firmware_bt_bad/firmware.txt"
rm -f "$firmware_bt_bad/firmware.txt.bak"
export STUB_FIXTURES="$firmware_bt_bad"
run_radios bluetooth_firmware_mismatch inventory
firmware_bt_bad_tsv="$work/bluetooth_firmware_mismatch/pixel-9a/radios/evidence-latest.tsv"
check "inventory: Bluetooth firmware mismatch stays unsupported" \
  sh -c 'awk -F "\t" '\''$1 == "bluetooth" && $2 == "unsupported" && $3 == "controller-powered-firmware-unverified" { found=1 } END { exit !found }'\'' "$1"' sh "$firmware_bt_bad_tsv"
check "inventory: Bluetooth firmware mismatch names exact BCM.hcd path" \
  sh -c 'grep "^bluetooth\t" "$1" | grep -Fq "/lib/firmware/brcm/BCM.hcd"' sh "$firmware_bt_bad_tsv"
export STUB_FIXTURES="$fixtures"

# A present driver/controller is not sufficient evidence. If the typed
# service sockets return an authorization/error envelope or the core reports
# a disconnected network, the workflow must stay conservative.
degraded="$work/degraded-fixtures"
cp -R "$fixtures" "$degraded"
printf '%s\n' '{"reply":"hello","schema_version":1}' '{"reply":"error","error":{"kind":"unauthorized","capability":"network-state"}}' > "$degraded/networkd-probe.txt"
printf '%s\n' '{"reply":"state","schema_version":1,"target":"google-pixel-9a-tegu-gxq96-mp1-0","profile":"pixel-9a-native-linux","capabilities":{"bluetooth":"unsupported","network":"unsupported"},"state":{"network":{"wifi":"disconnected"},"bluetooth":{"present":true,"enabled":true}}}' > "$degraded/core-probe.txt"
export STUB_FIXTURES="$degraded"
run_radios degraded inventory
degraded_tsv="$work/degraded/pixel-9a/radios/evidence-latest.tsv"
check "inventory: degraded typed state is not promoted" \
  sh -c 'awk -F "\t" '\''$1 == "network" && $2 == "unsupported" && $3 == "linux-wifi-up-ferrix-adapter-not-observed" { network=1 } $1 == "bluetooth" && $2 == "supported" && $3 == "linux-and-ferrix-adapter-observed" { bluetooth=1 } END { exit !(network && bluetooth) }'\'' "$1"' sh "$degraded_tsv"
export STUB_FIXTURES="$fixtures"

# --- enable: wrong confirmation ---
printf 'CONFIRM PIXEL 9A ENABLE RADIOS: wifi\n' > "$work/wrong-phrase"
mkdir -p "$work/enable_wrong"
export FERRIX_OUT_DIR="$work/enable_wrong"
: > "$calls"; rm -f "$calls.mutations"
status=0
"$radios" enable all < "$work/wrong-phrase" > "$work/enable_wrong/stdout" 2> "$work/enable_wrong/stderr" || status=$?
check "enable: mismatched phrase exits 6" [ "$status" -eq 6 ]
check "enable: mismatched phrase performs zero mutations" [ ! -f "$calls.mutations" ]
check "enable: mismatched phrase logs no mutation command" \
  sh -c '! grep -Eq "rfkill unblock|radio wifi on|power on|--enable" "$1"' sh "$calls"

# --- enable: correct confirmation ---
printf 'CONFIRM PIXEL 9A ENABLE RADIOS: wifi bluetooth modem\n' > "$work/right-phrase"
mkdir -p "$work/enable_ok"
export FERRIX_OUT_DIR="$work/enable_ok"
: > "$calls"; rm -f "$calls.mutations"
status=0
"$radios" enable all < "$work/right-phrase" > "$work/enable_ok/stdout" 2> "$work/enable_ok/stderr" || status=$?
check "enable: matching phrase succeeds" [ "$status" -eq 0 ]
expected_mutations='doas rfkill unblock wifi
doas nmcli radio wifi on
doas rfkill unblock bluetooth
doas bluetoothctl power on'
actual_mutations=$(grep -E '^doas (rfkill unblock|nmcli radio wifi on|bluetoothctl power on|mmcli -m any --enable)' "$calls" || true)
check "enable: exact ordered mutation commands" [ "$actual_mutations" = "$expected_mutations" ]
[ "$actual_mutations" = "$expected_mutations" ] || printf 'actual:\n%s\n' "$actual_mutations"
check "enable: prints mutation= before each command" \
  sh -c 'test "$(grep -c "^mutation=doas " "$1")" -eq 4' sh "$work/enable_ok/stdout"
check "enable: modem skipped with the CPIF note when mmcli lists none" \
  sh -c 'grep "^modem=skipped" "$1" | grep -Fq "$2"' sh "$work/enable_ok/stdout" "$cpif_sentence"
check "enable: no wwan/modem mutation without a modem" sh -c '! grep -Eq "wwan|--enable" "$1"' sh "$calls"
check "enable: confirmation happens after identity verification" \
  sh -c 'grep -q "^guest_identity=verified$" "$1" && grep -q "^confirmation=accepted$" "$1"' sh "$work/enable_ok/stdout"

# The guest identity is re-read after confirmation and before the first radio
# mutation. A changed device-tree model must therefore issue no mutation.
export STUB_IDENTITY_RECHECK=wrong-model
printf 'CONFIRM PIXEL 9A ENABLE RADIOS: wifi\n' > "$work/changed-identity-phrase"
run_radios enable_changed_identity enable wifi < "$work/changed-identity-phrase"
check "enable: changed guest identity is refused after confirmation" [ "$status" -eq 4 ]
check "enable: changed guest identity issues zero mutations" [ ! -f "$calls.mutations" ]
check "enable: changed guest identity names the recheck" grep -q 'guest device-tree model changed before mutation' "$work/enable_changed_identity/stderr"
unset STUB_IDENTITY_RECHECK

# A failed remote radio mutation must stop the selection immediately.  The
# previous implementation logged mutation_exit=42 but returned success, which
# could make a partial enable look complete and continue to the next radio.
export STUB_MUTATION_FAIL='doas nmcli radio wifi on'
printf 'CONFIRM PIXEL 9A ENABLE RADIOS: wifi bluetooth\n' > "$work/failing-mutation-phrase"
run_radios enable_failed_mutation enable wifi bluetooth < "$work/failing-mutation-phrase"
check "enable: remote mutation failure exits nonzero" [ "$status" -eq 1 ]
check "enable: failure explains the failed command" grep -q '^FAIL pixel-9a-radios: radio mutation failed (exit 42): doas nmcli radio wifi on$' "$work/enable_failed_mutation/stderr"
check "enable: later radio mutations are not attempted after failure" \
  sh -c '! grep -q "bluetoothctl power on" "$1"' sh "$calls"
unset STUB_MUTATION_FAIL

# --- connect-wifi ---
ssid='Synthetic Home 5G'
psk='s3cret-psk-13'
mkdir -p "$work/connect"
export FERRIX_OUT_DIR="$work/connect" FERRIX_PIXEL9A_WIFI_PSK="$psk"
: > "$calls"
status=0
ssid_hash=$(printf '%s' "$ssid" | shasum -a 256 | cut -c1-12)
printf 'CONFIRM PIXEL 9A CONNECT WIFI SSID SHA256 %s\n' "$ssid_hash" | \
  "$radios" connect-wifi "$ssid" > "$work/connect/stdout" 2> "$work/connect/stderr" || status=$?
unset FERRIX_PIXEL9A_WIFI_PSK
check "connect-wifi: succeeds" [ "$status" -eq 0 ]
check "connect-wifi: reports wifi_connected=yes" grep -q '^wifi_connected=yes$' "$work/connect/stdout"
connect_transcript=$(find "$work/connect/pixel-9a/radios" -name 'connect-wifi-*.txt' -type f | head -1)
check "connect-wifi: transcript exists" [ -n "$connect_transcript" ]
check "connect-wifi: PSK reached nmcli --ask via stdin" grep -q '^psk-bytes=13$' "$connect_transcript"
check "privacy: PSK is absent from the remote command log" sh -c '! grep -Fq "$2" "$1"' sh "$calls" "$psk"
check "privacy: PSK is absent from transcript and stdout" \
  sh -c '! grep -Fq "$3" "$1" && ! grep -Fq "$3" "$2"' sh "$connect_transcript" "$work/connect/stdout" "$psk"
check "privacy: PSK is not in the nmcli argv" sh -c 'grep "nmcli --ask device wifi connect" "$1" | grep -vq password' sh "$calls"
check "privacy: SSID plaintext is absent from transcript and stdout" \
  sh -c '! grep -Fq "$3" "$1" && ! grep -Fq "$3" "$2"' sh "$connect_transcript" "$work/connect/stdout" "$ssid"
check "connect-wifi: SSID hash recorded" grep -q '^ssid_sha256=[0-9a-f]\{12\}$' "$connect_transcript"
check "connect-wifi: mutation line hashes the SSID" grep -q '^mutation=doas nmcli --ask device wifi connect .ssid:[0-9a-f]\{12\}' "$work/connect/stdout"

# --- redaction filter ---
redacted=$(REDACT_SSID='Synthetic Home 5G' "$radios" redact < "$fixtures/redaction.txt")
check "redact: no plaintext MAC" sh -c '! printf "%s\n" "$1" | grep -Eq "[0-9A-Fa-f]{2}(:[0-9A-Fa-f]{2}){5}"' sh "$redacted"
check "redact: MACs become mac:<hash>" sh -c 'test "$(printf "%s\n" "$1" | grep -c "mac:[0-9a-f]\{12\}")" -eq 2' sh "$redacted"
check "redact: IMEI/ICCID/IMSI digits removed" \
  sh -c '! printf "%s\n" "$1" | grep -Eq "356789012345678|89014103211118510720|310150123456789"' sh "$redacted"
check "redact: identifiers marked" sh -c 'test "$(printf "%s\n" "$1" | grep -c "<redacted-id>")" -eq 3' sh "$redacted"
check "redact: phone number removed" sh -c '! printf "%s\n" "$1" | grep -q "15551234567"' sh "$redacted"
check "redact: serials removed" sh -c '! printf "%s\n" "$1" | grep -Eq "3A091FDJG0012X|SYN0001234"' sh "$redacted"
check "redact: SSID plaintext removed" sh -c '! printf "%s\n" "$1" | grep -q "Synthetic Home 5G"' sh "$redacted"
check "redact: SSID hashed consistently" \
  sh -c 'test "$(printf "%s\n" "$1" | grep -o "ssid:[0-9a-f]\{12\}" | sort -u | wc -l | tr -d " ")" -eq 1' sh "$redacted"
check "redact: IPv6 global/ULA removed" sh -c '! printf "%s\n" "$1" | grep -Eq "2001:db8|fd12:3456"' sh "$redacted"
check "redact: ordinary lines survive" sh -c 'printf "%s\n" "$1" | grep -q "^ordinary line with hostname ferrix-tegu stays readable$"' sh "$redacted"

# --- evidence fragment ---
run_radios evidence evidence
check "evidence: succeeds" [ "$status" -eq 0 ]
check "evidence: prints network/bluetooth/telephony tables" \
  sh -c 'grep -q "^\[network\]$" "$1" && grep -q "^\[bluetooth\]$" "$1" && grep -q "^\[telephony\]$" "$1"' sh "$work/evidence/stdout"
check "evidence: fragment names the physical target" grep -q "evidence = \"physical: $target_id; " "$work/evidence/stdout"
check "evidence: statuses are supported/unsupported only" \
  sh -c 'test "$(grep -c "^status = \"supported\"$" "$1")" -eq 2 && test "$(grep -c "^status = \"unsupported\"$" "$1")" -eq 1 && ! grep -Eq "^status = \"(partial|mocked)\"" "$1"' sh "$work/evidence/stdout"
check "evidence: telephony carries the CPIF sentence" sh -c 'grep -Fq "$2" "$1"' sh "$work/evidence/stdout" "$cpif_sentence"
check "evidence: capabilities.toml is untouched" \
  sh -c 'cd "$1" && git diff --quiet -- "targets/boards/$2/capabilities.toml"' sh "$root" "$target_id"

# --- usage ---
run_radios usage bogus
check "usage: unknown command exits 2 without contacting the guest" sh -c 'test "$1" -eq 2 && test ! -s "$2"' sh "$status" "$calls"

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-radios-test","status":"passed"}'
else
  printf '{"event":"pixel-9a-radios-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
