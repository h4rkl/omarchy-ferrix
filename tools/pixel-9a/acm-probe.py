#!/usr/bin/env python3
"""Run one checked-in, read-only guest probe over the Ferrix CDC-ACM shell.

Target: google-pixel-9a-tegu-gxq96-mp1-0 running a Ferrix dev image, whose
bring-up shell is the USB CDC-ACM function on ttyGS1 (macOS host node
/dev/cu.usbmodemferrix_tegu3; override with FERRIX_SHELL). The guest image is
built without sshd, so this replaces `ssh ... 'sh -s' < probe` for the
target-scoped inventory workflows. Python standard library only.

  acm-probe.py --probe FILE --timeout SECONDS --max-bytes N [--stderr merge|discard]

Protocol (one session, one probe run):
  1. Send one short POSIX bootstrap line to the interactive shell. It turns
     tty echo and NL->CRNL off, prints a READY marker, reads exactly the
     announced number of base64 lines, decodes them, and runs the probe only
     if its SHA-256 equals the host's hash of the exact file bytes. The probe
     is piped to `sh`, as `ssh ... 'sh -s'` does, so it cannot read the tty.
  2. Every marker is FERRIXACM:<128-bit random nonce>:<word>. The bootstrap
     assembles it at run time, so the echoed command line never contains it
     and probe output cannot forge it.
  3. The output between BEGIN and END:<status> is written to stdout and this
     program exits with the guest status. Anything outside the frame (echo,
     prompts, banners) is discarded.

Exit 255 means the transport failed; stderr then carries exactly one
`ferrix-acm: ... reason=<token>` line. No port path, USB serial, guest text or
nonce is ever printed. On a timeout or output overrun after the guest
acknowledged the bootstrap, the host sends Ctrl-C to stop its own probe and
restores the saved tty settings. The bootstrap line itself may be kept in the
interactive shell's history; it contains only the nonce, hash and line count.
"""
import argparse
import base64
import errno
import fcntl
import hashlib
import os
import re
import secrets
import select
import stat
import sys
import termios
import time
import tty

DEFAULT_PORT = "/dev/cu.usbmodemferrix_tegu3"
TRANSPORT_FAILURE = 255
MAX_PROBE_BYTES = 1024 * 1024
LINE_WIDTH = 76
READY_TIMEOUT = 15.0
# Bytes kept while searching for a marker outside the frame; older noise is
# discarded so a chatty console cannot grow the buffer without bound.
SEARCH_WINDOW = 8192

BOOTSTRAP = (
    "o=$(stty -g 2>/dev/null);stty -echo -onlcr 2>/dev/null;k=FERRIXACM;n=@NONCE@;"
    "printf '\\n%s:%s:READY\\n' $k $n;"
    "s=$(head -n @LINES@|base64 -d;printf x);s=${s%x};"
    "if [ \"$(printf %s \"$s\"|sha256sum|cut -c1-64)\" = @HASH@ ];then "
    "printf '\\n%s:%s:BEGIN\\n' $k $n;printf %s \"$s\"|sh @STDERR@;r=$?;"
    "printf '\\n%s:%s:END:%s\\n' $k $n $r;"
    "else printf '\\n%s:%s:HASH\\n' $k $n;fi;"
    "stty \"$o\" 2>/dev/null;unset o k n s r\n"
)
RESTORE = b"stty \"$o\" 2>/dev/null;unset o k n s r\n"


class TransportError(Exception):
    def __init__(self, reason):
        Exception.__init__(self, reason)
        self.reason = reason


def fail(reason):
    sys.stderr.write("ferrix-acm: transport=cdc-acm status=failed reason=%s\n" % reason)
    sys.exit(TRANSPORT_FAILURE)


