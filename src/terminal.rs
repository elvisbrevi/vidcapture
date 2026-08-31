use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// ANSI escape used to move the cursor back to the start of the current line.
/// Using `\r` (carriage return) instead of `\n` lets the status line update
/// in-place rather than scrolling the terminal on every poll iteration.
const CARRIAGE_RETURN: &str = "\r";

/// ANSI escape that erases from the cursor to the end of the line. Sent after
/// each status update so residual characters from a longer previous line do
/// not leak into the next refresh.
const CLEAR_LINE: &str = "\x1b[K";

/// Format a duration as a compact human-readable string for the status line.
///
/// Examples:
/// - `format_status_duration(0s)` → `"0s"`
/// - `format_status_duration(65s)` → `"1m05s"`
/// - `format_status_duration(3_661s)` → `"1h01m01s"`
pub fn format_status_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{}h{:02}m{:02}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m{:02}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Build the capture status line for `elapsed` and the optional `duration`.
///
/// When `duration` is `Some`, the status includes a remaining-time component;
/// when `None`, only the elapsed component is shown. The line ends with the
/// "press s to stop" prompt so the user always knows how to stop recording.
pub fn format_capture_status(elapsed: Duration, duration: Option<Duration>) -> String {
    let elapsed_str = format_status_duration(elapsed);
    match duration {
        Some(total) => {
            // Clamp remaining at zero so a slow refresh never shows a negative
            // countdown if a poll races past the configured duration.
            let remaining = total.saturating_sub(elapsed);
            format!(
                "Capturing [{} elapsed, {} remaining], press s to stop.",
                elapsed_str,
                format_status_duration(remaining),
            )
        }
        None => format!(
            "Capturing [{} elapsed], press s to stop.",
            elapsed_str,
        ),
    }
}

/// Print the capture status line, overwriting any previous one in-place.
///
/// Emits the status followed by `CLEAR_LINE` so a shorter line replaces a
/// longer one cleanly. Callers should invoke this from a poll loop so the
/// user sees the timer advance without spamming the terminal.
pub fn print_capture_status(elapsed: Duration, duration: Option<Duration>) {
    let line = format_capture_status(elapsed, duration);
    eprint!("{}{}{}", CARRIAGE_RETURN, line, CLEAR_LINE);
    use std::io::Write;
    let _ = std::io::stderr().flush();
}

/// Owns terminal raw mode for one capture session.
///
/// Raw mode is enabled once before polling begins and is always disabled when
/// the listener is dropped, including when polling or key reading returns an
/// error. This keeps the user's terminal usable after a capture stops.
pub struct StopKeyListener {
    raw_mode_enabled: bool,
}

impl StopKeyListener {
    /// Create a listener. Non-interactive terminals do not support raw mode;
    /// in that case the listener falls back to sleeping until the timeout.
    pub fn new() -> Self {
        Self {
            raw_mode_enabled: enable_raw_mode().is_ok(),
        }
    }

    /// Wait for an `s` key press, returning false when the timeout expires.
    pub fn wait_for_stop_key(&self, timeout: Duration) -> anyhow::Result<bool> {
        if !self.raw_mode_enabled {
            std::thread::sleep(timeout);
            return Ok(false);
        }

        if !poll(timeout)? {
            return Ok(false);
        }

        if let Event::Key(key) = read()? {
            Ok(matches!(key.code, KeyCode::Char('s' | 'S')))
        } else {
            Ok(false)
        }
    }
}

impl Drop for StopKeyListener {
    fn drop(&mut self) {
        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
        }
    }
}

/// Poll for an `s` key press for callers that need a one-shot check.
///
/// Capture orchestration should prefer [`StopKeyListener`] so raw mode is not
/// repeatedly toggled during a recording.
pub fn wait_for_stop_key(timeout: Duration) -> anyhow::Result<bool> {
    let listener = StopKeyListener::new();
    listener.wait_for_stop_key(timeout)
}

/// Print the saved output path to stderr.
pub fn print_saved(path: &std::path::Path) {
    eprintln!("Saved to {}", path.display());
}

/// Print the saved cut path to stderr.
pub fn print_cut_saved(path: &std::path::Path) {
    eprintln!("Cut saved to {}", path.display());
}

/// Print the saved labeled-video path to stderr.
pub fn print_label_saved(path: &std::path::Path) {
    eprintln!("Labeled video saved to {}", path.display());
}

/// Print a colored warning message to stderr.
pub fn print_warning(msg: &str) {
    eprintln!("\x1b[33mwarning\x1b[0m: {}", msg);
}

