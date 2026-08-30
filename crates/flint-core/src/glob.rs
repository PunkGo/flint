//! Scope glob matching + well-formedness (PIVOT P0a: extracted from `projector`).
//!
//! Pure, no IO. Shared by the runtime gate (`crate::touchstone`), the Canon
//! reducer, and (during the kernel-line transition) `projector` via re-export.
//! This is the ONLY clean keep from the old event-sourcing `projector` (the
//! reframe-and-diff §2.1): the glob logic is harness/ledger agnostic, so it
//! survives the kernel teardown and gets its own home here.

/// A scope must be a canonical slash path over a SMALL ASCII charset: non-empty,
/// `/`-separated segments each matching `[A-Za-z0-9._-]+`, none being `.` or `..`.
/// Rejecting everything else at claim admission (percent-encoding `%`, backslash,
/// control chars, Unicode slash-confusables, whitespace) keeps the lexical glob
/// matcher sound — a non-canonical scope can never sit "really inside" a high-risk
/// glob while evading the matcher, and no downstream normalizer can reinterpret it
/// (codex P2 / P2-R2).
pub(crate) fn scope_is_canonical(scope: &str) -> bool {
    !scope.is_empty()
        && scope.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

/// Lexically normalize a file-path scope reported by a harness hook into the canonical
/// `/`-separated form the gate globs are written against — WITHOUT touching the
/// filesystem (the hook fires before the write; the path may not exist yet).
///
/// This is the harness-boundary canonicalization the PIVOT needs and the kernel-line did
/// at claim admission: a harness delivers attacker-influenced path SPELLINGS (`./x`,
/// `a//b`, `a/./b`, `a/b/../c`, trailing `/`), and the lexical [`glob_match`] is only
/// sound against canonical scopes — so an un-normalized `./knowledge/secret/key` would
/// DODGE a `knowledge/secret/**` deny while the filesystem still writes inside the denied
/// tree (codex P1). We collapse `//`, drop `.` and a leading `./`, resolve `..` lexically,
/// and strip a trailing `/`; a leading `/` (absolute) is preserved.
///
/// HONEST BOUNDARY (§12.6, the macOS/Linux Claude Code + Codex surface): only `/` is a
/// path separator here, so `\`, percent-encoding, control chars, whitespace, and Unicode
/// slash-confusables are LITERAL filename bytes (they cannot escape a subtree on Unix) —
/// structural normalization is therefore sufficient for matcher soundness. A `\`-separated
/// (Windows) harness feeds its paths through [`normalize_separators`] FIRST (the adapter
/// boundary), so by the time they arrive here `/` is again the only separator.
/// Absolute action paths still won't match the relative gate globs (a pre-existing,
/// unchanged design boundary — not a regression of this fix).
pub(crate) fn normalize_path_scope(raw: &str) -> String {
    let absolute = raw.starts_with('/');
    // A Windows drive-absolute spelling (`C:/ws/...`) carries its ROOT in the FIRST
    // segment, so `..` must be a no-op there exactly as it is above a leading `/` —
    // otherwise `C:/../ws/secrets/key` would pop the drive off and decay into a
    // relative path that the workspace join then re-roots one level too deep, dodging
    // a `secrets/**` deny the filesystem write still lands inside.
    let drive_rooted = is_drive_absolute(raw);
    let mut out: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => continue, // collapse `//`, leading/trailing `/`, and `.`
            ".." => match out.last() {
                Some(&last) if last != ".." && !(drive_rooted && out.len() == 1) => {
                    out.pop(); // climb back over a real segment
                }
                _ if absolute || drive_rooted => {} // `..` above the root is a no-op
                _ => out.push(".."), // a relative path that escapes upward keeps the `..`
            },
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string() // degenerate (e.g. `a/..`) -> cwd; matches no specific subtree glob
    } else {
        joined
    }
}

