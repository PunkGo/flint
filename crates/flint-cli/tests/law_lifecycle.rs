//! WP-C acceptance: the `flint law` lifecycle (spec §2 sovereignty model) + the epoch-wart
//! regression, end-to-end on the REAL `flint` binary. Requires `ssh-keygen` (dev/CI mac+linux).
//!
//! Covers: a PROPOSED law does not bear weight until `accept` signs it (propose≠pick);
//! `accept --all`; `disable` + `remove` deactivate; and — the bug this WP fixes — auto-epoch
//! is read from the manifest HEADER, so a re-pick over a DIRTY tree bumps the epoch instead of
//! silently resetting it to 1 (a rollback).

use std::fs;
use std::process::Command;

fn flint_bin() -> &'static str {
    env!("CARGO_BIN_EXE_flint")
}

// An accepted law (no status field → accepted by default) and a proposed one.
const KEEP_RULE: &str = "---\nschema: flint/v1\nid: keep-rule\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nkeep this.\n";
const PROP_RULE: &str = "---\nschema: flint/v1\nid: prop-rule\ntype: rule\nkind: path\nglob: b/**\nresponse: block\nstatus: proposed\n---\nproposed law.\n";

struct Env {
    _dir: tempfile::TempDir,
    config: std::path::PathBuf,
    canon_root: std::path::PathBuf,
    key: std::path::PathBuf,
}

fn setup() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let canon_root = root.join("canon");
    fs::create_dir_all(canon_root.join("rules")).unwrap();
    fs::write(canon_root.join("rules/keep.md"), KEEP_RULE).unwrap();
    fs::write(canon_root.join("rules/prop.md"), PROP_RULE).unwrap();

    let key = root.join("sov_key");
    assert!(Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C", "flint-sovereign", "-f"])
        .arg(&key)
        .status()
        .expect("ssh-keygen")
        .success());
    let pubk = fs::read_to_string(root.join("sov_key.pub")).unwrap();
    let key_only: Vec<&str> = pubk.split_whitespace().take(2).collect();
    let allowed = root.join("allowed_signers");
    fs::write(&allowed, format!("flint-sovereign {}\n", key_only.join(" "))).unwrap();

    let config = root.join("flint.toml");
    fs::write(
        &config,
        format!(
            "flint_bin = {bin:?}\ncanon_root = {cr:?}\n[trust]\nallowed_signers = {al:?}\nsigner_identity = \"flint-sovereign\"\nscope = \"test-instance\"\nmin_epoch = 0\n",
            bin = flint_bin(),
            cr = canon_root,
            al = allowed,
        ),
    )
    .unwrap();

    Env { _dir: dir, config, canon_root, key }
}

fn run(env: &Env, args: &[&str]) -> (i32, String, String) {
    let mut full: Vec<&str> = args.to_vec();
    let cfg = env.config.to_str().unwrap();
    full.push("--config");
    full.push(cfg);
    let out = Command::new(flint_bin()).args(&full).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn run_key(env: &Env, args: &[&str]) -> (i32, String, String) {
    let mut full: Vec<&str> = args.to_vec();
    full.push("--key");
    full.push(env.key.to_str().unwrap());
    run(env, &full)
}

/// The `epoch N` line of the signed manifest.
fn manifest_epoch(env: &Env) -> u64 {
    let text = fs::read_to_string(env.canon_root.join("CANON.manifest")).unwrap();
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("epoch ") {
            return n.trim().parse().unwrap();
        }
    }
    panic!("no epoch line in manifest:\n{text}");
}

/// The active SIGNED policy ids (from `flint canon list`).
fn active_ids(env: &Env) -> String {
    let (code, out, e) = run(env, &["canon", "list"]);
    assert_eq!(code, 0, "canon list failed: {e}");
    out
}

#[test]
fn law_list_shows_lifecycle_status() {
    let env = setup();
    let (code, out, e) = run(&env, &["law", "list"]);
    assert_eq!(code, 0, "law list failed: {e}");
    assert!(out.contains("accepted\tkeep-rule"), "keep-rule should be accepted:\n{out}");
    assert!(out.contains("proposed\tprop-rule"), "prop-rule should be proposed:\n{out}");
}

#[test]
fn proposed_law_is_not_signed_until_accepted() {
    let env = setup();
    // A plain pick signs the accepted law but EXCLUDES the proposed one (propose≠pick).
    let (code, _o, e) = run_key(&env, &["canon", "pick"]);
    assert_eq!(code, 0, "pick failed: {e}");
    let ids = active_ids(&env);
    assert!(ids.contains("keep-rule"), "accepted law must be active:\n{ids}");
    assert!(!ids.contains("prop-rule"), "proposed law must NOT bear weight:\n{ids}");

    // Accepting it signs it in — the first real pick of that law.
    let (code, out, e) = run_key(&env, &["law", "accept", "--name", "prop-rule"]);
    assert_eq!(code, 0, "accept failed: {e}");
    assert!(out.contains("proposed -> accepted"), "accept message:\n{out}");
    let ids = active_ids(&env);
    assert!(ids.contains("prop-rule"), "accepted law must now be active:\n{ids}");
}

