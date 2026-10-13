//! `krz_wav` — extract samples from Kurzweil K2x00 files (`.krz`/`.k25`/`.k26`) as WAV.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

mod krz;
mod output;
mod wav;

const BIN: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
Usage: krz_wav [OPTIONS] <KRZ_FILE> <OUTPUT_DIR>

Extract all samples from a Kurzweil K2x00 file (.krz/.k25/.k26) as WAV files.

Arguments:
  <KRZ_FILE>    Path to the source Kurzweil file
  <OUTPUT_DIR>  Directory to write the WAV files into

Options:
      --no-loop    Do not write loop points (smpl chunk) into the WAV files
      --floor      Use the exact floor(1e9 / samplePeriod) rate (KurzFiler
                   compatible) instead of snapping to standard sample rates
  -h, --help       Print this help and exit
      --version    Print the version and exit";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    input: PathBuf,
    output_dir: PathBuf,
    no_loop: bool,
    floor_rate: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Run(Args),
    Help,
    Version,
}

fn parse(raw: &[String]) -> Result<Parsed, String> {
    let mut input: Option<PathBuf> = None;
    let mut output_dir: Option<PathBuf> = None;
    let mut no_loop = false;
    let mut floor_rate = false;
    let mut positional_only = false;

    for arg in raw {
        if positional_only {
            push_positional(arg, &mut input, &mut output_dir)?;
            continue;
        }

        match arg.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--version" => return Ok(Parsed::Version),
            "--no-loop" => no_loop = true,
            "--floor" => floor_rate = true,
            "--" => positional_only = true,
            value if value.starts_with('-') && value != "-" => {
                return Err(format!("unknown option: {value}"));
            }
            value => push_positional(value, &mut input, &mut output_dir)?,
        }
    }

    let input = input.ok_or("missing <KRZ_FILE> argument")?;
    let output_dir = output_dir.ok_or("missing <OUTPUT_DIR> argument")?;

    Ok(Parsed::Run(Args {
        input,
        output_dir,
        no_loop,
        floor_rate,
    }))
}

fn push_positional(
    value: &str,
    input: &mut Option<PathBuf>,
    output_dir: &mut Option<PathBuf>,
) -> Result<(), String> {
    if input.is_none() {
        *input = Some(PathBuf::from(value));
    } else if output_dir.is_none() {
        *output_dir = Some(PathBuf::from(value));
    } else {
        return Err(format!("unexpected extra argument: {value}"));
    }
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = env::args().skip(1).collect();

    match parse(&raw) {
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Parsed::Version) => {
            println!("{BIN} {VERSION}");
            ExitCode::SUCCESS
        }
        Ok(Parsed::Run(args)) => run(&args),
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("Try '{BIN} --help' for more information.");
            ExitCode::from(1)
        }
    }
}

fn loop_info(sound: &krz::Soundfilehead, no_loop: bool) -> Option<wav::LoopInfo> {
    if no_loop || !sound.is_looped() {
        return None;
    }
    if sound.sample_loop_start < 0 || sound.sample_loop_start >= sound.sample_end {
        return None;
    }
    Some(wav::LoopInfo {
        rootkey: sound.rootkey,
        sample_period: sound.sample_period,
        start: sound.sample_loop_start as u32,
        end: sound.sample_end as u32,
    })
}