def load_probe(path):
    try:
        info = os.lstat(path)
    except OSError:
        raise TransportError("probe-unreadable")
    if not stat.S_ISREG(info.st_mode):
        raise TransportError("probe-not-regular-file")
    with open(path, "rb") as handle:
        data = handle.read(MAX_PROBE_BYTES + 1)
    if not data:
        raise TransportError("probe-empty")
    if len(data) > MAX_PROBE_BYTES:
        raise TransportError("probe-too-large")
    # A shell variable cannot carry NUL bytes; refuse rather than alter them.
    if b"\0" in data:
        raise TransportError("probe-not-text")
    return data


def open_port(path):
    try:
        info = os.stat(path)
    except OSError:
        raise TransportError("port-absent")
    if not stat.S_ISCHR(info.st_mode):
        raise TransportError("port-not-character-device")
    try:
        fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    except OSError as error:
        if error.errno == errno.EBUSY:
            raise TransportError("port-busy")
        raise TransportError("port-open-failed")
    if not os.isatty(fd):
        os.close(fd)
        raise TransportError("port-not-tty")
    # Serialise concurrent transport runs; devfs may not support flock at all.
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError as error:
        if error.errno in (errno.EWOULDBLOCK, errno.EAGAIN):
            os.close(fd)
            raise TransportError("port-busy")
    if hasattr(termios, "TIOCEXCL"):
        try:
            fcntl.ioctl(fd, termios.TIOCEXCL)
        except OSError:
            pass
    return fd


