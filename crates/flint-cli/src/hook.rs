//! `flint hook` — the PIVOT runtime gate (Canon-backed, kernel-free).
//!
//! Reads a harness PreToolUse hook JSON on stdin, derives a neutral [`Action`] via the
//! per-harness adapter, judges it against the SIGNED Canon policy (root of trust §3.5.1),
//! records a redacted receipt to the obs-log, and — only in `--mode block` — ENFORCES the
//! four-valued [`Verdict`] (see [`enforce`], the pure seam both this path and its tests
//! go through):
//!   - **Affirm**   -> emit nothing (the call proceeds untouched).
//!   - **Warn**     -> exit 0 + `hookSpecificOutput.additionalContext` (the call PROCEEDS;
//!     the harness shows the text to the agent next to the tool result). Deliberately no
//!     `permissionDecision`: `"allow"` would AUTHORIZE a call the operator's own permission
//!     flow might have stopped, and flint has no allow-authorizing output (invariant ④).
//!   - **Critique** -> exit 0 + a deny envelope whose reason carries the message
//!     and suggestion, so the agent gets the recovery path and self-corrects.
//!   - **Deny**     -> exit 0 + a deny envelope with the sovereign-freeze wording.
//!
//! The DENY ENVELOPE is per-harness (see [`deny_envelope`]): Claude Code and Codex block on
//! `hookSpecificOutput.permissionDecision`, Grok blocks only on `{"decision":"deny"}`.
//!
//! NEITHER blocking tier uses the exit code, which is a MEASURED decision (2026-08-08):
//! codex on Windows classifies a non-zero PreToolUse exit as a hook ERROR, tells the model
//! nothing, and runs the command — while the identical binary and verdict block correctly
//! on macOS. See [`enforce`].
//!
//! FAIL-CLOSED (reframe-and-diff §3.5.1 / §3.6): the policy load (signature verify ->
//! fallible Canon reduce) returning `Err` is a gate error; in block+closed it emits a
//! deny rather than letting an unverified/malformed Canon silently Affirm. The new
//! Canon->policy reduce is FALLIBLE (any malformed rule -> Err), so a malformed deny never
//! vanishes into Affirm-by-absence (codex r3). Record-only never blocks (advisory by
//! design — fail-open residual is explicit).

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::{json, Value};

use flint_core::budget;
use flint_core::canon;
use flint_core::config::FlintConfig;
use flint_core::harness::{derive_action, Harness};
use flint_core::model_veto::{judge_with_veto, NoopVeto};
use flint_core::obslog::receipt_line;
use flint_core::touchstone::{Action, TouchstonePolicy, Verdict};
use flint_core::trust::{CanonTrust, ManifestSignedCanon};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Record,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailMode {
    Open,
    Closed,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "record" => Ok(Mode::Record),
            "block" => Ok(Mode::Block),
            other => anyhow::bail!("--mode must be record|block, got {other}"),
        }
    }
}

impl FailMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "closed" => Ok(FailMode::Closed),
            "open" => Ok(FailMode::Open),
            other => anyhow::bail!("--fail-mode must be closed|open, got {other}"),
        }
    }
}

/// Verify + reduce the signed Canon into the load-bearing [`TouchstonePolicy`]. FALLIBLE
/// on every surface (bad signature / scope / rollback / hash, OR a malformed rule).
/// The sovereign trust with the PERSISTENT anti-rollback floor folded in: `min_epoch` is
/// raised to `max(config min_epoch, durable high-water epoch)`, so a checkout of an old but
/// validly-signed manifest is rejected even when config `min_epoch` is 0 (the common case).
/// `max()` so it only ever tightens; a missing/corrupt floor contributes 0 (fail-soft to the
/// config baseline). Weak-tier honest boundary: see `[trust] epoch_floor` in the config.
fn effective_trust(config: &FlintConfig) -> flint_core::trust::SovereignTrust {
    let mut trust = config.sovereign_trust();
    if let Some(floor_path) = &config.trust.epoch_floor {
        trust.min_epoch = trust.min_epoch.max(flint_core::trust::read_epoch_floor(floor_path));
    }
    trust
}

