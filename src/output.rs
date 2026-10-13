//! Output file naming and directory layout.
//!
//! Implements the agreed rules:
//!
//! - file name = internal sample name (+ `_<NOTE>` when the sample has several
//!   headers, i.e. multi-root or stereo), plus `.wav`;
//! - an existing `.wav` extension inside the internal name is stripped;
//! - characters illegal in file names are replaced with `_`;
//! - name collisions get `_2`, `_3`, ... suffixes;
//! - one resulting file goes straight into the output directory, several go
//!   into a sub-directory named after the KRZ file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::krz::{Sample, Soundfilehead, read_pcm_le};

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Kurzweil note name for a MIDI-style root key (`NoteNr.getNote`).
pub fn note_name(rootkey: u8) -> String {
    let octave = rootkey / 12;
    format!(
        "{}{}",
        NOTE_NAMES[(rootkey % 12) as usize],
        octave as i32 - 1
    )
}

/// Strip a trailing `.wav` (any case) and make the rest safe for a file name.
pub fn file_stem(internal_name: &str) -> String {
    let without_extension = internal_name
        .strip_suffix(".wav")
        .or_else(|| internal_name.strip_suffix(".WAV"))
        .unwrap_or(internal_name);

    let mut sanitized: String = without_extension
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            character if (character as u32) < 0x20 => '_',
            character => character,
        })
        .collect();

    // Windows does not allow file names ending with a dot or a space.
    while sanitized.ends_with(['.', ' ']) {
        sanitized.pop();
    }

    if sanitized.is_empty() {
        sanitized.push_str("sample");
    }
    sanitized
}

/// Build the file name (without directory) for one output file.
pub fn file_name(internal_name: &str, rootkey: u8, with_note: bool) -> String {
    let stem = file_stem(internal_name);
    if with_note {
        format!("{stem}_{}.wav", note_name(rootkey))
    } else {
        format!("{stem}.wav")
    }
}

/// One planned output file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFile {
    /// File name including the `.wav` extension (without directory).
    pub name: String,
    /// One entry for mono, two for stereo (left, right).
    pub headers: Vec<Soundfilehead>,
}

impl OutputFile {
    pub fn is_stereo(&self) -> bool {
        self.headers.len() == 2
    }
}

/// Plan all output files for the given samples.
///
/// Headers without usable PCM data are skipped. Names are de-duplicated with
/// `_2`, `_3`, ... suffixes.
pub fn plan(samples: &[Sample], data: &[u8], osize: u32) -> Vec<OutputFile> {
    let mut used = HashSet::new();
    let mut files = Vec::new();

    for sample in samples {
        let usable = |header: &Soundfilehead| read_pcm_le(data, header, osize).is_some();

        if sample.is_stereo() {
            let pairs = sample.headers.as_chunks::<2>().0;
            for pair in pairs {
                let (left, right) = (pair[0], pair[1]);
                if !usable(&left) || !usable(&right) {
                    continue;
                }
                let candidate = file_name(sample.name(), left.rootkey, true);
                files.push(OutputFile {
                    name: unique_name(&mut used, &candidate),
                    headers: vec![left, right],
                });
            }
        } else {
            let multiple_roots = sample.headers.len() > 1;
            for header in &sample.headers {
                if !usable(header) {
                    continue;
                }
                let candidate = file_name(sample.name(), header.rootkey, multiple_roots);
                files.push(OutputFile {
                    name: unique_name(&mut used, &candidate),
                    headers: vec![*header],
                });
            }
        }
    }

    files
}

fn unique_name(used: &mut HashSet<String>, candidate: &str) -> String {
    if used.insert(candidate.to_string()) {
        return candidate.to_string();
    }

    let (stem, extension) = match candidate.rfind('.') {
        Some(index) => (&candidate[..index], &candidate[index..]),
        None => (candidate, ""),
    };

    for counter in 2.. {
        let name = format!("{stem}_{counter}{extension}");
        if used.insert(name.clone()) {
            return name;
        }
    }
    unreachable!()
}

