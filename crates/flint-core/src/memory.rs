//! Memory — the bring-your-own-store PORT (self-contained spec v2.1 §4).
//!
//! flint's wedge is the ENFORCEMENT gate (signed Canon, action-time block). Memory is the
//! OPT-IN other half: the place captured judgment sediments. The design decision (the owner's, the
//! unbundle) is that flint does NOT own that store — it defines a small [`MemoryPort`] and
//! ships ONE thin adapter, [`FsVault`], over markdown-in-a-folder. Because Obsidian, logseq,
//! foam, dendron and git-wikis are ALL "markdown files in a directory", pointing the fs
//! adapter at an existing vault is near-free: flint connects to your notes, it does not
//! replace them (§12 — the differentiator, not a commodity PKM).
//!
//! SCOPE (lean-day-one): the fs adapter is the whole v1. API-backed adapters (Notion, a
//! hosted wiki) are staged until a real need exists — the PORT is the stable seam that lets
//! them slot in without touching callers.
//!
//! NOT load-bearing (the deliberate contrast with [`crate::canon`] / [`crate::trust`]):
//! memory is knowledge, never a rule. Nothing here is signed, judged, or injected as a gate.
//! What flint ships is a GENERIC empty scaffold ([`scaffold_vault`]) — an inbox + an
//! orientation stub — NOT anyone's methodology (§4: the 5-bucket / retro shape stays in the
//! owner's private vault, never in this MIT crate).

use std::path::{Component, Path, PathBuf};

/// Why a memory operation failed. Memory is not fail-closed (a bad note is just a bad note),
/// so these surface honest IO / confinement problems — they never silently swallow a write.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory io ({0}): {1}")]
    Io(String, #[source] std::io::Error),
    /// A `source.ref` (or note id) that would escape the vault — rejected before any read.
    #[error("unsafe memory reference (must stay inside the vault): {0}")]
    UnsafeReference(String),
    /// A durable-note id that is not a safe filename slug.
    #[error("invalid note id `{0}`: ascii letters / digits / `-` / `_` only, no separators")]
    BadId(String),
    /// `write_durable` refuses to clobber an existing note (the owner edits it directly).
    #[error("durable note already exists at {0} — edit it directly")]
    NoteExists(PathBuf),
    /// A hot capture with no content.
    #[error("empty gist — a capture is a one-line note of what you learned")]
    EmptyGist,
    #[error("memory content at {0} is not UTF-8")]
    NotUtf8(PathBuf),
}

type Result<T> = std::result::Result<T, MemoryError>;

/// The storage-agnostic memory interface (the "port"). Five moves — hot capture, read the
/// inbox, write a durable note, resolve a source pointer, read the orientation doc — cover
/// what the suite skills need; a backend is anything that can do these over its own store.
///
/// The fs adapter ([`FsVault`]) is the only v1 implementor; this trait exists so a future
/// Notion / hosted-wiki adapter slots in behind the same calls (§4 stable seam).
pub trait MemoryPort {
    /// Append a one-line hot-capture gist to the inbox; returns the new pending gist count.
    fn capture(&self, gist: &str) -> Result<usize>;
    /// The pending inbox gists (the leading `- ` marker stripped), oldest first.
    fn list_inbox(&self) -> Result<Vec<String>>;
    /// Write a durable note `<id>` with `content`; returns its path. Refuses to overwrite an
    /// existing note.
    fn write_durable(&self, id: &str, content: &str) -> Result<PathBuf>;
    /// Resolve a `source.ref` to its content, or `None` if the vault holds no such file.
    /// The reference is vault-relative and CONFINED (no `..` / absolute escape).
    fn resolve_source(&self, reference: &str) -> Result<Option<String>>;
    /// The orientation document (a `PORTAL.md` / `README.md` at the vault root), if present —
    /// what a session reads first to reload its bearings.
    fn read_orientation(&self) -> Result<Option<String>>;
}

/// The markdown-in-a-folder adapter: the whole memory backend for v1. `root` is a vault
/// directory (a fresh scaffold, or an existing Obsidian/wiki/notes folder). It owns no
/// format beyond "files are markdown" — no bucket layout, no frontmatter schema imposed.
pub struct FsVault {
    root: PathBuf,
}

