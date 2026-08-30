//! Observation log (PIVOT write-path feed · reframe-and-diff §3.7 / heart §2).
//!
//! Every judged action appends ONE redacted receipt line here. This is the cold-path
//! input the Forge distiller (P2) mines for repeated corrections. CRITICAL: it stores a
//! REDACTED fingerprint via [`Action::receipt_view`] — NEVER the conversation, NEVER a
//! raw command (a Bash argv may itself contain a secret; heart §2 + secret-zero). The
//! recording is best-effort: a log write failure must NEVER change the verdict (the
//! verdict is the gate; the receipt is an audit side-effect).

use serde_json::json;

use crate::touchstone::{Action, Verdict};

/// Build the redacted JSONL receipt line for one judged action. Pure: the caller does the
/// (best-effort) append. Uses `receipt_view` so a command-shaped action's fields are all
/// fixed constants — a forged adapter can't leak a secret through any field.
pub fn receipt_line(harness: &str, action: &Action, verdict: &Verdict, ts_unix: u64) -> String {
    let view = action.receipt_view();
    json!({
        "schema": "flint-receipt-v1",
        "ts": ts_unix,
        "harness": harness,
        "tool_kind": view.tool_kind,
        "scopes": view.scopes,
        "context": view.context,
        "verdict": verdict.tag(),
        // The matched rule id is safe to record on a warn/critique/deny (sovereign-authored,
        // not agent content); affirm has none. A warn names its rule for the same reason a
        // critique does — an unattributed ledger line cannot be counted per-rule.
        "rule_id": match verdict {
            Verdict::Warn { rule_id, .. }
            | Verdict::Critique { rule_id, .. }
            | Verdict::Deny { rule_id, .. } => Some(rule_id.as_str()),
            Verdict::Affirm => None,
        },
        "energy": action.energy,
    })
    .to_string()
}

/// Build the receipt line for a rule whose critique was SUPPRESSED by its exempt regex
/// (a declared downgrade). Same redaction contract as [`receipt_line`]; the verdict tag
/// is `exempt` and the suppressed rule is named, so bypass usage is a countable ledger
/// event instead of an unlogged pass-through.
pub fn exempt_receipt_line(harness: &str, action: &Action, rule_id: &str, ts_unix: u64) -> String {
    let view = action.receipt_view();
    json!({
        "schema": "flint-receipt-v1",
        "ts": ts_unix,
        "harness": harness,
        "tool_kind": view.tool_kind,
        "scopes": view.scopes,
        "context": view.context,
        "verdict": "exempt",
        // The suppressed rule id is sovereign-authored (never agent content) — safe.
        "rule_id": rule_id,
        "energy": action.energy,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_shaped_receipt_never_leaks_secret() {
        let secret = "ghp_aaaaaaaaaaaaaaaaaaaaaa";
        let action = Action {
            tool_kind: format!("exec {secret}"),
            scopes: vec![format!("knowledge/{secret}")],
            command: Some(format!("echo {secret}")),
            context: format!("ctx={secret}"),
            ..Default::default()
        };
        let v = Verdict::Deny { rule_id: "secret-zero".into(), scope: "exec:bash".into(), reason: "no".into() };
        let line = receipt_line("claude", &action, &v, 1234);
        assert!(!line.contains(secret), "no field leaks the secret into the obs-log");
        assert!(line.contains("\"verdict\":\"deny\""));
        assert!(line.contains("secret-zero"));
        assert!(line.contains("exec:bash")); // redacted scope label
    }

    #[test]
    fn exempt_receipt_names_the_rule_and_redacts_the_command() {
        let secret = "ghp_aaaaaaaaaaaaaaaaaaaaaa";
        let action = Action {
            tool_kind: "exec".into(),
            scopes: vec!["exec:bash".into()],
            command: Some(format!("BYPASS=reason grep {secret} x.rs")),
            ..Default::default()
        };
        let line = exempt_receipt_line("claude", &action, "lsp-over-grep", 99);
        assert!(line.contains("\"verdict\":\"exempt\""));
        assert!(line.contains("\"rule_id\":\"lsp-over-grep\""));
        assert!(!line.contains(secret), "the raw command never reaches the ledger");
        assert!(line.contains("\"schema\":\"flint-receipt-v1\""));
    }

    #[test]
    fn affirm_has_no_rule_id_records_scope() {
        let action = Action {
            tool_kind: "write".into(),
            scopes: vec!["knowledge/notes/x".into()],
            ..Default::default()
        };
        let line = receipt_line("codex", &action, &Verdict::Affirm, 7);
        assert!(line.contains("\"verdict\":\"affirm\""));
        assert!(line.contains("\"rule_id\":null"));
        assert!(line.contains("knowledge/notes/x")); // non-command scope recorded (legit audit target)
        assert!(line.contains("\"harness\":\"codex\""));
    }
}
