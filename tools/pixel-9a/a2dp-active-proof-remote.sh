# Guest body of the issue #91 bounded A2DP playback proof. It is never sent
# alone: a2dp-active-proof.sh composes one CDC-ACM probe from
#
#   proof_mode=dry|play, proof_salt=<32 hex>, proof_expect=<64 hex>|none
#   the pinned constants, expected_keys, validate, get, is_n and
#   preflight_problems, copied unchanged from a2dp-output-preflight.sh
#   preflight_body() { <a2dp-output-preflight-remote.sh, unchanged> }
#   this file
#
# so the read-only v3 preflight and its host-side blocker logic run in the
# same session, immediately before any tone. Both modes:
#
#   1. refuse unless this is the exact tegu device tree and no proof scratch
#      directory is left over, then create a private 0700 tmpfs scratch
#      directory (removed on every exit);
#   2. run the v3 preflight body and require a complete, valid,
#      candidate-observed result;
#   3. re-derive the sink identity independently (bluetoothctl and wpctl
#      query verbs only): exactly one paired, connected Audio Sink and exactly
#      one bluez A2DP sink whose node name and api.bluez5.address both name it,
#      with a non-zero object.serial; and hash it, salted by the host's secret
#      per-decision salt, with the kernel, package versions and WirePlumber
#      tree result. Only that salted hash leaves the guest.
#
# dry stops there and changes nothing else. play additionally requires the
# hash to equal proof_expect, and before the tone:
#
#   - arms the graph guard first, before step 2: `pw-mon -N -a -p` (pinned
#     pipewire-tools; line-buffered, one blank line after every event, every
#     object carries object.serial) feeds an awk guard through a FIFO. The
#     guard is armed once pw-mon reports its own client; that client's serial
#     S0 splits pre-existing objects from new ones. Every object created after
#     S0 is reported even if it is destroyed again, and a removal of an object
#     never reported trips the guard, so a transient route cannot pass
#     unobserved;
#   - requires that, since arming, the graph changed only by clients and by
#     state changes of the bound sink and its device (else refuse);
#   - starts two pollers, each armed only after one clean pass: open ALSA
#     playback PCMs (/proc/*/fd, the v3 opener-scan contract) and the two
#     speaker-path controls (amixer cget only);
#   - starts pw-cat blocked on opening its stdin FIFO, so its pid is known to
#     every guard before it can connect, and waits for the graph guard to
#     confirm enforcement for that pid.
#
# The tone: 72000 bytes of a generated 750 Hz sine (one 64-sample cycle table
# below), unsigned 8-bit mono 48 kHz, peak 13/128 (about -20 dBFS): 1.5 s.
# `head -c` bounds the bytes; a watchdog kills pw-cat after 4 s of wall time.
#
#   pw-cat --playback --raw --rate 48000 --channels 1 --format u8
#     --target <sink object.serial> --properties { node.name = <random>
#     target.object = <serial> node.dont-fallback node.dont-reconnect
#     node.dont-move = true node.linger state.restore-props
#     state.restore-target = false media.name = ferrix-a2dp-proof } -
#
# During the tone any guard kills pw-cat with SIGKILL and records why:
#   graph   any node other than our one Stream/Output/Audio node (so any new
#           sink, ALSA or not), any port not on our node, any link whose input
#           is not the bound sink or whose output is not our node, any device
#           or other object added, any change to an object other than
#           clients, our objects, the bound sink, its ports and its device,
#           removal of the bound sink or device (disconnect) or of any other
#           pre-existing object, an object never observed, or a line pw-mon
#           would not print;
#   pcm     any open playback PCM, or an incomplete scan;
#   ctl     either speaker-path control not reading `off`, or a failed read;
#   live    any guard process (awk, pw-mon, either poller) exiting while the
#           tone runs; pw-mon exiting is caught here.
#
# The armed, review and enforcing acknowledgements also show that the guard
# awk handles each line as it arrives: an awk that waited to fill an input
# buffer would never acknowledge, and the run would refuse.
#
# Afterwards it stops the guards, runs the v3 preflight body again (final
# observation) and removes the scratch directory. Nothing here pairs,
# connects, trusts, scans, selects a profile or default, writes a volume or
# mixer control, records audio, loads modules, starts or stops a service, or
# plays any file. No address, name, node name or serial is printed; the
# graph guard prints classes and counts only and no waveform is kept.
set -u

