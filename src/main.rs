mod capture;
mod cli;
mod ffmpeg;
mod output;
mod terminal;

use std::path::Path;
use std::time::Duration;

use chrono::Local;
use clap::Parser;

use capture::{CaptureSession, RealFfmpegProcess};
use cli::{Args, Command, CutArgs, StartArgs};
use ffmpeg::CaptureConfig;

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
    // Validate source path exists and is a file
    if !args.source.exists() {
        anyhow::bail!("Source file not found: {}", args.source.display());
    }
    if !args.source.is_file() {
        anyhow::bail!("Source is not a file: {}", args.source.display());
    }

    // Validate the cut range
    let range = args.validate_cut_range().map_err(|e| anyhow::anyhow!(e))?;

    // Determine output path
    let output_path = match &args.output {
        Some(path) => path.clone(),
        None => {
            // Default: <source>_cut.mp4 next to the source
            let stem = args
                .source
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            let dir = args
                .source
                .parent()
                .unwrap_or_else(|| Path::new("."));
            dir.join(format!("{}_cut.mp4", stem))
        }
    };

    // Build ffmpeg command for cutting
    let start_secs = range.start.as_secs_f64();
    let length_secs = range.length.as_secs_f64();

    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-y"]);
    cmd.args(["-ss", &format!("{:.3}", start_secs)]);
    cmd.args(["-i", &args.source.to_string_lossy()]);
    cmd.args(["-t", &format!("{:.3}", length_secs)]);

    if args.fast {
        // Stream-copy instead of re-encoding
        cmd.args(["-c", "copy"]);
    } else {
        // Re-encode with H.264 + AAC
        cmd.args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23"]);
        cmd.args(["-c:a", "aac", "-b:a", "128k"]);
    }

    cmd.arg(&output_path);

    if args.verbose {
        // Show ffmpeg output by inheriting stdout/stderr
        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!("ffmpeg exited with status: {}", status);
        }
    } else {
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ffmpeg failed:\n{}", stderr);
        }
    }

    terminal::print_saved(&output_path);
    Ok(())
}
