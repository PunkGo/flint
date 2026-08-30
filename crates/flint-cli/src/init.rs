//! `flint init` + key custody — the one-command bootstrap (self-contained spec §2).
//!
//! `flint init` gives a fresh machine the enforcement gate: a flint home and a sovereign
//! signing key (generated here, private key NEVER leaves the machine). The canon starts
//! EMPTY on purpose: flint ships the mechanism, not rules — rules are yours, grown from
//! practice. Sample laws live in the repo under `examples/laws/`; copy the ones you want
//! into `<home>/canon/rules/` (they carry `status: proposed`) and nothing bears weight
//! until `flint law accept` signs them with your key (propose≠pick from t=0).
//!
//! Key custody (secret-zero): the private key lives in `<home>/keys/` (dir 0700, key 0600),
//! is created into a securely-moded directory (no 0755 window), is never overwritten, and its
//! permissions are re-checked before every signing use (see [`assert_key_perms`]). A world- or
//! group-readable key, or a symlink where the key should be, is a hard refusal — flint will not
//! sign with an exposed key.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The sovereign key filename inside `<home>/keys/`.
const KEY_NAME: &str = "sovereign_ed25519";
/// The identity principal recorded in `allowed_signers` (the key represents THE sovereign).
const SIGNER_IDENTITY: &str = "flint-sovereign";

pub struct InitArgs {
    /// The flint home (default `~/.flint`).
    pub home: PathBuf,
    /// The instance namespace (manifest scope — anti cross-instance replay). Default `local`.
    pub scope: String,
    /// The installed flint binary path recorded in the config (default: this executable).
    pub flint_bin: Option<String>,
    /// Also wire an opt-in memory vault (spec §4).
    pub with_memory: bool,
    /// The memory vault path (with `--with-memory`; default `<home>/memory`).
    pub vault: Option<PathBuf>,
}