proof_dir=/run/ferrix-a2dp-proof
dt_root=/proc/device-tree
proc_root=/proc
runtime_dir=/run/user/1000
random_dev=/dev/urandom
card=0
audio_sink_uuid=0000110b-0000-1000-8000-00805f9b34fb
watched_controls='TDM_0_RX Mixer EP1|TAS25XX ASI Switch'
tone_bytes=72000
tone_cycles=1125
play_wall=4
arm_wait=50
# One cycle of 750 Hz at 48 kHz: 128 + round(13 sin(2 pi i / 64)), as octal.
tone_cycle='\200\201\203\204\205\206\207\210\211\212\213\213\214\214\215\215\215\215\215\214\214\213\213\212\211\210\207\206\205\204\203\201\200\177\175\174\173\172\171\170\167\166\165\165\164\164\163\163\163\163\163\164\164\165\165\166\167\170\171\172\173\174\175\177'

model=$(tr '\000' '\n' <"$dt_root/model" 2>/dev/null | sed -n '1p')
[ "$model" = 'ZUMA PRO TEGU MP on ZUMA PRO' ] || exit 4
tr '\000' '\n' <"$dt_root/compatible" 2>/dev/null | grep -Fxq 'google,ZUMA PRO TEGU' || exit 4

have() { command -v "$1" >/dev/null 2>&1; }
run_t() { if have timeout; then timeout "$@" </dev/null; else shift; "$@" </dev/null; fi; }
wp() { run_t 5 env XDG_RUNTIME_DIR="$runtime_dir" wpctl "$@"; }
out() { printf '%s=%s\n' "$1" "$2"; }
clean() { tr '_' '-' | tr -cd 'A-Za-z0-9.+-' | cut -c1-48; }
rand_hex() { od -An -N"$1" -tx1 "$random_dev" 2>/dev/null | tr -cd '0-9a-f'; }

printf 'active_probe=a2dp-active-proof\nactive_probe_version=1\n'
case "$proof_mode" in dry|play) out active_mode "$proof_mode" ;; *) out active_mode invalid; out active_complete refused; exit 6 ;; esac

mon_pid=''; guard_pid=''; player_pid=''; writer_pid=''; watchdog_pid=''; pcm_pid=''; ctl_pid=''; live_pid=''; fd3=no
cleanup() {
	for pid in $player_pid $writer_pid $watchdog_pid $live_pid; do kill -KILL "$pid" 2>/dev/null; done
	[ ! -d "$proof_dir" ] || : >"$proof_dir/stop" 2>/dev/null
	for pid in $pcm_pid $ctl_pid $mon_pid; do kill -TERM "$pid" 2>/dev/null; done
	[ "$fd3" = no ] || exec 3>&-
	fd3=no
	[ -z "$guard_pid" ] || { kill -TERM "$guard_pid" 2>/dev/null; wait "$guard_pid" 2>/dev/null; }
	for pid in $player_pid $writer_pid $watchdog_pid $live_pid $pcm_pid $ctl_pid $mon_pid; do wait "$pid" 2>/dev/null; done
	mon_pid=''; guard_pid=''; player_pid=''; writer_pid=''; watchdog_pid=''; pcm_pid=''; ctl_pid=''; live_pid=''
	rm -rf "$proof_dir"
}
# refuse REASON: nothing was played; a scratch directory this run made goes
# with the trap.
refuse() {
	out active_refusal "$1"
	out tone_started no
	out active_complete refused
	exit 6
}

for tool in pw-mon pw-cat mkfifo od head sha256sum awk; do
	have "$tool" || refuse "tool-missing-$tool"
