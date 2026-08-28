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

`ffmpeg.rs` owns spawning and running ffmpeg processes (`run_to_completion`, `build_*_command`). `cut.rs`/`capture.rs` only orchestrate — decide what to run and react to the result. New process-spawning code that isn't in `ffmpeg.rs` is a standards violation here, not a style nit.

## Test seams

- Unit tests live in `#[cfg(test)] mod tests` inside the file they test.
- End-to-end behavior is asserted in `tests/cli.rs` by spawning `env!("CARGO_BIN_EXE_vidcapture")`; `create_test_video()` there builds a real fixture via `ffmpeg -f lavfi` — reuse it, don't hand-roll another fixture generator.
- A test that changes `std::env::current_dir` mutates process-global state and will race any other test that reads it. Serialize with a `static ... Mutex<()>` guard (see `CWD_LOCK` in `src/output.rs`) rather than adding a new mechanism.

## Gotcha

`cargo fmt` is not applied repo-wide — running it reformats unrelated files and balloons the diff. Don't run it; match the surrounding file's existing style by hand.
