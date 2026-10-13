"""Smoke-test service script for the krz_wav project.

Run with uv:

    uv run scripts/hello.py
"""

from __future__ import annotations

import sys


def main() -> int:
    print(f"hello from uv, Python {sys.version.split()[0]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
