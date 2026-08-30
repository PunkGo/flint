//! Touchstone (试金石) — the harness-agnostic dialectical judge (spec §5, M11/M12).
//!
//! Flint strikes the fire (an action); Touchstone tests its temper (a verdict).
//! A per-harness ADAPTER maps one tool call into a neutral [`Action`]; Touchstone
//! — harness-agnostic, in `flint-core` — judges it against the sovereign
//! [`TouchstonePolicy`] into a four-valued [`Verdict`] (strongest first):
//!   - **Deny** — a sovereign-freeze rule matched, OR a weaker tier escalated by
//!     irreversibility (the deny frontier is the irreversible line).
//!   - **Critique** — a recoverable rule matched: the action is wrong, but the
//!     agent is told *how to do it right* (restorative). The harness lets it
//!     self-correct and continue. The action does NOT run.
//!   - **Warn** — a non-blocking rule matched: the action RUNS, the agent is told,
//!     and the passage is receipted. The tier that lets a high-friction rule keep
//!     its voice and its ledger without stopping legitimate work.
//!   - **Affirm** — no rule matched. NOT an authorization: the ABSENCE of a deny.
//!
//! FOUR verdicts, THREE `response:` tiers (`warn` / `critique` / `block`). `Affirm`
//! is the no-match case, which no rule can request — that asymmetry IS invariant ④.
//! The strongest matching tier always wins, so a Warn can never mask a rule that
//! would have stopped the action.
//!
//! A rule matches an action one of two ways (its [`Matcher`]):
//!   - **Path** (M11): a glob over the action's TARGET scopes (file paths).
//!   - **Command** (M12): a regex over the action's COMMAND TEXT, with an optional
//!     `exempt` regex (a path-producer fast-path / inline downgrade declaration).
//!     This is the syntactic-tier's argv/content subset — what the original
//!     `lsp-over-grep` and `secret-zero` shell gates matched. The command is judged
//!     IN MEMORY and is NEVER written to the receipt (hard redaction — the ledger
//!     must not store a command that itself contains a secret; heart §2 + secret-zero).
//!
//! INVARIANT ④ (the dialectical-gate soundness root): `judge` has NO
//! allow-authorizing input. There is no field a constrained party can set to turn
//! a Critique/Deny into an Affirm. A forged [`Action`] can at most MISLABEL its
//! scope / command (dodge a rule — an honest §12.6 adapter limit), never AUTHORIZE
//! one. The policy is derived ONLY from SOVEREIGN-written authorities, so an agent
//! can author no rule.

use regex::Regex;

use crate::glob::glob_match;

/// How a matched rule responds. The three tiers of whitepaper action 3 (allow / warn /
/// block), weakest first — `Affirm` is the absence of a match, not a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// Non-blocking: the action PROCEEDS; the agent is told, and the passage is receipted.
    /// The tier that lets a high-friction rule keep its voice and its ledger without
    /// stopping legitimate work.
    Warn,
    /// Recoverable: block this action but tell the agent how to do it right.
    Critique,
    /// Sovereign freeze: no recovery path is offered.
    Deny,
}

impl Response {
    /// Stable tag for display / receipts (`warn` | `critique` | `deny`).
    pub fn tag(&self) -> &'static str {
        match self {
            Response::Warn => "warn",
            Response::Critique => "critique",
            Response::Deny => "deny",
        }
    }
}

/// Whether the guarded action is causally reversible. Irreversibility is the deny
/// frontier: an irreversible action escalates even a `Critique` rule to a `Deny`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reversibility {
    #[default]
    Reversible,
    Irreversible,
}

/// How a rule decides whether it applies to an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matcher {
    /// Match the action's TARGET scopes against a glob (M11): `<prefix>/**` or exact.
    Path { glob: String },
    /// Match the action's COMMAND text against a `pattern` regex (M12). A match on the
    /// optional `exempt` regex SUPPRESSES the rule (the path-producer fast-path /
    /// inline downgrade declaration). Both are sovereign-written and validated to
    /// compile at parse time (a non-compiling pattern makes the rule malformed →
    /// dropped → fall back to a valid version).
    Command { pattern: String, exempt: Option<String> },
}

impl Matcher {
    /// A short, redaction-safe description for `--show` / debugging. For a command
    /// matcher this is the PATTERN (a sovereign-written regex, not agent content) —
    /// never the matched command.
    pub fn describe(&self) -> String {
        match self {
            Matcher::Path { glob } => format!("scope:{glob}"),
            Matcher::Command { pattern, .. } => format!("command:/{pattern}/"),
        }
    }
}

