//! Minimal RIFF/WAVE writer (16-bit PCM) with optional loop (`smpl`) metadata.

const BITS_PER_SAMPLE: u16 = 16;
const WAVE_FORMAT_PCM: u16 = 1;
const FALLBACK_SAMPLE_RATE: u32 = 44100;
const FALLBACK_SAMPLE_PERIOD: i32 = 22675;

/// Sample rates a snapped rate may land on.
const STANDARD_SAMPLE_RATES: [u32; 11] = [
    8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 88200, 96000, 192000,
];

/// Relative tolerance for snapping to a standard rate (0.1%).
const SNAP_TOLERANCE: f64 = 0.001;

/// Size of the `smpl` chunk payload (header + one loop).
const SMPL_CHUNK_SIZE: u32 = 36 + 24;

/// Loop points to store in the `smpl` chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopInfo {
    /// MIDI unity note (the sample root key).
    pub rootkey: u8,
    /// Sample period in nanoseconds.
    pub sample_period: i32,
    /// Loop start, inclusive, in samples.
    pub start: u32,
    /// Loop end, inclusive, in samples.
    pub end: u32,
}

/// Exact sample rate for a Kurzweil sample period: `floor(1e9 / period)`.
///
/// This is what KurzFiler writes; a non-positive period falls back to 44100 Hz.
pub fn exact_sample_rate(sample_period: i32) -> u32 {
    if sample_period <= 0 {
        return FALLBACK_SAMPLE_RATE;
    }
    (1_000_000_000u64 / sample_period as u64) as u32
}

/// Exact rate snapped to the nearest standard rate within [`SNAP_TOLERANCE`].
///
/// The Kurzweil period is stored as whole nanoseconds, so `1e9 / period` can be
/// off by a hair (44100 Hz comes back as 44101). Snapping recovers the intended
/// standard rate; non-standard rates are kept as the exact value.
pub fn snapped_sample_rate(sample_period: i32) -> u32 {
    if sample_period <= 0 {
        return FALLBACK_SAMPLE_RATE;
    }

    let exact = exact_sample_rate(sample_period);
    let mut best = None;
    let mut best_diff = f64::MAX;

    for &rate in &STANDARD_SAMPLE_RATES {
        let diff = (f64::from(rate) - f64::from(exact)).abs() / f64::from(rate);
        if diff <= SNAP_TOLERANCE && diff < best_diff {
            best = Some(rate);
            best_diff = diff;
        }
    }

    best.unwrap_or(exact)
}

/// Sample rate to write: snapped by default, exact when `snap` is false.
pub fn sample_rate(sample_period: i32, snap: bool) -> u32 {
    if sample_period <= 0 {
        return FALLBACK_SAMPLE_RATE;
    }
    let rate = if snap {
        snapped_sample_rate(sample_period)
    } else {
        exact_sample_rate(sample_period)
    };
    if rate == 0 {
        exact_sample_rate(FALLBACK_SAMPLE_PERIOD)
    } else {
        rate
    }
}

/// Build a complete WAV file (RIFF/WAVE, `fmt ` + `data` + optional `smpl`).
pub fn build_wav(
    channels: u16,
    sample_rate: u32,
    pcm: &[u8],
    loop_info: Option<LoopInfo>,
) -> Vec<u8> {
    let block_align = channels * BITS_PER_SAMPLE / 8;
    let byte_rate = sample_rate * u32::from(block_align);
    let data_len = pcm.len() as u32;
    let padding = data_len & 1;
    let smpl_len = if loop_info.is_some() {
        8 + SMPL_CHUNK_SIZE
    } else {
        0
    };

    // "WAVE" + fmt chunk (8 + 16) + data chunk + optional smpl chunk.
    let riff_size = 4 + (8 + 16) + (8 + data_len + padding) + smpl_len;

    let mut out = Vec::with_capacity(8 + riff_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());

    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    if padding == 1 {
        out.push(0);
    }

    if let Some(info) = loop_info {
        append_smpl(&mut out, &info);
    }

    out
}