done
sleep 0.01 2>/dev/null || refuse fractional-sleep-unsupported
[ ! -e "$proof_dir" ] && [ ! -L "$proof_dir" ] || refuse proof-dir-left-over
(umask 077 && mkdir "$proof_dir") 2>/dev/null || refuse proof-dir-create-failed
# Only a directory this run created is ever removed.
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
out scratch created

# --- the graph guard (play only), armed before the in-session preflight -------
# Input: pw-mon's events and FERRIXCTL <token> lines from this shell. Output:
# files in the scratch directory (armed, review, enforcing, final, trip.graph).
guard_awk='
function put(name, text,   f) { f = dir "/" name ".tmp"; printf "%s\n", text > f; close(f); system("mv -f " f " " dir "/" name) }
function trip(r) {
	if (!(r in tripped)) { tripped[r] = 1; trips = trips (trips == "" ? "" : ",") r; put("trip.graph", trips) }
	if (pid != "") system("kill -KILL " pid " 2>/dev/null")
}
function reset_block() { inblk = 0; hdr = ""; id = ""; type = ""; outn = ""; inn = ""; lstate = ""; bad = ""; split("", p) }
function arm_check() {
	if (!armed && monpid != "" && (monpid in client_serial)) { armed = 1; s0 = client_serial[monpid]; put("armed", "armed") }
}
function cls(c) { return (c ~ /Sink/) ? "sink" : "other" }
function evaluate(h, i, t, s, n, c, pn, o, in_, st,   k) {
	if (h == "added") {
		if (s + 0 <= s0 + 0) return
		if (t == "Client") return
		if (t == "Node") {
			if (n == own && c == "Stream/Output/Audio" && own_id == "") {
				own_id = i
				for (k in pend_link) if (pend_link[k] == i) { own_links++; delete pend_link[k]; ownobj[k] = 1 }
				for (k in pend_port) if (pend_port[k] == i) { delete pend_port[k]; ownobj[k] = 1 }
				return
			}
			trip(cls(c) == "sink" ? "new-sink-node" : "foreign-node"); return
		}
		if (t == "Port") {
			if (own_id != "" && pn == own_id) { ownobj[i] = 1; return }
			if (own_id == "" && pn != "" && !(pn in seen)) { pend_port[i] = pn; return }
			trip("foreign-port"); return
		}
		if (t == "Link") {
			if (in_ != sink) { trip("link-outside-bound-sink"); return }
			if (own_id != "" && o == own_id) { own_links++; ownobj[i] = 1; if (st == "active") active = 1; return }
			if (own_id == "" && o != "" && !(o in seen)) { pend_link[i] = o; if (st == "active") pend_active[i] = 1; return }
			trip("foreign-link-to-bound-sink"); return
		}
		trip(t == "Device" ? "device-added" : "graph-object-added"); return
	}
	if (h == "changed") {
		if (t == "Client" || i == sink || i == dev || (i in ownobj) || i == own_id) {
			if (t == "Link" && st == "active" && (i in ownobj)) active = 1
			return
		}
		if ((i in pend_link) || (i in pend_port)) { if (st == "active") pend_active[i] = 1; return }
		if (t == "Port" && pn == sink) return
		trip("graph-object-changed"); return
	}
	# removed
	if (t == "") { trip("unobserved-object-removed"); return }
	if (i == sink) { trip("bound-sink-removed"); return }
	if (i == dev) { trip("bound-device-removed"); return }
	if (t == "Client" || (i in ownobj) || i == own_id || (i in pend_link) || (i in pend_port)) return
	trip("graph-object-removed")
}
function end_block(   s, n, c, pn, t, k) {
	if (hdr == "core") { reset_block(); return }
	if (bad != "") { trip("graph-observation-malformed"); reset_block(); return }
	if (id == "") { trip("graph-observation-malformed"); reset_block(); return }
	if (hdr == "removed") {
		t = otype[id]
		if (phase == "observe") { qn++; qh[qn] = "removed"; qi[qn] = id; qt[qn] = t }
		else if (phase == "enforce") evaluate("removed", id, t)
		delete otype[id]; delete seen[id]
		reset_block(); return
	}
	s = p["object.serial"]
	if (type == "" || s !~ /^[0-9]+$/) { trip("graph-observation-malformed"); reset_block(); return }
	if (hdr == "changed" && !(id in seen)) { trip("unobserved-object-changed"); reset_block(); return }
	if (hdr == "added") { otype[id] = type; seen[id] = 1 }
	if (type == "Client" && p["pipewire.sec.pid"] ~ /^[0-9]+$/) { client_serial[p["pipewire.sec.pid"]] = s; arm_check() }
	if (type == "Port" && p["node.id"] != "") portnode[id] = p["node.id"]
	if (phase == "observe") {
		qn++; qh[qn] = hdr; qi[qn] = id; qt[qn] = type; qs[qn] = s; qnm[qn] = p["node.name"]; qc[qn] = p["media.class"]
		qp[qn] = p["node.id"]; qo[qn] = outn; qin[qn] = inn; qst[qn] = lstate
	} else if (phase == "enforce") {
		evaluate(hdr, id, type, s, p["node.name"], p["media.class"], p["node.id"], outn, inn, lstate)
	}
	reset_block()
}
function control(rest,   f, k) {
	split(rest, f, " ")
	if (f[1] == "monitor") { monpid = f[2]; arm_check() }
	else if (f[1] == "preflight") { if (!armed) trip("graph-guard-not-armed"); arm_q = qn }
	else if (f[1] == "bind") { sink = f[2]; dev = f[3]; own = f[4] }
	else if (f[1] == "review") {
		if (!armed) trip("graph-guard-not-armed")
		phase = "enforce"
		# Events before the in-session preflight started are part of what it
		# observed; only later ones are changes.
		for (k = arm_q + 1; k <= qn; k++) evaluate(qh[k], qi[k], qt[k], qs[k], qnm[k], qc[k], qp[k], qo[k], qin[k], qst[k])
		put("review", trips == "" ? "ok" : trips)
	}
	else if (f[1] == "play") { pid = f[2]; if (trips != "") trip("tripped-before-tone"); put("enforcing", pid) }
	else if (f[1] == "stop") {
		for (k in pend_link) { trip("unresolved-link"); break }
		for (k in pend_port) { trip("unresolved-port"); break }
		phase = "stopped"
		put("final", (own_id != "" ? "yes" : "no") " " (own_links + 0) " " (active ? "yes" : "no") " " (trips == "" ? "none" : trips))
	}
	else trip("graph-control-malformed")
}
BEGIN { phase = "observe"; reset_block(); s0 = -1; arm_q = 0 }
{
	line = $0
	if (index(line, "FERRIXCTL " tok " ") == 1) { control(substr(line, length(tok) + 12)); next }
	if (phase == "stopped") next
	if (line == "") { if (inblk) end_block(); next }
	if (line == "added:" || line == "changed:" || line == "removed:") {
		if (inblk) end_block()
		inblk = 1; hdr = substr(line, 1, length(line) - 1); next
	}
	if (!inblk) {
		if (line ~ /^\ttype: PipeWire:Interface:Core/) { inblk = 1; hdr = "core"; next }
		trip("graph-observation-malformed"); next
	}
	if (hdr == "core") next
	if (line ~ /^\tid: [0-9]+$/) { if (id != "") bad = "id"; id = substr(line, 6); next }
	if (line ~ /^\ttype: PipeWire:Interface:[A-Za-z]+ \(version [0-9]+\)$/) {
		if (type != "") bad = "type"
		type = substr(line, 27); sub(/ .*/, "", type); next
	}
	if (line ~ /^\toutput-node-id: [0-9]+$/) { outn = substr(line, 18); next }
	if (line ~ /^\tinput-node-id: [0-9]+$/) { inn = substr(line, 17); next }
	if (line ~ /^[ *]\tstate: "[a-z]+"/) { lstate = substr(line, 11); sub(/".*/, "", lstate); next }
	if (line ~ /^[ *]\t\t[^ \t=]+ = /) {
		k = substr(line, 4); sub(/ = .*/, "", k)
		if (k == "object.serial" || k == "node.name" || k == "media.class" || k == "node.id" || k == "pipewire.sec.pid") {
			v = line; sub(/^[ *]\t\t[^ \t=]+ = "/, "", v); sub(/"$/, "", v)
			if (k in p) bad = "dup"
			p[k] = v
		}
		next
	}
	if (line ~ /^[ *\t]/) next
	bad = "shape"
}
END { if (phase != "stopped") trip("graph-observer-ended") }
'

# guard_trip NAME REASON: record the first reason, then kill the player if it
# exists. Writing the record before reading player.pid, and the shell writing
# player.pid before checking records, means no trip can miss both.
guard_trip() {
	[ -e "$proof_dir/trip.$1" ] || printf '%s\n' "$2" >"$proof_dir/trip.$1"
	p=$(cat "$proof_dir/player.pid" 2>/dev/null)
	[ -z "$p" ] || kill -KILL "$p" 2>/dev/null
	exit 0
}
# pcm_scan: "complete N" or "incomplete", by the v3 opener-scan contract.
pcm_scan() {
	procs=0
	for fd_dir in "$proc_root"/[0-9]*/fd; do [ -d "$fd_dir" ] && procs=$((procs + 1)); done
	scan=$(run_t 5 ls -l "$proc_root"/[0-9]*/fd/ 2>&1)
	rc=$?
	set -- $(printf '%s\n' "$scan" | awk '
		/^ls: / { if ($0 !~ /No such file or directory$/) other++; next }
		/^l[-r][-w][-xsS]/ && / -> / {
			t = $0; sub(/^.* -> /, "", t); sub(/ \(deleted\)$/, "", t); m = split(t, part, "/")
			if (part[m] ~ /^pcmC[0-9]+D[0-9]+p$/) hits++
			next
		}
		/^total [0-9]+$/ || /^$/ || /\/fd\/?:$/ { next }
		{ other++ }
		END { print hits + 0, other + 0 }')
	case "$rc" in 124|126|127|137|143) echo incomplete; return ;; esac
	[ "${2:-1}" = 0 ] && [ "$procs" -gt 0 ] || { echo incomplete; return; }
	echo "complete $1"
}
control_values() {
	old_ifs=$IFS
	IFS='|'
	for control in $watched_controls; do
		IFS=$old_ifs
		got=$(run_t 5 amixer -c "$card" cget "iface=MIXER,name='$control'" 2>/dev/null) || { echo failed; continue; }
		v=$(printf '%s\n' "$got" | sed -n 's/^[[:space:]]*: values=//p' | sed -n '1p')
		echo "${v:-failed}"
	done
	IFS=$old_ifs
}
pcm_guard() {
	first=1
	while [ ! -e "$proof_dir/stop" ]; do
		case "$(pcm_scan)" in
			'complete 0') ;;
			complete*) guard_trip pcm playback-pcm-opener ;;
			*) guard_trip pcm pcm-scan-incomplete ;;
		esac
		[ "$first" = 0 ] || { echo armed >"$proof_dir/armed.pcm"; first=0; }
		sleep 0.05
	done
}
ctl_guard() {
	first=1
	while [ ! -e "$proof_dir/stop" ]; do
		for v in $(control_values); do
			case "$v" in off) ;; failed) guard_trip ctl speaker-control-read-failed ;; *) guard_trip ctl speaker-control-not-off ;; esac
		done
		[ "$first" = 0 ] || { echo armed >"$proof_dir/armed.ctl"; first=0; }
		sleep 0.05
	done
}
# wait_file NAME: up to arm_wait tenths of a second for a non-empty guard file.
wait_file() {
	n=0
	while [ ! -s "$proof_dir/$1" ]; do
		n=$((n + 1))
		[ "$n" -le "$arm_wait" ] || return 1
		sleep 0.1
	done
}
any_trip() { for f in "$proof_dir"/trip.*; do [ -e "$f" ] && return 0; done; return 1; }
trip_of() { if [ -s "$proof_dir/trip.$1" ]; then sed -n '1p' "$proof_dir/trip.$1" | tr -cd 'a-z0-9,-'; else echo none; fi; }

