# Sourced by the Pixel 9a CDC-ACM workflow tests; never contacts a device.
#
#   acm_guest_init ROOT WORK PROBE
#                               write guest-side stubs under WORK/acm-guest;
#                               PROBE (ACM_GUEST_PROBE) is the only file the
#                               stub accepts and may be changed before a start
#   acm_guest_start [--silent]  start a fresh fake guest; exports FERRIX_SHELL
#   acm_guest_stop              stop it
#   acm_guest_ctl NAME VALUE    set a per-case control: fixture, exit, sleep,
#                               flood (bytes), real (run the decoded probe
#                               with the host /bin/sh)
#
# The fake guest (acm-fake-guest.py) runs a real interactive /bin/sh on a pty
# bridged to the port that tools/pixel-9a/acm-probe.py opens, so the
# bootstrap, base64 transfer, guest-side SHA-256 check, framing and exit
# status all go through a real shell and tty. Only the final `sh` that runs
# the decoded probe is a stub: it records the exact bytes it received,
# refuses anything but PROBE, and answers from the case controls.

acm_guest_init() {
	acm_root=$1
	acm_guest_dir="$2/acm-guest"
	ACM_GUEST_CTL="$acm_guest_dir/ctl"
	ACM_GUEST_PROBE=$3
	mkdir -p "$acm_guest_dir/bin" "$ACM_GUEST_CTL"
	cat >"$acm_guest_dir/bin/sh" <<'STUB'
#!/bin/sh
# Stand-in for the guest `sh` that receives the decoded probe on stdin.
ctl=$ACM_GUEST_CTL
cat >"$ctl/stdin"
printf 'run\n' >>"$ctl/calls"
cmp -s "$ctl/stdin" "$ACM_GUEST_PROBE" || { echo 'stub: probe bytes differ'; exit 97; }
[ ! -f "$ctl/sleep" ] || sleep "$(cat "$ctl/sleep")"
[ ! -f "$ctl/flood" ] || awk -v n="$(cat "$ctl/flood")" \
	'BEGIN { line = sprintf("%063d", 0); for (i = 0; i < n; i += 64) print line }'
[ ! -f "$ctl/real" ] || exec /bin/sh "$ctl/stdin"
[ ! -f "$ctl/fixture" ] || cat "$(cat "$ctl/fixture")"
exit "$(cat "$ctl/exit" 2>/dev/null || echo 0)"
STUB
	chmod 0755 "$acm_guest_dir/bin/sh"
	# Busybox provides sha256sum on the guest; some hosts only have shasum.
	if ! command -v sha256sum >/dev/null 2>&1; then
		printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' >"$acm_guest_dir/bin/sha256sum"
		chmod 0755 "$acm_guest_dir/bin/sha256sum"
	fi
	export ACM_GUEST_CTL ACM_GUEST_PROBE
	acm_guest_pid=''
}

acm_guest_ctl() {
	printf '%s\n' "$2" >"$ACM_GUEST_CTL/$1"
}

acm_guest_start() {
	acm_guest_stop
	rm -f "$ACM_GUEST_CTL"/*
	: >"$ACM_GUEST_CTL/calls"
	export ACM_GUEST_PROBE
	port_file="$acm_guest_dir/port"
	rm -f "$port_file"
	PATH="$acm_guest_dir/bin:$PATH" python3 "$acm_root/tools/pixel-9a/tests/acm-fake-guest.py" \
		"$port_file" "$@" </dev/null >/dev/null 2>&1 &
	acm_guest_pid=$!
	tries=0
	while [ ! -s "$port_file" ]; do
		tries=$((tries + 1))
		[ "$tries" -le 100 ] || { echo 'fake guest did not start' >&2; return 1; }
		sleep 0.1
	done
	FERRIX_SHELL=$(cat "$port_file")
	export FERRIX_SHELL
	# Let the interactive shell print its first prompt.
	sleep 0.3
}

acm_guest_stop() {
	[ -n "${acm_guest_pid:-}" ] || return 0
	kill "$acm_guest_pid" 2>/dev/null || :
	wait "$acm_guest_pid" 2>/dev/null || :
	acm_guest_pid=''
}

acm_guest_runs() {
	wc -l <"$ACM_GUEST_CTL/calls" | tr -d ' '
}
