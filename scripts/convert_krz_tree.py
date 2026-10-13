#!/usr/bin/env python3
"""Convert every ``.krz`` in a directory tree to WAV with the ``krz_wav`` binary.

The sub-directory structure of the source tree is preserved::

    <SRC>/Bass/bassman.krz   ->  <OUT>/Bass/UNNAMED WS.wav
    <SRC>/Drums/kit.krz      ->  <OUT>/Drums/kit/Name_C3.wav   (multi-sample)

Every file is converted into a private staging directory first, so that outputs
of different ``.krz`` files never overwrite each other; on a name clash the source
file stem is appended, e.g. ``Name (bassman).wav``.

Usage::

    uv run scripts/convert_krz_tree.py <SRC_DIR> <OUT_DIR> [--binary PATH]
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
STAGING_ROOT = PROJECT_ROOT / ".tmp" / "krz_stage"


def find_binary(explicit: str | None) -> Path:
    if explicit:
        binary = Path(explicit)
        if not binary.is_file():
            raise SystemExit(f"error: binary not found: {binary}")
        return binary

    for candidate in ("target/release/krz_wav.exe", "target/debug/krz_wav.exe"):
        binary = PROJECT_ROOT / candidate
        if binary.is_file():
            return binary
    raise SystemExit("error: krz_wav binary not found, build it or pass --binary")


def find_krz_files(source: Path) -> list[Path]:
    return sorted(
        path
        for path in source.rglob("*")
        if path.is_file() and path.suffix.lower() == ".krz"
    )


def unique_destination(destination: Path, stem: str) -> Path:
    """Avoid overwriting: append the source stem, then a counter."""
    if not destination.exists():
        return destination

    candidate = destination.with_name(f"{destination.stem} ({stem}){destination.suffix}")
    counter = 2
    while candidate.exists():
        candidate = destination.with_name(
            f"{destination.stem} ({stem} {counter}){destination.suffix}"
        )
        counter += 1
    return candidate


def merge(staging: Path, target: Path, stem: str) -> tuple[int, int]:
    """Move everything from ``staging`` into ``target``. Returns (files, renamed)."""
    files = 0
    renamed = 0
    for path in sorted(staging.rglob("*")):
        if not path.is_file():
            continue
        destination = target / path.relative_to(staging)
        final = unique_destination(destination, stem)
        if final != destination:
            renamed += 1
        final.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(path), str(final))
        files += 1
    return files, renamed


def convert_one(binary: Path, krz: Path, staging: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(binary), str(krz), str(staging)],
        check=False,
        capture_output=True,
        text=True,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", type=Path, help="directory with .krz files")
    parser.add_argument("output", type=Path, help="directory for the .wav files")
    parser.add_argument("--binary", help="path to the krz_wav binary")
    args = parser.parse_args(argv)

    binary = find_binary(args.binary)
    source: Path = args.source
    output: Path = args.output

    if not source.is_dir():
        print(f"error: source directory not found: {source}", file=sys.stderr)
        return 1

    if STAGING_ROOT.exists():
        shutil.rmtree(STAGING_ROOT)
    STAGING_ROOT.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)

    files = find_krz_files(source)
    print(f"found {len(files)} .krz files in {source}")

    converted = empty = failed = 0
    total_wav = renamed_total = 0

    for index, krz in enumerate(files, start=1):
        relative_dir = krz.parent.relative_to(source)
        staging = STAGING_ROOT / relative_dir / krz.stem
        staging.mkdir(parents=True, exist_ok=True)

        result = convert_one(binary, krz, staging)
        label = f"[{index}/{len(files)}] {relative_dir / krz.name}"

        if result.returncode != 0:
            failed += 1
            message = (result.stderr or result.stdout).strip().splitlines()
            print(f"{label}: FAILED ({message[-1] if message else 'unknown error'})")
            continue

        target = output / relative_dir
        moved, renamed = merge(staging, target, krz.stem)

        if moved == 0:
            empty += 1
            print(f"{label}: no samples")
        else:
            converted += 1
            total_wav += moved
            renamed_total += renamed
            suffix = f", {renamed} renamed" if renamed else ""
            print(f"{label}: {moved} wav{suffix}")

    shutil.rmtree(STAGING_ROOT, ignore_errors=True)

    print(
        f"\ndone: {converted} converted ({total_wav} wav, {renamed_total} renamed), "
        f"{empty} without samples, {failed} failed -> {output}"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