pub fn run_init(args: &InitArgs) -> Result<()> {
    let home = &args.home;
    let canon_root = home.join("canon");
    let rules_dir = canon_root.join("rules");
    let keys_dir = home.join("keys");
    let pits_dir = home.join("pits");
    let config_path = home.join("flint.toml");
    let allowed = home.join("allowed_signers");

    // 1. Directories. home + keys are created with 0700 (private, no readable window); the
    //    rest inherit the umask (they hold only public rule text).
    create_dir_secure(home, 0o700)?;
    std::fs::create_dir_all(&rules_dir).with_context(|| format!("create {}", rules_dir.display()))?;
    create_dir_secure(&keys_dir, 0o700)?;
    std::fs::create_dir_all(&pits_dir).with_context(|| format!("create {}", pits_dir.display()))?;

    // 2. Sovereign key — generated once, never overwritten (idempotent re-init reuses it).
    let key_path = keys_dir.join(KEY_NAME);
    let pub_path = keys_dir.join(format!("{KEY_NAME}.pub"));
    let key_created = if key_exists(&key_path)? {
        assert_key_perms(&key_path)?; // an existing key must still be secure.
        // A key restored via `flint key import` copies only the PRIVATE file; derive the
        // public key so `allowed_signers` can be rebuilt from it (codex final-review P2).
        ensure_pubkey(&key_path, &pub_path)?;
        false
    } else {
        generate_key(&key_path, &args.scope)?;
        true
    };

    // 3. allowed_signers (the pinned public trust set — WP-F extends it to a fleet).
    write_new(&allowed, &allowed_signers_line(&pub_path)?)?;

    // 4. Config.
    let flint_bin = match &args.flint_bin {
        Some(b) => b.clone(),
        None => std::env::current_exe()
            .context("resolve current flint executable")?
            .display()
            .to_string(),
    };
    let vault = if args.with_memory {
        Some(args.vault.clone().unwrap_or_else(|| home.join("memory")))
    } else {
        None
    };
    let config_written = write_new(
        &config_path,
        &render_config(&flint_bin, &canon_root, &home.join("obs.jsonl"), &pits_dir, &home.join("knowledge"), &allowed, &args.scope, vault.as_deref(), &home.join("epoch_floor")),
    )?;
    // Re-init `--with-memory` on a home first created WITHOUT memory: the config already
    // exists (write_new skipped it), so add the `[memory]` block it lacks — otherwise we'd
    // scaffold + report a vault the config never points at (codex final-review P2).
    if !config_written {
        if let Some(v) = &vault {
            ensure_memory_in_config(&config_path, v)?;
        }
    }

    // 5. Opt-in memory vault scaffold.
    if let Some(v) = &vault {
        flint_core::memory::scaffold_vault(v).map_err(|e| anyhow::anyhow!("scaffold vault: {e}"))?;
    }

    // 6. Report + next steps.
    println!("flint init: home {}", home.display());
    println!(
        "  key      {} ({})",
        key_path.display(),
        if key_created { "generated (0600)" } else { "reused" }
    );
    println!("  config   {} ({})", config_path.display(), if config_written { "written" } else { "kept existing" });
    println!("  canon    {} (empty — rules are yours; sample laws: examples/laws/ in the repo)", rules_dir.display());
    println!("  ore      raw-ore store (粗矿) @ {}", pits_dir.display());
    println!("  精矿     knowledge store @ {} (`flint knowledge review` → promote)", home.join("knowledge").display());
    if let Some(v) = &vault {
        println!("  memory   vault @ {}", v.display());
    }
    println!("\nAdd rules (copy from examples/laws/ or write your own), review, then sign:");
    println!("  cp <repo>/examples/laws/<law>.md {}", rules_dir.display());
    println!("  flint law list   --config {}", config_path.display());
    println!("  flint law accept --all --config {} --key {}", config_path.display(), key_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Key custody
// ---------------------------------------------------------------------------

/// Re-check a sovereign key is safe to sign with — called at generation AND before EVERY use
/// (spec §2 "每次用前校验权限"). Hard refusals: a symlink where the key should be (don't sign
/// through an indirection), or a group/world-accessible key (an exposed key must not sign).
pub(crate) fn assert_key_perms(key: &Path) -> Result<()> {
    let md = std::fs::symlink_metadata(key)
        .with_context(|| format!("stat sovereign key {}", key.display()))?;
    if md.file_type().is_symlink() {
        bail!("sovereign key {} is a symlink — refusing to sign through an indirection", key.display());
    }
    if !md.is_file() {
        bail!("sovereign key {} is not a regular file — refusing to use it", key.display());
    }
    check_mode_private(key, &md)
}

/// POSIX: refuse a group/world-accessible key. Windows: a no-op — access is ACL-based, not
/// POSIX mode bits (the key still lives under the user profile; ACL tightening is staged).
#[cfg(unix)]
fn check_mode_private(key: &Path, md: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = md.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        bail!(
            "sovereign key {} is group/world-accessible (mode {mode:o}) — `chmod 600` it; \
             flint refuses to sign with an exposed key",
            key.display()
        );
    }
    Ok(())
}
#[cfg(not(unix))]
fn check_mode_private(_key: &Path, _md: &std::fs::Metadata) -> Result<()> {
    Ok(())
}

/// POSIX chmod; a Windows no-op (ACL-based access — POSIX mode bits do not apply).
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {mode:o} {}", path.display()))
}
#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

/// Create a directory with `mode` at creation time (POSIX: no readable window); on Windows the
/// mode is ignored (the directory inherits the parent ACL).
#[cfg(unix)]
fn make_dir_moded(dir: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(mode)
        .create(dir)
        .with_context(|| format!("create {} (mode {mode:o})", dir.display()))
}
#[cfg(not(unix))]
fn make_dir_moded(dir: &Path, _mode: u32) -> Result<()> {
    std::fs::create_dir(dir).with_context(|| format!("create {}", dir.display()))
}

/// Generate an ed25519 sovereign key at `key_path` (private 0600). The parent `keys/` dir is
/// already 0700-owned by us, so no other user can plant a symlink inside it (the same-UID case
/// is the honest weak-tier boundary — §3.5). Never called when the key already exists.
fn generate_key(key_path: &Path, comment: &str) -> Result<()> {
    let status = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C"])
        .arg(comment)
        .arg("-f")
        .arg(key_path)
        .status()
        .context("run ssh-keygen -t ed25519 (is OpenSSH installed?)")?;
    if !status.success() {
        bail!("ssh-keygen failed to generate the sovereign key");
    }
    // ssh-keygen writes 0600 already; enforce it explicitly (belt + suspenders), then verify.
    set_mode(key_path, 0o600)?;
    assert_key_perms(key_path)?;
    Ok(())
}

