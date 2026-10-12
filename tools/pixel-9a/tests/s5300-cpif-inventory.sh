#!/bin/sh
# Offline proof for the issue #1 E1 read-only CPIF inventory. No device is
# contacted. Three layers:
#   1. static checks that the guest probe can only stat CPIF /dev nodes;
#   2. the real probe, with its three roots rewritten to a fixture tree whose
#      CPIF "nodes" are FIFOs: open(2) on a FIFO with no peer blocks, so any
#      accidental open hangs and fails the bounded run;
#   3. the host workflow through the real CDC-ACM transport and a fake guest
#      (acm-fake-guest.sh) answering with synthetic probe transcripts.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
inventory="$root/tools/pixel-9a/s5300-cpif-inventory.sh"
probe="$root/tools/pixel-9a/s5300-cpif-inventory-remote.sh"
fails=0
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for the CDC-ACM transport test' >&2; exit 1; }

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

nodes='umts_boot0 umts_ipc0 umts_ipc1 umts_rfs0 oem_ipc0 oem_ipc1 oem_ipc2 oem_ipc3 oem_ipc4 oem_ipc5 oem_ipc6 oem_ipc7'

# --- 1. static ------------------------------------------------------------------

check "static: host POSIX syntax" sh -n "$inventory"
check "static: probe POSIX syntax" sh -n "$probe"
for script in "$inventory" "$probe"; do
	offending=$(grep -nwE 'adb|fastboot|ssh|scp|nc|telnet|curl|dmesg|modprobe|insmod|rmmod|mmcli|rc-service|rc-update|openrc|exynos-modem|sit-proxy|ioctl|mknod|mount|dd|reboot|flash|erase' "$script" |
		grep -vE '^[0-9]+:[[:space:]]*#' || true)
	check "static: no device, service, module, or log verbs in ${script##*/}" [ -z "$offending" ]
	[ -z "$offending" ] || printf '%s\n' "$offending"
done
code=$(grep -vE '^[[:space:]]*#' "$probe")
check "static: the probe has the three default roots" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq proc_root=/proc && printf "%s\n" "$1" | grep -Fxq sys_root=/sys && printf "%s\n" "$1" | grep -Fxq dev_root=/dev' sh "$code"
check "static: no literal CPIF /dev path in the probe" \
	sh -c '! printf "%s\n" "$1" | grep -nE "/dev/(umts|oem|wwan)"' sh "$code"
dev_uses=$(printf '%s\n' "$code" | grep -nF 'dev_root' |
	grep -vxE '[0-9]+:dev_root=/dev' |
	grep -vxE '[0-9]+:[[:space:]]*path="\$dev_root/\$node"' |
	grep -vxE '[0-9]+:for entry in "\$dev_root"/oem_ipc\*; do' || true)
check "static: dev_root is used only to build node paths and list oem_ipc names" [ -z "$dev_uses" ]
[ -z "$dev_uses" ] || printf '%s\n' "$dev_uses"
path_uses=$(printf '%s\n' "$code" | grep -nF '"$path"' |
	grep -vE '\[ -[Lcbpe] "\$path" \]' |
	grep -vxE "[0-9]+:[[:space:]]*meta=\\\$\\(stat -L -c '%t %T %a %u %g' \"\\\$path\" 2>/dev/null\\) \\|\\| meta=''" || true)
check "static: node paths reach only file tests and stat(2)" [ -z "$path_uses" ]
[ -z "$path_uses" ] || printf '%s\n' "$path_uses"
check "static: oem_ipc glob entries reach only file tests" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -cF "\"\$entry\"" )" -eq "$(printf "%s\n" "$1" | grep -cE "\[ -[eLf] \"\\\$entry\"")" ]' sh "$code"
redirects=$(printf '%s\n' "$code" | grep -nE '[^2]>[^&]|^>' | grep -vE '>/dev/null' || true)
check "static: the probe never writes a file" [ -z "$redirects" ]
[ -z "$redirects" ] || printf '%s\n' "$redirects"
check "static: probe gates on the exact tegu device tree" \
	sh -c 'grep -Fq "ZUMA PRO TEGU MP on ZUMA PRO" "$1" && grep -Fq "google,ZUMA PRO TEGU" "$1"' sh "$probe"