/// Orientation filenames tried in order (first that exists wins). Generic on purpose — a
/// vault names its front-door doc one of these; flint imposes no bucket structure.
const ORIENTATION_CANDIDATES: &[&str] = &["PORTAL.md", "README.md", "ORIENTATION.md"];

const INBOX_FILE: &str = "inbox.md";

impl FsVault {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn io<T>(&self, what: &str, r: std::io::Result<T>) -> Result<T> {
        r.map_err(|e| MemoryError::Io(what.to_string(), e))
    }

    /// Remove the FIRST pending inbox gist whose text equals `gist` — triage (§5/§7): the owner
    /// promoted or tossed it, so it leaves the raw-ore inbox and stops re-appearing in review.
    /// Returns whether a line was removed. The inbox is NON-load-bearing raw ore, so a plain
    /// rewrite (no atomic rename) is acceptable — the worst case of a crash mid-write is one
    /// un-triaged gist the owner re-captures, never a corrupted rule. A missing inbox removes
    /// nothing (`Ok(false)`), never an error.
    pub fn resolve_inbox(&self, gist: &str) -> Result<bool> {
        let inbox = self.root.join(INBOX_FILE);
        let text = match std::fs::read_to_string(&inbox) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(MemoryError::Io("read inbox".into(), e)),
        };
        let want = gist.trim();
        let mut removed = false;
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            // Drop only the FIRST matching `- <gist>` bullet; everything else is preserved.
            if !removed {
                if let Some(g) = line.trim_start().strip_prefix("- ") {
                    if g.trim() == want {
                        removed = true;
                        continue;
                    }
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        if removed {
            self.io("rewrite inbox", std::fs::write(&inbox, out))?;
        }
        Ok(removed)
    }
}

impl MemoryPort for FsVault {
    fn capture(&self, gist: &str) -> Result<usize> {
        if gist.trim().is_empty() {
            return Err(MemoryError::EmptyGist);
        }
        self.io("create vault", std::fs::create_dir_all(&self.root))?;
        let inbox = self.root.join(INBOX_FILE);
        let line = crate::pit::format_inbox_gist(gist);
        use std::io::Write;
        let mut f = self.io(
            "open inbox",
            std::fs::OpenOptions::new().create(true).append(true).open(&inbox),
        )?;
        self.io("append inbox", f.write_all(line.as_bytes()))?;
        let text = self.io("read inbox", std::fs::read_to_string(&inbox))?;
        Ok(crate::pit::count_inbox(&text))
    }

    fn list_inbox(&self) -> Result<Vec<String>> {
        let inbox = self.root.join(INBOX_FILE);
        let text = match std::fs::read_to_string(&inbox) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(MemoryError::Io("read inbox".into(), e)),
        };
        Ok(text
            .lines()
            .filter_map(|l| {
                let t = l.trim_start();
                // A gist is a non-empty `- …` bullet; strip the marker (parity with count_inbox).
                t.strip_prefix("- ").map(str::trim).filter(|g| !g.is_empty()).map(String::from)
            })
            .collect())
    }

    fn write_durable(&self, id: &str, content: &str) -> Result<PathBuf> {
        if !crate::pit::is_safe_id(id) {
            return Err(MemoryError::BadId(id.to_string()));
        }
        self.io("create vault", std::fs::create_dir_all(&self.root))?;
        let path = self.root.join(format!("{id}.md"));
        // create_new (O_CREAT|O_EXCL): refuses an existing file OR a planted (possibly
        // dangling) symlink a same-UID actor could use to redirect the write out of the
        // vault — same symlink-safe discipline as `flint pit save`.
        use std::io::Write;
        let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(MemoryError::NoteExists(path));
            }
            Err(e) => return Err(MemoryError::Io(format!("create {}", path.display()), e)),
        };
        // Ensure a trailing newline so notes concatenate cleanly in a markdown vault.
        let body = if content.ends_with('\n') { content.to_string() } else { format!("{content}\n") };
        self.io("write note", f.write_all(body.as_bytes()))?;
        Ok(path)
    }

    fn resolve_source(&self, reference: &str) -> Result<Option<String>> {
        let rel = confine(reference).ok_or_else(|| MemoryError::UnsafeReference(reference.into()))?;
        // Try the reference verbatim; if it names no extension, also try `<ref>.md` (markdown
        // vaults reference notes by bare slug). A DIRECTORY at the bare-slug path (the common
        // Obsidian "folder note" layout: `decisions/` next to `decisions.md`) is "not a note"
        // — `read_confined` returns None for it, so we fall through to the `.md` sibling.
        let mut candidates = vec![self.root.join(&rel)];
        if rel.extension().is_none() {
            candidates.push(self.root.join(format!("{reference}.md")));
        }
        for full in &candidates {
            if let Some(content) = read_confined(&self.root, full)? {
                return Ok(Some(content));
            }
        }
        Ok(None)
    }

    fn read_orientation(&self) -> Result<Option<String>> {
        for name in ORIENTATION_CANDIDATES {
            if let Some(content) = read_confined(&self.root, &self.root.join(name))? {
                return Ok(Some(content));
            }
        }
        Ok(None)
    }
}

