//! Cut orchestration: runs one ffmpeg cut to completion, surfaces ffmpeg
//! failures, and warns when the written range falls short of the requested
//! cut length.
//!
//! One-shot by design: no raw mode, no polling loop, no stop key.

use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use crate::cli::{self, CutArgs};
use crate::ffmpeg::{self, CutConfig};
use crate::{output, terminal};

/// A written range shorter than the requested cut length by more than this is
/// reported to the user as a short cut.
const SHORT_CUT_TOLERANCE: Duration = Duration::from_millis(250);

pub fn run(args: CutArgs) -> anyhow::Result<()> {
    if !args.source.exists() {
        anyhow::bail!("Source file not found: {}", args.source.display());
    }
    if !args.source.is_file() {
        anyhow::bail!("Source is not a file: {}", args.source.display());
    }

    let range = args.validate_cut_range().map_err(|e| anyhow::anyhow!(e))?;
    let output_path = output::resolve_cut_output_path(&args.source, args.output.as_deref())?;

    let config =
        CutConfig::new(&args.source, range.start, range.length, &output_path).with_fast(args.fast);

    let ffmpeg_run = run_ffmpeg(ffmpeg::build_cut_command(&config), args.verbose);

    let (status, stderr) = match ffmpeg_run {
        Ok(run) => run,
        Err(e) => {
            remove_partial_cut(&output_path);
            return Err(e.into());
        }
    };

    if !status.success() {
        remove_partial_cut(&output_path);
        // Under `-v` ffmpeg's own output already reached the terminal as it
        // ran; repeating it in the error message would print it twice.
        if args.verbose {
            anyhow::bail!("ffmpeg exited with {}", status);
        }
        anyhow::bail!("ffmpeg failed:\n{}", stderr);
    }

    if let Some(written) = parse_ffmpeg_time(&stderr)
        && range.length.saturating_sub(written) > SHORT_CUT_TOLERANCE
    {
        terminal::print_warning(&format!(
            "cut range extends past end of source (requested {}ms, wrote {}ms)",
            range.length.as_millis(),
            written.as_millis()
        ));
    }

    terminal::print_cut_saved(&output_path);
    Ok(())
}

/// Run ffmpeg to completion, returning its exit status and its stderr.
///
/// stderr is always captured — the short-cut warning is scraped from it — and,
/// when `verbose`, echoed to our own stderr as it arrives so a long cut still
/// shows ffmpeg's progress live.
fn run_ffmpeg(mut cmd: Command, verbose: bool) -> std::io::Result<(ExitStatus, String)> {
    let mut child = cmd.stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
    let mut ffmpeg_stderr = child.stderr.take().expect("stderr was piped");

    let mut captured: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = ffmpeg_stderr.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if verbose {
            let _ = std::io::stderr().write_all(&buffer[..read]);
        }
        captured.extend_from_slice(&buffer[..read]);
    }

    let status = child.wait()?;
    Ok((status, String::from_utf8_lossy(&captured).into_owned()))
}

/// Delete a partially written cut so a failed run leaves no corrupt file.
fn remove_partial_cut(path: &std::path::Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

/// Parse the last `time=` token from ffmpeg stderr progress output.
///
/// ffmpeg emits lines like `time=00:00:15.00` while encoding, separating
/// progress updates with a carriage return rather than a newline, so several
/// tokens can share one `\n`-delimited line. Returns the last one.
fn parse_ffmpeg_time(stderr: &str) -> Option<Duration> {
    stderr.split(['\n', '\r']).rev().find_map(|chunk| {
        let token: String = chunk
            .split_once("time=")?
            .1
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ':' || *c == '.')
            .collect();
        // ffmpeg writes `HH:MM:SS.mmm`; a bare seconds token needs a unit
        // before the shared timespec parser will take it.
        let timespec = if token.contains(':') {
            token
        } else {
            format!("{}s", token)
        };
        cli::parse_timespec(&timespec).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ffmpeg_time_reads_hms_token() {
        assert_eq!(
            parse_ffmpeg_time("frame= 1 time=00:00:15.000 bitrate=N/A"),
            Some(Duration::from_secs(15))
        );
    }

    #[test]
    fn parse_ffmpeg_time_reads_minutes_seconds_token() {
        assert_eq!(
            parse_ffmpeg_time("time=01:30.500 speed=1x"),
            Some(Duration::from_millis(90_500))
        );
    }

    #[test]
    fn parse_ffmpeg_time_finds_last_of_many_newline_separated_updates() {
        let stderr = "frame=  120 size=  1024kB time=00:00:05.000\n\
                      frame=  240 size=  2048kB time=00:00:10.000\n\
                      frame=  360 size=  3072kB time=00:00:15.000";
        assert_eq!(parse_ffmpeg_time(stderr), Some(Duration::from_secs(15)));
    }

    /// ffmpeg overwrites its progress line with `\r`, so a single
    /// `\n`-delimited line can carry several `time=` tokens. The last one is
    /// the written range; the first one would under-report it and fire a
    /// spurious short-cut warning.
    #[test]
    fn parse_ffmpeg_time_finds_last_of_carriage_return_separated_updates() {
        let stderr = "frame=  120 time=00:00:05.00 speed=1x\r\
                      frame=  240 time=00:00:10.00 speed=1x\r\
                      frame=  360 time=00:00:16.50 speed=1x\r\
                      frame=  480 time=00:00:49.43 speed=1x";
        assert_eq!(
            parse_ffmpeg_time(stderr),
            Some(Duration::from_millis(49_430))
        );
    }

    #[test]
    fn parse_ffmpeg_time_ignores_trailing_summary_without_progress() {
        let stderr = "frame=  120 time=00:00:05.00 speed=1x\r\
                      frame=  240 time=00:00:12.00 speed=1x\n\
                      [out#0] video:1024kB audio:64kB muxing overhead: 0.4%\n";
        assert_eq!(parse_ffmpeg_time(stderr), Some(Duration::from_secs(12)));
    }

    #[test]
    fn parse_ffmpeg_time_without_time_token() {
        assert_eq!(parse_ffmpeg_time("frame=  120 fps=0.0 q=28.0"), None);
        assert_eq!(parse_ffmpeg_time(""), None);
    }
}
