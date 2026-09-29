#!/bin/sh

# Checked-in device-data policy gate shared by every Pixel 9a mutating
# workflow.  A development device may explicitly declare that it contains no
# user data, but that declaration is never treated as a verified backup.  The
# owner must still opt in for the specific session before a destructive run.

pixel9a_data_safety_check() {
	pixel9a_data_safety_reason=''
	pixel9a_data_safety_status=''
	backup_value=$1
	policy_value=$2
	case "$backup_value" in
		verified)
			pixel9a_data_safety_status=backup-verified
			return 0
			;;
		not-required-dev-device)
			[ "$policy_value" = dev-device-no-user-data ] || {
				pixel9a_data_safety_reason="backup gate is '$backup_value' but data_policy is '$policy_value'; checked-in dev-device/no-user-data policy is missing"
				return 1
			}
			[ "${FERRIX_PIXEL9A_NO_USER_DATA:-}" = 'I-understand-this-is-a-new-development-device-with-no-personal-data' ] || {
				pixel9a_data_safety_reason='backup is not marked verified; owner must explicitly authorize the checked-in dev-device/no-user-data policy for this session with FERRIX_PIXEL9A_NO_USER_DATA=I-understand-this-is-a-new-development-device-with-no-personal-data'
				return 1
			}
			pixel9a_data_safety_status=dev-device-no-user-data
			return 0
			;;
		*)
			pixel9a_data_safety_reason="backup gate is '$backup_value'; owner must verify a readable backup outside Git before this mutation"
			return 1
			;;
	esac
}
