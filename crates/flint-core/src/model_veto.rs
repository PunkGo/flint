//! Layer-2 cross-vendor model veto (M11.2) — "the model as a polygraph, not a judge".
//!
//! Layer-1 is the deterministic [`crate::touchstone`] gate (sovereign rules). Layer-2
//! is an OPTIONAL second pass: a cross-vendor model (e.g. codex judging a Claude
//! agent) consulted on an action. The heart's rule is that this model is role A (a
//! CALIBRATED POLYGRAPH) and NEVER role B (a final LLM-judge). Four defenses keep it
//! safe:
//!   1. cross-vendor non-collusion — a different vendor's model judges this agent;
//!   2. fixture-anchored — it shows discriminating power on sovereign-frozen
//!      known-good/known-bad, it does not set its own standard;
//!   3. VETO-ONLY — this trait has no `Authorize`/`Affirm` variant by construction;
//!   4. honestly graded as the WEAKEST evidence — it drifts by model version.
//!
//! INVARIANT ④ at layer-2: the model can only RAISE a recoverable critique on an
//! otherwise-affirmed action. It can NEVER (a) produce a sovereign-freeze `Deny`
//! (a freeze is sovereign-only), (b) UPGRADE or DOWNGRADE an existing layer-1
//! verdict, or (c) authorize anything. The worst a drifted/hacked model can do is
//! over-flag (recoverable — the agent self-corrects) or abstain (miss a flag) —
//! never freeze, never authorize. Being hacked degrades the consequence from
//! "wrongly load-bearing" to "missed one veto" (heart §6).
//!
//! Phase 0.5: the ACTUAL model call is MOCKED. Which model, transport, and when to
//! consult are the owner's call (build plan §8.4 #5). `flint-core` defines only the
//! contract + the veto-only fold + a Noop/Mock impl, so the plug-in is provably
//! sound before any real cross-vendor call is wired in.

use crate::touchstone::{judge, Action, TouchstonePolicy, Verdict};

/// A layer-2 model's verdict on an action. By CONSTRUCTION there is no
/// `Authorize`/`Affirm` variant — the type itself enforces veto-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelVerdict {
    /// The model flags this action (a recoverable concern). `reason` is surfaced to
    /// the agent. It does NOT name a verdict strength — the fold decides that, and the
    /// strongest a model veto can reach is a recoverable critique.
    Veto { reason: String },
    /// No opinion — leave the layer-1 verdict unchanged.
    Abstain,
}

/// A cross-vendor model consulted as a calibrated polygraph (role A). The actual call
/// (process / network to e.g. codex) is the adapter's job; `flint-core` only defines
/// the contract so the veto-only fold can be verified without a real model.
///
/// REDACTION CONTRACT (codex M12-R4-P2): `consult` receives the raw [`Action`] —
/// including the command — because a polygraph must SEE the action to judge it. But a
/// `Veto`'s `reason` is SURFACED to the agent (it becomes a `Critique.message` the hook
/// prints), so a real impl MUST NOT echo the raw `command` / `scopes` / `context` /
/// `tool_kind` into its reason — that would leak what the command gate redacts. The
/// fold defensively BOUNDS the reason length, but only the impl can guarantee it does
/// not embed a secret. Phase 0.5 ships `NoopVeto`, so this is dormant until a real
/// model is wired (§8.4 #5).
pub trait ModelVeto {
    fn consult(&self, action: &Action) -> ModelVerdict;
}

/// The default: no layer-2 model wired. Always abstains, so the layer-1 verdict
/// stands unchanged (Phase 0.5 default — a real model is opt-in, §8.4 #5).
pub struct NoopVeto;

impl ModelVeto for NoopVeto {
    fn consult(&self, _action: &Action) -> ModelVerdict {
        ModelVerdict::Abstain
    }
}

