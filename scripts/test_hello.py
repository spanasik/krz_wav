"""Smoke test for the service-script toolchain (uv + pytest)."""

from __future__ import annotations

from hello import main


def test_main_returns_zero() -> None:
    assert main() == 0