ctl_tok=$(rand_hex 8)
own_tok=$(rand_hex 8)
[ "${#ctl_tok}" -eq 16 ] && [ "${#own_tok}" -eq 16 ] || refuse random-source-unavailable
own_name="ferrix-a2dp-proof-$own_tok"
if [ "$proof_mode" = play ]; then
	mkfifo "$proof_dir/graph.fifo" "$proof_dir/tone.fifo" 2>/dev/null || refuse fifo-create-failed
	awk -v tok="$ctl_tok" -v dir="$proof_dir" "$guard_awk" <"$proof_dir/graph.fifo" &
	guard_pid=$!
	exec 3>"$proof_dir/graph.fifo"
	fd3=yes
	env XDG_RUNTIME_DIR="$runtime_dir" pw-mon -N -a -p >"$proof_dir/graph.fifo" 2>/dev/null </dev/null &
	mon_pid=$!
	printf 'FERRIXCTL %s monitor %s\n' "$ctl_tok" "$mon_pid" >&3
	if wait_file armed; then out guard_armed yes; else out guard_armed no; refuse graph-guard-not-armed; fi
	printf 'FERRIXCTL %s preflight\n' "$ctl_tok" >&3
fi

# --- the read-only v3 preflight, in this session ------------------------------
(preflight_body) >"$proof_dir/pre.out" 2>/dev/null
pre_exit=$?
out active_section pre-begin
cat "$proof_dir/pre.out"
out active_section pre-end
out preflight_exit "$pre_exit"
[ "$pre_exit" -eq 0 ] || refuse preflight-failed
validate "$proof_dir/pre.out" >/dev/null 2>&1 || refuse preflight-output-invalid
problems=$(preflight_problems "$proof_dir/pre.out")
[ -z "$problems" ] || refuse "preflight-blocked"
out preflight candidate-observed

