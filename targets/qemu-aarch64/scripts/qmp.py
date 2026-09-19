#!/usr/bin/env python3
"""Minimal QMP client for the checked-in QEMU agent workflow."""

import argparse
import json
import os
import socket
import struct
import sys
import zlib


def request(path, command, arguments=None):
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(10)
    client.connect(path)
    stream = client.makefile("rwb")
    json.loads(stream.readline())
    stream.write(json.dumps({"execute": "qmp_capabilities", "id": "capabilities"}).encode() + b"\n")
    stream.flush()
    json.loads(stream.readline())
    message = {"execute": command, "id": "request"}
    if arguments:
        message["arguments"] = arguments
    stream.write(json.dumps(message).encode() + b"\n")
    stream.flush()
    while True:
        reply = json.loads(stream.readline())
        if reply.get("id") == "request":
            if "error" in reply:
                raise RuntimeError(reply["error"].get("desc", str(reply["error"])))
            return reply.get("return")


def ppm_to_png(source, destination):
    with open(source, "rb") as image:
        if image.readline().strip() != b"P6":
            raise RuntimeError("QEMU did not produce a binary PPM framebuffer")
        tokens = []
        while len(tokens) < 3:
            line = image.readline()
            if not line:
                raise RuntimeError("truncated QEMU framebuffer")
            if not line.startswith(b"#"):
                tokens.extend(line.split())
        width, height, depth = map(int, tokens[:3])
        if depth != 255:
            raise RuntimeError("unsupported framebuffer color depth")
        pixels = image.read()
    if len(pixels) != width * height * 3:
        raise RuntimeError("unexpected QEMU framebuffer size")
    raw = b"".join(b"\0" + pixels[row * width * 3:(row + 1) * width * 3] for row in range(height))
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    encoded = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(destination, "wb") as output:
        output.write(encoded)
    return any(pixels)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--socket", required=True)
    action = parser.add_subparsers(dest="action", required=True)
    shot = action.add_parser("screenshot")
    shot.add_argument("--ppm", required=True)
    shot.add_argument("--png", required=True)
    tap = action.add_parser("tap")
    tap.add_argument("x", type=int)
    tap.add_argument("y", type=int)
    tap.add_argument("--width", type=int, required=True)
    tap.add_argument("--height", type=int, required=True)
    key = action.add_parser("key")
    key.add_argument("key")
    action.add_parser("quit")
    args = parser.parse_args()
    if args.action == "screenshot":
        request(args.socket, "screendump", {"filename": os.path.abspath(args.ppm)})
        nonblank = ppm_to_png(args.ppm, args.png)
        print(json.dumps({
            "evidence": "QEMU",
            "event": "framebuffer",
            "status": "ok" if nonblank else "failed",
            "reason": None if nonblank else "blank-framebuffer",
            "path": args.png,
        }))
        if not nonblank:
            sys.exit(3)
    elif args.action == "tap":
        if not 0 <= args.x < args.width or not 0 <= args.y < args.height:
            raise RuntimeError("tap coordinates are outside the configured display")
        x = round(args.x * 32767 / max(args.width - 1, 1))
        y = round(args.y * 32767 / max(args.height - 1, 1))
        events = [
            {"type": "abs", "data": {"axis": "x", "value": x}},
            {"type": "abs", "data": {"axis": "y", "value": y}},
            {"type": "btn", "data": {"button": "left", "down": True}},
            {"type": "btn", "data": {"button": "left", "down": False}},
        ]
        request(args.socket, "input-send-event", {"events": events})
    elif args.action == "key":
        request(args.socket, "send-key", {"keys": [{"type": "qcode", "data": args.key}]})
    else:
        request(args.socket, "quit")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"QMP error: {error}", file=sys.stderr)
        sys.exit(2)
