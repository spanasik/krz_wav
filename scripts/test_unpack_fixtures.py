"""Tests for the RCS fixture unpacker."""

from __future__ import annotations

from pathlib import Path

from unpack_fixtures import output_path, rcs_text


def test_extracts_newest_revision() -> None:
    data = (
        b"head\t1.1;\n"
        b"desc\n@@\n\n"
        b"1.1\ntext\n@PRAM\x00\x01binary@@marker\n@\n"
        b"1.0\ntext\n@OLD\n@\n"
    )
    assert rcs_text(data) == b"PRAM\x00\x01binary@marker\n"


def test_requires_text_block() -> None:
    try:
        rcs_text(b"not an rcs file")
    except ValueError:
        return
    raise AssertionError("expected ValueError")


def test_output_path_strips_rcs_suffix() -> None:
    root = Path("tests")
    result = output_path(
        Path("tests") / "oldbugs" / "HOUK2-021.wav.krz,v",
        root,
        Path("out"),
    )
    assert result == Path("out") / "oldbugs" / "HOUK2-021.wav.krz"
