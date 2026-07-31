mod capture;
mod cli;
mod ffmpeg;
mod output;
mod terminal;

use std::time::Duration;

use chrono::Local;
use clap::Parser;

use capture::{CaptureSession, RealFfmpegProcess};
use cli::{Args, Command, StartArgs};
use ffmpeg::CaptureConfig;
use output::OutputConfig;

fn main() {
    let args = Args::parse();

    let result = match args.command {
        Command::Start(start_args) => run_capture(start_args),
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
    // Resolve output directory
    let dir = match &args.output {
        Some(path) => output::resolve_output_directory(path)?,
        None => std::env::current_dir()?,
    };

    // Generate timestamped filename
    let timestamp = Local::now();
    let config = OutputConfig::new(dir, "vidcapture".to_string());
    let path = output::generate_filename(&config, &timestamp);

    // Avoid collision
    let path = output::avoid_collision(&path);

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

    // Print status
    terminal::print_capturing();

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
    }

    // Wait for ffmpeg to finish and check its exit code
    session.finish()?;

    terminal::print_saved(&path);
    Ok(())
}
