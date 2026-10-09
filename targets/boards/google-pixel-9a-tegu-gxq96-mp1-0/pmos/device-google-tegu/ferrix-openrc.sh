# Shared helpers for the Ferrix OpenRC services on the Pixel 9a target.
# Installed as /usr/lib/ferrix/openrc.sh and sourced from start_post().

# ferrix_grant_socket PATH: wait for a root-owned daemon socket and open it to
# the unprivileged `user` session (ferrix-shell and apps) that connects to it.
ferrix_grant_socket() {
	tries=0
	while [ ! -S "$1" ] && [ "$tries" -lt 10 ]; do
		sleep 1
		tries=$((tries + 1))
	done
	if [ -S "$1" ]; then
		chown root:user "$1" && chmod 0660 "$1"
	else
		ewarn "$1 did not appear; clients running as user cannot connect"
	fi
}
