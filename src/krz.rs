//! Minimal reader for the Kurzweil K2x00 container format.
//!
//! Only what the extractor needs: the file header, the chain of objects, and
//! the sample description (a sample plus its sound file headers).

use std::fmt;
use std::ops::Range;

/// Object type id for samples (`KHash.T_SAMPLE`).
pub const T_SAMPLE: u16 = 38;

/// Offset of the first object in the file: after `magic` + `osize` + 6 words.
const OBJECTS_OFFSET: usize = 32;

/// Size of one `Soundfilehead` block.
const SOUNDFILEHEAD_SIZE: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Absolute offset of the PCM data block.
    pub osize: u32,
    /// Six reserved 32-bit fields (software version and friends).
    pub fields: [u32; 6],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub hash: u16,
    pub type_id: u16,
    /// Offset of the object body (right after its 4-byte block size).
    pub offset: usize,
    /// Offset of the type-specific fields (right after the name block).
    pub fields_offset: usize,
    /// Name as stored in the file, up to the first `NUL`.
    pub name: String,
}

impl Object {
    pub fn is_sample(&self) -> bool {
        self.type_id == T_SAMPLE
    }
}

/// One `Soundfilehead` block: describes one channel/root of sample data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Soundfilehead {
    pub rootkey: u8,
    pub flags: u8,
    pub volume_adjust: u8,
    pub alt_volume_adjust: u8,
    pub max_pitch: i16,
    pub offset_to_name: i16,
    pub sample_start: i32,
    pub alt_sample_start: i32,
    pub sample_loop_start: i32,
    pub sample_end: i32,
    pub offset_to_envelope: i16,
    pub alt_offset_to_envelope: i16,
    pub sample_period: i32,
}

impl Soundfilehead {
    /// Sample data is stored in the file (flag `0x40`, `needsLoad()`).
    pub fn needs_load(&self) -> bool {
        self.flags & 0x40 == 0x40
    }

    /// Looping is enabled (flag `0x80` clear, `isLooped()`).
    pub fn is_looped(&self) -> bool {
        self.flags & 0x80 == 0
    }

    /// Absolute byte range of this header's PCM data in the file.
    ///
    /// Returns `None` when the data is not embedded (`needs_load()` false) or
    /// when the header describes an out-of-bounds range.
    pub fn pcm_range(&self, osize: u32, file_len: usize) -> Option<Range<usize>> {
        if !self.needs_load() || self.sample_start < 0 || self.sample_end < self.sample_start {
            return None;
        }

        let start = (osize as usize).checked_add(self.sample_start as usize * 2)?;
        let sample_count = i64::from(self.sample_end) - i64::from(self.sample_start) + 1;
        let length = usize::try_from(sample_count).ok()?.checked_mul(2)?;
        let end = start.checked_add(length)?;

        (end <= file_len).then_some(start..end)
    }
}

/// A parsed sample object: metadata plus all of its sound file headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub object: Object,
    pub base_id: i16,
    pub num_headers: i16,
    pub headers_ofs: i16,
    pub flags: u8,
    pub ks1: u8,
    pub copy_id: i16,
    pub ks2: i16,
    pub headers: Vec<Soundfilehead>,
}

impl Sample {
    pub fn name(&self) -> &str {
        &self.object.name
    }

    /// Stereo sample: headers come in L/R pairs.
    pub fn is_stereo(&self) -> bool {
        self.flags == 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    TooShort {
        needed: usize,
        available: usize,
    },
    BadMagic([u8; 4]),
    Truncated {
        what: &'static str,
        offset: usize,
        needed: usize,
        available: usize,
    },
    InvalidNameLength {
        offset: usize,
        name_ofs: u16,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort { needed, available } => write!(
                f,
                "file too short: need at least {needed} bytes, got {available}"
            ),
            Error::BadMagic(magic) => write!(
                f,
                "not a Kurzweil file: expected magic \"PRAM\", got {magic:02X?}"
            ),
            Error::Truncated {
                what,
                offset,
                needed,
                available,
            } => write!(
                f,
                "truncated {what} at offset {offset}: need {needed} bytes, {available} available"
            ),
            Error::InvalidNameLength { offset, name_ofs } => write!(
                f,
                "invalid name length {name_ofs} at object offset {offset}"
            ),
        }
    }
}

impl std::error::Error for Error {}

fn read_u8(data: &[u8], offset: usize, what: &'static str) -> Result<u8, Error> {
    if offset >= data.len() {
        return Err(Error::Truncated {
            what,
            offset,
            needed: 1,
            available: 0,
        });
    }
    Ok(data[offset])
}

