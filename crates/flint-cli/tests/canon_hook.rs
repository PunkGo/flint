//! P1 acceptance: one signed Canon -> both harnesses intercept + critique feedback.
//! End-to-end on the REAL `flint` binary (Canon pick/sign -> lint -> hook claude/codex ->
//! tamper fail-closed). Requires `ssh-keygen` (present on dev/CI mac+linux).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn flint_bin() -> &'static str {
    env!("CARGO_BIN_EXE_flint")
}

const LSP_RULE: &str = "---\nschema: flint/v1\nid: lsp-over-grep\ntype: rule\nkind: command\npattern: '\\bgrep\\b'\nresponse: critique\n---\nUse the LSP (goToDefinition/findReferences) for code symbols, not grep.\n";
// The non-blocking tier is v2-only: `warn` under v1 still means "blocks" and is rejected
// outright, so a signed v1 canon can never be silently re-read as fail-open.
const NUDGE_RULE: &str = "---\nschema: flint/v2\nid: prefer-fd\ntype: rule\nkind: command\npattern: '\\bfind\\b'\nresponse: warn\n---\nPrefer fd over find.\n";
const SECRET_RULE: &str = "---\nschema: flint/v1\nid: no-secrets\ntype: rule\nkind: path\nglob: knowledge/secret/**\nresponse: block\nreversibility: irreversible\n---\nNever write secrets into knowledge/secret. Use the Keychain.\n";
const LSP_FIXTURE: &str = "---\nrule_id: lsp-over-grep\n---\n## bad\ngrep -rn foo src/main.rs\n## good\ncat src/main.rs\nrg foo x.rs\n";
const VERIFY_ADVISORY: &str = "---\nschema: flint/v1\nid: verify-before-claiming\ntype: rule\nkind: advisory\ndescription: Verify external state before asserting it.\nscope: global, agent:claude, agent:codex\ntrigger: before-reporting-status, before-claiming-tests-pass\n---\nDo not assert a fact from memory when a cheap check is available.\n";

struct Env {
    _dir: tempfile::TempDir,
    config: std::path::PathBuf,
    canon_root: std::path::PathBuf,
    key: std::path::PathBuf,
}

fn setup() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let canon_root = root.join("canon");
    fs::create_dir_all(canon_root.join("rules")).unwrap();
    fs::write(canon_root.join("rules/lsp.md"), LSP_RULE).unwrap();
    fs::write(canon_root.join("rules/no-secrets.md"), SECRET_RULE).unwrap();
    fs::create_dir_all(canon_root.join("fixtures")).unwrap();
    fs::write(canon_root.join("fixtures/lsp-over-grep.md"), LSP_FIXTURE).unwrap();

    // sovereign signing key + allowed_signers
    let key = root.join("sov_key");
    assert!(Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C", "flint-sovereign", "-f"])
        .arg(&key)
        .status()
        .expect("ssh-keygen")
        .success());
    let pubk = fs::read_to_string(root.join("sov_key.pub")).unwrap();
    let key_only: Vec<&str> = pubk.split_whitespace().take(2).collect();
    let allowed = root.join("allowed_signers");
    fs::write(&allowed, format!("flint-sovereign {}\n", key_only.join(" "))).unwrap();

    let config = root.join("flint.toml");
    fs::write(
        &config,
        format!(
            "flint_bin = {bin:?}\ncanon_root = {cr:?}\nworkspace_root = {wr:?}\nobs_log = {ol:?}\n[budget]\nsidecar = {bs:?}\ncritique_threshold = 100000\n[trust]\nallowed_signers = {al:?}\nsigner_identity = \"flint-sovereign\"\nscope = \"test-instance\"\nmin_epoch = 0\n",
            bin = flint_bin(),
            cr = canon_root,
            wr = root,
            ol = root.join("obs.jsonl"),
            bs = root.join("budget"),
            al = allowed,
        ),
    )
    .unwrap();

    Env { _dir: dir, config, canon_root, key }
}

