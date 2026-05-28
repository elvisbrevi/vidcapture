# vidcapture

CLI screen and audio recorder for macOS. Captures full screen + system audio + microphone via ffmpeg, with timed and interval-based recording modes.

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

### Example dialogue

> **Dev**: "When a user starts a capture session with `-e 10s`, how many segments do we get?"
>
> **Domain expert**: "As many as fit in the session. If they also pass `-d 1m`, that's 6 segments. If no duration, it runs until they press `s`."
>
> **Dev**: "And each segment is a standalone MP4?"
>
> **Domain expert**: "Yes — ffmpeg's segment muxer handles the splitting. No gaps between segments."