#[test]
fn accept_all_signs_every_proposed() {
    let env = setup();
    let (code, out, e) = run_key(&env, &["law", "accept", "--all"]);
    assert_eq!(code, 0, "accept --all failed: {e}");
    assert!(out.contains("prop-rule"), "accept --all should name prop-rule:\n{out}");
    let ids = active_ids(&env);
    assert!(ids.contains("prop-rule") && ids.contains("keep-rule"), "both active:\n{ids}");
    // Re-running has nothing left to accept.
    let (code, out, _e) = run_key(&env, &["law", "accept", "--all"]);
    assert_eq!(code, 0);
    assert!(out.contains("no proposed laws"), "second accept --all:\n{out}");
}

#[test]
fn disable_and_remove_deactivate() {
    let env = setup();
    run_key(&env, &["law", "accept", "--all"]);
    assert!(active_ids(&env).contains("keep-rule"));

    let (code, _o, e) = run_key(&env, &["law", "disable", "--name", "keep-rule"]);
    assert_eq!(code, 0, "disable failed: {e}");
    assert!(!active_ids(&env).contains("keep-rule"), "disabled law must leave the active policy");

    let (code, out, e) = run_key(&env, &["law", "remove", "--name", "prop-rule"]);
    assert_eq!(code, 0, "remove failed: {e}");
    assert!(out.contains("-> removed"), "remove message:\n{out}");
    assert!(!active_ids(&env).contains("prop-rule"), "removed law must leave the active policy");
    // The tombstone is still visible in `law list` (auditable, not a silent vanish).
    let (_c, list, _e) = run(&env, &["law", "list"]);
    assert!(list.contains("removed\tprop-rule"), "tombstone must be listed:\n{list}");
}

#[test]
fn already_in_state_is_a_loud_error() {
    let env = setup();
    // keep-rule is accepted; accepting it again errors.
    let (code, _o, e) = run_key(&env, &["law", "accept", "--name", "keep-rule"]);
    assert_ne!(code, 0, "accepting an already-accepted law should error");
    assert!(e.contains("already accepted"), "stderr:\n{e}");
}

#[test]
fn failed_transition_rolls_back_the_law_file() {
    // P2 (codex WP-C review): a law verb rewrites the source file before re-signing. If the
    // re-sign fails (here: a nonexistent key), the file must be rolled back so the failure is
    // a no-op — never a mutated-but-unsigned status a later pick would silently commit.
    let env = setup();
    run_key(&env, &["law", "accept", "--all"]);
    let keep = env.canon_root.join("rules/keep.md");
    let before = fs::read_to_string(&keep).unwrap();
    let (code, _o, e) = run(&env, &["law", "disable", "--name", "keep-rule", "--key", "/nonexistent/key"]);
    assert_ne!(code, 0, "disable with a bad key must fail");
    assert_eq!(fs::read_to_string(&keep).unwrap(), before, "law file must be rolled back on sign failure: {e}");
    assert!(active_ids(&env).contains("keep-rule"), "the signed policy is unchanged");
}

#[test]
fn tampered_manifest_epoch_is_not_trusted() {
    // P1 (codex WP-C review): CANON.manifest is agent-writable. Tampering only the epoch line
    // to u64::MAX breaks the signature; the auto-epoch source verifies the sig first, so the
    // tampered header is ignored — no overflow, no huge-epoch DoS.
    let env = setup();
    run_key(&env, &["canon", "pick"]);
    assert_eq!(manifest_epoch(&env), 1);
    let mp = env.canon_root.join("CANON.manifest");
    let tampered: String = fs::read_to_string(&mp)
        .unwrap()
        .lines()
        .map(|l| if l.starts_with("epoch ") { "epoch 18446744073709551615".to_string() } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&mp, format!("{tampered}\n")).unwrap();
    let (code, _o, e) = run_key(&env, &["canon", "pick"]);
    assert_eq!(code, 0, "pick after tamper failed: {e}");
    assert!(manifest_epoch(&env) <= 2, "epoch must not follow the tampered u64::MAX (got {})", manifest_epoch(&env));
}

#[test]
fn epoch_does_not_reset_on_a_dirty_tree() {
    // THE epoch-wart regression: after the first signed pick (epoch 1), edit a rule so the
    // working tree no longer matches the signed manifest (the NORMAL pre-re-pick state), then
    // re-pick. Auto-epoch must read the manifest HEADER and bump to 2 — NOT full-verify, fail
    // on the dirty tree, and reset to 1 (a rollback).
    let env = setup();
    run_key(&env, &["canon", "pick"]);
    assert_eq!(manifest_epoch(&env), 1, "first pick is epoch 1");

    // Dirty the tree: append to a signed rule's body (still a valid rule, but its sha now
    // differs from the epoch-1 manifest entry).
    let keep = env.canon_root.join("rules/keep.md");
    let mut body = fs::read_to_string(&keep).unwrap();
    body.push_str("\nan edit after signing.\n");
    fs::write(&keep, body).unwrap();

    let (code, _o, e) = run_key(&env, &["canon", "pick"]);
    assert_eq!(code, 0, "re-pick over a dirty tree failed: {e}");
    assert_eq!(manifest_epoch(&env), 2, "auto-epoch must bump to 2 from the header, never reset to 1");

    // And once more, still climbing.
    fs::write(&keep, KEEP_RULE).unwrap();
    run_key(&env, &["canon", "pick"]);
    assert_eq!(manifest_epoch(&env), 3, "epoch is strictly climbing across picks");
}