pub fn load_policy(config: &FlintConfig) -> Result<TouchstonePolicy, String> {
    let canon = ManifestSignedCanon::new(config.canon_root.clone());
    let lb = canon
        .resolve_load_bearing(&effective_trust(config))
        .map_err(|e| e.to_string())?;
    canon::reduce(&lb.files).map_err(|e| e.to_string())
}

/// Run the hook over a PreToolUse JSON string for `harness`.
pub fn run_hook(config: &FlintConfig, harness: Harness, mode: Mode, fail_mode: FailMode, stdin: &str) -> i32 {
    let fail_closed_block = mode == Mode::Block && fail_mode == FailMode::Closed;

    let mut action = match parse_action(harness, stdin) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("flint hook: cannot derive target: {e}");
            if fail_closed_block {
                emit_deny(harness, &format!("flint hook: undeterminable target, fail-closed ({e})"));
            }
            return 0;
        }
    };

    // Resolve file-path scopes against the workspace root (codex P1 r2): a harness reports
    // ABSOLUTE paths (the normal Claude `file_path` case) / `..`-escaping spellings; the
    // relative gate globs only bite once paths are resolved to the workspace they are
    // written against. Order: the operator's config, then the root the HARNESS itself
    // reported on stdin (Grok alone does — and on Windows the hook process's cwd is not
    // guaranteed to be the workspace, so its own word beats a guess), then the cwd (the
    // harness runs us at the project root). With none of the three we cannot locate
    // targets -> fail-closed.
    let workspace_root = config
        .workspace_root
        .clone()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| harness_workspace_root(harness, stdin))
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));
    match workspace_root {
        Some(root) => flint_core::harness::relativize_scopes(&mut action, &root),
        None => {
            eprintln!("flint hook: cannot determine workspace root");
            if fail_closed_block {
                emit_deny(harness, "flint hook: no workspace root, fail-closed");
            }
            return 0;
        }
    }

    // P3: read the cumulative REAL measured energy (tokens) from the sidecar. The
    // PreToolUse hook has no token field, but the TRANSCRIPT it points to does — a
    // Stop/PostToolUse hook reads the new turn's tokens from the transcript tail and calls
    // `flint budget record` (the universal source; jack proves the transcript is parseable
    // at scale). `None` when no sidecar is configured. flint never fabricates tokens.
    action.energy = config.budget.sidecar.as_ref().map(|p| budget::read_cumulative(p));

    let policy = match load_policy(config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("flint hook: gate error: {e}");
            if fail_closed_block {
                emit_deny(harness, "flint hook: gate unavailable (fail-closed)");
            }
            return 0;
        }
    };

    let verdict = judge_with_veto(&policy, &action, &NoopVeto);
    // P3: fold the energy budget onto the verdict (veto-only: Affirm -> Critique at/over
    // the threshold; never a deny, never a relax).
    let threshold = (config.budget.critique_threshold > 0).then_some(config.budget.critique_threshold);
    let verdict = budget::energy_fold(verdict, action.energy, threshold);
    record_receipt(config, harness, &action, &verdict);
    record_exempt_receipts(config, harness, &policy, &action);

    if mode != Mode::Block {
        return 0;
    }
    let effect = enforce(harness, &verdict);
    if let Some(out) = effect.stdout_json {
        println!("{out}");
    }
    if let Some(err) = effect.stderr {
        eprintln!("{err}");
    }
    effect.exit_code
}

/// The harness-facing effect of a [`Verdict`], as data. Kept pure and separate from the
/// I/O so the enforcement contract has ONE seam that both `run_hook` and its tests go
/// through — the alternative (asserting on captured stdout) would test a different path
/// than the one the harness actually drives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enforcement {
    pub exit_code: i32,
    pub stdout_json: Option<Value>,
    pub stderr: Option<String>,
}