# --- independent sink identity -------------------------------------------------
hex2='[0-9A-F][0-9A-F]'
mac_re="^$hex2:$hex2:$hex2:$hex2:$hex2:$hex2\$"
paired=$(run_t 10 bluetoothctl devices Paired 2>/dev/null) || refuse identity-paired-query-failed
connected=''
for address in $(printf '%s\n' "$paired" | awk -v re="$mac_re" '$1 == "Device" && toupper($2) ~ re { print toupper($2) }' | sort -u); do
	info=$(run_t 10 bluetoothctl info "$address" 2>/dev/null) || refuse identity-info-failed
	case "$(printf '%s\n' "$info" | awk -v uuid="$audio_sink_uuid" '
		$1 == "Paired:" { p = $2 } $1 == "Connected:" { c = $2 }
		$1 == "UUID:" && index(tolower($0), "(" uuid ")") { s = 1 }
		END { printf "%s %s %s\n", p, c, s ? "sink" : "none" }')" in
		'yes yes sink') connected="$connected $address" ;;
	esac
done
set -- $connected
[ "$#" -eq 1 ] || refuse identity-connected-sink-not-one
id_addr=$1
status_text=$(wp status 2>/dev/null) || refuse identity-graph-unreachable
sink_ids=$(printf '%s\n' "$status_text" | awk '
	/^Audio/ { audio = 1; next } /^[A-Z]/ { audio = 0 } !audio { next }
	/^[^0-9A-Za-z]*Sinks:[[:space:]]*$/ { s = 1; next }
	/^[^0-9A-Za-z]*[A-Za-z][A-Za-z ]*:[[:space:]]*$/ { s = 0; next }
	s && match($0, /[0-9]+\. /) { print substr($0, RSTART, RLENGTH - 2) }')
found=0
for sid in $sink_ids; do
	props=$(wp inspect "$sid" 2>/dev/null | awk '
		{ line = $0; sub(/^[[:space:]*]+/, "", line) }
		match(line, /^[A-Za-z0-9._-]+ = "/) {
			k = substr(line, 1, RLENGTH - 4); v = substr(line, RLENGTH + 1); sub(/"[[:space:]]*$/, "", v)
			if (k == "media.class" || k == "node.name" || k == "device.api" || k == "api.bluez5.profile" ||
				k == "api.bluez5.address" || k == "api.bluez5.codec" || k == "device.id" || k == "object.serial") {
				if (k in seen) dup = 1
				seen[k] = 1; out = out k "=" v "\n"
			}
		}
		END { if (!dup) printf "%s", out }')
	prop() { printf '%s\n' "$props" | sed -n "s/^$1=//p" | sed -n '1p'; }
	[ -n "$props" ] || refuse identity-sink-inspect-failed
	case "$(prop media.class):$(prop device.api):$(prop node.name):$(prop api.bluez5.profile)" in
		Audio/Sink:bluez5:bluez_output.*:a2dp-sink*) ;;
		*) continue ;;
	esac
	found=$((found + 1))
	from_name=$(prop node.name | sed -n 's/^bluez_output\.\([0-9A-Fa-f_]\{17\}\)\..*$/\1/p' | tr '_a-f' ':A-F')
	from_prop=$(prop api.bluez5.address | tr 'a-f' 'A-F')
	[ "$from_name" = "$id_addr" ] && [ "$from_prop" = "$id_addr" ] || refuse identity-sink-device-mismatch
	id_sink=$sid
	id_serial=$(prop object.serial)
	id_dev=$(prop device.id)
	id_name=$(prop node.name)
	id_profile=$(prop api.bluez5.profile)
	id_codec=$(prop api.bluez5.codec)
done
[ "$found" -eq 1 ] || refuse identity-a2dp-sink-not-one
case "$id_serial" in ''|0|*[!0-9]*) refuse identity-sink-serial-invalid ;; esac
case "$id_dev" in ''|*[!0-9]*) refuse identity-sink-device-invalid ;; esac
pre_get() { get "$1" "$proof_dir/pre.out"; }
binding=$({
	printf 'ferrix-a2dp-active-proof-binding-v1\nsalt=%s\ntarget=google-pixel-9a-tegu-gxq96-mp1-0\n' "$proof_salt"
	for key in kernel_release pkg.bluez pkg.pipewire pkg.wireplumber pkg.pipewire_spa_bluez pkg.pipewire_tools \
		wireplumber_data_tree a2dp_sink_codec; do
		printf '%s=%s\n' "$key" "$(pre_get "$key")"
	done
	printf 'address=%s\nserial=%s\ndevice=%s\nnode=%s\nprofile=%s\ncodec=%s\n' \
		"$id_addr" "$id_serial" "$id_dev" "$id_name" "$id_profile" "$id_codec"
} | sha256sum | cut -c1-64)
printf '%s\n' "$binding" | grep -Eqx '[0-9a-f]{64}' || refuse binding-unavailable
out identity ok
out sink_profile_class "$(printf '%s' "$id_profile" | clean)"
out sink_codec "$(printf '%s' "$id_codec" | clean)"

