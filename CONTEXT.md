# vidcapture

CLI screen and audio recorder for macOS. Captures full screen + system audio + microphone via ffmpeg, with timed and interval-based recording modes. It also cuts a range out of an existing video file, and draws timed text labels onto one.

## Language

**Capture session**:
A single recording from start to stop. One `vidcapture start` invocation produces one capture session (which may contain multiple segments).
_Avoid_: recording, clip, video

**Segment**:
A portion of a capture session produced by interval mode (`-e`). Each segment is an independent MP4 file. A session without `-e` has exactly one segment.
_Avoid_: chunk, part, split

**System audio**:
Audio output from the machine's speakers, captured via BlackHole virtual audio device. Requires a Multi-Output Device configured in Audio MIDI Setup.
_Avoid_: speaker output, desktop audio

**Microphone**:
Audio input from the user's mic, captured alongside system audio during a capture session.
_Avoid_: mic input, voice

**Duration**:
The time limit for a capture session (`-d`). When reached, the session stops automatically.
_Avoid_: timeout, length

**Interval**:
The time between segment boundaries in interval mode (`-e`). Each segment is this long.
_Avoid_: frequency, period

**Cut**:
A single `vidcapture cut` invocation: reads a source video, extracts one cut range, and writes one new MP4. A cut never modifies the source video.
_Avoid_: trim, clip, edit, splice

**Source video**:
The existing video file a cut reads from, given as the positional argument to `vidcapture cut`. It may or may not have been produced by vidcapture.
_Avoid_: input file, original

**Cut range**:
The portion of the source video a cut extracts, expressed as a start offset (`--from`) plus either an end offset (`--to`) or a cut length (`--length`). Offsets are measured from the beginning of the source video.
_Avoid_: selection, window, slice

**Cut length**:
How long the cut range lasts (`--length`). Equivalent to `--to` minus `--from`; the two ways of expressing a cut range are mutually exclusive.
_Avoid_: size, amount

**Label**:
A piece of text drawn onto a video for a fixed span of time. One `vidcapture label` invocation reads a source video, draws every label given to it, and writes one new MP4. A label never modifies the source video.
_Avoid_: caption, subtitle, overlay, annotation, watermark

**Label window**:
The span a single label is visible for, expressed as a start offset (`from=`) plus either an end offset (`to=`) or a length (`length=`). Offsets are measured from the beginning of the source video, exactly as a cut range's are.
_Avoid_: range, interval, duration, timeframe

**Label spec**:
The `key=value` string given to one `-l/--label` flag, describing one label: its text, its label window, and how it is drawn. One `-l` per label; there is no limit on how many a single invocation takes.
_Avoid_: descriptor, definition, config

**Label background**:
The colored band drawn behind a label's text so the text stays readable over the footage. Drawn by default; `background=none` is how a label spec asks for text with no band behind it.
_Avoid_: box, banner, highlight, fill

**Timespec**:
A user-supplied point in time or span of time, accepted anywhere the CLI takes a time value. Two notations: unit-suffixed (`10s`, `1500ms`, `1h30m`, `1.5s`) and timestamp (`00:01:30.500`). Resolved to millisecond precision.
_Avoid_: duration string, time format

### Example dialogue

> **Dev**: "When a user starts a capture session with `-e 10s`, how many segments do we get?"
>
> **Domain expert**: "As many as fit in the session. If they also pass `-d 1m`, that's 6 segments. If no duration, it runs until they press `s`."
>
> **Dev**: "And each segment is a standalone MP4?"
>
> **Domain expert**: "Yes — ffmpeg's segment muxer handles the splitting. No gaps between segments."
>
> **Dev**: "And `vidcapture cut talk.mp4 --from 1m --length 1500ms`?"
>
> **Domain expert**: "One cut. The cut range starts one minute into the source video and its cut length is a second and a half. We write `talk_cut.mp4` next to the source and leave `talk.mp4` untouched."
>
> **Dev**: "What if they pass both `--to` and `--length`?"
>
> **Domain expert**: "That's an error. A cut range is either an end offset or a cut length, never both."
>
> **Dev**: "And `vidcapture label talk.mp4 -l \"text=Intro,from=1m32s,to=2m,position=top\"`?"
>
> **Domain expert**: "One label. Its label window runs from a minute and a half in to the two-minute mark, and it's drawn across the top of the frame for that whole window and nowhere else. Add another `-l` and you get a second label — they're independent, so two can be on screen at once."
>
> **Dev**: "What if a label window starts after the video ends?"
>
> **Domain expert**: "We still write the video and we warn. The label simply never appears, and the user should hear that rather than go looking for it."
