//! Best-effort side effect of `cargo install`: drop the project's Claude Code
//! skill into `~/.claude/skills/` so a contributor who installs the CLI also
//! gets the dev workflow, without a second manual step.
//!
//! Gated to release builds (what `cargo install` and `cargo build --release`
//! use) so the debug edit-compile-test loop stays free of filesystem side
//! effects outside the target directory. Every write is best-effort: a
//! failure here must never fail the build.

use std::env;
use std::fs;
use std::path::PathBuf;

const SKILL_SOURCE: &str = ".claude/skills/ship-feature/SKILL.md";
const SKILL_TARGET_DIR: &str = "skills/vidcapture-ship-feature";

fn main() {
    println!("cargo:rerun-if-env-changed=VIDCAPTURE_SKIP_SKILL_INSTALL");
    println!("cargo:rerun-if-changed={SKILL_SOURCE}");

    if env::var_os("VIDCAPTURE_SKIP_SKILL_INSTALL").is_some() {
        return;
    }
    if env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
    let Some(home) = env::var_os("HOME") else {
        return;
    };
    let claude_dir = PathBuf::from(home).join(".claude");
    if !claude_dir.is_dir() {
        // Claude Code isn't set up on this machine; nothing to integrate with.
        return;
    }

    let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let Ok(contents) = fs::read(PathBuf::from(manifest_dir).join(SKILL_SOURCE)) else {
        return;
    };

    let target = claude_dir.join(SKILL_TARGET_DIR).join("SKILL.md");
    let already_current = fs::read(&target).map(|existing| existing == contents).unwrap_or(false);
    if already_current {
        return;
    }

    let installed = target
        .parent()
        .map(fs::create_dir_all)
        .transpose()
        .and_then(|_| fs::write(&target, &contents));

    if installed.is_ok() {
        println!(
            "cargo:warning=Installed the vidcapture Claude Code skill to {}",
            target.display()
        );
    }
}
