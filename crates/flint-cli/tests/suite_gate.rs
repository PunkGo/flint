//! WP1b acceptance (plan v4.3 F5 + Eng-3): `flint install` end-to-end on the REAL
//! binary in a sandboxed HOME — full install/check/remove cycle, idempotency, the
//! F2 error table (batch-abort), lock conservatism (F1), allowlist escapes,
//! --quiet fail-open, $INSTANCE skip, unsigned-canon refusal for generators, and
//! block-type upsert/diff/removal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn flint_bin() -> &'static str {
    env!("CARGO_BIN_EXE_flint")
}

/// A sandbox: fake HOME + a "repo" holding manifest + sources.
struct Sandbox {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let repo = dir.path().join("repo");
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".flint")).unwrap();
        fs::create_dir_all(repo.join("scripts")).unwrap();
        fs::create_dir_all(repo.join("skills/a")).unwrap();
        fs::create_dir_all(repo.join("skills/b")).unwrap();
        fs::write(repo.join("skills/a/SKILL.md"), "---\nname: a\n---\nalpha body\n").unwrap();
        fs::write(repo.join("skills/b/SKILL.md"), "---\nname: b\n---\nbeta body\n").unwrap();
        Sandbox { _dir: dir, home, repo }
    }

    fn write_manifest(&self, body: &str) {
        fs::write(self.repo.join("scripts/manifest.toml"), body).unwrap();
    }

    fn install(&self, extra: &[&str]) -> Output {
        self.install_command(extra).output().unwrap()
    }

    fn install_command(&self, extra: &[&str]) -> Command {
        let mut cmd = Command::new(flint_bin());
        cmd.arg("install")
            .arg("--manifest")
            .arg(self.repo.join("scripts/manifest.toml"))
            .args(extra)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home);
        cmd
    }

    fn config_path(&self) -> PathBuf {
        let path = self.home.join(".flint/flint.toml");
        if !path.exists() {
            fs::write(&path, "").unwrap();
        }
        path
    }

    fn install_full(&self, extra: &[&str]) -> Output {
        let config = self.config_path();
        let mut args = vec!["--stage", "full", "--config", config.to_str().unwrap()];
        args.extend_from_slice(extra);
        self.install(&args)
    }

    fn target(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    fn lock_path(&self) -> PathBuf {
        // the sandbox home is inside a tempdir which may itself sit behind a
        // symlink (macOS /var -> /private/var): resolve like the binary does.
        self.home.canonicalize().unwrap().join(".flint/installed.lock")
    }
}

const TWO_SKILLS: &str = r#"
[[artifact]]
stage = "skills"
type = "file"
source = "skills/a/SKILL.md"
target = "$CLAUDE_HOME/skills/a/SKILL.md"

[[artifact]]
stage = "skills"
type = "file"
source = "skills/b/SKILL.md"
target = "$CLAUDE_HOME/skills/b/SKILL.md"
"#;

const ONE_SKILL: &str = r#"
[[artifact]]
stage = "skills"
type = "file"
source = "skills/a/SKILL.md"
target = "$CLAUDE_HOME/skills/a/SKILL.md"
"#;

const CODEX_SESSION_SYNC: &str = r#"
[[artifact]]
stage = "full"
type = "codex-hook"
hook = "flint-session-sync"
target = "$CODEX_HOME/hooks.json"
event = "SessionStart"
matcher = "startup|resume"
timeout = 30
"#;

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn codex_hook_preserves_jack_is_idempotent_and_checks_owned_fragment_only() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home.join(".codex")).unwrap();
    fs::write(
        sb.target(".codex/hooks.json"),
        r#"{"hooks":{"SessionStart":[{"matcher":"startup|resume","hooks":[{"type":"command","command":"jack ingest","timeout":10}]}]}}"#,
    )
    .unwrap();
    sb.write_manifest(CODEX_SESSION_SYNC);
    let approved_head = init_main(&sb.repo);

    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let installed = fs::read_to_string(sb.target(".codex/hooks.json")).unwrap();
    assert!(installed.contains("jack ingest"));
    assert!(installed.contains("Flint: sync suite"));
    assert!(installed.contains(" install --quiet --expected-repo-head "));
    assert!(installed.contains(" --stage full --config "));
    assert!(installed.contains(&format!("--expected-repo-head {approved_head}")));
    assert!(!installed.contains("//?/"));

    let lock: toml::Value = toml::from_str(&fs::read_to_string(sb.lock_path()).unwrap()).unwrap();
    let entry = lock["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["type"].as_str() == Some("codex-hook"))
        .unwrap();
    assert_eq!(entry["marker"].as_str(), Some("flint-session-sync"));
    assert_eq!(entry["rendered_sha256"].as_str().unwrap().len(), 64);

    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        fs::read_to_string(sb.target(".codex/hooks.json")).unwrap(),
        installed
    );

    let mut value: serde_json::Value = serde_json::from_str(&installed).unwrap();
    value["hooks"]["Stop"] =
        serde_json::json!([{"hooks":[{"type":"command","command":"user stop"}]}]);
    let unrelated = serde_json::to_string_pretty(&value).unwrap();
    fs::write(sb.target(".codex/hooks.json"), &unrelated).unwrap();
    let out = sb.install_full(&["--check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(
        fs::read_to_string(sb.target(".codex/hooks.json")).unwrap(),
        unrelated
    );

    let groups = value["hooks"]["SessionStart"].as_array_mut().unwrap();
    let owned = groups
        .iter_mut()
        .find(|group| {
            group["hooks"].as_array().is_some_and(|handlers| {
                handlers
                    .iter()
                    .any(|handler| handler["statusMessage"] == "Flint: sync suite")
            })
        })
        .unwrap();
    owned["matcher"] = serde_json::json!("startup");
    fs::write(
        sb.target(".codex/hooks.json"),
        serde_json::to_string_pretty(&value).unwrap(),
    )
    .unwrap();
    assert_eq!(sb.install_full(&["--check"]).status.code(), Some(1));

    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let repaired: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sb.target(".codex/hooks.json")).unwrap())
            .unwrap();
    assert_eq!(repaired["hooks"]["Stop"], value["hooks"]["Stop"]);
    let session_start = repaired["hooks"]["SessionStart"].as_array().unwrap();
    assert!(session_start.iter().any(|group| {
        group["matcher"] == "startup|resume"
            && group["hooks"].as_array().is_some_and(|handlers| {
                handlers
                    .iter()
                    .any(|handler| handler["statusMessage"] == "Flint: sync suite")
            })
    }));
    assert!(session_start.iter().any(|group| {
        group["hooks"].as_array().is_some_and(|handlers| {
            handlers
                .iter()
                .any(|handler| handler["command"] == "jack ingest")
        })
    }));
}

