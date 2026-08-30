//! Canon trust — the ROOT OF TRUST after the kernel was cut (reframe-and-diff §3.5.1).
//!
//! Load-bearing Canon = a SIGNED manifest. The sovereign signs (SSH-sig ed25519) a
//! `CANON.manifest` that lists every load-bearing file by sha256; Striker compiles ONLY
//! from a manifest whose signature verifies against a PINNED sovereign public key the
//! agent cannot use. The agent may freely edit the working tree — unsigned/unlisted edits
//! never bear weight (so "delete a deny rule" never authorizes by absence; invariant ④).
//!
//! This is the IO/crypto half (§3.5.1 IO-vs-pure split): it VERIFIES + returns bytes.
//! The pure half (`canon::reduce`, bytes -> policy) consumes the returned
//! [`LoadBearingCanon`]. Verification reruns on every compile (cheap) — never cached.
//!
//! FAIL-CLOSED everywhere (codex sig-iface review, 7 hard requirements folded in):
//!   1. rollback is an INHERENT residual of offline signatures — `min_epoch` (protected
//!      state, §3.5 strong tier) is the only defense; weak tier = §12.6 discipline.
//!   3. whole-manifest fail-closed: ANY missing/mismatched listed file -> `Err` for the
//!      WHOLE Canon (never "skip the missing one and load the rest").
//!   4. scope pinned (cross-instance replay) + the Canon ROOT is caller-controlled (the
//!      entrypoint boundary — `SovereignTrust`/root are NOT read from agent-writable Canon).
//!   5. verify over the RAW manifest bytes (never reparse-then-reserialize) + the SSH-sig
//!      namespace is pinned to [`SIG_NAMESPACE`] (a signature for another purpose can't
//!      be replayed as a manifest sig).
//!   6. path/TOCTOU: listed paths must be canonical relative paths under the root (no
//!      absolute, no `..`, no symlink escape); verified bytes are returned and parsed
//!      from memory (no reopen-after-hash race).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// The SSH-sig namespace the manifest signature MUST carry (codex req #5: context
/// separation — a sovereign signature made for any other namespace cannot verify here).
pub const SIG_NAMESPACE: &str = "flint-canon-manifest-v1";

/// The manifest's first-line magic.
const MANIFEST_SCHEMA: &str = "flint-canon-manifest-v1";

const MANIFEST_FILE: &str = "CANON.manifest";
const MANIFEST_SIG_FILE: &str = "CANON.manifest.sig";

/// The pinned root of trust. NOT read from agent-writable Canon — installed with the
/// harness (the entrypoint boundary). `min_epoch` is the anti-rollback floor; advancing
/// it across runs (strong tier) needs agent-unwritable durable state (the caller's job).
#[derive(Debug, Clone)]
pub struct SovereignTrust {
    /// Path to an OpenSSH `allowed_signers` file pinning the sovereign key(s).
    pub allowed_signers: PathBuf,
    /// The signer principal/identity (must be a principal in `allowed_signers`).
    pub signer_identity: String,
    /// This instance's namespace (anti cross-instance replay; must equal `manifest.scope`).
    pub expected_scope: String,
    /// Highest manifest epoch accepted so far (anti-rollback; `manifest.epoch >= min_epoch`).
    pub min_epoch: u64,
}

/// The verified load-bearing snapshot: the listed files (path -> verified bytes) + the
/// accepted epoch. Feed `files` to [`crate::canon::reduce`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadBearingCanon {
    pub epoch: u64,
    pub files: BTreeMap<PathBuf, Vec<u8>>,
}

/// Why a Canon snapshot is not load-bearing. Every variant is fail-closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustError {
    NoManifest,
    SignatureInvalid,
    /// The verify tool itself could not run (missing `ssh-keygen`, IO). Fail-closed.
    VerifyUnavailable(String),
    BadManifest(String),
    ScopeMismatch { expected: String, found: String },
    RollbackDetected { min_epoch: u64, found: u64 },
    UnsafePath(String),
    FileMissing(String),
    ContentHashMismatch(String),
}

impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustError::NoManifest => write!(f, "no signed CANON.manifest / .sig"),
            TrustError::SignatureInvalid => write!(f, "manifest signature did not verify against the pinned sovereign key"),
            TrustError::VerifyUnavailable(e) => write!(f, "signature verification unavailable (fail-closed): {e}"),
            TrustError::BadManifest(e) => write!(f, "malformed manifest: {e}"),
            TrustError::ScopeMismatch { expected, found } => write!(f, "manifest scope `{found}` != expected `{expected}`"),
            TrustError::RollbackDetected { min_epoch, found } => {
                write!(f, "manifest epoch {found} < min_epoch {min_epoch} (rollback)")
            }
            TrustError::UnsafePath(p) => write!(f, "manifest lists an unsafe path: {p}"),
            TrustError::FileMissing(p) => write!(f, "manifest-listed file missing: {p}"),
            TrustError::ContentHashMismatch(p) => write!(f, "manifest-listed file hash mismatch: {p}"),
        }
    }
}

impl std::error::Error for TrustError {}

/// Resolve the load-bearing Canon, verifying it traces to a sovereign signature.
pub trait CanonTrust {
    fn resolve_load_bearing(&self, trust: &SovereignTrust) -> Result<LoadBearingCanon, TrustError>;
}

/// The portable, git-agnostic root of trust (owner decision D2): a signed `CANON.manifest` over a
/// Canon directory. `root` is caller-controlled (the entrypoint boundary, codex req #4).
pub struct ManifestSignedCanon {
    pub root: PathBuf,
}

