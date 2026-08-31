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
    /// Draw timed text labels onto an existing video file
    Label(LabelArgs),
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
        let (start, length) =
            resolve_start_and_length(self.from, self.to, self.length, &RangeSpelling::CUT_FLAGS)?;
        Ok(CutRange { start, length })
    }
}

/// How the user spells the three range options, so a validation error names
/// what they typed: `--from` on `cut`, `from=` inside a label spec.
struct RangeSpelling {
    from: &'static str,
    to: &'static str,
    length: &'static str,
}

impl RangeSpelling {
    const CUT_FLAGS: Self = Self {
        from: "--from",
        to: "--to",
        length: "--length",
    };
    const LABEL_KEYS: Self = Self {
        from: "from=",
        to: "to=",
        length: "length=",
    };
}

/// Resolve a start offset plus either an end offset or a length into the
/// `(start, length)` pair ffmpeg is given.
///
/// A cut range and a label window are validated by the same three rules — one
/// of the two ends is required, the start must precede the end, and a length
/// must be positive — so both go through here.
fn resolve_start_and_length(
    from: Duration,
    to: Option<Duration>,
    length: Option<Duration>,
    spelling: &RangeSpelling,
) -> Result<(Duration, Duration), String> {
    match (to, length) {
        (None, None) => Err(format!(
            "Neither {} nor {} specified. One of them is required.",
            spelling.to, spelling.length
        )),
        (Some(to), None) => {
            if from >= to {
                return Err(format!(
                    "{} ({}) must be less than {} ({})",
                    spelling.from,
                    format_duration(from),
                    spelling.to,
                    format_duration(to),
                ));
            }
            Ok((from, to - from))
        }
        (None, Some(len)) => {
            if len.is_zero() {
                return Err(format!("{} must be greater than zero", spelling.length));
            }
            Ok((from, len))
        }
        // On `cut` clap's conflicts_with rejects this first; a label spec has
        // no clap guard, so the rule is enforced here.
        (Some(_), Some(_)) => Err(format!(
            "{} and {} are mutually exclusive.",
            spelling.to, spelling.length
        )),
    }
}

#[derive(Parser, Debug, Clone)]
pub struct LabelArgs {
    /// Path to the source video file
    pub source: PathBuf,

    /// A label spec — repeat -l for each label
    /// (e.g. "text=Intro,from=1m32s,to=2m,position=top")
    #[arg(short = 'l', long = "label", value_name = "SPEC", required = true,
          value_parser = parse_label_spec)]
    pub labels: Vec<Label>,

    /// Font file to draw labels with (default: the system font)
    #[arg(long, value_name = "PATH")]
    pub font: Option<PathBuf>,

    /// Output file or directory
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Show ffmpeg output
    #[arg(short, long)]
    pub verbose: bool,
}

/// Where in the frame a label is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelPosition {
    Top,
    #[default]
    Bottom,
}

/// Text color used when a label spec does not set `color=`.
const DEFAULT_LABEL_COLOR: &str = "white";

/// Font size, in pixels, used when a label spec does not set `size=`.
const DEFAULT_LABEL_SIZE: u32 = 32;

/// Label background used when a label spec does not set `background=`.
///
/// A label is captioning footage nobody has seen yet, so the readable thing —
/// light text on a dark band — is what a spec that says nothing about styling
/// gets. `background=none` draws the text bare instead.
const DEFAULT_LABEL_BACKGROUND: &str = "black@0.5";

/// The `background=` value that turns the band off, rather than naming a color
/// for it.
const NO_LABEL_BACKGROUND: &str = "none";

/// One validated label: its text, the label window it is visible for, and how
/// it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub text: String,
    /// Start of the label window, measured from the beginning of the source
    /// video.
    pub start: Duration,
    /// How long the label window lasts.
    pub length: Duration,
    pub position: LabelPosition,
    /// Text color, in ffmpeg's color syntax.
    pub color: String,
    /// Font size in pixels.
    pub size: u32,
    /// Label background drawn behind the text, in ffmpeg's color syntax.
    /// `None` draws the text bare, which a spec asks for with
    /// `background=none`.
    pub background: Option<String>,
}

impl Label {
    /// End of the label window, measured from the beginning of the source
    /// video.
    pub fn end(&self) -> Duration {
        self.start + self.length
    }
}

