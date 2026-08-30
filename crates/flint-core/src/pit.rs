//! Pit — the knowledge carrier (Plan 3). A pit is a NOTE, not a rule.
//!
//! Whitepaper boundary: flint's four actions are about RULES (write / carry / enforce /
//! revise). A pit is the KNOWLEDGE those rules grow from — a wall you hit, sedimented so
//! next time is cheaper. A pit is:
//!   - **never signed** (pits live under `pits/`, which `canon pick` does not sweep),
//!   - **never judged** (not a [`crate::touchstone::GateRule`]),
//!   - **never injected** into agent context (not an [`crate::touchstone::AdvisoryRule`]).
//!
//! Capture is TWO deliberate steps, both owner-driven — NOT the frozen Forge's automatic
//! mining:
//!   - **mark** (hot): the agent, the moment it hits a wall, appends a one-line gist to
//!     the inbox. Non-interrupting, written from what's in front of it (no transcript
//!     scrape).
//!   - **save** (cold): the owner expands a gist into a full pit note and files it. The
//!     owner decides what is worth keeping. A pit is promoted to an *enforced* rule only
//!     by the owner WRITING a rule — there is no automatic pit→rule classifier (that
//!     would be the frozen evidence-mining reflex).
//!
//! LENIENT parsing (the deliberate contrast with [`crate::canon::parse_rule`]): a rule is
//! load-bearing, so a malformed rule fails the whole Canon closed. A pit is not
//! load-bearing, so a malformed pit is just a bad note — [`parse_pit`] is best-effort and
//! NEVER errors. It reuses canon's fence splitter + value helpers (no second hand-rolled
//! parser) but drops the fail-closed strictness (unknown keys ignored, no frontmatter is
//! fine).

/// A parsed pit note. Thin — a pit carries knowledge (the `body`), an `id`, and a
/// one-line `description` for listing. No verdict, no matcher, no evidence grade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pit {
    pub id: String,
    pub description: String,
    pub tags: Vec<String>,
    pub body: String,
}

/// Lenient parse of a pit markdown file. `stem` is the filename without extension (the
/// id fallback). Extracts `id` / `description` / `tags` from frontmatter IF present;
/// with no (or broken) frontmatter the whole text is the body. NEVER errors — a bad note
/// is still a note.
pub fn parse_pit(stem: &str, text: &str) -> Pit {
    // Reuse canon's exact fence splitter; a missing/unterminated fence (its `Err`) just
    // means "no frontmatter" for a pit — the whole text is the body.
    let (fm, body) = match crate::canon::split_frontmatter("<pit>", text) {
        Ok((fm, body)) => (fm, body),
        Err(_) => (String::new(), text.to_string()),
    };
    let mut id = stem.to_string();
    let mut description = String::new();
    let mut tags = Vec::new();
    for raw in fm.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue; // lenient: a non key:value line is ignored, not an error
        };
        let val = crate::canon::strip_quotes(v.trim());
        match k.trim() {
            "id" if !val.is_empty() => id = val,
            "description" => description = val,
            "tags" => tags = crate::canon::split_csv(&val),
            _ => {} // unknown / gate fields ignored — a pit is not fail-closed
        }
    }
    Pit { id, description, body: body.trim().to_string(), tags }
}

/// Format one hot-mark gist as a single inbox line. Collapses internal whitespace so a
/// gist is always exactly one `- …` bullet (a mark is a single line, never multi-line).
pub fn format_inbox_gist(gist: &str) -> String {
    let one = gist.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("- {one}\n")
}

/// Count the pending gist bullets in an inbox file body (non-empty `- …` lines).
pub fn count_inbox(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- ") && t.trim().len() > 2
        })
        .count()
}

/// Whether `id` is a safe pit filename slug: non-empty, no path separators / traversal /
/// whitespace; ASCII alnum + `-`/`_`. Keeps `save` from writing outside `pits/`.
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains("..")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Derive a SUGGESTED safe slug from a rejected id, so the `save` error can say
/// "try `this`" instead of only "no". Never applied automatically — the owner re-runs
/// with the suggestion if it's right (rejecting stays fail-closed; suggesting is UX).
/// Takes the last path component of either separator flavor (`/` and `\` — a Windows
/// path pasted on any OS still yields its basename), drops a `.md` extension, lowercases,
/// maps every non-`[a-z0-9_]` run to a single `-`, and trims `-`. `None` when nothing
/// safe survives (e.g. all-CJK input) or the input was already safe.
pub fn suggest_id(raw: &str) -> Option<String> {
    if is_safe_id(raw) {
        return None;
    }
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_end_matches(".md");
    let mut slug = String::with_capacity(base.len());
    let mut pending_dash = false;
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    (!slug.is_empty() && is_safe_id(&slug)).then_some(slug)
}

