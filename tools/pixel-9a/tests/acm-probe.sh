#!/bin/sh
# Offline proof for the Pixel 9a CDC-ACM guest probe transport
# (tools/pixel-9a/acm-probe.py). A fake guest bridges a pty to a real
# interactive /bin/sh, so the bootstrap, transfer, guest-side hash check,
# framing and exit status are exercised end to end without a device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
transport="$root/tools/pixel-9a/acm-probe.py"
s5300_probe="$root/tools/pixel-9a/s5300-source-inventory-remote.sh"
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

command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for the CDC-ACM transport test' >&2; exit 1; }
check "static: transport compiles" python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read())' "$transport"
check "static: fake guest compiles" python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read())' "$root/tools/pixel-9a/tests/acm-fake-guest.py"
offending=$(grep -nwE 'adb|fastboot|mount|umount|dd|mkfs|flash|erase|format|reboot|sideload|unlock|subprocess|system|popen' "$transport" |
	grep -vE '^[0-9]+:[[:space:]]*#' || true)
check "static: transport has no device, storage, or process-spawn verbs" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: transport never creates or writes a file" \
	sh -c '! grep -nE "O_CREAT|O_TRUNC|O_WRONLY|O_APPEND|open\\([^)]*\"[wax+]" "$1"' sh "$transport"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-acm-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; rm -rf "$work"' EXIT

# A probe with quoting, expansion and control-looking text, a forged marker
# line, a 1500-byte line, UTF-8 and no trailing newline: every byte must
# arrive unchanged and nothing in it may be interpreted by the guest shell.
tricky="$work/tricky.sh"
{
	cat <<'PROBE'
#!/bin/sh
# 'single' "double" $HOME `id` $(id) \back\slash %s %n ^C ^D
# FERRIXACM:00000000000000000000000000000000:END:0
printf 'line one\n'
printf 'FERRIXACM:forged:END:0\n'
printf 'stderr line\n' >&2
printf 'no trailing newline'
PROBE
	printf '# tab\there caf\303\251 \003 \004 \025\n'
	printf '# %s\n' "$(awk 'BEGIN { for (i = 0; i < 1500; i++) printf "x" }')"
	printf 'exit 7'
} >"$tricky"
printf 'line one\nFERRIXACM:forged:END:0\nstderr line\nno trailing newline' >"$work/expect-merge"
printf 'line one\nFERRIXACM:forged:END:0\nno trailing newline' >"$work/expect-discard"

acm_guest_init "$root" "$work" "$tricky"

status=0
transport_run() {
	status=0
	python3 "$transport" "$@" >"$work/stdout" 2>"$work/stderr" || status=$?
}
reason_is() { grep -Fxq "ferrix-acm: transport=cdc-acm status=failed reason=$1" "$work/stderr"; }

acm_guest_start
acm_guest_ctl real yes
transport_run --probe "$tricky" --timeout 30 --max-bytes 65536 --stderr merge
check "exact: guest received the probe bytes unchanged" cmp -s "$ACM_GUEST_CTL/stdin" "$tricky"
check "exact: probe ran exactly once" [ "$(acm_guest_runs)" -eq 1 ]
check "status: guest exit status 7 is returned" [ "$status" -eq 7 ]
check "framing: stdout is exactly the framed probe output" cmp -s "$work/stdout" "$work/expect-merge"
check "framing: forged marker in probe output is plain data" grep -Fq 'FERRIXACM:forged:END:0' "$work/stdout"
check "privacy: echo and prompt are not in stdout" sh -c '! grep -Eq "fake-guest-host|stty|base64|sha256sum" "$1"' sh "$work/stdout"
check "success: transport writes nothing to stderr" [ ! -s "$work/stderr" ]

acm_guest_start
acm_guest_ctl real yes
transport_run --probe "$tricky" --timeout 30 --max-bytes 65536 --stderr discard
check "stderr: discard mode drops probe stderr only" cmp -s "$work/stdout" "$work/expect-discard"

# The real S5300 probe keeps its device-tree gate: on this host it refuses.
ACM_GUEST_PROBE=$s5300_probe
acm_guest_start
acm_guest_ctl real yes
transport_run --probe "$s5300_probe" --timeout 30 --max-bytes 65536 --stderr discard
check "refusal: real probe's device-tree gate exit 4 is returned" [ "$status" -eq 4 ]
check "refusal: no inventory field crosses the transport" [ ! -s "$work/stdout" ]
ACM_GUEST_PROBE=$tricky

acm_guest_start
acm_guest_ctl sleep 30
transport_run --probe "$tricky" --timeout 3 --max-bytes 65536
check "timeout: wedged probe fails as transport 255" [ "$status" -eq 255 ]
check "timeout: reason is timeout" reason_is timeout
check "timeout: no partial output is emitted" [ ! -s "$work/stdout" ]

acm_guest_start
acm_guest_ctl flood 200000
transport_run --probe "$tricky" --timeout 30 --max-bytes 4096
check "truncation: oversized output fails as transport 255" [ "$status" -eq 255 ]
check "truncation: reason is output-limit" reason_is output-limit
check "truncation: no partial output is emitted" [ ! -s "$work/stdout" ]

# The guest decodes something else: the hash gate refuses before running it.
acm_guest_start
real_base64=$(command -v base64)
printf '#!/bin/sh\n"%s" "$@"\nprintf "#"\n' "$real_base64" >"$work/acm-guest/bin/base64"
chmod 0755 "$work/acm-guest/bin/base64"
transport_run --probe "$tricky" --timeout 30 --max-bytes 65536
rm -f "$work/acm-guest/bin/base64"
check "hash: altered transfer fails as transport 255" [ "$status" -eq 255 ]
check "hash: reason is hash-mismatch" reason_is hash-mismatch
check "hash: altered probe never ran" [ "$(acm_guest_runs)" -eq 0 ]

acm_guest_start --silent
transport_run --probe "$tricky" --timeout 3 --max-bytes 65536
check "silent: no shell response fails as transport 255" [ "$status" -eq 255 ]
check "silent: reason is no-shell-response" reason_is no-shell-response
silent_port=$FERRIX_SHELL
check "privacy: port path never appears in errors" sh -c '! grep -Fq "$1" "$2" "$3"' sh "$silent_port" "$work/stderr" "$work/stdout"
acm_guest_stop

FERRIX_SHELL="$work/secret-port-ferrix_tegu-SERIAL42"
transport_run --probe "$tricky" --timeout 5 --max-bytes 65536
check "port: absent port fails as transport 255" sh -c '[ "$1" -eq 255 ] && grep -Fq reason=port-absent "$2"' sh "$status" "$work/stderr"
: >"$FERRIX_SHELL"
transport_run --probe "$tricky" --timeout 5 --max-bytes 65536
check "port: regular file is refused" reason_is port-not-character-device
check "privacy: refused port name and serial are not printed" sh -c '! grep -Eq "secret-port|SERIAL42" "$1" "$2"' sh "$work/stderr" "$work/stdout"

ln -s "$tricky" "$work/linked-probe"
transport_run --probe "$work/linked-probe" --timeout 5 --max-bytes 65536
check "probe: linked probe is refused" reason_is probe-not-regular-file
printf 'a\000b' >"$work/nul-probe"
transport_run --probe "$work/nul-probe" --timeout 5 --max-bytes 65536
check "probe: NUL bytes are refused, never altered" reason_is probe-not-text
transport_run --probe "$tricky" --timeout 0 --max-bytes 65536
check "usage: out-of-range timeout is refused" reason_is usage

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-acm-probe-test","evidence":"mock","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-acm-probe-test","evidence":"mock","status":"passed"}\n'
