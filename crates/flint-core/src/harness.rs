//! Harness adapters (PIVOT Contract B input side · reframe-and-diff §3.5.1 / striker).
//!
//! ONE judge (`touchstone`), MANY harnesses. Each harness delivers a PreToolUse hook
//! JSON in its own shape; this maps that shape into the neutral [`Action`] the judge
//! consumes. The INPUT differs per harness here; the VERDICT output is NOT fully shared —
//! Claude Code and Codex both block on `hookSpecificOutput.permissionDecision`, while Grok
//! ignores that envelope entirely and blocks only on `{"decision":"deny"}` (measured
//! 2026-08-20), so the emitter forks too (see `flint-cli`'s `hook::enforce`). The exit code
//! is part of NO harness's protocol — measured 2026-08-08, codex on Windows reads a
//! non-zero PreToolUse exit as a hook error and runs the command anyway.
//!
//! HONEST BOUNDARY (§12.6) per harness, from the official specs:
//!   - Claude Code: PreToolUse fires for Write/Edit/MultiEdit/NotebookEdit (clean
//!     `file_path`), Read/LS/Glob/Grep, Bash (`command`). Scope + command rules both
//!     enforce.
//!   - Codex: PreToolUse fires for Bash (`tool_input.command`), `apply_patch` (file
//!     edits — path is INSIDE the patch text in `tool_input.command`), and MCP tools.
//!     Command rules enforce on Bash. Scope rules on Codex depend on parsing the
//!     apply_patch envelope AND on apply_patch interception being reliable (community
//!     reports it as flaky mid-2026) — so file-scope governance on Codex is ALSO
//!     emitted as AGENTS.md advisory (the data path), not relied on as enforcement.
//!   - Grok: PreToolUse fires for every tool, with a camelCase top level and a per-tool
//!     `toolInput` key set. Scope + command rules both enforce. Grok is fail-OPEN by
//!     design (a crashed, slow, or malformed hook lets the call through), so flint's own
//!     fail-CLOSED path has to speak Grok's deny shape or it is fail-closed in name only.

use serde_json::Value;

use crate::glob::{
    is_synthetic_scope, normalize_path_scope, normalize_separators, relativize_to_workspace,
};
use crate::touchstone::Action;

/// Which harness produced the hook JSON. Selects the input adapter AND the verdict
/// envelope (Grok does not honour the Claude/Codex `permissionDecision` shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    Grok,
}

impl Harness {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "claude" => Ok(Harness::Claude),
            "codex" => Ok(Harness::Codex),
            "grok" => Ok(Harness::Grok),
            other => Err(format!("--harness must be claude|codex|grok, got {other}")),
        }
    }
    pub fn tag(&self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Grok => "grok",
        }
    }
}

/// Map a harness PreToolUse hook JSON into the neutral [`Action`]. FALLIBLE: a malformed
/// JSON or a KNOWN mutating tool with no usable target is `Err` — the caller fails CLOSED
/// in block mode (an underivable target must NOT become an empty scope set that Affirms).
pub fn derive_action(harness: Harness, hook: &Value) -> Result<Action, String> {
    match harness {
        Harness::Claude => derive_claude(hook),
        Harness::Codex => derive_codex(hook),
        Harness::Grok => derive_grok(hook),
    }
}

