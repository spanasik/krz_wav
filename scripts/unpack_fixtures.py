"""Unpack the RCS-stored test fixtures of KurzFiler into ``.tmp/fixtures``.

The fixtures under ``kurzfiler/KurzFiler1.0/tests`` are kept in CVS/RCS format
(``*.krz,v``, ``*.K26,v``, ``*.wav,v`` ...).  This script extracts the newest
revision of every RCS file and writes it without the trailing ``,v``.

Run with uv::

    uv run scripts/unpack_fixtures.py
"""

from __future__ import annotations

import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
TESTS_DIR = PROJECT_ROOT / "kurzfiler" / "KurzFiler1.0" / "tests"
OUTPUT_DIR = PROJECT_ROOT / ".tmp" / "fixtures"
RCS_SUFFIX = ",v"


def rcs_text(data: bytes) -> bytes:
    """Return the text of the newest revision stored in an RCS file.

    The first ``text`` block after the ``desc`` section holds the newest
    revision.  Its content is delimited by ``@`` and a literal ``@`` is
    escaped as ``@@``.
    """
    marker = b"\ntext\n"
    start = data.find(marker)
    if start < 0:
        raise ValueError("no RCS 'text' block found")
    pos = data.find(b"@", start + len(marker))
    if pos < 0:
        raise ValueError("malformed RCS 'text' block: no opening '@'")
    pos += 1

    out = bytearray()
    while pos < len(data):
        byte = data[pos]
        if byte == ord("@"):
            if pos + 1 < len(data) and data[pos + 1] == ord("@"):
                out.append(ord("@"))
                pos += 2
                continue
            break
        out.append(byte)
        pos += 1
    return bytes(out)


def output_path(rcs_file: Path, source_root: Path, destination_root: Path) -> Path:
    """Map ``<root>/a/b.krz,v`` to ``<dest>/a/b.krz``."""
    relative = rcs_file.relative_to(source_root)
    name = relative.name[: -len(RCS_SUFFIX)]
    return destination_root / relative.parent / name


def is_rcs_file(path: Path) -> bool:
    return path.is_file() and path.name.endswith(RCS_SUFFIX)


def main() -> int:
    if not TESTS_DIR.is_dir():
        print(f"error: fixtures directory not found: {TESTS_DIR}", file=sys.stderr)
        return 1

    if OUTPUT_DIR.exists():
        for path in sorted(OUTPUT_DIR.rglob("*"), reverse=True):
            if path.is_file():
                path.unlink()
            elif path.is_dir():
                path.rmdir()

    extracted: list[tuple[Path, int]] = []
    for rcs_file in sorted(TESTS_DIR.rglob(f"*{RCS_SUFFIX}")):
        if not is_rcs_file(rcs_file) or rcs_file.name.startswith("."):
            continue
        try:
            content = rcs_text(rcs_file.read_bytes())
        except ValueError as error:
            print(f"warning: skipping {rcs_file}: {error}", file=sys.stderr)
            continue

        destination = output_path(rcs_file, TESTS_DIR, OUTPUT_DIR)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(content)
        extracted.append((destination, len(content)))

    for path, size in extracted:
        print(f"{size:>9}  {path.relative_to(PROJECT_ROOT).as_posix()}")

    total = sum(size for _, size in extracted)
    print(
        f"\n{len(extracted)} files, {total} bytes -> {OUTPUT_DIR.relative_to(PROJECT_ROOT)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