#[test]
fn codex_hook_first_install_creates_hooks_json() {
    let sb = Sandbox::new();
    sb.write_manifest(CODEX_SESSION_SYNC);
    init_main(&sb.repo);

    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let hooks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sb.target(".codex/hooks.json")).unwrap())
            .unwrap();
    let groups = hooks["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["hooks"][0]["statusMessage"], "Flint: sync suite");
}

#[test]
fn codex_hook_removal_preserves_jack() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home.join(".codex")).unwrap();
    fs::write(
        sb.target(".codex/hooks.json"),
        r#"{"hooks":{"SessionStart":[{"matcher":"startup|resume","hooks":[{"type":"command","command":"jack ingest"}]}]}}"#,
    )
    .unwrap();
    sb.write_manifest(CODEX_SESSION_SYNC);
    init_main(&sb.repo);
    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));

    sb.write_manifest("");
    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let hooks = fs::read_to_string(sb.target(".codex/hooks.json")).unwrap();
    assert!(hooks.contains("jack ingest"));
    assert!(!hooks.contains("Flint: sync suite"));

    sb.write_manifest(CODEX_SESSION_SYNC);
    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let restored = fs::read_to_string(sb.target(".codex/hooks.json")).unwrap();
    assert!(restored.contains("jack ingest"));
    assert!(restored.contains("Flint: sync suite"));
}

#[test]
fn codex_hook_stale_lock_removal_tolerates_missing_group() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home.join(".codex")).unwrap();
    fs::write(
        sb.target(".codex/hooks.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"jack ingest"}] }]}}"#,
    )
    .unwrap();
    sb.write_manifest(CODEX_SESSION_SYNC);
    init_main(&sb.repo);
    assert!(sb.install_full(&[]).status.success());

    let mut hooks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sb.target(".codex/hooks.json")).unwrap())
            .unwrap();
    hooks["hooks"]["SessionStart"]
        .as_array_mut()
        .unwrap()
        .retain(|group| group["hooks"][0]["statusMessage"] != "Flint: sync suite");
    fs::write(
        sb.target(".codex/hooks.json"),
        serde_json::to_string_pretty(&hooks).unwrap(),
    )
    .unwrap();

    sb.write_manifest("");
    let out = sb.install_full(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let after = fs::read_to_string(sb.target(".codex/hooks.json")).unwrap();
    assert!(after.contains("jack ingest"));
    assert!(!after.contains("Flint: sync suite"));
    let lock: toml::Value = toml::from_str(&fs::read_to_string(sb.lock_path()).unwrap()).unwrap();
    assert!(lock["installed"].as_array().unwrap().iter().all(|entry| {
        entry["marker"].as_str() != Some("flint-session-sync")
    }));
}

#[test]
fn codex_hook_invalid_json_aborts_without_write() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home.join(".codex")).unwrap();
    fs::write(sb.target(".codex/hooks.json"), "{broken").unwrap();
    sb.write_manifest(CODEX_SESSION_SYNC);
    init_main(&sb.repo);

    let out = sb.install_full(&[]);
    assert!(!out.status.success());
    assert_eq!(
        fs::read_to_string(sb.target(".codex/hooks.json")).unwrap(),
        "{broken"
    );
    assert!(!sb.lock_path().exists());
}

