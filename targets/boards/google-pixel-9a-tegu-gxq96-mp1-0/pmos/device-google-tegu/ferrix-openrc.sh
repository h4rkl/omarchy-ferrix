# Shared helpers for the Ferrix OpenRC services on the Pixel 9a target.
# Installed as /usr/lib/ferrix/openrc.sh and sourced by the service scripts.

# ferrix_user_runtime_dir: the `user` session's XDG_RUNTIME_DIR. /run is a
# fresh tmpfs at boot and checkpath does not create missing parents.
ferrix_user_runtime_dir() {
	checkpath -d -m 0755 /run/user &&
		checkpath -d -o user:user -m 0700 /run/user/1000
}

# ferrix_grant_socket PATH: wait for a root-owned daemon socket and open it to
# the unprivileged `user` session (ferrix-shell and apps) that connects to it.
ferrix_grant_socket() {
	tries=0
	while [ ! -S "$1" ] && [ "$tries" -lt 40 ]; do
		sleep 1
		tries=$((tries + 1))
	done
	if [ -S "$1" ]; then
		chown root:user "$1" && chmod 0660 "$1"
	else
		ewarn "$1 did not appear; clients running as user cannot connect"
	fi
}