/// Scaffold a cold-store pit draft file. Frontmatter is the self-describing `type:pit`
/// carrier (it lives under `pits/`, so it never reaches `canon::reduce` — self-describing
/// only). The owner expands the body; `seed` (an inbox gist) primes it.
pub fn scaffold(id: &str, description: Option<&str>, seed: Option<&str>) -> String {
    let desc = description.unwrap_or("").trim();
    let body = seed.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(
        "<!-- The pit: what you hit, why it bit, what you'll do next time. -->",
    );
    format!(
        "---\nschema: flint/v1\nid: {id}\ntype: pit\nkind: pit\ndescription: {desc}\nsource.kind: observation\ntags:\n---\n{body}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pit_with_frontmatter() {
        let t = "---\nschema: flint/v1\nid: real-id\ntype: pit\nkind: pit\ndescription: a wall\ntags: rust, ci\n---\nthe body\nmore\n";
        let p = parse_pit("filename-stem", t);
        assert_eq!(p.id, "real-id"); // frontmatter id wins over stem
        assert_eq!(p.description, "a wall");
        assert_eq!(p.tags, vec!["rust", "ci"]);
        assert_eq!(p.body, "the body\nmore");
    }

    #[test]
    fn parse_pit_no_frontmatter_is_lenient() {
        // No frontmatter → whole text is the body, id falls back to the stem. NO error.
        let p = parse_pit("my-note", "just a raw note, no fences\n");
        assert_eq!(p.id, "my-note");
        assert_eq!(p.description, "");
        assert!(p.tags.is_empty());
        assert_eq!(p.body, "just a raw note, no fences");
    }

    #[test]
    fn parse_pit_ignores_unknown_and_broken_lines() {
        // Unknown keys, a bare line, and a gate-only field are all ignored (lenient).
        let t = "---\nid: x\nresponse: block\nnot-a-kv-line\nrandom: stuff\n---\nbody\n";
        let p = parse_pit("stem", t);
        assert_eq!(p.id, "x");
        assert_eq!(p.body, "body");
    }

    #[test]
    fn parse_pit_empty_id_falls_back_to_stem() {
        let t = "---\nid:\ndescription: d\n---\nb\n";
        let p = parse_pit("stem-fallback", t);
        assert_eq!(p.id, "stem-fallback");
    }

    #[test]
    fn format_inbox_gist_is_one_line() {
        assert_eq!(format_inbox_gist("hit  a\nwall today"), "- hit a wall today\n");
        assert_eq!(format_inbox_gist("  trimmed  "), "- trimmed\n");
    }

    #[test]
    fn count_inbox_counts_only_bullets() {
        let inbox = "# Pit inbox\n\n- first gist\n- second gist\nnot a bullet\n- \n";
        assert_eq!(count_inbox(inbox), 2); // the empty `- ` bullet doesn't count
    }

    #[test]
    fn is_safe_id_rejects_traversal_and_seps() {
        assert!(is_safe_id("good-id_1"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("../etc/passwd"));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id("has space"));
        assert!(!is_safe_id("dot.in.id"));
        assert!(!is_safe_id(r"a\b")); // Windows separator is not a valid slug char either
    }

    #[test]
    fn suggest_id_derives_slug_from_pathish_input() {
        // the Windows-path confusion: a path (either separator flavor) is not an id —
        // the suggestion is its slugified basename, never applied automatically.
        assert_eq!(suggest_id("pits/my pit.md").as_deref(), Some("my-pit"));
        assert_eq!(suggest_id(r"D:\notes\R2 Key Expiry.md").as_deref(), Some("r2-key-expiry"));
        assert_eq!(suggest_id("dot.in.id").as_deref(), Some("dot-in-id"));
        assert_eq!(suggest_id("Has Space").as_deref(), Some("has-space"));
        assert_eq!(suggest_id("../etc/passwd").as_deref(), Some("passwd"));
        // already-safe input gets no suggestion; hopeless input gets None, not garbage.
        assert_eq!(suggest_id("good-id_1"), None);
        assert_eq!(suggest_id("配置"), None);
        assert_eq!(suggest_id(""), None);
    }

    #[test]
    fn scaffold_roundtrips_through_parse_pit() {
        let md = scaffold("my-pit", Some("a short desc"), Some("the seed gist"));
        let p = parse_pit("my-pit", &md);
        assert_eq!(p.id, "my-pit");
        assert_eq!(p.description, "a short desc");
        assert_eq!(p.body, "the seed gist");
    }

    #[test]
    fn scaffold_without_seed_has_placeholder() {
        let md = scaffold("p", None, None);
        assert!(md.contains("<!-- The pit:"));
        assert!(md.contains("type: pit"));
        assert!(md.contains("kind: pit"));
    }
}