/// Read a vault file that must resolve — THROUGH any symlinks — to a location inside `root`.
/// `Ok(None)` means absent OR a directory (a folder-note, not a readable note); an escape is
/// a hard [`MemoryError::UnsafeReference`], never a silent miss. `confine` blocks string-level
/// `..`/absolute escapes; this adds the symlink-escape guard the rest of the codebase applies
/// on every untrusted read (canonicalize + `starts_with`, mirroring `trust.rs` /
/// `install.rs`) — a symlink INSIDE the vault pointing at `~/.ssh/id_ed25519` must not leak.
fn read_confined(root: &Path, full: &Path) -> Result<Option<String>> {
    // symlink_metadata does NOT follow the final component — a missing path (or dangling
    // symlink) is "absent", not an error.
    match std::fs::symlink_metadata(full) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(MemoryError::Io(format!("stat {}", full.display()), e)),
    }
    let root_canon = std::fs::canonicalize(root)
        .map_err(|e| MemoryError::Io(format!("canonicalize vault {}", root.display()), e))?;
    // canonicalize RESOLVES symlinks: a link pointing outside the vault canonicalizes to its
    // real target, which then fails the `starts_with` guard.
    let full_canon = std::fs::canonicalize(full)
        .map_err(|e| MemoryError::Io(format!("canonicalize {}", full.display()), e))?;
    if !full_canon.starts_with(&root_canon) {
        return Err(MemoryError::UnsafeReference(full.display().to_string()));
    }
    if full_canon.is_dir() {
        return Ok(None); // a folder-note directory, not a note file — treat as absent.
    }
    let bytes = std::fs::read(&full_canon)
        .map_err(|e| MemoryError::Io(format!("read {}", full_canon.display()), e))?;
    String::from_utf8(bytes).map(Some).map_err(|_| MemoryError::NotUtf8(full_canon))
}

/// Confine a vault-relative reference to normal components (reject absolute paths and any
/// `..` traversal). `.` segments are allowed and dropped. Returns the normalized relative
/// path, or `None` if the reference would escape the vault.
fn confine(reference: &str) -> Option<PathBuf> {
    let p = Path::new(reference);
    if p.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            // ParentDir / RootDir / Prefix all escape — reject.
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Scaffold a fresh, GENERIC empty vault at `root`: an inbox and an orientation stub. Both
/// are created only if absent (idempotent — never clobbers an existing vault). This is what
/// `flint init --with-memory` writes when no `--vault` points at an existing notes folder;
/// it deliberately carries NO methodology (empty structure only, §4).
pub fn scaffold_vault(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root).map_err(|e| MemoryError::Io("create vault".into(), e))?;
    create_if_absent(&root.join(INBOX_FILE), "# Memory inbox\n\nHot captures land here as `- …` bullets (`flint memory capture`).\n")?;
    create_if_absent(
        &root.join("README.md"),
        "# Memory vault\n\nYour bring-your-own memory for flint. Markdown files in this folder ARE the store — point an Obsidian/wiki vault here, or let this scaffold hold notes.\n",
    )?;
    Ok(())
}

