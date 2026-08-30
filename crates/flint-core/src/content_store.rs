//! Content-addressed store (`.flint/objects/`).
//!
//! sha256 CAS for freezing a verify's command + stdin + stdout, so `flint check`
//! can re-run the frozen artifacts on any machine. Flint stores its own content
//! because the kernel's `BlobStore` exists but is not wired (build-plan §4).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

/// A filesystem content-addressed store rooted at a directory.
pub struct ContentStore {
    root: PathBuf,
}

impl ContentStore {
    /// Open (creating if needed) the store at `root`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .with_context(|| format!("create objects dir {}", root.display()))?;
        Ok(Self { root })
    }

    /// Store `bytes`, returning the content id `sha256:<hex>`. Idempotent.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let hex = sha256_hex(bytes);
        // sha256_hex always produces a valid 64-char lowercase hex string.
        let path = self
            .path_for(&hex)
            .expect("sha256 hex is always a valid content id");
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, bytes).with_context(|| format!("write object {hex}"))?;
        }
        Ok(format!("sha256:{hex}"))
    }

    /// Load the bytes for a content id (`sha256:<hex>` or bare hex). A malformed
    /// id is an error, never a panic (codex P1 #12).
    pub fn get(&self, oid: &str) -> Result<Vec<u8>> {
        let hex = oid.strip_prefix("sha256:").unwrap_or(oid);
        let path = self
            .path_for(hex)
            .ok_or_else(|| anyhow!("invalid content id {oid:?}"))?;
        fs::read(&path).with_context(|| format!("read object {oid}"))
    }

    /// Whether an object exists. A malformed id is simply `false`, not a panic.
    pub fn exists(&self, oid: &str) -> bool {
        let hex = oid.strip_prefix("sha256:").unwrap_or(oid);
        self.path_for(hex).map(|p| p.exists()).unwrap_or(false)
    }

    // OCI-style two-char bucket. Returns None for malformed hex (too short or
    // non-hex), so callers fail closed instead of slicing out of bounds.
    fn path_for(&self, hex: &str) -> Option<PathBuf> {
        if hex.len() < 3 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(self.root.join(&hex[..2]).join(&hex[2..]))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("objects")).unwrap();
        let oid = store.put(b"rsync -an --checksum output").unwrap();
        assert!(oid.starts_with("sha256:"));
        assert_eq!(store.get(&oid).unwrap(), b"rsync -an --checksum output");
        assert!(store.exists(&oid));
    }

    #[test]
    fn same_bytes_same_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("objects")).unwrap();
        assert_eq!(store.put(b"abc").unwrap(), store.put(b"abc").unwrap());
        assert_ne!(store.put(b"abc").unwrap(), store.put(b"abd").unwrap());
    }

    #[test]
    fn missing_object_errors() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("objects")).unwrap();
        assert!(store.get(&format!("sha256:{}", "ab".repeat(32))).is_err());
        assert!(!store.exists("sha256:deadbeef"));
    }

    // codex P1 #12: a malformed content id must error, never panic on slicing.
    #[test]
    fn malformed_id_does_not_panic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ContentStore::open(tmp.path().join("objects")).unwrap();
        assert!(store.get("x").is_err());
        assert!(store.get("sha256:").is_err());
        assert!(store.get("sha256:zz").is_err());
        assert!(!store.exists("x"));
        assert!(!store.exists("sha256:"));
    }
}