/// Claude Code PreToolUse adapter (ported from the M10 `flint-cli::hook::derive_target`,
/// confirmed current against code.claude.com/docs/en/hooks 2026-06).
fn derive_claude(hook: &Value) -> Result<Action, String> {
    let tool = hook
        .get("tool_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "hook JSON has no (non-empty) tool_name".to_string())?;
    let input = hook.get("tool_input").cloned().unwrap_or(Value::Null);
    // NORMALIZE the harness-reported path at the boundary (codex P1): a spelling like
    // `./knowledge/secret/key` must collapse to its canonical form BEFORE the lexical
    // matcher sees it, or it dodges a `knowledge/secret/**` deny while the FS still writes
    // inside the denied tree. See `glob::normalize_path_scope`.
    let path: Option<String> = ["file_path", "notebook_path", "path"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(normalize_path_scope);
    let path = path.as_deref();

    let tool_kind = match tool {
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => "write",
        "Read" | "LS" => "read",
        "Glob" | "Grep" => "search",
        "Bash" => "exec",
        _ => "tool",
    }
    .to_string();

    let scopes = match tool {
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => match path {
            Some(p) => vec![p.to_string()],
            None => return Err(format!("{tool} has no file_path/notebook_path to gate")),
        },
        "Read" | "LS" | "Glob" | "Grep" => match path {
            Some(p) => vec![p.to_string()],
            None => vec![format!("tool:{tool}")],
        },
        "Bash" => vec!["exec:bash".to_string()],
        other => vec![format!("tool:{other}")],
    };
    let command = if tool == "Bash" {
        input.get("command").and_then(Value::as_str).map(str::to_string)
    } else {
        None
    };
    Ok(Action {
        actor_id: String::new(),
        tool_kind,
        scopes,
        command,
        context: String::new(),
        energy: None,
    })
}

/// Codex CLI PreToolUse adapter (per developers.openai.com/codex/hooks 2026-06).
/// `tool_name` is "Bash" | "apply_patch" | "mcp__<server>__<tool>" | other; for Bash and
/// apply_patch the payload is `tool_input.command` (apply_patch's "command" is the PATCH
/// text, whose `*** {Add,Update,Delete} File:` lines name the target paths).
fn derive_codex(hook: &Value) -> Result<Action, String> {
    let tool = hook
        .get("tool_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "hook JSON has no (non-empty) tool_name".to_string())?;
    let input = hook.get("tool_input").cloned().unwrap_or(Value::Null);
    let command_text = input.get("command").and_then(Value::as_str);

    match tool {
        "Bash" => Ok(Action {
            actor_id: String::new(),
            tool_kind: "exec".into(),
            scopes: vec!["exec:bash".into()],
            command: command_text.map(str::to_string),
            context: String::new(),
            energy: None,
        }),
        "apply_patch" => {
            // The path(s) live in the patch text. A patch with NO parseable target is an
            // Err (fail-closed: a file edit we can't locate must not silently Affirm).
            let patch = command_text
                .ok_or_else(|| "apply_patch has no tool_input.command (patch text)".to_string())?;
            let scopes = apply_patch_paths(patch);
            if scopes.is_empty() {
                return Err("apply_patch patch names no target file (cannot gate)".to_string());
            }
            Ok(Action {
                actor_id: String::new(),
                tool_kind: "write".into(),
                scopes,
                command: None, // the patch body is not a shell command; don't feed it to command rules
                context: String::new(),
                energy: None,
            })
        }
        other => Ok(Action {
            actor_id: String::new(),
            tool_kind: "tool".into(),
            scopes: vec![format!("tool:{other}")],
            command: None,
            context: String::new(),
            energy: None,
        }),
    }
}

/// Read a TOP-LEVEL hook field under either spelling, camelCase FIRST. Grok's own runtime
/// delivers camelCase (`toolName`), while a hook registered through the official Grok SDK
/// re-emits the same event in snake_case (`tool_name`) — one adapter has to read both, and
/// a present-but-null camel key must not shadow a real snake one.
fn dual_key<'a>(hook: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    hook.get(camel).filter(|v| !v.is_null()).or_else(|| hook.get(snake))
}

/// Grok (xAI Grok Build) PreToolUse adapter.
///
/// PROTOCOL ANCHOR: `~/.grok/docs/user-guide/10-hooks.md` @ grok 1.0.5 (macOS and Windows installs
/// byte-identical) plus the measured wire receipts in `~/.flint/grok-wire/wire-facts.md`
/// (2026-08-20). Everything below is from those receipts, not from inference:
///   - the top level is camelCase (`toolName` / `toolInput` / `toolInputTruncated` /
///     `workspaceRoot` / `cwd` / `hookEventName`, whose VALUE is snake `pre_tool_use`);
///   - each tool names its target under its own key — `run_terminal_command.command`,
///     `write`/`search_replace`.`file_path`, `read_file.target_file`, `grep.path`
///     (OMITTED when the agent greps the whole tree), `list_dir.target_directory`;
///   - paths arrive relative on mac and may arrive drive-absolute with `\` separators on
///     Windows, so every path value goes through `normalize_separators` before the shared
///     boundary normalization.
///
/// FAIL-CLOSED on `toolInputTruncated: true`: a truncated payload may have had the very
/// path or command the gate exists to see cut out of it, and an underivable target must
/// never become an empty scope set that Affirms.
fn derive_grok(hook: &Value) -> Result<Action, String> {
    let tool = dual_key(hook, "toolName", "tool_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "grok hook JSON has no (non-empty) toolName".to_string())?;
    if dual_key(hook, "toolInputTruncated", "tool_input_truncated").and_then(Value::as_bool)
        == Some(true)
    {
        return Err(format!(
            "grok reports toolInput TRUNCATED for {tool} — the target may be cut away, failing closed"
        ));
    }
    let input = dual_key(hook, "toolInput", "tool_input").cloned().unwrap_or(Value::Null);

    // NORMALIZE at the boundary, separators first (Windows `\`) then structurally, so a
    // `.\knowledge\secret\key` spelling cannot dodge a `knowledge/secret/**` deny that the
    // filesystem write still lands inside. See `glob::normalize_separators`.
    let path_scope = |key: &str| -> Option<String> {
        input
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| normalize_path_scope(&normalize_separators(p)))
    };

    let (tool_kind, scopes, command) = match tool {
        "run_terminal_command" => (
            "exec",
            vec!["exec:bash".to_string()],
            input.get("command").and_then(Value::as_str).map(str::to_string),
        ),
        // A file edit whose target we cannot read is an Err, not an empty scope set.
        "write" | "search_replace" => {
            let p = path_scope("file_path")
                .ok_or_else(|| format!("grok {tool} has no toolInput.file_path to gate"))?;
            ("write", vec![p], None)
        }
        // The read-shaped tools may legitimately omit their target (a whole-tree grep);
        // those fall back to the synthetic tool scope rather than failing closed.
        "read_file" => (
            "read",
            vec![path_scope("target_file").unwrap_or_else(|| "tool:read_file".to_string())],
            None,
        ),
        "grep" => (
            "search",
            vec![path_scope("path").unwrap_or_else(|| "tool:grep".to_string())],
            None,
        ),
        "list_dir" => (
            "read",
            vec![path_scope("target_directory").unwrap_or_else(|| "tool:list_dir".to_string())],
            None,
        ),
        // web_search / image_gen / use_tool / spawn_subagent / a qualified MCP
        // `server__tool` / anything Grok adds later.
        other => ("tool", vec![format!("tool:{other}")], None),
    };

    Ok(Action {
        actor_id: String::new(),
        tool_kind: tool_kind.to_string(),
        scopes,
        command,
        context: String::new(),
        energy: None,
    })
}

/// The workspace root the HARNESS itself reported on stdin, if it reports one. Only Grok
/// does: its PreToolUse payload carries `workspaceRoot` (measured WITH a trailing slash,
/// which the scope normalizer eats). This matters on Windows, where the hook process's cwd
/// is not guaranteed to be the workspace — and the harness's own word beats a guess.
/// Returns `None` for every other harness and for a payload that names no root.
pub fn stdin_workspace_root(harness: Harness, hook: &Value) -> Option<String> {
    if harness != Harness::Grok {
        return None;
    }
    dual_key(hook, "workspaceRoot", "workspace_root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string)
}

/// Extract EVERY target file path from an OpenAI `apply_patch` envelope: the args after
/// `*** Add File:`, `*** Update File:`, `*** Delete File:`, AND `*** Move to:` (a rename's
/// DESTINATION — codex P1: without it a patch that updates an allowed path then moves it
/// into a denied scope evades a scope deny). Each path is NORMALIZED at the boundary
/// (codex P1: `*** Add File: ./knowledge/secret/key` must collapse so it can't dodge a
/// `knowledge/secret/**` deny — see `glob::normalize_path_scope`), then deduped on the
/// canonical form in first-seen order. A path-bearing marker with no arg is ignored (the
/// no-target case is caught by the Err in `derive_codex`).
pub fn apply_patch_paths(patch: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in patch.lines() {
        let l = line.trim();
        for marker in ["*** Add File:", "*** Update File:", "*** Delete File:", "*** Move to:"] {
            if let Some(rest) = l.strip_prefix(marker) {
                let raw = rest.trim();
                if raw.is_empty() {
                    continue;
                }
                let p = normalize_path_scope(raw);
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Relativize a derived Action's FILE-PATH scopes against the workspace root so the lexical
/// gate globs (written relative to the workspace) match regardless of how the harness
/// spelled the path — ABSOLUTE (`/abs/.../knowledge/secret/key`, the normal Claude case) or
/// `..`-escaping (codex P1 r2). Synthetic scopes (`exec:`/`tool:`) pass through unchanged. A
/// file-path scope that resolves OUTSIDE the workspace is dropped (no relative in-tree glob
/// governs it). `workspace_root` is the absolute dir the harness runs the hook in.
pub fn relativize_scopes(action: &mut Action, workspace_root: &str) {
    action.scopes = action
        .scopes
        .iter()
        .filter_map(|s| {
            if is_synthetic_scope(s) {
                Some(s.clone())
            } else {
                relativize_to_workspace(s, workspace_root)
            }
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_write_maps_file_path() {
        let h = json!({ "tool_name": "Write", "tool_input": { "file_path": "knowledge/secret/x" } });
        let a = derive_action(Harness::Claude, &h).unwrap();
        assert_eq!(a.scopes, vec!["knowledge/secret/x"]);
        assert_eq!(a.tool_kind, "write");
        assert_eq!(a.command, None);
    }

    #[test]
    fn claude_bash_carries_command() {
        let h = json!({ "tool_name": "Bash", "tool_input": { "command": "grep -rn foo src/x.rs" } });
        let a = derive_action(Harness::Claude, &h).unwrap();
        assert_eq!(a.scopes, vec!["exec:bash"]);
        assert_eq!(a.command.as_deref(), Some("grep -rn foo src/x.rs"));
    }

    #[test]
    fn claude_write_without_path_is_err() {
        let h = json!({ "tool_name": "Write", "tool_input": {} });
        assert!(derive_action(Harness::Claude, &h).is_err());
    }

    #[test]
    fn codex_bash_same_as_claude() {
        let h = json!({ "tool_name": "Bash", "tool_input": { "command": "rg foo x.ts" } });
        let a = derive_action(Harness::Codex, &h).unwrap();
        assert_eq!(a.scopes, vec!["exec:bash"]);
        assert_eq!(a.command.as_deref(), Some("rg foo x.ts"));
        assert_eq!(a.tool_kind, "exec");
    }

    #[test]
    fn codex_apply_patch_extracts_paths() {
        let patch = "*** Begin Patch\n*** Update File: src/main.rs\n@@ fn main\n-old\n+new\n*** Add File: knowledge/secret/key\n*** End Patch\n";
        let h = json!({ "tool_name": "apply_patch", "tool_input": { "command": patch } });
        let a = derive_action(Harness::Codex, &h).unwrap();
        assert_eq!(a.scopes, vec!["src/main.rs".to_string(), "knowledge/secret/key".to_string()]);
        assert_eq!(a.tool_kind, "write");
        assert_eq!(a.command, None, "patch body must NOT be fed to command rules");
    }

    #[test]
    fn codex_apply_patch_no_target_is_err() {
        let h = json!({ "tool_name": "apply_patch", "tool_input": { "command": "*** Begin Patch\n*** End Patch\n" } });
        assert!(derive_action(Harness::Codex, &h).is_err());
    }

    #[test]
    fn codex_mcp_tool_is_namespaced() {
        let h = json!({ "tool_name": "mcp__fs__read", "tool_input": {} });
        let a = derive_action(Harness::Codex, &h).unwrap();
        assert_eq!(a.scopes, vec!["tool:mcp__fs__read"]);
    }

    #[test]
    fn missing_tool_name_is_err_both() {
        assert!(derive_action(Harness::Claude, &json!({})).is_err());
        assert!(derive_action(Harness::Codex, &json!({})).is_err());
    }

    #[test]
    fn apply_patch_paths_dedup_first_seen() {
        let patch = "*** Update File: a.rs\n*** Update File: a.rs\n*** Add File: b.rs\n";
        assert_eq!(apply_patch_paths(patch), vec!["a.rs".to_string(), "b.rs".to_string()]);
    }

    #[test]
    fn claude_path_spelling_is_normalized_at_boundary() {
        // codex P1: a `./` (or `//`, `/./`, `..`) spelling must collapse so it can't dodge
        // a `knowledge/secret/**` deny while the FS writes inside the denied tree.
        let h = json!({ "tool_name": "Write", "tool_input": { "file_path": "./knowledge/secret/key" } });
        let a = derive_action(Harness::Claude, &h).unwrap();
        assert_eq!(a.scopes, vec!["knowledge/secret/key"]);
        let h2 = json!({ "tool_name": "Write", "tool_input": { "file_path": "knowledge/./secret/../secret/key" } });
        assert_eq!(derive_action(Harness::Claude, &h2).unwrap().scopes, vec!["knowledge/secret/key"]);
    }

    #[test]
    fn codex_apply_patch_path_spelling_is_normalized() {
        // same codex P1 bypass on the codex apply_patch envelope.
        let patch = "*** Begin Patch\n*** Add File: ./knowledge/secret/key\n*** End Patch\n";
        let h = json!({ "tool_name": "apply_patch", "tool_input": { "command": patch } });
        let a = derive_action(Harness::Codex, &h).unwrap();
        assert_eq!(a.scopes, vec!["knowledge/secret/key"]);
        // `./a.rs` and `a.rs` dedup on the canonical form.
        let dedup = "*** Update File: ./a.rs\n*** Update File: a.rs\n";
        assert_eq!(apply_patch_paths(dedup), vec!["a.rs".to_string()]);
    }

    #[test]
    fn relativize_scopes_resolves_files_keeps_synthetic_drops_outside() {
        let root = "/Users/x/flint";
        // an absolute in-tree file path + a synthetic scope: file relativized, synthetic kept.
        let mut a = Action {
            tool_kind: "write".into(),
            scopes: vec!["/Users/x/flint/knowledge/secret/key".into(), "exec:bash".into()],
            ..Default::default()
        };
        relativize_scopes(&mut a, root);
        assert_eq!(a.scopes, vec!["knowledge/secret/key".to_string(), "exec:bash".to_string()]);
        // an out-of-tree absolute path is dropped (no in-tree glob governs it).
        let mut b = Action { tool_kind: "write".into(), scopes: vec!["/etc/passwd".into()], ..Default::default() };
        relativize_scopes(&mut b, root);
        assert!(b.scopes.is_empty());
    }

    // -----------------------------------------------------------------------
    // Grok (measured wire shape — see `derive_grok`'s protocol anchor)
    // -----------------------------------------------------------------------

    fn grok(tool: &str, input: Value) -> Value {
        json!({
            "hookEventName": "pre_tool_use",
            "workspaceRoot": "/ws/",
            "cwd": "/ws",
            "toolName": tool,
            "toolInput": input,
            "toolInputTruncated": false,
        })
    }

    #[test]
    fn grok_run_terminal_command_carries_the_command() {
        let a = derive_action(Harness::Grok, &grok("run_terminal_command", json!({ "command": "grep -rn foo src/main.rs", "description": "look" }))).unwrap();
        assert_eq!(a.tool_kind, "exec");
        assert_eq!(a.scopes, vec!["exec:bash"]);
        assert_eq!(a.command.as_deref(), Some("grep -rn foo src/main.rs"));
    }

    #[test]
    fn grok_write_and_search_replace_map_file_path() {
        for tool in ["write", "search_replace"] {
            let a = derive_action(Harness::Grok, &grok(tool, json!({ "file_path": "./knowledge/secret/key" }))).unwrap();
            assert_eq!(a.tool_kind, "write", "{tool}");
            assert_eq!(a.scopes, vec!["knowledge/secret/key"], "{tool} normalizes at the boundary");
            assert_eq!(a.command, None, "{tool}");
        }
    }

    #[test]
    fn grok_write_without_file_path_is_err() {
        // a file edit we cannot locate must NOT decay into an empty scope set that Affirms.
        assert!(derive_action(Harness::Grok, &grok("write", json!({ "content": "x" }))).is_err());
        assert!(derive_action(Harness::Grok, &grok("search_replace", json!({}))).is_err());
    }

    #[test]
    fn grok_read_shaped_tools_map_their_own_key() {
        let r = derive_action(Harness::Grok, &grok("read_file", json!({ "target_file": "src/main.rs" }))).unwrap();
        assert_eq!((r.tool_kind.as_str(), r.scopes.as_slice()), ("read", ["src/main.rs".to_string()].as_slice()));
        let g = derive_action(Harness::Grok, &grok("grep", json!({ "pattern": "foo", "path": "src" }))).unwrap();
        assert_eq!((g.tool_kind.as_str(), g.scopes.as_slice()), ("search", ["src".to_string()].as_slice()));
        let l = derive_action(Harness::Grok, &grok("list_dir", json!({ "target_directory": "." }))).unwrap();
        assert_eq!(l.tool_kind, "read");
        assert_eq!(l.scopes, vec!["."]);
    }

    #[test]
    fn grok_read_shaped_tools_fall_back_to_synthetic_scope() {
        // MEASURED: `grep` may arrive with ONLY a pattern (a whole-tree grep). A missing
        // target on a read-shaped tool is legal — it becomes the synthetic tool scope.
        let g = derive_action(Harness::Grok, &grok("grep", json!({ "pattern": "foo" }))).unwrap();
        assert_eq!(g.scopes, vec!["tool:grep"]);
        let r = derive_action(Harness::Grok, &grok("read_file", json!({}))).unwrap();
        assert_eq!(r.scopes, vec!["tool:read_file"]);
        let l = derive_action(Harness::Grok, &grok("list_dir", json!({}))).unwrap();
        assert_eq!(l.scopes, vec!["tool:list_dir"]);
    }

    #[test]
    fn grok_unknown_and_mcp_tools_are_namespaced() {
        for tool in ["web_search", "image_gen", "use_tool", "spawn_subagent", "server__tool"] {
            let a = derive_action(Harness::Grok, &grok(tool, json!({}))).unwrap();
            assert_eq!(a.tool_kind, "tool", "{tool}");
            assert_eq!(a.scopes, vec![format!("tool:{tool}")]);
            assert_eq!(a.command, None);
        }
    }

    #[test]
    fn grok_truncated_input_fails_closed() {
        // a truncated payload may have had the very path/command the gate exists to see
        // cut away — Err, so block+closed denies instead of Affirming an empty scope set.
        let h = json!({ "toolName": "write", "toolInput": { "file_path": "a.rs" }, "toolInputTruncated": true });
        assert!(derive_action(Harness::Grok, &h).is_err());
        // the snake spelling fails closed identically.
        let snake = json!({ "tool_name": "write", "tool_input": { "file_path": "a.rs" }, "tool_input_truncated": true });
        assert!(derive_action(Harness::Grok, &snake).is_err());
        // false / absent is the normal case and derives fine.
        assert!(derive_action(Harness::Grok, &grok("write", json!({ "file_path": "a.rs" }))).is_ok());
        let absent = json!({ "toolName": "write", "toolInput": { "file_path": "a.rs" } });
        assert!(derive_action(Harness::Grok, &absent).is_ok());
    }

    #[test]
    fn grok_reads_snake_case_top_level_too() {
        // the Grok SDK re-emits the same event snake_cased; one adapter reads both.
        let h = json!({
            "hook_event_name": "pre_tool_use",
            "tool_name": "run_terminal_command",
            "tool_input": { "command": "rg foo x.ts" },
            "tool_input_truncated": false,
        });
        let a = derive_action(Harness::Grok, &h).unwrap();
        assert_eq!(a.scopes, vec!["exec:bash"]);
        assert_eq!(a.command.as_deref(), Some("rg foo x.ts"));
        // camel WINS when both are present, and a null camel key does not shadow snake.
        let both = json!({ "toolName": "write", "tool_name": "read_file", "toolInput": { "file_path": "a.rs" } });
        assert_eq!(derive_action(Harness::Grok, &both).unwrap().tool_kind, "write");
        let nulled = json!({ "toolName": null, "tool_name": "read_file", "toolInput": { "target_file": "a.rs" } });
        assert_eq!(derive_action(Harness::Grok, &nulled).unwrap().tool_kind, "read");
        // no toolName under either spelling -> Err.
        assert!(derive_action(Harness::Grok, &json!({})).is_err());
        assert!(derive_action(Harness::Grok, &json!({ "toolName": "  " })).is_err());
    }

    #[test]
    fn grok_windows_path_relativizes_into_the_in_tree_deny_scope() {
        // the Windows deployment shape: a drive-absolute `\`-separated file_path must resolve
        // to the same in-tree scope a `secrets/**` deny is written against.
        let h = grok("write", json!({ "file_path": r"C:\ws\secrets\key" }));
        let mut a = derive_action(Harness::Grok, &h).unwrap();
        assert_eq!(a.scopes, vec!["C:/ws/secrets/key"], "separators normalize at the adapter");
        relativize_scopes(&mut a, "C:/ws");
        assert_eq!(a.scopes, vec!["secrets/key"]);
        // the drive letter's case is not load-bearing …
        let mut b = derive_action(Harness::Grok, &grok("write", json!({ "file_path": r"c:\ws\secrets\key" }))).unwrap();
        relativize_scopes(&mut b, r"C:\ws");
        assert_eq!(b.scopes, vec!["secrets/key"]);
        // … and a RELATIVE path against a trailing-slash root (Grok's measured
        // `workspaceRoot` shape) round-trips to the same scope.
        let mut c = derive_action(Harness::Grok, &grok("write", json!({ "file_path": r"secrets\key" }))).unwrap();
        relativize_scopes(&mut c, "C:/ws/");
        assert_eq!(c.scopes, vec!["secrets/key"]);
        // out of tree is dropped, exactly as on Unix.
        let mut d = derive_action(Harness::Grok, &grok("write", json!({ "file_path": r"D:\other\secrets\key" }))).unwrap();
        relativize_scopes(&mut d, "C:/ws");
        assert!(d.scopes.is_empty());
    }

    #[test]
    fn grok_alone_reports_a_workspace_root_from_stdin() {
        let h = grok("read_file", json!({ "target_file": "a.rs" }));
        // MEASURED: Grok's workspaceRoot carries a trailing slash; the scope normalizer eats it.
        assert_eq!(stdin_workspace_root(Harness::Grok, &h).as_deref(), Some("/ws/"));
        assert_eq!(stdin_workspace_root(Harness::Claude, &h), None, "claude never self-reports");
        assert_eq!(stdin_workspace_root(Harness::Codex, &h), None);
        let snake = json!({ "workspace_root": r"C:\ws\", "tool_name": "read_file" });
        assert_eq!(stdin_workspace_root(Harness::Grok, &snake).as_deref(), Some(r"C:\ws\"));
        assert_eq!(stdin_workspace_root(Harness::Grok, &json!({})), None);
        assert_eq!(stdin_workspace_root(Harness::Grok, &json!({ "workspaceRoot": "  " })), None);
    }

    #[test]
    fn codex_apply_patch_move_destination_is_a_scope() {
        // codex P1: a rename's DESTINATION must be judged, else a move into a denied scope
        // (knowledge/secret/**) evades the deny by only exposing the allowed source path.
        let patch = "*** Begin Patch\n*** Update File: tmp/allowed.md\n*** Move to: knowledge/secret/key.md\n@@\n x\n*** End Patch\n";
        let h = json!({ "tool_name": "apply_patch", "tool_input": { "command": patch } });
        let a = derive_action(Harness::Codex, &h).unwrap();
        assert!(a.scopes.contains(&"knowledge/secret/key.md".to_string()), "move destination must be a scope: {:?}", a.scopes);
        assert!(a.scopes.contains(&"tmp/allowed.md".to_string()));
    }
}
