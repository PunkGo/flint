//! Forge — the load-bearing gate (PIVOT write-path · reframe-and-diff §3.7 + heart §6).
//!
//! 入库 ≠ 承重 (in-Canon != load-bearing): a SIGNED rule is acknowledged by the sovereign,
//! but its EVIDENCE TIER — how much weight it bears — is DERIVED, never self-reported
//! (codex r1 / pit self-reported-flag-is-not-a-gate). The Forge gate is the differentiator
//! the validated landscape found missing: an EXECUTABLE external-evidence gate applied to
//! a portable PROSE rule.
//!
//! For a gate rule, the "executable falsifier" is its DISCRIMINATION FIXTURE: known-bad
//! inputs the matcher SHOULD fire on + known-good inputs it should NOT. A rule earns the
//! `reproduced` tier iff its matcher discriminates on the sovereign fixture — a PURE,
//! deterministic, RE-DERIVABLE check (anyone re-runs the matcher over the fixture and gets
//! the same verdict; the verifier-ceiling §7 is exactly "how many prose rules admit such a
//! fixture" — measured: ~3/8 iron laws, the syntactic ones; judgment/trajectory laws stay
//! prose). This relocates the kernel's discrimination-fixture (F10) + replay discipline
//! from ledger-replay to "re-run the matcher from a signed promotion record".
//!
//! L2 (cross-vendor model veto) is consulted veto-only (NoopVeto default, §8.4): it can
//! only WITHHOLD a promotion, never manufacture one (invariant ④ at the write path).

use crate::model_veto::{ModelVeto, ModelVerdict, NoopVeto};
use crate::touchstone::{judge, Action, GateRule, Matcher, TouchstonePolicy};
use crate::trust::sha256_hex;

/// A sovereign discrimination fixture for a rule: inputs the matcher SHOULD fire on
/// (`bad`) and inputs it should NOT (`good`). For a Command rule the cases are command
/// strings; for a Scope rule they are target paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    pub rule_id: String,
    pub bad: Vec<String>,
    pub good: Vec<String>,
}

/// The outcome of a promotion attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Promotion {
    /// The rule discriminates: it earns `reproduced`. Carries the record to sign into Canon.
    Reproduced(PromotionRecord),
    /// The rule did NOT discriminate — it stays prose (NOT load-bearing at a high tier).
    Rejected { rule_id: String, reason: String },
}

/// A re-derivable promotion record (signed into `canon/promotion/<rule_id>.md` via pick, so
/// it inherits the §3.5 root of trust). Anyone re-runs the matcher over the fixture (whose
/// hash is recorded) to re-derive the tier — the evidence is the re-run, not this record's
/// say-so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionRecord {
    pub rule_id: String,
    pub fixture_sha256: String,
    pub bad_cases: usize,
    pub good_cases: usize,
}

impl PromotionRecord {
    /// Render the signable markdown promotion record.
    pub fn to_markdown(&self) -> String {
        format!(
            "---\nschema: flint-promotion-v1\nrule_id: {}\ntier: reproduced\nfixture_sha256: {}\nbad_cases: {}\ngood_cases: {}\n---\nPromotion: `{}` discriminates on its sovereign fixture ({} known-bad fire, {} known-good do not). Re-derive by re-running the matcher over the fixture.\n",
            self.rule_id, self.fixture_sha256, self.bad_cases, self.good_cases, self.rule_id, self.bad_cases, self.good_cases,
        )
    }
}

/// Build the neutral [`Action`] a fixture case represents, given the rule's matcher kind.
fn case_action(matcher: &Matcher, case: &str) -> Action {
    match matcher {
        Matcher::Command { .. } => Action {
            tool_kind: "exec".into(),
            scopes: vec!["exec:bash".into()],
            command: Some(case.to_string()),
            ..Default::default()
        },
        Matcher::Path { .. } => Action {
            tool_kind: "write".into(),
            scopes: vec![case.to_string()],
            ..Default::default()
        },
    }
}

