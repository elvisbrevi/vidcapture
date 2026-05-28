# PRD: vidcapture — CLI Screen & Audio Recorder

## Problem Statement

As a developer, I need a lightweight CLI tool to capture my screen and audio (system + microphone) directly from the terminal, with support for timed captures and automatic segment splitting, so that I can record sessions without opening a GUI app.

## Solution

A Rust CLI app (`vidcapture`) that shells out to ffmpeg for screen + audio capture on macOS. It supports continuous recording, timed capture, interval-based segment splitting, and interactive stop via keyboard. Output is MP4 (H.264 + AAC) with timestamped filenames.

## User Stories

1. As a user, I want to run `vidcapture start` so that I can begin recording my entire screen and all audio (system + microphone).
2. As a user, I want to see "Capturing, press s to stop." in the terminal while recording, so that I know the capture is active and how to stop it.
3. As a user, I want to press `s` in the active terminal to stop recording, so that I don't need to reach for Ctrl+C or switch windows.
4. As a user, I want to run `vidcapture start -d 10s` so that the capture automatically stops after 10 seconds.
5. As a user, I want to run `vidcapture start -d 2m` so that the capture automatically stops after 2 minutes.
6. As a user, I want to run `vidcapture start -e 10s` so that the capture splits into 10-second segments automatically, continuing until I press `s`.
7. As a user, I want to run `vidcapture start -e 2m` so that the capture splits into 2-minute segments automatically.
8. As a user, I want to run `vidcapture start -o ./recordings/` so that the output file is saved to a specific directory.
9. As a user, I want output files named with timestamps (e.g., `vidcapture_2026-05-28_14-30-00.mp4`) so that they are unique and sortable.
10. As a user, I want the app to auto-increment filenames if a file with the same name exists, so that no recordings are overwritten.
11. As a user, I want to run `vidcapture help` so that I can see all available commands and flags with explanations.
12. As a user, I want clean, concise terminal output by default, so that the tool is not noisy.
13. As a user, I want verbose ffmpeg output when I set `RUST_LOG=vidcapture=debug`, so that I can troubleshoot issues.
14. As a user, I want partial/unfinished files cleaned up on error, so that I don't end up with corrupt recordings.
15. As a user, I want colored error messages on failure, so that I can quickly identify what went wrong.
16. As a user, I want to combine duration and interval flags (e.g., `vidcapture start -d 1m -e 10s`) so that I get 6 segments of 10 seconds each.
17. As a user, I want the app to require BlackHole for system audio capture, with a clear error message if it's not installed.
18. As a user, I want the app to use the current working directory as the default output location.

## Implementation Decisions

### Architecture

Five modules, each with a focused responsibility:

- **cli** — Clap argument parsing, subcommand routing, flag validation. Derive-based structs for `Args`, `StartArgs`.
- **ffmpeg** — Builds ffmpeg command strings, spawns/manages ffmpeg processes, handles segment output via ffmpeg's `-f segment`. Deep module with a clean interface.
- **capture** — Orchestration layer. Manages capture lifecycle: start, stop, interval logic, duration timers. Calls into ffmpeg module.
- **terminal** — Puts terminal in raw mode via crossterm, polls for `s` key, prints colored status/error messages.
- **output** — Resolves output directory, generates timestamped filenames, handles auto-increment on collision.

### CLI Structure

```
vidcapture <command> [flags]

Commands:
  start    Start capturing screen and audio
  help     Show help with flag explanations

Flags (start):
  -d, --duration <time>    Capture duration (e.g., 10s, 2m). Stops automatically.
  -e, --every <time>       Interval mode — split into segments of this duration.
  -o, --output <dir>       Output directory (default: current directory).
  -v, --verbose            Show ffmpeg output (alternative to RUST_LOG).
```

### Screen & Audio Capture

- Shell out to ffmpeg via `std::process::Command`.
- macOS screen capture via ffmpeg's `avfoundation` input device.
- System audio via BlackHole 2ch (same setup as interview-assistant: BlackHole + Multi-Output Device in Audio MIDI Setup).
- Microphone captured alongside system audio.
- Output: MP4 container, H.264 video codec, AAC audio codec.

### Interval Mode

- Use ffmpeg's built-in `-f segment` muxer with `-segment_time` for seamless splitting.
- Each segment is a fully playable, independent MP4 file.
- Segment filenames: `vidcapture_2026-05-28_14-30-00_seg001.mp4`.
- Capture continues until user presses `s` or duration limit is reached.

### Duration & Interval Interaction

- `-d 1m -e 10s` → 6 segments of 10 seconds, then stop.
- `-e 10s` alone → infinite segments until `s` pressed.
- `-d 10s` alone → single capture, stops at 10s.

### File Naming

- Pattern: `vidcapture_YYYY-MM-DD_HH-MM-SS.mp4`
- Interval segments: `vidcapture_YYYY-MM-DD_HH-MM-SS_segNNN.mp4`
- Auto-increment: if file exists, append `_1`, `_2`, etc.
- Default output directory: current working directory.

### Error Handling

- Colored error output to stderr via crossterm.
- Clean up partial/unfinished files on error or crash.
- Non-zero exit code (`1`) on failure.
- `anyhow` for error context propagation.
- Clear error if BlackHole is not detected.

### Logging

- `tracing` + `tracing-subscriber`.
- Default: warnings only (clean output).
- `RUST_LOG=vidcapture=debug` for verbose ffmpeg command output.
- `--verbose` flag as shortcut for debug logging.

### Dependencies

**Rust crates:**
- `clap` (derive) — CLI parsing
- `crossterm` — terminal raw mode, key detection, colored output
- `anyhow` — error handling
- `chrono` — timestamp generation for filenames
- `tracing` + `tracing-subscriber` — logging

**System:**
- `ffmpeg` — must be installed (`brew install ffmpeg`)
- `BlackHole 2ch` — virtual audio device for system audio capture

## Testing Decisions

- **cli module**: Unit tests for argument parsing — valid flags, invalid durations, missing subcommands.
- **ffmpeg module**: Unit tests for command string building — verify correct ffmpeg args for each mode (simple, duration, interval, combined).
- **output module**: Unit tests for filename generation, directory resolution, auto-increment logic.
- **capture module**: Integration tests with mocked ffmpeg interface — verify start/stop/interval orchestration.
- **terminal module**: Manual/integration testing — raw mode behavior is hard to unit test.

Priority: ffmpeg command building and output filename generation should have thorough unit tests.

## Out of Scope

- Cross-platform support (Linux, Windows) — macOS only for v1.
- GUI interface.
- Video editing, trimming, or post-processing.
- Webcam overlay / picture-in-picture.
- Custom codec selection (always H.264 + AAC).
- Custom ffmpeg arguments passthrough.
- Remote/network recording.
- Pause/resume during capture.

## Further Notes

- The BlackHole setup mirrors the interview-assistant project. Consider documenting the Audio MIDI Setup configuration (Multi-Output Device) in a README.
- ffmpeg's `avfoundation` device list can be queried with `ffmpeg -f avfoundation -list_devices true -i ""` — useful for validating setup.
- The `help` command should include setup instructions (ffmpeg install, BlackHole configuration).
