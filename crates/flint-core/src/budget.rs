//! Energy budget (PIVOT P3 · reframe-and-diff §5 P3 / heart §5 / energy gate).
//!
//! Energy (cumulative REAL tokens — never a TOPS constant, heart §5) is the one signal in
//! the flint loop that is naturally external, unforgeable, and immune to intelligence. P3
//! feeds it INTO the judgment gate (the landscape gap: token circuit-breakers exist, but
//! none feed budget into PER-ACTION judgment). A Stop/PostToolUse hook accumulates measured
//! tokens into a sidecar; the PreToolUse hook reads the running total into [`Action::energy`]
//! and folds it onto the verdict.
//!
//! VETO-ONLY (invariant ④, like `model_veto`): energy can only raise an otherwise-`Affirm`
//! action to a recoverable `Critique` ("you've spent a lot — verify or stop"). It NEVER
//! produces a `Deny` (budget is soft pressure, not a sovereign freeze), never upgrades or
//! relaxes an existing verdict. The worst a wrong threshold does is over-flag (recoverable).
//!
//! The accumulator is BEST-EFFORT (energy is a soft signal): a missing/unparseable sidecar
//! reads as 0 (no fold) — leniency here is safe because energy only ever TIGHTENS, and the
//! hard gates (scope/command rules) are unaffected.

use std::path::Path;

use crate::touchstone::Verdict;

/// Read the cumulative energy (tokens) from the sidecar. Best-effort: a missing or
/// unparseable file is `0` (energy unknown -> no tightening).
pub fn read_cumulative(path: &Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Add `tokens` to the sidecar's running total, returning the new total. Read-add-write
/// (best-effort, single-writer assumed — the Stop/PostToolUse hook is serial per session).
pub fn add(path: &Path, tokens: u64) -> std::io::Result<u64> {
    let total = read_cumulative(path).saturating_add(tokens);
    std::fs::write(path, total.to_string())?;
    Ok(total)
}

/// Reset the sidecar to 0 (a SessionStart hook resets per-session accumulation).
pub fn reset(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, "0")
}

/// Fold the energy signal onto a layer-1 verdict. VETO-ONLY: only a NON-BLOCKING layer-1
/// verdict (`Affirm` or `Warn`) with measured energy at/over a positive `threshold` becomes
/// a recoverable `Critique`; everything else is unchanged. Pure.
///
/// `Warn` is eligible for the same reason `Affirm` is — it does not stop the action. If it
/// were excluded, adding a matching warn rule would WEAKEN this gate: the identical
/// over-threshold action would block with no rule and proceed with one.
pub fn energy_fold(layer1: Verdict, energy: Option<u64>, threshold: Option<u64>) -> Verdict {
    match (&layer1, energy, threshold) {
        (Verdict::Affirm | Verdict::Warn { .. }, Some(e), Some(t)) if t > 0 && e >= t => Verdict::Critique {
            rule_id: "energy-budget".to_string(),
            scope: String::new(),
            message: format!(
                "energy budget pressure: {e} tokens spent (threshold {t}). Verify your load-bearing claims or stop before spending more."
            ),
            suggestion: "Re-check that recent work is externally verified; consider stopping or narrowing scope.".to_string(),
        },
        _ => layer1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn affirm() -> Verdict {
        Verdict::Affirm
    }
    fn critique() -> Verdict {
        Verdict::Critique { rule_id: "r".into(), scope: "s".into(), message: "m".into(), suggestion: "x".into() }
    }
    fn warn() -> Verdict {
        Verdict::Warn { rule_id: "r".into(), scope: "s".into(), message: "m".into(), suggestion: "x".into() }
    }
    fn deny() -> Verdict {
        Verdict::Deny { rule_id: "r".into(), scope: "s".into(), reason: "no".into() }
    }

    #[test]
    fn over_threshold_affirm_becomes_critique() {
        match energy_fold(affirm(), Some(120_000), Some(100_000)) {
            Verdict::Critique { rule_id, message, .. } => {
                assert_eq!(rule_id, "energy-budget");
                assert!(message.contains("120000"));
            }
            other => panic!("expected critique, got {other:?}"),
        }
    }

    #[test]
    fn under_threshold_unchanged() {
        assert_eq!(energy_fold(affirm(), Some(50_000), Some(100_000)), affirm());
    }

    #[test]
    fn no_energy_or_no_threshold_unchanged() {
        assert_eq!(energy_fold(affirm(), None, Some(100_000)), affirm());
        assert_eq!(energy_fold(affirm(), Some(999_999), None), affirm());
        assert_eq!(energy_fold(affirm(), Some(999_999), Some(0)), affirm()); // zero threshold = disabled
    }

    #[test]
    fn over_threshold_warn_also_escalates() {
        // A warn does NOT block, so it must be eligible for energy escalation exactly like
        // an affirm. Otherwise adding a matching warn rule WEAKENS the energy gate: the same
        // over-threshold action blocks with no rule and proceeds with a warn rule (codex P1).
        match energy_fold(warn(), Some(120_000), Some(100_000)) {
            Verdict::Critique { rule_id, .. } => assert_eq!(rule_id, "energy-budget"),
            other => panic!("expected critique, got {other:?}"),
        }
        // under threshold the warn survives untouched.
        assert_eq!(energy_fold(warn(), Some(50_000), Some(100_000)), warn());
    }

    #[test]
    fn never_downgrades_or_relaxes_a_block() {
        // a critique/deny is never touched (energy is veto-only, Affirm->Critique max).
        assert_eq!(energy_fold(critique(), Some(999_999), Some(1)), critique());
        assert_eq!(energy_fold(deny(), Some(999_999), Some(1)), deny());
        // energy can never produce an Affirm.
        assert!(!energy_fold(deny(), None, None).is_affirm());
    }

    #[test]
    fn accumulator_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("budget");
        assert_eq!(read_cumulative(&p), 0); // absent -> 0
        assert_eq!(add(&p, 100).unwrap(), 100);
        assert_eq!(add(&p, 50).unwrap(), 150);
        assert_eq!(read_cumulative(&p), 150);
        reset(&p).unwrap();
        assert_eq!(read_cumulative(&p), 0);
    }

    #[test]
    fn unparseable_sidecar_reads_zero() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("budget");
        std::fs::write(&p, "garbage").unwrap();
        assert_eq!(read_cumulative(&p), 0);
    }
}
