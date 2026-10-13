# krz_wav

Based on **[KurzFiler](https://kurzfiler.sourceforge.io/)** by Marc Halbruegge —
the Java GUI editor for Kurzweil K2x00 files. The KRZ reading logic and the expected
behaviour were taken from it; the only thing missing there was a small
command-line converter that is not written in Java, so this is our Rust version
doing the same job. See [License](#license) below for the legal part.

Kurzweil K2x00 (`.krz`, `.k25`, `.k26`) to WAV converter.

Extracts **all samples** stored in a Kurzweil file and writes them as 16-bit
WAV files. Written in Rust with no external dependencies, so the binary is
portable and small.

Started by Stanislav Panasik on Oct 13 2026 21:00.
Rust implementation written by **DeepSeek-V4.1-Flash** (AI assistant).

## Build

```bash
cargo build --release
# binary: target/release/krz_wav(.exe)
```

## Usage

```text
krz_wav [OPTIONS] <KRZ_FILE> <OUTPUT_DIR>

Options:
      --no-loop    Do not write loop points (smpl chunk) into the WAV files
      --floor      Use the exact floor(1e9 / samplePeriod) rate (KurzFiler
                   compatible) instead of snapping to standard sample rates
  -h, --help       Print help
      --version    Print version
```

Examples:

```bash
krz_wav bass/bassman.krz out            # -> out/UNNAMED WS.wav
krz_wav samples/2oscs.krz out           # -> out/2oscs/*.wav
krz_wav --no-loop looped.krz out        # no loop metadata
krz_wav --floor old.krz out             # byte-exact KurzFiler sample rates
```

## Output layout and names

- The file name is the internal sample name (`Name` column in KurzFiler), plus
  `.wav`. An existing `.wav` inside the name is stripped: `test.wav` → `test.wav`.
- Samples with **several headers** (multi-root) are split per root key and the
  root note is appended: `Name_C3.wav`, `Name_C5.wav`, …
- **Stereo** samples (two headers L/R) are merged into one stereo WAV and also get
  the note suffix: `houk2-021_C4.wav`.
- **One** resulting file goes straight into `<OUTPUT_DIR>`. **Several** files go
  into `<OUTPUT_DIR>/<krz-file-stem>/` (e.g. `out/2oscs/...`).
- Name collisions get `_2`, `_3`, … suffixes; characters that are illegal in file
  names are replaced with `_`.
- The output directory (and sub-directory) is created if missing.

## Loop metadata (`smpl`) and `--no-loop`

WAV files may carry loop information in a `smpl` chunk: where in the sample the
loop starts and ends. Samplers use it to play the sustain part of a sound
seamlessly. Without it the sample just plays once (one-shot).

Kurzweil samples store loop points (`sampleLoopStart` / `sampleEnd`). `krz_wav`
writes them into the `smpl` chunk when the sample is looped and the loop region
is non-empty. Pass `--no-loop` to omit the chunk entirely (useful if the target
sampler should treat everything as one-shots, or if a tool chokes on `smpl`).

`smpl` contents: manufacturer 0, product 0, sample period, MIDI unity note
(root key), one forward loop with `start`/`end` in samples.

## Format notes

`.krz`/`.k25`/`.k26` (big-endian):

1. 32-byte header: magic `PRAM`, `u32` offset of the PCM block, 6 reserved words.
2. Object chain from offset 32: each object is preceded by a negative `i32`
   block size; chain ends when the size is non-negative. Object = `u16` hash,
   `u16` size, `u16` name offset, name, then type-specific fields. Type 38 = sample.
3. A sample has `numHeaders + 1` `Soundfilehead` blocks of 32 bytes each with
   `rootkey`, flags, `sampleStart`, `sampleLoopStart`, `sampleEnd`, `samplePeriod`.
4. PCM is 16-bit **big-endian**, located at `pcm_offset + sampleStart * 2`.
5. Sample rate. The Kurzweil period is stored as whole nanoseconds, so
   `1e9 / samplePeriod` is slightly off (44100 Hz comes back as 44101).
   By default `krz_wav` **snaps to the nearest standard rate**
   (8000 … 192000) when it is within 0.1%, and keeps the exact value otherwise.
   Pass `--floor` for the exact `floor(1_000_000_000 / samplePeriod)` value,
   which is what KurzFiler writes — use it when comparing output byte-for-byte.

## Limitations

- Samples whose data is not embedded in the file (flag `0x40` clear) are skipped.
- Broken/out-of-range headers are skipped with a warning on stderr.
- Only 16-bit PCM is produced (`krz`/`k25`/`k26` store 16-bit).
- Byte-for-byte equality with KurzFiler output is not a goal (`JUNK`/`LIST`
  chunks are not reproduced); `fmt` and `data` do match, verified on fixtures.

## Development

```bash
cargo test          # unit + integration tests
cargo clippy --all-targets
```

Service/helper scripts are Python and run through [uv](https://docs.astral.sh/uv/):

```bash
uv run scripts/unpack_fixtures.py   # unpack RCS test fixtures into .tmp/fixtures
uv run scripts/compare_wav.py A.wav B.wav [--loop]
uv run pytest -q
uv run ruff check scripts
```

KurzFiler (the Java reference implementation) sources live in `kurzfiler/`.
Test fixtures are stored in CVS/RCS format under `kurzfiler/KurzFiler1.0/tests/`.

## License

`krz_wav` is released under the **GNU General Public License, version 2 or later**
(GPL-2.0-or-later) — the same license as
[KurzFiler](https://kurzfiler.sourceforge.io/), whose KRZ handling this project is
built on. The full text is in [`LICENSE`](LICENSE).

KurzFiler is copyright (c) 2001-2007 Marc Halbruegge and is licensed under
GPL-2.0-or-later. `krz_wav` is not affiliated with or endorsed by it. The KurzFiler
sources kept under `kurzfiler/` remain under their own license.