fn read_u16(data: &[u8], offset: usize, what: &'static str) -> Result<u16, Error> {
    let bytes = read_array::<2>(data, offset, what)?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_u32(data: &[u8], offset: usize, what: &'static str) -> Result<u32, Error> {
    let bytes = read_array::<4>(data, offset, what)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_i16(data: &[u8], offset: usize, what: &'static str) -> Result<i16, Error> {
    let bytes = read_array::<2>(data, offset, what)?;
    Ok(i16::from_be_bytes(bytes))
}

fn read_i32(data: &[u8], offset: usize, what: &'static str) -> Result<i32, Error> {
    let bytes = read_array::<4>(data, offset, what)?;
    Ok(i32::from_be_bytes(bytes))
}

fn read_array<const N: usize>(
    data: &[u8],
    offset: usize,
    what: &'static str,
) -> Result<[u8; N], Error> {
    let end = offset + N;
    if end > data.len() {
        return Err(Error::Truncated {
            what,
            offset,
            needed: N,
            available: data.len().saturating_sub(offset),
        });
    }
    let mut bytes = [0u8; N];
    bytes.copy_from_slice(&data[offset..end]);
    Ok(bytes)
}

/// Read and validate the 32-byte file header.
pub fn read_header(data: &[u8]) -> Result<Header, Error> {
    if data.len() < OBJECTS_OFFSET {
        return Err(Error::TooShort {
            needed: OBJECTS_OFFSET,
            available: data.len(),
        });
    }

    let magic = [data[0], data[1], data[2], data[3]];
    if &magic != b"PRAM" {
        return Err(Error::BadMagic(magic));
    }

    let osize = read_u32(data, 4, "header")?;
    let mut fields = [0u32; 6];
    for (index, field) in fields.iter_mut().enumerate() {
        *field = read_u32(data, 8 + index * 4, "header")?;
    }

    Ok(Header { osize, fields })
}

/// Object type for a raw object hash (`KHash.getType`).
pub fn object_type(hash: u16) -> u16 {
    if hash & 0x8000 != 0 {
        hash >> 10
    } else {
        hash >> 8
    }
}

/// Walk the chain of objects and return all of them in file order.
pub fn read_objects(data: &[u8]) -> Result<Vec<Object>, Error> {
    if data.len() < OBJECTS_OFFSET + 4 {
        return Err(Error::TooShort {
            needed: OBJECTS_OFFSET + 4,
            available: data.len(),
        });
    }

    let mut objects = Vec::new();
    let mut block_offset = OBJECTS_OFFSET;

    loop {
        let block_size = read_u32(data, block_offset, "object block size")? as i32;
        if block_size >= 0 {
            break;
        }

        let object_offset = block_offset + 4;
        let object_size = (-block_size) as usize;
        if object_size < 4 {
            return Err(Error::Truncated {
                what: "object",
                offset: block_offset,
                needed: 4,
                available: object_size,
            });
        }

        let hash = read_u16(data, object_offset, "object hash")?;
        let name_ofs = read_u16(data, object_offset + 4, "object name offset")?;
        if name_ofs < 2 {
            return Err(Error::InvalidNameLength {
                offset: object_offset,
                name_ofs,
            });
        }

        let name_len = (name_ofs - 2) as usize;
        let name_start = object_offset + 6;
        let name_end = name_start + name_len;
        if name_end > data.len() {
            return Err(Error::Truncated {
                what: "object name",
                offset: name_start,
                needed: name_len,
                available: data.len().saturating_sub(name_start),
            });
        }

        let raw_name = &data[name_start..name_end];
        let name_bytes = match raw_name.iter().position(|&byte| byte == 0) {
            Some(length) => &raw_name[..length],
            None => raw_name,
        };
        let name = String::from_utf8_lossy(name_bytes).into_owned();

        objects.push(Object {
            hash,
            type_id: object_type(hash),
            offset: object_offset,
            fields_offset: name_end,
            name,
        });

        // The block size is negative and covers the object body plus its own
        // 4-byte prefix, so the next block starts here.
        let next_offset = block_offset + object_size;
        if next_offset <= block_offset {
            return Err(Error::Truncated {
                what: "object chain",
                offset: block_offset,
                needed: 4,
                available: 0,
            });
        }
        block_offset = next_offset;

        if block_offset + 4 > data.len() {
            break;
        }
    }

    Ok(objects)
}

/// Parse the fields and sound file headers of a sample object.
pub fn read_sample(data: &[u8], object: &Object) -> Result<Sample, Error> {
    let base = object.fields_offset;

    let base_id = read_i16(data, base, "sample base id")?;
    let num_headers = read_i16(data, base + 2, "sample header count")?;
    let headers_ofs = read_i16(data, base + 4, "sample headers offset")?;
    let flags = read_u8(data, base + 6, "sample flags")?;
    let ks1 = read_u8(data, base + 7, "sample ks1")?;
    let copy_id = read_i16(data, base + 8, "sample copy id")?;
    let ks2 = read_i16(data, base + 10, "sample ks2")?;

    let header_count = (num_headers.max(-1) + 1) as usize;
    let mut headers = Vec::with_capacity(header_count);
    let mut offset = base + 12;

    for _ in 0..header_count {
        let header = read_soundfilehead(data, offset)?;
        headers.push(header);
        offset += SOUNDFILEHEAD_SIZE;
    }

    Ok(Sample {
        object: object.clone(),
        base_id,
        num_headers,
        headers_ofs,
        flags,
        ks1,
        copy_id,
        ks2,
        headers,
    })
}

fn read_soundfilehead(data: &[u8], offset: usize) -> Result<Soundfilehead, Error> {
    Ok(Soundfilehead {
        rootkey: read_u8(data, offset, "soundfilehead rootkey")?,
        flags: read_u8(data, offset + 1, "soundfilehead flags")?,
        volume_adjust: read_u8(data, offset + 2, "soundfilehead volume")?,
        alt_volume_adjust: read_u8(data, offset + 3, "soundfilehead alt volume")?,
        max_pitch: read_i16(data, offset + 4, "soundfilehead max pitch")?,
        offset_to_name: read_i16(data, offset + 6, "soundfilehead name offset")?,
        sample_start: read_i32(data, offset + 8, "soundfilehead sample start")?,
        alt_sample_start: read_i32(data, offset + 12, "soundfilehead alt sample start")?,
        sample_loop_start: read_i32(data, offset + 16, "soundfilehead loop start")?,
        sample_end: read_i32(data, offset + 20, "soundfilehead sample end")?,
        offset_to_envelope: read_i16(data, offset + 24, "soundfilehead envelope offset")?,
        alt_offset_to_envelope: read_i16(data, offset + 26, "soundfilehead alt envelope offset")?,
        sample_period: read_i32(data, offset + 28, "soundfilehead sample period")?,
    })
}

/// Parse every sample object in the file.
///
/// Objects whose fields cannot be parsed are skipped with a warning, so a
/// single broken object does not abort the whole extraction.
pub fn read_samples(data: &[u8]) -> Result<Vec<Sample>, Error> {
    let objects = read_objects(data)?;
    let mut samples = Vec::new();

    for object in objects.iter().filter(|object| object.is_sample()) {
        match read_sample(data, object) {
            Ok(sample) => samples.push(sample),
            Err(error) => {
                eprintln!(
                    "warning: skipping sample object 0x{:04X} '{}': {error}",
                    object.hash, object.name
                );
            }
        }
    }

    Ok(samples)
}

/// Read the PCM of one sound file header and convert it from big-endian to
/// little-endian 16-bit samples (the byte order used by WAV).
///
/// Returns `None` when the header has no embedded data or an out-of-bounds range.
pub fn read_pcm_le(data: &[u8], header: &Soundfilehead, osize: u32) -> Option<Vec<u8>> {
    let range = header.pcm_range(osize, data.len())?;
    let raw = &data[range];

    let mut little_endian = Vec::with_capacity(raw.len());
    for sample in raw.as_chunks::<2>().0 {
        little_endian.push(sample[1]);
        little_endian.push(sample[0]);
    }
    Some(little_endian)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn bassman() -> Vec<u8> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/bass/bassman.krz");
        std::fs::read(path).expect("bass/bassman.krz must be present")
    }

    fn fixture(path: &str) -> Option<Vec<u8>> {
        read(&format!(
            "{}/.tmp/fixtures/{path}",
            env!("CARGO_MANIFEST_DIR")
        ))
    }

    #[test]
    fn object_type_matches_kurzfiler() {
        assert_eq!(object_type(0x90C8), 36); // Program
        assert_eq!(object_type(0x94CA), 37); // Keymap
        assert_eq!(object_type(0x98CD), 38); // Sample
        assert_eq!(object_type(0xCE00), 51); // other
        assert_eq!(object_type(0x1234), 0x12); // no high bit
    }

    #[test]
    fn reads_header() {
        let header = read_header(&bassman()).unwrap();
        assert_eq!(header.osize, 1692);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut data = bassman();
        data[0] = b'X';
        assert!(matches!(read_header(&data), Err(Error::BadMagic(_))));
    }

    #[test]
    fn walks_bassman_objects() {
        let data = bassman();
        let objects = read_objects(&data).unwrap();
        assert_eq!(objects.len(), 9);

        let samples: Vec<_> = objects.into_iter().filter(Object::is_sample).collect();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "UNNAMED WS");
        assert_eq!(samples[0].type_id, T_SAMPLE);
    }

    #[test]
    fn parses_bassman_sample() {
        let data = bassman();
        let samples = read_samples(&data).unwrap();
        assert_eq!(samples.len(), 1);

        let sample = &samples[0];
        assert_eq!(sample.name(), "UNNAMED WS");
        assert!(!sample.is_stereo());
        assert_eq!(sample.headers.len(), 1);

        let header = sample.headers[0];
        assert_eq!(header.rootkey, 48);
        assert!(header.needs_load());
        assert!(header.is_looped());
        assert_eq!(header.sample_start, 0);
        assert_eq!(header.sample_end, 16123);
        assert_eq!(header.sample_loop_start, 15520);
        assert_eq!(header.sample_period, 25575);

        let range = header.pcm_range(1692, data.len()).unwrap();
        assert_eq!(range, 1692..data.len());
        assert_eq!(range.len(), 32248);
    }

    #[test]
    fn parses_stereo_fixture() {
        let Some(data) = fixture("oldbugs/HOUK2-021.wav.krz") else {
            eprintln!("skipping: fixtures not unpacked (run scripts/unpack_fixtures.py)");
            return;
        };
        let samples = read_samples(&data).unwrap();
        assert_eq!(samples.len(), 1);

        let sample = &samples[0];
        assert!(sample.is_stereo());
        assert_eq!(sample.headers.len(), 2);
        assert_eq!(sample.headers[0].rootkey, 60);
        assert_eq!(sample.headers[1].rootkey, 60);
        assert_eq!(sample.headers[0].sample_period, 22675);
    }

    #[test]
    fn parses_multi_sample_fixtures() {
        let Some(data) = fixture("2oscs.krz") else {
            eprintln!("skipping: fixtures not unpacked (run scripts/unpack_fixtures.py)");
            return;
        };
        let samples = read_samples(&data).unwrap();
        assert_eq!(samples.len(), 27);
        assert!(samples.iter().all(|sample| sample.headers.len() == 1));

        let Some(data) = fixture("2oscsCompacted.krz") else {
            return;
        };
        let samples = read_samples(&data).unwrap();
        assert_eq!(samples.len(), 3);
        assert!(samples.iter().any(|sample| sample.headers.len() > 1));
    }

    #[test]
    fn broken_headers_do_not_panic() {
        let Some(data) = fixture("2oscsCompacted.krz") else {
            eprintln!("skipping: fixtures not unpacked (run scripts/unpack_fixtures.py)");
            return;
        };
        let samples = read_samples(&data).unwrap();
        // Every header may be asked for a range without panicking.
        for sample in &samples {
            for header in &sample.headers {
                let _ = header.pcm_range(0, data.len());
            }
        }
    }

    fn wav_data_chunk(wav: &[u8]) -> &[u8] {
        let mut offset = 12;
        while offset + 8 <= wav.len() {
            let id = &wav[offset..offset + 4];
            let size = u32::from_le_bytes([
                wav[offset + 4],
                wav[offset + 5],
                wav[offset + 6],
                wav[offset + 7],
            ]) as usize;
            if id == b"data" {
                return &wav[offset + 8..offset + 8 + size];
            }
            offset += 8 + size + (size & 1);
        }
        panic!("reference WAV has no data chunk");
    }

    #[test]
    fn pcm_matches_kurzfiler_reference() {
        let data = bassman();
        let samples = read_samples(&data).unwrap();
        let pcm = read_pcm_le(&data, &samples[0].headers[0], 1692).unwrap();
        assert_eq!(pcm.len(), 32248);

        let reference = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/bass/bassman.wav 1 C3.wav"
        ))
        .expect("reference wav must be present");
        assert_eq!(pcm, wav_data_chunk(&reference));
    }

    #[test]
    fn pcm_is_none_without_usable_data() {
        let data = bassman();
        let samples = read_samples(&data).unwrap();

        let mut no_data = samples[0].headers[0];
        no_data.flags = 0; // needs_load() false
        assert!(read_pcm_le(&data, &no_data, 1692).is_none());

        let mut out_of_bounds = samples[0].headers[0];
        out_of_bounds.sample_end = i32::MAX;
        assert!(read_pcm_le(&data, &out_of_bounds, 1692).is_none());
    }
}
