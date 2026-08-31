//! Label orchestration: runs one ffmpeg label pass to completion, surfaces
//! ffmpeg failures, and warns about a label window the source video is too
//! short to reach.
//!
//! One-shot by design: no raw mode, no polling loop, no stop key.

use std::time::Duration;

use crate::cli::{Label, LabelArgs};
use crate::ffmpeg::{self, LabelConfig};
use crate::{output, terminal};

pub fn run(args: LabelArgs) -> anyhow::Result<()> {
    if !args.source.exists() {
        anyhow::bail!("Source file not found: {}", args.source.display());
    }
    if !args.source.is_file() {
        anyhow::bail!("Source is not a file: {}", args.source.display());
    }

    let output_path = output::resolve_label_output_path(&args.source, args.output.as_deref())?;

    let config = LabelConfig::new(&args.source, args.labels, &output_path).with_font(args.font);

    let ffmpeg_run = ffmpeg::run_to_completion(ffmpeg::build_label_command(&config), args.verbose);

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
        if let Some(diagnosis) = diagnose_drawtext_failure(&stderr) {
            anyhow::bail!("{}", diagnosis);
        }
        // Under `-v` ffmpeg's own output already reached the terminal as it
        // ran; repeating it in the error message would print it twice.
        if args.verbose {
            anyhow::bail!("ffmpeg exited with {}", status);
        }
        anyhow::bail!("ffmpeg failed:\n{}", stderr);
    }

    if let Some(written) = ffmpeg::parse_written_length(&stderr) {
        warn_about_unreachable_labels(&config.labels, written);
    }

    terminal::print_label_saved(&output_path);
    Ok(())
}

/// Warn about every label whose label window starts at or after the end of the
/// video ffmpeg wrote, because such a label never becomes visible.
///
/// A label window that merely runs past the end is not reported: the label is
/// on screen for the footage that exists, which is what the user asked for.
fn warn_about_unreachable_labels(labels: &[Label], written: Duration) {
    for (number, label) in labels.iter().enumerate() {
        if label.start >= written {
            terminal::print_warning(&format!(
                "label {} ('{}') starts at {}ms, past the end of the source video \
                 ({}ms), so it never appears",
                number + 1,
                label.text,
                label.start.as_millis(),
                written.as_millis()
            ));
        }
    }
}

/// Turn the two ffmpeg build problems that stop `label` specifically into
/// instructions, instead of leaving the user to read a filtergraph error.
///
/// `drawtext` is compiled in only with libfreetype, and it resolves the
/// default font through fontconfig. An ffmpeg without either runs `start` and
/// `cut` perfectly well and fails only here.
fn diagnose_drawtext_failure(stderr: &str) -> Option<String> {
    if stderr.contains("No such filter: 'drawtext'") {
        return Some(String::from(
            "This ffmpeg was built without the drawtext filter, which labels need.\n\
             Install one built with libfreetype:\n\
                  brew install ffmpeg",
        ));
    }
    if stderr.contains("Cannot find a valid font")
        || stderr.contains("No font filename provided")
        || stderr.contains("Font not found")
    {
        return Some(String::from(
            "ffmpeg could not find a font to draw labels with.\n\
             Point it at one explicitly, e.g.:\n\
                  --font /System/Library/Fonts/Helvetica.ttc",
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnose_drawtext_failure_explains_a_missing_filter() {
        let diagnosis =
            diagnose_drawtext_failure("[AVFilterGraph] No such filter: 'drawtext'").unwrap();
        assert!(
            diagnosis.contains("libfreetype"),
            "should name what the ffmpeg build is missing, got: {}",
            diagnosis
        );
    }

    #[test]
    fn diagnose_drawtext_failure_explains_a_missing_font() {
        let diagnosis = diagnose_drawtext_failure(
            "[Parsed_drawtext_0] Cannot find a valid font for the family Sans",
        )
        .unwrap();
        assert!(
            diagnosis.contains("--font"),
            "should point at the --font flag, got: {}",
            diagnosis
        );
    }

    #[test]
    fn diagnose_drawtext_failure_passes_other_errors_through() {
        assert_eq!(
            diagnose_drawtext_failure("Invalid data found when processing input"),
            None
        );
    }
}