check "static: exactly one guest session, through the CDC-ACM transport" \
	sh -c '[ "$(grep -v "^[[:space:]]*#" "$1" | grep -cE "python3 \"\\\$transport\" --probe \"\\\$probe\" ")" -eq 1 ]' sh "$inventory"
check "static: the host never edits capabilities or status" \
	sh -c '! grep -v "^[[:space:]]*#" "$1" | grep -nE "capabilities\.toml|docs/status"' sh "$inventory"

probe_status=0
sh "$probe" >/dev/null 2>&1 </dev/null || probe_status=$?
check "probe: refuses a host that is not the tegu board" [ "$probe_status" -eq 4 ]

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-s5300-cpif-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; rm -rf "$work"' EXIT

# --- 2. real probe against a FIFO fixture tree -------------------------------------

fx="$work/tree"
mkdir -p "$fx/proc/device-tree" "$fx/sys/bus/platform/devices" "$fx/sys/module/cpif" "$fx/dev" \
	"$fx/sys/class/cpif" "$fx/sys/class/net/lo" "$fx/sys/bus/platform/drivers/cpif"
printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fx/proc/device-tree/model"
printf 'google,ZUMA PRO TEGU\000google,ZUMA PRO\000' >"$fx/proc/device-tree/compatible"
printf 'cpif 1 0 - Live 0x0\nshm_ipc 1 1 cpif, Live 0x0\ncpif_page 1 1 cpif, Live 0x0\n' >"$fx/proc/modules"
mkdir -p "$fx/sys/bus/platform/devices/cpif/of_node" "$fx/sys/bus/platform/devices/other/of_node"
printf 'samsung,exynos-cp\000' >"$fx/sys/bus/platform/devices/cpif/of_node/compatible"
printf 'samsung,exynos-cp-thermal\000' >"$fx/sys/bus/platform/devices/other/of_node/compatible"
ln -s ../../drivers/cpif "$fx/sys/bus/platform/devices/cpif/driver"
printf 'OFFLINE\n' >"$fx/sys/bus/platform/devices/cpif/modem_state"
minor=0
for node in $nodes; do
	mkfifo "$fx/dev/$node"
	mkdir -p "$fx/sys/class/cpif/$node"
	printf '510:%s\n' "$minor" >"$fx/sys/class/cpif/$node/dev"
	ln -s ../../cpif "$fx/sys/class/cpif/$node/subsystem"
	minor=$((minor + 1))
done
sed -e "s#^proc_root=/proc\$#proc_root=$fx/proc#" -e "s#^sys_root=/sys\$#sys_root=$fx/sys#" \
	-e "s#^dev_root=/dev\$#dev_root=$fx/dev#" "$probe" >"$work/probe-fixture.sh"
check "fixture probe: only the three roots differ from the checked-in probe" \
	sh -c '[ "$(diff "$1" "$2" | grep -c "^>")" -eq 3 ]' sh "$probe" "$work/probe-fixture.sh"
sh "$work/probe-fixture.sh" >"$work/probe-fixture.out" 2>"$work/probe-fixture.err" </dev/null &
probe_pid=$!
waited=0
while kill -0 "$probe_pid" 2>/dev/null && [ "$waited" -lt 100 ]; do
	sleep 0.1
	waited=$((waited + 1))
done
hung=no
if kill -0 "$probe_pid" 2>/dev/null; then
	hung=yes
	kill "$probe_pid" 2>/dev/null || :
fi
real_status=0
wait "$probe_pid" 2>/dev/null || real_status=$?
fo() { grep -Fxq "$1" "$work/probe-fixture.out"; }
check "fixture probe: finishes without opening any FIFO node" [ "$hung" = no ]
check "fixture probe: exit 0" [ "$real_status" -eq 0 ]
check "fixture probe: complete" fo 'probe_complete=yes'
check "fixture probe: modules observed Live" sh -c 'grep -Fxq module.cpif.state=Live "$1" && grep -Fxq module.cpif_page.count=1 "$1"' sh "$work/probe-fixture.out"
check "fixture probe: exactly the exynos-cp platform device" fo 'cp_platform_count=1'
check "fixture probe: driver name from the link" fo 'cp_platform_driver=cpif'
check "fixture probe: modem_state read from sysfs" fo 'cp_modem_state=OFFLINE'
check "fixture probe: every node seen as a FIFO, not a char device" \
	sh -c '[ "$(grep -c "^node\.[a-z_0-9]*\.kind=fifo$" "$1")" -eq 12 ]' sh "$work/probe-fixture.out"