/// Print a colored error message to stderr.
pub fn print_error(msg: &str) {
    eprintln!("\x1b[31merror\x1b[0m: {}", msg);
}

/// Format the help text — a `String` view of the user-facing help output.
/// Kept separate from `print_help` so tests can assert against the content.
pub fn format_help() -> String {
    let mut help = String::new();

    help.push_str("vidcapture — CLI screen and audio recorder for macOS\n\n");

    help.push_str("USAGE:\n");
    help.push_str("    vidcapture <COMMAND> [FLAGS]\n");
    help.push_str("    vidcapture -h | --help    Show short flag help (auto-generated by clap)\n\n");

    help.push_str("COMMANDS:\n");
    help.push_str("    start    Start capturing screen and audio\n");
    help.push_str("    cut      Cut a range out of an existing video file\n");
    help.push_str("    label    Draw timed text labels onto an existing video file\n");
    help.push_str("    help     Show this help message with setup instructions\n\n");

    help.push_str("FLAGS (start):\n");
    help.push_str("    -d, --duration <TIME>    Capture duration. Stops automatically after this time.\n");
    help.push_str("                             Examples: 10s, 2m, 1h, 1h30m10s\n");
    help.push_str("    -e, --every <TIME>       Interval mode. Split the capture into segments of this duration.\n");
    help.push_str("                             Examples: 10s, 2m\n");
    help.push_str("                             Each segment is a standalone, playable MP4.\n");
    help.push_str("    -o, --output <DIR>       Output directory for the recorded file(s).\n");
    help.push_str("                             Default: current working directory.\n");
    help.push_str("                             Example: ./recordings/\n");
    help.push_str("                             Created if it does not exist. Filenames auto-increment on collision.\n");
    help.push_str("    -v, --verbose            Show ffmpeg output while recording.\n\n");

    help.push_str("FLAGS (cut):\n");
    help.push_str("    <SOURCE>                 Path to the source video file. Required.\n");
    help.push_str("    -f, --from <TIME>        Start offset of the cut range (default: 0s).\n");
    help.push_str("    -t, --to <TIME>          End offset of the cut range. Conflicts with --length.\n");
    help.push_str("    -l, --length <TIME>      Cut length. Conflicts with --to.\n");
    help.push_str("    -o, --output <PATH>      Output file or directory.\n");
    help.push_str("        --fast               Stream-copy instead of re-encoding.\n");
    help.push_str("    -v, --verbose            Show ffmpeg output.\n");
    help.push_str("    cut requires ffmpeg but not BlackHole, and never touches the source video.\n\n");

    help.push_str("FLAGS (label):\n");
    help.push_str("    <SOURCE>                 Path to the source video file. Required.\n");
    help.push_str("    -l, --label <SPEC>       A label spec. Repeat -l once per label. Required.\n");
    help.push_str("                             Example: -l \"text=Intro,from=1m32s,to=2m,position=top\"\n");
    help.push_str("        --font <PATH>        Font file to draw labels with.\n");
    help.push_str("                             Default: the system font ffmpeg resolves.\n");
    help.push_str("                             Example: /System/Library/Fonts/Helvetica.ttc\n");
    help.push_str("    -o, --output <PATH>      Output file or directory.\n");
    help.push_str("                             Default: <source>_labeled.mp4 beside the source.\n");
    help.push_str("    -v, --verbose            Show ffmpeg output.\n");
    help.push_str("    label re-encodes the video and never touches the source video.\n\n");

    help.push_str("LABEL SPEC:\n");
    help.push_str("    A comma-separated list of key=value pairs describing one label:\n");
    help.push_str("    the text, the label window it is visible for, and how it is drawn.\n");
    help.push('\n');
    help.push_str("    text=<TEXT>              The text drawn on the video. Required.\n");
    help.push_str("    from=<TIME>              Start of the label window (default: 0s).\n");
    help.push_str("    to=<TIME>                End of the label window. Conflicts with length.\n");
    help.push_str("    length=<TIME>            How long the label window lasts. Conflicts with to.\n");
    help.push_str("    position=top|bottom      Where the label sits in the frame (default: bottom).\n");
    help.push_str("    color=<COLOR>            Text color (default: white).\n");
    help.push_str("    size=<PIXELS>            Font size in pixels (default: 32).\n");
    help.push_str("    background=<COLOR>       Color of the band drawn behind the text.\n");
    help.push_str("                             Default: none — the text is drawn bare.\n");
    help.push('\n');
    help.push_str("    Colors are ffmpeg color names or #RRGGBB, with an optional alpha\n");
    help.push_str("    suffix: white, yellow, #ffcc00, black@0.5.\n");
    help.push_str("    Time values use the same timespec as every other flag (see below).\n");
    help.push_str("    Write a literal comma in text as \\, and a literal backslash as \\\\.\n\n");

    help.push_str("DURATION FORMAT:\n");
    help.push_str("    A timespec is one of two notations:\n");
    help.push('\n');
    help.push_str("    Unit-suffixed: a sequence of <number><unit> pairs.\n");
    help.push_str("        Units: h (hours), m (minutes), s (seconds), ms (milliseconds).\n");
    help.push_str("        Decimals are allowed: 1.5s = 1500ms, 0.25m = 15s.\n");
    help.push_str("        Examples:\n");
    help.push_str("            10s         10 seconds\n");
    help.push_str("            1500ms      1.5 seconds\n");
    help.push_str("            1.5s        1.5 seconds\n");
    help.push_str("            2m          2 minutes\n");
    help.push_str("            1h          1 hour\n");
    help.push_str("            1h30m       1 hour 30 minutes\n");
    help.push_str("            1h30m10s    1 hour 30 minutes 10 seconds\n");
    help.push('\n');
    help.push_str("    Timestamp: HH:MM:SS[.mmm] or MM:SS[.mmm].\n");
    help.push_str("        Examples:\n");
    help.push_str("            00:01:30.500   1 minute 30.5 seconds\n");
    help.push_str("            01:30          1 minute 30 seconds\n");
    help.push_str("            1:02:03.250    1 hour 2 minutes 3.25 seconds\n\n");

    help.push_str("SETUP:\n\n");

    help.push_str("  1. Install ffmpeg:\n");
    help.push_str("       brew install ffmpeg\n");
    help.push_str("     Verify with: ffmpeg -version\n\n");

    help.push_str("  2. Install BlackHole 2ch (for system audio capture):\n");
    help.push_str("       brew install blackhole-2ch\n\n");

    help.push_str("  3. Configure Multi-Output Device in Audio MIDI Setup:\n");
    help.push_str("       a. Open Audio MIDI Setup (in /Applications/Utilities).\n");
    help.push_str("       b. Click the + button at the bottom-left and choose \"Create Multi-Output Device\".\n");
    help.push_str("       c. In the new device, check both \"BlackHole 2ch\" and your speakers/headphones.\n");
    help.push_str("       d. Right-click the Multi-Output Device and select \"Use This Device For Sound Output\".\n");
    help.push_str("       e. Set its drift correction to the BlackHole entry.\n\n");

    help.push_str("  4. Sanity check that ffmpeg sees your devices:\n");
    help.push_str("       ffmpeg -f avfoundation -list_devices true -i \"\"\n\n");

    help.push_str("EXAMPLES:\n");
    help.push_str("    vidcapture start                      # Record until you press 's'\n");
    help.push_str("    vidcapture start -d 30s               # Record for 30 seconds\n");
    help.push_str("    vidcapture start -e 10s               # Cut into 10-second segments\n");
    help.push_str("    vidcapture start -d 1m -e 10s         # 6 segments of 10 seconds, then stop\n");
    help.push_str("    vidcapture start -o ./recordings/     # Save into ./recordings/\n");
    help.push_str("    vidcapture start -v                   # Show ffmpeg output while recording\n");
    help.push_str("    vidcapture cut talk.mp4 --length 5s   # First 5 seconds\n");
    help.push_str("    vidcapture cut talk.mp4 --from 10s --to 25s  # From 10s to 25s\n");
    help.push_str("    vidcapture cut talk.mp4 --from 10s --length 1.5s  # 1.5s starting at 10s\n");
    help.push('\n');
    help.push_str("    # A label across the bottom from 1m32s to 2m:\n");
    help.push_str("    vidcapture label talk.mp4 -l \"text=Setting up,from=1m32s,to=2m\"\n");
    help.push('\n');
    help.push_str("    # Two labels, one on top with a band behind it:\n");
    help.push_str("    vidcapture label talk.mp4 \\\n");
    help.push_str("        -l \"text=Intro,from=0s,to=1m,position=top,background=black@0.5\" \\\n");
    help.push_str("        -l \"text=Live demo,from=1m,length=90s,color=#ffcc00,size=48\"\n");

    help
}

