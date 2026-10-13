"""Tests for the .krz tree converter (no real conversion, no network)."""

from __future__ import annotations

from pathlib import Path

from convert_krz_tree import merge, unique_destination


def test_unique_destination_without_conflict(tmp_path: Path) -> None:
    destination = tmp_path / "a.wav"
    assert unique_destination(destination, "src") == destination


def test_unique_destination_appends_stem(tmp_path: Path) -> None:
    destination = tmp_path / "a.wav"
    destination.write_bytes(b"x")
    assert unique_destination(destination, "src") == tmp_path / "a (src).wav"


def test_unique_destination_counts_up(tmp_path: Path) -> None:
    (tmp_path / "a.wav").write_bytes(b"x")
    (tmp_path / "a (src).wav").write_bytes(b"x")
    assert unique_destination(tmp_path / "a.wav", "src") == tmp_path / "a (src 2).wav"


def test_merge_preserves_layout_and_renames(tmp_path: Path) -> None:
    staging = tmp_path / "staging"
    target = tmp_path / "target"
    (staging / "sub").mkdir(parents=True)
    (staging / "one.wav").write_bytes(b"1")
    (staging / "sub" / "two.wav").write_bytes(b"2")

    # A file with the same name already exists in the target.
    target.mkdir()
    (target / "one.wav").write_bytes(b"old")

    files, renamed = merge(staging, target, "krz")

    assert files == 2
    assert renamed == 1
    assert (target / "sub" / "two.wav").read_bytes() == b"2"
    assert (target / "one (krz).wav").read_bytes() == b"1"
    assert (target / "one.wav").read_bytes() == b"old"