/// Whether a command-rule regex compiles (the `regex` crate is linear-time, so a
/// valid pattern can't ReDoS the judge on agent input). The writers validate with
/// this so a bad pattern is a LOUD error, not a silently-dropped rule (M10-R2
/// lesson); the projector also drops a non-compiling pattern (fail-closed).
pub fn pattern_compiles(pattern: &str) -> bool {
    Regex::new(pattern).is_ok()
}

/// One active sovereign gate rule. Derived ONLY from a sovereign-written authority
/// (`hold_on` or `gate_rule`) — a non-sovereign one is dropped by the projector (F16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRule {
    pub id: String,
    pub matcher: Matcher,
    pub response: Response,
    pub reversibility: Reversibility,
    /// Why the rule fired (the deny reason / the critique's "what's wrong").
    pub message: String,
    /// The recovery suggestion handed to the agent on a Critique. Empty for a bare Deny.
    pub suggestion: String,
}

/// An advisory rule (schema v1 `kind: advisory`): a behavioral guideline (an iron law
/// like verify-before-claiming) COMPILED INTO agent context by Striker, NOT a runtime
/// gate. It has no matcher and never produces a verdict — it is guidance delivered at
/// the user-message level (official Claude/Codex semantics: context is guidance, hooks
/// enforce). advisory is the "carry it over" arm of the whitepaper's second action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryRule {
    pub id: String,
    /// One-line third-person "what + when" — the routing/injection headline.
    pub description: String,
    /// The full guidance body (the rule prose the agent reads).
    pub message: String,
    /// When it applies (keywords / situations) — injected so the agent knows the trigger.
    pub trigger: Vec<String>,
    /// Effect-range selectors (which agents / projects it is compiled into).
    pub scope: Vec<String>,
}

/// The active sovereign policy. `rules` are runtime gates (judged); `advisory` are
/// guidelines compiled into agent context (never judged — invariant ④: affirm is the
/// absence of a matching gate rule, and advisory rules are not gates at all).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TouchstonePolicy {
    pub rules: Vec<GateRule>,
    pub advisory: Vec<AdvisoryRule>,
}

/// A harness-reported action — the adapter contract IN. HONEST BOUNDARY §12.6: every
/// field is reported by the constrained party, so a forge can MISLABEL/DODGE, never
/// AUTHORIZE (invariant ④).
#[derive(Debug, Clone, Default)]
pub struct Action {
    /// The kernel-recorded actor, where available (unforgeable only under a daemon).
    pub actor_id: String,
    /// Coarse tool class: `write` | `read` | `search` | `exec` | `tool:<name>`.
    pub tool_kind: String,
    /// Target scopes (file paths / scopes) the action touches.
    pub scopes: Vec<String>,
    /// The raw command text, for tools that run one (Bash). Used ONLY by the
    /// in-memory command matcher (M12); it is NEVER serialized to the receipt — the
    /// ledger must not store a command that may itself contain a secret (hard
    /// redaction, secret-zero + heart §2). `None` for non-command tools.
    pub command: Option<String>,
    /// A short context fingerprint — NOT the conversation. Free-form, advisory.
    pub context: String,
    /// Physical energy the harness/kernel measured (tokens / calls / wall-time), if
    /// reported. `None` when not measured — only real-measured energy bears weight.
    pub energy: Option<u64>,
}

impl Action {
    /// A compact, conversation-free, command-free fingerprint for the receipt:
    /// `<tool_kind> <scope,scope,…>` with scopes SORTED + deduped (a pure function of
    /// the scope SET). The raw `command` is deliberately EXCLUDED — it never reaches
    /// the ledger.
    pub fn fingerprint(&self) -> String {
        let mut scopes = self.scopes.clone();
        scopes.sort();
        scopes.dedup();
        format!("{} {}", self.tool_kind, scopes.join(","))
    }

    /// The redaction-safe view of this action recorded on the ledger. CORE-ENFORCED
    /// redaction (codex M12-P1): for a COMMAND-SHAPED action — one carrying a command,
    /// OR an `exec` class / `exec:bash` scope — EVERY recorded field (`action`,
    /// `tool_kind`, `scopes`, `context`) is a FIXED constant. NONE is read from the
    /// caller, because a forged/buggy adapter could load any of them with the command's
    /// secret. The raw command is never serialized in any case.
    ///
    /// For a non-command action the target scopes (a legit file path — the audit
    /// target, not a secret) + a bounded context are recorded. HONEST BOUNDARY §12.6:
    /// flint cannot tell a legit file path from a forged secret a LYING adapter put in
    /// the target field; it guards the interception surface it is wired into, not a
    /// malicious harness. The agent-controlled leak vector (a secret in a Bash argv) is
    /// `command`, which is never serialized — that one flint fully closes.
    /// Whether this action is COMMAND-SHAPED: it carries a command, or is an `exec`
    /// class / `exec:bash` scope action. For such actions BOTH the receipt AND the
    /// reported verdict scope are redacted to a fixed label, because the caller scopes
    /// / command may carry a secret (codex M12-R3-P1).
    pub fn is_command_shaped(&self) -> bool {
        self.command.is_some()
            || self.tool_kind == "exec"
            || self.scopes.iter().any(|s| s == COMMAND_SCOPE_LABEL)
    }

