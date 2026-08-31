//! Cut orchestration: runs one ffmpeg cut to completion, surfaces ffmpeg
//! failures, and warns when the written range falls short of the requested
//! cut length.
//!
//! One-shot by design: no raw mode, no polling loop, no stop key.

use std::time::Duration;

use crate::cli::CutArgs;
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

    let ffmpeg_run = ffmpeg::run_to_completion(ffmpeg::build_cut_command(&config), args.verbose);

    let (status, stderr) = match ffmpeg_run {
        Ok(run) => run,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("ffmpeg not found. Install it with: brew install ffmpeg");
        }
        Err(e) => {
            output::remove_partial_output(&output_path);
            return Err(e.into());
        }
    };

    if !status.success() {
        output::remove_partial_output(&output_path);
        // Under `-v` ffmpeg's own output already reached the terminal as it
        // ran; repeating it in the error message would print it twice.
        if args.verbose {
            anyhow::bail!("ffmpeg exited with {}", status);
        }
        anyhow::bail!("ffmpeg failed:\n{}", stderr);
    }

    if let Some(written) = ffmpeg::parse_written_length(&stderr)
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