/// Write `contents` to `path` only if it does not already exist (create_new = symlink-safe);
/// an existing file (or symlink) is left untouched, not an error.
fn create_if_absent(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => f
            .write_all(contents.as_bytes())
            .map_err(|e| MemoryError::Io(format!("write {}", path.display()), e)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(MemoryError::Io(format!("create {}", path.display()), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, FsVault) {
        let dir = tempfile::tempdir().unwrap();
        let v = FsVault::new(dir.path());
        (dir, v)
    }

    #[test]
    fn capture_appends_and_counts() {
        let (_d, v) = vault();
        assert_eq!(v.capture("hit a wall on epoch reset").unwrap(), 1);
        assert_eq!(v.capture("second gist").unwrap(), 2);
        let inbox = v.list_inbox().unwrap();
        assert_eq!(inbox, vec!["hit a wall on epoch reset", "second gist"]);
    }

    #[test]
    fn capture_collapses_to_one_line() {
        let (_d, v) = vault();
        v.capture("multi\nline   gist").unwrap();
        assert_eq!(v.list_inbox().unwrap(), vec!["multi line gist"]);
    }

    #[test]
    fn capture_empty_is_err() {
        let (_d, v) = vault();
        assert!(matches!(v.capture("   \n  "), Err(MemoryError::EmptyGist)));
    }

    #[test]
    fn list_inbox_missing_is_empty_not_error() {
        let (_d, v) = vault();
        assert!(v.list_inbox().unwrap().is_empty());
    }

    #[test]
    fn write_durable_creates_and_refuses_overwrite() {
        let (_d, v) = vault();
        let p = v.write_durable("epoch-reset-pit", "the note body").unwrap();
        assert!(p.ends_with("epoch-reset-pit.md"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "the note body\n"); // trailing NL added
        // second write to the same id refuses (edit it directly).
        assert!(matches!(v.write_durable("epoch-reset-pit", "x"), Err(MemoryError::NoteExists(_))));
    }

    #[test]
    fn write_durable_preserves_existing_trailing_newline() {
        let (_d, v) = vault();
        let p = v.write_durable("n", "body\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "body\n"); // no double newline
    }

    #[test]
    fn write_durable_rejects_unsafe_id() {
        let (_d, v) = vault();
        for bad in ["../escape", "a/b", "has space", ""] {
            assert!(matches!(v.write_durable(bad, "x"), Err(MemoryError::BadId(_))), "must reject `{bad}`");
        }
    }

    #[test]
    fn resolve_source_reads_confined_and_md_fallback() {
        let (_d, v) = vault();
        std::fs::create_dir_all(v.root().join("notes")).unwrap();
        std::fs::write(v.root().join("notes/foo.md"), "the note").unwrap();
        // exact path
        assert_eq!(v.resolve_source("notes/foo.md").unwrap().as_deref(), Some("the note"));
        // bare slug → `.md` fallback
        assert_eq!(v.resolve_source("notes/foo").unwrap().as_deref(), Some("the note"));
        // absent → None (not an error)
        assert_eq!(v.resolve_source("notes/missing").unwrap(), None);
    }

    #[test]
    fn resolve_source_rejects_escape() {
        let (_d, v) = vault();
        for bad in ["../secret", "/etc/passwd", "a/../../b"] {
            assert!(matches!(v.resolve_source(bad), Err(MemoryError::UnsafeReference(_))), "must reject `{bad}`");
        }
    }

    #[test]
    fn resolve_source_bare_slug_dir_falls_back_to_md() {
        // Obsidian "folder note": a `decisions/` dir sitting next to a `decisions.md` index.
        // The bare slug `decisions` must skip the directory and resolve the `.md` sibling —
        // NOT surface a raw IsADirectory IO error (eng-review finding 2).
        let (_d, v) = vault();
        std::fs::create_dir_all(v.root().join("decisions")).unwrap();
        std::fs::write(v.root().join("decisions.md"), "the index note").unwrap();
        assert_eq!(v.resolve_source("decisions").unwrap().as_deref(), Some("the index note"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_source_rejects_symlink_escape() {
        // A symlink INSIDE the vault pointing at a file OUTSIDE it must not leak the target
        // (eng-review finding 1) — string-level `confine` passes `link`, but the canonicalize
        // + starts_with guard rejects the resolved-out-of-vault target.
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "PRIVATE KEY BYTES").unwrap();
        let (_d, v) = vault();
        std::fs::create_dir_all(v.root()).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), v.root().join("leak")).unwrap();
        assert!(matches!(v.resolve_source("leak"), Err(MemoryError::UnsafeReference(_))));
    }

    #[cfg(unix)]
    #[test]
    fn read_orientation_rejects_symlink_escape() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("elsewhere"), "not yours").unwrap();
        let (_d, v) = vault();
        std::fs::create_dir_all(v.root()).unwrap();
        std::os::unix::fs::symlink(outside.path().join("elsewhere"), v.root().join("PORTAL.md")).unwrap();
        assert!(matches!(v.read_orientation(), Err(MemoryError::UnsafeReference(_))));
    }

    #[test]
    fn read_orientation_prefers_portal_then_readme() {
        let (_d, v) = vault();
        assert_eq!(v.read_orientation().unwrap(), None); // nothing yet
        std::fs::write(v.root().join("README.md"), "readme orientation").unwrap();
        assert_eq!(v.read_orientation().unwrap().as_deref(), Some("readme orientation"));
        std::fs::write(v.root().join("PORTAL.md"), "portal orientation").unwrap();
        assert_eq!(v.read_orientation().unwrap().as_deref(), Some("portal orientation")); // PORTAL wins
    }

    #[test]
    fn scaffold_is_idempotent_and_generic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("brain");
        scaffold_vault(&root).unwrap();
        let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
        assert!(root.join("inbox.md").exists());
        // does not clobber on re-run: edit README, scaffold again, edit survives.
        std::fs::write(root.join("README.md"), "my own words").unwrap();
        scaffold_vault(&root).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("README.md")).unwrap(), "my own words");
        // ships no methodology (a generic vault stub, no private PKM vocabulary baked in).
        assert!(!readme.to_lowercase().contains("bucket"));
    }

    #[test]
    fn confine_normalizes_and_rejects() {
        assert_eq!(confine("a/b.md"), Some(PathBuf::from("a/b.md")));
        assert_eq!(confine("./a/b"), Some(PathBuf::from("a/b")));
        assert_eq!(confine(".."), None);
        assert_eq!(confine("a/../../b"), None);
        assert_eq!(confine("/abs"), None);
        assert_eq!(confine(""), None);
    }

    #[test]
    fn fsvault_is_usable_as_dyn_port() {
        // The port abstraction earns its place: callers program against &dyn MemoryPort.
        let (_d, v) = vault();
        let port: &dyn MemoryPort = &v;
        assert_eq!(port.capture("via trait object").unwrap(), 1);
    }

    #[test]
    fn resolve_inbox_removes_only_the_named_gist() {
        let (_d, v) = vault();
        v.capture("wall one").unwrap();
        v.capture("wall two").unwrap();
        v.capture("wall three").unwrap();
        assert!(v.resolve_inbox("wall two").unwrap(), "the middle gist is removed");
        assert_eq!(v.list_inbox().unwrap(), vec!["wall one", "wall three"]);
    }

    #[test]
    fn resolve_inbox_unknown_gist_removes_nothing() {
        let (_d, v) = vault();
        v.capture("only gist").unwrap();
        assert!(!v.resolve_inbox("not present").unwrap());
        assert_eq!(v.list_inbox().unwrap(), vec!["only gist"]);
    }

    #[test]
    fn resolve_inbox_removes_first_of_duplicates() {
        let (_d, v) = vault();
        v.capture("dup").unwrap();
        v.capture("dup").unwrap();
        assert!(v.resolve_inbox("dup").unwrap());
        assert_eq!(v.list_inbox().unwrap(), vec!["dup"], "one duplicate remains");
    }

    #[test]
    fn resolve_inbox_missing_inbox_is_false_not_error() {
        let (_d, v) = vault();
        assert!(!v.resolve_inbox("anything").unwrap());
    }

    #[test]
    fn resolve_inbox_preserves_non_bullet_lines() {
        // A header / free line in the inbox survives triage — only bullets are gists.
        let (_d, v) = vault();
        std::fs::create_dir_all(v.root()).unwrap();
        std::fs::write(v.root().join("inbox.md"), "# Inbox\n\n- keep me\n- toss me\n").unwrap();
        assert!(v.resolve_inbox("toss me").unwrap());
        let text = std::fs::read_to_string(v.root().join("inbox.md")).unwrap();
        assert!(text.contains("# Inbox"));
        assert!(text.contains("- keep me"));
        assert!(!text.contains("- toss me"));
    }
}
