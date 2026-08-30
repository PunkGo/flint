use std::path::Path;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

pub(crate) const SESSION_SYNC_ID: &str = "flint-session-sync";
pub(crate) const SESSION_SYNC_EVENT: &str = "SessionStart";
pub(crate) const SESSION_SYNC_STATUS: &str = "Flint: sync suite";

pub(crate) fn validate_repo_head(head: &str) -> Result<()> {
    if !matches!(head.len(), 40 | 64) || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("approved repo HEAD must be a 40- or 64-character hexadecimal Git object ID");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ManagedHook {
    pub(crate) identity: &'static str,
    pub(crate) event: String,
    pub(crate) group: Value,
}

pub(crate) fn session_sync(
    exe: &Path,
    config: &Path,
    manifest: &Path,
    approved_repo_head: &str,
    event: &str,
    matcher: &str,
    timeout: u64,
) -> Result<ManagedHook> {
    if event != SESSION_SYNC_EVENT {
        bail!("flint-session-sync only supports SessionStart, got `{event}`");
    }
    validate_repo_head(approved_repo_head)?;
    let command = format!(
        "{} install --quiet --expected-repo-head {approved_repo_head} --stage full --config {} --manifest {}",
        shell_quote(exe)?,
        shell_quote(config)?,
        shell_quote(manifest)?
    );
    Ok(ManagedHook {
        identity: SESSION_SYNC_ID,
        event: event.to_string(),
        group: json!({
            "matcher": matcher,
            "hooks": [{
                "type": "command",
                "command": command,
                "timeout": timeout,
                "statusMessage": SESSION_SYNC_STATUS
            }]
        }),
    })
}

pub(crate) fn rendered_fragment(hook: &ManagedHook) -> String {
    serde_json::to_string_pretty(&hook.group).expect("ManagedHook is serializable")
}

pub(crate) fn reconcile(
    existing: Option<&str>,
    desired: Option<&ManagedHook>,
    identity: &str,
) -> Result<Option<String>> {
    if identity != SESSION_SYNC_ID {
        bail!("unknown Codex hook identity `{identity}`");
    }
    if existing.is_none() && desired.is_none() {
        return Ok(None);
    }

    let mut root: Value = match existing {
        Some(text) => serde_json::from_str(text)?,
        None => json!({"hooks": {}}),
    };
    if !root.is_object() {
        bail!("Codex hooks target must be a JSON object");
    }

    let positions = owned_positions(&root, SESSION_SYNC_EVENT)?;
    if positions.len() > 1 {
        bail!("duplicate Codex hook identity `{identity}`");
    }
    if let Some(index) = positions.first().copied() {
        let handlers = root["hooks"][SESSION_SYNC_EVENT][index]["hooks"]
            .as_array()
            .ok_or_else(|| anyhow!("owned Codex hook group has no hooks array"))?;
        if handlers.len() != 1 {
            bail!("owned Codex hook group mixes Flint and non-Flint handlers");
        }
        if !is_flint_session_sync_handler(&handlers[0]) {
            bail!("Codex hook identity collides with a non-Flint handler");
        }
    }

    match desired {
        Some(hook) => {
            if hook.identity != identity || hook.event != SESSION_SYNC_EVENT {
                bail!("desired Codex hook identity/event mismatch");
            }
            if let Some(index) = positions.first().copied() {
                if root["hooks"][SESSION_SYNC_EVENT][index] == hook.group {
                    return Ok(existing.map(str::to_owned));
                }
                root["hooks"][SESSION_SYNC_EVENT][index] = hook.group.clone();
            } else {
                let hooks = root
                    .as_object_mut()
                    .expect("root was validated as an object")
                    .entry("hooks")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .ok_or_else(|| anyhow!("`hooks` must be an object"))?;
                let groups = hooks
                    .entry(SESSION_SYNC_EVENT)
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .ok_or_else(|| anyhow!("SessionStart must be an array"))?;
                groups.push(hook.group.clone());
            }
        }
        None => {
            let Some(index) = positions.first().copied() else {
                return Ok(existing.map(str::to_owned));
            };
            root["hooks"][SESSION_SYNC_EVENT]
                .as_array_mut()
                .expect("owned position came from a SessionStart array")
                .remove(index);
        }
    }

    let mut out = serde_json::to_string_pretty(&root)?;
    out.push('\n');
    Ok(Some(out))
}

fn owned_positions(root: &Value, event: &str) -> Result<Vec<usize>> {
    let Some(hooks) = root.get("hooks") else {
        return Ok(Vec::new());
    };
    let hooks = hooks
        .as_object()
        .ok_or_else(|| anyhow!("`hooks` must be an object"))?;
    let Some(groups) = hooks.get(event) else {
        return Ok(Vec::new());
    };
    let groups = groups
        .as_array()
        .ok_or_else(|| anyhow!("{event} must be an array"))?;
    Ok(groups
        .iter()
        .enumerate()
        .filter_map(|(index, group)| {
            let handlers = group.get("hooks")?.as_array()?;
            handlers
                .iter()
                .any(|handler| {
                    handler.get("statusMessage").and_then(Value::as_str)
                        == Some(SESSION_SYNC_STATUS)
                })
                .then_some(index)
        })
        .collect())
}

fn is_flint_session_sync_handler(handler: &Value) -> bool {
    handler.get("type").and_then(Value::as_str) == Some("command")
        && handler.get("statusMessage").and_then(Value::as_str) == Some(SESSION_SYNC_STATUS)
        && handler
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| {
                command.contains(" install --quiet --expected-repo-head ")
                    && command.contains(" --stage full --config ")
                    && command.contains(" --manifest ")
            })
}

