#!/usr/bin/env python3
"""Send one mock-control command to the guest ferrix-core over the QEMU
virtio-serial control socket and print the JSON ack.

Usage:
  mock_control.py --socket PATH '{"cmd":"set-power","charging":true}'
  mock_control.py --socket PATH set-power charging=true battery_percent=40

Values in key=value form are typed: true/false, integers, otherwise strings.
Exit status is 0 only for an `"status":"ok"` ack. This drives *mock* state in
the generic QEMU virtual phone; it is never a physical-device control.
"""

import argparse
import json
import socket
import sys
import time


def parse_value(text):
    if text == "true":
        return True
    if text == "false":
        return False
    try:
        return int(text)
    except ValueError:
        return text


def build_command(words):
    if not words:
        raise SystemExit("missing command")
    if words[0].lstrip().startswith("{"):
        if len(words) != 1:
            raise SystemExit("raw JSON takes no further arguments")
        return json.loads(words[0])
    command = {"cmd": words[0]}
    for word in words[1:]:
        key, sep, value = word.partition("=")
        if not sep or not key:
            raise SystemExit(f"expected key=value, got {word!r}")
        command[key] = parse_value(value)
    return command


def send(path, command, timeout, connect_wait):
    deadline = time.monotonic() + connect_wait
    while True:
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        client.settimeout(timeout)
        try:
            client.connect(path)
            break
        except OSError as error:
            client.close()
            if time.monotonic() >= deadline:
                raise SystemExit(f"cannot connect to control socket {path}: {error}")
            time.sleep(0.1)
    try:
        client.sendall(json.dumps(command).encode() + b"\n")
        buffer = b""
        while not buffer.endswith(b"\n"):
            chunk = client.recv(65536)
            if not chunk:
                raise SystemExit("control channel closed before an ack arrived")
            buffer += chunk
    except socket.timeout:
        raise SystemExit(f"no ack within {timeout}s (is ferrix-core running in the guest?)")
    finally:
        client.close()
    return json.loads(buffer.decode())


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--socket", required=True, help="host Unix socket of the ferrix.control chardev")
    parser.add_argument("--timeout", type=float, default=15.0, help="seconds to wait for the ack")
    parser.add_argument("--connect-wait", type=float, default=5.0, help="seconds to wait for the socket to appear")
    parser.add_argument("command", nargs="+", help="raw JSON or: NAME key=value ...")
    args = parser.parse_args()
    ack = send(args.socket, build_command(args.command), args.timeout, args.connect_wait)
    print(json.dumps(ack, separators=(",", ":")))
    return 0 if ack.get("status") == "ok" else 1


if __name__ == "__main__":
    sys.exit(main())
