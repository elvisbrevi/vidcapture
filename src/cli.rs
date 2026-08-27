use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "vidcapture",
    about = "CLI screen and audio recorder for macOS",
    disable_help_subcommand = true
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start capturing screen and audio
    Start(StartArgs),
    /// Cut a range out of an existing video file
    Cut(CutArgs),
    /// Show help with setup instructions
    Help,
}

#[derive(Parser, Debug, Clone)]
pub struct StartArgs {
    /// Capture duration (e.g., 10s, 1.5s, 1500ms, 1h30m, 00:01:30.500)
    #[arg(short, long, value_parser = parse_positive_timespec)]
    pub duration: Option<Duration>,

    /// Interval mode — split into segments of this duration (e.g., 10s, 2m)
    #[arg(short, long, value_parser = parse_positive_timespec)]
    pub every: Option<Duration>,

    /// Output directory (default: current directory)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Show ffmpeg output (shortcut for RUST_LOG debug)
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Parser, Debug, Clone)]
pub struct CutArgs {
    /// Path to the source video file
    pub source: PathBuf,

    /// Start offset of the cut range (default: 0s)
    #[arg(short, long, value_parser = parse_timespec, default_value = "0s")]
    pub from: Duration,

    /// End offset of the cut range (conflicts with --length)
    #[arg(short, long, value_parser = parse_positive_timespec, conflicts_with = "length")]
    pub to: Option<Duration>,

    /// Cut length (conflicts with --to)
    #[arg(short, long, value_parser = parse_positive_timespec, conflicts_with = "to")]
    pub length: Option<Duration>,

    /// Output file or directory
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Stream-copy instead of re-encoding
    #[arg(long)]
    pub fast: bool,

    /// Show ffmpeg output
    #[arg(short, long)]
    pub verbose: bool,
}

/// Validated cut range: a start offset and a length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutRange {
    pub start: Duration,
    pub length: Duration,
}

impl CutArgs {
    /// Validate the cut arguments and return a normalized cut range.
    ///
    /// Returns an error if:
    /// - Neither `--to` nor `--length` is provided
    /// - `--from` >= `--to`
    /// - `--length` is zero
    pub fn validate_cut_range(&self) -> Result<CutRange, String> {
        let length = match (self.to, self.length) {
            (None, None) => {
                return Err(
                    "Neither --to nor --length specified. \
                     One of them is required."
                        .to_string(),
                );
            }
            (Some(to), None) => {
                if self.from >= to {
                    return Err(format!(
                        "--from ({}) must be less than --to ({})",
                        format_duration(self.from),
                        format_duration(to),
                    ));
                }
                to - self.from
            }
            (None, Some(len)) => {
                if len.is_zero() {
                    return Err("--length must be greater than zero".to_string());
                }
                len
            }
            // Both --to and --length: clap's conflicts_with prevents this.
            (Some(_), Some(_)) => unreachable!("clap conflicts_with should have rejected this"),
        };

        Ok(CutRange {
            start: self.from,
            length,
        })
    }
}

/// Format a duration for user-facing error messages.
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs.fract() == 0.0 && secs < 60.0 {
        format!("{}s", secs as u64)
    } else {
        format!("{:.3}s", secs)
    }
}

/// Parse a timespec into a `std::time::Duration`, at millisecond precision.
///
/// Two notations are accepted:
/// - Unit-suffixed: a sequence of `<number><unit>` pairs, units `ms`, `s`,
///   `m`, `h`, with an optional decimal point. E.g. `10s`, `1500ms`, `1.5s`,
///   `1h30m10s`.
/// - Timestamp: `HH:MM:SS[.mmm]` or `MM:SS[.mmm]`. E.g. `00:01:30.500`,
///   `01:30`, `1:02:03.250`.
///
/// `0s` parses successfully — callers that require a positive value should
/// use [`parse_positive_timespec`] instead.
pub fn parse_timespec(input: &str) -> Result<Duration, String> {
    if input.is_empty() {
        return Err("Timespec cannot be empty".to_string());
    }

    if input.contains(':') {
        parse_timestamp(input)
    } else {
        parse_unit_suffixed(input)
    }
}

/// Like [`parse_timespec`], but rejects a zero duration. Use this for flags
/// where a zero-length value is meaningless (`-d`, `-e`, `--length`).
pub fn parse_positive_timespec(input: &str) -> Result<Duration, String> {
    let duration = parse_timespec(input)?;
    if duration.is_zero() {
        return Err(format!(
            "Timespec must be greater than zero, got: '{}'",
            input
        ));
    }
    Ok(duration)
}