check "fixture probe: sysfs class and dev recorded" sh -c 'grep -Fxq node.umts_ipc1.classes=cpif "$1" && grep -Fxq node.umts_ipc1.sysfs_dev=510:2 "$1" && grep -Fxq node.umts_ipc1.subsystem=cpif "$1"' sh "$work/probe-fixture.out"
check "fixture probe: oem_ipc names counted" fo 'oem_ipc_name_count=8'
check "fixture probe: no rmnet before CP boot" fo 'rmnet_count=0'

# --- 3. host workflow through the fake CDC-ACM guest ---------------------------------

mkdir -p "$work/fx"
acm_guest_init "$root" "$work" "$probe"
unset FERRIX_PIXEL9A_ACM_TIMEOUT 2>/dev/null || true

{
	printf 'probe=s5300-cpif-inventory\ndt_model=ZUMA PRO TEGU MP on ZUMA PRO\ndt_compatible=google,ZUMA PRO TEGU\n'
	printf 'kernel_release=6.1.124-android16-test\nuid=0\nmodules_readable=yes\n'
	for m in cpif shm_ipc cpif_page; do printf 'module.%s.count=1\nmodule.%s.state=Live\nmodule.%s.sysfs=yes\n' "$m" "$m" "$m"; done
	printf 'platform_readable=yes\ncp_platform_count=1\ncp_platform_driver=cpif\ncp_modem_state_file=present\ncp_modem_state=OFFLINE\nstat_impl=present\n'
	minor=0
	for node in $nodes; do
		printf 'node.%s.kind=char\nnode.%s.link=no\nnode.%s.rdev=510:%s\nnode.%s.mode=660\nnode.%s.owner=0:1001\n' \
			"$node" "$node" "$node" "$minor" "$node" "$node"
		printf 'node.%s.classes=cpif\nnode.%s.sysfs_dev=510:%s\nnode.%s.subsystem=cpif\n' "$node" "$node" "$minor" "$node"
		minor=$((minor + 1))
	done
	printf 'oem_ipc_name_count=8\nwwan_devices=0\nrmnet_count=0\nprobe_complete=yes\n'
} >"$work/fx/go"

derive() {
	name=$1
	shift
	sed "$@" "$work/fx/go" >"$work/fx/$name"
}
derive rmnet -e 's/^rmnet_count=0$/rmnet_count=30/'
derive misc -e 's/\.classes=cpif$/.classes=misc/' -e 's/\.subsystem=cpif$/.subsystem=misc/'
derive mixed -e 's/^node\.oem_ipc3\.classes=cpif$/node.oem_ipc3.classes=misc/' -e 's/^node\.oem_ipc3\.subsystem=cpif$/node.oem_ipc3.subsystem=misc/'
derive absent -e 's/^module\.cpif\.count=1$/module.cpif.count=0/' -e 's/^module\.cpif\.state=Live$/module.cpif.state=/' -e 's/^module\.cpif\.sysfs=yes$/module.cpif.sysfs=no/'
derive builtin -e 's/^module\.shm_ipc\.count=1$/module.shm_ipc.count=0/' -e 's/^module\.shm_ipc\.state=Live$/module.shm_ipc.state=/'
derive loading -e 's/^module\.cpif_page\.state=Live$/module.cpif_page.state=Loading/'
derive modules-unreadable -e 's/^modules_readable=yes$/modules_readable=no/' -e 's/^module\.\([a-z_]*\)\.count=1$/module.\1.count=0/' -e 's/^module\.\([a-z_]*\)\.state=Live$/module.\1.state=/'
derive module-dup -e 's/^module\.cpif\.count=1$/module.cpif.count=2/'
derive no-platform -e 's/^cp_platform_count=1$/cp_platform_count=0/' -e 's/^cp_platform_driver=cpif$/cp_platform_driver=/' \
	-e 's/^cp_modem_state_file=present$/cp_modem_state_file=missing/' -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=/'