/// Fold a layer-2 [`ModelVerdict`] onto a layer-1 [`Verdict`]. VETO-ONLY:
///   - `Abstain` -> the layer-1 verdict is unchanged.
///   - `Veto` on an `Affirm` or a `Warn` -> a recoverable `Critique` (the model raises a
///     flag the agent can self-correct on). NEVER a `Deny` — a sovereign freeze is
///     sovereign-only. A warn folds like an affirm because it does NOT block: were it
///     treated as already-blocked, adding a sovereign warn rule would SILENCE a veto the
///     same action would otherwise have drawn.
///   - `Veto` on a `Critique` / `Deny` -> unchanged (already blocked; the model adds
///     nothing it is allowed to add — no upgrade).
///
/// There is no `(layer1, model)` pair that yields an `Affirm` unless layer-1 was
/// already `Affirm` AND the model abstained. A veto can therefore never authorize past
/// a block, and never relax one (invariant ④ at layer-2).
pub fn fold_veto(layer1: Verdict, model: ModelVerdict) -> Verdict {
    // The veto reason is surfaced to the agent; bound it so a misbehaving model can't
    // dump a transcript (the impl must still not embed a secret — redaction contract).
    const MAX_VETO_REASON: usize = 200;
    match (layer1, model) {
        (v, ModelVerdict::Abstain) => v,
        // A model veto raises at most a RECOVERABLE critique on a non-blocking verdict.
        (Verdict::Affirm | Verdict::Warn { .. }, ModelVerdict::Veto { reason }) => Verdict::Critique {
            rule_id: "model-veto".to_string(),
            scope: String::new(),
            message: reason.chars().take(MAX_VETO_REASON).collect(),
            suggestion: String::new(),
        },
        // Already blocked (critique or deny): the model agrees but cannot upgrade.
        (v @ Verdict::Critique { .. }, ModelVerdict::Veto { .. }) => v,
        (v @ Verdict::Deny { .. }, ModelVerdict::Veto { .. }) => v,
    }
}