/// Map a verdict onto the PreToolUse wire contract for `harness`.
///
/// - **Affirm**   -> nothing at all.
/// - **Warn**     -> exit 0 + `hookSpecificOutput.additionalContext` (Claude Code wraps it
///   in a system reminder next to the tool result, so the agent hears the rule while the
///   call PROCEEDS). Deliberately carries NO `permissionDecision`: `"allow"` would
///   auto-approve a call the operator's own permission flow might have stopped, and flint
///   has no allow-authorizing output (invariant ④). Omitting the field leaves the normal
///   permission flow exactly as it was.
/// - **Critique** -> a [`deny_envelope`] (exit 0) whose reason carries the message
///   and the suggestion, so the agent gets the recovery path and self-corrects.
/// - **Deny**     -> a [`deny_envelope`] (exit 0) with the sovereign-freeze wording.
///
/// Only the BLOCKING envelope forks per harness; the tier wording and the warn channel are
/// identical everywhere, so a verdict reads the same in every transcript.
///
/// BOTH blocking tiers ride the JSON channel and NEITHER depends on the exit code. That is
/// a measured decision, not a stylistic one. Critique used to exit 2 with stderr, which
/// works on Claude Code and on codex/macOS — and silently does NOT work on codex/Windows,
/// where the same exit 2 is classified as a hook ERROR ("hook exited with code 1"), no text
/// reaches the model, and the command RUNS. Identical binary, identical verdict, identical
/// receipt; only the platform differs. The `permissionDecision` JSON was verified to block
/// AND to deliver its full reason on claude/macOS, codex/macOS and codex/Windows, so it is
/// the only channel that enforces everywhere. The tiers stay distinguishable where it
/// matters: the receipt's verdict tag, and the reason's own wording.
///
/// WARN DELIVERY IS DOCUMENTED, NOT MEASURED. Claude Code and Codex both document
/// `hookSpecificOutput.additionalContext` as model-visible context that does not block
/// (Codex's wording: "to add model-visible context without blocking, return
/// `hookSpecificOutput.additionalContext`"). What was MEASURED on all three surfaces is
/// `permissionDecision`, not this field. The exit-code defect gives no specific reason to
/// distrust it — a warn exits 0 and rides the same JSON envelope that is proven to arrive —
/// but note the failure mode is structurally invisible: the call would correctly proceed,
/// the agent would silently miss the warning, and the receipt would still say `warn`.
/// Nothing flint can see distinguishes a delivered warn from a swallowed one. Treat a warn
/// as load-bearing on a given harness only after a live sentinel probe there. ON GROK THE
/// SWALLOW IS NOW MEASURED, NOT SUSPECTED: probe C (2026-08-20, grok 1.0.5) sent exactly
/// this envelope and the model reported `NO_HOOK_TEXT_SEEN` — the call correctly proceeded
/// and the receipt was written, but the prose never reached the model. flint keeps emitting
/// it (Grok fail-opens on any JSON carrying no `decision`, so it costs nothing and the
/// channel may start working) as a KNOWN, NON-LOAD-BEARING DEBT: never treat a grok warn as
/// delivered.
pub fn enforce(harness: Harness, verdict: &Verdict) -> Enforcement {
    match verdict {
        Verdict::Affirm => Enforcement { exit_code: 0, stdout_json: None, stderr: None },
        Verdict::Warn { rule_id, scope, message, suggestion, .. } => {
            let fix = if suggestion.is_empty() { String::new() } else { format!(" Suggestion: {suggestion}") };
            let context = format!("flint warn [{rule_id}] on {scope}: {message}.{fix} This is a warning, not a block — the call proceeds and the passage is receipted.");
            Enforcement {
                exit_code: 0,
                stdout_json: Some(json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "additionalContext": context,
                    }
                })),
                stderr: None,
            }
        }
        Verdict::Critique { scope, message, suggestion, .. } => {
            let fix = if suggestion.is_empty() { String::new() } else { format!(" Suggestion: {suggestion}") };
            Enforcement {
                exit_code: 0,
                stdout_json: Some(deny_envelope(harness, &format!("flint critique on {scope}: {message}.{fix}"))),
                stderr: None,
            }
        }
        Verdict::Deny { scope, reason, .. } => Enforcement {
            exit_code: 0,
            stdout_json: Some(deny_envelope(harness, &format!("flint: sovereign freeze on {scope} ({reason})"))),
            stderr: None,
        },
    }
}

