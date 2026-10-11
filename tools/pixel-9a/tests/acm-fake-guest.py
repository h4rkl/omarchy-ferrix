#!/usr/bin/env python3
"""Offline stand-in for the Ferrix CDC-ACM bring-up shell (tests only).

Two pseudo-terminals are bridged byte for byte. The "host" pty slave plays
the macOS /dev/cu.usbmodem* node that tools/pixel-9a/acm-probe.py opens; a
real interactive POSIX `/bin/sh -i` runs on the "guest" pty, which keeps
the default cooked mode, echo and NL->CRNL like busybox ash on ttyGS1. No
device is contacted.

  acm-fake-guest.py PORT_FILE [--silent]

PORT_FILE receives the host-side tty path once the bridge is ready. With
--silent no shell is started and every byte is swallowed (an unresponsive
port). The bridge exits when the shell exits or on SIGTERM.
"""
import os
import pty
import select
import signal
import sys
import tty


def write_all(fd, data):
    view = memoryview(data)
    while view:
        view = view[os.write(fd, view):]


def main():
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[2] != "--silent"):
        sys.stderr.write("usage: acm-fake-guest.py PORT_FILE [--silent]\n")
        sys.exit(2)
    port_file = sys.argv[1]
    silent = len(sys.argv) == 3
    host_master, host_slave = os.openpty()
    # Keep the host slave open and raw so guest output is never echoed back
    # into the guest before the transport configures the port itself.
    tty.setraw(host_slave)
    child = None
    guest_master = None
    if not silent:
        child, guest_master = pty.fork()
        if child == 0:
            env = dict(os.environ)
            env.pop("ENV", None)
            env.update(PS1="fake-guest-host # ", PS2="> ", HISTFILE="/dev/null", TERM="dumb")
            os.execve("/bin/sh", ["/bin/sh", "-i"], env)

    def stop(*_):
        if child:
            try:
                os.kill(child, signal.SIGKILL)
            except OSError:
                pass
        os._exit(0)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    tmp = port_file + ".tmp"
    with open(tmp, "w") as handle:
        handle.write(os.ttyname(host_slave) + "\n")
    os.rename(tmp, port_file)

    fds = [host_master] + ([guest_master] if guest_master is not None else [])
    while True:
        ready, _, _ = select.select(fds, [], [])
        for fd in ready:
            try:
                data = os.read(fd, 65536)
            except OSError:
                data = b""
            if fd == host_master:
                if data and guest_master is not None:
                    write_all(guest_master, data)
            elif not data:
                stop()
            else:
                write_all(host_master, data)


if __name__ == "__main__":
    main()
