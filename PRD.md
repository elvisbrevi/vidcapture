# PRD: vidcapture — CLI Screen & Audio Recorder

## Problem Statement

As a developer, I need a lightweight CLI tool to capture my screen and audio (system + microphone) directly from the terminal, with support for timed captures and automatic segment splitting, so that I can record sessions without opening a GUI app. Once I have a recording, I also need to pull a precise range out of it from the same terminal, without opening a video editor — and to caption what is happening on screen at the moments it happens, so a walkthrough explains itself without a voiceover or an editing session.

## Solution

A Rust CLI app (`vidcapture`) that shells out to ffmpeg for screen + audio capture on macOS. It supports continuous recording, timed capture, interval-based segment splitting, and interactive stop via keyboard. Output is MP4 (H.264 + AAC) with timestamped filenames.

A second command, `vidcapture cut`, extracts one cut range from an existing source video into a new MP4, leaving the source untouched. The cut range is given as a start offset plus either an end offset or a cut length, at millisecond precision.

A third command, `vidcapture label`, draws timed text labels onto an existing source video into a new MP4, again leaving the source untouched. Each label carries its own label window and its own styling — position, text color, font size, and an optional background band — and any number of labels can be drawn in one pass.

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
19. As a user, I want to run `vidcapture cut talk.mp4 --from 10s --to 25s` so that I get a new video containing only that range of the source video.
20. As a user, I want to run `vidcapture cut talk.mp4 --from 10s --length 1500ms` so that I can express the cut range as a length instead of an end offset.
21. As a user, I want to express cut offsets in milliseconds and as timestamps (`1500ms`, `1.5s`, `00:01:30.500`) so that I can cut precisely without doing arithmetic in seconds.
22. As a user, I want the cut to land exactly on the times I asked for, so that the result is not silently shifted to the nearest keyframe.
23. As a user, I want to run `vidcapture cut talk.mp4 --from 10s --to 25s --fast` so that I can trade frame accuracy for a near-instant cut when I don't need the precision.
24. As a user, I want the cut written next to the source video as `talk_cut.mp4` by default, so that I don't have to name it.
25. As a user, I want `-o` on `cut` to accept either a file path or a directory, so that I can control where the cut lands and what it's called.
26. As a user, I want a clear error when the cut range is invalid (start at or after end, zero length, missing source file), so that I don't produce an empty file.
27. As a user, I want a warning — not a failure — when my cut range runs past the end of the source video, so that I still get the footage that does exist.
28. As a user, I want the source video left byte-for-byte untouched by a cut, so that I can cut the same recording repeatedly.
29. As a user, I want to run `vidcapture label talk.mp4 -l "text=Setting up,from=1m32s,to=2m"` so that the text appears on the video for exactly that span and nowhere else.
30. As a user, I want to repeat `-l` so that I can add as many labels to one video as the walkthrough needs, in a single pass.
31. As a user, I want to choose whether each label sits at the top or the bottom of the frame, so that a label never covers what it is describing.
32. As a user, I want to set each label's text color and font size, so that it stays readable over whatever the footage is.
33. As a user, I want an optional colored band behind a label's text, so that the text stays legible over busy or light-colored footage.
34. As a user, I want a label's window expressed in the same timespec as everything else (`1m32s`, `00:01:32`, `--length`-style `length=`), so that I don't learn a second time format.
35. As a user, I want a clear error when a label spec is malformed (missing text, no end, unknown key, bad color), before ffmpeg is run, so that I don't wait through a re-encode to find a typo.
36. As a user, I want a warning when a label window starts past the end of the source video, so that I know why the label I asked for never shows up.
37. As a user, I want the labeled video written next to the source as `talk_labeled.mp4` by default, so that I don't have to name it.
38. As a user, I want the source video left byte-for-byte untouched by a label pass, so that I can re-label the same recording with different text.

## Implementation Decisions

### Architecture

Seven modules, each with a focused responsibility:

- **cli** — Clap argument parsing, subcommand routing, flag validation. Derive-based structs for `Args`, `StartArgs`, `CutArgs`. Owns the timespec parser shared by every time-valued flag.
- **ffmpeg** — Builds ffmpeg command strings, spawns/manages ffmpeg processes, handles segment output via ffmpeg's `-f segment`. Also builds the cut and label commands, including the `drawtext` filter chain and its filtergraph escaping, and parses ffmpeg's stderr (the avfoundation device listing, and the `time=` progress token both `cut` and `label` read back). Deep module with a clean interface.
- **capture** — Orchestration layer for capture sessions. Manages capture lifecycle: start, stop, interval logic, duration timers. Calls into ffmpeg module.
- **cut** — Orchestration layer for cuts. Runs the cut command to completion, surfaces ffmpeg failures, and detects a short result. One-shot: no raw mode, no polling loop, no stop key.
- **label** — Orchestration layer for label passes. Runs the label command to completion, turns a missing `drawtext` filter or a missing font into setup instructions, and warns about a label the source video is too short to reach. One-shot, like `cut`.
- **terminal** — Puts terminal in raw mode via crossterm, polls for `s` key, prints colored status/error/warning messages.
- **output** — Resolves output directory, generates timestamped filenames, handles auto-increment on collision, resolves the derived output path for both `cut` (`_cut.mp4`) and `label` (`_labeled.mp4`), and deletes a partially written output after a failed run.

### CLI Structure

```
vidcapture <command> [flags]

Commands:
  start    Start capturing screen and audio
  cut      Cut a range out of an existing video
  label    Draw timed text labels onto an existing video
  help     Show help with flag explanations

Flags (start):
  -d, --duration <TIME>    Capture duration (e.g., 10s, 2m). Stops automatically.
  -e, --every <TIME>       Interval mode — split into segments of this duration.
  -o, --output <DIR>       Output directory (default: current directory).
  -v, --verbose            Show ffmpeg output (alternative to RUST_LOG).

Args and flags (cut):
  <SOURCE>                 Path to the source video. Positional, required.
  -f, --from <TIME>        Start offset of the cut range (default: 0s).
  -t, --to <TIME>          End offset of the cut range. Conflicts with --length.
  -l, --length <TIME>      Cut length. Conflicts with --to.
  -o, --output <PATH>      Output file or directory.
      --fast               Stream-copy instead of re-encoding (keyframe-aligned).
  -v, --verbose            Show ffmpeg output.
```

Args and flags (label):
```
  <SOURCE>                 Path to the source video. Positional, required.
  -l, --label <SPEC>       A label spec. Repeatable, at least one required.
      --font <PATH>        Font file to draw with (default: the system font).
  -o, --output <PATH>      Output file or directory.
  -v, --verbose            Show ffmpeg output.
```

`--to` and `--length` are mutually exclusive via clap's `conflicts_with`; supplying neither is an error, since a cut range with no end has no meaning. `--from` defaults to `0s`, so `vidcapture cut talk.mp4 --length 5s` takes the first five seconds.

`-l` is repeated once per label rather than spreading a label's settings across separate flags, because per-label styling would otherwise need positional correspondence between several repeated flags — an easy thing to get silently wrong. One flag carries one label whole.

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

### Label Spec

A label spec is a comma-separated list of `key=value` pairs, parsed by clap's `value_parser` so a malformed spec is rejected with the rest of the argument errors, before anything runs. Keys and values are trimmed; `\,` is a literal comma and `\\` a literal backslash, so a label's text may contain either. A backslash before anything else is kept verbatim, so a path survives unchanged.

| Key | Meaning | Default |
| --- | --- | --- |
| `text` | The text drawn on the video. Required. | — |
| `from` | Start of the label window. | `0s` |
| `to` | End of the label window. Conflicts with `length`. | — |
| `length` | How long the label window lasts. Conflicts with `to`. | — |
| `position` | `top` or `bottom`. | `bottom` |
| `color` | Text color. | `white` |
| `size` | Font size in pixels. | `32` |
| `background` | Color of the label background band. | none |

Time-valued keys take the same timespec as every other time-valued flag. The label window is validated by exactly the three rules a cut range is — one of the two ends is required, the start must precede the end, a length must be positive — so both go through one shared helper, which is told how the user spelled the options (`--from` vs `from=`) so each error names what they typed.