fn run(args: &Args) -> ExitCode {
    if !args.input.is_file() {
        eprintln!(
            "error: input file not found or not a file: {}",
            args.input.display()
        );
        return ExitCode::from(1);
    }

    let data = match fs::read(&args.input) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("error: cannot read {}: {error}", args.input.display());
            return ExitCode::from(1);
        }
    };

    let header = match krz::read_header(&data) {
        Ok(header) => header,
        Err(error) => {
            eprintln!("error: {}: {error}", args.input.display());
            return ExitCode::from(1);
        }
    };

    let samples = match krz::read_samples(&data) {
        Ok(samples) => samples,
        Err(error) => {
            eprintln!("error: {}: {error}", args.input.display());
            return ExitCode::from(1);
        }
    };

    let files = output::plan(&samples, &data, header.osize);
    let directory = output::output_directory(&args.output_dir, &args.input, files.len());

    // TODO(T07-T10): write the planned WAV files.
    println!("{BIN} {VERSION}");
    println!("input:      {}", args.input.display());
    println!("output dir: {}", directory.display());
    println!("samples: {}, files: {}", samples.len(), files.len());
    if files.is_empty() {
        println!("no WAV files to write");
        return ExitCode::SUCCESS;
    }

    if let Err(error) = fs::create_dir_all(&directory) {
        eprintln!("error: cannot create {}: {error}", directory.display());
        return ExitCode::from(1);
    }

    let mut written = 0usize;
    for file in &files {
        let target = directory.join(&file.name);

        let (channels, rate, pcm, loop_meta) = if file.is_stereo() {
            let left = krz::read_pcm_le(&data, &file.headers[0], header.osize);
            let right = krz::read_pcm_le(&data, &file.headers[1], header.osize);
            let (Some(left), Some(right)) = (left, right) else {
                continue;
            };
            if left.len() != right.len() {
                eprintln!(
                    "warning: {}: stereo channels differ in length, using the shorter one",
                    target.display()
                );
            }
            let rate = wav::sample_rate(file.headers[0].sample_period, !args.floor_rate);
            (
                2u16,
                rate,
                wav::interleave_stereo(&left, &right),
                loop_info(&file.headers[0], args.no_loop),
            )
        } else {
            let sound = file.headers[0];
            let Some(pcm) = krz::read_pcm_le(&data, &sound, header.osize) else {
                continue;
            };
            (
                1u16,
                wav::sample_rate(sound.sample_period, !args.floor_rate),
                pcm,
                loop_info(&sound, args.no_loop),
            )
        };

        let bytes = wav::build_wav(channels, rate, &pcm, loop_meta);
        if let Err(error) = fs::write(&target, &bytes) {
            eprintln!("error: cannot write {}: {error}", target.display());
            return ExitCode::from(1);
        }
        println!(
            "  wrote {} ({} channels, {} bytes, {rate} Hz)",
            target.display(),
            channels,
            bytes.len()
        );
        written += 1;
    }
    println!("wrote {written} of {} files", files.len());

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_two_positionals() {
        let parsed = parse(&args(&["in.krz", "out"])).unwrap();
        assert_eq!(
            parsed,
            Parsed::Run(Args {
                input: PathBuf::from("in.krz"),
                output_dir: PathBuf::from("out"),
                no_loop: false,
                floor_rate: false,
            })
        );
    }

    #[test]
    fn parses_flags() {
        let parsed = parse(&args(&["--no-loop", "--floor", "in.krz", "out"])).unwrap();
        assert_eq!(
            parsed,
            Parsed::Run(Args {
                input: PathBuf::from("in.krz"),
                output_dir: PathBuf::from("out"),
                no_loop: true,
                floor_rate: true,
            })
        );
    }

    #[test]
    fn parses_no_loop_flag() {
        let parsed = parse(&args(&["--no-loop", "in.krz", "out"])).unwrap();
        assert_eq!(
            parsed,
            Parsed::Run(Args {
                input: PathBuf::from("in.krz"),
                output_dir: PathBuf::from("out"),
                no_loop: true,
                floor_rate: false,
            })
        );
    }

    #[test]
    fn parses_help_and_version() {
        assert_eq!(parse(&args(&["--help"])).unwrap(), Parsed::Help);
        assert_eq!(parse(&args(&["-h"])).unwrap(), Parsed::Help);
        assert_eq!(parse(&args(&["--version"])).unwrap(), Parsed::Version);
    }

    #[test]
    fn rejects_missing_positionals() {
        assert!(parse(&args(&[])).is_err());
        assert!(parse(&args(&["in.krz"])).is_err());
    }

    #[test]
    fn rejects_extra_positionals() {
        let err = parse(&args(&["in.krz", "out", "extra"])).unwrap_err();
        assert!(err.contains("extra"));
    }

    #[test]
    fn rejects_unknown_option() {
        let err = parse(&args(&["--nope", "in.krz", "out"])).unwrap_err();
        assert!(err.contains("--nope"));
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let parsed = parse(&args(&["--", "--weird.krz", "out"])).unwrap();
        assert_eq!(
            parsed,
            Parsed::Run(Args {
                input: PathBuf::from("--weird.krz"),
                output_dir: PathBuf::from("out"),
                no_loop: false,
                floor_rate: false,
            })
        );
    }
}
