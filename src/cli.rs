use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "vidcapture", about = "CLI screen and audio recorder for macOS")]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start capturing screen and audio
    Start(StartArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct StartArgs {
    /// Capture duration (e.g., 10s, 2m, 1h30m)
    #[arg(short, long, value_parser = parse_duration)]
    pub duration: Option<Duration>,

    /// Interval mode — split into segments of this duration (e.g., 10s, 2m)
    #[arg(short, long, value_parser = parse_duration)]
    pub every: Option<Duration>,

    /// Output directory (default: current directory)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Show ffmpeg output (shortcut for RUST_LOG debug)
    #[arg(short, long)]
    pub verbose: bool,
}

/// Parse a human-readable duration string into a `std::time::Duration`.
///
/// Supported formats:
/// - `10s` → 10 seconds
/// - `2m` → 120 seconds
/// - `1h` → 3600 seconds
/// - `1h30m` → 5400 seconds
/// - `1h30m10s` → 5410 seconds
pub fn parse_duration(input: &str) -> Result<Duration, String> {
    if input.is_empty() {
        return Err("Duration cannot be empty".to_string());
    }

    let mut total_seconds: u64 = 0;
    let mut current_number = String::new();

    for ch in input.chars() {
        if ch.is_ascii_digit() {
            current_number.push(ch);
        } else {
            let value: u64 = current_number
                .parse()
                .map_err(|_| format!("Invalid number in duration: '{}'", current_number))?;

            match ch {
                's' => total_seconds += value,
                'm' => total_seconds += value * 60,
                'h' => total_seconds += value * 3600,
                _ => return Err(format!("Unknown duration unit: '{}'", ch)),
            }

            current_number.clear();
        }
    }

    if !current_number.is_empty() {
        return Err(format!(
            "Duration must end with a unit (s, m, or h), got: '{}'",
            current_number
        ));
    }

    if total_seconds == 0 {
        return Err("Duration cannot be zero".to_string());
    }

    Ok(Duration::from_secs(total_seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_simple_duration_seconds() {
        let duration = parse_duration("10s").unwrap();
        assert_eq!(duration, Duration::from_secs(10));
    }

    #[test]
    fn parse_simple_duration_minutes() {
        let duration = parse_duration("2m").unwrap();
        assert_eq!(duration, Duration::from_secs(120));
    }

    #[test]
    fn parse_simple_duration_hours() {
        let duration = parse_duration("1h").unwrap();
        assert_eq!(duration, Duration::from_secs(3600));
    }

    #[test]
    fn parse_compound_duration() {
        let duration = parse_duration("1h30m").unwrap();
        assert_eq!(duration, Duration::from_secs(5400));
    }

    #[test]
    fn parse_full_compound_duration() {
        let duration = parse_duration("1h30m10s").unwrap();
        assert_eq!(duration, Duration::from_secs(5410));
    }

    #[test]
    fn reject_empty_duration() {
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn reject_no_unit() {
        assert!(parse_duration("10").is_err());
    }

    #[test]
    fn reject_invalid_number() {
        assert!(parse_duration("abcs").is_err());
    }

    #[test]
    fn reject_unknown_unit() {
        assert!(parse_duration("10x").is_err());
    }

    #[test]
    fn reject_zero_duration() {
        assert!(parse_duration("0s").is_err());
    }

    #[test]
    fn parse_start_with_duration() {
        let args = Args::try_parse_from(["vidcapture", "start", "-d", "10s"]).unwrap();
        match args.command {
            Command::Start(start_args) => {
                assert_eq!(start_args.duration, Some(Duration::from_secs(10)));
                assert_eq!(start_args.every, None);
                assert_eq!(start_args.output, None);
                assert!(!start_args.verbose);
            }
        }
    }

    #[test]
    fn parse_start_defaults() {
        let args = Args::try_parse_from(["vidcapture", "start"]).unwrap();
        match args.command {
            Command::Start(start_args) => {
                assert_eq!(start_args.duration, None);
                assert_eq!(start_args.every, None);
                assert_eq!(start_args.output, None);
                assert!(!start_args.verbose);
            }
        }
    }

    #[test]
    fn parse_start_with_interval() {
        let args = Args::try_parse_from(["vidcapture", "start", "-e", "30s"]).unwrap();
        match args.command {
            Command::Start(start_args) => {
                assert_eq!(start_args.duration, None);
                assert_eq!(start_args.every, Some(Duration::from_secs(30)));
            }
        }
    }

    #[test]
    fn parse_start_with_all_flags() {
        let args = Args::try_parse_from([
            "vidcapture",
            "start",
            "-d",
            "1m",
            "-e",
            "10s",
            "-o",
            "/tmp/output",
            "-v",
        ])
        .unwrap();
        match args.command {
            Command::Start(start_args) => {
                assert_eq!(start_args.duration, Some(Duration::from_secs(60)));
                assert_eq!(start_args.every, Some(Duration::from_secs(10)));
                assert_eq!(start_args.output, Some(PathBuf::from("/tmp/output")));
                assert!(start_args.verbose);
            }
        }
    }
}
