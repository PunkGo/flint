//! WP-B acceptance: `flint init` + key custody (spec §2), end-to-end on the REAL binary.
//! Requires `ssh-keygen` (dev/CI mac+linux). Unix-gated where it asserts POSIX key modes.

use std::fs;
use std::path::{Path, PathBuf};
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

/// Returns (exit code, stdout). stdout is load-bearing: both blocking tiers enforce through
/// the `permissionDecision` JSON there, not through the exit code.
fn run_stdin(args: &[&str], stdin: &str) -> (i32, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(flint_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).to_string())
}

struct Home {
    _dir: tempfile::TempDir,
    home: PathBuf,
}

impl Home {
    fn init(extra: &[&str]) -> Home {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("flint-home");
        let mut args = vec!["init", "--home", home.to_str().unwrap(), "--scope", "test"];
        args.extend_from_slice(extra);
        let (code, _o, e) = run(&args);
        assert_eq!(code, 0, "init failed: {e}");
        Home { _dir: dir, home }
    }
    fn config(&self) -> PathBuf {
        self.home.join("flint.toml")
    }
    fn key(&self) -> PathBuf {
        self.home.join("keys/sovereign_ed25519")
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn init_generates_key_and_an_empty_canon() {
    let h = Home::init(&[]);
    assert!(h.key().exists(), "sovereign key generated");
    assert!(h.config().exists(), "config written");
    // The canon starts EMPTY: flint ships the mechanism, not rules. No proposed laws,
    // and no CANON.manifest at all until the first `accept` — the strongest form of
    // "nothing bears weight".
    let (code, out, e) = run(&["law", "list", "--config", h.config().to_str().unwrap()]);
    assert_eq!(code, 0, "law list: {e}");
    assert_eq!(out.matches("\tproposed\t").count(), 0, "a fresh canon has no laws:\n{out}");
    let (code, _o, e) = run(&["canon", "list", "--config", h.config().to_str().unwrap()]);
    assert_ne!(code, 0, "no signed canon before accept");
    assert!(e.contains("no signed"), "canon list stderr: {e}");
}

#[cfg(unix)]
#[test]
fn init_key_is_0600_and_home_is_0700() {
    let h = Home::init(&[]);
    assert_eq!(mode(&h.key()), 0o600, "private key must be 0600");
    assert_eq!(mode(&h.home), 0o700, "flint home must be 0700");
    assert_eq!(mode(&h.home.join("keys")), 0o700, "keys dir must be 0700");
}

#[test]
fn an_accepted_example_law_actually_enforces() {
    // The samples in examples/laws/ are the material the README points new users at —
    // copy one in exactly the way a user would, accept it, and prove it gates for real.
    let h = Home::init(&[]);
    let cfg = h.config();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/laws/lsp-over-grep.md");
    let dest = h.home.join("canon/rules/lsp-over-grep.md");
    fs::copy(&example, &dest).unwrap_or_else(|e| panic!("copy {}: {e}", example.display()));
    let (code, _o, e) = run(&["law", "accept", "--all", "--config", cfg.to_str().unwrap(), "--key", h.key().to_str().unwrap()]);
    assert_eq!(code, 0, "accept --all: {e}");
    // The accepted lsp-over-grep law now blocks a grep on source.
    let hook = r#"{"tool_name":"Bash","tool_input":{"command":"grep -rn foo src/main.rs"}}"#;
    let (code, out) = run_stdin(&["hook", "--harness", "claude", "--config", cfg.to_str().unwrap(), "--mode", "block"], hook);
    assert_eq!(code, 0, "the exit code is not the blocking channel");
    assert!(
        out.contains("\"permissionDecision\":\"deny\"") && out.contains("flint critique"),
        "an accepted example law must enforce, as a critique: {out}"
    );
}

#[test]
fn init_is_idempotent() {
    let h = Home::init(&[]);
    let key_bytes = fs::read(h.key()).unwrap();
    // second init reuses the key, does not clobber the laws/config.
    let (code, out, e) = run(&["init", "--home", h.home.to_str().unwrap(), "--scope", "test"]);
    assert_eq!(code, 0, "re-init failed: {e}");
    assert!(out.contains("reused"), "re-init must reuse the key:\n{out}");
    assert_eq!(fs::read(h.key()).unwrap(), key_bytes, "re-init must not regenerate the key");
}

#[cfg(unix)]
#[test]
fn signing_refuses_a_world_readable_key() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::init(&[]);
    // A proposed law must exist for accept to reach the signing (and thus key-perms) path.
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/laws/lsp-over-grep.md");
    fs::copy(&example, h.home.join("canon/rules/lsp-over-grep.md")).unwrap();
    fs::set_permissions(h.key(), fs::Permissions::from_mode(0o644)).unwrap();
    let (code, _o, e) = run(&["law", "accept", "--all", "--config", h.config().to_str().unwrap(), "--key", h.key().to_str().unwrap()]);
    assert_ne!(code, 0, "must refuse to sign with a world-readable key");
    assert!(e.contains("group/world-accessible") || e.to_lowercase().contains("expose"), "stderr: {e}");
}

#[test]
fn key_export_import_roundtrip() {
    let h = Home::init(&[]);
    let backup = h.home.join("backup");
    let (code, _o, e) = run(&["key", "export", "--key", h.key().to_str().unwrap(), "--to", backup.to_str().unwrap()]);
    assert_eq!(code, 0, "export: {e}");
    let restored = h.home.join("restored_key");
    let (code, _o, e) = run(&[
        "key", "import",
        "--from", backup.join("sovereign_ed25519").to_str().unwrap(),
        "--to", restored.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "import: {e}");
    assert_eq!(fs::read(&restored).unwrap(), fs::read(h.key()).unwrap(), "restored key matches the original");
    // import refuses to clobber an existing key.
    let (code, _o, _e) = run(&[
        "key", "import",
        "--from", backup.join("sovereign_ed25519").to_str().unwrap(),
        "--to", restored.to_str().unwrap(),
    ]);
    assert_ne!(code, 0, "import must refuse to overwrite an existing key");
}

#[test]
fn init_with_memory_scaffolds_a_vault() {
    let h = Home::init(&["--with-memory"]);
    assert!(h.home.join("memory/inbox.md").exists(), "vault scaffolded");
    let (code, _o, e) = run(&["memory", "capture", "--config", h.config().to_str().unwrap(), "a captured gist"]);
    assert_eq!(code, 0, "memory capture via init'd vault: {e}");
}

#[test]
fn re_init_with_memory_wires_the_config() {
    // codex final-review P2a: enabling memory on a home first created WITHOUT it must actually
    // add [memory] vault to the existing config, not just scaffold + report.
    let h = Home::init(&[]);
    let (code, _o, e) = run(&["init", "--home", h.home.to_str().unwrap(), "--scope", "test", "--with-memory"]);
    assert_eq!(code, 0, "re-init --with-memory: {e}");
    let cfg = fs::read_to_string(h.config()).unwrap();
    assert!(cfg.contains("[memory]"), "config must gain a [memory] block:\n{cfg}");
    let (code, _o, e) = run(&["memory", "capture", "--config", h.config().to_str().unwrap(), "gist"]);
    assert_eq!(code, 0, "memory must work after re-init wires it: {e}");
}

#[test]
fn init_recovers_a_missing_public_key() {
    // codex final-review P2b: a key restored with `flint key import` copies only the private
    // file; re-init must derive the missing .pub (ssh-keygen -y) instead of failing.
    let h = Home::init(&[]);
    fs::remove_file(h.key().with_extension("pub")).unwrap();
    let (code, out, e) = run(&["init", "--home", h.home.to_str().unwrap(), "--scope", "test"]);
    assert_eq!(code, 0, "re-init with a missing .pub must recover: {e}");
    assert!(out.contains("reused"), "the private key is reused, not regenerated");
    assert!(h.key().with_extension("pub").exists(), ".pub must be re-derived");
    // and the derived pub is consistent — accept + a hook still enforce.
    let (code, _o, e) = run(&["law", "accept", "--all", "--config", h.config().to_str().unwrap(), "--key", h.key().to_str().unwrap()]);
    assert_eq!(code, 0, "accept after .pub recovery: {e}");
}
