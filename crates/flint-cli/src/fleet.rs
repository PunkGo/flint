//! `flint fleet` — the cross-machine trust set (self-contained spec §8, fleet keyring).
//!
//! Every machine keeps its OWN private key (never leaves the machine — secret-zero). Each
//! publishes its PUBLIC key; the fleet trust set is all your machines' public keys, listed in
//! the `allowed_signers` file under ONE shared principal (`trust.signer_identity`). Because
//! `ssh-keygen -Y verify -I <principal>` accepts a signature from ANY key listed under that
//! principal, a Canon signed on machine A verifies on machine B with ZERO re-signing — sign
//! once, the whole fleet enforces it (§8). This layer only MANAGES the trust set; the
//! verification itself is the unchanged [`flint_core::trust`] path.
//!
//! Sovereignty holds: only YOUR machines' public keys are trusted (a stranger's or an unsigned
//! Canon is rejected by the existing signature gate); and an agent with no fleet private key
//! cannot sign anything into the Canon. Revocation = `fleet remove` a machine's public key.
//!
//! Wiring a real second machine (e.g. your laptop): on that machine `flint init` prints its public
//! key path (`<home>/keys/sovereign_ed25519.pub`); copy it here and `flint fleet add --pubkey
//! <that.pub> --label laptop`. (The integration tests stand in a locally-generated key for the
//! second machine — the mechanism is real; only the specific remote key is yours to add.)

use std::path::Path;

use anyhow::{Context, Result, bail};

use flint_core::config::FlintConfig;

/// A parsed OpenSSH public key line: (keytype, base64 key, comment).
struct PubKey {
    keytype: String,
    key: String,
    comment: String,
}

fn parse_pubkey(path: &Path) -> Result<PubKey> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read pubkey {}", path.display()))?;
    let line = text.lines().next().unwrap_or("").trim();
    let mut it = line.split_whitespace();
    let keytype = it.next().unwrap_or("").to_string();
    let key = it.next().unwrap_or("").to_string();
    let comment = it.collect::<Vec<_>>().join(" ");
    if !(keytype.starts_with("ssh-") || keytype.starts_with("ecdsa-") || keytype.starts_with("sk-")) {
        bail!("{} does not look like an OpenSSH public key (got keytype `{keytype}`)", path.display());
    }
    if key.is_empty() {
        bail!("{} has no key material", path.display());
    }
    Ok(PubKey { keytype, key, comment })
}

/// The comment/label of an `allowed_signers` line = everything after `principal keytype key`.
fn line_label(line: &str) -> String {
    line.split_whitespace().skip(3).collect::<Vec<_>>().join(" ")
}

pub fn add(cfg: &FlintConfig, pubkey: &Path, label: Option<&str>) -> Result<()> {
    let allowed = &cfg.trust.allowed_signers;
    let pk = parse_pubkey(pubkey)?;
    let existing = std::fs::read_to_string(allowed).unwrap_or_default();
    // Idempotent: the KEY (not the label) is the identity — never add the same key twice.
    if existing.split_whitespace().any(|tok| tok == pk.key) {
        println!("fleet: that public key is already in the trust set — nothing to do");
        return Ok(());
    }
    let label = label
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(pk.comment);
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let line = format!("{} {} {} {}", cfg.trust.signer_identity, pk.keytype, pk.key, label);
    out.push_str(line.trim_end());
    out.push('\n');
    atomic_write(allowed, out.as_bytes())?;
    println!("fleet: added {} key ({label}) — a Canon it signs now verifies fleet-wide", pk.keytype);
    Ok(())
}

pub fn list(cfg: &FlintConfig) -> Result<()> {
    let allowed = std::fs::read_to_string(&cfg.trust.allowed_signers)
        .with_context(|| format!("read {}", cfg.trust.allowed_signers.display()))?;
    let mut n = 0usize;
    for line in allowed.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let mut it = l.split_whitespace();
        let principal = it.next().unwrap_or("?");
        let keytype = it.next().unwrap_or("?");
        let key = it.next().unwrap_or("");
        let short = if key.len() > 20 { format!("{}…", &key[..20]) } else { key.to_string() };
        let label = line_label(l);
        let label = if label.is_empty() { "(this machine)".to_string() } else { label };
        println!("FLEET\t{principal}\t{keytype}\t{short}\t{label}");
        n += 1;
    }
    println!("{n} key(s) in the fleet trust set");
    Ok(())
}

pub fn remove(cfg: &FlintConfig, label: Option<&str>, pubkey: Option<&Path>) -> Result<()> {
    if label.is_none() && pubkey.is_none() {
        bail!("specify --label <name> or --pubkey <file> to identify which key to remove");
    }
    let allowed = &cfg.trust.allowed_signers;
    let key_to_remove = match pubkey {
        Some(p) => Some(parse_pubkey(p)?.key),
        None => None,
    };
    let existing = std::fs::read_to_string(allowed).with_context(|| format!("read {}", allowed.display()))?;
    let mut kept: Vec<&str> = Vec::new();
    let mut removed = 0usize;
    for line in existing.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            kept.push(line);
            continue;
        }
        let matches_key = key_to_remove.as_deref().is_some_and(|k| l.split_whitespace().any(|t| t == k));
        let matches_label = label.is_some_and(|lab| line_label(l) == lab);
        if matches_key || matches_label {
            removed += 1;
        } else {
            kept.push(line);
        }
    }
    if removed == 0 {
        bail!("no trust-set entry matched — nothing removed");
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    atomic_write(allowed, out.as_bytes())?;
    println!("fleet: removed {removed} key(s) — their signatures no longer verify here (revoked)");
    Ok(())
}

/// Write the trust file via a same-dir temp + atomic rename (a torn allowed_signers would
/// fail-closed every verification until repaired — never leave it half-written).
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| anyhow::anyhow!("no parent dir for {}", path.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).with_context(|| format!("temp in {}", dir.display()))?;
    tmp.write_all(bytes).context("write temp trust file")?;
    tmp.flush().context("flush temp trust file")?;
    tmp.persist(path).map_err(|e| anyhow::anyhow!("atomic rename trust file: {e}"))?;
    Ok(())
}
