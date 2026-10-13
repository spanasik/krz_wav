#!/usr/bin/env python3
"""Statistical sanity check for extracted WAV samples.

Scans WAV files frame by frame and warns about regions that look like white
noise (corrupted or byte-swapped source data), e.g. the ones described in
kanban task T15.

Frames are 2048 samples with a 1024 hop. For every frame we compute RMS and
*spectral flatness* (geometric mean / arithmetic mean of the power spectrum).
White noise has a flatness close to 1, real tonal/percussive audio is well
below. A frame counts as noise when the flatness is high enough *and* the frame
is loud enough relative to the file.

Usage::

    uv run scripts/analyze_samples.py [PATH ...] [options]

Defaults to scanning ``samples/``.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
import wave
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

FRAME = 2048
HOP = 1024
WINDOW = np.hanning(FRAME)

DEFAULT_FLATNESS = 0.5
DEFAULT_MIN_RMS_RATIO = 0.15
DEFAULT_MIN_RMS_ABS = 500.0
DEFAULT_MIN_DURATION = 0.05
DEFAULT_MIN_FRACTION = 0.05


@dataclass
class Region:
    """A contiguous noisy region of a file, in seconds."""

    start: float
    end: float
    max_flatness: float
    frames: int

    @property
    def duration(self) -> float:
        return self.end - self.start


@dataclass
class ChannelReport:
    channel: int
    frames: int
    noise_frames: int
    regions: list[Region] = field(default_factory=list)

    @property
    def noise_fraction(self) -> float:
        return self.noise_frames / self.frames if self.frames else 0.0


@dataclass
class FileReport:
    path: Path
    sample_rate: int
    channels: int
    duration: float
    worst: ChannelReport

    @property
    def suspicious(self) -> bool:
        return bool(self.worst.regions)


def read_wav(path: Path) -> tuple[np.ndarray, int, int]:
    """Return (samples, sample_rate, channels); samples has shape (n, channels)."""
    with wave.open(str(path)) as handle:
        channels = handle.getnchannels()
        width = handle.getsampwidth()
        sample_rate = handle.getframerate()
        raw = handle.readframes(handle.getnframes())

    if width != 2:
        raise ValueError(f"unsupported sample width: {width * 8} bit")

    data = np.frombuffer(raw, dtype="<i2").astype(np.float64)
    if channels > 1:
        data = data.reshape(-1, channels)
    else:
        data = data.reshape(-1, 1)
    return data, sample_rate, channels


def frame_features(signal: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Per-frame RMS and spectral flatness."""
    if len(signal) < FRAME:
        return np.empty(0), np.empty(0)

    starts = np.arange(0, len(signal) - FRAME, HOP)
    frames = np.lib.stride_tricks.sliding_window_view(signal, FRAME)[starts] * WINDOW

    rms = np.sqrt((frames**2).mean(axis=1))
    power = np.abs(np.fft.rfft(frames, axis=1)) ** 2
    power = power[:, 1:] + 1e-12
    flatness = np.exp(np.log(power).mean(axis=1)) / power.mean(axis=1)
    return rms, flatness


def merge_regions(
    noisy: np.ndarray,
    flatness: np.ndarray,
    sample_rate: int,
    min_duration: float,
    gap_frames: int = 2,
) -> list[Region]:
    """Group noisy frames into regions, allowing small gaps."""
    regions: list[Region] = []
    start: int | None = None
    last: int | None = None

    for index, is_noisy in enumerate(noisy):
        if is_noisy:
            if start is None:
                start = index
            last = index
        elif start is not None and last is not None and index - last > gap_frames:
            regions.append(_region(start, last, flatness, sample_rate))
            start = last = None

    if start is not None and last is not None:
        regions.append(_region(start, last, flatness, sample_rate))

    return [region for region in regions if region.duration >= min_duration]


def _region(start: int, last: int, flatness: np.ndarray, sample_rate: int) -> Region:
    return Region(
        start=start * HOP / sample_rate,
        end=(last * HOP + FRAME) / sample_rate,
        max_flatness=float(flatness[start : last + 1].max()),
        frames=last - start + 1,
    )


def analyze_channel(
    signal: np.ndarray,
    sample_rate: int,
    *,
    flatness_threshold: float,
    min_rms_ratio: float,
    min_rms_abs: float,
    min_duration: float,
    channel: int = 0,
) -> ChannelReport:
    rms, flatness = frame_features(signal)
    if len(rms) == 0:
        return ChannelReport(channel=channel, frames=0, noise_frames=0)

    loud_threshold = max(min_rms_abs, min_rms_ratio * float(np.percentile(rms, 99)))
    noisy = (flatness >= flatness_threshold) & (rms >= loud_threshold)

    regions = merge_regions(noisy, flatness, sample_rate, min_duration)
    return ChannelReport(
        channel=channel,
        frames=len(rms),
        noise_frames=int(noisy.sum()),
        regions=regions,
    )


