//! Knowledge — the promoted-ore store (精矿, knowledge-layer spec skeleton §4/§7).
//!
//! The refinery OUTPUT. A raw gist lands in an ore store (粗矿); once the OWNER promotes it
//! (人裁 — never auto, never a classifier, spec §3 red line), it becomes a durable note here.
//! This store is the flint-side, take-away 精矿 (bare markdown in git; `flint rm` → the notes
//! survive, the north star), DISTINCT from the raw-ore stores where gists are captured.
//!
//! NEUTRAL FORMAT ONLY (§4): title + body + a source/date footer — NO casefile bucket schema,
//! no flint-proprietary frontmatter. Any tool reads it; nothing here is signed / judged /
//! injected. The promote WRITE reuses [`flint_core::memory::FsVault::write_durable`] (same
//! symlink-safe, refuse-clobber discipline); this module owns the FORMAT + the review READ.

use flint_core::config::ResolvedOreStore;
use flint_core::memory::{FsVault, MemoryPort};

/// Format a promoted knowledge note: a `# title` heading, the body, and a provenance footer
/// (where the gist came from + the ISO date it was promoted). Deliberately bare markdown — not
/// a flint schema — so the note keeps its worth if flint is ever removed (§6 north star). The
/// caller passes the date (flint never fabricates one; see [`iso_date`]).
pub fn format_note(title: &str, body: &str, source: &str, date: &str) -> String {
    let title = title.trim();
    let body = body.trim();
    let source = source.trim();
    format!("# {title}\n\n{body}\n\n---\nsource: {source} · promoted {date}\n")
}

/// Epoch seconds (UTC) → `YYYY-MM-DD`, dependency-free (Hinnant's civil-from-days). flint does
/// not pull a date crate for one stamp; the caller passes `SystemTime::now()` seconds so the
/// note records WHEN it was promoted. Valid for the positive (post-1970) range flint stamps.
pub fn iso_date(epoch_secs: i64) -> String {
    let days = epoch_secs.div_euclid(86_400);
    // civil_from_days (Howard Hinnant): shift the era to start on 0000-03-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // day-of-era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // year-of-era [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day-of-year (Mar-based) [0, 365]
    let mp = (5 * doy + 2) / 153; // month-prime [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// The pending inbox gists of ONE ore store (spec §5 review). `gists` is `Err` when that store
/// could not be read — surfaced, never swallowed, so a broken external vault does not silently
/// hide itself while the rest of the union still lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePending {
    pub label: String,
    /// Whether this is the active write target (where the next auto-capture lands).
    pub active: bool,
    pub gists: Result<Vec<String>, String>,
}

/// The UNION of pending inbox gists across every ore store (spec §5 read set) — what the owner
/// reviews before promoting. Read-only: it mutates nothing. Order follows the configured store
/// order (active first in the common single-store / legacy shape). A missing inbox is an empty
/// list, not an error (parity with [`flint_core::memory::FsVault::list_inbox`]).
pub fn pending_across(stores: &[ResolvedOreStore]) -> Vec<StorePending> {
    stores
        .iter()
        .map(|s| StorePending {
            label: s.label.clone(),
            active: s.active,
            gists: FsVault::new(s.path.clone()).list_inbox().map_err(|e| e.to_string()),
        })
        .collect()
}

/// Total pending gists across all readable stores (broken stores contribute 0). A cheap count
/// for the review header / a "nothing to triage" short-circuit.
pub fn total_pending(pending: &[StorePending]) -> usize {
    pending.iter().filter_map(|p| p.gists.as_ref().ok()).map(Vec::len).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn format_note_neutral_template() {
        let n = format_note("Epoch reset on dirty tree", "The pick auto-epoch probes None…", "pits #3", "2026-07-15");
        assert_eq!(
            n,
            "# Epoch reset on dirty tree\n\nThe pick auto-epoch probes None…\n\n---\nsource: pits #3 · promoted 2026-07-15\n"
        );
    }

    #[test]
    fn format_note_trims_inputs() {
        let n = format_note("  T  ", "\n body \n", "  src ", "2026-01-02");
        assert_eq!(n, "# T\n\nbody\n\n---\nsource: src · promoted 2026-01-02\n");
    }

    #[test]
    fn iso_date_known_values() {
        assert_eq!(iso_date(0), "1970-01-01"); // the epoch
        assert_eq!(iso_date(1_735_689_600), "2025-01-01"); // 2025-01-01T00:00:00Z
        assert_eq!(iso_date(1_784_053_489), "2026-07-14"); // a real receipt ts; UTC (was 02:24 +0800 → prev UTC day)
        assert_eq!(iso_date(951_782_400), "2000-02-29"); // leap day (400-year rule)
    }

    fn store_with(dir: &std::path::Path, label: &str, active: bool, gists: &[&str]) -> ResolvedOreStore {
        let v = FsVault::new(dir);
        for g in gists {
            v.capture(g).unwrap();
        }
        ResolvedOreStore { path: dir.to_path_buf(), label: label.to_string(), active }
    }

    #[test]
    fn pending_across_unions_in_store_order() {
        let d0 = tempfile::tempdir().unwrap();
        let d1 = tempfile::tempdir().unwrap();
        let stores = vec![
            store_with(d0.path(), "pits", true, &["wall one", "wall two"]),
            store_with(d1.path(), "obsidian", false, &["a note"]),
        ];
        let pending = pending_across(&stores);
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].label, "pits");
        assert!(pending[0].active);
        assert_eq!(pending[0].gists.as_ref().unwrap(), &vec!["wall one".to_string(), "wall two".to_string()]);
        assert_eq!(pending[1].label, "obsidian");
        assert!(!pending[1].active);
        assert_eq!(pending[1].gists.as_ref().unwrap(), &vec!["a note".to_string()]);
        assert_eq!(total_pending(&pending), 3);
    }

    #[test]
    fn pending_across_missing_inbox_is_empty_not_error() {
        // A configured store whose dir has no inbox yet → empty, not an error.
        let stores = vec![ResolvedOreStore {
            path: PathBuf::from("/nonexistent/flint/ore/store"),
            label: "ghost".into(),
            active: true,
        }];
        let pending = pending_across(&stores);
        assert_eq!(pending[0].gists.as_ref().unwrap().len(), 0);
        assert_eq!(total_pending(&pending), 0);
    }

    #[test]
    fn promote_write_roundtrips_via_fsvault() {
        // The promote WRITE path: a neutral note written into the knowledge store, then read
        // back verbatim (the store is bare markdown; format_note is the whole schema).
        let dir = tempfile::tempdir().unwrap();
        let store = FsVault::new(dir.path());
        let note = format_note("dirty-tree-epoch", "body of the lesson", "pits #1", "2026-07-15");
        let path = store.write_durable("dirty-tree-epoch", &note).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), note);
    }
}
