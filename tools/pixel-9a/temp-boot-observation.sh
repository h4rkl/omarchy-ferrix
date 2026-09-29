#!/bin/sh
# Shared evidence validation for the existing shell provisioning adapters.
# This is file/hash orchestration, not a guest service or hardware API. The
# caller supplies die, require_file, sha256_of, bytes_of and evidence_value.

pixel9a_check_kernel_observation_log() (
	log=$1
	require_file "$log" 'reviewed temporary-boot kernel log'
	[ -s "$log" ] || die 'temporary-boot kernel log is empty'
	grep -Eq 'Linux version [0-9]' "$log" || die 'temporary-boot observation has no Linux kernel version record'
	grep -Fq 'ZUMA PRO TEGU' "$log" || die 'temporary-boot observation has no tegu machine record'
	if grep -Eiq 'Kernel panic|Oops:|BUG:|watchdog.*lockup|soft lockup|hard LOCKUP' "$log"; then
		die 'temporary-boot observation contains a kernel failure; stop and review recovery'
	fi
)

pixel9a_verify_temp_observation() (
	observation=$1
	boot_transcript=$2
	boot_hash=$3
	serial_hash=$4
	[ -n "$observation" ] || die 'observed temporary-boot evidence is required; run pixel-9a-observe-temp-boot after reviewing the kernel capture'
	require_file "$observation" 'temporary-boot observation'
	# All records used for authorization must be singular. Do not select the
	# first success value from a contradictory or appended observation.
	for key in operation target_id serial_sha256_prefix boot_img_sha256 boot_transcript_sha256 kernel_log kernel_log_bytes kernel_log_sha256 result; do
		count=$(awk -F '=' -v key="$key" '$1 == key { n++ } END { print n+0 }' "$observation")
		[ "$count" = 1 ] || die "temporary-boot observation must contain exactly one $key record"
	done
	[ "$(evidence_value "$observation" operation)" = pixel-9a-radio-observe-temp ] || die 'temporary-boot observation operation is not recognized'
	[ "$(evidence_value "$observation" target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'temporary-boot observation target differs'
	[ "$(evidence_value "$observation" serial_sha256_prefix)" = "$serial_hash" ] || die 'temporary-boot observation was recorded on a different device'
	[ "$(evidence_value "$observation" boot_img_sha256)" = "$boot_hash" ] || die 'temporary-boot observation names a different boot image'
	[ "$(evidence_value "$observation" boot_transcript_sha256)" = "$(sha256_of "$boot_transcript")" ] || die 'temporary-boot observation names a different boot transcript'
	[ "$(evidence_value "$observation" result)" = temporary-boot-observed ] || die 'temporary-boot observation did not complete'
	log_name=$(evidence_value "$observation" kernel_log)
	case "$log_name" in ''|.*|*[!A-Za-z0-9_.-]*) die 'temporary-boot observation log filename is unsafe' ;; esac
	log="$(dirname -- "$observation")/$log_name"
	pixel9a_check_kernel_observation_log "$log"
	[ "$(bytes_of "$log")" = "$(evidence_value "$observation" kernel_log_bytes)" ] || die 'temporary-boot kernel log size changed after observation'
	[ "$(sha256_of "$log")" = "$(evidence_value "$observation" kernel_log_sha256)" ] || die 'temporary-boot kernel log hash changed after observation'
)