/// Run the load-bearing gate for `rule` against `fixture` (and a fixture-content hash for
/// the re-derivable record). The matcher must FIRE on every `bad` case and NOT fire on
/// every `good` case (discriminating power, anti-self-certify F10). A fixture with no bad
/// or no good case cannot prove discrimination -> Rejected. Then L2 `veto` is consulted
/// veto-only (a veto WITHHOLDS the promotion; it can never create one).
pub fn promote(rule: &GateRule, fixture: &Fixture, fixture_bytes: &[u8], veto: &dyn ModelVeto) -> Promotion {
    if fixture.rule_id != rule.id {
        return Promotion::Rejected { rule_id: rule.id.clone(), reason: "fixture rule_id != rule id".into() };
    }
    if fixture.bad.is_empty() || fixture.good.is_empty() {
        return Promotion::Rejected {
            rule_id: rule.id.clone(),
            reason: "fixture needs >=1 bad and >=1 good case to prove discrimination".into(),
        };
    }
    let policy = TouchstonePolicy { rules: vec![rule.clone()], advisory: Vec::new() };

    // Every known-bad input MUST fire the rule (a non-Affirm verdict).
    for case in &fixture.bad {
        if judge(&policy, &case_action(&rule.matcher, case)).is_affirm() {
            return Promotion::Rejected {
                rule_id: rule.id.clone(),
                reason: format!("known-bad case did not fire the rule: {case:?}"),
            };
        }
    }
    // Every known-good input must NOT fire (Affirm) — else the rule is over-broad.
    for case in &fixture.good {
        if !judge(&policy, &case_action(&rule.matcher, case)).is_affirm() {
            return Promotion::Rejected {
                rule_id: rule.id.clone(),
                reason: format!("known-good case wrongly fired the rule: {case:?}"),
            };
        }
    }

    // L2 veto-only (NoopVeto default): a cross-vendor model may WITHHOLD, never create.
    // We probe it on a representative bad case; an Abstain (the default) lets promotion stand.
    let probe = case_action(&rule.matcher, &fixture.bad[0]);
    if let ModelVerdict::Veto { reason } = veto.consult(&probe) {
        return Promotion::Rejected { rule_id: rule.id.clone(), reason: format!("layer-2 veto: {reason}") };
    }

    Promotion::Reproduced(PromotionRecord {
        rule_id: rule.id.clone(),
        fixture_sha256: sha256_hex(fixture_bytes),
        bad_cases: fixture.bad.len(),
        good_cases: fixture.good.len(),
    })
}

/// Convenience: promote with the default (no) layer-2 veto.
pub fn promote_default(rule: &GateRule, fixture: &Fixture, fixture_bytes: &[u8]) -> Promotion {
    promote(rule, fixture, fixture_bytes, &NoopVeto)
}

/// Parse a fixture markdown file: `---\nrule_id: <id>\n---` frontmatter, then `## bad` and
/// `## good` sections, one case per non-empty line. Strict-ish; fail-closed to `None` on a
/// missing rule_id or no sections.
pub fn parse_fixture(text: &str) -> Option<Fixture> {
    let mut rule_id: Option<String> = None;
    let mut bad = Vec::new();
    let mut good = Vec::new();
    #[derive(PartialEq)]
    enum Sect {
        None,
        Fm,
        Bad,
        Good,
    }
    let mut sect = Sect::None;
    let mut fm_open = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "---" {
            if !fm_open {
                fm_open = true;
                sect = Sect::Fm;
            } else {
                sect = Sect::None;
            }
            continue;
        }
        match sect {
            Sect::Fm => {
                if let Some(v) = t.strip_prefix("rule_id:") {
                    rule_id = Some(v.trim().to_string());
                }
            }
            _ => {
                if t.eq_ignore_ascii_case("## bad") {
                    sect = Sect::Bad;
                } else if t.eq_ignore_ascii_case("## good") {
                    sect = Sect::Good;
                } else if !t.is_empty() && !t.starts_with('#') {
                    match sect {
                        Sect::Bad => bad.push(t.to_string()),
                        Sect::Good => good.push(t.to_string()),
                        _ => {}
                    }
                }
            }
        }
    }
    let rule_id = rule_id?;
    if rule_id.is_empty() {
        return None;
    }
    Some(Fixture { rule_id, bad, good })
}

