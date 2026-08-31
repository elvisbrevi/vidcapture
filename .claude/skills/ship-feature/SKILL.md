---
name: ship-feature
description: Repo-specific loop for building or fixing a vidcapture CLI feature — where the spec and standards live, module ownership rules, test seams, and known gotchas. Use when implementing a GitHub issue for elvisbrevi/vidcapture, or applying findings from a code-review pass on this repo.
---

# Ship a vidcapture feature

## Spec and standards, for `code-review`

- Spec source: `gh issue view <n>` (elvisbrevi/vidcapture) — acceptance criteria are the seams to test. PRD.md's user stories give the "why"; its Architecture section is the module map.
- Standards sources: CONTEXT.md (domain vocabulary — test/type names must match its terms, not synonyms it lists as "Avoid"), PRD.md, `docs/adr/*.md`.

## The loop

1. `tdd` skill, one seam at a time, against the issue's acceptance criteria.
2. `cargo test` and `cargo clippy --all-targets` green.
3. `code-review` skill against the commit — Standards axis reads CONTEXT.md/PRD.md/ADRs; Spec axis reads the issue.
4. `ponytail` lens on the findings before fixing: skip speculative generality, prefer reusing an existing parser/helper over a new one.
5. Fix via `tdd` again, then repeat step 3 — don't call it done until a review pass finds nothing new.

## Module ownership (repeat finding across two reviews)

`ffmpeg.rs` owns spawning and running ffmpeg processes (`run_to_completion`, `build_*_command`), building the filter strings they carry, and parsing what they write to stderr (`parse_avfoundation_listing`, `parse_written_length`). `cut.rs`/`label.rs`/`capture.rs` only orchestrate — decide what to run and react to the result. New process-spawning code that isn't in `ffmpeg.rs` is a standards violation here, not a style nit.

`cli.rs` owns every parser and every validation rule, including `parse_label_spec` — an orchestration module receives values that are already valid. `output.rs` owns every output path and the cleanup of a partial one.

Before adding a helper, check whether the one you want already exists under another command's name: `parse_timespec` serves every time-valued flag *and* every time-valued label spec key, `resolve_start_and_length` validates a cut range and a label window alike, and `resolve_derived_output_path` names both `_cut.mp4` and `_labeled.mp4` outputs. A near-copy of one of these is the finding reviews here raise most often after module ownership.

## Test seams

- Unit tests live in `#[cfg(test)] mod tests` inside the file they test.
- End-to-end behavior is asserted in `tests/cli.rs` by spawning `env!("CARGO_BIN_EXE_vidcapture")`; `create_test_video()` there builds a real fixture via `ffmpeg -f lavfi` — reuse it, don't hand-roll another fixture generator.
- A test that changes `std::env::current_dir` mutates process-global state and will race any other test that reads it. Serialize with a `static ... Mutex<()>` guard (see `CWD_LOCK` in `src/output.rs`) rather than adding a new mechanism.

## Gotchas

`cargo fmt` is not applied repo-wide — running it reformats unrelated files and balloons the diff. Don't run it; match the surrounding file's existing style by hand.

**Filtergraph strings are not verifiable by reading.** `escape_filter_value` in `ffmpeg.rs` uses a different backslash count per character (1 for `,;[]`, 2 for `:`, 3 for `'`, 4 for `\`) because a filter option value is parsed by two passes and each eats one. Three traps if you touch it:

- Too few backslashes makes `drawtext` draw *nothing at all* — no error, no exit code, just a video with no label on it. A unit test on the ffmpeg args passes happily; only a rendered frame catches it.
- The counts depend on `expansion=none` staying in the filter. Drop it and `drawtext` reads the text a third time, every count above goes up by one, and `%` needs escaping it does not need today.
- A `textfile=` render is not a valid reference for `%` or `\`: `drawtext` expands its text the same way whichever option it came from, so both sides come back blank and the comparison reports a match.

To change a count, verify it the way it was established: render the character onto a fixture with the real ffmpeg (`-frames:v 1 -update 1 out.png`) and read the frame back. Match a full-frame render against a reference only for characters the outer passes alone consume.

**A temp dir name leaks into assertions.** An e2e test asserting `!stderr.contains("warning")` will fail if its own temp directory is named `..._no_warning` — the path is echoed in the success line. Name fixture directories after the case, not after the assertion.
