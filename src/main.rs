mod capture;
mod cli;
mod ffmpeg;
mod output;
mod terminal;

use std::time::Duration;

use chrono::Local;
use clap::Parser;

use capture::{CaptureSession, RealFfmpegProcess};
use cli::{Args, Command, CutArgs, StartArgs};
use ffmpeg::{CaptureConfig, CutConfig};

fn main() {
    let args = Args::parse();

    let result = match args.command {
        Command::Start(start_args) => run_capture(start_args),
        Command::Cut(cut_args) => run_cut(cut_args),
        Command::Help => {
            terminal::print_help();
            Ok(())
        }
    };

    if let Err(e) = result {
        terminal::print_error(&e.to_string());
        std::process::exit(1);
    }
}

fn run_capture(args: StartArgs) -> anyhow::Result<()> {
    // Resolve output directory (creating it if -o is set), generate the
    // timestamped filename, and pick a non-colliding final path.
    let timestamp = Local::now();
    let path = output::prepare_output_path(args.output.as_deref(), &timestamp)?;

    // Detect avfoundation devices and resolve BlackHole + microphone indices.
    // Required for the mixed-audio capture pipeline from issue #3 (system +
    // mic). If BlackHole is missing, fail fast with setup instructions rather
    // than letting ffmpeg die with an opaque error.
    let audio = match ffmpeg::detect_audio_setup() {
        Ok(audio) => Some(audio),
        Err(diag) => {
            anyhow::bail!("{}\n\n{}", diag, ffmpeg::blackhole_setup_instructions());
        }
    };

    // Build ffmpeg config
    let mut ffmpeg_config = CaptureConfig::new(path.to_string_lossy().to_string())
        .with_verbose(args.verbose);

    if let Some(a) = audio {
        ffmpeg_config = ffmpeg_config.with_audio(a);
    }

    let ffmpeg_config = match args.duration {
        Some(d) => ffmpeg_config.with_duration(d),
        None => ffmpeg_config,
    };

    let ffmpeg_config = match args.every {
        Some(e) => ffmpeg_config.with_interval(e),
        None => ffmpeg_config,
    };

    // Create and start capture session
    let process = RealFfmpegProcess::new(ffmpeg_config);
    let mut session = CaptureSession::new(Box::new(process), args.duration);
    session.start()?;

    // Keep raw mode active for the whole session. The listener's Drop
    // implementation restores the terminal even when capture returns an error.
    let key_listener = terminal::StopKeyListener::new();

    // Render the first status line. Subsequent ticks overwrite it in-place via
    // a carriage return; the line already includes the "press s to stop"
    // prompt so a separate static "Capturing..." line is no longer needed.
    terminal::print_capture_status(session.elapsed(), session.duration());

    // Poll for 's' key or duration expiry.
    loop {
        if key_listener.wait_for_stop_key(Duration::from_millis(100))? {
            // User pressed 's' - send SIGINT to ffmpeg so it can finalize MP4.
            session.stop()?;
            break;
        }
        if session.check_and_stop_if_expired()? {
            // Duration expired - ffmpeg should exit naturally, just wait.
            break;
        }
        if session.has_exited() {
            // Do not wait forever if ffmpeg failed before the user pressed 's'.
            break;
        }
        // Refresh the on-screen elapsed/remaining counter on each tick.
        terminal::print_capture_status(session.elapsed(), session.duration());
    }

    // Move to a fresh line so the final "Saved to ..." message doesn't land
    // mid-status-line when the terminal overwrites with carriage returns.
    eprintln!();

    // Wait for ffmpeg to finish and check its exit code
    session.finish()?;

    if args.every.is_some() {
        // `path` is the .mp4 base name; ffmpeg actually wrote a family of
        // files following the segment pattern. Show a shell-glob so the
        // user can copy/paste it (e.g. `ls ${Saved to ...}`).
        let display = output::segment_display_pattern(&path);
        terminal::print_saved(&display);
    } else {
        terminal::print_saved(&path);
    }
    Ok(())
}

