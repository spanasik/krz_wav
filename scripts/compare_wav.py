"""Compare two WAV files by their `fmt ` and `data` chunks.

Extra chunks (``JUNK``, ``LIST``, ``smpl``) are ignored unless ``--loop`` is given,
so a file produced by ``krz_wav`` can be compared with a reference exported by
another tool.

Usage::

    uv run scripts/compare_wav.py EXPECTED.wav ACTUAL.wav [--loop]
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path


def chunks(data: bytes) -> dict[bytes, bytes]:
    """Return a mapping of chunk id -> payload (first occurrence wins)."""
    result: dict[bytes, bytes] = {}
    offset = 12
    while offset + 8 <= len(data):
        chunk_id = data[offset : offset + 4]
        size = struct.unpack_from("<I", data, offset + 4)[0]
        payload = data[offset + 8 : offset + 8 + size]
        result.setdefault(chunk_id, payload)
        offset += 8 + size + (size & 1)
    return result


def compare(expected: bytes, actual: bytes, check_loop: bool) -> list[str]:
    expected_chunks = chunks(expected)
    actual_chunks = chunks(actual)
    errors: list[str] = []

    for name in (b"fmt ", b"data"):
        left = expected_chunks.get(name)
        right = actual_chunks.get(name)
        if left is None or right is None:
            errors.append(f"chunk {name!r} missing in one of the files")
        elif left != right:
            errors.append(f"chunk {name!r} differs: {len(left)} vs {len(right)} bytes")

    if check_loop and expected_chunks.get(b"smpl") != actual_chunks.get(b"smpl"):
        errors.append("smpl (loop) chunk differs")

    return errors


def main(argv: list[str]) -> int:
    paths = [arg for arg in argv[1:] if not arg.startswith("--")]
    if len(paths) != 2:
        print(__doc__, file=sys.stderr)
        return 2

    expected = Path(paths[0]).read_bytes()
    actual = Path(paths[1]).read_bytes()
    errors = compare(expected, actual, "--loop" in argv[1:])

    if errors:
        for error in errors:
            print(f"DIFF: {error}", file=sys.stderr)
        return 1

    print(
        f"OK: {paths[0]} == {paths[1]} (fmt, data{' , smpl' if '--loop' in argv else ''})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