impl ManifestSignedCanon {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl CanonTrust for ManifestSignedCanon {
    fn resolve_load_bearing(&self, trust: &SovereignTrust) -> Result<LoadBearingCanon, TrustError> {
        let manifest_path = self.root.join(MANIFEST_FILE);
        let sig_path = self.root.join(MANIFEST_SIG_FILE);
        // Read the RAW manifest bytes — we verify + parse THESE exact bytes (codex req #5).
        let manifest_bytes = std::fs::read(&manifest_path).map_err(|_| TrustError::NoManifest)?;
        if !sig_path.exists() {
            return Err(TrustError::NoManifest);
        }

        // (2/5) Verify the SSH-sig over the raw bytes, namespace-pinned, against the
        // pinned allowed_signers + identity. ANY non-success is fail-closed.
        verify_manifest_sig(trust, &manifest_bytes, &sig_path)?;

        // Parse the (now-trusted) manifest bytes.
        let manifest = Manifest::parse(&manifest_bytes)?;

        // (4) scope pin + (1) rollback floor.
        if manifest.scope != trust.expected_scope {
            return Err(TrustError::ScopeMismatch {
                expected: trust.expected_scope.clone(),
                found: manifest.scope,
            });
        }
        if manifest.epoch < trust.min_epoch {
            return Err(TrustError::RollbackDetected { min_epoch: trust.min_epoch, found: manifest.epoch });
        }

        // (3/6) Each listed file: safe path, read, hash, compare. ANY failure -> whole Canon Err.
        let root_canon = std::fs::canonicalize(&self.root)
            .map_err(|e| TrustError::VerifyUnavailable(format!("canon root: {e}")))?;
        let mut files: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
        for entry in &manifest.files {
            let rel = safe_relative(&entry.path).ok_or_else(|| TrustError::UnsafePath(entry.path.clone()))?;
            let full = root_canon.join(&rel);
            // Canonicalize and confirm it stays under the root (symlink-escape guard).
            let full_canon = std::fs::canonicalize(&full).map_err(|_| TrustError::FileMissing(entry.path.clone()))?;
            if !full_canon.starts_with(&root_canon) {
                return Err(TrustError::UnsafePath(entry.path.clone()));
            }
            let bytes = std::fs::read(&full_canon).map_err(|_| TrustError::FileMissing(entry.path.clone()))?;
            if sha256_hex(&bytes) != entry.sha256 {
                return Err(TrustError::ContentHashMismatch(entry.path.clone()));
            }
            files.insert(rel, bytes);
        }

        Ok(LoadBearingCanon { epoch: manifest.epoch, files })
    }
}

/// Verify `data` (the EXACT manifest bytes) against `sig_path` using `ssh-keygen -Y
/// verify`, namespace-pinned to [`SIG_NAMESPACE`], against `trust`'s allowed_signers +
/// identity. Public so `flint canon pick` can SELF-VERIFY that the sovereign key signed
/// its EXACT in-memory bytes (closing the sign-the-path TOCTOU, codex P1). Fail-closed:
/// any non-success (bad sig / unknown signer / namespace mismatch / tool error) is `Err`.
pub fn verify_manifest_sig(trust: &SovereignTrust, data: &[u8], sig_path: &Path) -> Result<(), TrustError> {
    verify_detached_sig(&trust.allowed_signers, &trust.signer_identity, SIG_NAMESPACE, data, sig_path)
}

/// Verify a detached SSH signature over `data` under an EXPLICIT `namespace`, against
/// `allowed_signers` + `identity`. This is the namespace-pinned core that
/// [`verify_manifest_sig`] wraps with [`SIG_NAMESPACE`] (codex req #5: context separation —
/// a signature made for any other namespace cannot verify here). Fail-closed: any
/// non-success (bad sig / unknown signer / namespace mismatch / tool error) is `Err`.
pub fn verify_detached_sig(
    allowed_signers: &Path,
    identity: &str,
    namespace: &str,
    data: &[u8],
    sig_path: &Path,
) -> Result<(), TrustError> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("ssh-keygen")
        .arg("-Y")
        .arg("verify")
        .arg("-f")
        .arg(allowed_signers)
        .arg("-I")
        .arg(identity)
        .arg("-n")
        .arg(namespace)
        .arg("-s")
        .arg(sig_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| TrustError::VerifyUnavailable(format!("spawn ssh-keygen: {e}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| TrustError::VerifyUnavailable("no stdin".into()))?
        .write_all(data)
        .map_err(|e| TrustError::VerifyUnavailable(format!("write stdin: {e}")))?;
    let status = child
        .wait()
        .map_err(|e| TrustError::VerifyUnavailable(format!("wait: {e}")))?;
    // ssh-keygen -Y verify exits 0 only on a good signature by an allowed principal.
    // A bad sig, an unknown signer, or a namespace mismatch all exit non-zero -> fail-closed.
    if status.success() {
        Ok(())
    } else {
        Err(TrustError::SignatureInvalid)
    }
}

/// A parsed manifest. Strict line format (hand-parsed, fail-closed):
/// ```text
/// flint-canon-manifest-v1
/// scope <scope>
/// epoch <u64>
/// file <sha256hex> <relative/path>
/// ...
/// ```
struct Manifest {
    scope: String,
    epoch: u64,
    files: Vec<ManifestEntry>,
}

struct ManifestEntry {
    sha256: String,
    path: String,
}

impl Manifest {
    fn parse(bytes: &[u8]) -> Result<Self, TrustError> {
        let text = std::str::from_utf8(bytes).map_err(|_| TrustError::BadManifest("non-utf8".into()))?;
        let mut lines = text.lines();
        match lines.next() {
            Some(l) if l == MANIFEST_SCHEMA => {}
            other => return Err(TrustError::BadManifest(format!("bad schema line: {other:?}"))),
        }
        let mut scope: Option<String> = None;
        let mut epoch: Option<u64> = None;
        let mut files = Vec::new();
        for line in lines {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.splitn(2, ' ');
            match it.next() {
                Some("scope") => {
                    let v = it.next().ok_or_else(|| TrustError::BadManifest("scope: missing value".into()))?;
                    if scope.replace(v.trim().to_string()).is_some() {
                        return Err(TrustError::BadManifest("duplicate scope".into()));
                    }
                }
                Some("epoch") => {
                    let v = it.next().ok_or_else(|| TrustError::BadManifest("epoch: missing value".into()))?;
                    let n = v.trim().parse::<u64>().map_err(|_| TrustError::BadManifest("epoch: not a u64".into()))?;
                    if epoch.replace(n).is_some() {
                        return Err(TrustError::BadManifest("duplicate epoch".into()));
                    }
                }
                Some("file") => {
                    // `file <sha256hex> <path>` — path may contain spaces (take the rest).
                    let rest = it.next().ok_or_else(|| TrustError::BadManifest("file: missing args".into()))?;
                    let mut parts = rest.splitn(2, ' ');
                    let sha = parts.next().unwrap_or("").trim();
                    let path = parts.next().ok_or_else(|| TrustError::BadManifest("file: missing path".into()))?;
                    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(TrustError::BadManifest(format!("file: bad sha256 `{sha}`")));
                    }
                    files.push(ManifestEntry { sha256: sha.to_ascii_lowercase(), path: path.to_string() });
                }
                other => return Err(TrustError::BadManifest(format!("unknown directive: {other:?}"))),
            }
        }
        Ok(Manifest {
            scope: scope.ok_or_else(|| TrustError::BadManifest("missing scope".into()))?,
            epoch: epoch.ok_or_else(|| TrustError::BadManifest("missing epoch".into()))?,
            files,
        })
    }
}

/// A manifest path is admissible iff it is a relative path with only normal components
/// (no `.`, no `..`, no root/prefix). Returns the normalized relative path, or `None`.
fn safe_relative(p: &str) -> Option<PathBuf> {
    let path = Path::new(p);
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            // `.`, `..`, RootDir, Prefix are all rejected (no traversal / absolute).
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        return None;
    }
    Some(out)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Build the canonical `CANON.manifest` bytes for a set of (relative-path, content) files
/// at `scope`/`epoch`. The exact bytes returned are what the sovereign signs (`ssh-keygen
/// -Y sign -n flint-canon-manifest-v1`) and what [`Manifest::parse`] consumes — sign the
/// EXACT bytes, never a reparse (codex sig-iface req #5). Files are emitted in sorted path
/// order so the manifest is a deterministic function of the file set.
pub fn build_manifest(scope: &str, epoch: u64, files: &[(String, Vec<u8>)]) -> String {
    let mut entries: Vec<(String, String)> =
        files.iter().map(|(p, b)| (p.clone(), sha256_hex(b))).collect();
    entries.sort();
    let mut out = format!("{MANIFEST_SCHEMA}\nscope {scope}\nepoch {epoch}\n");
    for (path, sha) in entries {
        out.push_str(&format!("file {sha} {path}\n"));
    }
    out
}

/// The scope + epoch of the current on-disk manifest header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestHeader {
    pub scope: String,
    pub epoch: u64,
}

/// Read ONLY the `scope` + `epoch` from the on-disk `CANON.manifest`, WITHOUT verifying the
/// signature and WITHOUT hashing any working-tree file. `None` if there is no parseable
/// manifest yet.
///
/// This exists to fix the epoch-wart rollback bug: `canon pick` computes the next epoch as
/// (current + 1), but it used to read the current epoch through FULL verification
/// ([`ManifestSignedCanon::resolve_load_bearing`]), which hashes every manifest-listed file.
/// The normal pre-pick state is a DIRTY working tree (you edited a rule, now you re-sign) —
/// so that hash check failed, epoch detection returned `None`, and the next epoch silently
/// reset to 1: a rollback below the already-signed epoch, which the hook then rejects (or,
/// worse, which resets the anti-rollback baseline). Reading just the header is the fix
/// (self-contained spec §6 / the epoch-wart pit): the epoch of the LAST signed manifest is a
/// property of the manifest header, independent of whether the tree has since been edited.
pub fn read_manifest_header(canon_root: &Path) -> Option<ManifestHeader> {
    let bytes = std::fs::read(canon_root.join(MANIFEST_FILE)).ok()?;
    parse_manifest_header(&bytes)
}

/// Parse the scope + epoch header out of raw manifest bytes (no IO). Used to read the epoch of
/// bytes you have ALREADY verified — so the epoch and the signature you checked describe the
/// exact same bytes (no reread TOCTOU).
pub fn parse_manifest_header(bytes: &[u8]) -> Option<ManifestHeader> {
    let m = Manifest::parse(bytes).ok()?;
    Some(ManifestHeader { scope: m.scope, epoch: m.epoch })
}

/// Read the PERSISTENT anti-rollback floor (a single u64 epoch) at `path`, or 0 if absent /
/// empty / unparseable. Fail-SOFT to 0 is correct here and NOT a downgrade: the floor only
/// ever RAISES the effective min_epoch (config `min_epoch` is the always-present baseline), so
/// a missing or corrupt floor can never LOWER protection below that baseline — it just fails
/// to add its increment. (A fail-CLOSED floor would let a corrupt file DoS the whole gate,
/// which a hardening increment must never do.)
pub fn read_epoch_floor(path: &Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Monotonically advance the persistent floor at `path` to `epoch` (never lowers it). Called
/// by `canon pick` after a successful sign.
///
/// CRASH-SAFE (codex anti-rollback P1): the value goes to a SAME-DIR temp file which is
/// `sync_all`'d BEFORE the rename, so the floor is never observable as a truncated or empty
/// file. That matters because `read_epoch_floor` is fail-soft: an empty floor reads as 0, which
/// would drop the ENTIRE durable high-water mark — not just this bump — and reopen rollback
/// acceptance down to the config `min_epoch` baseline. A plain truncating `fs::write` has
/// exactly that hole; so does tmp+rename WITHOUT the fsync, because the rename is atomic for
/// the directory entry while the file's data may still be unflushed.
///
/// The parent directory is fsync'd after the rename on a best-effort basis (opening a directory
/// as a file is not portable — Windows refuses), which is what makes the rename itself durable
/// rather than merely atomic.
///
/// SINGLE-WRITER: `canon pick` is a rare sovereign op. Two concurrent picks could still race
/// the read-modify-write and let a lower epoch's rename land last (a transient lowering healed
/// by the next pick). The rename closes the torn-write hole; a cross-pick lock is the
/// escalation if concurrent picks are ever a real workload (same boundary as install's lock, §7).
pub fn bump_epoch_floor(path: &Path, epoch: u64) -> std::io::Result<()> {
    if epoch <= read_epoch_floor(path) {
        return Ok(());
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("epoch_floor");
    let tmp = path.with_file_name(format!("{name}.tmp.{epoch}"));

    let placed = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(format!("{epoch}\n").as_bytes())?;
        // Durable BEFORE the rename — without this the rename can expose an unflushed file.
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        if let Some(dir) = path.parent() {
            // Best-effort: makes the rename itself survive a power loss. Not portable, and a
            // failure here costs at most this one bump (the next pick re-applies it), so it
            // must never fail the bump.
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    })();

    if placed.is_err() {
        // Never leave `.tmp.*` litter in the sovereign home on a failed bump.
        let _ = std::fs::remove_file(&tmp);
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Generate an ed25519 key, an allowed_signers file, sign `manifest_bytes`, and lay
    /// out a Canon dir. Returns (root, trust). Requires `ssh-keygen` (present on dev/CI mac+linux).
    fn fixture(scope: &str, epoch: u64, rule_files: &[(&str, &str)], manifest_files: &[(&str, &str)]) -> (tempfile::TempDir, SovereignTrust) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // key
        let key = root.join("sov_key");
        let st = Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-N", "", "-C", "flint-sovereign", "-f"])
            .arg(&key)
            .status()
            .expect("ssh-keygen keygen");
        assert!(st.success());
        let pub_line = fs::read_to_string(root.join("sov_key.pub")).unwrap();
        let pub_line = pub_line.trim();
        // allowed_signers: `<identity> <keytype> <key>`
        let identity = "flint-sovereign";
        // pub_line is `ssh-ed25519 AAAA... comment`; allowed_signers wants `<id> ssh-ed25519 AAAA...`
        let key_only: Vec<&str> = pub_line.split_whitespace().take(2).collect();
        let allowed = root.join("allowed_signers");
        fs::write(&allowed, format!("{identity} {}\n", key_only.join(" "))).unwrap();

        // lay out rule files
        for (rel, content) in rule_files {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        // build manifest over manifest_files (hash from the actual file on disk)
        let mut man = format!("{MANIFEST_SCHEMA}\nscope {scope}\nepoch {epoch}\n");
        for (rel, _) in manifest_files {
            let bytes = fs::read(root.join(rel)).unwrap();
            man.push_str(&format!("file {} {}\n", sha256_hex(&bytes), rel));
        }
        let manifest_path = root.join(MANIFEST_FILE);
        fs::write(&manifest_path, man.as_bytes()).unwrap();
        // sign exact manifest bytes, namespace-pinned
        let st = Command::new("ssh-keygen")
            .args(["-Y", "sign", "-f"])
            .arg(&key)
            .args(["-n", SIG_NAMESPACE])
            .arg(&manifest_path)
            .status()
            .expect("ssh-keygen sign");
        assert!(st.success()); // produces CANON.manifest.sig

        let trust = SovereignTrust {
            allowed_signers: allowed,
            signer_identity: identity.to_string(),
            expected_scope: scope.to_string(),
            min_epoch: 0,
        };
        (dir, trust)
    }

    const RULE: &str = "---\nschema: flint/v1\nid: r\ntype: rule\nkind: path\nglob: knowledge/secret/**\nresponse: block\n---\nno secrets\n";

    #[test]
    fn verifies_and_returns_listed_files() {
        let (dir, trust) = fixture("inst-a", 1, &[("rules/r.md", RULE)], &[("rules/r.md", RULE)]);
        let canon = ManifestSignedCanon::new(dir.path());
        let lb = canon.resolve_load_bearing(&trust).expect("valid signed canon resolves");
        assert_eq!(lb.epoch, 1);
        assert_eq!(lb.files.get(Path::new("rules/r.md")).map(|b| b.as_slice()), Some(RULE.as_bytes()));
        // and it reduces to a 1-rule policy.
        let policy = crate::canon::reduce(&lb.files).expect("reduces");
        assert_eq!(policy.rules.len(), 1);
    }

    #[test]
    fn tampered_rule_file_is_hash_mismatch() {
        let (dir, trust) = fixture("inst-a", 1, &[("rules/r.md", RULE)], &[("rules/r.md", RULE)]);
        // agent edits the working-tree rule AFTER signing -> hash mismatch -> fail-closed.
        fs::write(dir.path().join("rules/r.md"), "---\nid: r\ntype: scope\nglob: a/**\nresponse: critique\n---\ntampered\n").unwrap();
        let canon = ManifestSignedCanon::new(dir.path());
        assert!(matches!(canon.resolve_load_bearing(&trust), Err(TrustError::ContentHashMismatch(_))));
    }

    #[test]
    fn tampered_manifest_breaks_signature() {
        let (dir, trust) = fixture("inst-a", 1, &[("rules/r.md", RULE)], &[("rules/r.md", RULE)]);
        // agent edits the manifest (e.g. drops a deny rule) -> signature no longer verifies.
        let mp = dir.path().join(MANIFEST_FILE);
        let mut m = fs::read_to_string(&mp).unwrap();
        m.push_str("file 0000000000000000000000000000000000000000000000000000000000000000 rules/evil.md\n");
        fs::write(&mp, m).unwrap();
        let canon = ManifestSignedCanon::new(dir.path());
        assert!(matches!(canon.resolve_load_bearing(&trust), Err(TrustError::SignatureInvalid)));
    }

    #[test]
    fn scope_mismatch_is_rejected() {
        let (dir, mut trust) = fixture("inst-a", 1, &[("rules/r.md", RULE)], &[("rules/r.md", RULE)]);
        trust.expected_scope = "inst-b".into(); // point trust at a different instance
        let canon = ManifestSignedCanon::new(dir.path());
        assert!(matches!(canon.resolve_load_bearing(&trust), Err(TrustError::ScopeMismatch { .. })));
    }

    #[test]
    fn rollback_below_min_epoch_is_rejected() {
        let (dir, mut trust) = fixture("inst-a", 2, &[("rules/r.md", RULE)], &[("rules/r.md", RULE)]);
        trust.min_epoch = 5; // we've seen a newer epoch; this older signed manifest is a rollback.
        let canon = ManifestSignedCanon::new(dir.path());
        assert!(matches!(canon.resolve_load_bearing(&trust), Err(TrustError::RollbackDetected { .. })));
    }

    #[test]
    fn missing_listed_file_fails_whole_canon() {
        // manifest lists a file that doesn't exist on disk -> whole Canon Err (req #3).
        let (dir, trust) = fixture("inst-a", 1, &[("rules/r.md", RULE)], &[("rules/r.md", RULE)]);
        // re-sign a manifest that ALSO lists a missing file would change the sig; instead
        // delete the listed file after signing -> FileMissing.
        fs::remove_file(dir.path().join("rules/r.md")).unwrap();
        let canon = ManifestSignedCanon::new(dir.path());
        assert!(matches!(canon.resolve_load_bearing(&trust), Err(TrustError::FileMissing(_))));
    }

    #[test]
    fn no_manifest_is_err() {
        let dir = tempfile::tempdir().unwrap();
        let trust = SovereignTrust {
            allowed_signers: dir.path().join("nope"),
            signer_identity: "x".into(),
            expected_scope: "inst-a".into(),
            min_epoch: 0,
        };
        let canon = ManifestSignedCanon::new(dir.path());
        assert!(matches!(canon.resolve_load_bearing(&trust), Err(TrustError::NoManifest)));
    }

    #[test]
    fn unsafe_manifest_path_is_rejected() {
        assert!(safe_relative("../escape").is_none());
        assert!(safe_relative("/abs/path").is_none());
        assert!(safe_relative("a/../../b").is_none());
        assert!(safe_relative("rules/ok.md").is_some());
        assert!(safe_relative("").is_none());
    }

    #[test]
    fn read_manifest_header_survives_a_dirty_tree() {
        // The epoch-wart regression guard: a rule edited AFTER the last pick (the normal
        // pre-re-pick state) makes FULL verification fail — but the header epoch is a
        // property of the manifest, not the tree, so `read_manifest_header` must still
        // return it. (Full verify failing here is exactly why epoch used to reset to 1.)
        let (dir, trust) = fixture("inst-a", 7, &[("rules/r.md", RULE)], &[("rules/r.md", RULE)]);
        fs::write(dir.path().join("rules/r.md"), "---\nedited after signing\n").unwrap();
        // full verify now fails (tree dirty)…
        assert!(ManifestSignedCanon::new(dir.path()).resolve_load_bearing(&trust).is_err());
        // …but the header epoch is still readable, so pick bumps 7 -> 8, never back to 1.
        let hdr = read_manifest_header(dir.path()).expect("header still readable on a dirty tree");
        assert_eq!(hdr.epoch, 7);
        assert_eq!(hdr.scope, "inst-a");
    }

    #[test]
    fn read_manifest_header_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_manifest_header(dir.path()).is_none());
    }

    #[test]
    fn epoch_floor_reads_soft_and_bumps_monotonic() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("floor");
        // absent → 0 (fail-soft to the config baseline; never lowers protection)
        assert_eq!(read_epoch_floor(&f), 0);
        bump_epoch_floor(&f, 5).unwrap();
        assert_eq!(read_epoch_floor(&f), 5);
        // a lower bump is a no-op (monotonic — the floor never retreats)
        bump_epoch_floor(&f, 3).unwrap();
        assert_eq!(read_epoch_floor(&f), 5);
        // a higher bump advances it
        bump_epoch_floor(&f, 9).unwrap();
        assert_eq!(read_epoch_floor(&f), 9);
        // the same-dir temp is consumed by the rename, none is left behind. NOTE: this asserts
        // no temp LITTER, not atomicity — a plain `fs::write` would pass it too (it creates no
        // temp at all). Crash-atomicity is not unit-testable without fault injection; the
        // error-path guard below is the one that actually fails on a regression.
        let leftover = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
        assert!(!leftover, "a successful bump must leave no temp file behind");
        // a corrupt floor reads as 0 (fail-soft), so it can only fail to ADD its increment —
        // never DoS the gate, never lower it below the config min_epoch baseline.
        std::fs::write(&f, "not-a-number\n").unwrap();
        assert_eq!(read_epoch_floor(&f), 0);
    }

