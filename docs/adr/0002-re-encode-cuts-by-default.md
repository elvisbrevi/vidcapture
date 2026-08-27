# Re-encode cuts by default, stream-copy behind --fast

`vidcapture cut` re-encodes the cut range by default (`libx264 -preset ultrafast -crf 23`, `aac -b:a 128k`) with `-ss` placed before `-i`. Stream copy (`-c copy`) is available behind an explicit `--fast` flag.

**Why**: With `-c copy` ffmpeg cannot start a cut anywhere except a keyframe, so it silently rewinds the start to the nearest preceding one. Captures are encoded with `-preset ultrafast`, whose keyframe interval can be seconds long — a cut asked for at `10.500s` could easily begin at `8s`. The whole point of accepting millisecond offsets is that the cut lands where the user asked, so accuracy is the default and speed is opt-in. Re-encoding also normalises every source into the same H.264/AAC MP4 the rest of the tool produces, whatever container came in.

The tradeoff is CPU time and a generation of quality loss on each cut, both proportional to the cut length. `--preset ultrafast` keeps the cost near real time or better, and `--fast` remains there for users cutting long ranges who don't need the precision.

**Alternatives considered**:
- Stream copy by default with an `--accurate` opt-in — rejected: the common case silently produces the wrong range, and a wrong result is worse than a slow one.
- `-ss` after `-i` (decode-and-discard seek) — rejected: accurate but decodes the whole prefix, so cutting near the end of a long video is needlessly slow. Placing `-ss` before `-i` seeks fast and, because we re-encode, is still frame-accurate.
- Two-step cut (copy to keyframe boundary, then re-encode only the head) — rejected: meaningfully more code and process orchestration for a saving that `ultrafast` largely erases.
- Using `ffprobe` to inspect keyframes and pick a strategy per cut — rejected: adds a second system binary to the install instructions for a decision the user can make with one flag.
