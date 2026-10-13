//! End-to-end tests: run the built `krz_wav` binary on the sample fixtures.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_krz_wav"))
}

fn project(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("failed to run krz_wav")
}

fn fresh_dir(name: &str) -> PathBuf {
    let dir = project(&format!(".tmp/it/{name}"));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove old test dir");
    }
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn wav_chunk<'a>(wav: &'a [u8], id: &[u8; 4]) -> Option<&'a [u8]> {
    let mut offset = 12;
    while offset + 8 <= wav.len() {
        let chunk_id = &wav[offset..offset + 4];
        let size = u32::from_le_bytes([
            wav[offset + 4],
            wav[offset + 5],
            wav[offset + 6],
            wav[offset + 7],
        ]) as usize;
        if chunk_id == id {
            return Some(&wav[offset + 8..offset + 8 + size]);
        }
        offset += 8 + size + (size & 1);
    }
    None
}

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

fn wav_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
        .collect();
    files.sort();
    files
}

fn fixture(path: &str) -> Option<PathBuf> {
    let path = project(&format!(".tmp/fixtures/{path}"));
    if path.is_file() {
        Some(path)
    } else {
        eprintln!("skipping: fixtures not unpacked (run scripts/unpack_fixtures.py)");
        None
    }
}

#[test]
fn extracts_single_sample_into_output_dir() {
    let out = fresh_dir("bassman");
    let output = run(&[
        project("bass/bassman.krz").to_str().unwrap(),
        out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let file = out.join("UNNAMED WS.wav");
    assert!(file.is_file(), "expected {}", file.display());

    let wav = fs::read(&file).unwrap();
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(u16_at(&wav, 22), 1); // mono
    assert_eq!(u32_at(&wav, 24), 39100);
    assert!(wav_chunk(&wav, b"smpl").is_some()); // looped sample

    let reference = fs::read(project("bass/bassman.wav 1 C3.wav")).unwrap();
    assert_eq!(
        wav_chunk(&wav, b"data").unwrap(),
        wav_chunk(&reference, b"data").unwrap()
    );
}

#[test]
fn extracts_multiple_samples_into_subdirectory() {
    let Some(krz) = fixture("2oscs.krz") else {
        return;
    };

    let out = fresh_dir("2oscs");
    let output = run(&[krz.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(output.status.success());

    let dir = out.join("2oscs");
    assert!(dir.is_dir(), "expected sub-directory {}", dir.display());

    let files = wav_files(&dir);
    assert_eq!(files.len(), 27);
    let unique: std::collections::HashSet<_> = files.iter().collect();
    assert_eq!(unique.len(), 27, "file names must be unique");
}

#[test]
fn extracts_stereo_sample() {
    let Some(krz) = fixture("oldbugs/HOUK2-021.wav.krz") else {
        return;
    };

    let out = fresh_dir("houk2");
    let output = run(&[krz.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(output.status.success());

    let file = out.join("houk2-021_C4.wav");
    assert!(file.is_file(), "expected {}", file.display());

    let wav = fs::read(&file).unwrap();
    assert_eq!(u16_at(&wav, 22), 2); // stereo
    assert_eq!(u32_at(&wav, 24), 44100); // snapped standard rate
    let data = wav_chunk(&wav, b"data").unwrap();
    assert_eq!(data.len(), 549264);

    let reference = fs::read(project(".tmp/fixtures/oldbugs/HOUK2-021.wav")).unwrap();
    assert_eq!(data, wav_chunk(&reference, b"data").unwrap());
}

#[test]
fn floor_flag_uses_kurzfiler_rate() {
    let Some(krz) = fixture("oldbugs/HOUK2-021.wav.krz") else {
        return;
    };

    let out = fresh_dir("floor");
    let output = run(&["--floor", krz.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(output.status.success());

    let file = out.join("houk2-021_C4.wav");
    let wav = fs::read(&file).unwrap();
    assert_eq!(u32_at(&wav, 24), 44101); // exact floor(1e9 / 22675)
}

#[test]
fn no_loop_flag_removes_smpl_chunk() {
    let Some(krz) = fixture("Tralla16BitMonoLoop.wav.krz") else {
        return;
    };

    let with_loop = fresh_dir("loop");
    let output = run(&[krz.to_str().unwrap(), with_loop.to_str().unwrap()]);
    assert!(output.status.success());
    let file = wav_files(&with_loop).remove(0);
    assert!(wav_chunk(&fs::read(&file).unwrap(), b"smpl").is_some());

    let without_loop = fresh_dir("noloop");
    let output = run(&[
        "--no-loop",
        krz.to_str().unwrap(),
        without_loop.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    let file = wav_files(&without_loop).remove(0);
    assert!(wav_chunk(&fs::read(&file).unwrap(), b"smpl").is_none());
}

#[test]
fn fails_on_missing_input_file() {
    let out = fresh_dir("missing");
    let output = run(&["no-such-file.krz", out.to_str().unwrap()]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("input file not found"));
}
