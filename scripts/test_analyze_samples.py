"""Tests for the sample noise analyzer (synthetic WAV files, no network)."""

from __future__ import annotations

import struct
import wave
from pathlib import Path

import numpy as np
from analyze_samples import analyze_file, is_suspicious

SAMPLE_RATE = 44100


def write_wav(path: Path, samples: np.ndarray, channels: int = 1) -> Path:
    data = np.clip(samples, -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(channels)
        handle.setsampwidth(2)
        handle.setframerate(SAMPLE_RATE)
        handle.writeframes(data.tobytes())
    return path


def tone(seconds: float, amplitude: float = 8000.0) -> np.ndarray:
    t = np.arange(int(SAMPLE_RATE * seconds)) / SAMPLE_RATE
    return amplitude * np.sin(2 * np.pi * 440.0 * t)


def white_noise(
    seconds: float, amplitude: float = 20000.0, seed: int = 1
) -> np.ndarray:
    rng = np.random.default_rng(seed)
    return amplitude * rng.uniform(-1.0, 1.0, int(SAMPLE_RATE * seconds))


def test_tone_is_not_flagged(tmp_path: Path) -> None:
    path = write_wav(tmp_path / "tone.wav", tone(1.0))
    report = analyze_file(path)
    assert not is_suspicious(report, 0.05)
    assert report.worst.regions == []


def test_silence_is_not_flagged(tmp_path: Path) -> None:
    path = write_wav(tmp_path / "silence.wav", np.zeros(SAMPLE_RATE))
    report = analyze_file(path)
    assert not is_suspicious(report, 0.05)


def test_noise_tail_is_flagged(tmp_path: Path) -> None:
    samples = np.concatenate([tone(0.5), white_noise(0.5)])
    path = write_wav(tmp_path / "tail.wav", samples)
    report = analyze_file(path)
    assert is_suspicious(report, 0.05)
    assert report.worst.regions
    last = report.worst.regions[-1]
    assert last.end > 0.9
    assert last.max_flatness > 0.5


def test_full_noise_is_flagged(tmp_path: Path) -> None:
    path = write_wav(tmp_path / "noise.wav", white_noise(1.0))
    report = analyze_file(path)
    assert is_suspicious(report, 0.05)
    assert report.worst.noise_fraction > 0.8


def test_unreadable_width_raises(tmp_path: Path) -> None:
    path = tmp_path / "bad.wav"
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(1)
        handle.setframerate(SAMPLE_RATE)
        handle.writeframes(struct.pack("<4B", 1, 2, 3, 4))
    try:
        analyze_file(path)
    except ValueError:
        return
    raise AssertionError("expected ValueError for 8-bit audio")