derive two-platforms -e 's/^cp_platform_count=1$/cp_platform_count=2/' -e 's/^cp_platform_driver=cpif$/cp_platform_driver=/' \
	-e 's/^cp_modem_state_file=present$/cp_modem_state_file=missing/' -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=/'
derive state-without-platform -e 's/^cp_platform_count=1$/cp_platform_count=0/'
derive unbound -e 's/^cp_platform_driver=cpif$/cp_platform_driver=/'
derive online -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=ONLINE/'
derive crashed -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=WDT_RESET/'
derive booting -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=BOOTING/'
derive future-state -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=FUTURE_STATE/'
derive state-unreadable -e 's/^cp_modem_state_file=present$/cp_modem_state_file=unreadable/' -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=/'
derive state-missing -e 's/^cp_modem_state_file=present$/cp_modem_state_file=missing/' -e 's/^cp_modem_state=OFFLINE$/cp_modem_state=/'
derive node-missing -e 's/^node\.oem_ipc7\.kind=char$/node.oem_ipc7.kind=missing/' -e 's/^node\.oem_ipc7\.rdev=.*/node.oem_ipc7.rdev=/' \
	-e 's/^node\.oem_ipc7\.classes=cpif$/node.oem_ipc7.classes=/' -e 's/^node\.oem_ipc7\.sysfs_dev=.*/node.oem_ipc7.sysfs_dev=/' \
	-e 's/^node\.oem_ipc7\.subsystem=cpif$/node.oem_ipc7.subsystem=/' -e 's/^oem_ipc_name_count=8$/oem_ipc_name_count=7/'
derive node-symlink -e 's/^node\.umts_ipc0\.link=no$/node.umts_ipc0.link=yes/'
derive node-block -e 's/^node\.umts_rfs0\.kind=char$/node.umts_rfs0.kind=block/' -e 's/^node\.umts_rfs0\.rdev=.*/node.umts_rfs0.rdev=/'
derive dev-mismatch -e 's/^node\.umts_boot0\.sysfs_dev=510:0$/node.umts_boot0.sysfs_dev=510:9/'
derive two-classes -e 's/^node\.umts_ipc1\.classes=cpif$/node.umts_ipc1.classes=cpif misc/' -e 's/^node\.umts_ipc1\.sysfs_dev=.*/node.umts_ipc1.sysfs_dev=/' \
	-e 's/^node\.umts_ipc1\.subsystem=cpif$/node.umts_ipc1.subsystem=/'
derive no-class -e 's/^node\.umts_ipc1\.classes=cpif$/node.umts_ipc1.classes=/' -e 's/^node\.umts_ipc1\.sysfs_dev=.*/node.umts_ipc1.sysfs_dev=/' \
	-e 's/^node\.umts_ipc1\.subsystem=cpif$/node.umts_ipc1.subsystem=/'
derive subsystem -e 's/^node\.umts_ipc0\.subsystem=cpif$/node.umts_ipc0.subsystem=misc/'
derive no-stat -e 's/^stat_impl=present$/stat_impl=missing/' -e 's/^node\.\([a-z_0-9]*\)\.rdev=.*/node.\1.rdev=/' \
	-e 's/^node\.\([a-z_0-9]*\)\.mode=.*/node.\1.mode=/' -e 's/^node\.\([a-z_0-9]*\)\.owner=.*/node.\1.owner=/'
derive wwan -e 's/^wwan_devices=0$/wwan_devices=3/'
derive qemu -e 's/^dt_model=.*/dt_model=QEMU Virtual Machine/'
derive unknown-key -e '/^probe_complete=/i\
imei=ABC123'
derive duplicate-key -e '/^probe_complete=/i\
rmnet_count=1'
derive missing-key -e '/^node\.umts_rfs0\.owner=/d'
derive bad-value -e 's/^cp_platform_driver=cpif$/cp_platform_driver=..\/cpif/'
derive bad-rdev -e 's/^node\.umts_ipc0\.rdev=.*/node.umts_ipc0.rdev=1:2:3/'
derive bad-kind -e 's/^node\.umts_ipc0\.kind=char$/node.umts_ipc0.kind=socket/'
derive bad-count -e 's/^rmnet_count=0$/rmnet_count=many/'
derive incomplete-run -e '/^probe_complete=/d'
printf 'probe_complete=no\n' >>"$work/fx/incomplete-run"

