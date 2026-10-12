#!/usr/bin/env python3
"""Check relative Markdown links and #anchors in every tracked *.md file.

Stdlib only (Python >= 3.11). Run from anywhere inside the repository:

    python3 tools/dev/check-links.py [--allowlist FILE] [FILE.md ...]

For each inline link or image (`[text](target)`, `![alt](target)`) and each
reference definition (`[id]: target`) outside code fences and code spans:

- external targets (any `scheme:` or `//host`) are skipped;
- the path part is resolved against the linking file's directory (a leading
  `/` means the repository root, as on GitHub) and must exist;
- a `#fragment` on a Markdown target (or a bare `#fragment`) must match a
  heading anchor generated with GitHub's slug rules (lowercase, punctuation
  removed, spaces to hyphens, `-1`, `-2` ... for duplicates) or an explicit
  HTML `id`/`name` attribute. Fragments on non-Markdown targets (for example
  `#L10`) are not checked.

Known pre-existing breakage can be listed in the allowlist file
(`tools/dev/check-links-allowlist.txt` by default), one `source<TAB>target`
pair per line exactly as reported. Allowlisted entries that no longer fail are
reported so the list cannot go stale. Exit status: 0 clean, 1 broken links.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_ALLOWLIST = ROOT / "tools/dev/check-links-allowlist.txt"

FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
ATX = re.compile(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$")
SETEXT = re.compile(r"^ {0,3}(=+|-+)[ \t]*$")
HTML_ID = re.compile(r"""<[^>]*\b(?:id|name)\s*=\s*["']([^"']+)["']""", re.I)
REF_DEF = re.compile(r"^ {0,3}\[([^\]]+)\]:[ \t]*(<[^>]*>|\S+)")
SCHEME = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*:")
CODE_SPAN = re.compile(r"(`+)(.+?)\1")


def strip_code_spans(line: str) -> str:
    return CODE_SPAN.sub(lambda m: " " * len(m.group(0)), line)


def heading_text(raw: str) -> str:
    """Approximate the rendered text of a heading's inline Markdown."""
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", r"\1", raw)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\[[^\]]*\]", r"\1", text)
    text = re.sub(r"<[^>]+>", "", text)
    return text.replace("`", "")


def slug(text: str) -> str:
    """GitHub (github-slugger) anchor for a heading's rendered text."""
    text = text.strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def anchors(path: Path, cache: dict[Path, set[str]]) -> set[str]:
    if path in cache:
        return cache[path]
    found: set[str] = set()
    counts: dict[str, int] = {}

    def add(text: str) -> None:
        base = slug(heading_text(text))
        n = counts.get(base, 0)
        counts[base] = n + 1
        found.add(base if n == 0 else f"{base}-{n}")

    lines = path.read_text(encoding="utf-8").splitlines()
    fence: str | None = None
    prev = ""
    for line in lines:
        m = FENCE.match(line)
        if fence:
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence):
                fence = None
            prev = ""
            continue
        if m:
            fence = m.group(1)
            prev = ""
            continue
        for hid in HTML_ID.findall(line):
            found.add(hid)
        h = ATX.match(line)
        if h:
            add(h.group(2) or "")
            prev = ""
            continue
        if SETEXT.match(line) and prev.strip() and not prev.startswith((" " * 4, "\t")):
            if not (line.strip().startswith("-") and not prev.strip()):
                add(prev.strip())
            prev = ""
            continue
        prev = line
    cache[path] = found
    return found


def link_targets(line: str):
    """Yield link destinations from one line (code spans already blanked)."""
    m = REF_DEF.match(line)
    if m:
        yield m.group(2)
        return
    i = 0
    while True:
        j = line.find("](", i)
        if j < 0:
            return
        k = j + 2
        depth = 1
        while k < len(line) and depth:
            if line[k] == "(":
                depth += 1
            elif line[k] == ")":
                depth -= 1
            k += 1
        if depth == 0:
            dest = line[j + 2 : k - 1].strip()
            if dest.startswith("<"):
                dest = dest[1 : dest.find(">")] if ">" in dest else dest[1:]
            else:
                dest = dest.split()[0] if dest else dest
            yield dest
        i = j + 2


def check_file(md: Path, cache: dict[Path, set[str]]) -> list[tuple[str, str, str]]:
    errors = []
    rel = md.relative_to(ROOT).as_posix()
    fence: str | None = None
    for lineno, line in enumerate(md.read_text(encoding="utf-8").splitlines(), 1):
        m = FENCE.match(line)
        if fence:
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence):
                fence = None
            continue
        if m:
            fence = m.group(1)
            continue
        for target in link_targets(strip_code_spans(line)):
            if not target or SCHEME.match(target) or target.startswith("//"):
                continue
            path_part, _, fragment = target.partition("#")
            path_part = urllib.parse.unquote(path_part)
            fragment = urllib.parse.unquote(fragment)
            if path_part:
                base = ROOT if path_part.startswith("/") else md.parent
                dest = (base / path_part.lstrip("/")).resolve()
            else:
                dest = md
            try:
                dest.relative_to(ROOT)
            except ValueError:
                errors.append((rel, target, f"{rel}:{lineno}: {target}: outside repository"))
                continue
            if not dest.exists():
                errors.append((rel, target, f"{rel}:{lineno}: {target}: missing file"))
                continue
            if fragment and dest.is_file() and dest.suffix.lower() == ".md":
                if fragment not in anchors(dest, cache):
                    errors.append((rel, target, f"{rel}:{lineno}: {target}: missing anchor #{fragment}"))
    return errors


def tracked_markdown() -> list[Path]:
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z", "--", "*.md"],
        check=True,
        capture_output=True,
    ).stdout.decode()
    return [ROOT / p for p in out.split("\0") if p and (ROOT / p).is_file()]


def load_allowlist(path: Path) -> set[tuple[str, str]]:
    allowed: set[tuple[str, str]] = set()
    if not path.exists():
        return allowed
    for raw in path.read_text(encoding="utf-8").splitlines():
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        src, sep, target = raw.partition("\t")
        if not sep:
            raise SystemExit(f"{path}: malformed allowlist line (want source<TAB>target): {raw!r}")
        allowed.add((src.strip(), target.strip()))
    return allowed


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--allowlist", type=Path, default=DEFAULT_ALLOWLIST)
    ap.add_argument("files", nargs="*", type=Path)
    args = ap.parse_args()
    files = [p.resolve() for p in args.files] if args.files else tracked_markdown()
    allowed = load_allowlist(args.allowlist)
    cache: dict[Path, set[str]] = {}
    failures, seen = [], set()
    for md in sorted(files):
        for src, target, msg in check_file(md, cache):
            seen.add((src, target))
            if (src, target) not in allowed:
                failures.append(msg)
    for msg in failures:
        print(f"broken link: {msg}", file=sys.stderr)
    stale = sorted(allowed - seen) if not args.files else []
    for src, target in stale:
        print(f"stale allowlist entry (link now resolves or is gone): {src}\t{target}", file=sys.stderr)
    checked = len(files)
    if failures or stale:
        print(f"check-links: {len(failures)} broken, {len(stale)} stale allowlist entries in {checked} files", file=sys.stderr)
        return 1
    print(f"check-links: ok ({checked} files, {len(allowed)} allowlisted)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
