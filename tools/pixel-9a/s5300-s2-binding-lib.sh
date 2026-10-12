# Sourced by s5300-s2-preflight.sh and s5300-s2-binding.sh (issue #108).
# Host side only; never sent to the guest and never contacts a device.
#
# Exact target google-pixel-9a-tegu-gxq96-mp1-0. This file owns the binding
# salt, the record schema and seal, and the host pins a decision binding is
# checked against (s5300-s2-preflight.md, "Decision binding").
#
# The salt is 32 random bytes, hex, in the ignored
#   ${FERRIX_OUT_DIR:-out}/pixel-9a/s5300-s2-binding/salt   (dir 0700, file 0600)
# created once on first use and never printed, logged, put in Git, or passed
# as a command-line argument. Every unit and boot identity is a salted
# SHA-256 prefix computed in the guest, so a hash is meaningful only on the
# host that holds this salt. Rotating (deleting) the salt invalidates every
# record and changes every identity hash.

s2b_record_keys='record record_version record_id target_id evidence preflight_result preflight_gate preflight_phase unit_identity unit_identity_source boot_identity binding_salt_id kernel_release slot_suffix library_sha256 probe_body_sha256 composed_probe_sha256 package_pins_sha256 mounts_service_sha256 pkg.device-google-tegu pkg.exynos-modem pkg.modemmanager-samsung-sit factory_archive_sha256 host_recovery_factory_archive preflight_transcript preflight_transcript_sha256 created_epoch expires_epoch single_use cp_boot_authorized record_mac'
s2b_lifetime=600
s2b_transcript_marker='## decision binding (single use; s5300-s2-preflight.md)'

# s2b_init ROOT: state paths under the ignored out/ tree.
s2b_init() {
	s2b_root=$1
	s2b_dir="${FERRIX_OUT_DIR:-$1/out}/pixel-9a/s5300-s2-binding"
	s2b_salt_file="$s2b_dir/salt"
	s2b_ledger="$s2b_dir/consumed-ids"
	s2b_salt=''
	s2b_reason=''
}

s2b_sha_file() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }
s2b_sha_stdin() {
	if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi | cut -c1-64
}
s2b_mode() { stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1" 2>/dev/null; }
s2b_owner() { stat -c %u "$1" 2>/dev/null || stat -f %u "$1" 2>/dev/null; }
# s2b_private PATH MODE: not a link, owned by this user, exactly MODE.
s2b_private() {
	[ ! -L "$1" ] && [ "$(s2b_owner "$1")" = "$(id -u)" ] && [ "$(s2b_mode "$1")" = "$2" ]
}
s2b_hex() {
	case "$1" in ''|*[!0-9a-f]*) return 1 ;; esac
	[ "${#1}" -eq "$2" ]
}

# s2b_salt_load yes|no: validate the salt (creating it first with `yes`) and
# keep it in s2b_salt. On failure s2b_reason names the refusal. A salt with
# the wrong owner or mode is refused, never repaired.
s2b_salt_load() {
	if [ ! -e "$s2b_dir" ] && [ ! -L "$s2b_dir" ]; then
		[ "$1" = yes ] || { s2b_reason=salt-missing; return 1; }
		(umask 077 && mkdir -p "$s2b_dir") || { s2b_reason=salt-dir-create-failed; return 1; }
	fi
	{ [ -d "$s2b_dir" ] && s2b_private "$s2b_dir" 700; } || { s2b_reason=salt-dir-not-private; return 1; }
	if [ ! -e "$s2b_salt_file" ] && [ ! -L "$s2b_salt_file" ]; then
		[ "$1" = yes ] || { s2b_reason=salt-missing; return 1; }
		python3 -c 'import os, secrets, sys
fd = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
os.write(fd, (secrets.token_hex(32) + "\n").encode("ascii"))
os.close(fd)' "$s2b_salt_file" 2>/dev/null || { s2b_reason=salt-create-failed; return 1; }
	fi
	{ [ -f "$s2b_salt_file" ] && s2b_private "$s2b_salt_file" 600; } || { s2b_reason=salt-not-private; return 1; }
	[ "$(wc -c <"$s2b_salt_file" | tr -d ' ')" -eq 65 ] || { s2b_reason=salt-malformed; return 1; }
	s2b_salt=$(cat "$s2b_salt_file")
	s2b_hex "$s2b_salt" 64 || { s2b_salt=''; s2b_reason=salt-malformed; return 1; }
}

