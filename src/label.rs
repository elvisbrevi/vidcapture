//! Label orchestration: runs one ffmpeg label pass to completion, surfaces
//! ffmpeg failures, and warns about a label window the source video is too
//! short to reach.
//!
//! One-shot by design: no raw mode, no polling loop, no stop key.

use std::time::Duration;

use crate::cli::{Label, LabelArgs};
use crate::ffmpeg::{self, LabelConfig};
use crate::platform::Platform;
use crate::{output, terminal};

pub fn run(args: LabelArgs) -> anyhow::Result<()> {
    if !args.source.exists() {
        anyhow::bail!("Source file not found: {}", args.source.display());
    }
    if !args.source.is_file() {
        anyhow::bail!("Source is not a file: {}", args.source.display());
    }

    let output_path = output::resolve_label_output_path(&args.source, args.output.as_deref())?;

    let platform = Platform::current();
    let font = args.font.or_else(|| platform.default_label_font());
    let config = LabelConfig::new(&args.source, args.labels, &output_path).with_font(font);

    let ffmpeg_run = ffmpeg::run_to_completion(ffmpeg::build_label_command(&config), args.verbose);

    let (status, stderr) = match ffmpeg_run {
        Ok(run) => run,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("{}", platform.ffmpeg_not_found_message());
        }
        Err(e) => {
            output::remove_partial_output(&output_path);
            return Err(e.into());
        }
    };

    if !status.success() {
        output::remove_partial_output(&output_path);
        if let Some(diagnosis) = diagnose_drawtext_failure(&stderr, platform) {
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
/// instructions for `platform`, instead of leaving the user to read a
/// filtergraph error.
///
/// `drawtext` is compiled in only with libfreetype, and it resolves the
/// default font through fontconfig. An ffmpeg without either runs `start` and
/// `cut` perfectly well and fails only here.
fn diagnose_drawtext_failure(stderr: &str, platform: Platform) -> Option<String> {
    if stderr.contains("No such filter: 'drawtext'") {
        let install = match platform {
            Platform::MacOs => String::from(
                "Install Homebrew's full build with libfreetype and put it first on PATH:\n\
                      brew install ffmpeg-full\n\
                      export PATH=\"$(brew --prefix ffmpeg-full)/bin:$PATH\"",
            ),
            Platform::Linux | Platform::Windows => format!(
                "Install an ffmpeg built with libfreetype and put it first on PATH, e.g.:\n\
                      {}",
                platform.ffmpeg_install_command()
            ),
        };
        return Some(format!(
            "This ffmpeg was built without the drawtext filter, which labels need.\n{}",
            install
        ));
    }
    if stderr.contains("Cannot find a valid font")
        || stderr.contains("No font filename provided")
        || stderr.contains("Font not found")
    {
        return Some(format!(
            "ffmpeg could not find a font to draw labels with.\n\
             Point it at one explicitly, e.g.:\n\
                  --font {}",
            platform.example_font()
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnose_drawtext_failure_explains_a_missing_filter() {
        let diagnosis = diagnose_drawtext_failure(
            "[AVFilterGraph] No such filter: 'drawtext'",
            Platform::MacOs,
        )
        .unwrap();
        assert!(
            diagnosis.contains("libfreetype"),
            "should name what the ffmpeg build is missing, got: {}",
            diagnosis
        );
        assert!(diagnosis.contains("brew install ffmpeg-full"));
        assert!(diagnosis.contains("brew --prefix ffmpeg-full"));
    }

    #[test]
    fn diagnose_drawtext_failure_installs_ffmpeg_the_platform_way() {
        for (platform, install) in [
            (Platform::Linux, "sudo apt install ffmpeg"),
            (Platform::Windows, "winget install Gyan.FFmpeg"),
        ] {
            let diagnosis =
                diagnose_drawtext_failure("No such filter: 'drawtext'", platform).unwrap();
            assert!(diagnosis.contains("libfreetype"), "got: {}", diagnosis);
            assert!(diagnosis.contains(install), "got: {}", diagnosis);
            assert!(
                !diagnosis.contains("brew"),
                "Homebrew is macOS advice, got: {}",
                diagnosis
            );
        }
    }

    #[test]
    fn diagnose_drawtext_failure_explains_a_missing_font() {
        let diagnosis = diagnose_drawtext_failure(
            "[Parsed_drawtext_0] Cannot find a valid font for the family Sans",
            Platform::MacOs,
        )
        .unwrap();
        assert!(
            diagnosis.contains("--font"),
            "should point at the --font flag, got: {}",
            diagnosis
        );
        assert!(diagnosis.contains("/System/Library/Fonts/Helvetica.ttc"));
    }

    #[test]
    fn diagnose_drawtext_failure_suggests_a_font_the_platform_has() {
        let stderr = "Cannot find a valid font for the family Sans";
        assert!(diagnose_drawtext_failure(stderr, Platform::Linux)
            .unwrap()
            .contains("DejaVuSans.ttf"));
        assert!(diagnose_drawtext_failure(stderr, Platform::Windows)
            .unwrap()
            .contains(r"C:\Windows\Fonts\arial.ttf"));
    }

    #[test]
    fn diagnose_drawtext_failure_passes_other_errors_through() {
        assert_eq!(
            diagnose_drawtext_failure("Invalid data found when processing input", Platform::MacOs),
            None
        );
    }
}