if [ "$proof_mode" = dry ]; then
	out binding "$binding"
	out tone_started no
	cleanup
	out scratch_removed "$([ ! -e "$proof_dir" ] && echo yes || echo no)"
	out active_complete yes
	exit 0
fi

# --- play: same-session gates ---------------------------------------------------
[ "$binding" = "$proof_expect" ] || refuse binding-mismatch
out binding_match yes
printf 'FERRIXCTL %s bind %s %s %s\n' "$ctl_tok" "$id_sink" "$id_dev" "$own_name" >&3
(pcm_guard) &
pcm_pid=$!
(ctl_guard) &
ctl_pid=$!
wait_file armed.pcm && wait_file armed.ctl || { any_trip || refuse kernel-guards-not-armed; }
any_trip && refuse "kernel-guard-tripped-before-tone"
out kernel_guards_armed yes
printf 'FERRIXCTL %s review\n' "$ctl_tok" >&3
wait_file review || refuse graph-review-timeout
review=$(sed -n '1p' "$proof_dir/review" | tr -cd 'a-z0-9,-')
out guard_review "$review"
[ "$review" = ok ] || refuse graph-changed-before-tone

# pw-cat blocks opening its stdin FIFO until the writer starts, so the guards
# know its pid before it can connect to PipeWire.
props="{ node.name = \"$own_name\" target.object = \"$id_serial\" node.dont-fallback = true node.dont-reconnect = true node.dont-move = true node.linger = false state.restore-props = false state.restore-target = false media.name = \"ferrix-a2dp-proof\" }"
env XDG_RUNTIME_DIR="$runtime_dir" pw-cat --playback --raw --rate 48000 --channels 1 --format u8 \
	--target "$id_serial" --properties "$props" - <"$proof_dir/tone.fifo" >/dev/null 2>&1 &