    #[test]
    fn a_failed_bump_leaves_no_temp_behind() {
        // The real guard for the temp-file contract: force the rename to fail (a DIRECTORY at
        // the floor path can never be replaced by a file) and assert the temp is cleaned up.
        // Without the cleanup this fails — unlike the success-path assertion above, which a
        // non-atomic implementation passes. `$FLINT_HOME` must not accumulate `.tmp.*` litter.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("floor");
        std::fs::create_dir(&f).unwrap();
        assert_eq!(read_epoch_floor(&f), 0, "a directory reads fail-soft as 0");

        let err = bump_epoch_floor(&f, 7).expect_err("renaming a file over a directory must fail");
        let _ = err;
        let litter: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(litter.is_empty(), "a failed bump must clean up its temp, found: {litter:?}");
    }

    #[test]
    fn manifest_parse_strict() {
        assert!(Manifest::parse(b"wrong-schema\n").is_err());
        assert!(Manifest::parse(b"flint-canon-manifest-v1\nscope a\n").is_err()); // missing epoch
        assert!(Manifest::parse(b"flint-canon-manifest-v1\nepoch 1\n").is_err()); // missing scope
        assert!(Manifest::parse(b"flint-canon-manifest-v1\nscope a\nepoch x\n").is_err()); // bad epoch
        let ok = Manifest::parse(b"flint-canon-manifest-v1\nscope a\nepoch 3\nfile aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa rules/x.md\n").unwrap();
        assert_eq!(ok.scope, "a");
        assert_eq!(ok.epoch, 3);
        assert_eq!(ok.files.len(), 1);
        // bad sha length
        assert!(Manifest::parse(b"flint-canon-manifest-v1\nscope a\nepoch 1\nfile abc rules/x.md\n").is_err());
    }
}