/// Export a copy of the sovereign key (+ its `.pub`) to `dest_dir` for backup — refusing to
/// export an already-exposed key, and writing the copy 0600. `flint key export`.
pub fn export_key(key: &Path, dest_dir: &Path) -> Result<()> {
    assert_key_perms(key)?;
    create_dir_secure(dest_dir, 0o700)?;
    let name = key.file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow::anyhow!("bad key filename"))?;
    copy_secure(key, &dest_dir.join(name), 0o600)?;
    let pubk = key.with_file_name(format!("{name}.pub"));
    if pubk.exists() {
        copy_secure(&pubk, &dest_dir.join(format!("{name}.pub")), 0o644)?;
    }
    println!("key export: backed up {} to {}", key.display(), dest_dir.display());
    Ok(())
}

/// Restore a sovereign key from a backup to `key_path` (0600), refusing to clobber an existing
/// key. `flint key import`.
pub fn import_key(src: &Path, key_path: &Path) -> Result<()> {
    if key_exists(key_path)? {
        bail!("a key already exists at {} — refusing to overwrite it (import to a fresh path)", key_path.display());
    }
    if let Some(dir) = key_path.parent() {
        create_dir_secure(dir, 0o700)?;
    }
    copy_secure(src, key_path, 0o600)?;
    assert_key_perms(key_path)?;
    println!("key import: restored {} from {}", key_path.display(), src.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// Ensure the public key exists next to the private key, deriving it with `ssh-keygen -y` if
/// absent (a `flint key import` restore copies only the private file). No-op if it's there.
fn ensure_pubkey(key_path: &Path, pub_path: &Path) -> Result<()> {
    if pub_path.exists() {
        return Ok(());
    }
    let out = Command::new("ssh-keygen")
        .arg("-y")
        .arg("-f")
        .arg(key_path)
        .output()
        .context("run ssh-keygen -y to derive the public key")?;
    if !out.status.success() {
        bail!("ssh-keygen -y could not derive a public key from {}", key_path.display());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.trim_end();
    if line.is_empty() {
        bail!("ssh-keygen -y produced no public key for {}", key_path.display());
    }
    write_new(pub_path, &format!("{line}\n"))?;
    Ok(())
}

/// Add a `[memory] vault = …` block to an existing config that lacks one. No-op if present.
fn ensure_memory_in_config(config_path: &Path, vault: &Path) -> Result<()> {
    let existing = std::fs::read_to_string(config_path).with_context(|| format!("read {}", config_path.display()))?;
    if existing.contains("[memory]") {
        return Ok(());
    }
    let mut out = existing;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("\n[memory]\nvault = {vault:?}\n"));
    std::fs::write(config_path, out).with_context(|| format!("update {}", config_path.display()))?;
    Ok(())
}

/// Does the key exist? (Follows the path; a symlink pointing at a real key counts — the caller
/// then refuses to sign through it via [`assert_key_perms`].)
fn key_exists(key: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(key) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow::Error::new(e).context(format!("stat {}", key.display()))),
    }
}

/// Create `dir` with exactly `mode` and no readable window: a fresh dir is made via a
/// mode-set DirBuilder (created 0700, never briefly 0755); an existing dir is verified to be a
/// real directory (not a symlink) and tightened to `mode`.
fn create_dir_secure(dir: &Path, mode: u32) -> Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(md) => {
            if md.file_type().is_symlink() || !md.is_dir() {
                bail!("{} exists and is not a real directory (symlink?) — refusing to use it", dir.display());
            }
            set_mode(dir, mode)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = dir.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
            }
            make_dir_moded(dir, mode)?;
        }
        Err(e) => return Err(anyhow::Error::new(e).context(format!("stat {}", dir.display()))),
    }
    Ok(())
}

/// Copy `src` to `dest` (create_new — refusing to clobber, symlink-safe), then apply `mode`.
fn copy_secure(src: &Path, dest: &Path, mode: u32) -> Result<()> {
    use std::io::Write;
    let bytes = std::fs::read(src).with_context(|| format!("read {}", src.display()))?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .with_context(|| format!("create {}", dest.display()))?;
    f.write_all(&bytes).with_context(|| format!("write {}", dest.display()))?;
    drop(f);
    set_mode(dest, mode)?;
    Ok(())
}