status=0
out=''
# run CASE FIXTURE [EXIT [EVIDENCE]]: one fresh fake guest per case.
run() {
	case_dir="$work/run-$1"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl fixture "$work/fx/$2"
	acm_guest_ctl exit "${3:-0}"
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_S5300_CPIF_EVIDENCE="${4:-mock}" \
		sh "$inventory" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	transcript="$case_dir/pixel-9a/s5300-cpif-inventory/inventory-latest.txt"
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }

run go go
check "go: completes" [ "$status" -eq 0 ]
check "go: the probe ran exactly once over CDC-ACM" [ "$runs" -eq 1 ]
check "go: the guest received the exact checked-in probe bytes" cmp -s "$ACM_GUEST_CTL/stdin" "$probe"
check "go: E1 result is go" has 'e1_result=go'
check "go: mock never reports a physical gate" has 'e1_gate=not-evidence-mock'
check "go: modules loaded" sh -c '[ "$(printf "%s\n" "$1" | grep -cE "^module\.[a-z_]+=loaded$")" -eq 3 ]' sh "$out"
check "go: every node is a char device" sh -c '[ "$(printf "%s\n" "$1" | grep -cE "^node\.[a-z_0-9]+=char-device$")" -eq 12 ]' sh "$out"
check "go: class recorded and matches the source claim" sh -c 'printf "%s\n" "$1" | grep -Fxq node_class=cpif && printf "%s\n" "$1" | grep -Fxq node_class_matches_source_claim=yes' sh "$out"
check "go: rmnet recorded before CP boot" has 'rmnet_count_before_cp_boot=0'
check "go: runtime unknowns are always listed" sh -c 'printf "%s\n" "$1" | grep -q "^e1_unknowns=cp-boot-runtime,nv-file-sizes,"' sh "$out"
check "go: no capability change" has 'capability_change=none'
check "go: transcript is labelled mock and states no node was opened" \
	sh -c 'grep -Fxq evidence=mock "$1" && grep -Fxq cp_nodes_opened=no "$1" && grep -Eq "^probe_sha256=[0-9a-f]{64}$" "$1"' sh "$transcript"

run physical go 0 physical
check "physical: gate is go" has 'e1_gate=go'
check "physical: transcript names the exact target" grep -Fxq 'evidence=physical:google-pixel-9a-tegu-gxq96-mp1-0' "$transcript"

run rmnet rmnet
check "rmnet: present before boot is recorded, not a stop" sh -c 'printf "%s\n" "$1" | grep -Fxq rmnet_count_before_cp_boot=30 && printf "%s\n" "$1" | grep -Fxq e1_result=go' sh "$out"
run misc misc
check "misc class: still go, source claim mismatch recorded" sh -c 'printf "%s\n" "$1" | grep -Fxq e1_result=go && printf "%s\n" "$1" | grep -Fxq node_class_matches_source_claim=no' sh "$out"
run mixed mixed
check "mixed classes: no-go" sh -c 'printf "%s\n" "$1" | grep -Fxq node_class=mixed && printf "%s\n" "$1" | grep -Fxq e1_result=no-go' sh "$out"