/// Parse a harness PreToolUse hook JSON and derive the [`Action`].
pub fn parse_action(harness: Harness, stdin: &str) -> Result<Action, String> {
    let v: Value = serde_json::from_str(stdin.trim()).map_err(|e| format!("invalid hook JSON: {e}"))?;
    derive_action(harness, &v)
}

/// The workspace root the harness itself declared on stdin, when its payload carries one
/// (Grok's `workspaceRoot`; see `flint_core::harness::stdin_workspace_root`). Re-parses the
/// stdin JSON rather than threading a second return through the adapter — this runs once
/// per hook call, after `parse_action` already proved the payload parses.
fn harness_workspace_root(harness: Harness, stdin: &str) -> Option<String> {
    let v: Value = serde_json::from_str(stdin.trim()).ok()?;
    flint_core::harness::stdin_workspace_root(harness, &v)
}

/// Print the active (signed) policy for operator inspection (`--show`). Returns Err on a
/// gate error (an operator query — a setup failure here is a plain error, not a deny).
pub fn show_policy(config: &FlintConfig) -> Result<(), String> {
    let policy = load_policy(config)?;
    for r in &policy.rules {
        println!("RULE\t{}\t{}\t{}\t{}", r.id, r.matcher.describe(), r.response.tag(), r.message.lines().next().unwrap_or(""));
    }
    Ok(())
}

/// Load the signed Canon and pair each rule with its DERIVED evidence tier (reproduced iff
/// a signed `promotion/<id>.md` record + matching `fixtures/<id>.md` exist — re-derivable,
/// reframe-and-diff §3.7). For `canon list`.
pub fn load_canon_tiered(config: &FlintConfig) -> Result<Vec<(flint_core::touchstone::GateRule, flint_core::canon::EvidenceTier)>, String> {
    use std::path::PathBuf;
    let canon = ManifestSignedCanon::new(config.canon_root.clone());
    let lb = canon.resolve_load_bearing(&effective_trust(config)).map_err(|e| e.to_string())?;
    let policy = canon::reduce(&lb.files).map_err(|e| e.to_string())?;
    let tiered = policy
        .rules
        .into_iter()
        .map(|r| {
            // A signed promotion record = the sovereign PICKED this rule for promotion; the
            // tier is then RE-DERIVED by re-running the gate on the signed rule + fixture
            // (codex P2 / §3.7 — never trust the record's say-so).
            let prom_present = lb.files.contains_key(&PathBuf::from(format!("promotion/{}.md", r.id)));
            let fx = lb.files.get(&PathBuf::from(format!("fixtures/{}.md", r.id))).map(|v| v.as_slice());
            let tier = flint_core::forge::effective_tier(&r, prom_present, fx);
            (r, tier)
        })
        .collect();
    Ok(tiered)
}

/// Best-effort redacted receipt append (heart §2): NEVER changes the verdict; a write
/// failure is ignored. No-op when `obs_log` is unset.
fn record_receipt(config: &FlintConfig, harness: Harness, action: &Action, verdict: &Verdict) {
    let Some(path) = &config.obs_log else { return };
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = receipt_line(harness.tag(), action, verdict, ts);
    let _ = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(f, "{line}")
    })();
}

/// Best-effort audit of DECLARED DOWNGRADES (heart §2 + pit declared-bypass-creep): one
/// `exempt` receipt per rule whose critique this command suppressed via its exempt regex.
/// Never changes the verdict; a write failure is ignored. No-op when `obs_log` is unset.
fn record_exempt_receipts(config: &FlintConfig, harness: Harness, policy: &TouchstonePolicy, action: &Action) {
    let Some(path) = &config.obs_log else { return };
    let exempted = flint_core::touchstone::exempted_rules(policy, action);
    if exempted.is_empty() {
        return;
    }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        for rid in &exempted {
            writeln!(f, "{}", flint_core::obslog::exempt_receipt_line(harness.tag(), action, rid, ts))?;
        }
        Ok(())
    })();
}

