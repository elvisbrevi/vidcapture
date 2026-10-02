---
name: vidcapture
description: Use when implementing a vidcapture change or fixing review findings; follow its product contract, module ownership, test seams, and ffmpeg edge cases.
---

# Develop vidcapture

## Change loop

1. **Pin down the contract.** For an issue, read `gh issue view <number> --repo elvisbrevi/vidcapture` or the issue text supplied to you; its acceptance criteria define the behavior. Without an issue, use the user's request and relevant `PRD.md` user stories. For a review fix, treat the finding as the contract and trace it to the relevant code and spec. **Done when** every requested behavior or finding has a concrete resolution and way to verify it.
2. **Trace ownership before editing.** Read relevant terms in `CONTEXT.md` (use its preferred vocabulary; terms under “Avoid” are synonyms to keep out of tests and type names), applicable parts of `PRD.md`, and related `docs/adr/*.md`. Find the owning module and search for an existing helper before adding one. **Done when** each planned change has an owner and any reusable helper is identified.
3. **Implement one seam at a time.** Add or adapt a focused check that fails for the missing behavior, make the smallest change, and confirm the check passes. Keep orchestration modules delegating work to their owners below. **Done when** every changed behavior has a passing focused check.
4. **Run the project gates.** Run `cargo test` and `cargo clippy --all-targets`; resolve failures caused by the change. Preserve local formatting by matching the surrounding code: repo-wide `cargo fmt` reformats unrelated files. **Done when** both commands pass and the diff contains no unrelated formatting changes.
5. **Review the completed diff.** Check standards against `CONTEXT.md`, `PRD.md`, and relevant ADRs; check behavior against issue acceptance criteria, the user's request, or the review finding. Prefer the smallest fix that reuses existing helpers and avoid speculative generality. Resolve every actionable finding, rerun relevant checks, and review the updated diff. **Done when** all requested behavior is accounted for and a final review finds no actionable issues.

## Ownership map

- `ffmpeg.rs` owns ffmpeg command construction, process execution (`run_to_completion`), filter strings, and output parsing (`parse_avfoundation_listing`, `parse_device_sources`, `parse_dshow_audio_devices`, `parse_written_length`). `capture.rs`, `cut.rs`, and `label.rs` orchestrate these operations; they do not spawn ffmpeg themselves.
- Capture is the only per-platform ffmpeg command (ADR 0004). `build_capture_command` takes `CaptureDevices` as data, so test every platform's command on any host by building it with `Screen::Avfoundation`, `Screen::X11`, or `Screen::Gdi`; never gate a command-shape test on the host OS. `detect_capture_devices` is the only code that asks the host. The macOS command with BlackHole and a microphone is pinned by `macos_capture_with_blackhole_and_microphone_keeps_its_command`: keep it passing unchanged.
- `platform.rs` owns which OS the binary runs on and the user-facing advice that depends on it (`ffmpeg_not_found_message`, `example_font`). Reuse it rather than writing `brew`, `apt`, or a font path into a message.
- `cli.rs` owns argument parsing and validation, including `parse_label_spec`. Orchestration receives validated values.
- `output.rs` owns output path resolution and cleanup of partial outputs.
- Reuse shared helpers: `parse_timespec` handles every time-valued flag and label key; `resolve_start_and_length` handles both cut ranges and label windows; `resolve_derived_output_path` names cut and labeled outputs. Search for these before adding parallel logic.

## Test seams

- Unit tests live in the tested file's `#[cfg(test)] mod tests`.
- CLI end-to-end tests live in `tests/cli.rs` and spawn `env!("CARGO_BIN_EXE_vidcapture")`. Reuse its `create_test_video()` fixture, which creates a real video with `ffmpeg -f lavfi`.
- Tests that change `std::env::current_dir` share process-global state. Guard them with the existing `CWD_LOCK` in `src/output.rs`.
- Tests run on macOS, Linux, and Windows. A test that drives a POSIX shell is `#[cfg(unix)]`; one that needs real ffmpeg spawns `ffmpeg` itself rather than `sh -c`.
- For filter escaping changes, follow the rendered-frame procedure below; command-argument assertions alone cannot verify what appears in the video.

## ffmpeg gotchas

### `drawtext` filter escaping

Filtergraph strings are not verifiable by reading. `escape_filter_value` in `ffmpeg.rs` uses one backslash for `,;[]`, two for `:`, three for `'`, and four for `\\`: ffmpeg parses the filter option in two passes, and each pass consumes one. Keep `expansion=none` in the filter; without it, `drawtext` reads the text a third time, each count increases by one, and `%` also needs escaping.

To verify a changed escape count, render the character onto a fixture with real ffmpeg (`-frames:v 1 -update 1 out.png`) and read the frame back. A unit test of the arguments can pass while the rendered label is missing. A `textfile=` render is not a valid reference for `%` or `\\`, because both sides are expanded the same way and can appear to match while blank. Compare a full-frame render to a reference only for characters consumed by the outer parsing passes.

### End-to-end assertion paths

An assertion such as `!stderr.contains("warning")` can match the test's own temporary directory name if that path is echoed in the success output. Name fixture directories after the test case, not after the assertion.