/// Derive a rule's load-bearing evidence tier — by RE-RUNNING the gate, not by trusting a
/// record's say-so (codex P2 / §3.7 "re-derive by re-running the matcher"; pit
/// self-reported-flag-is-not-a-gate). A rule is `reproduced` iff BOTH:
///   1. a signed `promotion/<rule_id>.md` record is present (the sovereign PICKED this rule
///      for promotion — the record's content is just acknowledgment, not evidence), AND
///   2. re-running [`promote`] on the SIGNED rule + SIGNED fixture NOW yields `Reproduced`
///      (the live evidence — a stale/synthetic record cannot assert a tier the rule no
///      longer earns; a rule edited to no-longer-discriminate falls back to `prose`).
///
/// Otherwise `prose` — acknowledged-but-not-high-tier. Inputs come from the SIGNED
/// load-bearing set (the caller passes trust-verified bytes), never the working tree.
///
/// NOTE (mock, §8.4): the tier is DERIVED + SURFACED (`canon list`); whether `prose`
/// downgrades a rule from a hard gate to advisory-only is the enforcement-semantics decision
/// deferred to the owner (P4). Enforcement currently stays "signed = enforced"; this is the seam.
pub fn effective_tier(rule: &GateRule, promotion_present: bool, fixture_bytes: Option<&[u8]>) -> crate::canon::EvidenceTier {
    use crate::canon::EvidenceTier;
    if !promotion_present {
        return EvidenceTier::Prose;
    }
    let Some(bytes) = fixture_bytes else { return EvidenceTier::Prose };
    let Ok(text) = std::str::from_utf8(bytes) else { return EvidenceTier::Prose };
    let Some(fixture) = parse_fixture(text) else { return EvidenceTier::Prose };
    if fixture.rule_id != rule.id {
        return EvidenceTier::Prose;
    }
    match promote_default(rule, &fixture, bytes) {
        Promotion::Reproduced(_) => EvidenceTier::Reproduced,
        Promotion::Rejected { .. } => EvidenceTier::Prose,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canon::EvidenceTier;
    use crate::model_veto::ModelVerdict;
    use crate::touchstone::{Response, Reversibility};

    fn cmd_rule(pattern: &str) -> GateRule {
        GateRule {
            id: "lsp-over-grep".into(),
            matcher: Matcher::Command { pattern: pattern.into(), exempt: None },
            response: Response::Critique,
            reversibility: Reversibility::Reversible,
            message: "use lsp".into(),
            suggestion: String::new(),
        }
    }
    fn scope_rule(glob: &str) -> GateRule {
        GateRule {
            id: "no-secrets".into(),
            matcher: Matcher::Path { glob: glob.into() },
            response: Response::Deny,
            reversibility: Reversibility::Irreversible,
            message: "no".into(),
            suggestion: String::new(),
        }
    }

    #[test]
    fn discriminating_command_rule_is_reproduced() {
        let rule = cmd_rule(r"\bgrep\b.*\.(rs|ts)\b");
        let fx = Fixture {
            rule_id: "lsp-over-grep".into(),
            bad: vec!["grep -rn foo src/main.rs".into(), "grep bar lib.ts".into()],
            good: vec!["grep -rn TODO README.md".into(), "cat src/main.rs".into()],
        };
        match promote_default(&rule, &fx, b"fixturebytes") {
            Promotion::Reproduced(rec) => {
                assert_eq!(rec.rule_id, "lsp-over-grep");
                assert_eq!(rec.bad_cases, 2);
                assert_eq!(rec.good_cases, 2);
                assert_eq!(rec.fixture_sha256, sha256_hex(b"fixturebytes"));
            }
            other => panic!("expected reproduced, got {other:?}"),
        }
    }

    #[test]
    fn over_broad_rule_is_rejected() {
        // a pattern that also fires on a known-good case (grep on a non-source file).
        let rule = cmd_rule(r"\bgrep\b"); // too broad: fires on README too
        let fx = Fixture {
            rule_id: "lsp-over-grep".into(),
            bad: vec!["grep foo src/main.rs".into()],
            good: vec!["grep TODO README.md".into()], // this wrongly fires -> reject
        };
        assert!(matches!(promote_default(&rule, &fx, b"x"), Promotion::Rejected { .. }));
    }

    #[test]
    fn rule_that_misses_a_bad_case_is_rejected() {
        let rule = cmd_rule(r"\bgrep\b.*\.rs\b");
        let fx = Fixture {
            rule_id: "lsp-over-grep".into(),
            bad: vec!["grep foo y.ts".into()], // rule only catches .rs, misses this .ts bad case
            good: vec!["cat x".into()],
        };
        assert!(matches!(promote_default(&rule, &fx, b"x"), Promotion::Rejected { .. }));
    }

    #[test]
    fn scope_rule_discriminates() {
        let rule = scope_rule("knowledge/secret/**");
        let fx = Fixture {
            rule_id: "no-secrets".into(),
            bad: vec!["knowledge/secret/key".into(), "knowledge/secret/a/b".into()],
            good: vec!["knowledge/public/note".into()],
        };
        assert!(matches!(promote_default(&rule, &fx, b"x"), Promotion::Reproduced(_)));
    }

    #[test]
    fn empty_fixture_side_rejected() {
        let rule = cmd_rule(r"grep");
        let fx = Fixture { rule_id: "lsp-over-grep".into(), bad: vec!["grep x".into()], good: vec![] };
        assert!(matches!(promote_default(&rule, &fx, b"x"), Promotion::Rejected { .. }));
    }

    #[test]
    fn layer2_veto_withholds_promotion() {
        struct AlwaysVeto;
        impl ModelVeto for AlwaysVeto {
            fn consult(&self, _a: &Action) -> ModelVerdict {
                ModelVerdict::Veto { reason: "drift".into() }
            }
        }
        let rule = cmd_rule(r"\bgrep\b.*\.rs\b");
        let fx = Fixture {
            rule_id: "lsp-over-grep".into(),
            bad: vec!["grep x a.rs".into()],
            good: vec!["cat a.rs".into()],
        };
        // discriminates, but L2 vetoes -> rejected (veto-only can withhold, never create).
        assert!(matches!(promote(&rule, &fx, b"x", &AlwaysVeto), Promotion::Rejected { .. }));
    }

    #[test]
    fn parse_fixture_roundtrip() {
        let text = "---\nrule_id: lsp-over-grep\n---\n## bad\ngrep foo src/x.rs\nrg bar y.ts\n## good\ngrep TODO README.md\n";
        let fx = parse_fixture(text).expect("parses");
        assert_eq!(fx.rule_id, "lsp-over-grep");
        assert_eq!(fx.bad, vec!["grep foo src/x.rs".to_string(), "rg bar y.ts".to_string()]);
        assert_eq!(fx.good, vec!["grep TODO README.md".to_string()]);
    }

    #[test]
    fn effective_tier_re_runs_the_matcher() {
        // codex P2 / §3.7: tier is RE-DERIVED by re-running the gate, not trusting a record.
        let rule = cmd_rule(r"\bgrep\b.*\.rs\b");
        let fixture_bytes = b"---\nrule_id: lsp-over-grep\n---\n## bad\ngrep x a.rs\n## good\ncat a.rs\n";
        // signed promotion present + the rule STILL discriminates on the signed fixture -> reproduced.
        assert_eq!(effective_tier(&rule, true, Some(fixture_bytes)), EvidenceTier::Reproduced);
        // no signed promotion record (sovereign didn't pick it) -> prose.
        assert_eq!(effective_tier(&rule, false, Some(fixture_bytes)), EvidenceTier::Prose);
        // promotion record present BUT the rule no longer discriminates (a `.ts` bad case the
        // .rs-only pattern misses) -> prose. A stale/synthetic record CANNOT assert reproduced.
        let stale_fixture = b"---\nrule_id: lsp-over-grep\n---\n## bad\ngrep x a.ts\n## good\ncat a.rs\n";
        assert_eq!(effective_tier(&rule, true, Some(stale_fixture)), EvidenceTier::Prose);
        // fixture rule_id mismatch -> prose.
        let wrong_id = b"---\nrule_id: other\n---\n## bad\ngrep x a.rs\n## good\ncat a.rs\n";
        assert_eq!(effective_tier(&rule, true, Some(wrong_id)), EvidenceTier::Prose);
        // no fixture -> prose.
        assert_eq!(effective_tier(&rule, true, None), EvidenceTier::Prose);
    }
}