/// Fail-closed deny for a gate-SETUP failure (config load / stdin read) BEFORE `run_hook`.
/// Takes the harness because the deny has to be spoken in that harness's envelope — a
/// `permissionDecision` deny handed to Grok is silently ignored, which would make
/// `--fail-mode closed` fail-closed in name only.
pub fn deny_on_setup_failure(harness: Harness, mode: Mode, fail_mode: FailMode, reason: &str) {
    eprintln!("flint hook: setup error: {reason}");
    if mode == Mode::Block && fail_mode == FailMode::Closed {
        emit_deny(harness, &format!("flint hook: gate unavailable at setup, fail-closed ({reason})"));
    }
}

/// The blocking envelope for `harness`. MEASURED 2026-08-20 (grok 1.0.5, probes A/B in
/// `~/.flint/grok-wire/wire-facts.md`): Grok IGNORES `hookSpecificOutput.permissionDecision`
/// entirely and lets the call run (probe A — the grep executed), and honours ONLY
/// `{"decision":"deny","reason":…}`, whose reason text reaches the model verbatim as
/// `Hook denied: <reason>` (probe B). Claude Code and Codex are the other way round and are
/// left byte-for-byte unchanged. The exit code is not the channel on any of the three.
fn deny_envelope(harness: Harness, reason: &str) -> Value {
    match harness {
        Harness::Claude | Harness::Codex => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        }),
        Harness::Grok => json!({ "decision": "deny", "reason": reason }),
    }
}