fn run_cut(args: CutArgs) -> anyhow::Result<()> {
    if !args.source.exists() {
        anyhow::bail!("Source file not found: {}", args.source.display());
    }
    if !args.source.is_file() {
        anyhow::bail!("Source is not a file: {}", args.source.display());
    }

    let range = args.validate_cut_range().map_err(|e| anyhow::anyhow!(e))?;

    let output_path = output::resolve_cut_output_path(&args.source, args.output.as_deref())?;

    let config = CutConfig::new(
        args.source.to_string_lossy().to_string(),
        range.start,
        range.length,
        output_path.to_string_lossy().to_string(),
    )
    .with_fast(args.fast)
    .with_verbose(args.verbose);

    let mut cmd = ffmpeg::build_cut_command(&config);

    let result = if args.verbose {
        cmd.status().map(|s| (s, Vec::new()))
    } else {
        cmd.output().map(|o| (o.status, o.stderr))
    };

    match result {
        Ok((status, stderr)) if status.success() => {
            if !args.verbose {
                let stderr_str = String::from_utf8_lossy(&stderr);
                if let Some(written) = parse_ffmpeg_time(&stderr_str) {
                    let requested_ms = range.length.as_millis() as u64;
                    let written_ms = (written * 1000.0) as u64;
                    if requested_ms > written_ms && (requested_ms - written_ms) > 250 {
                        eprintln!(
                            "\x1b[33mwarning\x1b[0m: cut range extends past end of source \
                             (requested {}ms, wrote {}ms)",
                            requested_ms, written_ms
                        );
                    }
                }
            }
            terminal::print_saved(&output_path);
            Ok(())
        }
        Ok((status, stderr)) => {
            if output_path.exists() {
                let _ = std::fs::remove_file(&output_path);
            }
            let msg = if args.verbose {
                format!("ffmpeg exited with status: {}", status)
            } else {
                let stderr_str = String::from_utf8_lossy(&stderr);
                format!("ffmpeg failed:\n{}", stderr_str)
            };
            anyhow::bail!("{}", msg)
        }
        Err(e) => {
            if output_path.exists() {
                let _ = std::fs::remove_file(&output_path);
            }
            Err(e.into())
        }
    }
}

/// Parse the last `time=` token from ffmpeg stderr progress output.
///
/// ffmpeg emits lines like `time=00:00:15.000` or `time=15.000` during
/// encoding. Returns the time in seconds as a float, or `None` if no
/// `time=` token is found.
fn parse_ffmpeg_time(stderr: &str) -> Option<f64> {
    stderr.lines().rev().find_map(|line| {
        let time_pos = line.find("time=")?;
        let rest = &line[time_pos + 5..];
        let token: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == ':' || *c == '.').collect();
        if token.is_empty() {
            return None;
        }
        parse_time_token(&token)
    })
}

/// Parse an ffmpeg time token like `00:00:15.000` or `15.000` into seconds.
fn parse_time_token(token: &str) -> Option<f64> {
    let parts: Vec<&str> = token.split(':').collect();
    match parts.as_slice() {
        [h, m, s] => {
            let h: f64 = h.parse().ok()?;
            let m: f64 = m.parse().ok()?;
            let s: f64 = s.parse().ok()?;
            Some(h * 3600.0 + m * 60.0 + s)
        }
        [m, s] => {
            let m: f64 = m.parse().ok()?;
            let s: f64 = s.parse().ok()?;
            Some(m * 60.0 + s)
        }
        [s] => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_time_token_hms() {
        assert_eq!(parse_time_token("00:00:15.000"), Some(15.0));
    }

    #[test]
    fn parse_time_token_minutes_seconds() {
        assert_eq!(parse_time_token("01:30.500"), Some(90.5));
    }

    #[test]
    fn parse_time_token_bare_seconds() {
        assert_eq!(parse_time_token("15.000"), Some(15.0));
    }

    #[test]
    fn parse_time_token_empty() {
        assert_eq!(parse_time_token(""), None);
    }

    #[test]
    fn parse_ffmpeg_time_finds_last_time_token() {
        let stderr = "frame=  120 fps=0.0 q=28.0 size=    1024kB time=00:00:05.000\n\
                       frame=  240 fps=0.0 q=28.0 size=    2048kB time=00:00:10.000\n\
                       frame=  360 fps=0.0 q=28.0 size=    3072kB time=00:00:15.000";
        assert_eq!(parse_ffmpeg_time(stderr), Some(15.0));
    }

    #[test]
    fn parse_ffmpeg_time_no_time_token() {
        let stderr = "frame=  120 fps=0.0 q=28.0 size=    1024kB\n";
        assert_eq!(parse_ffmpeg_time(stderr), None);
    }

    #[test]
    fn parse_ffmpeg_time_empty_string() {
        assert_eq!(parse_ffmpeg_time(""), None);
    }
}
