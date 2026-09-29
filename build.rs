//! Best-effort side effect of `cargo install`: install the project's portable
//! skill for Claude Code, Codex, and OpenCode without a second manual step.
//!
//! Gated to release builds (what `cargo install` and `cargo build --release`
//! use) so the debug edit-compile-test loop stays free of filesystem side
//! effects outside the target directory. Every write is best-effort: a
//! failure here must never fail the build.

use std::env;
use std::fs;
use std::path::PathBuf;

const SKILL_SOURCE: &str = ".agents/skills/vidcapture/SKILL.md";
const SKILL_TARGET_DIR: &str = "skills/vidcapture";

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
    let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let Ok(contents) = fs::read(PathBuf::from(manifest_dir).join(SKILL_SOURCE)) else {
        return;
    };

    let home = PathBuf::from(home);
    let targets = [
        home.join(".agents").join(SKILL_TARGET_DIR).join("SKILL.md"),
        home.join(".claude").join(SKILL_TARGET_DIR).join("SKILL.md"),
    ];

    for target in targets {
        if fs::read(&target).map(|existing| existing == contents).unwrap_or(false) {
            continue;
        }

        let installed = target
            .parent()
            .map(fs::create_dir_all)
            .transpose()
            .and_then(|_| fs::write(&target, &contents));

        if installed.is_ok() {
            println!(
                "cargo:warning=Installed the vidcapture skill to {}",
                target.display()
            );
        }
    }
}
