# vidcapture

[![Crates.io](https://img.shields.io/crates/v/vidcapture.svg)](https://crates.io/crates/vidcapture)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](#requirements)

Record your screen and audio from the terminal. Stop it with one key. Cut a
precise range out of the result, and caption it, without opening an editor.

```
$ vidcapture start
Capturing [12s elapsed], press s to stop.
Saved to vidcapture_2026-08-27_21-40-03.mp4

$ vidcapture cut vidcapture_2026-08-27_21-40-03.mp4 --from 3s --to 9s
Cut saved to vidcapture_2026-08-27_21-40-03_cut.mp4

$ vidcapture label talk.mp4 -l "text=Setting up,from=1m32s,to=2m"
Labeled video saved to talk_labeled.mp4
```

No GUI, no project files, no export dialog — one binary that shells out to
`ffmpeg` and gets out of the way.

## Features

- **`start`** — records the full screen plus system audio and microphone,
  mixed into one track, as H.264/AAC MP4.
- **Stop on demand or on a timer** — press `s` to stop, or set `-d 30s` /
  `-d 2m` to stop automatically.
- **Interval mode** (`-e 10s`) — splits a long recording into seamless,
  independently playable segments as it goes, so a crash only costs the
  current segment.
- **`cut`** — pulls a millisecond-precise range out of any existing video
  into a new file. The source is opened read-only and never modified.
  Re-encodes by default for a frame-accurate start; `--fast` stream-copies
  for a near-instant, keyframe-aligned cut.
- **`label`** — draws timed text onto any existing video: each label appears
  for the span you give it and disappears again. Repeat `-l` for as many
  labels as you need, each with its own position (top or bottom), text color,
  font size, and optional background band. The source is never modified.
- **One timespec format everywhere** — `10s`, `1500ms`, `1h30m10s`, or
  `00:01:30.500`, accepted by every time-valued flag and label spec key.
- **No lingering partial files** — a failed capture or cut cleans up after
  itself.

## Requirements

- macOS (uses `ScreenCaptureKit` via `ffmpeg`'s `avfoundation` input; not
  portable to Linux/Windows).
- [ffmpeg](https://ffmpeg.org): `brew install ffmpeg`. `label` additionally
  needs an ffmpeg built with libfreetype for its `drawtext` filter — the
  Homebrew build is.
- [BlackHole 2ch](https://github.com/ExistentialAudio/BlackHole), only for
  `start` (system audio capture): `brew install blackhole-2ch`, then a
  one-time Multi-Output Device setup — run `vidcapture help` for the exact
  steps. **`cut` and `label` need neither BlackHole nor screen-recording
  permission.**

## Install

```
cargo install vidcapture
```

Or build from source:

```
git clone https://github.com/elvisbrevi/vidcapture
cd vidcapture
cargo install --path .
```

Re-running either command upgrades an existing install in place.

## Usage

```
vidcapture start                      # record until you press 's'
vidcapture start -d 30s               # stop automatically after 30 seconds
vidcapture start -e 10s               # split into 10-second segments
vidcapture start -o ./recordings/     # save into ./recordings/

vidcapture cut talk.mp4 --length 5s               # first 5 seconds
vidcapture cut talk.mp4 --from 10s --to 25s       # 10s through 25s
vidcapture cut talk.mp4 --from 1m --length 1500ms --fast   # instant, no re-encode

# One label across the bottom, from 1m32s to 2m:
vidcapture label talk.mp4 -l "text=Setting up,from=1m32s,to=2m"

# Several labels in one pass, each styled on its own:
vidcapture label talk.mp4 \
    -l "text=Intro,from=0s,to=1m32s,position=top,background=black@0.5" \
    -l "text=Setting up,from=1m32s,to=2m" \
    -l "text=Live demo,from=2m,length=90s,color=#ffcc00,size=48,background=black@0.6"
```

### Label specs

Each `-l` takes one label as comma-separated `key=value` pairs:

| Key | Meaning | Default |
| --- | --- | --- |
| `text` | The text to draw. **Required.** | — |
| `from` | When the label appears. | `0s` |
| `to` | When it disappears. Use this *or* `length`. | — |
| `length` | How long it stays up. Use this *or* `to`. | — |
| `position` | `top` or `bottom`. | `bottom` |
| `color` | Text color. | `white` |
| `size` | Font size in pixels. | `32` |
| `background` | Color of the band behind the text. | none |

Times take any timespec (`92s`, `1m32s`, `00:01:32`). Colors are ffmpeg
names or `#RRGGBB`, with an optional alpha suffix: `white`, `#ffcc00`,
`black@0.5`. To put a literal comma in a label's text, write `\,`.

The labeled video is written beside the source as `talk_labeled.mp4` unless
`-o` says otherwise; `talk.mp4` itself is never modified. Labels are drawn
into the pixels, so re-labeling means going back to the source.

Every flag, the full timespec grammar, and BlackHole setup instructions are
in `vidcapture help`.

## Claude Code integration

This repo ships a [Claude Code](https://claude.com/claude-code) skill
(`.claude/skills/ship-feature/SKILL.md`) that encodes the project's own
build/review loop — where the spec and coding standards live, module
ownership rules, and known test gotchas — for anyone extending vidcapture
with Claude Code.

A release build (`cargo install`, `cargo build --release`) copies it to
`~/.claude/skills/vidcapture-ship-feature/`, kept in sync on every reinstall.
This never touches a debug build, and is skipped entirely if `~/.claude`
doesn't exist or `VIDCAPTURE_SKIP_SKILL_INSTALL=1` is set. See `build.rs`.

## Design docs

- [`PRD.md`](PRD.md) — product spec and implementation decisions
- [`CONTEXT.md`](CONTEXT.md) — domain vocabulary
- [`docs/adr/`](docs/adr) — architecture decision records

## License

[MIT](LICENSE)
