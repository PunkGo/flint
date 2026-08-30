//! Bakes the build's git identity into `--version` (e.g. `flint 0.1.1 (d672402)`),
//! with `+dirty` when built from an uncommitted tree. Version strings alone proved
//! useless for telling binaries apart across machines; the hash is the receipt.
//! Non-git builds (crates.io tarball) get a bare version — the suffix is empty.

use std::process::Command;

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
            let dirty = git(&["status", "--porcelain"]).is_none_or(|s| !s.is_empty());
            if dirty { format!(" ({hash}+dirty)") } else { format!(" ({hash})") }
        }
        _ => String::new(),
    };
    println!("cargo:rustc-env=FLINT_VERSION_SUFFIX={suffix}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
