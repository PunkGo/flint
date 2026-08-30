//! Bakes the build's git identity into `--version` (e.g. `flint 0.1.3 (d672402)`),
//! with `+dirty` when the source that went into this binary differs from that commit.
//! Version strings alone proved useless for telling binaries apart across machines;
//! the hash is the receipt — so it must never claim clean while it is not.
//! Non-git builds (crates.io tarball) get a bare version — the suffix is empty.

use std::process::Command;

/// The paths whose contents end up in the binary. Both the dirty check and the
/// rerun triggers use exactly this list: watching only `.git/HEAD` + `.git/index`
/// let an edited-but-unstaged tree build with a stale `clean` suffix, since editing
/// a tracked file touches neither (only `git add` does).
const SOURCE_PATHS: &[&str] = &[
    "src",
    "build.rs",
    "Cargo.toml",
    "../flint-core/src",
    "../flint-core/Cargo.toml",
    "../../Cargo.toml",
    "../../Cargo.lock",
];

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    let suffix = match git(&["rev-parse", "--short", "HEAD"]) {
        Some(hash) if !hash.is_empty() => {
            let mut args = vec!["status", "--porcelain", "--"];
            args.extend_from_slice(SOURCE_PATHS);
            // A failed git call counts as dirty: claiming clean is the one error
            // this suffix exists to prevent.
            let dirty = git(&args).is_none_or(|s| !s.is_empty());
            if dirty { format!(" ({hash}+dirty)") } else { format!(" ({hash})") }
        }
        _ => String::new(),
    };
    println!("cargo:rustc-env=FLINT_VERSION_SUFFIX={suffix}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    for p in SOURCE_PATHS {
        println!("cargo:rerun-if-changed={p}");
    }
}