fn parse_unit_suffixed(input: &str) -> Result<Duration, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut total_millis: f64 = 0.0;

    while i < chars.len() {
        let number_start = i;
        let mut seen_dot = false;
        while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && !seen_dot)) {
            seen_dot = seen_dot || chars[i] == '.';
            i += 1;
        }
        if i == number_start {
            return Err(format!("Invalid timespec: '{}'", input));
        }

        let number_str: String = chars[number_start..i].iter().collect();
        let value: f64 = number_str
            .parse()
            .map_err(|_| format!("Invalid number in timespec: '{}'", number_str))?;

        if i + 1 < chars.len() && chars[i] == 'm' && chars[i + 1] == 's' {
            total_millis += value;
            i += 2;
        } else if i < chars.len() {
            let unit = chars[i];
            i += 1;
            match unit {
                's' => total_millis += value * 1_000.0,
                'm' => total_millis += value * 60_000.0,
                'h' => total_millis += value * 3_600_000.0,
                _ => return Err(format!("Unknown timespec unit: '{}'", unit)),
            }
        } else {
            return Err(format!(
                "Timespec must end with a unit (ms, s, m, or h), got: '{}'",
                input
            ));
        }
    }

    Ok(Duration::from_millis(total_millis.round() as u64))
}

fn parse_timestamp(input: &str) -> Result<Duration, String> {
    let parts: Vec<&str> = input.split(':').collect();
    let (hours, minutes, seconds_str) = match parts.as_slice() {
        [h, m, s] => (
            parse_timestamp_int(h, input)?,
            parse_timestamp_int(m, input)?,
            *s,
        ),
        [m, s] => (0, parse_timestamp_int(m, input)?, *s),
        _ => return Err(format!("Invalid timestamp: '{}'", input)),
    };

    if !is_valid_decimal(seconds_str) {
        return Err(format!("Invalid timestamp: '{}'", input));
    }
    let seconds: f64 = seconds_str
        .parse()
        .map_err(|_| format!("Invalid timestamp: '{}'", input))?;

    if minutes >= 60 {
        return Err(format!(
            "Invalid timestamp: '{}' (minutes must be < 60)",
            input
        ));
    }
    if seconds >= 60.0 {
        return Err(format!(
            "Invalid timestamp: '{}' (seconds must be < 60)",
            input
        ));
    }

    let millis = hours * 3_600_000 + minutes * 60_000 + (seconds * 1_000.0).round() as u64;
    Ok(Duration::from_millis(millis))
}

fn parse_timestamp_int(component: &str, original: &str) -> Result<u64, String> {
    if component.is_empty() || !component.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid timestamp: '{}'", original));
    }
    component
        .parse()
        .map_err(|_| format!("Invalid timestamp: '{}'", original))
}

