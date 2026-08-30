//! Cross-vendor judge (PIVOT P4 · reframe-and-diff §3.7 L2 / heart §6 "codex as a
//! polygraph, not a judge"). A REAL transport for the layer-2 [`ModelVeto`] that the prior
//! milestones left as `NoopVeto`.
//!
//! Used on the COLD path (Forge promotion), NOT the hot per-action hook: a cross-vendor
//! shell-out is seconds-slow, fine for an infrequent promotion decision, impractical per
//! tool call. A different vendor's model (codex, judging a Claude-produced rule) is asked
//! whether a candidate promotion should be WITHHELD. Four defenses (heart §6): cross-vendor
//! non-collusion, fixture-anchored (it sees the discrimination fixture), VETO-ONLY (the
//! type has no Authorize variant), honestly the weakest evidence (drifts by model version).
//!
//! VETO-ONLY + FAIL-SAFE: a model error / unclear reply / disabled judge -> `Abstain` (the
//! promotion stands on the executable discrimination alone). The model can only WITHHOLD,
//! never CREATE, a promotion (invariant ④ at the write path). DEFAULT OFF — which model,
//! transport, and when to consult is the owner's call (§8.4 #5); this is the mechanism, off until
//! the owner toggles `[judge] cross_vendor = true`.
//!
//! REDACTION (model_veto.rs contract): consult sees the action, but the surfaced veto reason
//! must not echo a raw command/secret. The probe describes the action by tool_kind + scopes
//! (already redaction-safe for command-shaped actions) and instructs the model not to echo
//! inputs; the reason is bounded by the fold.

use std::process::{Command, Stdio};

use flint_core::model_veto::{ModelVeto, ModelVerdict};
use flint_core::touchstone::Action;

/// A cross-vendor model (codex) consulted as a veto-only polygraph. Default disabled.
pub struct CodexVeto {
    pub enabled: bool,
    /// Optional model override passed to `codex exec -m`. `None` = codex's default.
    pub model: Option<String>,
}

impl CodexVeto {
    pub fn new(enabled: bool, model: Option<String>) -> Self {
        Self { enabled, model }
    }
}

impl ModelVeto for CodexVeto {
    fn consult(&self, action: &Action) -> ModelVerdict {
        if !self.enabled {
            return ModelVerdict::Abstain;
        }
        // Describe the action redaction-safely (tool_kind + scopes; never the raw command).
        let scopes = action.scopes.join(", ");
        let prompt = format!(
            "You are a cross-vendor reviewer acting as a VETO-ONLY polygraph for a rule \
             promotion. A sovereign rule (tool_kind={}, scopes=[{}]) is about to be made \
             load-bearing because it passed an executable discrimination fixture. Reply with \
             EXACTLY one line: `VETO: <short reason>` if you see a clear reason to WITHHOLD \
             promotion (e.g. the rule looks over-broad or unsafe), or `ABSTAIN` otherwise. Do \
             NOT echo any command text or secrets. Default to ABSTAIN if unsure.",
            action.tool_kind, scopes,
        );
        match self.run_codex(&prompt) {
            Some(output) => parse_codex_verdict(&output),
            // Fail-SAFE: a transport error abstains (a missed veto is the safe direction for a
            // veto-only signal — heart §6 "missed one veto" not "wrongly froze").
            None => ModelVerdict::Abstain,
        }
    }
}

impl CodexVeto {
    fn run_codex(&self, prompt: &str) -> Option<String> {
        let mut cmd = Command::new("codex");
        cmd.arg("exec").arg("-s").arg("read-only");
        if let Some(m) = &self.model {
            cmd.arg("-m").arg(m);
        }
        cmd.arg(prompt).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let out = cmd.output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

/// Parse a cross-vendor reply into a veto-only [`ModelVerdict`]. PURE + fail-safe: only an
/// explicit `VETO:` line produces a Veto; everything else (including a malformed/empty
/// reply) is Abstain. The reason is sanitized to one short line.
pub fn parse_codex_verdict(output: &str) -> ModelVerdict {
    for line in output.lines() {
        let t = line.trim();
        // Strict (codex capstone P2): only `VETO:` (with the colon) or a standalone `VETO`
        // line vetoes — NOT an arbitrary `VETO...` line (avoid over-withholding on prose
        // that merely mentions the word). A bare `VETO` carries no reason.
        let reason_opt = t.strip_prefix("VETO:").or_else(|| (t == "VETO").then_some(""));
        if let Some(reason) = reason_opt {
            let reason = reason.trim_start_matches([':', ' ']).trim();
            let reason = if reason.is_empty() { "cross-vendor judge withheld promotion" } else { reason };
            // one short line; the fold also bounds length, this strips newlines defensively.
            let reason: String = reason.chars().take(160).filter(|c| *c != '\n' && *c != '\r').collect();
            return ModelVerdict::Veto { reason };
        }
    }
    ModelVerdict::Abstain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_veto_line() {
        match parse_codex_verdict("some thinking...\nVETO: rule looks over-broad\n") {
            ModelVerdict::Veto { reason } => assert_eq!(reason, "rule looks over-broad"),
            other => panic!("expected veto, got {other:?}"),
        }
    }

    #[test]
    fn parse_abstain_default() {
        assert_eq!(parse_codex_verdict("ABSTAIN\n"), ModelVerdict::Abstain);
        assert_eq!(parse_codex_verdict("I think this is fine, abstaining"), ModelVerdict::Abstain);
        assert_eq!(parse_codex_verdict(""), ModelVerdict::Abstain); // empty -> abstain (fail-safe)
        assert_eq!(parse_codex_verdict("garbage no verdict line"), ModelVerdict::Abstain);
    }

    #[test]
    fn parser_is_strict_not_substring(){
        // codex capstone P2: a line that merely starts with VETO-something is NOT a veto.
        assert_eq!(parse_codex_verdict("VETOED nothing here"), ModelVerdict::Abstain);
        assert_eq!(parse_codex_verdict("no VETO: this is mid-sentence"), ModelVerdict::Abstain);
        // a bare standalone VETO line still vetoes (terse model), with no reason.
        assert!(matches!(parse_codex_verdict("VETO"), ModelVerdict::Veto { .. }));
    }

    #[test]
    fn disabled_codex_veto_abstains_without_shelling() {
        // enabled=false must NOT shell out; always abstains.
        let v = CodexVeto::new(false, None);
        assert_eq!(v.consult(&Action::default()), ModelVerdict::Abstain);
    }

    #[test]
    fn veto_reason_is_bounded_and_single_line() {
        let long = format!("VETO: {}", "x".repeat(500));
        match parse_codex_verdict(&long) {
            ModelVerdict::Veto { reason } => assert!(reason.len() <= 160),
            other => panic!("expected veto, got {other:?}"),
        }
    }
}