/// Parse one label spec — the value of a single `-l/--label` flag.
///
/// A label spec is a comma-separated list of `key=value` pairs. Inside it
/// `\,` is a literal comma and `\\` a literal backslash, so a label's text may
/// contain either. Keys and values are trimmed. Recognised keys:
///
/// - `text` (required) — the text drawn on the video.
/// - `from` — start of the label window (default `0s`).
/// - `to` — end of the label window. Conflicts with `length`.
/// - `length` — how long the label window lasts. Conflicts with `to`.
/// - `position` — `top` or `bottom` (default `bottom`).
/// - `color` — text color (default `white`).
/// - `size` — font size in pixels (default `32`).
/// - `background` — color of the band drawn behind the text
///   (default `black@0.5`; `none` draws the text bare).
///
/// Every time-valued key takes the same timespec the rest of the CLI does.
pub fn parse_label_spec(input: &str) -> Result<Label, String> {
    let mut text: Option<String> = None;
    let mut from = Duration::ZERO;
    let mut to: Option<Duration> = None;
    let mut length: Option<Duration> = None;
    let mut position = LabelPosition::default();
    let mut color = DEFAULT_LABEL_COLOR.to_string();
    let mut size = DEFAULT_LABEL_SIZE;
    let mut background = Some(DEFAULT_LABEL_BACKGROUND.to_string());
    let mut seen: Vec<String> = Vec::new();

    for pair in split_label_pairs(input) {
        if pair.trim().is_empty() {
            continue;
        }
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("Label spec entry is not key=value: '{}'", pair.trim()))?;
        let key = key.trim().to_lowercase();
        let value = value.trim();

        if seen.contains(&key) {
            return Err(format!("Duplicate key in label spec: '{}='", key));
        }
        seen.push(key.clone());

        match key.as_str() {
            "text" => {
                if value.is_empty() {
                    return Err("text= must not be empty".to_string());
                }
                text = Some(value.to_string());
            }
            "from" => from = parse_timespec(value)?,
            "to" => to = Some(parse_positive_timespec(value)?),
            "length" => length = Some(parse_positive_timespec(value)?),
            "position" => position = parse_label_position(value)?,
            "color" => color = parse_ffmpeg_color(value, "color")?,
            "background" => background = parse_label_background(value)?,
            "size" => {
                size = value.parse::<u32>().map_err(|_| {
                    format!("Invalid size= value: '{}'. Expected pixels, e.g. 32.", value)
                })?;
                if size == 0 {
                    return Err("size= must be greater than zero".to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Unknown key in label spec: '{}='. Valid keys: text, from, to, \
                     length, position, color, size, background.",
                    key
                ));
            }
        }
    }

    let text = text.ok_or("Label spec is missing text=")?;
    let (start, length) = resolve_start_and_length(from, to, length, &RangeSpelling::LABEL_KEYS)?;

    Ok(Label {
        text,
        start,
        length,
        position,
        color,
        size,
        background,
    })
}

/// Split a label spec into its `key=value` entries on unescaped commas,
/// resolving `\,` to a comma and `\\` to a backslash as it goes.
///
/// A backslash before anything else is kept verbatim, so a Windows-style path
/// or a stray backslash in a label's text survives unchanged.
fn split_label_pairs(input: &str) -> Vec<String> {
    let mut pairs = vec![String::new()];
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        let current = pairs.last_mut().expect("pairs is never empty");
        match ch {
            '\\' => match chars.peek() {
                Some(',') | Some('\\') => current.push(chars.next().expect("peeked")),
                _ => current.push('\\'),
            },
            ',' => pairs.push(String::new()),
            _ => current.push(ch),
        }
    }

    pairs
}

/// Parse a `background=` value: a color for the band, or `none` to draw the
/// label's text with no band behind it at all.
fn parse_label_background(value: &str) -> Result<Option<String>, String> {
    if value.eq_ignore_ascii_case(NO_LABEL_BACKGROUND) {
        return Ok(None);
    }
    Ok(Some(parse_ffmpeg_color(value, "background")?))
}

fn parse_label_position(value: &str) -> Result<LabelPosition, String> {
    match value.to_lowercase().as_str() {
        "top" => Ok(LabelPosition::Top),
        "bottom" => Ok(LabelPosition::Bottom),
        _ => Err(format!(
            "Invalid position= value: '{}'. Expected top or bottom.",
            value
        )),
    }
}

