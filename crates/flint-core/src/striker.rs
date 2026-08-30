//! Striker — the canonical->per-harness compiler (PIVOT Contract B · reframe-and-diff
//! §3.5.1 / §05). ONE Canon, compiled into the wiring for MANY harnesses. This is the
//! flint differentiator over TRACE (which copies rule variants per harness): the rules
//! live ONCE in Canon (read at runtime by `flint hook`), and Striker emits only the
//! per-harness HOOK WIRING that routes each harness's tool call into the one flint judge.
//!
//! Model A (the sound choice): one judge in Rust (`touchstone`, tested), per-harness
//! adapters (`harness`), and a thin wiring emitter here. The emitted config points the
//! harness at `flint hook --harness <x> --config <flint.toml>`; the binary reads Canon +
//! verifies its signature (`trust`) at runtime. The compiler is "throwaway" (§05): change
//! harness = re-emit; Canon never changes.
//!
//! Per-harness reality (verified against both vendors' official hook docs 2026-06):
//!   - Claude Code: PreToolUse enforces scope + command rules (clean file_path / command).
//!   - Codex: PreToolUse enforces command rules (Bash) reliably; file-scope governance
//!     also goes out as AGENTS.md advisory (apply_patch interception is flaky mid-2026 —
//!     honest boundary), so [`codex_agents_md`] emits the scope rules as guidance too.

use serde_json::{json, Value};

use crate::touchstone::{AdvisoryRule, Matcher, Response, TouchstonePolicy};

/// Inputs to the compiler: where the flint binary is and where its (entrypoint-boundary
/// protected) runtime config lives. The Canon rules themselves are NOT embedded — they
/// are read at runtime, so the wiring is stable across Canon edits.
#[derive(Debug, Clone)]
pub struct CompileParams {
    /// The installed flint binary (absolute path recommended — the harness runs it).
    pub flint_bin: String,
    /// The runtime config (flint.toml: trust + canon_root). Pinned with the harness install.
    pub config_path: String,
    /// The enforcement mode emitted into the wiring: `block` (the default — actually ENFORCE
    /// the verdict) or `record` (advisory, dogfood-first). CRITICAL (codex P1): the bare
    /// `flint hook` CLI defaults to `record`, so a wiring that omitted `--mode` installed a
    /// gate that only LOGGED and never blocked. The compiled wiring is the enforcement
    /// install, so it pins `--mode` explicitly; the operator can compile with `record` to
    /// run advisory-first before flipping to `block`.
    pub mode: String,
}

impl CompileParams {
    /// The hook command for a harness. Pins `--mode` (codex P1: never rely on the CLI's
    /// `record` default for the installed gate) and `--fail-mode closed` (deny on a gate
    /// error — explicit so a future CLI default change can't silently weaken the install).
    fn hook_command(&self, harness: &str) -> String {
        format!(
            "{} hook --harness {} --config {} --mode {} --fail-mode closed",
            self.flint_bin, harness, self.config_path, self.mode
        )
    }

    /// The same command with the two PATH arguments double-quoted. Grok runs the command
    /// through a shell and is fail-OPEN on a hook that fails to launch, so a space in
    /// `flint_bin`/`config_path` (routine on Windows) would split the command and silently
    /// disarm the gate (outside-voice round-1 P0). THE QUOTED SHAPE IS PER-PLATFORM:
    /// Grok's Windows shell is PowerShell, where a line STARTING with a quoted string is
    /// an expression, not a command — `"C:\..\flint.exe" hook ...` dies with ParserError
    /// and the gate silently fail-opens (measured 2026-08-20, Windows sentinel: both hooks
    /// exit 1, `hook failed; ignoring (fail-open)`). PowerShell needs the call operator:
    /// `& "path" args...`. On sh the bare quoted path IS the command and `&` would mean
    /// "background job", so the two shapes cannot be unified. `flint compile` runs on the
    /// machine it wires, so the build target selects the right shape. Claude/Codex
    /// wirings keep the unquoted form they shipped with — same latent surface, but their
    /// harnesses surface a hook error where Grok stays silent, and re-quoting them is
    /// outside this change's blast radius.
    fn hook_command_quoted(&self, harness: &str, windows: bool) -> String {
        let call = if windows { "& " } else { "" };
        format!(
            "{call}\"{}\" hook --harness {} --config \"{}\" --mode {} --fail-mode closed",
            self.flint_bin, harness, self.config_path, self.mode
        )
    }
}

