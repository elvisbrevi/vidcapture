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
