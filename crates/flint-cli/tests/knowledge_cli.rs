//! Regression coverage for the knowledge-layer CLI — the three codex-review P1s:
//!   1. `flint pit mark` follows the ACTIVE ore store (not the legacy `pits_root` alone), so an
//!      `[[ore_store]]`-only config still captures.
//!   2. an ambiguous store label is refused, never a silent first-match.
//!   3. `--gist <text>` selects a gist by STABLE identity — a shifted index can't misroute.
//!
//! Plus the follow-ups found by reviewing that fix: contradictory `--gist` + `--index` is
//! refused rather than silently reconciled, and a promoted note records the exact gist it came
//! from even when `--body` rewrites the text.
//!
//! End-to-end on the real `flint` binary.

use std::fs;
use std::path::Path;
use std::process::Command;

fn flint_bin() -> &'static str {
    env!("CARGO_BIN_EXE_flint")
}

/// A minimal flint.toml. Knowledge/pit verbs never touch trust, so a placeholder `[trust]` is
/// enough for `FlintConfig::load` to parse. `extra` appends the store config under test.
fn write_config(dir: &Path, extra: &str) -> std::path::PathBuf {
    let cfg = dir.join("flint.toml");
    fs::write(
        &cfg,
        format!(
            "flint_bin = \"flint\"\ncanon_root = {cr:?}\n{extra}\n\n[trust]\nallowed_signers = {al:?}\nsigner_identity = \"s\"\nscope = \"t\"\n",
            cr = dir.join("canon"),
            al = dir.join("allowed_signers"),
        ),
    )
    .unwrap();
    cfg
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(flint_bin()).args(args).output().expect("run flint");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

#[test]
fn pit_mark_writes_legacy_pits_root_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(dir.path(), &format!("pits_root = {:?}", dir.path().join("pits")));
    let (ok, o) = run(&["pit", "mark", "--config", cfg.to_str().unwrap(), "a wall"]);
    assert!(ok, "pit mark failed: {o}");
    let inbox = fs::read_to_string(dir.path().join("pits/inbox.md")).unwrap();
    assert!(inbox.contains("- a wall"));
}

#[test]
fn pit_mark_follows_active_ore_store_without_pits_root() {
    // codex P1 #3: an [[ore_store]]-only config (no pits_root) must still capture into the ACTIVE
    // store, not error / write to a missing pits_root.
    let dir = tempfile::tempdir().unwrap();
    let active = dir.path().join("obsidian");
    let cfg = write_config(
        dir.path(),
        &format!("[[ore_store]]\npath = {active:?}\nactive = true\nlabel = \"obsi\""),
    );
    let (ok, o) = run(&["pit", "mark", "--config", cfg.to_str().unwrap(), "into the active store"]);
    assert!(ok, "pit mark must succeed with an [[ore_store]]-only config: {o}");
    let inbox = fs::read_to_string(active.join("inbox.md"))
        .expect("the gist must land in the ACTIVE store, not a missing pits_root");
    assert!(inbox.contains("- into the active store"));
}

#[test]
fn selection_refuses_ambiguous_store_label() {
    // codex P1 #2: two stores whose basename-label collides ("pits") must not be silently
    // disambiguated to the first — `--from pits` is ambiguous and must error.
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a/pits"), dir.path().join("b/pits"));
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    let cfg = write_config(
        dir.path(),
        &format!("[[ore_store]]\npath = {a:?}\nactive = true\n\n[[ore_store]]\npath = {b:?}"),
    );
    let (ok, o) = run(&[
        "knowledge", "toss", "--config", cfg.to_str().unwrap(), "--from", "pits", "--index", "0",
    ]);
    assert!(!ok, "an ambiguous label must error, not act on the first store: {o}");
    assert!(o.contains("ambiguous"), "the error must name the ambiguity: {o}");
}