/// The Claude Code `.claude/settings.json` fragment registering flint as a PreToolUse
/// hook over ALL tools (`matcher: "*"`). Merge into the chosen settings scope.
pub fn claude_settings(p: &CompileParams) -> Value {
    json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        { "type": "command", "command": p.hook_command("claude"), "timeout": 600 }
                    ]
                }
            ]
        }
    })
}

/// The Codex `~/.codex/hooks.json` fragment. PreToolUse over Bash (command rules — reliable)
/// and apply_patch (file-scope — best-effort; also mirrored to AGENTS.md advisory).
pub fn codex_hooks(p: &CompileParams) -> Value {
    let entry = json!({ "type": "command", "command": p.hook_command("codex"), "timeout": 30 });
    json!({
        "hooks": {
            "PreToolUse": [
                { "matcher": "Bash", "hooks": [ entry.clone() ] },
                { "matcher": "apply_patch", "hooks": [ entry ] }
            ]
        }
    })
}

/// The Grok `~/.grok/hooks/flint.json` wiring: ONE PreToolUse group covering every tool.
///
/// The group deliberately carries NO `matcher` key, and that omission is BEHAVIOR, not
/// style: Grok's matcher is a REGEX, and an omitted matcher is its documented "match every
/// tool". The Claude spelling `"*"` is an INVALID regex there — writing it would either
/// throw the group away or make it match nothing, and because Grok is fail-OPEN a dead
/// matcher looks exactly like a quiet gate. Never emit a matcher here.
pub fn grok_hooks(p: &CompileParams) -> Value {
    grok_hooks_for(p, cfg!(windows))
}

/// [`grok_hooks`] with the platform made explicit, so BOTH command shapes stay testable
/// from any build host. `windows` selects the PowerShell call-operator form (see
/// [`CompileParams::hook_command_quoted`]).
pub fn grok_hooks_for(p: &CompileParams, windows: bool) -> Value {
    json!({
        "hooks": {
            "PreToolUse": [
                {
                    "hooks": [
                        { "type": "command", "command": p.hook_command_quoted("grok", windows), "timeout": 600 }
                    ]
                }
            ]
        }
    })
}

/// AGENTS.md advisory for Codex: the SCOPE (file-path) rules rendered as guidance, since
/// apply_patch hook interception is flaky (honest boundary). Command rules are omitted —
/// they enforce via the Bash hook. Returns an empty string if there are no scope rules.
pub fn codex_agents_md(policy: &TouchstonePolicy) -> String {
    // File-scope (path) rules — rendered as guidance since apply_patch hook interception
    // is flaky. Command rules are omitted (they enforce via the Bash hook).
    let mut path_lines: Vec<String> = Vec::new();
    for r in &policy.rules {
        if let Matcher::Path { glob } = &r.matcher {
            let verb = match r.response {
                Response::Deny => "DO NOT touch",
                Response::Critique => "Be careful with",
                Response::Warn => "Think twice before touching",
            };
            // The message is sovereign-authored Canon prose; one line, no raw command.
            let msg = r.message.lines().next().unwrap_or("").trim();
            path_lines.push(format!("- **{verb} `{glob}`** ({}): {msg}", r.id));
        }
    }
    // Advisory (kind:advisory) rules — behavioral guidelines injected as full guidance,
    // filtered to those whose scope applies to Codex.
    let advisory = render_advisory_blocks(policy, "  ", "codex");
    if path_lines.is_empty() && advisory.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "## Flint sovereign rules (advisory)\n\n\
         These are your operator's boundaries. Where the flint hook fires they are \
         enforced; here they are guidance for actions the hook cannot intercept.\n\n",
    );
    if !path_lines.is_empty() {
        out.push_str("### File-scope boundaries\n\n");
        out.push_str(&path_lines.join("\n"));
        out.push('\n');
        if !advisory.is_empty() {
            out.push('\n');
        }
    }
    if !advisory.is_empty() {
        out.push_str("### Behavioral guidelines\n\n");
        out.push_str(&advisory);
    }
    out
}