fn emit_deny(harness: Harness, reason: &str) {
    println!("{}", deny_envelope(harness, reason));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mode_and_failmode_parse() {
        assert_eq!(Mode::parse("record").unwrap(), Mode::Record);
        assert_eq!(Mode::parse("block").unwrap(), Mode::Block);
        assert!(Mode::parse("x").is_err());
        assert_eq!(FailMode::parse("closed").unwrap(), FailMode::Closed);
        assert!(FailMode::parse("x").is_err());
    }

    #[test]
    fn parse_action_routes_by_harness() {
        let claude = json!({ "tool_name": "Bash", "tool_input": { "command": "grep x y.rs" } }).to_string();
        let a = parse_action(Harness::Claude, &claude).unwrap();
        assert_eq!(a.command.as_deref(), Some("grep x y.rs"));
        // malformed JSON -> Err (fail-closed in block).
        assert!(parse_action(Harness::Claude, "not json {{{").is_err());
        // codex apply_patch with no target -> Err.
        let bad = json!({ "tool_name": "apply_patch", "tool_input": { "command": "*** Begin Patch\n*** End Patch\n" } }).to_string();
        assert!(parse_action(Harness::Codex, &bad).is_err());
    }

    #[test]
    fn warn_speaks_to_the_model_without_authorizing_the_call() {
        let v = Verdict::Warn {
            rule_id: "lsp-over-grep".into(),
            scope: "exec:bash".into(),
            message: "Code navigation defaults to LSP".into(),
            suggestion: "use goToDefinition".into(),
        };
        let e = enforce(Harness::Claude, &v);
        assert_eq!(e.exit_code, 0, "a warn never blocks the call");
        assert!(e.stderr.is_none(), "on exit 0 stderr reaches nobody — the prose must ride the JSON");
        let out = e.stdout_json.expect("a warn must actually speak");
        let hso = &out["hookSpecificOutput"];
        assert_eq!(hso["hookEventName"], "PreToolUse");
        let ctx = hso["additionalContext"].as_str().expect("additionalContext carries the prose");
        assert!(ctx.contains("lsp-over-grep"), "the receipt-able rule id is named: {ctx}");
        assert!(ctx.contains("Code navigation defaults to LSP"), "the rule's own message: {ctx}");
        assert!(ctx.contains("use goToDefinition"), "the recovery path: {ctx}");
        // INVARIANT ④: flint has NO allow-authorizing output. `permissionDecision: "allow"`
        // would auto-approve a call the operator's own permission flow might have stopped —
        // a warn must let the normal flow run, not grant it.
        assert!(
            hso.get("permissionDecision").is_none(),
            "a warn must never grant permission, got {hso}"
        );
    }

    #[test]
    fn both_blocking_tiers_ride_the_json_channel_never_the_exit_code() {
        // MEASURED 2026-08-08, the reason this is not exit 2: codex on Windows receives a
        // non-zero PreToolUse exit, classifies it as a hook ERROR rather than a block,
        // reports "hook exited with code 1", delivers no text to the model, and RUNS THE
        // COMMAND. Same flint binary, same verdict, same receipt as macOS, where the exact
        // same exit 2 blocks correctly. The `permissionDecision` JSON was verified to block
        // and to deliver its full reason on all three surfaces (claude/mac, codex/mac,
        // codex/Windows), so it is the only channel that actually enforces everywhere.
        let c = enforce(Harness::Claude, &Verdict::Critique {
            rule_id: "r".into(),
            scope: "exec:bash".into(),
            message: "m".into(),
            suggestion: "s".into(),
        });
        assert_eq!(c.exit_code, 0, "a blocking tier must not depend on the exit code");
        assert!(c.stderr.is_none(), "stderr on exit 0 reaches nobody");
        let hso = &c.stdout_json.expect("critique must speak on the JSON channel")["hookSpecificOutput"];
        assert_eq!(hso["permissionDecision"], "deny");
        let why = hso["permissionDecisionReason"].as_str().unwrap();
        assert!(why.contains("critique"), "the tier is named, not disguised as a freeze: {why}");
        assert!(why.contains('m') && why.contains('s'), "message + suggestion both ride along: {why}");

        let d = enforce(Harness::Claude, &Verdict::Deny { rule_id: "r".into(), scope: "exec:bash".into(), reason: "no".into() });
        assert_eq!(d.exit_code, 0);
        let dhso = &d.stdout_json.expect("deny speaks on the JSON channel")["hookSpecificOutput"];
        assert_eq!(dhso["permissionDecision"], "deny");
        assert!(
            dhso["permissionDecisionReason"].as_str().unwrap().contains("freeze"),
            "deny keeps its own harsher wording — the receipt is not the only place the tiers differ"
        );

        assert_eq!(enforce(Harness::Claude, &Verdict::Affirm), Enforcement { exit_code: 0, stdout_json: None, stderr: None });
    }

    fn critique() -> Verdict {
        Verdict::Critique {
            rule_id: "lsp-over-grep".into(),
            scope: "exec:bash".into(),
            message: "Code navigation defaults to LSP".into(),
            suggestion: "use goToDefinition".into(),
        }
    }

    #[test]
    fn one_verdict_two_envelopes_grok_speaks_decision_not_permissiondecision() {
        // MEASURED 2026-08-20 (grok 1.0.5): Grok IGNORES `permissionDecision` and runs the
        // command (probe A); it blocks only on `{"decision":"deny"}` (probe B). SAME verdict,
        // SAME wording — only the envelope forks.
        let claude = enforce(Harness::Claude, &critique()).stdout_json.expect("claude speaks");
        assert_eq!(claude["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(claude.get("decision").is_none(), "claude keeps its own envelope: {claude}");

        let grok = enforce(Harness::Grok, &critique());
        assert_eq!(grok.exit_code, 0, "the exit code is the channel on no harness");
        assert!(grok.stderr.is_none());
        let out = grok.stdout_json.expect("grok must speak the shape it honours");
        assert_eq!(out["decision"], "deny");
        assert!(
            out.get("hookSpecificOutput").is_none(),
            "the ignored envelope must not ride along: {out}"
        );
        let why = out["reason"].as_str().expect("the reason reaches the model verbatim");
        assert!(why.contains("critique"), "the tier is named, not disguised as a freeze: {why}");
        assert!(why.contains("Code navigation defaults to LSP"), "{why}");
        assert!(why.contains("use goToDefinition"), "the recovery path rides along: {why}");
        // INVARIANT ④: no path may authorize. `allow` appears in neither envelope, ever.
        assert!(!out.to_string().contains("allow"), "flint has no allow-authorizing output: {out}");

        // Deny keeps its harsher wording on grok too.
        let d = enforce(Harness::Grok, &Verdict::Deny { rule_id: "r".into(), scope: "knowledge/secret/key".into(), reason: "no".into() })
            .stdout_json
            .expect("grok deny speaks");
        assert_eq!(d["decision"], "deny");
        assert!(d["reason"].as_str().unwrap().contains("freeze"), "{d}");
    }

    #[test]
    fn grok_warn_carries_no_decision_and_affirm_is_silent_on_both() {
        // A warn must never block on ANY harness. Grok fail-opens on JSON with no
        // `decision` key, so the (measured non-delivering) additionalContext is harmless.
        let w = enforce(Harness::Grok, &Verdict::Warn {
            rule_id: "prefer-fd".into(),
            scope: "exec:bash".into(),
            message: "Prefer fd".into(),
            suggestion: String::new(),
        });
        assert_eq!(w.exit_code, 0);
        let out = w.stdout_json.expect("the warn still rides the JSON channel");
        assert!(out.get("decision").is_none(), "a warn must never carry a deny decision: {out}");
        assert!(out.get("permissionDecision").is_none(), "{out}");
        assert!(out["hookSpecificOutput"]["additionalContext"].is_string(), "{out}");

        for h in [Harness::Claude, Harness::Codex, Harness::Grok] {
            assert_eq!(
                enforce(h, &Verdict::Affirm),
                Enforcement { exit_code: 0, stdout_json: None, stderr: None },
                "affirm emits nothing on {}",
                h.tag()
            );
        }
    }

    #[test]
    fn claude_and_codex_envelopes_are_pinned_to_their_pre_grok_values() {
        // Outside-voice round-1 P1: "claude/codex unchanged" was previously asserted by
        // substring, which a field rename/move could slip past. Pin the COMPLETE JSON
        // value (field set + wording) of every speaking tier on both legacy harnesses —
        // these literals are the envelopes that shipped BEFORE the grok fork existed.
        let warn = Verdict::Warn {
            rule_id: "lsp-over-grep".into(),
            scope: "exec:bash".into(),
            message: "Code navigation defaults to LSP".into(),
            suggestion: "use goToDefinition".into(),
        };
        let critique = Verdict::Critique {
            rule_id: "r".into(),
            scope: "exec:bash".into(),
            message: "m".into(),
            suggestion: "s".into(),
        };
        let deny = Verdict::Deny {
            rule_id: "r".into(),
            scope: "knowledge/secret/key".into(),
            reason: "no".into(),
        };
        for h in [Harness::Claude, Harness::Codex] {
            assert_eq!(
                enforce(h, &warn).stdout_json.unwrap(),
                json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "additionalContext": "flint warn [lsp-over-grep] on exec:bash: Code navigation defaults to LSP. Suggestion: use goToDefinition This is a warning, not a block — the call proceeds and the passage is receipted.",
                    }
                }),
                "warn envelope drifted on {}",
                h.tag()
            );
            assert_eq!(
                enforce(h, &critique).stdout_json.unwrap(),
                json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": "flint critique on exec:bash: m. Suggestion: s",
                    }
                }),
                "critique envelope drifted on {}",
                h.tag()
            );
            assert_eq!(
                enforce(h, &deny).stdout_json.unwrap(),
                json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": "flint: sovereign freeze on knowledge/secret/key (no)",
                    }
                }),
                "deny envelope drifted on {}",
                h.tag()
            );
        }
    }
}