#[test]
fn gist_and_index_together_are_refused_not_silently_resolved() {
    // Two selectors that disagree used to resolve silently to `--gist` (the match arm was
    // `(Some(g), _)`), which contradicts this path's own invariant — a mis-selection must never
    // silently act on the wrong gist. Contradictory input is refused, on both verbs.
    let dir = tempfile::tempdir().unwrap();
    let pits = dir.path().join("pits");
    let cfg = write_config(
        dir.path(),
        &format!("pits_root = {pits:?}\nknowledge_root = {:?}", dir.path().join("knowledge")),
    );
    for g in ["alpha gist", "beta gist"] {
        assert!(run(&["pit", "mark", "--config", cfg.to_str().unwrap(), g]).0);
    }
    let c = cfg.to_str().unwrap();
    for args in [
        vec!["knowledge", "promote", "--config", c, "--from", "pits", "--gist", "alpha gist", "--index", "1", "--id", "x"],
        vec!["knowledge", "toss", "--config", c, "--from", "pits", "--gist", "alpha gist", "--index", "1"],
    ] {
        let (ok, o) = run(&args);
        assert!(!ok, "contradictory --gist + --index must be refused: {o}");
        assert!(
            o.contains("cannot be used with") || o.contains("--index"),
            "the refusal names the conflicting selectors: {o}"
        );
    }
    // Nothing was acted on: both gists are still pending.
    let inbox = fs::read_to_string(pits.join("inbox.md")).unwrap();
    assert!(inbox.contains("- alpha gist") && inbox.contains("- beta gist"), "a refused selection acts on nothing");
}

#[test]
fn promoted_note_records_the_exact_gist_even_with_a_custom_body() {
    // Provenance: `source` used to carry only the store label, so a promote with `--body` left
    // the durable note with NO link back to the raw gist it resolved out — the exact text was
    // echoed to stdout only, and stdout is not the account. The note is.
    let dir = tempfile::tempdir().unwrap();
    let pits = dir.path().join("pits");
    let cfg = write_config(
        dir.path(),
        &format!("pits_root = {pits:?}\nknowledge_root = {:?}", dir.path().join("knowledge")),
    );
    assert!(run(&["pit", "mark", "--config", cfg.to_str().unwrap(), "the raw wall I hit"]).0);
    let (ok, o) = run(&[
        "knowledge", "promote", "--config", cfg.to_str().unwrap(),
        "--from", "pits", "--gist", "the raw wall I hit",
        "--id", "refined", "--body", "a rewritten, generalised lesson",
    ]);
    assert!(ok, "promote failed: {o}");
    let note = fs::read_to_string(dir.path().join("knowledge/refined.md")).unwrap();
    assert!(note.contains("a rewritten, generalised lesson"), "the custom body is the note body:\n{note}");
    assert!(note.contains("the raw wall I hit"), "the note must record the gist it came from:\n{note}");
    assert!(note.contains("pits"), "the note must still record the store:\n{note}");
}

#[test]
fn promote_by_gist_is_stable_across_index_shift() {
    // codex P1 #1: selecting by --gist text promotes the RIGHT gist regardless of index shifts.
    let dir = tempfile::tempdir().unwrap();
    let pits = dir.path().join("pits");
    let cfg = write_config(
        dir.path(),
        &format!("pits_root = {pits:?}\nknowledge_root = {:?}", dir.path().join("knowledge")),
    );
    for g in ["first gist", "second gist", "third gist"] {
        assert!(run(&["pit", "mark", "--config", cfg.to_str().unwrap(), g]).0);
    }
    // Promote by EXACT text — the SECOND gist, wherever its index currently is.
    let (ok, o) = run(&[
        "knowledge", "promote", "--config", cfg.to_str().unwrap(),
        "--from", "pits", "--gist", "second gist", "--id", "second-note",
    ]);
    assert!(ok, "promote --gist failed: {o}");
    let note = fs::read_to_string(dir.path().join("knowledge/second-note.md")).unwrap();
    assert!(note.contains("second gist"), "the SECOND gist must be promoted:\n{note}");
    let inbox = fs::read_to_string(pits.join("inbox.md")).unwrap();
    assert!(!inbox.contains("- second gist"), "the promoted gist must be resolved out");
    assert!(inbox.contains("- first gist") && inbox.contains("- third gist"), "the others must remain");
}

#[test]
fn promote_unknown_gist_errors_not_silent() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = write_config(
        dir.path(),
        &format!("pits_root = {:?}\nknowledge_root = {:?}", dir.path().join("pits"), dir.path().join("knowledge")),
    );
    assert!(run(&["pit", "mark", "--config", cfg.to_str().unwrap(), "the only gist"]).0);
    let (ok, o) = run(&[
        "knowledge", "promote", "--config", cfg.to_str().unwrap(),
        "--from", "pits", "--gist", "no such gist", "--id", "x",
    ]);
    assert!(!ok, "a --gist that matches nothing must error, not promote something: {o}");
    assert!(o.contains("no pending gist"), "error should say the gist is not pending: {o}");
}