# expect CASE FIXTURE RESULT FRAGMENT: completed run with that result and
# the fragment in no_go/unknown_facts.
expect() {
	run "$1" "$2"
	check "$1: exit 0" [ "$status" -eq 0 ]
	check "$1: e1_result=$3" has "e1_result=$3"
	check "$1: reports $4" sh -c 'printf "%s\n" "$1" | grep -E "^(no_go|unknown_facts)=" | grep -Fq "$2"' sh "$out" "$4"
}
expect absent absent no-go module-cpif-absent
expect builtin builtin no-go module-shm_ipc-builtin
expect loading loading no-go module-cpif_page-transitional
expect modules-unreadable modules-unreadable incomplete module-cpif
expect no-platform no-platform no-go cp-platform-missing
check "no-platform: modem_state is not applicable" has 'cp_modem_state_status=not-applicable'
expect two-platforms two-platforms no-go cp-platform-ambiguous
expect unbound unbound no-go cp-platform-unbound
expect online online no-go modem-state-online
expect crashed crashed no-go modem-state-crashed
expect booting booting no-go modem-state-active
expect future-state future-state no-go modem-state-unrecognised
expect state-unreadable state-unreadable incomplete modem-state
expect state-missing state-missing no-go modem-state-missing
expect node-missing node-missing no-go oem_ipc7-missing
expect node-symlink node-symlink no-go umts_ipc0-symlink
expect node-block node-block no-go umts_rfs0-not-char
expect dev-mismatch dev-mismatch no-go umts_boot0-dev-mismatch
expect two-classes two-classes no-go umts_ipc1-ambiguous-class
expect no-class no-class no-go umts_ipc1-no-sysfs-class
expect subsystem subsystem no-go umts_ipc0-subsystem-mismatch
expect no-stat no-stat incomplete umts_boot0-dev
expect wwan wwan no-go wwan-surface-present

run physical-no-go online 0 physical
check "physical no-go: gate reports no-go" has 'e1_gate=no-go'

# Fail closed: malformed, ambiguous, or foreign evidence is refused.
refuse() {
	run "$1" "$2" "${4:-0}"
	check "$1: refused with exit $3" [ "$status" -eq "$3" ]
	check "$1: no E1 result" sh -c '! printf "%s\n" "$1" | grep -q "^e1_"' sh "$out"
}
refuse module-dup module-dup 5
refuse state-without-platform state-without-platform 5
refuse unknown-key unknown-key 5
check "unknown-key: value not echoed" sh -c '! printf "%s\n" "$1" | grep -Fq ABC123 && ! grep -rFq ABC123 "$2"' sh "$out" "$work/run-unknown-key"
refuse duplicate-key duplicate-key 5
refuse missing-key missing-key 5
refuse bad-value bad-value 5
refuse bad-rdev bad-rdev 5
refuse bad-kind bad-kind 5
refuse bad-count bad-count 5
refuse not-complete incomplete-run 5
refuse qemu qemu 4
refuse identity go 4 4
check "identity: refusal recorded" sh -c 'grep -Fxq result=refused "$1"/pixel-9a/s5300-cpif-inventory/inventory-*.txt' sh "$work/run-identity"
refuse guest-error go 3 1

mkdir -p "$work/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$work/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=3 FERRIX_PIXEL9A_S5300_CPIF_EVIDENCE=mock \
	sh "$inventory" 2>&1) || status=$?
silent_port=$FERRIX_SHELL
acm_guest_stop
check "unreachable: exit 3" [ "$status" -eq 3 ]
check "unreachable: sanitised transport reason" has 'REFUSING pixel-9a S5300 CPIF E1 inventory: guest CDC-ACM shell transport failed (no-shell-response)'
check "privacy: port path is neither printed nor retained" \
	sh -c '! printf "%s\n" "$1" | grep -Fq "$2" && ! grep -rFq "$2" "$3"' sh "$out" "$silent_port" "$work/run-unreachable"

mkdir -p "$work/run-bad-env"
status=0
FERRIX_OUT_DIR="$work/run-bad-env" FERRIX_PIXEL9A_S5300_CPIF_EVIDENCE=qemu sh "$inventory" >/dev/null 2>&1 || status=$?
check "usage: unknown evidence label is refused before any session" sh -c '[ "$1" -eq 2 ] && [ ! -d "$2/pixel-9a" ]' sh "$status" "$work/run-bad-env"

mock_gates=''
for file in "$work"/run-*/pixel-9a/s5300-cpif-inventory/inventory-*.txt; do
	[ -f "$file" ] || continue
	if grep -Fxq 'evidence=mock' "$file" &&
		grep -Eq '^e1_gate=(go|no-go|incomplete)$' "$file"; then
		mock_gates=yes
		break
	fi
done
check "no mock run ever reports a physical gate" [ -z "$mock_gates" ]

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-s5300-cpif-inventory-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-s5300-cpif-inventory-test","status":"passed"}\n'