/// The wired two-pass judge: run the deterministic layer-1 [`judge`], then fold an
/// optional layer-2 model veto over it. The default `veto` is [`NoopVeto`] (no
/// second pass). The result is still veto-only — layer-2 can only add a recoverable
/// flag, never authorize (the layer-1 invariant ④ is preserved end-to-end).
pub fn judge_with_veto(policy: &TouchstonePolicy, action: &Action, veto: &dyn ModelVeto) -> Verdict {
    let layer1 = judge(policy, action);
    // Only consult the (expensive, cross-vendor) layer-2 model when layer-1 did NOT block
    // (Affirm or Warn): a veto can only raise a flag on an unblocked action — it can never
    // relax or upgrade an existing block — so consulting on a Critique/Deny is wasted work
    // and would let a model side-effect / panic on an already-correctly-blocked action for
    // no verdict change (codex M11.2-P2). The fold on a block was already a no-op, so this
    // is behavior-preserving.
    //
    // `Warn` belongs on the consult side, not the blocked side: gating it out would mean a
    // sovereign warn rule SUPPRESSES a veto the same action would otherwise have drawn
    // (non-monotonic). An abstain folds the Warn through unchanged.
    match layer1 {
        v @ (Verdict::Affirm | Verdict::Warn { .. }) => fold_veto(v, veto.consult(action)),
        blocked => blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::touchstone::{GateRule, Matcher, Response, Reversibility};

    /// A test double whose verdict is fixed — stands in for a real cross-vendor call.
    struct MockVeto(ModelVerdict);
    impl ModelVeto for MockVeto {
        fn consult(&self, _action: &Action) -> ModelVerdict {
            self.0.clone()
        }
    }

    fn affirm() -> Verdict {
        Verdict::Affirm
    }
    fn critique() -> Verdict {
        Verdict::Critique {
            rule_id: "r".into(),
            scope: "s".into(),
            message: "m".into(),
            suggestion: "fix".into(),
        }
    }
    fn deny() -> Verdict {
        Verdict::Deny { rule_id: "r".into(), scope: "s".into(), reason: "no".into() }
    }
    fn veto() -> ModelVerdict {
        ModelVerdict::Veto { reason: "model concern".into() }
    }

    #[test]
    fn abstain_leaves_layer1_unchanged() {
        assert_eq!(fold_veto(affirm(), ModelVerdict::Abstain), affirm());
        assert_eq!(fold_veto(critique(), ModelVerdict::Abstain), critique());
        assert_eq!(fold_veto(deny(), ModelVerdict::Abstain), deny());
    }

    #[test]
    fn veto_on_affirm_raises_a_recoverable_critique_never_a_deny() {
        let v = fold_veto(affirm(), veto());
        match v {
            Verdict::Critique { rule_id, message, .. } => {
                assert_eq!(rule_id, "model-veto");
                assert_eq!(message, "model concern");
            }
            other => panic!("a model veto on affirm must be a recoverable critique, got {other:?}"),
        }
    }

    #[test]
    fn a_matching_warn_rule_does_not_suppress_a_model_veto() {
        // End-to-end through the WIRED judge, not just `fold_veto` (codex P2: the fold
        // handled Warn but the wrapper never consulted layer-2 for it, so the fix was
        // unreachable). Non-monotonicity guard: adding a sovereign warn rule must not
        // REMOVE a veto the same action would otherwise have drawn.
        let warn_rule = GateRule {
            id: "nudge".into(),
            matcher: Matcher::Command { pattern: r"\bfind\b".into(), exempt: None },
            response: Response::Warn,
            reversibility: Reversibility::Reversible,
            message: "prefer fd".into(),
            suggestion: "use fd".into(),
        };
        let action = Action {
            tool_kind: "exec".into(),
            scopes: vec!["exec:bash".into()],
            command: Some("find . -name x".into()),
            ..Default::default()
        };
        let bare = TouchstonePolicy { rules: Vec::new(), advisory: Vec::new() };
        let with_warn = TouchstonePolicy { rules: vec![warn_rule], advisory: Vec::new() };
        let mock = MockVeto(veto());
        // No rule at all: the veto raises a critique.
        assert!(judge_with_veto(&bare, &action, &mock).is_critique());
        // Same action, plus a warn rule: the veto must STILL raise a critique.
        assert!(
            judge_with_veto(&with_warn, &action, &mock).is_critique(),
            "a warn must not swallow a model veto"
        );
    }

    #[test]
    fn veto_never_upgrades_an_existing_block() {
        // a veto on a critique stays a critique (no upgrade to deny).
        assert_eq!(fold_veto(critique(), veto()), critique());
        // a veto on a deny stays a deny (already the strongest).
        assert_eq!(fold_veto(deny(), veto()), deny());
    }

    #[test]
    fn veto_can_never_produce_an_affirm() {
        // INVARIANT ④ at layer-2: the ONLY way out is Affirm is (Affirm, Abstain) —
        // the model didn't act. No Veto, on any layer-1 verdict, yields an Affirm.
        for layer1 in [affirm(), critique(), deny()] {
            assert!(
                !fold_veto(layer1.clone(), veto()).is_affirm(),
                "a model veto must never relax a verdict to affirm (was {layer1:?})"
            );
        }
        // And a model cannot authorize past a block via abstain either.
        assert!(fold_veto(deny(), ModelVerdict::Abstain).is_deny());
    }

    #[test]
    fn noop_veto_always_abstains() {
        let action = Action::default();
        assert_eq!(NoopVeto.consult(&action), ModelVerdict::Abstain);
    }

    #[test]
    fn judge_with_veto_composes_layer1_and_layer2() {
        // Layer-1 affirms (no rule); layer-2 mock vetoes -> recoverable critique.
        let policy = TouchstonePolicy::default();
        let action = Action {
            tool_kind: "write".into(),
            scopes: vec!["knowledge/x".into()],
            ..Default::default()
        };
        let v = judge_with_veto(&policy, &action, &MockVeto(veto()));
        assert!(v.is_critique(), "layer-2 veto raises a flag on a layer-1 affirm");

        // Layer-1 denies (a sovereign deny rule); a layer-2 veto cannot relax it.
        let deny_policy = TouchstonePolicy {
            rules: vec![GateRule {
                id: "hold:x".into(),
                matcher: Matcher::Path { glob: "knowledge/x".into() },
                response: Response::Deny,
                reversibility: Reversibility::Reversible,
                message: "frozen".into(),
                suggestion: String::new(),
            }],
            advisory: Vec::new(),
        };
        assert!(
            judge_with_veto(&deny_policy, &action, &MockVeto(veto())).is_deny(),
            "a layer-2 veto cannot relax a sovereign deny"
        );
        // ...and the NoopVeto default leaves layer-1 exactly as-is.
        assert!(judge_with_veto(&deny_policy, &action, &NoopVeto).is_deny());
    }
}