fn is_valid_decimal(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    for (idx, ch) in s.chars().enumerate() {
        if ch == '.' {
            if seen_dot || idx == 0 {
                return false;
            }
            seen_dot = true;
        } else if !ch.is_ascii_digit() {
            return false;
        }
    }
    !s.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_timespec_unit_suffixed_table() {
        let cases: &[(&str, u64)] = &[
            ("1500ms", 1_500),
            ("1.5s", 1_500),
            ("10s", 10_000),
            ("2m", 120_000),
            ("1h", 3_600_000),
            ("1h30m10s", 5_410_000),
            ("0.25m", 15_000),
            ("1h30m", 5_400_000),
            ("0s", 0),
        ];
        for (input, expected_millis) in cases {
            let duration = parse_timespec(input)
                .unwrap_or_else(|e| panic!("expected '{}' to parse, got error: {}", input, e));
            assert_eq!(
                duration,
                Duration::from_millis(*expected_millis),
                "input: '{}'",
                input
            );
        }
    }

    #[test]
    fn parse_timespec_timestamp_table() {
        let cases: &[(&str, u64)] = &[
            ("00:01:30.500", 90_500),
            ("01:30", 90_000),
            ("1:02:03.250", 3_723_250),
        ];
        for (input, expected_millis) in cases {
            let duration = parse_timespec(input)
                .unwrap_or_else(|e| panic!("expected '{}' to parse, got error: {}", input, e));
            assert_eq!(
                duration,
                Duration::from_millis(*expected_millis),
                "input: '{}'",
                input
            );
        }
    }

    #[test]
    fn parse_timespec_ms_not_confused_with_minutes() {
        // 1500ms must be 1.5 seconds, not 1500 minutes.
        let duration = parse_timespec("1500ms").unwrap();
        assert_eq!(duration, Duration::from_millis(1_500));
        assert_ne!(duration, Duration::from_secs(1500 * 60));
    }

    #[test]
    fn parse_timespec_rounds_sub_millisecond_fractions() {
        let duration = parse_timespec("1.0004s").unwrap();
        assert_eq!(duration, Duration::from_millis(1_000));

        let duration = parse_timespec("1.0006s").unwrap();
        assert_eq!(duration, Duration::from_millis(1_001));
    }

    #[test]
    fn parse_timespec_rejects_invalid_input() {
        let cases = ["", "10", "10x", "abcs"];
        for input in cases {
            assert!(
                parse_timespec(input).is_err(),
                "expected '{}' to be rejected",
                input
            );
        }
    }

    #[test]
    fn parse_timespec_zero_is_accepted() {
        assert_eq!(parse_timespec("0s").unwrap(), Duration::ZERO);
    }

    #[test]
    fn parse_positive_timespec_rejects_zero() {
        for input in ["0s", "0ms", "00:00:00"] {
            assert!(
                parse_positive_timespec(input).is_err(),
                "expected '{}' to be rejected as non-positive",
                input
            );
        }
    }

    #[test]
    fn parse_positive_timespec_accepts_nonzero() {
        assert_eq!(
            parse_positive_timespec("1.5s").unwrap(),
            Duration::from_millis(1_500)
        );
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
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_start_with_fractional_duration() {
        let args = Args::try_parse_from(["vidcapture", "start", "-d", "1.5s"]).unwrap();
        match args.command {
            Command::Start(start_args) => {
                assert_eq!(start_args.duration, Some(Duration::from_millis(1_500)));
            }
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_start_rejects_zero_duration() {
        let result = Args::try_parse_from(["vidcapture", "start", "-d", "0s"]);
        assert!(result.is_err(), "expected -d 0s to be rejected");
    }

    #[test]
    fn parse_start_rejects_zero_interval() {
        let result = Args::try_parse_from(["vidcapture", "start", "-e", "0s"]);
        assert!(result.is_err(), "expected -e 0s to be rejected");
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
            _ => panic!("expected Start command"),
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
            _ => panic!("expected Start command"),
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
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn parse_help_subcommand() {
        let args = Args::try_parse_from(["vidcapture", "help"]).unwrap();
        assert!(matches!(args.command, Command::Help));
    }

    #[test]
    fn parse_no_subcommand_shows_help() {
        // clap's default behavior is to print help and exit when no subcommand is provided.
        let result = Args::try_parse_from(["vidcapture"]);
        assert!(result.is_err(), "expected error when no subcommand given");
    }

    // ---- cut subcommand tests (issue #17) ----

    #[test]
    fn parse_cut_with_from_and_to() {
        let args = Args::try_parse_from(["vidcapture", "cut", "talk.mp4", "--from", "10s", "--to", "25s"]).unwrap();
        match args.command {
            Command::Cut(cut_args) => {
                assert_eq!(cut_args.from, Duration::from_secs(10));
                assert_eq!(cut_args.to, Some(Duration::from_secs(25)));
                assert_eq!(cut_args.length, None);
            }
            _ => panic!("expected Cut command"),
        }
    }

    #[test]
    fn parse_cut_with_from_and_length() {
        let args = Args::try_parse_from(["vidcapture", "cut", "talk.mp4", "--from", "10s", "--length", "1500ms"]).unwrap();
        match args.command {
            Command::Cut(cut_args) => {
                assert_eq!(cut_args.from, Duration::from_secs(10));
                assert_eq!(cut_args.to, None);
                assert_eq!(cut_args.length, Some(Duration::from_millis(1500)));
            }
            _ => panic!("expected Cut command"),
        }
    }

    #[test]
    fn parse_cut_defaults_from_to_zero() {
        let args = Args::try_parse_from(["vidcapture", "cut", "talk.mp4", "--length", "5s"]).unwrap();
        match args.command {
            Command::Cut(cut_args) => {
                assert_eq!(cut_args.from, Duration::ZERO);
                assert_eq!(cut_args.length, Some(Duration::from_secs(5)));
            }
            _ => panic!("expected Cut command"),
        }
    }

    #[test]
    fn parse_cut_short_flags() {
        let args = Args::try_parse_from(["vidcapture", "cut", "talk.mp4", "-f", "10s", "-t", "25s"]).unwrap();
        match args.command {
            Command::Cut(cut_args) => {
                assert_eq!(cut_args.from, Duration::from_secs(10));
                assert_eq!(cut_args.to, Some(Duration::from_secs(25)));
            }
            _ => panic!("expected Cut command"),
        }
    }

    #[test]
    fn parse_cut_both_to_and_length_errors() {
        let result = Args::try_parse_from([
            "vidcapture", "cut", "talk.mp4", "--to", "25s", "--length", "5s",
        ]);
        assert!(result.is_err(), "expected --to and --length to conflict");
    }

    #[test]
    fn parse_cut_with_output_and_verbose() {
        let args = Args::try_parse_from([
            "vidcapture", "cut", "talk.mp4", "--length", "5s", "-o", "out.mp4", "-v", "--fast",
        ])
        .unwrap();
        match args.command {
            Command::Cut(cut_args) => {
                assert_eq!(cut_args.output, Some(PathBuf::from("out.mp4")));
                assert!(cut_args.verbose);
                assert!(cut_args.fast);
            }
            _ => panic!("expected Cut command"),
        }
    }

    #[test]
    fn cut_validate_neither_to_nor_length_errors() {
        let cut_args = CutArgs {
            source: PathBuf::from("talk.mp4"),
            from: Duration::ZERO,
            to: None,
            length: None,
            output: None,
            fast: false,
            verbose: false,
        };
        assert!(cut_args.validate_cut_range().is_err());
    }

    #[test]
    fn cut_validate_from_greater_than_to_errors() {
        let cut_args = CutArgs {
            source: PathBuf::from("talk.mp4"),
            from: Duration::from_secs(25),
            to: Some(Duration::from_secs(10)),
            length: None,
            output: None,
            fast: false,
            verbose: false,
        };
        assert!(cut_args.validate_cut_range().is_err());
    }

    #[test]
    fn cut_validate_from_equals_to_errors() {
        let cut_args = CutArgs {
            source: PathBuf::from("talk.mp4"),
            from: Duration::from_secs(10),
            to: Some(Duration::from_secs(10)),
            length: None,
            output: None,
            fast: false,
            verbose: false,
        };
        assert!(cut_args.validate_cut_range().is_err());
    }

    #[test]
    fn cut_validate_zero_length_errors() {
        let cut_args = CutArgs {
            source: PathBuf::from("talk.mp4"),
            from: Duration::ZERO,
            to: None,
            length: Some(Duration::ZERO),
            output: None,
            fast: false,
            verbose: false,
        };
        assert!(cut_args.validate_cut_range().is_err());
    }

    #[test]
    fn cut_validate_from_to_range() {
        let cut_args = CutArgs {
            source: PathBuf::from("talk.mp4"),
            from: Duration::from_secs(10),
            to: Some(Duration::from_secs(25)),
            length: None,
            output: None,
            fast: false,
            verbose: false,
        };
        let range = cut_args.validate_cut_range().unwrap();
        assert_eq!(range.start, Duration::from_secs(10));
        assert_eq!(range.length, Duration::from_secs(15));
    }

    #[test]
    fn cut_validate_from_length_range() {
        let cut_args = CutArgs {
            source: PathBuf::from("talk.mp4"),
            from: Duration::from_secs(10),
            to: None,
            length: Some(Duration::from_millis(1500)),
            output: None,
            fast: false,
            verbose: false,
        };
        let range = cut_args.validate_cut_range().unwrap();
        assert_eq!(range.start, Duration::from_secs(10));
        assert_eq!(range.length, Duration::from_millis(1500));
    }

    #[test]
    fn cut_validate_defaults_from_zero_with_length() {
        let cut_args = CutArgs {
            source: PathBuf::from("talk.mp4"),
            from: Duration::ZERO,
            to: None,
            length: Some(Duration::from_secs(5)),
            output: None,
            fast: false,
            verbose: false,
        };
        let range = cut_args.validate_cut_range().unwrap();
        assert_eq!(range.start, Duration::ZERO);
        assert_eq!(range.length, Duration::from_secs(5));
    }
}