/// Directory to write the output files into: the output root for a single
/// file, or a sub-directory named after the KRZ file for several files.
pub fn output_directory(output_root: &Path, krz_path: &Path, file_count: usize) -> PathBuf {
    if file_count <= 1 {
        return output_root.to_path_buf();
    }

    let stem = krz_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    output_root.join(file_stem(&stem))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::krz::{read_header, read_samples};

    fn bassman() -> Vec<u8> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/bass/bassman.krz");
        std::fs::read(path).expect("bass/bassman.krz must be present")
    }

    fn fixture(path: &str) -> Option<Vec<u8>> {
        std::fs::read(format!(
            "{}/.tmp/fixtures/{path}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .ok()
    }

    #[test]
    fn note_names_match_kurzfiler() {
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(48), "C3");
        assert_eq!(note_name(57), "A3");
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(61), "C#4");
        assert_eq!(note_name(127), "G9");
    }

    #[test]
    fn file_stem_strips_extension_and_sanitizes() {
        assert_eq!(file_stem("test.wav"), "test");
        assert_eq!(file_stem("TEST.WAV"), "TEST");
        assert_eq!(file_stem("a<b>c:d\"e/f\\g|h?i*j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(file_stem("trailing...  "), "trailing");
        assert_eq!(file_stem(""), "sample");
        assert_eq!(file_stem("UNNAMED WS"), "UNNAMED WS");
    }

    #[test]
    fn file_name_adds_note_only_when_requested() {
        assert_eq!(file_name("UNNAMED WS", 48, false), "UNNAMED WS.wav");
        assert_eq!(file_name("test.wav", 60, true), "test_C4.wav");
        assert_eq!(file_name("800dv.krz0.wav", 108, false), "800dv.krz0.wav");
    }

    #[test]
    fn unique_name_appends_suffix() {
        let mut used = HashSet::new();
        assert_eq!(unique_name(&mut used, "a.wav"), "a.wav");
        assert_eq!(unique_name(&mut used, "a.wav"), "a_2.wav");
        assert_eq!(unique_name(&mut used, "a.wav"), "a_3.wav");
    }

    #[test]
    fn plans_single_file_for_bassman() {
        let data = bassman();
        let header = read_header(&data).unwrap();
        let samples = read_samples(&data).unwrap();

        let files = plan(&samples, &data, header.osize);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "UNNAMED WS.wav");
        assert!(!files[0].is_stereo());

        let directory =
            output_directory(Path::new("out"), Path::new("bass/bassman.krz"), files.len());
        assert_eq!(directory, Path::new("out"));
    }

    #[test]
    fn plans_many_files_with_unique_names() {
        let Some(data) = fixture("2oscs.krz") else {
            eprintln!("skipping: fixtures not unpacked (run scripts/unpack_fixtures.py)");
            return;
        };
        let header = read_header(&data).unwrap();
        let samples = read_samples(&data).unwrap();

        let files = plan(&samples, &data, header.osize);
        assert_eq!(files.len(), 27);

        let names: HashSet<_> = files.iter().map(|file| file.name.clone()).collect();
        assert_eq!(names.len(), 27, "names must be unique");

        let directory = output_directory(Path::new("out"), Path::new("in/2oscs.krz"), files.len());
        assert_eq!(directory, Path::new("out").join("2oscs"));
    }

    #[test]
    fn plans_stereo_file_with_note() {
        let Some(data) = fixture("oldbugs/HOUK2-021.wav.krz") else {
            eprintln!("skipping: fixtures not unpacked (run scripts/unpack_fixtures.py)");
            return;
        };
        let header = read_header(&data).unwrap();
        let samples = read_samples(&data).unwrap();

        let files = plan(&samples, &data, header.osize);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "houk2-021_C4.wav");
        assert!(files[0].is_stereo());

        let directory = output_directory(
            Path::new("out"),
            Path::new("in/HOUK2-021.wav.krz"),
            files.len(),
        );
        assert_eq!(directory, Path::new("out"));
    }

    #[test]
    fn skips_headers_without_data() {
        let data = bassman();
        let header = read_header(&data).unwrap();
        let mut samples = read_samples(&data).unwrap();
        samples[0].headers[0].flags = 0;

        assert!(plan(&samples, &data, header.osize).is_empty());
    }
}