/// Rewrite `\` into `/` so a Windows-spelled path enters [`normalize_path_scope`] on the
/// one separator the lexical matcher understands. This is the M12.1-P2-b follow-up the
/// glob HONEST BOUNDARY named: Grok is the first `\`-path harness, and on Windows `\` IS
/// a separator, so leaving it literal would let `secrets\key` sit "really inside" a
/// `secrets/**` deny while never matching it. Applied at the ADAPTER boundary (per path
/// value) and to the workspace root, never to a gate glob — a glob containing `\` is
/// rejected by [`glob_is_wellformed`] and stays rejected.
pub(crate) fn normalize_separators(raw: &str) -> String {
    raw.replace('\\', "/")
}

/// True for a Windows drive-absolute spelling (`C:/...`) AFTER separator normalization.
/// Lexical and ASCII-only, matching the rest of this module.
fn is_drive_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

/// Uppercase a leading drive letter (`c:/ws` -> `C:/ws`) so the two spellings of one drive
/// compare equal. ONLY the drive letter folds — the rest of the path stays byte-exact,
/// because the gate globs themselves are case-sensitive.
fn fold_drive_letter(p: &str) -> String {
    if is_drive_absolute(p) {
        let mut s = String::with_capacity(p.len());
        s.push(p.as_bytes()[0].to_ascii_uppercase() as char);
        s.push_str(&p[1..]);
        s
    } else {
        p.to_string()
    }
}

/// True for the SYNTHETIC scopes the adapter emits for non-file actions (`exec:bash`,
/// `tool:<name>`) — flint-controlled labels matched verbatim, NOT filesystem paths, so they
/// must NOT be relativized against the workspace.
pub(crate) fn is_synthetic_scope(s: &str) -> bool {
    s.starts_with("exec:") || s.starts_with("tool:")
}

/// Resolve a boundary-normalized FILE-PATH scope to its WORKSPACE-RELATIVE canonical form
/// for matching the relative gate globs. `workspace_root` is the (absolute) directory the
/// harness runs the hook in — the root the globs are written against.
///
/// codex P1 (r2): a harness reports paths ABSOLUTELY (`/abs/.../knowledge/secret/key` — the
/// normal Claude `file_path` case) or with a `..` that climbs out of and back into the tree
/// (`../<workspace-basename>/knowledge/secret/key`). Both must resolve to the same in-tree
/// relative scope a `knowledge/secret/**` deny is written against, or the deny silently
/// never fires while the filesystem write still lands in the denied subtree. We join (for a
/// relative scope) then lexically normalize, so `..` is honored, and strip the root prefix.
///
/// Returns the in-tree relative scope, or `None` when the path resolves OUTSIDE the
/// workspace (no relative in-tree glob governs it — the caller drops it). Pure; LEXICAL —
/// no symlink resolution (the hook fires pre-write; the path may not exist; §12.6 boundary).
///
/// WINDOWS (the Grok deployment surface): "absolute" here means a leading `/` OR a drive
/// prefix (`C:/...`), and the root is separator-normalized on the way in (a harness or a
/// config may hand us `C:\ws`). The drive LETTER compares case-insensitively — Windows
/// spells the same drive both ways and a `c:` vs `C:` mismatch would silently drop every
/// in-tree scope to `None`, i.e. fail OPEN.
pub(crate) fn relativize_to_workspace(scope: &str, workspace_root: &str) -> Option<String> {
    let root = normalize_path_scope(&normalize_separators(workspace_root));
    let abs = if scope.starts_with('/') || is_drive_absolute(scope) {
        normalize_path_scope(scope)
    } else {
        normalize_path_scope(&format!("{root}/{scope}"))
    };
    let root = fold_drive_letter(&root);
    let abs = fold_drive_letter(&abs);
    if abs == root {
        return Some(".".to_string()); // the workspace dir itself
    }
    abs.strip_prefix(&format!("{root}/")).map(str::to_string)
}