def analyze_file(
    path: Path,
    *,
    flatness_threshold: float = DEFAULT_FLATNESS,
    min_rms_ratio: float = DEFAULT_MIN_RMS_RATIO,
    min_rms_abs: float = DEFAULT_MIN_RMS_ABS,
    min_duration: float = DEFAULT_MIN_DURATION,
) -> FileReport:
    data, sample_rate, channels = read_wav(path)
    duration = len(data) / sample_rate if sample_rate else 0.0

    best: ChannelReport | None = None
    for channel in range(channels):
        report = analyze_channel(
            data[:, channel],
            sample_rate,
            flatness_threshold=flatness_threshold,
            min_rms_ratio=min_rms_ratio,
            min_rms_abs=min_rms_abs,
            min_duration=min_duration,
            channel=channel,
        )
        score = (report.noise_frames, len(report.regions))
        if best is None or score > (best.noise_frames, len(best.regions)):
            best = report

    assert best is not None
    return FileReport(
        path=path,
        sample_rate=sample_rate,
        channels=channels,
        duration=duration,
        worst=best,
    )


def is_suspicious(report: FileReport, min_fraction: float) -> bool:
    if report.worst.regions:
        return True
    return report.worst.noise_fraction >= min_fraction


def find_wavs(paths: list[Path]) -> list[Path]:
    files: list[Path] = []
    for path in paths:
        if path.is_dir():
            files.extend(
                sorted(p for p in path.rglob("*") if p.suffix.lower() == ".wav")
            )
        elif path.suffix.lower() == ".wav":
            files.append(path)
    return sorted(set(files))


def format_report(report: FileReport) -> str:
    channel = report.worst
    parts = []
    for region in channel.regions:
        at_end = " (at the end)" if region.end >= report.duration - 0.05 else ""
        parts.append(
            f"{region.start:.3f}-{region.end:.3f}s"
            f"[flat<={region.max_flatness:.2f}]{at_end}"
        )
    detail = ", ".join(parts) if parts else "noise frames: none"
    channel_note = f" ch{channel.channel}" if report.channels > 1 else ""
    return (
        f"{report.path}: {channel.noise_fraction * 100:.0f}% noise frames{channel_note}"
        f" in {report.duration:.2f}s -> {detail}"
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        default=[Path("samples")],
        help="files or directories to scan (default: samples)",
    )
    parser.add_argument(
        "--flatness",
        type=float,
        default=DEFAULT_FLATNESS,
        help=f"spectral flatness threshold (default: {DEFAULT_FLATNESS})",
    )
    parser.add_argument(
        "--min-rms-ratio",
        type=float,
        default=DEFAULT_MIN_RMS_RATIO,
        help=f"loudness threshold relative to p99 RMS (default: {DEFAULT_MIN_RMS_RATIO})",
    )
    parser.add_argument(
        "--min-duration",
        type=float,
        default=DEFAULT_MIN_DURATION,
        help=f"min noisy region length in seconds (default: {DEFAULT_MIN_DURATION})",
    )
    parser.add_argument(
        "--min-fraction",
        type=float,
        default=DEFAULT_MIN_FRACTION,
        help=f"min fraction of noisy frames (default: {DEFAULT_MIN_FRACTION})",
    )
    parser.add_argument("--top", type=int, default=0, help="show only N worst files")
    parser.add_argument("--json", action="store_true", help="machine-readable output")
    parser.add_argument(
        "--fail-on-warning",
        action="store_true",
        help="exit with code 1 if anything suspicious is found",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    files = find_wavs(args.paths)
    if not files:
        print("error: no .wav files found", file=sys.stderr)
        return 1

    reports: list[FileReport] = []
    errors = 0
    for path in files:
        try:
            report = analyze_file(
                path,
                flatness_threshold=args.flatness,
                min_rms_ratio=args.min_rms_ratio,
                min_duration=args.min_duration,
            )
        except (ValueError, wave.Error, OSError, struct.error) as error:
            print(f"error: cannot analyze {path}: {error}", file=sys.stderr)
            errors += 1
            continue
        reports.append(report)

    suspicious = [r for r in reports if is_suspicious(r, args.min_fraction)]
    suspicious.sort(key=lambda r: r.worst.noise_fraction, reverse=True)
    if args.top:
        suspicious = suspicious[: args.top]

    if args.json:
        print(
            json.dumps(
                {
                    "scanned": len(reports),
                    "suspicious": len(suspicious),
                    "files": [
                        {
                            "path": str(r.path),
                            "duration": round(r.duration, 4),
                            "channels": r.channels,
                            "channel": r.worst.channel,
                            "noise_fraction": round(r.worst.noise_fraction, 4),
                            "regions": [
                                {
                                    "start": round(region.start, 4),
                                    "end": round(region.end, 4),
                                    "max_flatness": round(region.max_flatness, 4),
                                }
                                for region in r.worst.regions
                            ],
                        }
                        for r in suspicious
                    ],
                },
                indent=2,
            )
        )
        return 1 if args.fail_on_warning and suspicious else 0

    for report in suspicious:
        print(f"WARNING: {format_report(report)}")

    print(f"\nscanned {len(reports)} files, {len(suspicious)} suspicious")
    if errors:
        print(f"{errors} file(s) could not be analyzed", file=sys.stderr)

    return 1 if args.fail_on_warning and suspicious else 0


if __name__ == "__main__":
    raise SystemExit(main())