Colors are ffmpeg's own syntax: a name (`white`), a hex triplet (`#ffcc00`), either optionally carrying an `@<alpha>` suffix (`black@0.5`). They are charset-validated rather than fully parsed — enough to catch a typo, and enough that a spec cannot smuggle `:` or `'` into the filtergraph, where they would be read as filter syntax rather than a color.

Duplicate and unknown keys are errors: a silently ignored `postion=top` would produce a video that looks right in the terminal and wrong on screen.

### Label Rendering

One `drawtext` filter per label, chained with commas into a single `-vf`, each gated to its own label window by `enable='between(t,<start>,<end>)'`. Labels are drawn in the order given, so a later label overlaps an earlier one where their windows and positions coincide. The video is always re-encoded — drawing on frames is the point — with the same codecs as a cut, so the output is the same H.264/AAC MP4 the rest of the tool produces.

- Horizontal placement is always centred (`x=(w-text_w)/2`).
- Vertical placement is a margin of 5% of the frame height in from the chosen edge, so a label sits the same distance in whatever the source resolution is.
- `background` is drawn as `box=1` with `boxborderw` set to a third of the font size, which turns a tight outline into a band with the text inside it. That padding is added to the margin so the band never overhangs the frame edge. Without a `background` there is no band and no padding.
- `--font` sets `fontfile`; with no `--font`, ffmpeg resolves the system font through fontconfig.

A label's text reaches ffmpeg as a filter option value, so it is escaped before it goes into the filtergraph, and `expansion=none` keeps `drawtext` from reading a `%{...}` in it as an ffmpeg expression. See `docs/adr/0003-draw-labels-with-drawtext.md`.

### Label Validation

Hard errors (colored message, exit code `1`, no file written):

- The source path does not exist, or is not a file.
- No `-l/--label` was given.
- A label spec is malformed: missing or empty `text=`, neither `to=` nor `length=`, both of them, `from=` at or after `to=`, a zero or non-numeric `size=`, an unknown key, a duplicate key, an invalid `position=`, or a `color=`/`background=` that is not color syntax.

Soft case: a label window that starts at or after the end of the source video is **not** an error — the rest of the labels are still drawn. `label` reads back the same `time=` token from ffmpeg's stderr that `cut` uses, and warns per unreachable label, naming it so the user knows which one to fix. A label window that merely *runs past* the end is not reported: the label is on screen for the footage that exists, which is what was asked for.

Two ffmpeg build problems surface only here, and are translated into instructions rather than a raw filtergraph error: an ffmpeg built without libfreetype has no `drawtext` filter at all, and one without a working fontconfig cannot resolve a default font (`--font` is the way out of the second).

### Timespec Format

One parser serves every time-valued flag across both commands, at millisecond precision. Two accepted notations:

- **Unit-suffixed**: a sequence of `<number><unit>` pairs. Units: `ms`, `s`, `m`, `h`. The number may carry a decimal point. Examples: `10s`, `1500ms`, `1.5s`, `2m`, `1h30m`, `1h30m10s`, `0.25m`.
- **Timestamp**: `HH:MM:SS[.mmm]` or `MM:SS[.mmm]`. Examples: `00:01:30.500`, `01:30`, `1:02:03.250`.

Fractions below one millisecond are rounded to the nearest millisecond. `ms` must be tried before `m` when matching units, or `1500ms` parses as 1500 minutes.

This replaces the seconds-only parser. Two consequences elsewhere:

- Durations are carried as `Duration` with millisecond resolution, so `-d`/`-e` accept sub-second values. `ffmpeg -t` and `-segment_time` are given fractional seconds (`10.500`) rather than `as_secs()`.
- Zero is no longer rejected inside the parser, because `--from 0s` is legitimate. The "must be greater than zero" check moves to the flags where it applies: `-d`, `-e`, and `--length`.

### Cut Accuracy

- Default: **re-encode**. `-ss` is placed before `-i` for a fast seek, and the range is re-encoded with the same codecs as a capture (`libx264 -preset ultrafast -crf 23`, `aac -b:a 128k`). The result starts exactly at the requested offset regardless of where the source keyframes sit.
- `--fast`: stream copy (`-c copy -avoid_negative_ts make_zero`). Near-instant and lossless, but the cut start snaps back to the nearest preceding keyframe, so the result can begin seconds early.
- `--to` is normalised to a length (`to - from`) before the command is built, so both spellings produce the same `-ss ... -t ...` command.
- Time values are passed to ffmpeg as seconds with three decimals (`10.500`).

See `docs/adr/0002-re-encode-cuts-by-default.md`.

### Cut Output Path

- `-o` accepts a file or a directory. It is treated as a directory when the path exists and is a directory, or when it ends in a path separator (creating it if missing, reusing `resolve_output_directory`). Otherwise it is the output file path, and its parent directory must already exist.
- With no `-o`, the cut is written beside the source video as `<source-stem>_cut.mp4`.
- When `-o` names a directory, the file inside it uses that same `<source-stem>_cut.mp4` name.
- Collisions auto-increment through the existing `avoid_collision` helper: `talk_cut_1.mp4`, `talk_cut_2.mp4`, …
- The output extension is always `.mp4`, whatever the source container is.
- The source video is opened read-only and never written to. A cut that would write over its own source is refused.

### Cut Range Validation

Hard errors (colored message, exit code `1`, no file written):

- The source path does not exist, or is not a file.
- Neither `--to` nor `--length` was given.
- `--from` is greater than or equal to `--to`.
- `--length` is zero.

Soft case: a cut range extending past the end of the source video is **not** an error. ffmpeg writes the footage that exists and stops. To report it without adding an `ffprobe` dependency, the cut module parses the last `time=` token from ffmpeg's stderr — the same stderr-scraping approach already used to parse the avfoundation device listing — and prints a warning when the written range falls short of the requested length by more than 250 ms.

### File Naming

- Pattern: `vidcapture_YYYY-MM-DD_HH-MM-SS.mp4`
- Interval segments: `vidcapture_YYYY-MM-DD_HH-MM-SS_segNNN.mp4`
- Cuts: `<source-stem>_cut.mp4`
- Labeled videos: `<source-stem>_labeled.mp4`
- Auto-increment: if file exists, append `_1`, `_2`, etc.
- Default output directory: current working directory for `start`, the source video's directory for `cut` and `label`.

### Error Handling

- Colored error output to stderr via crossterm.
- Clean up partial/unfinished files on error or crash, including a partially written cut.
- Non-zero exit code (`1`) on failure.
- `anyhow` for error context propagation.
- Clear error if BlackHole is not detected. Note that neither `cut` nor `label` needs BlackHole or screen recording permission, so neither must run the avfoundation device detection that `start` does.

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
- `BlackHole 2ch` — virtual audio device for system audio capture (required by `start` only)

No new dependency is introduced by the cut feature; in particular, `ffprobe` is deliberately not used.

## Testing Decisions

- **cli module**: Unit tests for argument parsing — valid flags, invalid timespecs, missing subcommands, `--to`/`--length` conflict, missing cut range, `--from` defaulting to zero. Thorough table-driven tests for the timespec parser: every unit, decimals, compound values, both timestamp shapes, `1500ms` vs `1500m`, rounding, and rejections.
- **ffmpeg module**: Unit tests for command string building — verify correct ffmpeg args for each mode (simple, duration, interval, combined) and for both cut modes (re-encode and `--fast`), including `-ss` position and the fractional-seconds formatting. For the label command, the `drawtext` chain per label, the placement expressions, the `enable` gate, and the filtergraph escaping — whose per-character backslash counts are verified against ffmpeg itself by rendering each character and reading it back off the frame, not by reasoning about the parser. Also the stderr `time=` scraping that drives both the short-cut and unreachable-label warnings.
- **output module**: Unit tests for filename generation, directory resolution, auto-increment logic, and cut output resolution (file vs directory `-o`, default beside source, extension override, refusing to overwrite the source).
- **cut module**: Unit tests for range validation; integration test cutting a tiny fixture video end to end.
- **label module**: Unit tests for the ffmpeg-build diagnostics. Unit tests for label spec parsing (every key, the defaults, comma and backslash escaping, and each rejection) live with the parser in the cli module, and tests for the `drawtext` chain with the builder in the ffmpeg module. Integration tests labeling a tiny fixture video end to end, including the unreachable-label warning and the source being left untouched.
- **capture module**: Integration tests with mocked ffmpeg interface — verify start/stop/interval orchestration.
- **terminal module**: Manual/integration testing — raw mode behavior is hard to unit test.

Priority: timespec parsing, ffmpeg command building, and output path resolution should have thorough unit tests.

## Out of Scope

- Cross-platform support (Linux, Windows) — macOS only for v1.
- GUI interface.
- Extracting more than one cut range per invocation.
- Concatenating, re-ordering, or otherwise joining videos.
- Video filters, scaling, or codec/quality selection on cuts (always H.264 + AAC).
- Anything drawn on a video other than a timed text label: images, logos, shapes, blurs, or a progress bar.
- Per-label horizontal placement, alignment, fonts, or animation — a label is centred, static, and drawn in one font per pass.
- Editing or removing a label once it is burned in. Labels are drawn into the pixels; re-labeling means going back to the source video.
- Cutting in place (a cut never modifies its source).
- Webcam overlay / picture-in-picture.
- Custom codec selection (always H.264 + AAC).
- Custom ffmpeg arguments passthrough.
- Remote/network recording.
- Pause/resume during capture.

## Further Notes

- The BlackHole setup mirrors the interview-assistant project. Consider documenting the Audio MIDI Setup configuration (Multi-Output Device) in a README.
- ffmpeg's `avfoundation` device list can be queried with `ffmpeg -f avfoundation -list_devices true -i ""` — useful for validating setup.
- The `help` command should include setup instructions (ffmpeg install, BlackHole configuration), the `cut` and `label` commands, the label spec keys, and the timespec format.
- Extending the timespec parser touches `-d` and `-e`, which currently pass `as_secs()` to ffmpeg. That truncation has to go, or `-d 1.5s` silently records for one second.