    pub fn receipt_view(&self) -> ReceiptView {
        const MAX_CONTEXT: usize = 200;
        if self.is_command_shaped() {
            ReceiptView {
                action: COMMAND_SCOPE_LABEL.to_string(),
                tool_kind: "exec".to_string(),
                scopes: vec![COMMAND_SCOPE_LABEL.to_string()],
                context: String::new(),
            }
        } else {
            ReceiptView {
                action: self.fingerprint(),
                tool_kind: self.tool_kind.clone(),
                scopes: self.scopes.clone(),
                context: self.context.chars().take(MAX_CONTEXT).collect(),
            }
        }
    }
}

/// The fixed, redaction-safe scope label reported for a COMMAND match and recorded in
/// a command-shaped action's receipt. Never derived from caller scopes / the command.
const COMMAND_SCOPE_LABEL: &str = "exec:bash";

/// The conversation-/command-free projection of an [`Action`] that reaches the ledger.
/// All fields are redacted to fixed constants for a command-shaped action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptView {
    /// The action fingerprint (the fixed coarse label for command-shaped actions).
    pub action: String,
    /// The tool class (`exec` constant for command-shaped actions — never caller-set).
    pub tool_kind: String,
    /// The recorded scopes (the fixed coarse label for command-shaped actions).
    pub scopes: Vec<String>,
    /// The bounded context (empty for command-shaped actions).
    pub context: String,
}

/// The verdict — the adapter contract OUT. `Affirm` carries nothing; `Warn`/`Critique`/
/// `Deny` name the offending rule + a REDACTED scope (never the raw command).
///
/// `Warn` differs from the other two in EFFECT, not in shape: the action proceeds. It is
/// still a fired rule with a receipt, which is the whole point — an unenforced advisory
/// leaves no ledger, and a critique that blocks legitimate work gets bypassed into
/// meaninglessness (pit: declared-bypass-creep-dulls-critique-gate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Affirm,
    Warn {
        rule_id: String,
        scope: String,
        message: String,
        suggestion: String,
    },
    Critique {
        rule_id: String,
        scope: String,
        message: String,
        suggestion: String,
    },
    Deny {
        rule_id: String,
        scope: String,
        reason: String,
    },
}

impl Verdict {
    pub fn is_affirm(&self) -> bool {
        matches!(self, Verdict::Affirm)
    }
    pub fn is_warn(&self) -> bool {
        matches!(self, Verdict::Warn { .. })
    }
    pub fn is_critique(&self) -> bool {
        matches!(self, Verdict::Critique { .. })
    }
    pub fn is_deny(&self) -> bool {
        matches!(self, Verdict::Deny { .. })
    }
    /// The verdict's stable tag for the receipt (`affirm` | `warn` | `critique` | `deny`).
    pub fn tag(&self) -> &'static str {
        match self {
            Verdict::Affirm => "affirm",
            Verdict::Warn { .. } => "warn",
            Verdict::Critique { .. } => "critique",
            Verdict::Deny { .. } => "deny",
        }
    }
}