/// Minimal scope glob, three mutually-exclusive shapes (a well-formed glob is exactly
/// one — see [`glob_is_wellformed`]):
///   - `<prefix>/**`  — the prefix and any descendant (subtree).
///   - `**/<suffix>`  — any scope whose trailing path component(s) equal `<suffix>`
///     (a basename glob: `**/Cargo.toml` matches `Cargo.toml` and `a/b/Cargo.toml`).
///     This is what `spec-over-precedent` needs (a vendor-format file ANYWHERE).
///   - otherwise an exact match.
///
/// Shared with the runtime gate (`crate::touchstone`) so hold / altitude / gate globs
/// match alike. Pure; no IO. NOTE: the `**/` basename arm is checked FIRST, but the
/// shapes can't overlap for a well-formed glob (a `**/` glob has no trailing `/**`
/// without an interior `*`, which the well-formedness gate rejects).
pub(crate) fn glob_match(glob: &str, scope: &str) -> bool {
    if let Some(suffix) = glob.strip_prefix("**/") {
        // Basename glob: the last path component(s) equal `suffix`. `scope == suffix`
        // covers a top-level file (`Cargo.toml`); the `/`-anchored ends_with prevents
        // `myCargo.toml` from matching `**/Cargo.toml`.
        scope == suffix || scope.ends_with(&format!("/{suffix}"))
    } else if let Some(prefix) = glob.strip_suffix("/**") {
        scope == prefix || scope.starts_with(&format!("{prefix}/"))
    } else {
        glob == scope
    }
}