/// Write `content` to `path` only if absent (create_new = symlink-safe). Returns whether it
/// wrote (false = already existed, left untouched — the idempotent-re-init contract).
fn write_new(path: &Path, content: &str) -> Result<bool> {
    use std::io::Write;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => {
            f.write_all(content.as_bytes()).with_context(|| format!("write {}", path.display()))?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(anyhow::Error::new(e).context(format!("create {}", path.display()))),
    }
}

/// The `allowed_signers` line for a generated public key: `<identity> <keytype> <key>`.
fn allowed_signers_line(pub_path: &Path) -> Result<String> {
    let pubk = std::fs::read_to_string(pub_path).with_context(|| format!("read {}", pub_path.display()))?;
    let key_only: Vec<&str> = pubk.split_whitespace().take(2).collect(); // drop the trailing comment
    if key_only.len() != 2 {
        bail!("malformed public key in {}", pub_path.display());
    }
    Ok(format!("{SIGNER_IDENTITY} {}\n", key_only.join(" ")))
}

/// Render `flint.toml`. Paths use TOML string escaping via `{:?}` (the established config
/// format — see the integration fixtures).
#[allow(clippy::too_many_arguments)]
fn render_config(
    flint_bin: &str,
    canon_root: &Path,
    obs_log: &Path,
    pits_root: &Path,
    knowledge_root: &Path,
    allowed: &Path,
    scope: &str,
    vault: Option<&Path>,
    epoch_floor: &Path,
) -> String {
    // pits_root is the default (active) raw-ore store (粗矿); knowledge_root is the 精矿 store
    // that `flint knowledge promote` writes into. Both bare markdown — take-away, no lock-in.
    let mut out = format!(
        "flint_bin = {flint_bin:?}\ncanon_root = {canon:?}\nobs_log = {obs:?}\npits_root = {pits:?}\nknowledge_root = {kn:?}\n",
        canon = canon_root,
        obs = obs_log,
        pits = pits_root,
        kn = knowledge_root,
    );
    if let Some(v) = vault {
        out.push_str(&format!("\n[memory]\nvault = {v:?}\n"));
    }
    // flint's opt-out capture default (L1, §3): written EXPLICITLY so you see what flint ships
    // into agent context and can flip it to false. On → the "mark your walls" nudge compiles in.
    out.push_str("\n[capture]\nauto_mine = true\n");
    // epoch_floor on by default: the persistent anti-rollback floor advances on every pick, so
    // a checkout of an old signed manifest is rejected without a manual min_epoch bump (weak
    // tier — a same-UID agent can edit the floor file too; see config docs).
    out.push_str(&format!(
        "\n[trust]\nallowed_signers = {allowed:?}\nsigner_identity = {id:?}\nscope = {scope:?}\nmin_epoch = 0\nepoch_floor = {floor:?}\n",
        id = SIGNER_IDENTITY,
        floor = epoch_floor,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo's sample laws (`examples/laws/`) — not shipped in the binary, but the
    /// README points new users at them, so they must stay lint-clean and unsigned.
    fn example_laws() -> Vec<(String, String)> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/laws");
        let mut laws: Vec<(String, String)> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .filter_map(|entry| {
                let p = entry.unwrap().path();
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                (p.extension().is_some_and(|x| x == "md") && name != "README.md")
                    .then(|| (name.clone(), std::fs::read_to_string(&p).unwrap()))
            })
            .collect();
        laws.sort();
        laws
    }

    #[test]
    fn example_laws_all_parse_and_are_proposed() {
        // Every sample law must lint clean and carry `status: proposed` — copying one into a
        // canon must never smuggle in something that claims to already bear weight.
        let laws = example_laws();
        assert!(!laws.is_empty(), "examples/laws/ must not be empty");
        for (name, content) in &laws {
            let parsed = flint_core::canon::parse_rule(&format!("rules/{name}"), content);
            assert!(parsed.is_ok(), "example law {name} must parse: {parsed:?}");
            let status = flint_core::canon::rule_status(name, content).unwrap();
            assert_eq!(status, flint_core::canon::Status::Proposed, "{name} must be proposed");
        }
    }

    #[test]
    fn example_laws_carry_no_private_paths() {
        // Neutralized for OSS: no private pointers / absolute home paths in the samples.
        for (name, content) in &example_laws() {
            let lower = content.to_lowercase();
            assert!(!lower.contains("etch:"), "{name} leaks a private pointer");
            assert!(!lower.contains("/users/"), "{name} leaks an absolute home path");
        }
    }
}
