use std::process::{Command, Output};

fn vidcapture(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vidcapture"))
        .args(args)
        .output()
        .expect("vidcapture should run")
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
