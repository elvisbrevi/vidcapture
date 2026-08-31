# Draw labels with one drawtext filter per label, and escape their text

`vidcapture label` builds a single `-vf` filtergraph holding one `drawtext` filter per label, chained with commas and each gated to its own label window by `enable='between(t,<start>,<end>)'`. Every label's text is escaped before it goes into that filtergraph, with a per-character backslash count verified against ffmpeg rather than derived from the docs.

**Why one filter per label**: a label's window, position, color, size, and background are all `drawtext` options, so a label maps one-to-one onto a filter. Chaining them means one ffmpeg process and one re-encode however many labels there are, and the chain order is the draw order, which is what a user reading their own command expects. `enable` is evaluated per frame, so the window costs nothing beyond the filter itself.

**Why escaping is its own concern**: a filtergraph is a string, and a label's text is user input going into it. An unescaped `:` ends an option and an unescaped `,` starts a new filter — so `text=Part 1: setup, done` does not merely render wrong, it changes the filtergraph. Escaping is what keeps a label's text a value rather than syntax.

The counts are not obvious and are not uniform, because the value is read twice over — the graph is split into filters and options, then each option value is unescaped — and every pass that treats a character as syntax eats one backslash, including the backslash written for the inner pass:

| Character | Backslashes |
| --- | --- |
| `,` `;` `[` `]` | 1 |
| `:` | 2 |
| `'` | 3 |
| `\` | 4 |

Each count was established by rendering the character into a frame with the real ffmpeg and reading it back, because the plausible-looking counts are wrong in a way that is invisible from the outside: too few backslashes and `drawtext` silently draws nothing at all rather than failing, so a label just quietly does not appear.

**Why `expansion=none`**: by default `drawtext` reads its own text a third time to expand `%{...}` into frame metadata. A label's text is literal — nobody captioning a demo means `%{pts}` as an expression — and turning the pass off does three things at once: `50% done` renders as written, `%` stops needing an escape at all, and every count above drops by one, since a backslash written for the innermost pass would have had to survive the outer two as well. It also makes one escaper correct for both the text and the `--font` path, which is never expanded. Verifying with a `textfile=` reference instead would not have caught any of this: `drawtext` expands text the same way whichever option it came from, so for `%` and `\` both sides come back blank and the comparison reports a match.

**Alternatives considered**:

- **`textfile=` instead of `text=`** — writes each label's text to a temp file, which sidesteps text escaping entirely. Rejected: it trades a pure function for temp-file lifecycle across a process that already has partial-output cleanup to do, and it does not remove the escaping problem, only moves it to the path. The escaper is testable in isolation; a temp file is not.
- **Rejecting text containing filter-special characters** — rejected: `50%`, `it's`, and `Part 1: setup` are ordinary label text, and refusing them to avoid an escaping function would be the tool's problem leaking into the user's captions.
- **`subtitles=` with a generated SRT/ASS file** — rejected: it gets timing for free but moves styling into ASS override tags, adds a subtitle-format dependency and a temp file, and gives no simpler answer for per-label position and background.
- **`drawbox` + `drawtext` for a full-width background band** — rejected: it needs the text's height computed ahead of time to place the box, where `drawtext`'s own `boxborderw` produces the same band around the text with one option and no arithmetic.