/// Print the formatted help text to stdout.
pub fn print_help() {
    print!("{}", format_help());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_key_listener_returns_false_without_a_ready_key() {
        let listener = StopKeyListener::new();
        assert!(!listener.wait_for_stop_key(Duration::ZERO).unwrap());
    }

    #[test]
    fn format_help_returns_non_empty_string() {
        let help = format_help();
        assert!(!help.is_empty(), "help text should not be empty");
    }

    #[test]
    fn format_help_documents_start_command() {
        let help = format_help();
        assert!(
            help.contains("start"),
            "help text should mention the 'start' command, got: {}",
            help
        );
    }

    #[test]
    fn format_help_documents_help_command() {
        let help = format_help();
        assert!(
            help.contains("help"),
            "help text should document the 'help' command, got: {}",
            help
        );
    }

    #[test]
    fn format_help_documents_duration_flag_with_examples() {
        let help = format_help();
        assert!(help.contains("-d"), "help text should mention -d");
        assert!(help.contains("--duration"), "help text should mention --duration");
        assert!(
            help.contains("10s"),
            "help text should include duration examples like 10s"
        );
        assert!(
            help.contains("2m"),
            "help text should include duration examples like 2m"
        );
        assert!(
            help.contains("1h"),
            "help text should include duration examples like 1h"
        );
    }

    #[test]
    fn format_help_documents_every_flag_with_examples() {
        let help = format_help();
        assert!(help.contains("-e"), "help text should mention -e");
        assert!(help.contains("--every"), "help text should mention --every");
        assert!(
            help.contains("segment"),
            "help text should describe interval mode segments"
        );
    }

    #[test]
    fn format_help_documents_output_flag_with_examples() {
        let help = format_help();
        assert!(help.contains("-o"), "help text should mention -o");
        assert!(help.contains("--output"), "help text should mention --output");
        assert!(
            help.contains("./recordings/"),
            "help text should include an output directory example"
        );
    }

    #[test]
    fn format_help_documents_verbose_flag() {
        let help = format_help();
        assert!(help.contains("-v"), "help text should mention -v");
        assert!(help.contains("--verbose"), "help text should mention --verbose");
    }

    #[test]
    fn format_help_documents_default_output_directory() {
        let help = format_help();
        assert!(
            help.contains("current working directory")
                || help.contains("current directory"),
            "help text should state the default output directory"
        );
    }

    #[test]
    fn format_help_documents_short_help_flag() {
        let help = format_help();
        assert!(
            help.contains("-h") && help.contains("--help"),
            "help text should mention the auto-generated short help flag"
        );
    }

    #[test]
    fn format_help_documents_duration_format_compound_examples() {
        let help = format_help();
        assert!(
            help.contains("1h30m"),
            "help text should include compound duration example 1h30m"
        );
        assert!(
            help.contains("1h30m10s"),
            "help text should include full compound duration example 1h30m10s"
        );
    }

    #[test]
    fn format_help_documents_timespec_ms_unit() {
        let help = format_help();
        assert!(
            help.contains("ms"),
            "help text should document the ms (millisecond) unit"
        );
        assert!(
            help.contains("1500ms"),
            "help text should include an ms example"
        );
    }

    #[test]
    fn format_help_documents_timespec_decimal() {
        let help = format_help();
        assert!(
            help.contains("1.5s"),
            "help text should include a decimal seconds example"
        );
    }

    #[test]
    fn format_help_documents_timestamp_notation() {
        let help = format_help();
        assert!(
            help.contains("HH:MM:SS"),
            "help text should document the HH:MM:SS timestamp notation"
        );
        assert!(
            help.contains("00:01:30.500"),
            "help text should include a timestamp example"
        );
    }

    #[test]
    fn format_help_notes_cut_needs_ffmpeg_not_blackhole() {
        let help = format_help();
        assert!(
            help.contains("ffmpeg") && help.contains("not BlackHole"),
            "help text should note that cut requires ffmpeg but not BlackHole"
        );
    }

    /// The note is about the `cut` command, not about `--verbose`. Hanging it
    /// off the `-v` entry reads as a property of that flag.
    #[test]
    fn format_help_attributes_the_blackhole_note_to_cut_not_to_verbose() {
        let help = format_help();
        let note_line = help
            .lines()
            .find(|line| line.contains("not BlackHole"))
            .expect("help should carry the BlackHole note");
        assert!(
            note_line.contains("cut"),
            "the note must name the cut command, got: {}",
            note_line.trim()
        );
    }

    #[test]
    fn format_help_includes_ffmpeg_install_instructions() {
        let help = format_help();
        assert!(
            help.contains("brew install ffmpeg"),
            "help text should include the ffmpeg install command"
        );
    }

    #[test]
    fn format_help_includes_blackhole_setup_instructions() {
        let help = format_help();
        assert!(
            help.to_lowercase().contains("blackhole"),
            "help text should mention BlackHole"
        );
        assert!(
            help.contains("Multi-Output Device"),
            "help text should explain Multi-Output Device setup"
        );
        assert!(
            help.contains("Audio MIDI Setup"),
            "help text should mention Audio MIDI Setup"
        );
    }

    #[test]
    fn format_help_includes_list_devices_check() {
        let help = format_help();
        assert!(
            help.contains("avfoundation") && help.contains("list_devices"),
            "help text should include the avfoundation list_devices sanity check"
        );
    }

    #[test]
    fn format_help_includes_examples_section() {
        let help = format_help();
        assert!(
            help.contains("EXAMPLES"),
            "help text should include an EXAMPLES section header"
        );
    }

    #[test]
    fn format_status_duration_seconds_only() {
        assert_eq!(format_status_duration(Duration::from_secs(0)), "0s");
        assert_eq!(format_status_duration(Duration::from_secs(7)), "7s");
        assert_eq!(format_status_duration(Duration::from_secs(59)), "59s");
    }

    #[test]
    fn format_status_duration_minutes_and_seconds() {
        // No hours → minutes:seconds form, with seconds zero-padded.
        assert_eq!(format_status_duration(Duration::from_secs(60)), "1m00s");
        assert_eq!(format_status_duration(Duration::from_secs(125)), "2m05s");
        assert_eq!(format_status_duration(Duration::from_secs(3599)), "59m59s");
    }

    #[test]
    fn format_status_duration_full_hours_minutes_seconds() {
        // With hours, minutes and seconds are both zero-padded.
        assert_eq!(
            format_status_duration(Duration::from_secs(3_661)),
            "1h01m01s"
        );
        assert_eq!(
            format_status_duration(Duration::from_secs(7_200)),
            "2h00m00s"
        );
    }

    #[test]
    fn format_capture_status_with_duration_includes_remaining() {
        let line = format_capture_status(Duration::from_secs(5), Some(Duration::from_secs(10)));
        assert!(line.contains("5s elapsed"), "got: {}", line);
        assert!(line.contains("5s remaining"), "got: {}", line);
        assert!(line.contains("press s to stop"), "got: {}", line);
    }

    #[test]
    fn format_capture_status_without_duration_omits_remaining() {
        let line = format_capture_status(Duration::from_secs(5), None);
        assert!(line.contains("5s elapsed"), "got: {}", line);
        assert!(
            !line.contains("remaining"),
            "indefinite status must not advertise a remaining time, got: {}",
            line
        );
    }

    #[test]
    fn format_capture_status_remaining_does_not_go_negative() {
        // Polling slightly past the duration must clamp remaining at 0s, not
        // produce a negative countdown that confuses the user.
        let line = format_capture_status(Duration::from_secs(11), Some(Duration::from_secs(10)));
        assert!(line.contains("0s remaining"), "got: {}", line);
    }

    #[test]
    fn format_capture_status_with_long_duration_uses_hours_form() {
        // 1h30m10s total, ~5 minutes elapsed.
        let line = format_capture_status(
            Duration::from_secs(300),
            Some(Duration::from_secs(5_410)),
        );
        assert!(line.contains("5m00s elapsed"), "got: {}", line);
        assert!(line.contains("1h25m10s remaining"), "got: {}", line);
    }

    #[test]
    fn format_help_documents_label_command() {
        let help = format_help();
        assert!(
            help.contains("label"),
            "help text should mention the 'label' command"
        );
        assert!(
            help.contains("-l, --label"),
            "help text should document the repeatable label flag"
        );
        assert!(
            help.contains("--font"),
            "help text should document the --font flag"
        );
    }

    #[test]
    fn format_help_documents_every_label_spec_key() {
        let help = format_help();
        for key in [
            "text=",
            "from=",
            "to=",
            "length=",
            "position=",
            "color=",
            "size=",
            "background=",
        ] {
            assert!(
                help.contains(key),
                "help text should document the '{}' key",
                key
            );
        }
    }

    #[test]
    fn format_help_documents_label_positions_and_colors() {
        let help = format_help();
        assert!(
            help.contains("top") && help.contains("bottom"),
            "help text should list both label positions"
        );
        assert!(
            help.contains("black@0.5"),
            "help text should show a color with an alpha suffix"
        );
    }

    #[test]
    fn format_help_includes_a_label_example() {
        let help = format_help();
        assert!(
            help.contains("vidcapture label"),
            "help text should include a label example"
        );
        assert!(
            help.contains("from=1m32s"),
            "help text should show a label window in the examples"
        );
    }

    #[test]
    fn format_help_notes_label_leaves_the_source_video_alone() {
        let help = format_help();
        assert!(
            help.contains("label re-encodes the video and never touches the source video."),
            "help text should say a label pass leaves the source alone"
        );
    }
}
