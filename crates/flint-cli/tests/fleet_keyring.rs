//! WP-F acceptance: the fleet keyring (spec §8), end-to-end on the REAL binary. Requires
//! `ssh-keygen`. A locally-generated key stands in for "the other machine" (machine B) — the
//! mechanism is real; only the specific remote public key is the operator's to add.
//!
//! Proves: (1) a key not in the trust set cannot sign a Canon in; (2) once its PUBLIC key is
//! added, a Canon that key signs verifies here (sign once → fleet-wide); (3) removing the key
//! revokes it — the Canon it signed no longer verifies.

use std::path::PathBuf;
use std::process::Command;

fn flint_bin() -> &'static str {
    env!("CARGO_BIN_EXE_flint")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(flint_bin()).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

struct Fleet {
    _dir: tempfile::TempDir,
    home: PathBuf,
    key_b: PathBuf, // the "other machine" (machine-B stand-in)
}

fn setup() -> Fleet {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("machine-a");
    // Machine A: init + accept the default pack (signed by A's key).
    let (c, _o, e) = run(&["init", "--home", home.to_str().unwrap(), "--scope", "fleet-test"]);
    assert_eq!(c, 0, "init: {e}");
    let cfg = home.join("flint.toml");
    let key_a = home.join("keys/sovereign_ed25519");
    let (c, _o, e) = run(&["law", "accept", "--all", "--config", cfg.to_str().unwrap(), "--key", key_a.to_str().unwrap()]);
    assert_eq!(c, 0, "accept: {e}");

    // "Machine B" (machine-B stand-in): a second sovereign key.
    let key_b = dir.path().join("machine-b-key");
    assert!(Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C", "machine-b", "-f"])
        .arg(&key_b)
        .status()
        .unwrap()
        .success());
    Fleet { _dir: dir, home, key_b }
}

impl Fleet {
    fn cfg(&self) -> String {
        self.home.join("flint.toml").to_str().unwrap().to_string()
    }
    fn key_a(&self) -> String {
        self.home.join("keys/sovereign_ed25519").to_str().unwrap().to_string()
    }
    fn key_b(&self) -> String {
        self.key_b.to_str().unwrap().to_string()
    }
    fn pub_b(&self) -> String {
        self.key_b.with_extension("pub").to_str().unwrap().to_string()
    }
}

#[test]
fn untrusted_key_cannot_sign_the_canon_in() {
    let f = setup();
    // B is not in the trust set yet: picking with B must fail self-verify (an untrusted key
    // cannot make rules load-bearing).
    let (code, _o, e) = run(&["canon", "pick", "--config", &f.cfg(), "--key", &f.key_b()]);
    assert_ne!(code, 0, "an untrusted key must not be able to sign the Canon: {e}");
}

#[test]
fn added_fleet_key_verifies_then_revocation_rejects() {
    let f = setup();
    // Trust machine B.
    let (code, out, e) = run(&["fleet", "add", "--config", &f.cfg(), "--pubkey", &f.pub_b(), "--label", "machine-b"]);
    assert_eq!(code, 0, "fleet add: {e}");
    assert!(out.contains("fleet-wide"), "{out}");
    let (code, out, _e) = run(&["fleet", "list", "--config", &f.cfg()]);
    assert_eq!(code, 0);
    assert_eq!(out.matches("FLEET\t").count(), 2, "trust set = machine A + machine-b:\n{out}");

    // B signs the Canon (as if picked on the other machine) — it now verifies HERE.
    let (code, _o, e) = run(&["canon", "pick", "--config", &f.cfg(), "--key", &f.key_b()]);
    assert_eq!(code, 0, "a trusted fleet key must be able to sign: {e}");
    let (code, _o, e) = run(&["canon", "list", "--config", &f.cfg()]);
    assert_eq!(code, 0, "B-signed Canon must verify while B is trusted: {e}");

    // Revoke B → the Canon B signed no longer verifies.
    let (code, _o, e) = run(&["fleet", "remove", "--config", &f.cfg(), "--pubkey", &f.pub_b()]);
    assert_eq!(code, 0, "fleet remove: {e}");
    let (code, _o, _e) = run(&["canon", "list", "--config", &f.cfg()]);
    assert_ne!(code, 0, "after revoking B, its signed Canon must be rejected");

    // Machine A re-picks → healthy again (A was never revoked).
    let (code, _o, e) = run(&["canon", "pick", "--config", &f.cfg(), "--key", &f.key_a()]);
    assert_eq!(code, 0, "A re-pick: {e}");
    let (code, _o, e) = run(&["canon", "list", "--config", &f.cfg()]);
    assert_eq!(code, 0, "A-signed Canon verifies again: {e}");
}

#[test]
fn fleet_add_is_idempotent_on_the_same_key() {
    let f = setup();
    run(&["fleet", "add", "--config", &f.cfg(), "--pubkey", &f.pub_b(), "--label", "machine-b"]);
    let (code, out, _e) = run(&["fleet", "add", "--config", &f.cfg(), "--pubkey", &f.pub_b()]);
    assert_eq!(code, 0);
    assert!(out.contains("already in the trust set"), "{out}");
    let (_c, list, _e) = run(&["fleet", "list", "--config", &f.cfg()]);
    assert_eq!(list.matches("FLEET\t").count(), 2, "no duplicate key:\n{list}");
}
