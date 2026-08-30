//! WP-E acceptance (spec §7 concurrency — reconcile-adopted staged defer): the
//! escalation boundary must be documented in code, never silently dropped. The
//! defer is deliberate (single-machine threat model; cross-artifact clobber is
//! folded, cross-process races are benign lost-updates, the enforced canon is
//! written by `canon pick` not install). If someone replaces the
//! read-modify-write with a real lock, they update these tests — that is the
//! point: adding a lock becomes a reviewed decision, not a silent one.
//!
//! These are documentation / structure tripwires, NOT concurrency tests — no
//! two-process race is exercised. WP-E's scope is explicitly "document the defer,
//! don't implement a lock"; these pin that the documentation + the read-modify-write
//! shape stay in place, so an escalation to a real lock cannot land silently.

use std::fs;
use std::path::Path;

fn install_src() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/install.rs");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn concurrency_escalation_boundary_is_documented() {
    let src = install_src();
    assert!(
        src.contains("CONCURRENCY") && src.contains("ESCALATION TRIGGER"),
        "install.rs must document the §7 concurrency staged-defer + its escalation \
         trigger (WP-E) — the deferred lock is an honest boundary, kept visible in code"
    );
}

#[test]
fn installed_lock_is_still_read_modify_write() {
    // The defer is real: installed.lock is a plain fs::write, not a locked section.
    // If this ever becomes a file lock / atomic replace, update the CONCURRENCY note
    // (and this test) so the escalation reads as a reviewed decision.
    let src = install_src();
    assert!(
        src.contains("fs::write(&lock_path"),
        "installed.lock is expected to be a read-modify-write (fs::write) per the §7 \
         staged defer; if you added a lock, update the CONCURRENCY note + this test"
    );
}