fn append_smpl(out: &mut Vec<u8>, info: &LoopInfo) {
    out.extend_from_slice(b"smpl");
    out.extend_from_slice(&SMPL_CHUNK_SIZE.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // manufacturer
    out.extend_from_slice(&0u32.to_le_bytes()); // product
    out.extend_from_slice(&(info.sample_period.max(0) as u32).to_le_bytes());
    out.extend_from_slice(&u32::from(info.rootkey).to_le_bytes()); // MIDI unity note
    out.extend_from_slice(&0u32.to_le_bytes()); // MIDI pitch fraction
    out.extend_from_slice(&0u32.to_le_bytes()); // SMPTE format
    out.extend_from_slice(&0u32.to_le_bytes()); // SMPTE offset
    out.extend_from_slice(&1u32.to_le_bytes()); // number of loops
    out.extend_from_slice(&0u32.to_le_bytes()); // sampler data
    // One forward loop.
    out.extend_from_slice(&0u32.to_le_bytes()); // cue point id
    out.extend_from_slice(&0u32.to_le_bytes()); // type: forward
    out.extend_from_slice(&info.start.to_le_bytes());
    out.extend_from_slice(&info.end.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // fraction
    out.extend_from_slice(&0u32.to_le_bytes()); // play count
}

/// Interleave two little-endian 16-bit mono streams into one stereo stream.
///
/// Uses the shorter of the two inputs as the frame count.
pub fn interleave_stereo(left: &[u8], right: &[u8]) -> Vec<u8> {
    let frames = left.len().min(right.len()) / 2;
    let mut out = Vec::with_capacity(frames * 4);
    for frame in 0..frames {
        let offset = frame * 2;
        out.extend_from_slice(&left[offset..offset + 2]);
        out.extend_from_slice(&right[offset..offset + 2]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(data: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    }

    fn u32_at(data: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    }

    fn find_chunk<'a>(wav: &'a [u8], id: &[u8; 4]) -> Option<&'a [u8]> {
        let mut offset = 12;
        while offset + 8 <= wav.len() {
            let chunk_id = &wav[offset..offset + 4];
            let size = u32_at(wav, offset + 4) as usize;
            if chunk_id == id {
                return Some(&wav[offset + 8..offset + 8 + size]);
            }
            offset += 8 + size + (size & 1);
        }
        None
    }

    #[test]
    fn exact_sample_rate_matches_kurzfiler() {
        assert_eq!(exact_sample_rate(25575), 39100);
        assert_eq!(exact_sample_rate(45351), 22050);
        assert_eq!(exact_sample_rate(22675), 44101); // floor, ignores 44100 source
        assert_eq!(exact_sample_rate(0), 44100);
        assert_eq!(exact_sample_rate(-5), 44100);
    }

    #[test]
    fn snapped_rate_recovers_standard_rates() {
        assert_eq!(snapped_sample_rate(22675), 44100); // was 44101 exactly
        assert_eq!(snapped_sample_rate(45351), 22050);
        assert_eq!(snapped_sample_rate(25575), 39100); // no standard rate nearby
        assert_eq!(sample_rate(22675, true), 44100);
        assert_eq!(sample_rate(22675, false), 44101);
    }

    #[test]
    fn builds_mono_wav_header() {
        let pcm = vec![0u8; 32248];
        let wav = build_wav(1, 39100, &pcm, None);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32_at(&wav, 16), 16);
        assert_eq!(u16_at(&wav, 20), 1); // PCM
        assert_eq!(u16_at(&wav, 22), 1); // channels
        assert_eq!(u32_at(&wav, 24), 39100);
        assert_eq!(u32_at(&wav, 28), 39100 * 2); // byte rate
        assert_eq!(u16_at(&wav, 32), 2); // block align
        assert_eq!(u16_at(&wav, 34), 16); // bits

        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32_at(&wav, 40), 32248);
        assert_eq!(wav.len(), 44 + 32248);
        assert_eq!(u32_at(&wav, 4), (wav.len() - 8) as u32);
        assert!(find_chunk(&wav, b"smpl").is_none());
    }

    #[test]
    fn builds_stereo_wav_header() {
        let pcm = vec![0u8; 274632 * 2];
        let wav = build_wav(2, 44100, &pcm, None);

        assert_eq!(u16_at(&wav, 22), 2);
        assert_eq!(u32_at(&wav, 24), 44100);
        assert_eq!(u32_at(&wav, 28), 176400);
        assert_eq!(u16_at(&wav, 32), 4);
        assert_eq!(u32_at(&wav, 40), pcm.len() as u32);
        assert_eq!(u32_at(&wav, 4), (wav.len() - 8) as u32);
    }

    #[test]
    fn writes_smpl_chunk() {
        let pcm = vec![0u8; 8780];
        let info = LoopInfo {
            rootkey: 57,
            sample_period: 45351,
            start: 188,
            end: 4335,
        };
        let wav = build_wav(1, 22050, &pcm, Some(info));

        let smpl = find_chunk(&wav, b"smpl").expect("smpl chunk");
        assert_eq!(smpl.len(), SMPL_CHUNK_SIZE as usize);
        assert_eq!(u32_at(smpl, 0), 0); // manufacturer
        assert_eq!(u32_at(smpl, 2 * 4), 45351); // sample period
        assert_eq!(u32_at(smpl, 3 * 4), 57); // unity note
        assert_eq!(u32_at(smpl, 7 * 4), 1); // loop count
        assert_eq!(u32_at(smpl, 9 * 4 + 8), 188); // loop start
        assert_eq!(u32_at(smpl, 9 * 4 + 12), 4335); // loop end
        assert_eq!(u32_at(&wav, 4), (wav.len() - 8) as u32);
    }

    #[test]
    fn interleaves_stereo() {
        let left = [1u8, 2, 3, 4];
        let right = [5u8, 6, 7, 8];
        assert_eq!(interleave_stereo(&left, &right), [1, 2, 5, 6, 3, 4, 7, 8]);
    }

    #[test]
    fn interleaves_using_shorter_input() {
        let left = [1u8, 2, 3, 4];
        let right = [5u8, 6];
        assert_eq!(interleave_stereo(&left, &right), [1, 2, 5, 6]);
    }

    #[test]
    fn pads_odd_data_chunks() {
        let pcm = vec![1u8, 2, 3];
        let wav = build_wav(1, 22050, &pcm, None);
        assert_eq!(u32_at(&wav, 40), 3);
        assert_eq!(wav.len(), 44 + 3 + 1);
        assert_eq!(wav[44 + 3], 0);
        assert_eq!(u32_at(&wav, 4), (wav.len() - 8) as u32);
    }
}