/// Does `rule` match `action`? Returns the REDACTED scope to report on a match (never
/// the raw command), or `None`.
fn rule_match(rule: &GateRule, action: &Action) -> Option<String> {
    match &rule.matcher {
        Matcher::Path { glob } => {
            // The smallest matching target scope — a pure function of the scope SET.
            let matched = action.scopes.iter().filter(|s| glob_match(glob, s)).min()?;
            // For a command-shaped action the matched scope is REDACTED to the fixed
            // label: a forged adapter could put a secret in scopes, and the verdict
            // scope is surfaced (e.g. hook deny message), so it must never carry caller
            // scope content for a command-shaped action (codex M12-R3-P1).
            if action.is_command_shaped() {
                Some(COMMAND_SCOPE_LABEL.to_string())
            } else {
                Some(matched.clone())
            }
        }
        Matcher::Command { pattern, exempt } => {
            let cmd = action.command.as_deref()?;
            // A non-compiling pattern can't reach here — parse_gate_rule validated it
            // and DROPPED a non-compiling rule (fail-closed). Defensive: a compile
            // failure yields no match.
            let re = Regex::new(pattern).ok()?;
            if !re.is_match(cmd) {
                return None;
            }
            // The exempt regex is honored ONLY for a RECOVERABLE tier — a warn or a
            // critique (the inline downgrade declaration / path-producer fast-path). A DENY
            // or IRREVERSIBLE rule IGNORES exempt: a sovereign freeze can NEVER be bypassed
            // by crafting the command to match an exempt — that would be
            // authorization-by-input (invariant ④, codex M12-P1-1).
            let recoverable = matches!(rule.response, Response::Warn | Response::Critique)
                && rule.reversibility == Reversibility::Reversible;
            if recoverable {
                if let Some(ex) = exempt {
                    if Regex::new(ex).map(|r| r.is_match(cmd)).unwrap_or(false) {
                        return None; // exempt -> not fired (recoverable downgrade)
                    }
                }
            }
            // The reported scope is a FIXED redaction-safe label — never the command.
            Some(COMMAND_SCOPE_LABEL.to_string())
        }
    }
}

/// The AUDIT surface for declared downgrades: every rule whose warn or critique was
/// SUPPRESSED by its exempt regex on this action. Pure and side-channel only — `judge`
/// never reads it (invariant ④: the audit cannot change a verdict), the hook writes each
/// entry to the obs-log as an `exempt` receipt so bypass habit is countable instead of
/// invisible (pit: declared-bypass-creep-dulls-critique-gate). Mirrors `rule_match`'s
/// exempt branch exactly: only a RECOVERABLE tier honors exempt, so a deny rule can never
/// appear here.
pub fn exempted_rules(policy: &TouchstonePolicy, action: &Action) -> Vec<String> {
    let Some(cmd) = action.command.as_deref() else { return Vec::new() };
    policy
        .rules
        .iter()
        .filter(|rule| {
            matches!(rule.response, Response::Warn | Response::Critique)
                && rule.reversibility == Reversibility::Reversible
                && match &rule.matcher {
                    Matcher::Command { pattern, exempt: Some(ex) } => {
                        Regex::new(pattern).map(|r| r.is_match(cmd)).unwrap_or(false)
                            && Regex::new(ex).map(|r| r.is_match(cmd)).unwrap_or(false)
                    }
                    _ => false,
                }
        })
        .map(|rule| rule.id.clone())
        .collect()
}

