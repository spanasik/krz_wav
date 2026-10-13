"""Tests for the WAV comparison helper."""

from __future__ import annotations

import struct

from compare_wav import compare

FMT = struct.pack("<HHIIHH", 1, 1, 44100, 88200, 2, 16)


def make_wav(data: bytes, extra: bytes = b"") -> bytes:
    body = b"WAVE"
    body += b"fmt " + struct.pack("<I", len(FMT)) + FMT
    body += b"data" + struct.pack("<I", len(data)) + data
    body += extra
    return b"RIFF" + struct.pack("<I", len(body)) + body


def test_equal_files_have_no_differences() -> None:
    wav = make_wav(b"\x01\x02\x03\x04")
    assert compare(wav, wav, check_loop=False) == []


def test_detects_different_data() -> None:
    errors = compare(make_wav(b"\x01\x02"), make_wav(b"\x01\x03"), check_loop=False)
    assert any("data" in error for error in errors)


def test_ignores_extra_chunks_without_loop() -> None:
    smpl = b"smpl" + struct.pack("<I", 60) + bytes(60)
    errors = compare(
        make_wav(b"\x01\x02"), make_wav(b"\x01\x02", smpl), check_loop=False
    )
    assert errors == []


def test_detects_loop_difference_when_requested() -> None:
    smpl = b"smpl" + struct.pack("<I", 60) + bytes(60)
    errors = compare(
        make_wav(b"\x01\x02"), make_wav(b"\x01\x02", smpl), check_loop=True
    )
    assert any("smpl" in error for error in errors)