fn run(args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
    let mut cmd = Command::new(flint_bin());
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(s) = stdin {
        child.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Add the warn-tier rule to a fresh Canon. Kept OUT of `setup` so the shared fixture's
/// rule set (which other tests count) does not drift for the sake of two tests.
fn with_warn_rule(env: &Env) {
    fs::write(env.canon_root.join("rules/nudge.md"), NUDGE_RULE).unwrap();
}

fn pick(env: &Env) {
    let (code, _o, e) = run(
        &["canon", "pick", "--config", env.config.to_str().unwrap(), "--key", env.key.to_str().unwrap()],
        None,
    );
    assert_eq!(code, 0, "pick failed: {e}");
    assert!(env.canon_root.join("CANON.manifest").exists());
    assert!(env.canon_root.join("CANON.manifest.sig").exists());
}

#[test]
fn lint_passes_on_clean_canon() {
    let env = setup();
    let (code, out, _e) = run(&["canon", "lint", "--config", env.config.to_str().unwrap()], None);
    assert_eq!(code, 0);
    assert!(out.contains("OK"), "lint output: {out}");
}

#[test]
fn lint_fails_on_malformed_rule() {
    let env = setup();
    fs::write(env.canon_root.join("rules/bad.md"), "---\nschema: flint/v1\nid: bad\ntype: rule\nkind: path\nglob: src/*\nresponse: block\n---\nx\n").unwrap();
    let (code, _o, e) = run(&["canon", "lint", "--config", env.config.to_str().unwrap()], None);
    assert_ne!(code, 0, "lint must fail on a malformed glob");
    assert!(e.contains("FAIL"), "stderr: {e}");
}

#[test]
fn claude_command_rule_critiques_grep() {
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Bash","tool_input":{"command":"grep -rn foo src/main.rs"}}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    // A critique blocks through the JSON channel, never the exit code: codex on Windows
    // treats a non-zero PreToolUse exit as a hook error and runs the command anyway.
    assert_eq!(code, 0, "the exit code is not the blocking channel. stderr: {e}");
    assert!(out.contains("\"permissionDecision\":\"deny\""), "a critique must block: {out}");
    assert!(out.contains("flint critique"), "and must name its tier, not pose as a freeze: {out}");
    assert!(out.to_lowercase().contains("lsp"), "critique message reaches the agent: {out}");
}

#[test]
fn claude_critique_stdout_is_pinned_to_its_pre_grok_bytes() {
    // Outside-voice round-2: a Value-equality pin (hook.rs unit test) cannot see the
    // SERIALIZATION layer drift — whitespace, key order, a pretty-printer slipping into
    // the output path. This pins the REAL binary's whole stdout line, byte for byte, to
    // the envelope that shipped before the grok fork. serde_json (no preserve_order)
    // sorts object keys, so this string is deterministic.
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Bash","tool_input":{"command":"grep -rn foo src/main.rs"}}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "stderr: {e}");
    assert_eq!(
        out,
        concat!(
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"flint critique on exec:bash: Use the LSP (goToDefinition/findReferences) for code symbols, not grep.."}}"#,
            "\n"
        ),
        "the claude stdout bytes drifted from the pre-grok envelope"
    );
}

#[test]
fn codex_command_rule_critiques_grep_same_canon() {
    // SAME signed Canon, the OTHER harness — the dual-harness contract (one Canon, both enforce).
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Bash","tool_input":{"command":"grep -rn foo src/main.rs"}}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "codex", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "stderr: {e}");
    assert!(out.contains("\"permissionDecision\":\"deny\""), "codex must enforce the same rule: {out}");
    assert!(out.contains("flint critique"), "{out}");
}

// ---------------------------------------------------------------------------
// Grok: the THIRD harness on the SAME signed Canon. Its wire shape is camelCase and
// — measured 2026-08-20 on grok 1.0.5 — it IGNORES `permissionDecision` and honours
// only `{"decision":"deny"}`, so every blocking path here asserts that envelope and
// its ABSENCE of the claude/codex one.
// ---------------------------------------------------------------------------