#[cfg(windows)]
fn shell_quote(path: &Path) -> Result<String> {
    let value = path.to_string_lossy();
    let value = if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value.into_owned()
    };
    let value = value.replace('\\', "/");
    if value.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '\0' | '%' | '!' | '"' | '&' | '|' | '<' | '>' | '^' | '(' | ')'
            )
    }) {
        bail!("Windows hook path cannot be represented as an unquoted cmd.exe token");
    }
    Ok(value)
}

#[cfg(not(windows))]
fn shell_quote(path: &Path) -> Result<String> {
    let value = path.to_string_lossy();
    if value.chars().any(|c| matches!(c, '\0' | '\r' | '\n')) {
        bail!("POSIX hook path contains an unsafe character");
    }
    Ok(format!("'{}'", value.replace('\'', "'\"'\"'")))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const APPROVED_HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    fn test_hook() -> ManagedHook {
        session_sync(
            Path::new("/opt/flint"),
            Path::new("/home/u/.flint/flint.toml"),
            Path::new("/src/flint/scripts/manifest.toml"),
            APPROVED_HEAD,
            SESSION_SYNC_EVENT,
            "startup|resume",
            30,
        )
        .unwrap()
    }

    #[test]
    fn preserves_jack_and_returns_original_bytes_when_current() {
        let existing = r#"{
  "hooks": {
    "SessionStart": [{"matcher":"startup|resume","hooks":[{"type":"command","command":"jack ingest","timeout":10}]}]
  }
}
"#;
        let desired = test_hook();
        let installed = reconcile(Some(existing), Some(&desired), SESSION_SYNC_ID)
            .unwrap()
            .unwrap();
        assert!(installed.contains("jack ingest"));
        let second = reconcile(Some(&installed), Some(&desired), SESSION_SYNC_ID)
            .unwrap()
            .unwrap();
        assert_eq!(second, installed);
    }

    #[test]
    fn removes_only_flint_group() {
        let desired = test_hook();
        let installed = reconcile(
            Some(
                r#"{"hooks":{"SessionStart":[{"matcher":"startup|resume","hooks":[{"type":"command","command":"jack ingest"}]}]}}"#,
            ),
            Some(&desired),
            SESSION_SYNC_ID,
        )
        .unwrap()
        .unwrap();
        let removed = reconcile(Some(&installed), None, SESSION_SYNC_ID)
            .unwrap()
            .unwrap();
        assert!(removed.contains("jack ingest"));
        assert!(!removed.contains(SESSION_SYNC_STATUS));
    }

    #[test]
    fn duplicate_or_mixed_identity_refuses() {
        let duplicate = format!(
            r#"{{"hooks":{{"SessionStart":[
      {{"matcher":"startup","hooks":[{{"type":"command","command":"a","statusMessage":"{0}"}}]}},
      {{"matcher":"resume","hooks":[{{"type":"command","command":"b","statusMessage":"{0}"}}]}}
    ]}}}}"#,
            SESSION_SYNC_STATUS
        );
        assert!(reconcile(Some(&duplicate), Some(&test_hook()), SESSION_SYNC_ID).is_err());

        let mixed = format!(
            r#"{{"hooks":{{"SessionStart":[{{"matcher":"startup|resume","hooks":[
      {{"type":"command","command":"flint","statusMessage":"{0}"}},
      {{"type":"command","command":"jack"}}
    ]}}]}}}}"#,
            SESSION_SYNC_STATUS
        );
        assert!(reconcile(Some(&mixed), Some(&test_hook()), SESSION_SYNC_ID).is_err());
    }

    #[test]
    fn invalid_json_refuses() {
        assert!(reconcile(Some("{broken"), Some(&test_hook()), SESSION_SYNC_ID).is_err());
    }

    #[test]
    fn legal_but_wrong_json_shapes_refuse() {
        for text in ["[]", r#"{"hooks":[]}"#, r#"{"hooks":{"SessionStart":{}}}"#] {
            assert!(
                reconcile(Some(text), Some(&test_hook()), SESSION_SYNC_ID).is_err(),
                "wrong JSON shape must be rejected: {text}"
            );
        }
    }

    #[test]
    fn same_status_non_flint_handler_refuses() {
        let collision = format!(
            r#"{{"hooks":{{"SessionStart":[{{"hooks":[{{"type":"command","command":"user command","statusMessage":"{}"}}]}}]}}}}"#,
            SESSION_SYNC_STATUS,
        );
        assert!(reconcile(Some(&collision), Some(&test_hook()), SESSION_SYNC_ID).is_err());
    }

    #[test]
    fn non_session_start_event_refuses() {
        assert!(
            session_sync(
                Path::new("flint"),
                Path::new("config"),
                Path::new("manifest"),
                APPROVED_HEAD,
                "Stop",
                "startup|resume",
                30,
            )
            .is_err()
        );
    }

    #[test]
    fn rendered_fragment_is_exactly_the_owned_group() {
        let hook = test_hook();
        let rendered: serde_json::Value = serde_json::from_str(&rendered_fragment(&hook)).unwrap();
        assert_eq!(rendered, hook.group);
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_rejects_paths_that_need_cmd_quotes() {
        assert!(
            session_sync(
                Path::new(r"C:\Program Files\flint.exe"),
                Path::new(r"C:\Users\u\.flint\flint.toml"),
                Path::new(r"D:\src\flint\scripts\manifest.toml"),
                APPROVED_HEAD,
                SESSION_SYNC_EVENT,
                "startup|resume",
                30,
            )
            .is_err()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_survives_codex_cmd_c_single_argument() {
        let temp = tempfile::tempdir().unwrap();
        let stub = temp.path().join("flint-stub.cmd");
        std::fs::write(&stub, "@exit /b 0\r\n").unwrap();

        let hook = session_sync(
            &stub,
            Path::new(r"C:\Users\u\.flint\flint.toml"),
            Path::new(r"D:\src\flint\scripts\manifest.toml"),
            APPROVED_HEAD,
            SESSION_SYNC_EVENT,
            "startup|resume",
            30,
        )
        .unwrap();
        let command = hook.group["hooks"][0]["command"].as_str().unwrap();

        let status = std::process::Command::new("cmd.exe")
            .arg("/C")
            .arg(command)
            .status()
            .unwrap();

        assert!(
            status.success(),
            "Codex passes the generated hook to `cmd.exe /C` as one argument"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_strips_canonical_verbatim_prefixes() {
        let hook = session_sync(
            Path::new(r"\\?\C:\tools\flint.exe"),
            Path::new(r"\\?\C:\Users\u\.flint\flint.toml"),
            Path::new(r"\\?\D:\src\flint\scripts\manifest.toml"),
            APPROVED_HEAD,
            SESSION_SYNC_EVENT,
            "startup|resume",
            30,
        )
        .unwrap();
        assert_eq!(
            hook.group["hooks"][0]["command"],
            r#"C:/tools/flint.exe install --quiet --expected-repo-head 0123456789abcdef0123456789abcdef01234567 --stage full --config C:/Users/u/.flint/flint.toml --manifest D:/src/flint/scripts/manifest.toml"#
        );

        let unc = session_sync(
            Path::new(r"\\?\UNC\server\share\flint.exe"),
            Path::new(r"\\?\C:\Users\u\.flint\flint.toml"),
            Path::new(r"\\?\D:\src\flint\scripts\manifest.toml"),
            APPROVED_HEAD,
            SESSION_SYNC_EVENT,
            "startup|resume",
            30,
        )
        .unwrap();
        assert!(
            unc.group["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .starts_with("//server/share/flint.exe install --quiet ")
        );
        assert!(
            !unc.group["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("//?/")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_rejects_environment_expansion_paths() {
        for exe in [r"C:\%FLINT_BIN%\flint.exe", r"C:\!FLINT_BIN!\flint.exe"] {
            assert!(
                session_sync(
                    Path::new(exe),
                    Path::new(r"C:\Users\u\.flint\flint.toml"),
                    Path::new(r"D:\src\flint\scripts\manifest.toml"),
                    APPROVED_HEAD,
                    SESSION_SYNC_EVENT,
                    "startup|resume",
                    30,
                )
                .is_err(),
                "unsafe expansion path must be rejected: {exe}"
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_command_single_quotes_paths() {
        let hook = session_sync(
            Path::new("/opt/flint's bin/flint"),
            Path::new("/home/u/.flint/flint.toml"),
            Path::new("/src/flint/scripts/manifest.toml"),
            APPROVED_HEAD,
            SESSION_SYNC_EVENT,
            "startup|resume",
            30,
        )
        .unwrap();
        assert_eq!(
            hook.group["hooks"][0]["command"],
            r#"'/opt/flint'"'"'s bin/flint' install --quiet --expected-repo-head 0123456789abcdef0123456789abcdef01234567 --stage full --config '/home/u/.flint/flint.toml' --manifest '/src/flint/scripts/manifest.toml'"#
        );
    }

    #[test]
    fn command_path_with_newline_refuses() {
        assert!(
            session_sync(
                Path::new("bad\npath"),
                Path::new("config"),
                Path::new("manifest"),
                APPROVED_HEAD,
                SESSION_SYNC_EVENT,
                "startup|resume",
                30,
            )
            .is_err()
        );
    }

    #[test]
    fn command_rejects_invalid_approved_repo_head() {
        for head in ["", "abc", "gggggggggggggggggggggggggggggggggggggggg"] {
            assert!(
                session_sync(
                    Path::new("flint"),
                    Path::new("config"),
                    Path::new("manifest"),
                    head,
                    SESSION_SYNC_EVENT,
                    "startup|resume",
                    30,
                )
                .is_err()
            );
        }
    }
}