/// Render `policy.advisory` as a markdown bullet list: each rule's description, its body
/// prose (indented by `indent`), and its trigger. Empty string when there are no
/// advisory rules. Shared by the Codex (AGENTS.md) and Claude (.claude/rules) emitters.
fn render_advisory_blocks(policy: &TouchstonePolicy, indent: &str, harness: &str) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for a in &policy.advisory {
        if !advisory_applies(a, harness) {
            continue;
        }
        let mut block = format!("- **{}** — {}", a.id, a.description.trim());
        let body = a.message.trim();
        if !body.is_empty() {
            block.push('\n');
            block.push_str(indent);
            block.push_str(&body.replace('\n', &format!("\n{indent}")));
        }
        if !a.trigger.is_empty() {
            block.push_str(&format!("\n{indent}_Applies when: {}_", a.trigger.join(", ")));
        }
        blocks.push(block);
    }
    if blocks.is_empty() {
        return String::new();
    }
    let mut out = blocks.join("\n");
    out.push('\n');
    out
}

/// Whether advisory `a` is compiled into `harness`. An advisory with NO agent selector
/// in its scope (only `global` / `project:*` / `os:*`, or empty) applies to EVERY
/// harness; one WITH agent selectors applies only to the listed agents. Prevents an
/// `agent:claude`-scoped rule from leaking into Codex's AGENTS.md (codex P2-3).
fn advisory_applies(a: &AdvisoryRule, harness: &str) -> bool {
    let has_agent_selector = a.scope.iter().any(|s| s.starts_with("agent:"));
    !has_agent_selector || a.scope.iter().any(|s| s == &format!("agent:{harness}"))
}

/// The body of `.claude/rules/flint-advisory.md`: flint's advisory (kind:advisory) rules
/// as always-on guidance (no `paths` frontmatter → loaded every session, delivered as a
/// user-message-level rule per Claude Code's memory semantics). Empty string when there
/// are no advisory rules (the caller then writes nothing).
pub fn claude_advisory_rules(policy: &TouchstonePolicy) -> String {
    let applicable: Vec<&AdvisoryRule> =
        policy.advisory.iter().filter(|a| advisory_applies(a, "claude")).collect();
    if applicable.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "# Flint advisory rules\n\n\
         Your operator's behavioral boundaries, compiled from Canon. These are guidance \
         (not enforced by a hook); follow them as you would your own standing rules.\n\n",
    );
    for a in applicable {
        out.push_str(&format!("## {}\n\n{}\n\n", a.id, a.description.trim()));
        let body = a.message.trim();
        if !body.is_empty() {
            out.push_str(body);
            out.push_str("\n\n");
        }
        if !a.trigger.is_empty() {
            out.push_str(&format!("_Applies when: {}_\n\n", a.trigger.join(", ")));
        }
    }
    out
}

/// The markers delimiting flint's managed block inside a shared doc (AGENTS.md).
pub const FLINT_BLOCK_BEGIN: &str = "<!-- flint:begin -->";
pub const FLINT_BLOCK_END: &str = "<!-- flint:end -->";