/// The pure dialectical verdict over the policy + a harness-reported action.
///
/// - **DENY PRECEDENCE**: any matching rule whose effective response is a deny (an
///   explicit `Deny`, or a `Warn`/`Critique` on an `Irreversible` rule — irreversibility
///   is the deny frontier for every tier) makes the verdict a `Deny`.
/// - else a matching recoverable `Critique`.
/// - else a matching recoverable `Warn` — fired and receipted, but the action proceeds.
/// - else `Affirm` — NO rule matched (not an authorization; invariant ④).
///
/// The strongest tier always wins: a warn can never mask a rule that would have stopped
/// the action.
///
/// DETERMINISM (invariant ③): the reported (scope, rule_id) is the total-order-minimal
/// `(scope, rule.id)` among matching candidates — a pure function of the action, not
/// of harness scope order. Reported scopes are always REDACTED (never the raw command).
pub fn judge(policy: &TouchstonePolicy, action: &Action) -> Verdict {
    let mut denies: Vec<(String, &GateRule)> = Vec::new();
    let mut critiques: Vec<(String, &GateRule)> = Vec::new();
    let mut warns: Vec<(String, &GateRule)> = Vec::new();
    for rule in &policy.rules {
        let Some(scope) = rule_match(rule, action) else {
            continue;
        };
        if rule.response == Response::Deny || rule.reversibility == Reversibility::Irreversible {
            denies.push((scope, rule));
        } else if rule.response == Response::Warn {
            warns.push((scope, rule));
        } else {
            critiques.push((scope, rule));
        }
    }
    if let Some((scope, rule)) = denies.into_iter().min_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id))) {
        return Verdict::Deny {
            rule_id: rule.id.clone(),
            scope,
            reason: rule.message.clone(),
        };
    }
    if let Some((scope, rule)) = critiques.into_iter().min_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id))) {
        return Verdict::Critique {
            rule_id: rule.id.clone(),
            scope,
            message: rule.message.clone(),
            suggestion: rule.suggestion.clone(),
        };
    }
    if let Some((scope, rule)) = warns.into_iter().min_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id))) {
        return Verdict::Warn {
            rule_id: rule.id.clone(),
            scope,
            message: rule.message.clone(),
            suggestion: rule.suggestion.clone(),
        };
    }
    Verdict::Affirm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_rule(id: &str, glob: &str, response: Response, rev: Reversibility) -> GateRule {
        GateRule {
            id: id.into(),
            matcher: Matcher::Path { glob: glob.into() },
            response,
            reversibility: rev,
            message: format!("rule {id}"),
            suggestion: format!("fix {id}"),
        }
    }

    fn cmd_rule(id: &str, pattern: &str, exempt: Option<&str>, response: Response, rev: Reversibility) -> GateRule {
        GateRule {
            id: id.into(),
            matcher: Matcher::Command { pattern: pattern.into(), exempt: exempt.map(String::from) },
            response,
            reversibility: rev,
            message: format!("rule {id}"),
            suggestion: format!("fix {id}"),
        }
    }

    fn policy(rules: Vec<GateRule>) -> TouchstonePolicy {
        TouchstonePolicy { rules, advisory: Vec::new() }
    }

    fn write_action(scopes: &[&str]) -> Action {
        Action {
            tool_kind: "write".into(),
            scopes: scopes.iter().map(|s| (*s).into()).collect(),
            ..Default::default()
        }
    }

    fn bash_action(command: &str) -> Action {
        Action {
            tool_kind: "exec".into(),
            scopes: vec!["exec:bash".into()],
            command: Some(command.into()),
            ..Default::default()
        }
    }

    // ── scope matcher (M11, preserved) ────────────────────────────────────────
    #[test]
    fn empty_policy_affirms() {
        assert_eq!(judge(&TouchstonePolicy::default(), &write_action(&["knowledge/x"])), Verdict::Affirm);
    }

    #[test]
    fn scope_deny_rule_denies() {
        let p = policy(vec![scope_rule("secret", "knowledge/secret/**", Response::Deny, Reversibility::Reversible)]);
        let v = judge(&p, &write_action(&["knowledge/secret/key"]));
        assert!(v.is_deny());
    }

    #[test]
    fn scope_critique_rule_critiques() {
        let p = policy(vec![scope_rule("lsp", "src/**", Response::Critique, Reversibility::Reversible)]);
        assert!(judge(&p, &write_action(&["src/main.rs"])).is_critique());
    }

    #[test]
    fn unmatched_scope_affirms() {
        let p = policy(vec![scope_rule("secret", "knowledge/secret/**", Response::Deny, Reversibility::Reversible)]);
        assert_eq!(judge(&p, &write_action(&["knowledge/public/note"])), Verdict::Affirm);
    }

    #[test]
    fn deny_outranks_critique_anywhere() {
        let p = policy(vec![
            scope_rule("crit", "src/**", Response::Critique, Reversibility::Reversible),
            scope_rule("deny", "knowledge/secret/**", Response::Deny, Reversibility::Reversible),
        ]);
        assert!(judge(&p, &write_action(&["src/main.rs", "knowledge/secret/key"])).is_deny());
    }

    #[test]
    fn irreversible_critique_escalates_to_deny() {
        let p = policy(vec![scope_rule("send", "exec:bash", Response::Critique, Reversibility::Irreversible)]);
        let a = Action { tool_kind: "exec".into(), scopes: vec!["exec:bash".into()], ..Default::default() };
        assert!(judge(&p, &a).is_deny());
    }

    #[test]
    fn forged_action_fields_cannot_authorize_an_affirm() {
        // INVARIANT ④: no Action field turns a matching deny into an affirm.
        let p = policy(vec![scope_rule("secret", "knowledge/secret/**", Response::Deny, Reversibility::Reversible)]);
        let forged = Action {
            actor_id: "root".into(),
            tool_kind: "allow".into(),
            scopes: vec!["knowledge/secret/key".into()],
            command: Some("trusted:true".into()),
            context: "authorized:true".into(),
            energy: Some(0),
        };
        assert!(judge(&p, &forged).is_deny());
    }

    #[test]
    fn verdict_is_deterministic_over_scope_set_not_order() {
        let p = policy(vec![
            scope_rule("a", "knowledge/a/**", Response::Deny, Reversibility::Reversible),
            scope_rule("b", "knowledge/b/**", Response::Deny, Reversibility::Reversible),
        ]);
        let forward = judge(&p, &write_action(&["knowledge/b/x", "knowledge/a/y"]));
        let reversed = judge(&p, &write_action(&["knowledge/a/y", "knowledge/b/x"]));
        match &forward {
            Verdict::Deny { scope, rule_id, .. } => {
                assert_eq!(scope, "knowledge/a/y");
                assert_eq!(rule_id, "a");
            }
            _ => unreachable!(),
        }
        assert_eq!(forward, reversed, "verdict is independent of harness scope order");
    }

    #[test]
    fn fingerprint_is_conversation_free_and_set_pure() {
        let a = write_action(&["knowledge/a", "knowledge/b"]);
        assert_eq!(a.fingerprint(), "write knowledge/a,knowledge/b");
        let reordered = write_action(&["knowledge/b", "knowledge/a", "knowledge/a"]);
        assert_eq!(reordered.fingerprint(), a.fingerprint(), "fingerprint is order/dedup-invariant");
    }

    // ── command matcher (M12) ─────────────────────────────────────────────────
    #[test]
    fn command_rule_matches_pattern() {
        // lsp-over-grep shape: grep token + source extension -> critique.
        let p = policy(vec![cmd_rule(
            "lsp", r"\b(grep|rg|ag)\b.*\.(rs|ts)\b", None, Response::Critique, Reversibility::Reversible,
        )]);
        assert!(judge(&p, &bash_action("grep -rn foo src/main.rs")).is_critique());
        // a plain-text grep (no source ext) does not fire.
        assert_eq!(judge(&p, &bash_action("grep -rn TODO README.md")), Verdict::Affirm);
    }

    #[test]
    fn command_rule_exempt_suppresses() {
        // exempt = an inline downgrade declaration -> not fired (recoverable bypass).
        let p = policy(vec![cmd_rule(
            "lsp", r"\bgrep\b.*\.rs\b", Some(r"FLINT_BYPASS=1"), Response::Critique, Reversibility::Reversible,
        )]);
        assert!(judge(&p, &bash_action("grep foo x.rs")).is_critique());
        assert_eq!(judge(&p, &bash_action("FLINT_BYPASS=1 grep foo x.rs")), Verdict::Affirm, "exempt suppresses");
    }

    #[test]
    fn command_rule_deny_is_irreversible_freeze() {
        // secret-zero shape: a credential pattern -> deny.
        let p = policy(vec![cmd_rule(
            "secret-zero", r"\bghp_[A-Za-z0-9]{20,}", None, Response::Deny, Reversibility::Irreversible,
        )]);
        let v = judge(&p, &bash_action("git remote set-url origin https://ghp_aaaaaaaaaaaaaaaaaaaaaa@github.com/x"));
        assert!(v.is_deny(), "a credential on the command denies");
    }

    #[test]
    fn command_verdict_never_leaks_the_raw_command() {
        // The reported scope on a command match is the REDACTED label (exec:bash),
        // never the raw command (which may contain the secret secret-zero blocks).
        let p = policy(vec![cmd_rule("secret-zero", r"ghp_[A-Za-z0-9]{20,}", None, Response::Deny, Reversibility::Irreversible)]);
        let secret_cmd = "echo ghp_aaaaaaaaaaaaaaaaaaaaaa";
        match judge(&p, &bash_action(secret_cmd)) {
            Verdict::Deny { scope, reason, rule_id } => {
                assert_eq!(scope, "exec:bash", "reported scope is the redacted label");
                assert!(!scope.contains("ghp_") && !reason.contains("ghp_") && !rule_id.contains("ghp_"),
                    "no part of the verdict contains the raw command/secret");
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn command_rule_without_command_action_affirms() {
        // A command rule never fires on an action with no command (e.g. a Write).
        let p = policy(vec![cmd_rule("lsp", r"grep", None, Response::Critique, Reversibility::Reversible)]);
        assert_eq!(judge(&p, &write_action(&["src/main.rs"])), Verdict::Affirm);
    }

    #[test]
    fn exempt_is_ignored_on_a_deny_rule() {
        // INVARIANT ④ (codex M12-P1-1): a DENY rule IGNORES exempt — the agent cannot
        // craft the command to match the exempt and turn the freeze into an affirm.
        let p = policy(vec![cmd_rule(
            "secret-zero", r"ghp_[A-Za-z0-9]{20,}", Some(r"PLEASE_ALLOW"),
            Response::Deny, Reversibility::Irreversible,
        )]);
        // even with the exempt token present, the deny still fires.
        let v = judge(&p, &bash_action("PLEASE_ALLOW echo ghp_aaaaaaaaaaaaaaaaaaaaaa"));
        assert!(v.is_deny(), "a deny rule's freeze cannot be bypassed by an exempt match");
    }

    #[test]
    fn exempt_only_suppresses_recoverable_critique() {
        // The same exempt DOES suppress a recoverable critique (the legit downgrade).
        let p = policy(vec![cmd_rule(
            "lsp", r"\bgrep\b.*\.rs", Some(r"BYPASS"), Response::Critique, Reversibility::Reversible,
        )]);
        assert!(judge(&p, &bash_action("grep x y.rs")).is_critique());
        assert_eq!(judge(&p, &bash_action("BYPASS grep x y.rs")), Verdict::Affirm);
    }

    #[test]
    fn receipt_view_redacts_command_shaped_actions_against_all_forged_fields() {
        // CORE-ENFORCED redaction (codex M12-R1/R2-P1): a forged adapter that stuffs the
        // secret into EVERY caller field — tool_kind, scopes, command, context — still
        // does not leak. A command-shaped action's receipt view is all fixed constants.
        let secret = "ghp_aaaaaaaaaaaaaaaaaaaaaa";
        let forged = Action {
            tool_kind: format!("exec {secret}"),     // forged tool_kind
            scopes: vec![format!("knowledge/{secret}")], // forged scope
            command: Some(format!("echo {secret}")), // the real command (never serialized)
            context: format!("secret={secret}"),     // forged context
            ..Default::default()
        };
        let view = forged.receipt_view();
        let all = format!("{} {} {} {}", view.action, view.tool_kind, view.scopes.join(","), view.context);
        assert!(!all.contains(secret), "no caller field leaks into a command-shaped receipt view");
        assert_eq!(view.tool_kind, "exec", "tool_kind redacted to a constant");
        assert_eq!(view.scopes, vec!["exec:bash".to_string()], "fixed coarse label only");

        // The exec-shape predicate also fires WITHOUT a command (forged exec adapter).
        let forged_no_cmd = Action {
            tool_kind: "exec".into(),
            scopes: vec![secret.to_string()],
            command: None,
            context: secret.to_string(),
            ..Default::default()
        };
        let v2 = forged_no_cmd.receipt_view();
        assert!(!format!("{} {} {}", v2.scopes.join(","), v2.context, v2.action).contains(secret),
            "an exec-shaped action with no command is still redacted");

        // A non-command action records its real (legit, non-secret) target scope.
        let write = write_action(&["knowledge/notes/x"]);
        assert_eq!(write.receipt_view().scopes, vec!["knowledge/notes/x".to_string()]);
    }

    #[test]
    fn scope_match_on_a_command_shaped_action_reports_redacted_scope() {
        // codex M12-R3-P1: a SCOPE rule matching a command-shaped action's (forged)
        // scope must report the fixed label in the verdict — never the caller scope,
        // which the hook surfaces in a deny message.
        let secret = "ghp_aaaaaaaaaaaaaaaaaaaaaa";
        let p = policy(vec![scope_rule("freeze", "knowledge/**", Response::Deny, Reversibility::Reversible)]);
        let forged = Action {
            tool_kind: "exec".into(),
            scopes: vec![format!("knowledge/{secret}")], // forged secret in scope
            command: Some("echo hi".into()),
            ..Default::default()
        };
        match judge(&p, &forged) {
            Verdict::Deny { scope, .. } => {
                assert_eq!(scope, "exec:bash", "the reported scope is redacted");
                assert!(!scope.contains(secret));
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    // ── exempt audit (the declared-downgrade ledger) ──────────────────────────
    #[test]
    fn exempted_rules_reports_a_suppressed_critique() {
        // A recoverable critique whose exempt fired is invisible to `judge` (Affirm) —
        // `exempted_rules` is the audit surface that names it.
        let p = policy(vec![cmd_rule(
            "lsp", r"\bgrep\b.*\.rs", Some(r"BYPASS=[a-z-]{4,}"), Response::Critique, Reversibility::Reversible,
        )]);
        let a = bash_action("BYPASS=stdout-grep grep foo x.rs");
        assert_eq!(judge(&p, &a), Verdict::Affirm, "exempt suppresses the critique");
        assert_eq!(exempted_rules(&p, &a), vec!["lsp".to_string()], "the suppression is auditable");
    }

    #[test]
    fn exempted_rules_empty_without_a_real_suppression() {
        let p = policy(vec![
            cmd_rule("lsp", r"\bgrep\b.*\.rs", Some(r"BYPASS=[a-z-]{4,}"), Response::Critique, Reversibility::Reversible),
            cmd_rule("freeze", r"ghp_[A-Za-z0-9]{20,}", Some(r"PLEASE"), Response::Deny, Reversibility::Irreversible),
        ]);
        // no exempt token -> the rule FIRES (critique), nothing was suppressed.
        assert!(exempted_rules(&p, &bash_action("grep foo x.rs")).is_empty());
        // pattern does not match at all -> nothing to suppress.
        assert!(exempted_rules(&p, &bash_action("ls -la")).is_empty());
        // a DENY rule ignores exempt (invariant 4) -> never reported as exempted.
        assert!(exempted_rules(&p, &bash_action("PLEASE echo ghp_aaaaaaaaaaaaaaaaaaaaaa")).is_empty());
        // a non-command action has nothing to exempt.
        assert!(exempted_rules(&p, &write_action(&["src/main.rs"])).is_empty());
    }

    #[test]
    fn scope_and_command_rules_coexist_deny_precedence() {
        // a command critique + a scope deny on the same action: deny wins.
        let p = policy(vec![
            cmd_rule("lsp", r"grep", None, Response::Critique, Reversibility::Reversible),
            scope_rule("freeze", "exec:bash", Response::Deny, Reversibility::Reversible),
        ]);
        assert!(judge(&p, &bash_action("grep foo x.rs")).is_deny());
    }

    // ── warn: the non-blocking tier (whitepaper action 3 = allow / warn / block) ──
    #[test]
    fn warn_rule_yields_a_warn_verdict_carrying_its_prose() {
        let p = policy(vec![cmd_rule("lsp", r"grep", None, Response::Warn, Reversibility::Reversible)]);
        match judge(&p, &bash_action("grep foo x.rs")) {
            Verdict::Warn { rule_id, scope, message, suggestion } => {
                assert_eq!(rule_id, "lsp");
                // the scope stays REDACTED for a command-shaped action, like every other tier.
                assert_eq!(scope, COMMAND_SCOPE_LABEL);
                assert_eq!(message, "rule lsp");
                assert_eq!(suggestion, "fix lsp");
            }
            other => panic!("expected warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_ranks_below_critique_and_deny() {
        // A warn must never mask a rule that actually stops the action.
        let crit = policy(vec![
            cmd_rule("warner", r"grep", None, Response::Warn, Reversibility::Reversible),
            cmd_rule("critic", r"grep", None, Response::Critique, Reversibility::Reversible),
        ]);
        assert!(judge(&crit, &bash_action("grep foo x.rs")).is_critique());
        let den = policy(vec![
            cmd_rule("warner", r"grep", None, Response::Warn, Reversibility::Reversible),
            cmd_rule("denier", r"grep", None, Response::Deny, Reversibility::Reversible),
        ]);
        assert!(judge(&den, &bash_action("grep foo x.rs")).is_deny());
    }

    #[test]
    fn irreversible_warn_escalates_to_deny() {
        // Irreversibility is the deny frontier for EVERY tier, not just critique — a
        // non-blocking tier must not become a hole in it.
        let p = policy(vec![scope_rule("send", "exec:bash", Response::Warn, Reversibility::Irreversible)]);
        let a = Action { tool_kind: "exec".into(), scopes: vec!["exec:bash".into()], ..Default::default() };
        assert!(judge(&p, &a).is_deny());
    }

    #[test]
    fn warn_honors_exempt_and_the_suppression_is_audited() {
        // `exempt` is a recoverable-tier affordance; a warn is recoverable, so an exempt
        // on a warn rule must suppress it AND show up in the audit surface — otherwise a
        // sovereign-written exempt would be silently ignored.
        let p = policy(vec![cmd_rule(
            "lsp", r"\bgrep\b.*\.rs", Some(r"BYPASS=[a-z-]{4,}"), Response::Warn, Reversibility::Reversible,
        )]);
        let a = bash_action("BYPASS=stdout-grep grep foo x.rs");
        assert_eq!(judge(&p, &a), Verdict::Affirm, "exempt suppresses the warn");
        assert_eq!(exempted_rules(&p, &a), vec!["lsp".to_string()], "the suppression is auditable");
    }

    #[test]
    fn verdict_and_response_tags_are_stable() {
        // The receipt tag is the audit vocabulary — `warn` must be its own countable word.
        assert_eq!(Response::Warn.tag(), "warn");
        assert_eq!(Response::Critique.tag(), "critique");
        assert_eq!(Response::Deny.tag(), "deny");
        let w = Verdict::Warn { rule_id: "r".into(), scope: "s".into(), message: "m".into(), suggestion: "x".into() };
        assert_eq!(w.tag(), "warn");
        assert!(w.is_warn());
        assert!(!w.is_critique() && !w.is_deny() && !w.is_affirm());
    }
}