/// Accept a color in ffmpeg's syntax: a name (`white`) or a hex triplet
/// (`#RRGGBB`), either optionally carrying an `@<alpha>` suffix.
///
/// The charset is restricted rather than fully parsed. That is enough to catch
/// typos, and it keeps a label spec from smuggling `:` or `'` into the
/// filtergraph, where they would be read as filter syntax rather than a color.
fn parse_ffmpeg_color(value: &str, key: &str) -> Result<String, String> {
    if value.is_empty() {
        return Err(format!("{}= must not be empty", key));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '#' | '@' | '.' | '_' | '-'))
    {
        return Err(format!(
            "Invalid {}= value: '{}'. Expected a color name (white), a hex color \
             (#RRGGBB), optionally with an alpha suffix (black@0.5).",
            key, value
        ));
    }
    Ok(value.to_string())
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

    // ---- label subcommand tests ----

    fn label_from(spec: &str) -> Label {
        parse_label_spec(spec)
            .unwrap_or_else(|e| panic!("expected '{}' to parse, got error: {}", spec, e))
    }

    #[test]
    fn parse_label_spec_reads_every_key() {
        let label = label_from(
            "text=Intro,from=1m32s,to=2m,position=top,color=#ffcc00,size=48,background=black@0.5",
        );
        assert_eq!(label.text, "Intro");
        assert_eq!(label.start, Duration::from_millis(92_000));
        assert_eq!(label.length, Duration::from_millis(28_000));
        assert_eq!(label.end(), Duration::from_millis(120_000));
        assert_eq!(label.position, LabelPosition::Top);
        assert_eq!(label.color, "#ffcc00");
        assert_eq!(label.size, 48);
        assert_eq!(label.background.as_deref(), Some("black@0.5"));
    }

    #[test]
    fn parse_label_spec_defaults_everything_but_the_label_window() {
        let label = label_from("text=Hello,to=5s");
        assert_eq!(label.start, Duration::ZERO);
        assert_eq!(label.length, Duration::from_secs(5));
        assert_eq!(label.position, LabelPosition::Bottom);
        assert_eq!(label.color, "white");
        assert_eq!(label.size, 32);
        assert_eq!(label.background.as_deref(), Some("black@0.5"));
    }

    /// Styling is optional, but a bare label spec is still readable: it gets
    /// white text at 32px on a dark band, not raw text on the footage.
    #[test]
    fn parse_label_spec_defaults_to_readable_styling() {
        let label = label_from("text=Hello,to=5s");
        assert_eq!(label.color, DEFAULT_LABEL_COLOR);
        assert_eq!(label.size, DEFAULT_LABEL_SIZE);
        assert_eq!(label.background.as_deref(), Some(DEFAULT_LABEL_BACKGROUND));
    }

    #[test]
    fn parse_label_spec_background_none_draws_the_text_bare() {
        for value in ["none", "NONE", "None"] {
            let label = label_from(&format!("text=Hello,to=5s,background={}", value));
            assert_eq!(label.background, None, "background={}", value);
        }
    }

    #[test]
    fn parse_label_spec_accepts_length_instead_of_to() {
        let label = label_from("text=Hello,from=10s,length=1500ms");
        assert_eq!(label.start, Duration::from_secs(10));
        assert_eq!(label.length, Duration::from_millis(1_500));
    }

    /// The label window takes the same timespec as every other time-valued
    /// flag, in both notations.
    #[test]
    fn parse_label_spec_accepts_both_timespec_notations() {
        let label = label_from("text=Hello,from=00:01:30.500,to=00:02:00");
        assert_eq!(label.start, Duration::from_millis(90_500));
        assert_eq!(label.end(), Duration::from_millis(120_000));
    }

    #[test]
    fn parse_label_spec_keeps_an_escaped_comma_in_the_text() {
        let label = label_from(r"text=Intro\, part one,to=5s");
        assert_eq!(label.text, "Intro, part one");
    }

    #[test]
    fn parse_label_spec_keeps_an_escaped_backslash_in_the_text() {
        let label = label_from(r"text=back\\slash,to=5s");
        assert_eq!(label.text, r"back\slash");
    }

    /// A backslash that escapes nothing the spec defines is part of the text,
    /// so a path or a stray backslash survives without being doubled.
    #[test]
    fn parse_label_spec_keeps_a_lone_backslash_in_the_text() {
        let label = label_from(r"text=C:\Users,to=5s");
        assert_eq!(label.text, r"C:\Users");
    }

    #[test]
    fn parse_label_spec_trims_whitespace_around_keys_and_values() {
        let label = label_from("text = Hello , from = 1s , to = 5s , position = TOP");
        assert_eq!(label.text, "Hello");
        assert_eq!(label.start, Duration::from_secs(1));
        assert_eq!(label.position, LabelPosition::Top);
    }

    #[test]
    fn parse_label_spec_keeps_an_equals_sign_inside_the_text() {
        let label = label_from("text=a=b,to=5s");
        assert_eq!(label.text, "a=b");
    }

    #[test]
    fn parse_label_spec_rejects_a_spec_without_text() {
        let error = parse_label_spec("from=1s,to=5s").unwrap_err();
        assert!(
            error.contains("text="),
            "error should name the missing key, got: {}",
            error
        );
    }

    #[test]
    fn parse_label_spec_rejects_an_empty_text() {
        assert!(parse_label_spec("text=,to=5s").is_err());
    }

    #[test]
    fn parse_label_spec_rejects_a_spec_without_an_end() {
        let error = parse_label_spec("text=Hello,from=1s").unwrap_err();
        assert!(
            error.contains("to=") && error.contains("length="),
            "error should name both ways to end a label window, got: {}",
            error
        );
    }

    #[test]
    fn parse_label_spec_rejects_both_to_and_length() {
        assert!(parse_label_spec("text=Hello,to=5s,length=2s").is_err());
    }

    #[test]
    fn parse_label_spec_rejects_a_label_window_that_ends_before_it_starts() {
        assert!(parse_label_spec("text=Hello,from=10s,to=5s").is_err());
        assert!(parse_label_spec("text=Hello,from=5s,to=5s").is_err());
    }

    #[test]
    fn parse_label_spec_rejects_an_unknown_key() {
        let error = parse_label_spec("text=Hello,to=5s,align=left").unwrap_err();
        assert!(
            error.contains("align"),
            "error should name the unknown key, got: {}",
            error
        );
    }

    #[test]
    fn parse_label_spec_rejects_a_duplicate_key() {
        let error = parse_label_spec("text=Hello,to=5s,text=Bye").unwrap_err();
        assert!(
            error.contains("Duplicate"),
            "error should call out the duplicate, got: {}",
            error
        );
    }

    #[test]
    fn parse_label_spec_rejects_an_entry_that_is_not_key_value() {
        assert!(parse_label_spec("text=Hello,to=5s,oops").is_err());
    }

    #[test]
    fn parse_label_spec_rejects_an_invalid_position() {
        let error = parse_label_spec("text=Hello,to=5s,position=middle").unwrap_err();
        assert!(
            error.contains("top") && error.contains("bottom"),
            "error should list the positions, got: {}",
            error
        );
    }

    #[test]
    fn parse_label_spec_rejects_an_invalid_size() {
        assert!(parse_label_spec("text=Hello,to=5s,size=big").is_err());
        assert!(parse_label_spec("text=Hello,to=5s,size=0").is_err());
    }

    /// A color goes into the filtergraph unescaped, so anything that is filter
    /// syntax rather than a color has to be rejected here.
    #[test]
    fn parse_label_spec_rejects_a_color_that_is_filter_syntax() {
        for spec in [
            "text=Hello,to=5s,color=white:box=1",
            "text=Hello,to=5s,background=black'",
            "text=Hello,to=5s,color=",
        ] {
            assert!(
                parse_label_spec(spec).is_err(),
                "expected '{}' to be rejected",
                spec
            );
        }
    }

    #[test]
    fn parse_label_spec_accepts_the_color_spellings_the_help_documents() {
        for color in ["white", "#ffcc00", "black@0.5", "0xFF0000"] {
            let label = label_from(&format!("text=Hello,to=5s,color={}", color));
            assert_eq!(label.color, color);
        }
    }

    #[test]
    fn parse_label_with_repeated_flags() {
        let args = Args::try_parse_from([
            "vidcapture",
            "label",
            "talk.mp4",
            "-l",
            "text=Intro,from=1m32s,to=2m,position=top",
            "-l",
            "text=Demo,from=2m,length=30s",
        ])
        .unwrap();
        match args.command {
            Command::Label(label_args) => {
                assert_eq!(label_args.source, PathBuf::from("talk.mp4"));
                assert_eq!(label_args.labels.len(), 2);
                assert_eq!(label_args.labels[0].text, "Intro");
                assert_eq!(label_args.labels[0].position, LabelPosition::Top);
                assert_eq!(label_args.labels[1].text, "Demo");
                assert_eq!(label_args.labels[1].position, LabelPosition::Bottom);
                assert_eq!(label_args.font, None);
                assert_eq!(label_args.output, None);
                assert!(!label_args.verbose);
            }
            _ => panic!("expected Label command"),
        }
    }

    #[test]
    fn parse_label_with_font_output_and_verbose() {
        let args = Args::try_parse_from([
            "vidcapture",
            "label",
            "talk.mp4",
            "-l",
            "text=Intro,to=5s",
            "--font",
            "/System/Library/Fonts/Helvetica.ttc",
            "-o",
            "out.mp4",
            "-v",
        ])
        .unwrap();
        match args.command {
            Command::Label(label_args) => {
                assert_eq!(
                    label_args.font,
                    Some(PathBuf::from("/System/Library/Fonts/Helvetica.ttc"))
                );
                assert_eq!(label_args.output, Some(PathBuf::from("out.mp4")));
                assert!(label_args.verbose);
            }
            _ => panic!("expected Label command"),
        }
    }

    #[test]
    fn parse_label_requires_at_least_one_label() {
        let result = Args::try_parse_from(["vidcapture", "label", "talk.mp4"]);
        assert!(result.is_err(), "expected label with no -l to be rejected");
    }

    #[test]
    fn parse_label_rejects_an_invalid_spec() {
        let result = Args::try_parse_from(["vidcapture", "label", "talk.mp4", "-l", "from=1s"]);
        assert!(result.is_err(), "expected a spec without text= to be rejected");
    }
}