#[test]
fn grok_command_rule_critiques_grep_same_canon() {
    let env = setup();
    pick(&env);
    let hook = r#"{"hookEventName":"pre_tool_use","workspaceRoot":"/ws/","cwd":"/ws","toolName":"run_terminal_command","toolInput":{"command":"grep -rn foo src/main.rs"},"toolInputTruncated":false}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "grok", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "the exit code is not the blocking channel. stderr: {e}");
    assert!(out.contains("\"decision\":\"deny\""), "grok blocks only on this shape: {out}");
    assert!(
        !out.contains("permissionDecision"),
        "the envelope grok IGNORES must not be what we send it: {out}"
    );
    assert!(out.contains("flint critique"), "and must name its tier, not pose as a freeze: {out}");
    assert!(out.to_lowercase().contains("lsp"), "critique message reaches the agent: {out}");
    let log = fs::read_to_string(env._dir.path().join("obs.jsonl")).unwrap_or_default();
    assert!(log.contains("\"harness\":\"grok\""), "the receipt is attributed to grok: {log}");
    assert!(log.contains("\"verdict\":\"critique\""), "{log}");
}

#[test]
fn grok_scope_deny_blocks_secret_write() {
    let env = setup();
    pick(&env);
    let hook = r#"{"hookEventName":"pre_tool_use","toolName":"write","toolInput":{"file_path":"knowledge/secret/key","content":"x"},"toolInputTruncated":false}"#;
    let (code, out, _e) = run(
        &["hook", "--harness", "grok", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "a deny is exit 0 with a decision JSON");
    assert!(out.contains("\"decision\":\"deny\""), "deny output: {out}");
    assert!(out.contains("freeze"), "deny keeps its own harsher wording: {out}");
    assert!(!out.contains("permissionDecision"), "{out}");
}

#[test]
fn grok_truncated_input_fails_closed() {
    // Grok is fail-OPEN everywhere else, so flint's own fail-CLOSED path has to speak the
    // shape grok honours. A TRUNCATED toolInput may have had the target cut away — it must
    // deny, not Affirm on an empty scope set.
    let env = setup();
    pick(&env);
    let hook = r#"{"hookEventName":"pre_tool_use","toolName":"write","toolInput":{"file_path":"knowledge/pu"},"toolInputTruncated":true}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "grok", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0);
    assert!(out.contains("\"decision\":\"deny\""), "truncated input must fail closed: {out} / {e}");
    assert!(
        !out.contains("permissionDecision"),
        "a fail-closed deny in the ignored envelope is fail-closed in name only: {out}"
    );
    assert!(out.to_lowercase().contains("fail-closed"), "{out}");
}

#[test]
fn grok_affirm_emits_nothing() {
    let env = setup();
    pick(&env);
    let hook = r#"{"hookEventName":"pre_tool_use","toolName":"write","toolInput":{"file_path":"knowledge/public/note"},"toolInputTruncated":false}"#;
    let (code, out, _e) = run(
        &["hook", "--harness", "grok", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0);
    assert!(out.is_empty(), "affirm emits nothing (flint never authorizes), got: {out}");
}

#[test]
fn grok_warn_speaks_additional_context_and_receipts() {
    // The warn tier on grok: the call PROCEEDS and the passage is receipted. The prose is
    // emitted in the claude shape and is a MEASURED non-delivery on grok 1.0.5 (probe C:
    // the model reported NO_HOOK_TEXT_SEEN) — kept because grok fail-opens on a JSON with
    // no `decision`, so it is harmless, and recorded here as debt, not as enforcement.
    let env = setup();
    with_warn_rule(&env);
    pick(&env);
    let hook = r#"{"hookEventName":"pre_tool_use","toolName":"run_terminal_command","toolInput":{"command":"find . -name x"},"toolInputTruncated":false}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "grok", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "a warn must NOT block. stderr: {e}");
    assert!(out.contains("\"additionalContext\""), "the warn rides the JSON channel: {out}");
    assert!(out.contains("prefer-fd"), "the warn names its rule: {out}");
    assert!(!out.contains("\"decision\""), "a warn must never carry grok's deny key: {out}");
    assert!(!out.contains("permissionDecision"), "{out}");
    let log = fs::read_to_string(env._dir.path().join("obs.jsonl")).unwrap_or_default();
    assert!(log.contains("\"harness\":\"grok\""), "the passage is attributed: {log}");
    assert!(log.contains("\"verdict\":\"warn\""), "the passage is countable: {log}");
    assert!(log.contains("\"rule_id\":\"prefer-fd\""), "attributed to its rule: {log}");
}

#[test]
fn grok_gate_error_fails_closed_with_the_decision_envelope() {
    // Outside-voice round-1 P1: the fail-closed paths (gate error / setup failure) must
    // ALSO speak the one envelope grok honours. A tampered working tree makes the signed
    // load fail — on grok the resulting deny must be `{"decision":"deny"}`, because the
    // shared `permissionDecision` shape is measured-ignored there (fail-open).
    let env = setup();
    pick(&env);
    fs::write(env.canon_root.join("rules/no-secrets.md"), "---\nid: no-secrets\ntype: scope\nglob: nothing/**\nresponse: critique\n---\nweakened\n").unwrap();
    let hook = r#"{"hookEventName":"pre_tool_use","toolName":"write","toolInput":{"file_path":"knowledge/secret/key"},"toolInputTruncated":false}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "grok", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0);
    assert!(out.contains("\"decision\":\"deny\""), "grok gate error must fail closed in the shape grok honours. out={out} err={e}");
    assert!(!out.contains("permissionDecision"), "the ignored envelope must not carry the fail-closed deny: {out}");
    assert!(e.contains("gate") || e.contains("mismatch"), "stderr: {e}");
}

#[test]
fn grok_workspace_root_from_stdin_locates_an_absolute_target() {
    // With no configured workspace_root, the root GROK ITSELF reported on stdin resolves
    // the absolute path into the in-tree scope the deny glob is written against. Without
    // that fallback the hook process's cwd decides — and on Windows the cwd is not
    // guaranteed to be the workspace, so the deny would silently stop firing.
    let env = setup();
    let cfg = fs::read_to_string(&env.config).unwrap();
    let stripped: Vec<&str> = cfg.lines().filter(|l| !l.starts_with("workspace_root")).collect();
    fs::write(&env.config, format!("{}\n", stripped.join("\n"))).unwrap();
    pick(&env);
    let root = env._dir.path().display().to_string();
    let abs = env._dir.path().join("knowledge/secret/key").display().to_string();
    let hook = format!(
        r#"{{"hookEventName":"pre_tool_use","workspaceRoot":"{root}/","cwd":"{root}","toolName":"write","toolInput":{{"file_path":"{abs}"}},"toolInputTruncated":false}}"#
    );
    let (code, out, e) = run(
        &["hook", "--harness", "grok", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(&hook),
    );
    assert_eq!(code, 0, "stderr: {e}");
    assert!(
        out.contains("\"decision\":\"deny\""),
        "the harness-reported workspaceRoot must locate the target: {out}"
    );
}

#[test]
fn claude_scope_deny_blocks_secret_write() {
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Write","tool_input":{"file_path":"knowledge/secret/key"}}"#;
    let (code, out, _e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "a deny is exit 0 with a permissionDecision JSON");
    assert!(out.contains("\"permissionDecision\":\"deny\""), "deny output: {out}");
}

#[test]
fn claude_scope_deny_blocks_dotslash_spelling() {
    // codex P1: a `./` (or `//`, `/./`, `..`) spelling of a denied path must still deny —
    // the FS writes inside `knowledge/secret/` regardless of how the path is spelled.
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Write","tool_input":{"file_path":"./knowledge/secret/key"}}"#;
    let (code, out, _e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0);
    assert!(out.contains("\"permissionDecision\":\"deny\""), "dot-slash spelling must still deny: {out}");
}

#[test]
fn claude_scope_deny_blocks_absolute_path() {
    // codex P1 r2: the NORMAL Claude case — file_path is ABSOLUTE. It must resolve against
    // the workspace root and still hit the relative `knowledge/secret/**` deny.
    let env = setup();
    pick(&env);
    let abs = env._dir.path().join("knowledge/secret/key");
    let hook = format!(r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}"}}}}"#, abs.display());
    let (code, out, _e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(&hook),
    );
    assert_eq!(code, 0);
    assert!(out.contains("\"permissionDecision\":\"deny\""), "absolute in-tree path must deny: {out}");
}

#[test]
fn claude_absolute_path_outside_workspace_affirms() {
    // an absolute path OUTSIDE the workspace is not governed by the relative in-tree globs.
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Write","tool_input":{"file_path":"/etc/knowledge/secret/key"}}"#;
    let (code, out, _e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0);
    assert!(out.is_empty(), "out-of-workspace path affirms (no in-tree glob governs it): {out}");
}

#[test]
fn codex_apply_patch_dotslash_spelling_denies() {
    // same bypass on the codex apply_patch envelope.
    let env = setup();
    pick(&env);
    let patch = "*** Begin Patch\\n*** Add File: ./knowledge/secret/key\\n*** End Patch\\n";
    let hook = format!(r#"{{"tool_name":"apply_patch","tool_input":{{"command":"{patch}"}}}}"#);
    let (code, out, _e) = run(
        &["hook", "--harness", "codex", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(&hook),
    );
    assert_eq!(code, 0);
    assert!(out.contains("\"permissionDecision\":\"deny\""), "codex dot-slash patch path must deny: {out}");
}

#[test]
fn affirm_when_no_rule_matches() {
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Write","tool_input":{"file_path":"knowledge/public/note"}}"#;
    let (code, out, _e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0);
    assert!(out.is_empty(), "affirm emits nothing, got: {out}");
}

#[test]
fn tampered_working_tree_fails_closed() {
    // Agent edits a rule in the working tree WITHOUT a sovereign re-pick -> the signed
    // manifest's hash no longer matches -> block+closed denies (the write boundary holds).
    let env = setup();
    pick(&env);
    // weaken the deny rule in the working tree (agent attack).
    fs::write(env.canon_root.join("rules/no-secrets.md"), "---\nid: no-secrets\ntype: scope\nglob: nothing/**\nresponse: critique\n---\nweakened\n").unwrap();
    let hook = r#"{"tool_name":"Write","tool_input":{"file_path":"knowledge/secret/key"}}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    // The gate can't load a trusted policy (hash mismatch) -> fail-closed deny.
    assert_eq!(code, 0);
    assert!(out.contains("\"permissionDecision\":\"deny\""), "tampered canon must fail-closed deny. out={out} err={e}");
    assert!(e.contains("gate") || e.contains("mismatch"), "stderr: {e}");
}

#[test]
fn claude_warn_rule_lets_the_call_through_and_speaks() {
    // The third tier end-to-end on the real binary: the action PROCEEDS (exit 0, no deny
    // JSON) but the agent is told and the passage is receipted.
    let env = setup();
    with_warn_rule(&env);
    pick(&env);
    let hook = r#"{"tool_name":"Bash","tool_input":{"command":"find . -name x"}}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "a warn must NOT block. stderr: {e}");
    assert!(out.contains("\"additionalContext\""), "the warn must reach the model: {out}");
    assert!(out.contains("prefer-fd"), "the warn names its rule: {out}");
    // flint never authorizes: a warn must not carry a permissionDecision of any kind.
    assert!(!out.contains("\"permissionDecision\""), "a warn must not decide permission: {out}");
    let log = fs::read_to_string(env._dir.path().join("obs.jsonl")).unwrap_or_default();
    assert!(log.contains("\"verdict\":\"warn\""), "the passage is countable: {log}");
    assert!(log.contains("\"rule_id\":\"prefer-fd\""), "attributed to its rule: {log}");
}

#[test]
fn a_warn_never_masks_a_critique_on_the_same_command() {
    // Precedence end-to-end: one command trips BOTH tiers; the blocking one must win.
    let env = setup();
    with_warn_rule(&env);
    pick(&env);
    let hook = r#"{"tool_name":"Bash","tool_input":{"command":"find . -name x | grep foo"}}"#;
    let (code, out, e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(hook),
    );
    assert_eq!(code, 0, "stderr={e}");
    assert!(out.contains("\"permissionDecision\":\"deny\""), "critique outranks warn — it must BLOCK: {out}");
    assert!(!out.contains("additionalContext"), "and must not degrade to the warn channel: {out}");
}

#[test]
fn record_mode_never_blocks_but_logs() {
    let env = setup();
    pick(&env);
    let hook = r#"{"tool_name":"Bash","tool_input":{"command":"grep -rn foo src/main.rs"}}"#;
    let (code, _o, _e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap()], // default mode=record
        Some(hook),
    );
    assert_eq!(code, 0, "record mode never blocks");
    let log = fs::read_to_string(env._dir.path().join("obs.jsonl")).unwrap_or_default();
    assert!(log.contains("\"verdict\":\"critique\""), "obs-log should record the critique receipt: {log}");
    // Redaction: the RAW command content must never reach the log. (The rule id
    // `lsp-over-grep` legitimately contains "grep" — that's the sovereign rule name, not
    // the agent's command — so we check for the command's distinctive args instead.)
    assert!(!log.contains("src/main.rs") && !log.contains("-rn"), "obs-log must NOT contain the raw command (redaction): {log}");
}

#[test]
fn compile_emits_claude_and_codex_wiring() {
    let env = setup();
    pick(&env);
    let (c1, o1, _) = run(&["compile", "--harness", "claude", "--config", env.config.to_str().unwrap()], None);
    assert_eq!(c1, 0);
    assert!(o1.contains("PreToolUse") && o1.contains("--harness claude"));
    // codex P1: the compiled wiring defaults to ENFORCING mode (never the CLI record default).
    assert!(o1.contains("--mode block"), "compiled claude wiring must enforce: {o1}");
    let (c2, o2, _) = run(&["compile", "--harness", "codex", "--config", env.config.to_str().unwrap()], None);
    assert_eq!(c2, 0);
    assert!(o2.contains("--harness codex") && o2.contains("--mode block"));
    assert!(o2.contains("AGENTS.md") && o2.contains("knowledge/secret/**"), "codex compile should emit the scope advisory: {o2}");
}

#[test]
fn compile_emits_grok_wiring() {
    let env = setup();
    pick(&env);
    let (code, out, e) = run(&["compile", "--harness", "grok", "--config", env.config.to_str().unwrap()], None);
    assert_eq!(code, 0, "grok must be a known harness now: {e}");
    assert!(!e.contains("must be claude|codex"), "stderr: {e}");
    assert!(out.contains("PreToolUse") && out.contains("--harness grok"), "{out}");
    assert!(out.contains("--mode block") && out.contains("--fail-mode closed"), "compiled grok wiring must enforce and fail closed: {out}");
    assert!(out.contains("\"timeout\": 600"), "{out}");
    // Grok's matcher is a REGEX: `"*"` is invalid there and an OMITTED matcher is how you
    // say "every tool". Emitting one would be a dead group on a fail-open harness.
    assert!(!out.contains("matcher"), "the grok group must carry no matcher key: {out}");
    // Advisory is the prompt layer — out of scope for grok wiring.
    assert!(!out.contains("AGENTS.md"), "{out}");
}

#[test]
fn forge_promote_then_list_shows_derived_tier() {
    let env = setup();
    // run the load-bearing gate: lsp-over-grep has a discrimination fixture -> reproduced.
    let (pc, po, pe) = run(
        &["forge", "promote", "--config", env.config.to_str().unwrap(), "--rule", "lsp-over-grep"],
        None,
    );
    assert_eq!(pc, 0, "promote should succeed (rule discriminates). out={po} err={pe}");
    assert!(env.canon_root.join("promotion/lsp-over-grep.md").exists(), "promotion record written");
    // sign everything (rules + promotion + fixtures) then list with derived tiers.
    pick(&env);
    let (lc, lo, _e) = run(&["canon", "list", "--config", env.config.to_str().unwrap()], None);
    assert_eq!(lc, 0);
    // lsp-over-grep earned Reproduced; no-secrets has no fixture -> Prose.
    let lsp_line = lo.lines().find(|l| l.contains("lsp-over-grep")).unwrap_or("");
    assert!(lsp_line.contains("Reproduced"), "lsp should be reproduced: {lsp_line}");
    let sec_line = lo.lines().find(|l| l.contains("no-secrets")).unwrap_or("");
    assert!(sec_line.contains("Prose"), "no-secrets should be prose: {sec_line}");
}

#[test]
fn forge_promote_rejects_nondiscriminating_rule() {
    let env = setup();
    // overwrite the fixture so a known-good case wrongly fires (over-broad) -> reject.
    fs::write(
        env.canon_root.join("fixtures/lsp-over-grep.md"),
        "---\nrule_id: lsp-over-grep\n---\n## bad\ngrep foo src/main.rs\n## good\ngrep TODO README.md\n",
    )
    .unwrap();
    let (code, _o, e) = run(
        &["forge", "promote", "--config", env.config.to_str().unwrap(), "--rule", "lsp-over-grep"],
        None,
    );
    assert_ne!(code, 0, "an over-broad rule must NOT be promoted");
    assert!(e.contains("NOT promoted"), "stderr: {e}");
}

#[test]
fn forge_ceiling_reports_counts() {
    let env = setup();
    let (code, out, _e) = run(&["forge", "ceiling", "--config", env.config.to_str().unwrap()], None);
    assert_eq!(code, 0);
    // lsp has a discriminating fixture; no-secrets has none -> 1/2.
    assert!(out.contains("1/2"), "ceiling report: {out}");
    assert!(out.contains("no fixture"), "no-secrets has no fixture: {out}");
}

#[test]
fn energy_budget_over_threshold_critiques_an_affirm() {
    let env = setup();
    pick(&env);
    let public_write = r#"{"tool_name":"Write","tool_input":{"file_path":"knowledge/public/note"}}"#;
    // under threshold: a public write affirms (no rule, low energy).
    let (c0, o0, _e0) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(public_write),
    );
    assert_eq!(c0, 0);
    assert!(o0.is_empty(), "under budget -> affirm, got: {o0}");

    // spend over the 100k threshold (real measured tokens, recorded by the source).
    let (rc, _ro, _re) = run(
        &["budget", "record", "--config", env.config.to_str().unwrap(), "--tokens", "150000"],
        None,
    );
    assert_eq!(rc, 0);

    // now the SAME affirm action is raised to a recoverable critique (veto-only pressure).
    let (c1, _o1, e1) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(public_write),
    );
    assert_eq!(c1, 0, "stderr: {e1}");
    assert!(_o1.contains("\"permissionDecision\":\"deny\""), "over budget -> energy critique blocks: {_o1}");
    assert!(_o1.contains("energy budget"), "energy critique message: {_o1}");
}

#[test]
fn energy_budget_never_relaxes_a_deny() {
    let env = setup();
    pick(&env);
    // spend way over threshold, then hit a deny rule — energy must NOT relax the deny.
    run(&["budget", "record", "--config", env.config.to_str().unwrap(), "--tokens", "999999"], None);
    let secret_write = r#"{"tool_name":"Write","tool_input":{"file_path":"knowledge/secret/key"}}"#;
    let (code, out, _e) = run(
        &["hook", "--harness", "claude", "--config", env.config.to_str().unwrap(), "--mode", "block"],
        Some(secret_write),
    );
    assert_eq!(code, 0);
    assert!(out.contains("\"permissionDecision\":\"deny\""), "deny must stand under any energy: {out}");
}

#[test]
fn compile_target_dir_writes_advisory_files() {
    let env = setup();
    // add an advisory rule alongside the gate rules, then re-sign the Canon.
    fs::write(env.canon_root.join("rules/verify.md"), VERIFY_ADVISORY).unwrap();
    pick(&env);
    let target = tempfile::tempdir().unwrap();

    // claude: writes .claude/rules/flint-advisory.md
    let (code, _o, e) = run(
        &[
            "compile", "--harness", "claude", "--config", env.config.to_str().unwrap(),
            "--target-dir", target.path().to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "compile claude failed: {e}");
    let rules = fs::read_to_string(target.path().join(".claude/rules/flint-advisory.md")).unwrap();
    assert!(rules.contains("verify-before-claiming"), "advisory id in rules: {rules}");
    assert!(rules.contains("Verify external state before asserting it."));
    assert!(rules.contains("Applies when: before-reporting-status"));

    // codex: upserts the flint block in AGENTS.md, preserving user content
    fs::write(target.path().join("AGENTS.md"), "# My project\n\nuser rules here\n").unwrap();
    let (code2, _o2, e2) = run(
        &[
            "compile", "--harness", "codex", "--config", env.config.to_str().unwrap(),
            "--target-dir", target.path().to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code2, 0, "compile codex failed: {e2}");
    let agents = fs::read_to_string(target.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("user rules here"), "user content preserved: {agents}");
    assert!(agents.contains("<!-- flint:begin -->"));
    assert!(agents.contains("verify-before-claiming"));
    assert!(agents.contains("Behavioral guidelines"));
}

#[test]
fn compile_removes_stale_advisory_when_none() {
    // setup()'s Canon has only gate rules (lsp/no-secrets), no advisory.
    let env = setup();
    pick(&env);
    // Turn flint's L1 capture default OFF to isolate the pure "no advisory rules → remove stale"
    // path — with auto_mine ON (the default) the advisory is never empty (the nudge is appended;
    // asserted in compile_writes_capture_nudge_when_auto_mine_on).
    let mut cfg_text = fs::read_to_string(&env.config).unwrap();
    cfg_text.push_str("\n[capture]\nauto_mine = false\n");
    fs::write(&env.config, cfg_text).unwrap();
    let target = tempfile::tempdir().unwrap();
    // pre-create a stale managed advisory file from a prior compile.
    let rules_dir = target.path().join(".claude/rules");
    fs::create_dir_all(&rules_dir).unwrap();
    let stale = rules_dir.join("flint-advisory.md");
    fs::write(&stale, "# old advisory that should be removed\n").unwrap();
    // compiling with no advisory rules must REMOVE the stale file (codex P2-2).
    let (code, _o, e) = run(
        &[
            "compile", "--harness", "claude", "--config", env.config.to_str().unwrap(),
            "--target-dir", target.path().to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "compile failed: {e}");
    assert!(!stale.exists(), "stale advisory file must be removed when there are no advisory rules");
}

#[test]
fn compile_writes_capture_nudge_when_auto_mine_on() {
    // With flint's L1 capture default ON (the init default), compile emits the capture nudge into
    // the advisory file even when the Canon carries no advisory rules — this is what makes
    // out-of-box "the agent feeds the loop" work without the owner authoring anything.
    let env = setup();
    pick(&env);
    let target = tempfile::tempdir().unwrap();
    let (code, _o, e) = run(
        &[
            "compile", "--harness", "claude", "--config", env.config.to_str().unwrap(),
            "--target-dir", target.path().to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "compile failed: {e}");
    let advisory = target.path().join(".claude/rules/flint-advisory.md");
    let body = fs::read_to_string(&advisory).expect("advisory written when auto_mine on");
    assert!(body.contains("Flint capture — feed the loop"), "capture nudge must be present:\n{body}");
    assert!(body.contains("flint pit mark"));
}

fn _assert_path(_p: &Path) {}