player_pid=$!
printf '%s\n' "$player_pid" >"$proof_dir/player.pid.tmp" && mv -f "$proof_dir/player.pid.tmp" "$proof_dir/player.pid"
printf 'FERRIXCTL %s play %s\n' "$ctl_tok" "$player_pid" >&3
if ! wait_file enforcing || [ "$(sed -n '1p' "$proof_dir/enforcing")" != "$player_pid" ] || any_trip; then
	kill -KILL "$player_pid" 2>/dev/null
	refuse guard-not-enforcing-before-tone
fi
(
	sleep "$play_wall"
	kill -KILL "$player_pid" 2>/dev/null && : >"$proof_dir/watchdog.fired"
) &
watchdog_pid=$!
(
	while kill -0 "$player_pid" 2>/dev/null; do
		for pid in $guard_pid $mon_pid $pcm_pid $ctl_pid; do
			kill -0 "$pid" 2>/dev/null || { [ -e "$proof_dir/trip.live" ] || echo guard-exited >"$proof_dir/trip.live"; kill -KILL "$player_pid" 2>/dev/null; exit 0; }
		done
		sleep 0.05
	done
) &
live_pid=$!
out tone_started yes
(
	i=0
	while [ "$i" -lt "$tone_cycles" ]; do printf "$tone_cycle"; i=$((i + 1)); done
) 2>/dev/null | head -c "$tone_bytes" >"$proof_dir/tone.fifo" 2>/dev/null &
writer_pid=$!
wait "$player_pid"
tone_exit=$?
player_pid=''
kill -KILL "$writer_pid" "$watchdog_pid" 2>/dev/null
wait "$writer_pid" 2>/dev/null
wait "$watchdog_pid" 2>/dev/null
writer_pid=''; watchdog_pid=''
# Removal events for our stream arrive after pw-cat exits; keep enforcing.
sleep 0.5
: >"$proof_dir/stop"
printf 'FERRIXCTL %s stop\n' "$ctl_tok" >&3
wait_file final || :
final=$(sed -n '1p' "$proof_dir/final" 2>/dev/null)
kill -KILL "$live_pid" 2>/dev/null; wait "$live_pid" 2>/dev/null; live_pid=''
for pid in $pcm_pid $ctl_pid; do wait "$pid" 2>/dev/null; done
pcm_pid=''; ctl_pid=''
kill -TERM "$mon_pid" 2>/dev/null; wait "$mon_pid" 2>/dev/null; mon_pid=''
exec 3>&-
fd3=no
wait "$guard_pid" 2>/dev/null; guard_pid=''
set -- $final
out tone_exit "$tone_exit"
out tone_watchdog "$([ -e "$proof_dir/watchdog.fired" ] && echo fired || echo idle)"
out graph_final "$([ "$#" -eq 4 ] && echo complete || echo missing)"
out graph_own_node "${1:-unknown}"
out graph_own_links "${2:-unknown}"
out graph_link_active "${3:-unknown}"
out guard_trip_graph "$(trip_of graph)"
out guard_trip_pcm "$(trip_of pcm)"
out guard_trip_ctl "$(trip_of ctl)"
out guard_trip_live "$(trip_of live)"

# --- final observation: the read-only v3 preflight again ------------------------
(preflight_body) >"$proof_dir/post.out" 2>/dev/null
out post_exit "$?"
out active_section post-begin
cat "$proof_dir/post.out"
out active_section post-end
cleanup
out scratch_removed "$([ ! -e "$proof_dir" ] && echo yes || echo no)"
out active_complete yes