/// Insert or replace flint's marked block in `existing`, preserving ALL user content
/// outside the markers. An empty (whitespace-only) `block` removes the marked section.
/// Pure + idempotent — the compile writer uses it to upsert a repo's AGENTS.md without
/// clobbering the user's own content.
pub fn upsert_marked_block(existing: &str, block: &str) -> String {
    let wrapped = if block.trim().is_empty() {
        String::new()
    } else {
        format!("{FLINT_BLOCK_BEGIN}\n{}\n{FLINT_BLOCK_END}\n", block.trim_end())
    };
    match (existing.find(FLINT_BLOCK_BEGIN), existing.find(FLINT_BLOCK_END)) {
        (Some(start), Some(end)) if end > start => {
            let end_line = end + FLINT_BLOCK_END.len();
            // drop one trailing newline after the end marker so replace is idempotent
            let after = existing[end_line..].strip_prefix('\n').unwrap_or(&existing[end_line..]);
            format!("{}{}{}", &existing[..start], wrapped, after)
        }
        _ if wrapped.is_empty() => existing.to_string(),
        _ => {
            let mut out = existing.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&wrapped);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::touchstone::{GateRule, Reversibility};

    fn params() -> CompileParams {
        CompileParams {
            flint_bin: "/usr/local/bin/flint".into(),
            config_path: "/home/u/.flint/flint.toml".into(),
            mode: "block".into(),
        }
    }

    fn scope_rule(id: &str, glob: &str, resp: Response, msg: &str) -> GateRule {
        GateRule {
            id: id.into(),
            matcher: Matcher::Path { glob: glob.into() },
            response: resp,
            reversibility: Reversibility::Reversible,
            message: msg.into(),
            suggestion: String::new(),
        }
    }
    fn cmd_rule(id: &str) -> GateRule {
        GateRule {
            id: id.into(),
            matcher: Matcher::Command { pattern: "grep".into(), exempt: None },
            response: Response::Critique,
            reversibility: Reversibility::Reversible,
            message: "use lsp".into(),
            suggestion: String::new(),
        }
    }

    #[test]
    fn claude_settings_wires_pretooluse_to_flint() {
        let v = claude_settings(&params());
        let cmd = v["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("flint hook --harness claude --config"));
        // codex P1: the installed wiring MUST enforce — never rely on the CLI record default.
        assert!(cmd.contains("--mode block"), "compiled wiring must pin block mode: {cmd}");
        assert!(cmd.contains("--fail-mode closed"), "compiled wiring must pin fail-closed: {cmd}");
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "*");
    }

    #[test]
    fn codex_hooks_covers_bash_and_apply_patch() {
        let v = codex_hooks(&params());
        let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
        let matchers: Vec<&str> = arr.iter().map(|e| e["matcher"].as_str().unwrap()).collect();
        assert!(matchers.contains(&"Bash") && matchers.contains(&"apply_patch"));
        let cmd = arr[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("--harness codex"));
        assert!(cmd.contains("--mode block"), "compiled codex wiring must pin block mode: {cmd}");
    }

    #[test]
    fn grok_hooks_matches_every_tool_by_omitting_the_matcher() {
        let v = grok_hooks(&params());
        let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "one group covers every tool");
        // BEHAVIOR, not style: Grok's matcher is a regex and `"*"` is invalid there, so
        // "match everything" is spelled by OMITTING the key. A dead matcher on a fail-open
        // harness is indistinguishable from a quiet gate.
        assert!(arr[0].get("matcher").is_none(), "grok group must carry no matcher: {arr:?}");
        let entry = &arr[0]["hooks"][0];
        assert_eq!(entry["type"], "command");
        assert_eq!(entry["timeout"], 600);
        let cmd = entry["command"].as_str().unwrap();
        assert!(cmd.contains("--harness grok"), "{cmd}");
        assert!(cmd.contains("--mode block"), "compiled grok wiring must enforce: {cmd}");
        assert!(cmd.contains("--fail-mode closed"), "compiled grok wiring must fail closed: {cmd}");
    }

    #[test]
    fn grok_hooks_quotes_paths_so_a_space_cannot_disarm_the_gate() {
        // Outside-voice round-1 P0: Grok is fail-OPEN on a hook that fails to launch, so
        // an unquoted space in flint_bin/config_path (routine on Windows) would split the
        // shell command and silently disarm the gate. Both path arguments must be quoted.
        let p = CompileParams {
            flint_bin: "C:/Program Files/flint/flint.exe".into(),
            config_path: "C:/Users/A User/.flint/flint.toml".into(),
            mode: "block".into(),
        };
        // sh shape: the bare quoted path IS the command.
        let v = grok_hooks_for(&p, false);
        let cmd = v["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            cmd.starts_with("\"C:/Program Files/flint/flint.exe\" hook"),
            "the binary path must be quoted: {cmd}"
        );
        assert!(
            cmd.contains("--config \"C:/Users/A User/.flint/flint.toml\""),
            "the config path must be quoted: {cmd}"
        );
        // PowerShell shape (Grok's Windows shell): a leading quoted string is an
        // EXPRESSION, not a command — ParserError, hook exit 1, silent fail-open
        // (measured 2026-08-20, Windows sentinel). The call operator makes it a command.
        let w = grok_hooks_for(&p, true);
        let wcmd = w["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            wcmd.starts_with("& \"C:/Program Files/flint/flint.exe\" hook"),
            "windows shape needs the PowerShell call operator: {wcmd}"
        );
        assert!(wcmd.contains("--config \"C:/Users/A User/.flint/flint.toml\""), "{wcmd}");
    }

    #[test]
    fn record_mode_is_emitted_verbatim_when_chosen() {
        // the operator can compile a dogfood-first advisory wiring; the mode is pinned.
        let p = CompileParams { mode: "record".into(), ..params() };
        let cmd = claude_settings(&p)["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap().to_string();
        assert!(cmd.contains("--mode record"), "explicit record mode flows into the wiring: {cmd}");
    }

    #[test]
    fn agents_md_lists_scope_rules_only() {
        let policy = TouchstonePolicy {
            rules: vec![
                scope_rule("no-secrets", "knowledge/secret/**", Response::Deny, "never write secrets\nmore detail"),
                cmd_rule("lsp"), // command rule — must NOT appear (enforced via Bash hook)
            ],
            advisory: Vec::new(),
        };
        let md = codex_agents_md(&policy);
        assert!(md.contains("knowledge/secret/**"));
        assert!(md.contains("DO NOT touch"));
        assert!(md.contains("never write secrets"));
        assert!(!md.contains("more detail"), "only the first message line is emitted");
        assert!(!md.contains("lsp"), "command rules are not in the advisory");
    }

    #[test]
    fn agents_md_empty_when_no_scope_rules() {
        let policy = TouchstonePolicy { rules: vec![cmd_rule("lsp")], advisory: Vec::new() };
        assert_eq!(codex_agents_md(&policy), "");
    }

    fn adv(id: &str) -> AdvisoryRule {
        AdvisoryRule {
            id: id.into(),
            description: format!("desc for {id}"),
            message: format!("body guidance for {id}"),
            trigger: vec!["when-x".into(), "when-y".into()],
            scope: vec!["global".into()],
        }
    }

    #[test]
    fn agents_md_includes_advisory() {
        let policy = TouchstonePolicy { rules: vec![], advisory: vec![adv("verify-first")] };
        let md = codex_agents_md(&policy);
        assert!(md.contains("### Behavioral guidelines"));
        assert!(md.contains("verify-first"));
        assert!(md.contains("desc for verify-first"));
        assert!(md.contains("body guidance for verify-first"));
        assert!(md.contains("Applies when: when-x, when-y"));
    }

    #[test]
    fn claude_advisory_rules_renders_each_and_empty() {
        let policy = TouchstonePolicy { rules: vec![], advisory: vec![adv("a1"), adv("a2")] };
        let md = claude_advisory_rules(&policy);
        assert!(md.starts_with("# Flint advisory rules"));
        assert!(md.contains("## a1"));
        assert!(md.contains("## a2"));
        assert!(md.contains("desc for a1"));
        assert!(md.contains("_Applies when: when-x, when-y_"));
        // empty advisory -> empty string (caller writes nothing)
        let empty = TouchstonePolicy { rules: vec![], advisory: vec![] };
        assert_eq!(claude_advisory_rules(&empty), "");
    }

    #[test]
    fn advisory_scope_filters_by_harness() {
        let claude_only = AdvisoryRule {
            id: "claude-rule".into(),
            description: "claude only".into(),
            message: "body".into(),
            trigger: vec![],
            scope: vec!["agent:claude".into()],
        };
        let global = adv("global-rule"); // adv() scopes to ["global"] → applies everywhere
        let policy = TouchstonePolicy { rules: vec![], advisory: vec![claude_only, global] };
        // codex gets the global rule, NOT the agent:claude-scoped one
        let codex_md = codex_agents_md(&policy);
        assert!(codex_md.contains("global-rule"));
        assert!(!codex_md.contains("claude-rule"), "agent:claude rule must not leak into codex");
        // claude gets both
        let claude_md = claude_advisory_rules(&policy);
        assert!(claude_md.contains("global-rule") && claude_md.contains("claude-rule"));
    }

    #[test]
    fn upsert_appends_then_replaces_idempotently() {
        let base = "# My AGENTS\n\nsome user rules\n";
        let one = upsert_marked_block(base, "flint block v1");
        assert!(one.contains("# My AGENTS") && one.contains("some user rules"));
        assert!(one.contains(FLINT_BLOCK_BEGIN) && one.contains("flint block v1"));
        // replace: user content preserved, block updated, exactly one block
        let two = upsert_marked_block(&one, "flint block v2");
        assert!(two.contains("some user rules") && two.contains("flint block v2"));
        assert!(!two.contains("flint block v1"));
        assert_eq!(two.matches(FLINT_BLOCK_BEGIN).count(), 1);
        // idempotent
        assert_eq!(upsert_marked_block(&two, "flint block v2"), two);
    }

    #[test]
    fn upsert_empty_block_removes_section() {
        let with = upsert_marked_block("user content\n", "flint stuff");
        let without = upsert_marked_block(&with, "");
        assert!(!without.contains(FLINT_BLOCK_BEGIN));
        assert!(without.contains("user content"));
    }
}