# Public, one-way fingerprint of the salt so a record names the salt it was
# made with. 64 bits of a SHA-256 over a 256-bit secret reveal nothing usable.
s2b_salt_id() { printf 'ferrix-s5300-s2-salt-id-v1\n%s\n' "$s2b_salt" | s2b_sha_stdin | cut -c1-16; }

# s2b_mac FILE: HMAC-SHA-256 of FILE keyed by the salt file (read by python3
# from disk, so the key is never an argument).
s2b_mac() {
	python3 -c 'import hashlib, hmac, sys
key = b"ferrix-s5300-s2-record-mac-v1:" + open(sys.argv[1], "rb").read().strip()
sys.stdout.write(hmac.new(key, open(sys.argv[2], "rb").read(), hashlib.sha256).hexdigest() + "\n")' "$s2b_salt_file" "$1"
}
s2b_mac_equal() {
	python3 -c 'import hmac, sys; sys.exit(0 if hmac.compare_digest(sys.argv[1], sys.argv[2]) else 1)' "$1" "$2"
}

# SHA-256 of a preflight transcript as it stood when its result was emitted:
# every line before the decision-binding section.
s2b_transcript_sha() {
	awk -v m="$s2b_transcript_marker" '$0 == m { exit } { print }' "$1" | s2b_sha_stdin
}

# s2b_host_pins ROOT: the checked-in inputs a record is bound to. Sets
# s2b_pin_* (empty when an input cannot be read).
s2b_host_pins() {
	s2b_board="$1/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
	s2b_pkgdir="$s2b_board/pmos/device-google-tegu"
	s2b_library="$s2b_pkgdir/ferrix-s5300-cpif.sh"
	s2b_body="$1/tools/pixel-9a/s5300-s2-preflight-remote.sh"
	s2b_pin_library=$(s2b_sha_file "$s2b_library" 2>/dev/null)
	s2b_pin_body=$(s2b_sha_file "$s2b_body" 2>/dev/null)
	s2b_pin_composed=$(cat "$s2b_library" "$s2b_body" 2>/dev/null | s2b_sha_stdin)
	s2b_pin_pins=$(s2b_sha_file "$s2b_pkgdir/s5300-cpif-services.sha256" 2>/dev/null)
	s2b_pin_mounts=$(s2b_sha_file "$s2b_pkgdir/ferrix-s5300-mounts.initd" 2>/dev/null)
	s2b_pin_device=$(sed -n 's/^pkgver=//p' "$s2b_pkgdir/APKBUILD" 2>/dev/null)-r$(sed -n 's/^pkgrel=//p' "$s2b_pkgdir/APKBUILD" 2>/dev/null)
	s2b_pin_modem=$(sed -n 's/^cpif_modem_version=//p' "$s2b_library" 2>/dev/null)
	s2b_pin_mm=$(sed -n 's/^pkgver=//p' "$s2b_board/pmos/modemmanager-samsung-sit/APKBUILD" 2>/dev/null)-r$(sed -n 's/^pkgrel=//p' "$s2b_board/pmos/modemmanager-samsung-sit/APKBUILD" 2>/dev/null)
	s2b_pin_factory=$(awk -F '\t' '$1 == "factory" { n++; h = $6 } END { if (n == 1) print h }' \
		"${FERRIX_PIXEL9A_PINS:-$s2b_board/recovery.pins}" 2>/dev/null)
}