#[test]
fn codex_hook_requires_existing_config_and_session_start() {
    let sb = Sandbox::new();
    sb.write_manifest(CODEX_SESSION_SYNC);
    init_main(&sb.repo);
    let missing = sb.home.join(".flint/missing.toml");
    let out = sb.install(&[
        "--stage",
        "full",
        "--config",
        missing.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(!sb.target(".codex/hooks.json").exists());

    sb.write_manifest(&CODEX_SESSION_SYNC.replace("event = \"SessionStart\"", "event = \"Stop\""));
    let out = sb.install_full(&[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("only supports SessionStart"));
    assert!(!sb.target(".codex/hooks.json").exists());
}

#[test]
fn codex_hook_cannot_share_target_with_file_or_block() {
    for conflicting in [
        r#"
[[artifact]]
stage = "full"
type = "file"
source = "skills/a/SKILL.md"
target = "$CODEX_HOME/hooks.json"
"#,
        r#"
[[artifact]]
stage = "full"
type = "block"
source = "skills/a/SKILL.md"
target = "$CODEX_HOME/hooks.json"
marker = "user"
"#,
    ] {
        let sb = Sandbox::new();
        sb.write_manifest(&format!("{CODEX_SESSION_SYNC}\n{conflicting}"));
        init_main(&sb.repo);
        let out = sb.install_full(&[]);
        assert!(!out.status.success(), "collision must fail: {}", stderr(&out));
        assert!(!sb.target(".codex/hooks.json").exists());
        assert!(!sb.lock_path().exists());
    }
}

#[test]
fn concurrent_quiet_installs_leave_valid_state() {
    let sb = Sandbox::new();
    sb.write_manifest(CODEX_SESSION_SYNC);
    let config = sb.config_path();
    let approved_head = init_main(&sb.repo);
    let mut children = Vec::new();
    for _ in 0..8 {
        children.push(
            sb.install_command(&[
                "--stage",
                "full",
                "--config",
                config.to_str().unwrap(),
                "--quiet",
                "--expected-repo-head",
                &approved_head,
            ])
            .spawn()
            .unwrap(),
        );
    }
    for child in children {
        assert!(child.wait_with_output().unwrap().status.success());
    }

    let hooks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sb.target(".codex/hooks.json")).unwrap())
            .unwrap();
    let flint_groups = hooks["hooks"]["SessionStart"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|group| group["hooks"][0]["statusMessage"] == "Flint: sync suite")
        .count();
    assert_eq!(flint_groups, 1);

    let lock: toml::Value = toml::from_str(&fs::read_to_string(sb.lock_path()).unwrap()).unwrap();
    let flint_entries = lock["installed"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["marker"].as_str() == Some("flint-session-sync"))
        .count();
    assert_eq!(flint_entries, 1);
}

// ---------------------------------------------------------------------------
// Full cycle: install → check green → drop entry → reinstall removes → restore
// ---------------------------------------------------------------------------

#[test]
fn full_cycle_install_check_remove_restore() {
    let sb = Sandbox::new();
    sb.write_manifest(TWO_SKILLS);

    // install
    let out = sb.install(&[]);
    assert!(out.status.success(), "install failed: {}", stderr(&out));
    assert_eq!(fs::read_to_string(sb.target(".claude/skills/a/SKILL.md")).unwrap(), "---\nname: a\n---\nalpha body\n");
    assert!(sb.target(".claude/skills/b/SKILL.md").exists());
    let lock = fs::read_to_string(sb.lock_path()).unwrap();
    let lock: toml::Value = toml::from_str(&lock).unwrap();
    let entries = lock.get("installed").and_then(toml::Value::as_array).unwrap();
    assert!(entries.iter().any(|entry| {
        let target = entry.get("target").and_then(toml::Value::as_str).unwrap();
        Path::new(target).ends_with(Path::new("skills/a/SKILL.md"))
            && entry.get("rendered_sha256").and_then(toml::Value::as_str).is_some()
    }));

    // check: clean, exit 0
    let out = sb.install(&["--check"]);
    assert!(out.status.success(), "check should be clean: {}", stdout(&out));

    // drop b from the manifest → reinstall removes it (lock-manifest diff) + prunes
    // the now-empty parent dir, but never the allowlist root.
    sb.write_manifest(ONE_SKILL);
    let out = sb.install(&[]);
    assert!(out.status.success(), "reinstall failed: {}", stderr(&out));
    assert!(!sb.target(".claude/skills/b/SKILL.md").exists(), "b must be removed");
    assert!(!sb.target(".claude/skills/b").exists(), "empty parent pruned");
    assert!(sb.target(".claude").exists(), "allowlist root never pruned");
    assert!(sb.target(".claude/skills/a/SKILL.md").exists(), "a survives");

    // restore manifest → b comes back
    sb.write_manifest(TWO_SKILLS);
    let out = sb.install(&[]);
    assert!(out.status.success());
    assert!(sb.target(".claude/skills/b/SKILL.md").exists());
}

#[test]
fn install_is_idempotent_three_runs_no_baks() {
    let sb = Sandbox::new();
    sb.write_manifest(TWO_SKILLS);
    for _ in 0..3 {
        let out = sb.install(&[]);
        assert!(out.status.success(), "{}", stderr(&out));
    }
    // adopt-in-place: unchanged content never rotates a .bak
    assert!(!sb.target(".claude/skills/a/SKILL.md.bak.1").exists());
    let out = sb.install(&[]);
    assert!(stdout(&out).contains("0 written"), "4th run writes nothing: {}", stdout(&out));
}

#[test]
fn deleted_artifact_is_restored_and_hand_edit_is_drift() {
    let sb = Sandbox::new();
    sb.write_manifest(TWO_SKILLS);
    assert!(sb.install(&[]).status.success());

    // delete one → --check reports MISSING (exit 1) → install restores
    fs::remove_file(sb.target(".claude/skills/a/SKILL.md")).unwrap();
    let out = sb.install(&["--check"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("MISSING"));
    assert!(sb.install(&[]).status.success());
    assert!(sb.target(".claude/skills/a/SKILL.md").exists());

    // hand-edit → DRIFT (exit 1); reinstall reconciles and leaves a .bak
    fs::write(sb.target(".claude/skills/a/SKILL.md"), "hand edited\n").unwrap();
    let out = sb.install(&["--check"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("DRIFT"));
    assert!(sb.install(&[]).status.success());
    assert_eq!(fs::read_to_string(sb.target(".claude/skills/a/SKILL.md")).unwrap(), "---\nname: a\n---\nalpha body\n");
    assert_eq!(fs::read_to_string(sb.target(".claude/skills/a/SKILL.md.bak.1")).unwrap(), "hand edited\n");
}

#[test]
fn check_ignores_trailing_whitespace_and_eol_only() {
    let sb = Sandbox::new();
    sb.write_manifest(ONE_SKILL);
    assert!(sb.install(&[]).status.success());
    // append trailing spaces + extra final newlines: NOT drift
    fs::write(sb.target(".claude/skills/a/SKILL.md"), "---\nname: a  \n---\nalpha body\n\n").unwrap();
    let out = sb.install(&["--check"]);
    assert!(out.status.success(), "ws/EOL-only difference must not count as drift: {}", stdout(&out));
}

#[test]
fn plan_is_a_dry_run() {
    let sb = Sandbox::new();
    sb.write_manifest(TWO_SKILLS);
    let out = sb.install(&["--plan"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("write"));
    assert!(!sb.target(".claude/skills/a/SKILL.md").exists(), "--plan must write nothing");
    assert!(!sb.lock_path().exists(), "--plan must not create a lock");
}

// ---------------------------------------------------------------------------
// F2 error table: four injections, whole batch untouched
// ---------------------------------------------------------------------------

#[test]
fn error_manifest_syntax_aborts_batch() {
    let sb = Sandbox::new();
    sb.write_manifest("[[artifact]\nnot toml");
    let out = sb.install(&[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("parse manifest"));
}

#[test]
fn error_manifest_schema_aborts_batch() {
    let sb = Sandbox::new();
    // generator carrying a source field = half-legal entry → parse-time refusal
    sb.write_manifest(
        r#"
[[artifact]]
stage = "skills"
type = "generator"
generator = "claude-advisory"
source = "skills/a/SKILL.md"
target = "$CLAUDE_HOME/rules/x.md"
"#,
    );
    let out = sb.install(&[]);
    assert!(!out.status.success());
}

#[test]
fn error_missing_source_aborts_whole_batch() {
    let sb = Sandbox::new();
    // first entry is valid, second's source is missing — the VALID one must not land
    sb.write_manifest(
        r#"
[[artifact]]
stage = "skills"
type = "file"
source = "skills/a/SKILL.md"
target = "$CLAUDE_HOME/skills/a/SKILL.md"

[[artifact]]
stage = "skills"
type = "file"
source = "skills/nope/SKILL.md"
target = "$CLAUDE_HOME/skills/nope/SKILL.md"
"#,
    );
    let out = sb.install(&[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("read source"));
    assert!(!sb.target(".claude/skills/a/SKILL.md").exists(), "batch must be all-or-nothing");
}

#[test]
fn error_target_outside_allowlist_aborts() {
    let sb = Sandbox::new();
    for bad in [
        "$CLAUDE_HOME/../evil.md",             // lexical escape
        "$GROK_HOME/../evil.md",               // …and the newest root is no exception
        "$INSTANCE/casefiles/x.md",            // $INSTANCE illegal in target
        "$WHAT_HOME/skills/x.md",              // unknown variable
        "skills/plain-relative.md",            // no allowlist prefix at all
    ] {
        sb.write_manifest(&format!(
            "[[artifact]]\nstage = \"skills\"\ntype = \"file\"\nsource = \"skills/a/SKILL.md\"\ntarget = \"{bad}\"\n"
        ));
        let out = sb.install(&[]);
        assert!(!out.status.success(), "target `{bad}` must be rejected");
    }
}

#[cfg(unix)]
#[test]
fn error_symlink_parent_escape_rejected() {
    let sb = Sandbox::new();
    // ~/.claude/skills is a symlink pointing OUTSIDE the allowlist → canonical
    // resolution lands outside → reject (the manifest path field must not become
    // an arbitrary-write primitive via a planted symlink).
    let outside = sb.home.join("outside");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, sb.home.join(".claude/skills")).unwrap();
    sb.write_manifest(ONE_SKILL);
    let out = sb.install(&[]);
    assert!(!out.status.success(), "symlink-parent escape must be rejected");
    assert!(stderr(&out).contains("allowlist"));
    assert!(!outside.join("a/SKILL.md").exists());
}

#[test]
fn error_duplicate_writer_rejected() {
    let sb = Sandbox::new();
    sb.write_manifest(
        r#"
[[artifact]]
stage = "skills"
type = "file"
source = "skills/a/SKILL.md"
target = "$CLAUDE_HOME/skills/a/SKILL.md"

[[artifact]]
stage = "skills"
type = "file"
source = "skills/b/SKILL.md"
target = "$CLAUDE_HOME/skills/a/SKILL.md"
"#,
    );
    let out = sb.install(&[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("two writers"));
}

// ---------------------------------------------------------------------------
// Lock conservatism (F1)
// ---------------------------------------------------------------------------

#[test]
fn missing_or_corrupt_lock_never_removes() {
    let sb = Sandbox::new();
    sb.write_manifest(TWO_SKILLS);
    assert!(sb.install(&[]).status.success());

    // lock DELETED + entry dropped from manifest → conservative: no removal
    fs::remove_file(sb.lock_path()).unwrap();
    sb.write_manifest(ONE_SKILL);
    let out = sb.install(&[]);
    assert!(out.status.success());
    assert!(
        sb.target(".claude/skills/b/SKILL.md").exists(),
        "no lock → never remove"
    );

    // lock CORRUPT → same, with a warning
    fs::write(sb.lock_path(), "not [ valid { toml").unwrap();
    let out = sb.install(&[]);
    assert!(out.status.success());
    assert!(stderr(&out).contains("corrupt"));
    assert!(sb.target(".claude/skills/b/SKILL.md").exists());

    // DOCUMENTED CONSEQUENCE of a lost lock: b is now an orphan. The rebuilt lock
    // records only what the manifest currently claims — without a trustworthy record
    // the installer cannot distinguish "we installed b" from "the user put b there",
    // so it must never remove it, even on later healthy runs.
    let out = sb.install(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        sb.target(".claude/skills/b/SKILL.md").exists(),
        "an orphaned artifact is never re-adopted for removal"
    );

    // re-claiming b in the manifest re-adopts it into the lock (adopt-in-place);
    // dropping it again THEN removes honestly.
    sb.write_manifest(TWO_SKILLS);
    assert!(sb.install(&[]).status.success());
    sb.write_manifest(ONE_SKILL);
    let out = sb.install(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !sb.target(".claude/skills/b/SKILL.md").exists(),
        "re-claimed then dropped → honest removal resumes"
    );
}

// ---------------------------------------------------------------------------
// --quiet: fail-open + clean/main precheck (T3)
// ---------------------------------------------------------------------------

#[test]
fn quiet_failure_is_fail_open() {
    let sb = Sandbox::new();
    sb.write_manifest("[[artifact]\nnot toml");
    let approved_head = init_main(&sb.repo);
    let out = sb.install(&["--quiet", "--expected-repo-head", &approved_head]);
    assert!(out.status.success(), "--quiet never blocks a session");
    assert!(stderr(&out).contains("skipped"));
}

fn git(repo: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?} failed");
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn init_main(repo: &Path) -> String {
    git(repo, &["init", "-b", "main"]);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-m", "init"]);
    git_stdout(repo, &["rev-parse", "--verify", "HEAD"])
}

#[test]
fn quiet_skips_dirty_or_offmain_repo_and_syncs_clean_main() {
    let sb = Sandbox::new();
    sb.write_manifest(ONE_SKILL);
    let approved_head = init_main(&sb.repo);

    // dirty tree → skip
    fs::write(sb.repo.join("dirty.txt"), "x").unwrap();
    let out = sb.install(&["--quiet", "--expected-repo-head", &approved_head]);
    assert!(out.status.success());
    assert!(stderr(&out).contains("dirty"));
    assert!(!sb.target(".claude/skills/a/SKILL.md").exists());
    fs::remove_file(sb.repo.join("dirty.txt")).unwrap();

    // off-main → skip
    git(&sb.repo, &["checkout", "-b", "feature"]);
    let out = sb.install(&["--quiet", "--expected-repo-head", &approved_head]);
    assert!(out.status.success());
    assert!(stderr(&out).contains("not main"));
    assert!(!sb.target(".claude/skills/a/SKILL.md").exists());

    // clean + main → syncs
    git(&sb.repo, &["checkout", "main"]);
    let out = sb.install(&["--quiet", "--expected-repo-head", &approved_head]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(sb.target(".claude/skills/a/SKILL.md").exists());
}

#[test]
fn quiet_requires_matching_approved_head() {
    let sb = Sandbox::new();
    sb.write_manifest(ONE_SKILL);
    let head = init_main(&sb.repo);

    let missing = sb.install(&["--quiet"]);
    assert!(missing.status.success());
    assert!(stderr(&missing).contains("expected-repo-head"));
    assert!(!sb.target(".claude/skills/a/SKILL.md").exists());

    let stale = "f".repeat(40);
    let mismatch = sb.install(&["--quiet", "--expected-repo-head", &stale]);
    assert!(mismatch.status.success());
    assert!(stderr(&mismatch).contains("HEAD"));
    assert!(!sb.target(".claude/skills/a/SKILL.md").exists());

    let matched = sb.install(&["--quiet", "--expected-repo-head", &head]);
    assert!(matched.status.success(), "{}", stderr(&matched));
    assert!(sb.target(".claude/skills/a/SKILL.md").exists());
}

// ---------------------------------------------------------------------------
// $INSTANCE: per-entry skip when the pointer is absent; resolves when present
// ---------------------------------------------------------------------------

#[test]
fn instance_source_skips_without_pointer_and_resolves_with_it() {
    let sb = Sandbox::new();
    sb.write_manifest(
        r#"
[[artifact]]
stage = "skills"
type = "file"
source = "skills/a/SKILL.md"
target = "$CLAUDE_HOME/skills/a/SKILL.md"

[[artifact]]
stage = "skills"
type = "file"
source = "$INSTANCE/briefs/orientation.md"
target = "$CLAUDE_HOME/rules/orientation.md"
"#,
    );
    // no instance.toml → the $INSTANCE entry skips, the other installs
    let out = sb.install(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("skipped"));
    assert!(sb.target(".claude/skills/a/SKILL.md").exists());
    assert!(!sb.target(".claude/rules/orientation.md").exists());

    // with a pointer → it resolves and installs
    let inst_root = sb.home.join("vaultlike");
    fs::create_dir_all(inst_root.join("briefs")).unwrap();
    fs::write(inst_root.join("briefs/orientation.md"), "private orientation\n").unwrap();
    fs::write(
        sb.home.join(".flint/instance.toml"),
        format!(
            "root = \"{}\"\nmachine = \"test\"\n",
            inst_root.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    let out = sb.install(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        fs::read_to_string(sb.target(".claude/rules/orientation.md")).unwrap(),
        "private orientation\n"
    );
}

// ---------------------------------------------------------------------------
// Generators: unsigned canon refuses stage-full
// ---------------------------------------------------------------------------

#[test]
fn unsigned_canon_refuses_generator_stage_full() {
    let sb = Sandbox::new();
    sb.write_manifest(
        r#"
[[artifact]]
stage = "full"
type = "generator"
generator = "claude-advisory"
target = "$CLAUDE_HOME/rules/flint-advisory.md"
"#,
    );
    // canon root exists but holds NO signed manifest
    let canon = sb.home.join(".flint/canon");
    fs::create_dir_all(canon.join("rules")).unwrap();
    let cfg = sb.home.join(".flint/flint.toml");
    fs::write(
        &cfg,
        format!(
            "flint_bin = \"flint\"\ncanon_root = \"{}\"\n[trust]\nallowed_signers = \"{}\"\nsigner_identity = \"s\"\nscope = \"i\"\n",
            canon.display(),
            sb.home.join(".flint/allowed_signers").display()
        ),
    )
    .unwrap();

    // stage skills: generator entry out of scope → fine, installs nothing
    let out = sb.install(&["--stage", "skills"]);
    assert!(out.status.success(), "{}", stderr(&out));

    // stage full without --config → loud refusal
    let out = sb.install(&["--stage", "full"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--config"));

    // stage full with config but unsigned canon → loud refusal, nothing written
    let out = sb.install(&["--stage", "full", "--config", cfg.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("canon"));
    assert!(!sb.target(".claude/rules/flint-advisory.md").exists());
}

// ---------------------------------------------------------------------------
// Grok wiring generator: renders from --config alone (no signed canon)
// ---------------------------------------------------------------------------

const GROK_HOOKS: &str = r#"
[[artifact]]
stage = "full"
type = "generator"
generator = "grok-hooks"
target = "$GROK_HOME/hooks/flint.json"
"#;

#[test]
fn grok_hooks_generator_writes_wiring_into_the_grok_home() {
    let sb = Sandbox::new();
    sb.write_manifest(GROK_HOOKS);
    // a canon root that holds NO signed manifest — this generator must not care.
    let canon = sb.home.join(".flint/canon");
    fs::create_dir_all(canon.join("rules")).unwrap();
    let cfg = sb.home.join(".flint/flint.toml");
    fs::write(
        &cfg,
        format!(
            "flint_bin = \"/usr/local/bin/flint\"\ncanon_root = \"{}\"\n[trust]\nallowed_signers = \"{}\"\nsigner_identity = \"s\"\nscope = \"i\"\n",
            canon.display(),
            sb.home.join(".flint/allowed_signers").display()
        ),
    )
    .unwrap();

    // without --config the wiring has no binary/config path to pin → loud refusal.
    let out = sb.install(&["--stage", "full"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--config"), "{}", stderr(&out));
    assert!(!sb.target(".grok/hooks/flint.json").exists());

    // with --config it installs — an UNSIGNED canon is fine here, because what this
    // renders is wiring, not canon content (the rules are read at hook time).
    let out = sb.install(&["--stage", "full", "--config", cfg.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let wiring: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sb.target(".grok/hooks/flint.json")).unwrap())
            .unwrap();
    let groups = wiring["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert!(
        groups[0].get("matcher").is_none(),
        "grok's matcher is a REGEX — `*` is invalid there, so all-tools is spelled by omission: {wiring}"
    );
    let handler = &groups[0]["hooks"][0];
    assert_eq!(handler["timeout"], 600);
    let cmd = handler["command"].as_str().unwrap();
    assert!(cmd.contains("--harness grok"), "{cmd}");
    assert!(cmd.contains("--mode block"), "installed grok wiring must enforce: {cmd}");
    assert!(cmd.contains("--fail-mode closed"), "installed grok wiring must fail closed: {cmd}");
    assert!(cmd.contains("flint.toml"), "the wiring pins the config the hook reads: {cmd}");

    // idempotent, and recorded as a whole-file artifact (no marker).
    let out = sb.install(&["--stage", "full", "--config", cfg.to_str().unwrap()]);
    assert!(stdout(&out).contains("0 written"), "{}", stdout(&out));
    let lock: toml::Value = toml::from_str(&fs::read_to_string(sb.lock_path()).unwrap()).unwrap();
    let entry = lock["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e.get("type").and_then(toml::Value::as_str) == Some("generator"))
        .expect("the grok wiring is locked");
    assert!(entry.get("marker").is_none(), "whole-file artifact owns no marker: {entry}");

    // dropping it from the manifest removes the file (and prunes the empty dir), never
    // the allowlist root itself.
    sb.write_manifest("");
    let out = sb.install(&["--stage", "full", "--config", cfg.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!sb.target(".grok/hooks/flint.json").exists());
    assert!(sb.target(".grok").exists(), "allowlist root never pruned");
}

// ---------------------------------------------------------------------------
// Block type: upsert / block-level diff / removal preserving user content
// ---------------------------------------------------------------------------

const BLOCK_MANIFEST: &str = r#"
[[artifact]]
stage = "skills"
type = "block"
source = "skills/a/SKILL.md"
target = "$CODEX_HOME/AGENTS.md"
marker = "vault-orientation"
"#;

#[test]
fn block_upserts_diffs_and_removes_preserving_user_content() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home.join(".codex")).unwrap();
    fs::write(
        sb.home.join(".codex/AGENTS.md"),
        "# my own header\n\nuser text stays.\n",
    )
    .unwrap();
    sb.write_manifest(BLOCK_MANIFEST);

    // upsert: user content intact, block appended
    assert!(sb.install(&[]).status.success());
    let agents = fs::read_to_string(sb.target(".codex/AGENTS.md")).unwrap();
    assert!(agents.starts_with("# my own header"));
    assert!(agents.contains("<!-- vault-orientation:begin -->"));
    assert!(agents.contains("alpha body"));
    assert!(agents.contains("user text stays."));

    // check clean; then edit INSIDE the block → drift at block level
    assert!(sb.install(&["--check"]).status.success());
    let tampered = agents.replace("alpha body", "tampered");
    fs::write(sb.target(".codex/AGENTS.md"), &tampered).unwrap();
    let out = sb.install(&["--check"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("DRIFT"));

    // drop the entry → block removed, user content survives
    sb.write_manifest(ONE_SKILL);
    assert!(sb.install(&[]).status.success());
    let agents = fs::read_to_string(sb.target(".codex/AGENTS.md")).unwrap();
    assert!(!agents.contains("vault-orientation:begin"));
    assert!(agents.contains("# my own header") && agents.contains("user text stays."));
}

// ---------------------------------------------------------------------------
// codex WP1b review regressions
// ---------------------------------------------------------------------------

/// P2-1: a hand-edited/stale lock must never become an arbitrary-delete primitive —
/// removal targets are re-validated against the allowlist.
#[test]
fn edited_lock_target_outside_allowlist_is_never_removed() {
    let sb = Sandbox::new();
    sb.write_manifest(TWO_SKILLS);
    assert!(sb.install(&[]).status.success());

    // victim file OUTSIDE the allowlist roots
    let victim = sb.home.join("victim.md");
    fs::write(&victim, "user data\n").unwrap();

    // point b's lock entry at the victim, then drop b from the manifest
    let lock = fs::read_to_string(sb.lock_path()).unwrap();
    let b_target = sb.home.canonicalize().unwrap().join(".claude/skills/b/SKILL.md");
    let tampered = lock.replace(&b_target.display().to_string(), &victim.display().to_string());
    assert_ne!(lock, tampered, "lock tamper must have applied");
    fs::write(sb.lock_path(), tampered).unwrap();
    sb.write_manifest(ONE_SKILL);

    let out = sb.install(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(victim.exists(), "outside-allowlist lock entry must never be deleted");
    assert!(stderr(&out).contains("outside the install allowlist"));
}

/// P2-2: two block entries sharing one document must chain on one evolving content.
#[test]
fn two_blocks_on_one_target_coexist_and_remove_independently() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home.join(".codex")).unwrap();
    fs::write(sb.home.join(".codex/AGENTS.md"), "# user header\n").unwrap();
    let two_blocks = r#"
[[artifact]]
stage = "skills"
type = "block"
source = "skills/a/SKILL.md"
target = "$CODEX_HOME/AGENTS.md"
marker = "alpha"

[[artifact]]
stage = "skills"
type = "block"
source = "skills/b/SKILL.md"
target = "$CODEX_HOME/AGENTS.md"
marker = "beta"
"#;
    sb.write_manifest(two_blocks);
    let out = sb.install(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let agents = fs::read_to_string(sb.target(".codex/AGENTS.md")).unwrap();
    assert!(agents.contains("<!-- alpha:begin -->"), "first block present");
    assert!(agents.contains("<!-- beta:begin -->"), "second block must NOT be clobbered");
    assert!(agents.contains("# user header"));
    assert!(sb.install(&["--check"]).status.success());

    // drop beta → alpha + user content survive
    let one_block = r#"
[[artifact]]
stage = "skills"
type = "block"
source = "skills/a/SKILL.md"
target = "$CODEX_HOME/AGENTS.md"
marker = "alpha"
"#;
    sb.write_manifest(one_block);
    assert!(sb.install(&[]).status.success());
    let agents = fs::read_to_string(sb.target(".codex/AGENTS.md")).unwrap();
    assert!(agents.contains("<!-- alpha:begin -->"));
    assert!(!agents.contains("beta:begin"));
    assert!(agents.contains("# user header"));
}

/// P2-3: sources are confined to their root — `..` and absolute paths refuse.
#[test]
fn source_escaping_repo_is_rejected() {
    let sb = Sandbox::new();
    fs::write(sb.repo.parent().unwrap().join("outside.md"), "leak me\n").unwrap();
    for bad in ["../outside.md", "/etc/hosts", "skills/../../outside.md"] {
        sb.write_manifest(&format!(
            "[[artifact]]\nstage = \"skills\"\ntype = \"file\"\nsource = \"{bad}\"\ntarget = \"$CLAUDE_HOME/skills/x/SKILL.md\"\n"
        ));
        let out = sb.install(&[]);
        assert!(!out.status.success(), "source `{bad}` must be rejected");
        assert!(!sb.target(".claude/skills/x/SKILL.md").exists());
    }
}

/// P2-4: --quiet collapses EVERY outcome to exit 0 (a drifted --check included).
#[test]
fn quiet_collapses_check_drift_to_zero() {
    let sb = Sandbox::new();
    sb.write_manifest(ONE_SKILL);
    let approved_head = init_main(&sb.repo);
    assert!(sb.install(&[]).status.success());
    fs::write(sb.target(".claude/skills/a/SKILL.md"), "hand edited\n").unwrap();
    let out = sb.install(&[
        "--quiet",
        "--expected-repo-head",
        &approved_head,
        "--check",
    ]);
    assert!(out.status.success(), "--quiet must never exit nonzero: {}", stderr(&out));
}

/// A file↔block transition colliding on one target in a single run is ambiguous —
/// refuse loudly rather than let ordering decide.
#[test]
fn whole_file_and_block_collision_refused() {
    let sb = Sandbox::new();
    sb.write_manifest(
        r#"
[[artifact]]
stage = "skills"
type = "file"
source = "skills/a/SKILL.md"
target = "$CODEX_HOME/AGENTS.md"

[[artifact]]
stage = "skills"
type = "block"
source = "skills/b/SKILL.md"
target = "$CODEX_HOME/AGENTS.md"
marker = "beta"
"#,
    );
    let out = sb.install(&[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("collide"));
    assert!(!sb.target(".codex/AGENTS.md").exists(), "nothing written on refusal");
}

#[test]
fn corrupted_double_marker_refuses_before_any_write() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home.join(".codex")).unwrap();
    fs::write(
        sb.home.join(".codex/AGENTS.md"),
        "<!-- vault-orientation:begin -->\na\n<!-- vault-orientation:end -->\n<!-- vault-orientation:begin -->\nb\n<!-- vault-orientation:end -->\n",
    )
    .unwrap();
    sb.write_manifest(BLOCK_MANIFEST);
    let out = sb.install(&[]);
    assert!(!out.status.success(), "duplicated marker must refuse");
    assert!(stderr(&out).contains("refusing"));
}
