#!/usr/bin/env python3
"""Minimal QMP client for the checked-in QEMU agent workflow."""

import argparse
import json
import os
import socket
import struct
import sys
import time
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
    # QEMU pads each PPM row to its surface stride (for example 830 px rows
    # carry two trailing bytes), so accept any stride of at least width * 3.
    stride, remainder = divmod(len(pixels), height) if height else (0, 1)
    if remainder or stride < width * 3:
        raise RuntimeError("unexpected QEMU framebuffer size")
    raw = b"".join(b"\0" + pixels[row * stride:row * stride + width * 3] for row in range(height))
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    encoded = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(destination, "wb") as output:
        output.write(encoded)
    return any(pixels)


QCODE_PLAIN = {
    " ": "spc", "\n": "ret", "\t": "tab", "-": "minus", "=": "equal", "[": "bracket_left",
    "]": "bracket_right", "\\": "backslash", ";": "semicolon", "'": "apostrophe",
    "`": "grave_accent", ",": "comma", ".": "dot", "/": "slash",
}
QCODE_SHIFTED = {
    "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9",
    ")": "0", "_": "minus", "+": "equal", "{": "bracket_left", "}": "bracket_right",
    "|": "backslash", ":": "semicolon", '"': "apostrophe", "~": "grave_accent", "<": "comma",
    ">": "dot", "?": "slash",
}


def text_to_qcodes(text):
    """Map ASCII text to QEMU send-key qcode combinations (US layout)."""
    combos = []
    for char in text:
        if char.isascii() and char.isalnum():
            combos.append(["shift", char.lower()] if char.isupper() else [char])
        elif char in QCODE_PLAIN:
            combos.append([QCODE_PLAIN[char]])
        elif char in QCODE_SHIFTED:
            combos.append(["shift", QCODE_SHIFTED[char]])
        else:
            raise RuntimeError(f"cannot type {char!r}: no US-layout qcode")
    return combos


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
    move = action.add_parser("move")
    move.add_argument("x", type=int)
    move.add_argument("y", type=int)
    move.add_argument("--width", type=int, required=True)
    move.add_argument("--height", type=int, required=True)
    swipe = action.add_parser("swipe")
    swipe.add_argument("x1", type=int)
    swipe.add_argument("y1", type=int)
    swipe.add_argument("x2", type=int)
    swipe.add_argument("y2", type=int)
    swipe.add_argument("--width", type=int, required=True)
    swipe.add_argument("--height", type=int, required=True)
    key = action.add_parser("key")
    key.add_argument("key")
    typed = action.add_parser("type", help="type ASCII text on the virtual keyboard; \\n sends Enter")
    typed.add_argument("text")
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
    elif args.action in ("tap", "move"):
        if not 0 <= args.x < args.width or not 0 <= args.y < args.height:
            raise RuntimeError("pointer coordinates are outside the configured display")
        x = round(args.x * 32767 / max(args.width - 1, 1))
        y = round(args.y * 32767 / max(args.height - 1, 1))
        motion = [
            {"type": "abs", "data": {"axis": "x", "value": x}},
            {"type": "abs", "data": {"axis": "y", "value": y}},
        ]
        if args.action == "tap":
            request(args.socket, "input-send-event", {"events": motion + [
                {"type": "btn", "data": {"button": "left", "down": True}}
            ]})
            time.sleep(0.05)
            request(args.socket, "input-send-event", {"events": [
                {"type": "btn", "data": {"button": "left", "down": False}}
            ]})
        else:
            request(args.socket, "input-send-event", {"events": motion})
    elif args.action == "swipe":
        points = [(args.x1, args.y1), (args.x2, args.y2)]
        if any(not 0 <= x < args.width or not 0 <= y < args.height for x, y in points):
            raise RuntimeError("swipe coordinates are outside the configured display")
        def absolute(x, y):
            return [
                {"type": "abs", "data": {"axis": "x", "value": round(x * 32767 / max(args.width - 1, 1))}},
                {"type": "abs", "data": {"axis": "y", "value": round(y * 32767 / max(args.height - 1, 1))}},
            ]
        request(args.socket, "input-send-event", {"events": absolute(args.x1, args.y1) + [
            {"type": "btn", "data": {"button": "left", "down": True}}
        ]})
        for step in range(1, 7):
            time.sleep(0.02)
            x = round(args.x1 + (args.x2 - args.x1) * step / 6)
            y = round(args.y1 + (args.y2 - args.y1) * step / 6)
            request(args.socket, "input-send-event", {"events": absolute(x, y)})
        request(args.socket, "input-send-event", {"events": [
            {"type": "btn", "data": {"button": "left", "down": False}}
        ]})
    elif args.action == "key":
        request(args.socket, "send-key", {"keys": [{"type": "qcode", "data": args.key}]})
    elif args.action == "type":
        for keys in text_to_qcodes(args.text):
            request(args.socket, "send-key", {"keys": [{"type": "qcode", "data": key} for key in keys]})
            time.sleep(0.05)
    else:
        request(args.socket, "quit")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"QMP error: {error}", file=sys.stderr)
        sys.exit(2)