class Session(object):
    def __init__(self, fd, deadline):
        self.fd = fd
        self.deadline = deadline
        self.buf = b""

    def remaining(self, limit=None):
        left = self.deadline - time.monotonic()
        if limit is not None:
            left = min(left, limit - time.monotonic())
        return left

    def write(self, data, limit=None):
        view = memoryview(data)
        while view:
            left = self.remaining(limit)
            if left <= 0:
                raise TransportError("timeout")
            _, ready, _ = select.select([], [self.fd], [], min(left, 0.5))
            if not ready:
                continue
            try:
                sent = os.write(self.fd, view)
            except (BlockingIOError, InterruptedError):
                continue
            except OSError:
                raise TransportError("port-io-error")
            view = view[sent:]

    def fill(self, limit=None, reason="timeout"):
        left = self.remaining(limit)
        if left <= 0:
            raise TransportError(reason)
        ready, _, _ = select.select([self.fd], [], [], min(left, 0.5))
        if not ready:
            return
        try:
            chunk = os.read(self.fd, 65536)
        except (BlockingIOError, InterruptedError):
            return
        except OSError:
            raise TransportError("port-io-error")
        if not chunk:
            raise TransportError("port-closed")
        self.buf += chunk

    def drain(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            ready, _, _ = select.select([self.fd], [], [], max(0.0, end - time.monotonic()))
            if not ready:
                return
            try:
                if not os.read(self.fd, 65536):
                    return
            except (BlockingIOError, InterruptedError):
                continue
            except OSError:
                return

    def wait_for(self, words, limit=None, reason="timeout"):
        """Wait for the first of `words`; consume the buffer through it."""
        while True:
            hits = [(self.buf.find(word), word) for word in words]
            hits = [hit for hit in hits if hit[0] >= 0]
            if hits:
                index, word = min(hits)
                self.buf = self.buf[index + len(word):]
                return word
            if len(self.buf) > SEARCH_WINDOW:
                self.buf = self.buf[-SEARCH_WINDOW:]
            self.fill(limit, reason)

    def consume_line_end(self):
        """Consume the newline that ends a marker line, across reads."""
        while True:
            for ending in (b"\n", b"\r\n"):
                if self.buf.startswith(ending):
                    self.buf = self.buf[len(ending):]
                    return
            if self.buf not in (b"", b"\r"):
                raise TransportError("framing")
            self.fill()


def run(probe_path, timeout, max_bytes, stderr_mode):
    data = load_probe(probe_path)
    digest = hashlib.sha256(data).hexdigest()
    encoded = base64.b64encode(data).decode("ascii")
    lines = [encoded[i:i + LINE_WIDTH] for i in range(0, len(encoded), LINE_WIDTH)]
    nonce = secrets.token_hex(16)
    tag = ("FERRIXACM:%s:" % nonce).encode("ascii")
    redirect = "2>&1" if stderr_mode == "merge" else "2>/dev/null"
    bootstrap = (
        BOOTSTRAP.replace("@NONCE@", nonce)
        .replace("@LINES@", str(len(lines)))
        .replace("@HASH@", digest)
        .replace("@STDERR@", redirect)
    )

    port = os.environ.get("FERRIX_SHELL") or DEFAULT_PORT
    fd = open_port(port)
    saved = None
    acknowledged = False
    session = Session(fd, time.monotonic() + timeout)
    try:
        saved = termios.tcgetattr(fd)
        tty.setraw(fd)
        # Discard stale console output, then clear any partial input line
        # (Ctrl-U) before the bootstrap.
        session.drain(0.3)
        session.write(b"\x15" + bootstrap.encode("ascii"))
        ready_limit = time.monotonic() + min(READY_TIMEOUT, timeout)
        session.wait_for([tag + b"READY"], ready_limit, "no-shell-response")
        acknowledged = True
        session.write("".join(line + "\n" for line in lines).encode("ascii"))
        word = session.wait_for([tag + b"BEGIN", tag + b"HASH"])
        if word == tag + b"HASH":
            acknowledged = False
            raise TransportError("hash-mismatch")
        session.consume_line_end()
        end = re.compile(b"\r?\n" + re.escape(tag) + b"END:([0-9]{1,3})\r?\n")
        body = session.buf
        session.buf = b""
        scanned = 0
        while True:
            match = end.search(body, max(0, scanned - len(tag) - 16))
            if match:
                break
            scanned = len(body)
            if scanned > max_bytes + len(tag) + 16:
                raise TransportError("output-limit")
            session.fill()
            body += session.buf
            session.buf = b""
        output = body[:match.start()]
        if len(output) > max_bytes:
            acknowledged = False
            raise TransportError("output-limit")
        status = int(match.group(1))
        if status > 255:
            acknowledged = False
            raise TransportError("framing")
        acknowledged = False
        session.drain(0.2)
    except TransportError:
        if acknowledged:
            # Stop our own still-running probe and put the tty back.
            try:
                session.deadline = time.monotonic() + 2.0
                session.write(b"\x03")
                session.drain(0.3)
                session.write(RESTORE)
                session.drain(0.3)
            except TransportError:
                pass
        raise
    finally:
        if saved is not None:
            try:
                termios.tcsetattr(fd, termios.TCSANOW, saved)
            except termios.error:
                pass
        if hasattr(termios, "TIOCNXCL"):
            try:
                fcntl.ioctl(fd, termios.TIOCNXCL)
            except OSError:
                pass
        os.close(fd)
    # The guest normally runs with -onlcr; normalise CRLF if stty was absent.
    return output.replace(b"\r\n", b"\n"), status


class Parser(argparse.ArgumentParser):
    def error(self, message):
        fail("usage")


def main():
    parser = Parser(add_help=False)
    parser.add_argument("--probe", required=True)
    parser.add_argument("--timeout", required=True, type=float)
    parser.add_argument("--max-bytes", required=True, type=int)
    parser.add_argument("--stderr", choices=("merge", "discard"), default="merge")
    args = parser.parse_args()
    if not 1 <= args.timeout <= 3600 or not 1 <= args.max_bytes <= 64 * 1024 * 1024:
        fail("usage")
    try:
        output, status = run(args.probe, args.timeout, args.max_bytes, args.stderr)
    except TransportError as error:
        fail(error.reason)
    except KeyboardInterrupt:
        fail("interrupted")
    sys.stdout.buffer.write(output)
    sys.stdout.buffer.flush()
    sys.exit(status)


if __name__ == "__main__":
    main()
