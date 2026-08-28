use std::process::{Command, Output};

fn vidcapture(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vidcapture"))
        .args(args)
        .output()
        .expect("vidcapture should run")
}

fn create_test_video(path: &std::path::Path, duration_secs: u64) {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi",
            "-i", &format!("color=c=red:s=320x240:d={}", duration_secs),
            "-c:v", "libx264",
            "-preset", "ultrafast",
            "-pix_fmt", "yuv420p",
            path.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("ffmpeg should be available for test fixture creation");
    assert!(status.success(), "failed to create test video fixture");
}

#[test]
fn no_subcommand_shows_help() {
    let output = vidcapture(&[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("Usage: vidcapture <COMMAND>"));
    assert!(stderr.contains("Commands:"));
    assert!(stderr.contains("start"));
    assert!(stderr.contains("help"));
}

#[test]
fn start_subcommand_is_recognized() {
    let output = vidcapture(&["start", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("Start capturing screen and audio"));
    assert!(stdout.contains("Usage: vidcapture start"));
}

#[test]
fn help_subcommand_describes_commands_and_flags() {
    let output = vidcapture(&["help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("COMMANDS"));
    assert!(stdout.contains("start"));
    assert!(stdout.contains("-d, --duration"));
    assert!(stdout.contains("-e, --every"));
    assert!(stdout.contains("-o, --output"));
    assert!(stdout.contains("-v, --verbose"));
}

#[test]
fn invalid_subcommand_shows_error_and_help() {
    let output = vidcapture(&["invalid"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("unrecognized subcommand 'invalid'"));
    assert!(stderr.contains("Usage: vidcapture <COMMAND>"));
    assert!(stderr.contains("For more information, try '--help'."));
}

#[test]
fn cut_end_to_end_with_from_and_to() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_to");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 30);

    let output = dir.join("cut_out.mp4");
    let result = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
        "--from", "5s",
        "--to", "10s",
        "-o", output.to_str().unwrap(),
    ]);

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "cut should succeed, stderr: {}",
        stderr
    );
    assert!(
        output.exists(),
        "output file should exist after successful cut"
    );
    let meta = std::fs::metadata(&output).unwrap();
    assert!(meta.len() > 0, "output file should not be empty");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cut_end_to_end_with_length() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_length");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 30);

    let output = dir.join("cut_out.mp4");
    let result = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
        "--from", "10s",
        "--length", "1500ms",
        "-o", output.to_str().unwrap(),
    ]);

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "cut with --length should succeed, stderr: {}",
        stderr
    );
    assert!(output.exists(), "output file should exist");
    let meta = std::fs::metadata(&output).unwrap();
    assert!(meta.len() > 0, "output file should not be empty");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cut_range_past_end_warns_and_succeeds() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_past_end");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 5);

    let output = dir.join("cut_out.mp4");
    let result = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
        "--from", "3s",
        "--to", "20s",
        "-o", output.to_str().unwrap(),
    ]);

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "cut past end should still succeed, stderr: {}",
        stderr
    );
    assert!(
        output.exists(),
        "output file should exist even when range extends past source"
    );
    assert!(
        stderr.contains("warning"),
        "should print a warning about short range, stderr: {}",
        stderr
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cut_nonexistent_source_errors() {
    let result = vidcapture(&[
        "cut",
        "/nonexistent/video.mp4",
        "--length", "5s",
    ]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("Source file not found"),
        "should report missing source, stderr: {}",
        stderr
    );
}

#[test]
fn cut_no_range_specified_errors() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_no_range");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 5);

    let result = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
    ]);
    assert!(!result.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cut_help_documents_flags() {
    let output = vidcapture(&["cut", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("--from"));
    assert!(stdout.contains("--to"));
    assert!(stdout.contains("--length"));
    assert!(stdout.contains("--fast"));
    assert!(stdout.contains("--output"));
}

#[test]
fn cut_success_reports_the_saved_cut_path() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_saved_msg");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 5);

    let output = dir.join("clip.mp4");
    let result = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
        "--length",
        "1s",
        "-o",
        output.to_str().unwrap(),
    ]);

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "cut should succeed, stderr: {}",
        stderr
    );
    assert!(
        stderr.contains(&format!("Cut saved to {}", output.display())),
        "success message should name the cut, got: {}",
        stderr
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The short-range warning is about the cut, not about the log level: `-v`
/// must not swallow it.
#[test]
fn cut_range_past_end_warns_in_verbose_mode_too() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_past_end_verbose");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 5);

    let output = dir.join("cut_out.mp4");
    let result = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
        "--from",
        "3s",
        "--to",
        "20s",
        "-o",
        output.to_str().unwrap(),
        "-v",
    ]);

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "cut past end should still succeed, stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("warning"),
        "-v should not swallow the short-range warning, stderr: {}",
        stderr
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `-v` shows ffmpeg's own output; without it the run stays quiet apart from
/// vidcapture's own messages.
#[test]
fn cut_verbose_shows_ffmpeg_output_and_quiet_run_does_not() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_verbosity");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 5);

    let verbose = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
        "--length",
        "1s",
        "-o",
        dir.join("verbose.mp4").to_str().unwrap(),
        "-v",
    ]);
    let verbose_stderr = String::from_utf8_lossy(&verbose.stderr);
    assert!(
        verbose_stderr.contains("ffmpeg version"),
        "-v should show ffmpeg output, got: {}",
        verbose_stderr
    );

    let quiet = vidcapture(&[
        "cut",
        source.to_str().unwrap(),
        "--length",
        "1s",
        "-o",
        dir.join("quiet.mp4").to_str().unwrap(),
    ]);
    let quiet_stderr = String::from_utf8_lossy(&quiet.stderr);
    assert!(
        !quiet_stderr.contains("ffmpeg version"),
        "a quiet run should not show ffmpeg output, got: {}",
        quiet_stderr
    );
    assert_eq!(
        quiet_stderr.lines().count(),
        1,
        "a quiet run should print only the saved-cut line, got: {}",
        quiet_stderr
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// An ffmpeg failure exits 1, leaves no partial file, and reports the error
/// once — `-v` already streamed ffmpeg's output, so the error message must not
/// repeat it.
#[test]
fn cut_failure_reports_the_ffmpeg_error_once_in_both_modes() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_failure");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // A file ffmpeg cannot decode: passes vidcapture's own source checks and
    // fails inside ffmpeg.
    let source = dir.join("broken.mp4");
    std::fs::write(&source, b"this is not a video").unwrap();

    let count_errors = |out: &Output| {
        String::from_utf8_lossy(&out.stderr)
            .matches("Invalid data found")
            .count()
    };

    let output = dir.join("cut_out.mp4");
    let quiet = vidcapture(&[
        "cut", source.to_str().unwrap(),
        "--length", "1s",
        "-o", output.to_str().unwrap(),
    ]);
    assert!(!quiet.status.success(), "a broken source should fail");
    assert!(
        !output.exists(),
        "a failed cut should leave no partial file behind"
    );

    let verbose = vidcapture(&[
        "cut", source.to_str().unwrap(),
        "--length", "1s",
        "-o", output.to_str().unwrap(),
        "-v",
    ]);
    assert!(!verbose.status.success(), "a broken source should fail under -v too");
    assert_eq!(
        count_errors(&verbose),
        count_errors(&quiet),
        "-v should not report ffmpeg's error twice, stderr: {}",
        String::from_utf8_lossy(&verbose.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `cut` shells out to ffmpeg; when it is not on PATH, say so instead of
/// leaking a bare OS error.
#[test]
fn cut_without_ffmpeg_on_path_explains_the_missing_dependency() {
    let dir = std::env::temp_dir().join("vidcapture_cut_e2e_no_ffmpeg");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let source = dir.join("source.mp4");
    create_test_video(&source, 5);

    let result = Command::new(env!("CARGO_BIN_EXE_vidcapture"))
        .args(["cut", source.to_str().unwrap(), "--length", "1s"])
        .env("PATH", dir.to_str().unwrap())
        .output()
        .expect("vidcapture should run");

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("ffmpeg not found"),
        "should name the missing dependency, got: {}",
        stderr
    );

    let _ = std::fs::remove_dir_all(&dir);
}