/// Whether a gate glob is WELL-FORMED for [`glob_match`]: an exact scope (no `*`), a
/// single trailing `<prefix>/**` wildcard, or a single leading `**/<suffix>` basename
/// wildcard. For the two wildcard shapes the literal part must itself be a CANONICAL
/// scope ([`scope_is_canonical`]): the same `[A-Za-z0-9._-]`-segment shape a real
/// MATCHABLE scope has — ASCII only, non-empty, no `*`, no `.`/`..`/empty segment, no
/// leading/trailing/doubled `/`. Anything else — a `*` outside those two positions
/// (`exec:**`, `src/*`, `a/**/b`, `**/a/**`), a literal no canonical scope can contain
/// (`**/Cargo.toml/`, `**/src//x`, `a//b/**`, `**/..`, `**/配置.toml`), surrounding
/// whitespace, or empty — silently matches NOTHING, a sovereign footgun. The writers
/// reject it, and the projector treats it as malformed so it cannot disable a prior valid
/// rule (codex M11-R4-P2 / M12.1-P2: the wildcard literal must be a canonical scope, not
/// just slash-free). HONEST BOUNDARY: scopes are matched as raw `/`-separated ASCII
/// strings (the macOS/Linux Claude Code surface, §12.6); a Windows `\`-separated action
/// scope would not match a `/` glob — scope normalization is a follow-up when a `\`-path
/// harness is added (codex M12.1-P2-b). Pure; no IO.
pub fn glob_is_wellformed(glob: &str) -> bool {
    if glob.is_empty() || glob != glob.trim() {
        return false;
    }
    if let Some(suffix) = glob.strip_prefix("**/") {
        scope_is_canonical(suffix)
    } else if let Some(prefix) = glob.strip_suffix("/**") {
        scope_is_canonical(prefix)
    } else {
        !glob.contains('*')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_only() {
        assert!(glob_match("knowledge/secret/key", "knowledge/secret/key"));
        assert!(!glob_match("knowledge/secret/key", "knowledge/secret/key2"));
    }

    #[test]
    fn prefix_subtree_glob() {
        assert!(glob_match("src/**", "src/main.rs"));
        assert!(glob_match("src/**", "src")); // the prefix itself
        assert!(glob_match("src/**", "src/a/b/c.rs"));
        assert!(!glob_match("src/**", "srcfoo")); // not a `/`-anchored descendant
        assert!(!glob_match("src/**", "other/x"));
    }

    #[test]
    fn basename_suffix_glob() {
        assert!(glob_match("**/Cargo.toml", "Cargo.toml")); // top-level
        assert!(glob_match("**/Cargo.toml", "a/b/Cargo.toml")); // nested
        assert!(!glob_match("**/Cargo.toml", "myCargo.toml")); // not `/`-anchored
        assert!(!glob_match("**/Cargo.toml", "Cargo.toml.bak"));
    }

    #[test]
    fn wellformed_accepts_exact_and_two_wildcard_shapes() {
        assert!(glob_is_wellformed("knowledge/secret/key"));
        assert!(glob_is_wellformed("src/**"));
        assert!(glob_is_wellformed("**/Cargo.toml"));
        assert!(glob_is_wellformed("exec:bash")); // exact, ascii, no `*`
    }

    #[test]
    fn wellformed_rejects_footguns() {
        // `*` outside the two anchored positions.
        assert!(!glob_is_wellformed("exec:**")); // not `<prefix>/**` (no `/` before `**`)
        assert!(!glob_is_wellformed("src/*"));
        assert!(!glob_is_wellformed("a/**/b"));
        assert!(!glob_is_wellformed("**/a/**"));
        // wildcard literal must be a canonical scope.
        assert!(!glob_is_wellformed("**/Cargo.toml/"));
        assert!(!glob_is_wellformed("**/src//x"));
        assert!(!glob_is_wellformed("a//b/**"));
        assert!(!glob_is_wellformed("**/.."));
        assert!(!glob_is_wellformed("**/配置.toml")); // non-ASCII
        // surrounding whitespace / empty.
        assert!(!glob_is_wellformed(" src/**"));
        assert!(!glob_is_wellformed("src/** "));
        assert!(!glob_is_wellformed(""));
    }

    #[test]
    fn normalize_collapses_spelling_tricks_into_canonical() {
        // the codex P1 bypass: `./` and friends must collapse so the deny glob still bites.
        assert_eq!(normalize_path_scope("./knowledge/secret/key"), "knowledge/secret/key");
        assert_eq!(normalize_path_scope("knowledge//secret/key"), "knowledge/secret/key");
        assert_eq!(normalize_path_scope("knowledge/./secret/key"), "knowledge/secret/key");
        assert_eq!(normalize_path_scope("knowledge/secret/../secret/key"), "knowledge/secret/key");
        assert_eq!(normalize_path_scope("a/../knowledge/secret/key"), "knowledge/secret/key");
        assert_eq!(normalize_path_scope("knowledge/secret/key/"), "knowledge/secret/key");
        // a normalized spelling now matches the deny glob it tried to dodge.
        assert!(glob_match("knowledge/secret/**", &normalize_path_scope("./knowledge/secret/key")));
        assert!(!glob_match("knowledge/secret/**", "./knowledge/secret/key"), "the RAW spelling dodges (why we normalize)");
    }

    #[test]
    fn normalize_preserves_absolute_and_relative_escape() {
        // absolute paths are preserved (they won't match relative globs — unchanged boundary).
        assert_eq!(normalize_path_scope("/Users/x/knowledge/secret/key"), "/Users/x/knowledge/secret/key");
        assert_eq!(normalize_path_scope("/a/../../b"), "/b"); // `..` above root is dropped
        // a relative path that escapes upward keeps the `..` (matches no relative subtree glob).
        assert_eq!(normalize_path_scope("../../etc/passwd"), "../../etc/passwd");
        // degenerate -> cwd marker.
        assert_eq!(normalize_path_scope("a/.."), ".");
    }

    #[test]
    fn relativize_resolves_absolute_and_escape_into_in_tree_scope() {
        let root = "/Users/x/flint";
        // codex P1 r2: the normal Claude case — an ABSOLUTE path under the workspace.
        assert_eq!(relativize_to_workspace("/Users/x/flint/knowledge/secret/key", root), Some("knowledge/secret/key".into()));
        // a `..` that climbs out of and back into the tree still resolves in-tree.
        assert_eq!(relativize_to_workspace("../flint/knowledge/secret/key", root), Some("knowledge/secret/key".into()));
        // a plain relative path is taken relative to the workspace root.
        assert_eq!(relativize_to_workspace("knowledge/secret/key", root), Some("knowledge/secret/key".into()));
        // the resolved in-tree scope matches the deny glob it would otherwise dodge.
        let rel = relativize_to_workspace("/Users/x/flint/knowledge/secret/key", root).unwrap();
        assert!(glob_match("knowledge/secret/**", &rel));
    }

    #[test]
    fn relativize_returns_none_for_out_of_tree_paths() {
        let root = "/Users/x/flint";
        assert_eq!(relativize_to_workspace("/etc/passwd", root), None);
        assert_eq!(relativize_to_workspace("/Users/x/other/knowledge/secret/key", root), None);
        assert_eq!(relativize_to_workspace("../other/x", root), None);
        // the root itself maps to the cwd marker.
        assert_eq!(relativize_to_workspace("/Users/x/flint", root), Some(".".into()));
    }

    #[test]
    fn separators_normalize_before_the_lexical_matcher() {
        // M12.1-P2-b: on a `\`-path harness (Grok/Windows) `\` IS a separator, so it must
        // become `/` BEFORE normalization or the deny glob never bites.
        assert_eq!(normalize_separators(r"C:\ws\secrets\key"), "C:/ws/secrets/key");
        assert_eq!(normalize_separators("already/slashed"), "already/slashed");
        let scope = normalize_path_scope(&normalize_separators(r".\secrets\..\secrets\key"));
        assert_eq!(scope, "secrets/key");
        assert!(glob_match("secrets/**", &scope));
    }

    #[test]
    fn drive_absolute_relativizes_and_folds_only_the_drive_letter() {
        let root = "C:/ws";
        assert_eq!(
            relativize_to_workspace("C:/ws/secrets/key", root),
            Some("secrets/key".into())
        );
        // the drive LETTER is case-insensitive (Windows spells one drive both ways) …
        assert_eq!(
            relativize_to_workspace("c:/ws/secrets/key", root),
            Some("secrets/key".into())
        );
        assert_eq!(
            relativize_to_workspace("C:/ws/secrets/key", "c:/ws"),
            Some("secrets/key".into())
        );
        // … but the REST of the path is not folded (the gate globs are case-sensitive).
        assert_eq!(relativize_to_workspace("C:/WS/secrets/key", root), None);
        // a relative scope joins onto the drive root; a trailing-slash root round-trips.
        assert_eq!(relativize_to_workspace("secrets/key", "C:/ws/"), Some("secrets/key".into()));
        // the root itself, and a `\`-spelled root, both resolve.
        assert_eq!(relativize_to_workspace("C:/ws", root), Some(".".into()));
        assert_eq!(
            relativize_to_workspace("C:/ws/secrets/key", r"C:\ws\"),
            Some("secrets/key".into())
        );
        // out-of-tree (other subtree, other drive) is dropped.
        assert_eq!(relativize_to_workspace("C:/other/secrets/key", root), None);
        assert_eq!(relativize_to_workspace("D:/ws/secrets/key", root), None);
    }

    #[test]
    fn dotdot_never_climbs_past_a_drive_root() {
        // `C:/../ws/secrets/key` is `C:/ws/secrets/key` on the real filesystem; popping the
        // drive off would decay it into a RELATIVE path that the workspace join re-roots one
        // level too deep, and the `secrets/**` deny would silently stop firing.
        assert_eq!(normalize_path_scope("C:/../ws/secrets/key"), "C:/ws/secrets/key");
        assert_eq!(normalize_path_scope("C:/ws/../ws/secrets/key"), "C:/ws/secrets/key");
        assert_eq!(
            relativize_to_workspace("C:/../ws/secrets/key", "C:/ws"),
            Some("secrets/key".into())
        );
    }

    #[test]
    fn synthetic_scopes_are_recognized() {
        assert!(is_synthetic_scope("exec:bash"));
        assert!(is_synthetic_scope("tool:mcp__fs__read"));
        assert!(!is_synthetic_scope("knowledge/secret/key"));
    }

    #[test]
    fn scope_canonical_charset() {
        assert!(scope_is_canonical("knowledge/secret/key-1.v2"));
        assert!(!scope_is_canonical(""));
        assert!(!scope_is_canonical("a//b"));
        assert!(!scope_is_canonical("a/../b"));
        assert!(!scope_is_canonical("a/./b"));
        assert!(!scope_is_canonical("配置.toml"));
        assert!(!scope_is_canonical("a b"));
    }
}
