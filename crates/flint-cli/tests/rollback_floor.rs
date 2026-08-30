//! Persistent anti-rollback floor (spec §3.5 / 12.6): `canon pick` advances a durable
//! high-water epoch, and the hook floors the accepted epoch at it — so a checkout of an OLD
//! but validly-signed manifest is rejected even when config `min_epoch` was never bumped.
//!
//! WEAK TIER (honest): a same-UID agent can edit the floor file just as it can checkout an old
//! manifest — this exercises that the mechanism WORKS + advances by default, not that it is a
//! hard boundary against a same-UID adversary (that needs the floor on OS-protected state).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn flint_bin() -> &'static str {
    env!("CARGO_BIN_EXE_flint")
}

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(flint_bin())
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()
        .expect("run flint")
}

#[test]
fn pick_advances_floor_and_a_rollback_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(".flint");

    let out = run(tmp.path(), &["init", "--home", home.to_str().unwrap()]);
    assert!(out.status.success(), "init: {}", String::from_utf8_lossy(&out.stderr));
    // The canon starts empty — seed one example law so accept --all has something to sign.
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/laws/lsp-over-grep.md"),
        home.join("canon/rules/lsp-over-grep.md"),
    )
    .unwrap();
    let config = home.join("flint.toml");
    let key = home.join("keys/sovereign_ed25519");
    let floor = home.join("epoch_floor");

    // First signed pick (accept --all → epoch 1). The floor advances to the signed epoch.
    let out = run(tmp.path(), &["law", "accept", "--all", "--config", config.to_str().unwrap(), "--key", key.to_str().unwrap()]);
    assert!(out.status.success(), "accept --all: {}", String::from_utf8_lossy(&out.stderr));
    assert!(floor.exists(), "pick must write the persistent epoch floor");
    let floor_v: u64 = fs::read_to_string(&floor).unwrap().trim().parse().unwrap();
    assert_eq!(floor_v, 1, "floor advances to the freshly-signed epoch");

    // At the current epoch the canon resolves fine (floor 1, epoch 1 — not a rollback).
    let out = run(tmp.path(), &["canon", "list", "--config", config.to_str().unwrap()]);
    assert!(out.status.success(), "canon list at current epoch: {}", String::from_utf8_lossy(&out.stderr));

    // Simulate the rollback: a LATER pick advanced the floor past this manifest's epoch, but
    // the on-disk manifest is still the old signed one (a `git checkout` of an old canon). The
    // gate must now reject it as a rollback, even though config min_epoch is still 0.
    fs::write(&floor, "5\n").unwrap();
    let out = run(tmp.path(), &["canon", "list", "--config", config.to_str().unwrap()]);
    assert!(!out.status.success(), "floor above the signed epoch → rollback rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("rollback") || err.contains("epoch"), "rejection names the rollback: {err}");
}

#[test]
fn explicit_epoch_at_or_below_floor_is_rejected() {
    // After the first pick the floor is 1. An explicit `--epoch` AT the floor (a same-epoch
    // collision — same-epoch rollback would go undetected) or BELOW it (self-lock — the hook
    // rejects the pick's own output) must be refused BEFORE anything is installed (codex P1).
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(".flint");
    let out = run(tmp.path(), &["init", "--home", home.to_str().unwrap()]);
    assert!(out.status.success(), "init: {}", String::from_utf8_lossy(&out.stderr));
    // The canon starts empty — seed one example law so accept --all has something to sign.
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/laws/lsp-over-grep.md"),
        home.join("canon/rules/lsp-over-grep.md"),
    )
    .unwrap();
    let config = home.join("flint.toml");
    let key = home.join("keys/sovereign_ed25519");
    let out = run(tmp.path(), &["law", "accept", "--all", "--config", config.to_str().unwrap(), "--key", key.to_str().unwrap()]);
    assert!(out.status.success(), "accept: {}", String::from_utf8_lossy(&out.stderr));

    // Snapshot the signed manifest to prove a rejected pick installs NOTHING.
    let manifest = home.join("canon/CANON.manifest");
    let before = fs::read(&manifest).unwrap();

    for bad in ["1", "0"] {
        let out = run(tmp.path(), &["canon", "pick", "--config", config.to_str().unwrap(), "--key", key.to_str().unwrap(), "--epoch", bad]);
        assert!(!out.status.success(), "--epoch {bad} at/below floor must be rejected");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("floor") || err.contains("rollback"), "rejection names the floor: {err}");
    }
    assert_eq!(fs::read(&manifest).unwrap(), before, "a rejected pick must not replace the signed manifest");

    // A higher explicit epoch still works and advances the floor.
    let out = run(tmp.path(), &["canon", "pick", "--config", config.to_str().unwrap(), "--key", key.to_str().unwrap(), "--epoch", "9"]);
    assert!(out.status.success(), "explicit epoch above the floor works: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fs::read_to_string(home.join("epoch_floor")).unwrap().trim(), "9", "floor advanced to 9");
}

#[test]
fn a_saturated_floor_refuses_both_epoch_paths_loudly() {
    // The `checked_add` guard: at `u64::MAX` there is no epoch strictly above the floor, so
    // auto-epoch must be a loud error rather than a saturating collision AT the floor (which
    // would be a same-epoch rollback the hook cannot detect). The explicit path must refuse
    // too. Neither branch had coverage when it was introduced.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join(".flint");
    let out = run(tmp.path(), &["init", "--home", home.to_str().unwrap()]);
    assert!(out.status.success(), "init: {}", String::from_utf8_lossy(&out.stderr));
    // The canon starts empty — seed one example law so accept --all has something to sign.
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/laws/lsp-over-grep.md"),
        home.join("canon/rules/lsp-over-grep.md"),
    )
    .unwrap();
    let config = home.join("flint.toml");
    let key = home.join("keys/sovereign_ed25519");
    let out = run(tmp.path(), &["law", "accept", "--all", "--config", config.to_str().unwrap(), "--key", key.to_str().unwrap()]);
    assert!(out.status.success(), "accept: {}", String::from_utf8_lossy(&out.stderr));

    // Saturate the floor. (Same-UID editability is the declared weak-tier boundary; here it is
    // simply the cheapest way to reach the u64::MAX branch.)
    fs::write(home.join("epoch_floor"), format!("{}\n", u64::MAX)).unwrap();
    let manifest = home.join("canon/CANON.manifest");
    let before = fs::read(&manifest).unwrap();

    let c = config.to_str().unwrap();
    let k = key.to_str().unwrap();
    let auto = run(tmp.path(), &["canon", "pick", "--config", c, "--key", k]);
    assert!(!auto.status.success(), "auto-epoch at a saturated floor must fail, not collide at MAX");
    let err = String::from_utf8_lossy(&auto.stderr);
    assert!(err.contains("u64::MAX") || err.contains("advance"), "the refusal explains the saturation: {err}");

    let explicit = run(tmp.path(), &["canon", "pick", "--config", c, "--key", k, "--epoch", &u64::MAX.to_string()]);
    assert!(!explicit.status.success(), "an explicit epoch AT the saturated floor must be refused");
    let err = String::from_utf8_lossy(&explicit.stderr);
    assert!(err.contains("floor") || err.contains("rollback"), "the refusal names the floor: {err}");

    assert_eq!(fs::read(&manifest).unwrap(), before, "no refused pick may replace the signed manifest");
}
