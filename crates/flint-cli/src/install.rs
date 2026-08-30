//! `flint install` — the suite installer (plan v4.3 WP1b, §2 semantics).
//!
//! Striker's "carry it over" action extended to the whole suite: a manifest
//! (`scripts/manifest.toml`) declares which artifacts (workflow skills, generated
//! advisory, marked blocks) land where; this module makes disk match it,
//! idempotently, with a lock file for honest removal and drift detection.
//!
//! Boundaries (PIP-0001 / R9):
//! - The judge/hook path NEVER runs through here; this module may legally read
//!   `instance.toml` (the memory-instance pointer) — the hook must not.
//! - Generators call Striker render functions IN-PROCESS. No shell, ever: the
//!   SessionStart sync path (--quiet) must have zero command-execution surface
//!   (the only subprocess in this file is the fixed-argv `git` state probe for
//!   the T3 clean+main+approved-HEAD precheck, which renders nothing).
//! - Targets are confined to an allowlist of canonical roots (`~/.claude`,
//!   `~/.codex`, `~/.grok`, `~/.flint`) — resolved, not string-compared (the
//!   manifest path field is otherwise an arbitrary-write primitive).
//! - Concurrency on `installed.lock` is a read-modify-write, NOT a locked
//!   critical section (spec §7 staged defer — WP-E): the full rationale +
//!   escalation trigger live in the CONCURRENCY note inside `run_install_inner`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use flint_core::config::FlintConfig;
use flint_core::striker;

use crate::hook;

// ---------------------------------------------------------------------------
// Manifest schema (serde tagged enum — half-legal entries fail at parse time)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Skills,
    Full,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Stage::Skills => "skills",
            Stage::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum GeneratorKind {
    #[serde(rename = "claude-advisory")]
    ClaudeAdvisory,
    #[serde(rename = "codex-agents-block")]
    CodexAgentsBlock,
    /// Grok hook WIRING (not canon content): the whole-file `~/.grok/hooks/flint.json`
    /// that points Grok's PreToolUse at `flint hook --harness grok`. Renders from the
    /// config path + flint binary alone, so it needs `--config` but no signed canon.
    #[serde(rename = "grok-hooks")]
    GrokHooks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum CodexHookKind {
    #[serde(rename = "flint-session-sync")]
    FlintSessionSync,
}

const CODEX_HOOK_TYPE: &str = "codex-hook";

/// One manifest entry. `type` is the serde tag; each variant strictly declares its
/// required fields and (via `deny_unknown_fields`) forbids the others — e.g. a
/// `type = "generator"` entry carrying a `source` field is a parse error, not a
/// half-legal entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Artifact {
    File {
        stage: Stage,
        source: String,
        target: String,
    },
    Generator {
        stage: Stage,
        generator: GeneratorKind,
        target: String,
    },
    Block {
        stage: Stage,
        source: String,
        target: String,
        marker: String,
    },
    #[serde(rename = "codex-hook")]
    CodexHook {
        stage: Stage,
        hook: CodexHookKind,
        target: String,
        event: String,
        matcher: String,
        timeout: u64,
    },
}

impl Artifact {
    fn stage(&self) -> Stage {
        match self {
            Artifact::File { stage, .. }
            | Artifact::Generator { stage, .. }
            | Artifact::Block { stage, .. }
            | Artifact::CodexHook { stage, .. } => *stage,
        }
    }
    fn target(&self) -> &str {
        match self {
            Artifact::File { target, .. }
            | Artifact::Generator { target, .. }
            | Artifact::Block { target, .. }
            | Artifact::CodexHook { target, .. } => target,
        }
    }
    fn type_str(&self) -> &'static str {
        match self {
            Artifact::File { .. } => "file",
            Artifact::Generator { .. } => "generator",
            Artifact::Block { .. } => "block",
            Artifact::CodexHook { .. } => CODEX_HOOK_TYPE,
        }
    }
    /// The block marker this entry owns on its target (generators that render into a
    /// shared doc own the flint block; plain files own no marker).
    fn marker(&self) -> Option<&str> {
        match self {
            Artifact::Block { marker, .. } => Some(marker),
            Artifact::Generator { generator: GeneratorKind::CodexAgentsBlock, .. } => Some("flint"),
            Artifact::CodexHook { hook: CodexHookKind::FlintSessionSync, .. } => {
                Some(crate::codex_hook::SESSION_SYNC_ID)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    #[serde(default, rename = "artifact")]
    artifacts: Vec<Artifact>,
}

// ---------------------------------------------------------------------------
// Lock file (installed.lock, TOML) — the honest record of what WE put on disk.
// Corrupt/missing lock → conservative: install + rebuild, never remove (F1).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
struct LockEntry {
    target: String,
    #[serde(rename = "type")]
    type_str: String,
    stage: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    marker: String,
    rendered_sha256: String,
    source_sha256: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct LockFile {
    #[serde(default)]
    version: u32,
    #[serde(default, rename = "installed")]
    entries: Vec<LockEntry>,
}

// ---------------------------------------------------------------------------
// Instance pointer (~/.flint/instance.toml). Tolerant of unknown keys (WP0.3:
// forward-compatible config evolution) — but NEVER read by judge/hook code.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct InstanceToml {
    root: PathBuf,
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// What an in-scope entry wants the world to look like.
#[derive(Debug, Clone, PartialEq)]
enum Desired {
    /// Whole-file content. Empty content means "this artifact should be ABSENT"
    /// (e.g. the advisory generator with zero advisory rules — parity with
    /// `compile --target-dir`'s stale-file removal).
    File(String),
    /// A marked block inside a shared document (user content around it preserved).
    /// Empty content removes the block.
    Block { marker: String, content: String },
    /// One structurally-owned Codex hook group inside a shared hooks.json file.
    CodexHook { hook: crate::codex_hook::ManagedHook, rendered: String },
}

#[derive(Debug)]
struct Resolved {
    spec: Artifact,
    /// Absolute, allowlist-validated target path.
    target: PathBuf,
    desired: Desired,
    /// sha256 of the source bytes (file/block) or rendered content (generator/hook).
    source_sha256: String,
}

struct Ctx {
    home: PathBuf,
    /// Canonical-resolved allowlist roots (claude, codex, flint homes).
    allow_roots: Vec<PathBuf>,
    flint_home: PathBuf,
    /// Repo root = parent of the directory holding the manifest.
    repo_root: PathBuf,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn home_dir() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        if !h.trim().is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        if !h.trim().is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    bail!("cannot resolve home directory (HOME / USERPROFILE unset)")
}

/// Canonicalize a path that may not fully exist yet: canonicalize the NEAREST
/// EXISTING ancestor (resolving symlinks), then re-append the remaining components
/// lexically. Rejects `..` in the non-existing remainder (nothing to resolve it
/// against). This is the §2 allowlist primitive.
fn canonicalize_nearest(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("target must resolve to an absolute path: {}", path.display());
    }
    let mut existing = path.to_path_buf();
    let mut remainder: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if existing.exists() {
            break;
        }
        match existing.file_name() {
            Some(name) => {
                remainder.push(name.to_os_string());
                existing = existing
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| anyhow::anyhow!("no existing ancestor for {}", path.display()))?;
            }
            None => break,
        }
    }
    let mut out = existing
        .canonicalize()
        .with_context(|| format!("canonicalize {}", existing.display()))?;
    for comp in remainder.iter().rev() {
        out.push(comp);
    }
    Ok(out)
}

impl Ctx {
    fn new(manifest_path: &Path) -> Result<Self> {
        let home = home_dir()?;
        let raw_roots = [
            home.join(".claude"),
            home.join(".codex"),
            home.join(".grok"),
            home.join(".flint"),
        ];
        let mut allow_roots = Vec::new();
        for r in &raw_roots {
            allow_roots.push(canonicalize_nearest(r)?);
        }
        let manifest_abs = manifest_path
            .canonicalize()
            .with_context(|| format!("manifest not found: {}", manifest_path.display()))?;
        let repo_root = manifest_abs
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| anyhow::anyhow!("manifest has no repo root: {}", manifest_abs.display()))?
            .to_path_buf();
        Ok(Ctx {
            flint_home: home.join(".flint"),
            home,
            allow_roots,
            repo_root,
        })
    }

    /// Expand `$CLAUDE_HOME|$CODEX_HOME|$GROK_HOME|$FLINT_HOME` (prefix only), then
    /// validate the result against the canonical allowlist. `$INSTANCE` or any unknown
    /// `$VAR` in a target is a manifest error.
    fn resolve_target(&self, raw: &str) -> Result<PathBuf> {
        let expanded = if let Some(rest) = raw.strip_prefix("$CLAUDE_HOME/") {
            self.home.join(".claude").join(rest)
        } else if let Some(rest) = raw.strip_prefix("$CODEX_HOME/") {
            self.home.join(".codex").join(rest)
        } else if let Some(rest) = raw.strip_prefix("$GROK_HOME/") {
            self.home.join(".grok").join(rest)
        } else if let Some(rest) = raw.strip_prefix("$FLINT_HOME/") {
            self.home.join(".flint").join(rest)
        } else if raw.starts_with("$INSTANCE") {
            bail!("$INSTANCE is not allowed in a target (targets live in the harness/flint homes): {raw}");
        } else {
            bail!(
                "target must start with $CLAUDE_HOME/, $CODEX_HOME/, $GROK_HOME/ or $FLINT_HOME/: {raw}"
            );
        };
        if expanded.components().any(|c| matches!(c, Component::ParentDir)) {
            bail!("target contains `..`: {raw}");
        }
        // Reject a `$` surviving anywhere (typo'd/unknown variable mid-path).
        if expanded.to_string_lossy().contains('$') {
            bail!("unresolved variable in target: {raw}");
        }
        let resolved = canonicalize_nearest(&expanded)?;
        if !self.allow_roots.iter().any(|r| resolved.starts_with(r)) {
            bail!(
                "target escapes the install allowlist (~/.claude, ~/.codex, ~/.grok, ~/.flint): {raw} -> {}",
                resolved.display()
            );
        }
        Ok(resolved)
    }

    /// Resolve a source path: repo-root-relative, or `$INSTANCE/`-prefixed (the private
    /// memory instance). Returns Ok(None) when the entry must be SKIPPED because the
    /// instance pointer is absent (per-entry skip + warning, not a batch error).
    ///
    /// Sources are CONFINED to their base (repo root / instance root): absolute paths
    /// and `..` are rejected, so a crafted manifest cannot read arbitrary local files
    /// into an allowed target (codex WP1b P2-3). Symlinks INSIDE the base are the
    /// owner's own repo content — not this check's problem.
    fn resolve_source(&self, raw: &str) -> Result<Option<PathBuf>> {
        let confine = |rel: &str| -> Result<()> {
            let p = Path::new(rel);
            if p.is_absolute() {
                bail!("source must be relative to its root, got absolute path: {rel}");
            }
            if p.components().any(|c| matches!(c, Component::ParentDir)) {
                bail!("source contains `..` (must stay under its root): {rel}");
            }
            Ok(())
        };
        if let Some(rest) = raw.strip_prefix("$INSTANCE/") {
            confine(rest)?;
            let inst_path = self.flint_home.join("instance.toml");
            let text = match fs::read_to_string(&inst_path) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => bail!("read {}: {e}", inst_path.display()),
            };
            let inst: InstanceToml = toml::from_str(&text)
                .with_context(|| format!("parse {}", inst_path.display()))?;
            return Ok(Some(inst.root.join(rest)));
        }
        if raw.contains('$') {
            bail!("unknown variable in source (only $INSTANCE/ is allowed): {raw}");
        }
        confine(raw)?;
        Ok(Some(self.repo_root.join(raw)))
    }
}

// ---------------------------------------------------------------------------
// Normalization (--check compares behaviorally-equal text, not bytes)
// ---------------------------------------------------------------------------

/// Trailing-whitespace + EOL normalization. HONEST SCOPE: textual equality here does
/// NOT prove behavioral equivalence of what an agent does with the text — that is the
/// deletion-window probe's job (WP2.3), not --check's.
fn normalize(s: &str) -> String {
    let mut out: String = s
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Marked blocks (generic marker; same shape as Striker's flint block)
// ---------------------------------------------------------------------------

fn block_begin(marker: &str) -> String {
    format!("<!-- {marker}:begin -->")
}
fn block_end(marker: &str) -> String {
    format!("<!-- {marker}:end -->")
}

/// Extract the marked block's inner content, if present. Errors on duplicated or
/// malformed (end-before-begin, missing end) markers — a corrupted target must fail
/// loudly, never be "repaired" by guessing (§2: marker 块重复 → 报错不写).
fn extract_block(existing: &str, marker: &str) -> Result<Option<String>> {
    let begin = block_begin(marker);
    let end = block_end(marker);
    let begins = existing.matches(&begin).count();
    let ends = existing.matches(&end).count();
    if begins == 0 && ends == 0 {
        return Ok(None);
    }
    if begins != 1 || ends != 1 {
        bail!("marker `{marker}` appears {begins} begin / {ends} end times — refusing to touch a corrupted block");
    }
    let s = existing.find(&begin).unwrap() + begin.len();
    let e = existing.find(&end).unwrap();
    if e < s {
        bail!("marker `{marker}` end precedes begin — refusing to touch a corrupted block");
    }
    Ok(Some(existing[s..e].trim_matches('\n').to_string()))
}

/// Upsert (or, with empty content, delete) the marked block, preserving everything else.
fn upsert_block(existing: &str, marker: &str, content: &str) -> Result<String> {
    let begin = block_begin(marker);
    let end = block_end(marker);
    let wrapped = if content.trim().is_empty() {
        String::new()
    } else {
        format!("{begin}\n{}\n{end}\n", content.trim_end())
    };
    match extract_block(existing, marker)? {
        Some(_) => {
            let s = existing.find(&begin).unwrap();
            let e = existing.find(&end).unwrap() + end.len();
            let after = existing[e..].strip_prefix('\n').unwrap_or(&existing[e..]);
            Ok(format!("{}{}{}", &existing[..s], wrapped, after))
        }
        None if wrapped.is_empty() => Ok(existing.to_string()),
        None => {
            let mut out = existing.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&wrapped);
            Ok(out)
        }
    }
}

// ---------------------------------------------------------------------------
// The install pipeline
// ---------------------------------------------------------------------------

pub struct InstallArgs {
    pub manifest: PathBuf,
    pub stage: Stage,
    pub config: Option<PathBuf>,
    pub check: bool,
    pub plan: bool,
    pub quiet: bool,
    pub expected_repo_head: Option<String>,
}

pub fn run_install(args: &InstallArgs) -> Result<i32> {
    if !args.quiet && args.expected_repo_head.is_some() {
        bail!("--expected-repo-head is only valid with --quiet");
    }
    if args.quiet {
        // The SessionStart sync path: NEVER block a session on installer trouble —
        // the installer is a courier, not a gate. EVERY outcome collapses to exit 0
        // (a stray --check returning drift=1 must not fail a session either —
        // codex WP1b P2-4); trouble is a one-line stderr warning.
        return Ok(match run_quiet(args) {
            Ok(0) => 0,
            Ok(code) => {
                eprintln!("flint install --quiet: non-clean result ({code}) — not blocking the session");
                0
            }
            Err(e) => {
                eprintln!("flint install --quiet: skipped ({e})");
                0
            }
        });
    }
    run_install_inner(args)
}

/// T3 precheck + delegated run for the --quiet auto-sync path.
fn run_quiet(args: &InstallArgs) -> Result<i32> {
    let manifest_abs = args
        .manifest
        .canonicalize()
        .with_context(|| format!("manifest not found: {}", args.manifest.display()))?;
    let repo_root = manifest_abs
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("manifest has no repo root"))?;
    let expected_repo_head = args
        .expected_repo_head
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--expected-repo-head is required with --quiet"))?;
    crate::codex_hook::validate_repo_head(expected_repo_head)?;
    // Fixed-argv git state probes (clean + main). Any probe failure → skip, warn.
    let dirty = git_stdout(repo_root, &["status", "--porcelain"], "status")?;
    if !dirty.is_empty() {
        eprintln!(
            "flint install --quiet: skipped — flint repo {} is dirty (sync only from a clean tree)",
            repo_root.display()
        );
        return Ok(0);
    }
    let branch = git_stdout(repo_root, &["branch", "--show-current"], "branch")?;
    if branch != "main" {
        eprintln!("flint install --quiet: skipped — flint repo on `{branch}`, not main");
        return Ok(0);
    }
    let actual_repo_head = repo_head(repo_root)?;
    if actual_repo_head != expected_repo_head {
        eprintln!(
            "flint install --quiet: skipped — flint repo HEAD does not match the explicitly approved commit"
        );
        return Ok(0);
    }
    run_install_inner(args)
}

fn git_stdout(repo_root: &Path, args: &[&str], label: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .with_context(|| format!("run git {label}"))?;
    if !output.status.success() {
        bail!("git {label} failed in {}", repo_root.display());
    }
    Ok(String::from_utf8(output.stdout)
        .with_context(|| format!("git {label} emitted non-UTF-8 output"))?
        .trim()
        .to_owned())
}

fn repo_head(repo_root: &Path) -> Result<String> {
    let head = git_stdout(repo_root, &["rev-parse", "--verify", "HEAD"], "rev-parse HEAD")?;
    crate::codex_hook::validate_repo_head(&head)?;
    Ok(head)
}

fn run_install_inner(args: &InstallArgs) -> Result<i32> {
    let ctx = Ctx::new(&args.manifest)?;

    // ---- parse + validate (F2: any schema/validation error aborts the whole batch
    // before a single write) ----
    let text = fs::read_to_string(&args.manifest)
        .with_context(|| format!("read manifest {}", args.manifest.display()))?;
    let manifest: ManifestFile = toml::from_str(&text)
        .with_context(|| format!("parse manifest {}", args.manifest.display()))?;

    // duplicate (target, marker) pairs = two writers for one artifact — reject.
    let mut seen = BTreeSet::new();
    for a in &manifest.artifacts {
        let key = (a.target().to_string(), a.marker().unwrap_or("").to_string());
        if !seen.insert(key.clone()) {
            bail!("manifest declares two writers for target `{}` (marker `{}`)", key.0, key.1);
        }
    }

    // ---- scope by stage ----
    let in_scope: Vec<&Artifact> = manifest
        .artifacts
        .iter()
        .filter(|a| a.stage() == Stage::Skills || args.stage == Stage::Full)
        .collect();

    // ---- resolve every entry up front (no partial batches) ----
    // The ADVISORY generators render from the SIGNED canon: loading it is required the
    // moment one is in scope; an unsigned/invalid canon refuses the batch (Eng-2). The
    // grok-hooks generator renders WIRING, not canon content — it needs `--config` (see
    // its arm below) but must not drag a canon-signature requirement onto a machine that
    // is only being pointed at the judge.
    let needs_policy = in_scope.iter().any(|a| {
        matches!(
            a,
            Artifact::Generator {
                generator: GeneratorKind::ClaudeAdvisory | GeneratorKind::CodexAgentsBlock,
                ..
            }
        )
    });
    // Alongside the signed policy, carry flint's opt-out capture toggle: the advisory generators
    // append the L1 capture default when it is on (§3). Off (or no config) → nothing appended.
    let (policy, auto_mine) = if needs_policy {
        let cfg_path = args.config.as_ref().ok_or_else(|| {
            anyhow::anyhow!("manifest has generator entries in scope — pass --config (they render from the signed canon)")
        })?;
        let cfg = FlintConfig::load(cfg_path)?;
        let policy = hook::load_policy(&cfg).map_err(|e| anyhow::anyhow!("signed canon unavailable: {e}"))?;
        (Some(policy), cfg.capture.auto_mine)
    } else {
        (None, false)
    };

    let mut resolved: Vec<Resolved> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for a in &in_scope {
        let target = ctx.resolve_target(a.target())?;
        match a {
            Artifact::File { source, .. } => {
                let Some(src) = ctx.resolve_source(source)? else {
                    skipped.push(format!("{} (source {source}: no instance.toml)", a.target()));
                    continue;
                };
                let bytes = fs::read(&src).with_context(|| format!("read source {}", src.display()))?;
                let content = String::from_utf8(bytes.clone())
                    .with_context(|| format!("source {} is not UTF-8", src.display()))?;
                resolved.push(Resolved {
                    spec: (*a).clone(),
                    target,
                    desired: Desired::File(content),
                    source_sha256: sha256_hex(&bytes),
                });
            }
            Artifact::Block { source, marker, .. } => {
                let Some(src) = ctx.resolve_source(source)? else {
                    skipped.push(format!("{} (source {source}: no instance.toml)", a.target()));
                    continue;
                };
                let bytes = fs::read(&src).with_context(|| format!("read source {}", src.display()))?;
                let content = String::from_utf8(bytes.clone())
                    .with_context(|| format!("source {} is not UTF-8", src.display()))?;
                resolved.push(Resolved {
                    spec: (*a).clone(),
                    target,
                    desired: Desired::Block { marker: marker.clone(), content },
                    source_sha256: sha256_hex(&bytes),
                });
            }
            Artifact::CodexHook { hook, event, matcher, timeout, .. } => {
                let config_path = args
                    .config
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("manifest has codex-hook entries in scope — pass --config"))?;
                let config_path = fs::canonicalize(config_path)
                    .with_context(|| format!("canonicalize config {}", config_path.display()))?;
                let manifest_path = fs::canonicalize(&args.manifest)
                    .with_context(|| format!("canonicalize manifest {}", args.manifest.display()))?;
                let exe = std::env::current_exe().context("resolve current flint executable")?;
                let approved_repo_head = match args.expected_repo_head.as_deref() {
                    Some(head) => {
                        crate::codex_hook::validate_repo_head(head)?;
                        head.to_owned()
                    }
                    None => repo_head(&ctx.repo_root)?,
                };
                let managed = match hook {
                    CodexHookKind::FlintSessionSync => crate::codex_hook::session_sync(
                        &exe,
                        &config_path,
                        &manifest_path,
                        &approved_repo_head,
                        event,
                        matcher,
                        *timeout,
                    )?,
                };
                let rendered = crate::codex_hook::rendered_fragment(&managed);
                resolved.push(Resolved {
                    spec: (*a).clone(),
                    target,
                    desired: Desired::CodexHook {
                        hook: managed,
                        rendered: rendered.clone(),
                    },
                    source_sha256: sha256_hex(rendered.as_bytes()),
                });
            }
            Artifact::Generator { generator, .. } => {
                let (desired, rendered) = match generator {
                    GeneratorKind::ClaudeAdvisory => {
                        let policy = policy.as_ref().expect("policy loaded when advisory generators in scope");
                        let body = crate::capture::append_advisory(&striker::claude_advisory_rules(policy), auto_mine);
                        (Desired::File(body.clone()), body)
                    }
                    GeneratorKind::CodexAgentsBlock => {
                        let policy = policy.as_ref().expect("policy loaded when advisory generators in scope");
                        let body = crate::capture::append_advisory(&striker::codex_agents_md(policy), auto_mine);
                        (
                            Desired::Block { marker: "flint".into(), content: body.clone() },
                            body,
                        )
                    }
                    GeneratorKind::GrokHooks => {
                        // WIRING, not canon content: the emitted command pins the flint
                        // binary and the config path the hook will read at run time, so
                        // --config is required here even though no policy is loaded.
                        let config_path = args.config.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("manifest has a grok-hooks entry in scope — pass --config (the emitted wiring pins the flint binary and the config path the hook reads)")
                        })?;
                        let config_abs = fs::canonicalize(config_path)
                            .with_context(|| format!("canonicalize config {}", config_path.display()))?;
                        let cfg = FlintConfig::load(config_path)?;
                        let wiring = striker::grok_hooks(&striker::CompileParams {
                            flint_bin: cfg.flint_bin.clone(),
                            config_path: config_abs.display().to_string(),
                            mode: "block".into(),
                        });
                        let mut body = serde_json::to_string_pretty(&wiring)
                            .context("render grok hook wiring")?;
                        body.push('\n');
                        (Desired::File(body.clone()), body)
                    }
                };
                resolved.push(Resolved {
                    spec: (*a).clone(),
                    target,
                    desired,
                    source_sha256: sha256_hex(rendered.as_bytes()),
                });
            }
        }
    }

    // ---- CONCURRENCY (spec §7 — reconcile-adopted defer, WP-E) ----
    // installed.lock is read here, folded, and rewritten near the end of this fn:
    // a read-modify-write, NOT a locked critical section. That is a DELIBERATE
    // staged boundary, weighed against the landed code, not an oversight:
    //   1. Cross-artifact clobber WITHIN one run is already closed: every target's
    //      actions fold onto one evolving `current` (per_target below), so the last
    //      writer to a shared document never overwrites another entry's block.
    //   2. Cross-PROCESS races (a manual `flint install` racing the Codex --quiet
    //      SessionStart sync) are a BENIGN lost-update — NOT because both always write
    //      identical bytes (a manual install may run at a different --stage, on a dirty
    //      tree, or at a different HEAD; only the --quiet side is pinned to
    //      clean+main+approved-HEAD). It is benign because each process computes its
    //      removal set from its OWN start-of-run lock snapshot + the current manifest,
    //      so a lost lock write only ever makes the persisted lock SHRINK — it never
    //      fabricates a removal — and the next full install self-heals it.
    //   3. The fail-closed "torn canon → spurious deny" worry is NOT on this path:
    //      the ENFORCED canon (CANON.manifest + .sig) is written ONLY by `canon pick`
    //      (atomic rename, main.rs), never by install. install writes advisory / skills
    //      / codex bindings + the lock — never the signed canon.
    // ESCALATION TRIGGER: one observed corruption / lost-update, OR a new
    // heterogeneous writer (a background installer, a hand-writer, a different
    // manifest) → then add an inter-process file lock + same-dir atomic replace
    // here. Until an observation earns it, this stays a read-modify-write.
    //
    // ---- lock: load conservatively (F1) ----
    let lock_path = canonicalize_nearest(&ctx.flint_home)?.join("installed.lock");
    let (mut lock, lock_trustworthy) = match fs::read_to_string(&lock_path) {
        Ok(t) => match toml::from_str::<LockFile>(&t) {
            Ok(l) => (l, true),
            Err(e) => {
                eprintln!(
                    "warning: {} is corrupt ({e}) — installing/rebuilding, REMOVING NOTHING this run",
                    lock_path.display()
                );
                (LockFile::default(), false)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (LockFile::default(), false),
        Err(e) => bail!("read {}: {e}", lock_path.display()),
    };

    // ---- removal set: lock entries in the requested scope that no in-scope manifest
    // entry claims any more (matched on target+marker). Skipped-for-instance entries
    // still COUNT as claimed — a missing pointer must not cascade into removals. ----
    let claimed: BTreeSet<(String, String)> = in_scope
        .iter()
        .map(|a| {
            (
                ctx.resolve_target(a.target()).expect("validated above").display().to_string(),
                a.marker().unwrap_or("").to_string(),
            )
        })
        .collect();
    let removals: Vec<LockEntry> = if lock_trustworthy {
        lock.entries
            .iter()
            .filter(|e| {
                let entry_stage_in_scope = e.stage == Stage::Skills.as_str() || args.stage == Stage::Full;
                entry_stage_in_scope && !claimed.contains(&(e.target.clone(), e.marker.clone()))
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    // ---- group all work per TARGET, then fold: two entries sharing one document
    // (e.g. AGENTS.md holding the generated flint block AND a manual block) must
    // chain on ONE evolving content — computing each from the original current would
    // let the last write clobber the others (codex WP1b P2-2). Corrupted marked
    // blocks abort BEFORE any write. ----
    enum Action {
        /// Whole-file content (file entry / claude-advisory generator). Empty = absent.
        Set { content: String, label: String },
        /// Marked-block upsert. Empty content removes the block.
        Upsert { marker: String, content: String, label: String },
        /// Lock-driven whole-file removal, carrying the lock's sha: when the disk
        /// still matches what WE wrote, removal is clean (no .bak — content is
        /// reproducible from the manifest source); a mismatch means a hand-edit
        /// would be lost, so a forensic .bak is taken first.
        RemoveWhole { lock_sha: String, label: String },
        /// Lock-driven block removal (goes through the write path, which baks).
        RemoveBlock { marker: String, label: String },
        /// Structurally upsert one owned group in a shared Codex hooks document.
        UpsertCodexHook { hook: crate::codex_hook::ManagedHook, label: String },
        /// Structurally remove one owned group without deleting the shared document.
        RemoveCodexHook { identity: String, label: String },
    }
    impl Action {
        fn label(&self) -> &str {
            match self {
                Action::Set { label, .. }
                | Action::Upsert { label, .. }
                | Action::RemoveWhole { label, .. }
                | Action::RemoveBlock { label, .. }
                | Action::UpsertCodexHook { label, .. }
                | Action::RemoveCodexHook { label, .. } => label,
            }
        }
    }
    let mut per_target: std::collections::BTreeMap<PathBuf, Vec<Action>> = Default::default();
    for r in &resolved {
        let label = format!("{} [{}]", r.target.display(), r.spec.type_str());
        let action = match &r.desired {
            Desired::File(content) => Action::Set { content: content.clone(), label },
            Desired::Block { marker, content } => {
                Action::Upsert { marker: marker.clone(), content: content.clone(), label }
            }
            Desired::CodexHook { hook, .. } => {
                Action::UpsertCodexHook { hook: hook.clone(), label }
            }
        };
        per_target.entry(r.target.clone()).or_default().push(action);
    }
    for e in &removals {
        // Re-run the allowlist gate on LOCK data before honoring a removal: the lock
        // is plain local TOML — a stale or hand-edited entry must not become an
        // arbitrary-delete primitive (codex WP1b P2-1).
        let raw = PathBuf::from(&e.target);
        let inside = canonicalize_nearest(&raw)
            .map(|t| ctx.allow_roots.iter().any(|r| t.starts_with(r)))
            .unwrap_or(false);
        if !inside {
            eprintln!(
                "warning: lock entry {} is outside the install allowlist — NOT removing (edited/stale lock?)",
                e.target
            );
            continue;
        }
        let label = format!("{} [remove {}]", e.target, e.type_str);
        let action = if e.type_str == CODEX_HOOK_TYPE {
            Action::RemoveCodexHook { identity: e.marker.clone(), label }
        } else if e.marker.is_empty() {
            Action::RemoveWhole { lock_sha: e.rendered_sha256.clone(), label }
        } else {
            Action::RemoveBlock { marker: e.marker.clone(), label }
        };
        per_target.entry(raw).or_default().push(action);
    }

    struct Op {
        target: PathBuf,
        current: Option<String>,
        next: Option<String>, // None = ensure absent
        label: String,
        /// Set only for whole-file removals (see Action::RemoveWhole).
        lock_sha: Option<String>,
    }
    let mut ops: Vec<Op> = Vec::new();
    for (target, actions) in &per_target {
        let whole = actions
            .iter()
            .any(|a| matches!(a, Action::Set { .. } | Action::RemoveWhole { .. }));
        let blocky = actions
            .iter()
            .any(|a| matches!(a, Action::Upsert { .. } | Action::RemoveBlock { .. }));
        let json_hooky = actions.iter().any(|a| {
            matches!(a, Action::UpsertCodexHook { .. } | Action::RemoveCodexHook { .. })
        });
        let shape_count = usize::from(whole) + usize::from(blocky) + usize::from(json_hooky);
        if shape_count > 1 || (whole && actions.len() > 1) {
            // A file↔block transition (or two whole-file writers surviving the dup
            // check via lock history) in one run: ordering would decide who wins —
            // refuse loudly instead of guessing.
            bail!(
                "{}: whole-file and block operations collide on this target in one run — resolve manually",
                target.display()
            );
        }
        let current = match fs::read_to_string(target) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => bail!("read target {}: {e}", target.display()),
        };
        let mut lock_sha = None;
        let next = if whole {
            match &actions[0] {
                Action::Set { content, .. } => {
                    if content.trim().is_empty() { None } else { Some(content.clone()) }
                }
                Action::RemoveWhole { lock_sha: sha, .. } => {
                    lock_sha = Some(sha.clone());
                    None
                }
                _ => unreachable!(),
            }
        } else if blocky {
            // fold every block operation over one evolving document
            let mut doc = current.clone().unwrap_or_default();
            for a in actions {
                match a {
                    Action::Upsert { marker, content, .. } => doc = upsert_block(&doc, marker, content)?,
                    Action::RemoveBlock { marker, .. } => doc = upsert_block(&doc, marker, "")?,
                    _ => unreachable!(),
                }
            }
            // Block ops never DELETE a shared doc: an emptied document stays a file
            // (removing our block must not take the user's file with it).
            if current.is_none() && doc.trim().is_empty() { None } else { Some(doc) }
        } else {
            let mut doc = current.clone();
            for action in actions {
                doc = match action {
                    Action::UpsertCodexHook { hook, .. } => crate::codex_hook::reconcile(
                        doc.as_deref(),
                        Some(hook),
                        hook.identity,
                    )?,
                    Action::RemoveCodexHook { identity, .. } => {
                        crate::codex_hook::reconcile(doc.as_deref(), None, identity)?
                    }
                    _ => unreachable!(),
                };
            }
            doc
        };
        let label = actions.iter().map(Action::label).collect::<Vec<_>>().join(" + ");
        ops.push(Op { target: target.clone(), current, next, label, lock_sha });
    }

    // ---- --check: report drift, write nothing ----
    if args.check {
        let mut dirty = 0usize;
        for op in &ops {
            let status = match (&op.current, &op.next) {
                (None, None) => continue,
                (Some(cur), Some(next)) if normalize(cur) == normalize(next) => continue,
                (Some(_), Some(_)) => "DRIFT",
                (None, Some(_)) => "MISSING",
                (Some(_), None) => "PENDING-REMOVAL",
            };
            dirty += 1;
            println!("{status}\t{}", op.label);
        }
        for s in &skipped {
            println!("SKIPPED\t{s}");
        }
        if dirty == 0 {
            println!("check: clean ({} artifact(s) in scope) — owned artifact match; behavioral equivalence is the deletion-probe's job", ops.len());
            return Ok(0);
        }
        println!("check: {dirty} artifact(s) out of sync — run `flint install` to reconcile");
        return Ok(1);
    }

    // ---- --plan: dry-run summary, zero writes ----
    if args.plan {
        for op in &ops {
            match (&op.current, &op.next) {
                (None, None) => {}
                (Some(cur), Some(next)) if cur == next => println!("keep\t{}", op.label),
                (_, Some(_)) => println!("write\t{}", op.label),
                (Some(_), None) => println!("remove\t{}", op.label),
            }
        }
        for s in &skipped {
            println!("skip\t{s}");
        }
        println!("plan: dry run — nothing written");
        return Ok(0);
    }

    // ---- apply ----
    let mut wrote = 0usize;
    let mut removed = 0usize;
    let mut kept = 0usize;
    for op in &ops {
        match (&op.current, &op.next) {
            (None, None) => {}
            (Some(cur), Some(next)) if cur == next => kept += 1, // adopt-in-place
            (_, Some(next)) => {
                revalidate(&ctx, &op.target)?;
                if op.current.is_some() {
                    rotate_bak(&op.target)?;
                }
                if let Some(parent) = op.target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                fs::write(&op.target, next).with_context(|| format!("write {}", op.target.display()))?;
                wrote += 1;
            }
            (Some(cur), None) => {
                revalidate(&ctx, &op.target)?;
                let untouched = op
                    .lock_sha
                    .as_ref()
                    .is_some_and(|sha| sha256_hex(cur.as_bytes()) == *sha);
                if !untouched {
                    rotate_bak(&op.target)?; // a hand-edit would be lost — keep forensics
                }
                fs::remove_file(&op.target).with_context(|| format!("remove {}", op.target.display()))?;
                remove_empty_parents(&op.target, &ctx.allow_roots);
                removed += 1;
            }
        }
    }

    // ---- rebuild lock: out-of-scope entries survive untouched; in-scope = what we
    // now actually manage (installed entries; skipped keep their previous record). ----
    let mut new_entries: Vec<LockEntry> = if lock_trustworthy {
        lock.entries
            .iter()
            .filter(|e| {
                let in_run_scope = e.stage == Stage::Skills.as_str() || args.stage == Stage::Full;
                !in_run_scope || {
                    // keep records for entries we skipped this run (still claimed)
                    let key = (e.target.clone(), e.marker.clone());
                    claimed.contains(&key)
                        && !resolved.iter().any(|r| {
                            r.target.display().to_string() == e.target
                                && r.spec.marker().unwrap_or("") == e.marker
                        })
                }
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    for r in &resolved {
        let rendered = match &r.desired {
            Desired::File(c) => c.clone(),
            Desired::Block { content, .. } => content.clone(),
            Desired::CodexHook { rendered, .. } => rendered.clone(),
        };
        if rendered.trim().is_empty() {
            continue; // ensured-absent artifacts hold no lock entry
        }
        new_entries.push(LockEntry {
            target: r.target.display().to_string(),
            type_str: r.spec.type_str().to_string(),
            stage: r.spec.stage().as_str().to_string(),
            marker: r.spec.marker().unwrap_or("").to_string(),
            rendered_sha256: sha256_hex(rendered.as_bytes()),
            source_sha256: r.source_sha256.clone(),
        });
    }
    new_entries.sort_by_key(|e| (e.target.clone(), e.marker.clone()));
    lock.version = 1;
    lock.entries = new_entries;
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    // read-modify-write completes here (see the CONCURRENCY note above). fs::write is
    // truncate-then-write (non-atomic): a concurrent READER could observe a partial
    // lock, but the only reader is install itself, whose F1 load treats a corrupt lock
    // as "rebuild, remove nothing" — the safe direction. Atomic-replace is the same
    // escalation as the lock above, deferred with it.
    fs::write(&lock_path, toml::to_string_pretty(&lock).context("serialize lock")?)
        .with_context(|| format!("write {}", lock_path.display()))?;

    for s in &skipped {
        eprintln!("warning: skipped {s}");
    }
    println!(
        "install: {wrote} written, {kept} already current, {removed} removed ({} skipped) — lock {}",
        skipped.len(),
        lock_path.display()
    );
    Ok(0)
}

/// Last-moment allowlist re-check right before a mutation: SHRINKS (does not close)
/// the validate→write window against a same-UID symlink swap (codex WP1b P3). Same
/// honest boundary as canon pick's temp-dir signing (§3.5 tier): a same-UID attacker
/// concurrent with the installer is outside what code can fix — OS isolation is the
/// strong tier.
fn revalidate(ctx: &Ctx, target: &Path) -> Result<()> {
    let t = canonicalize_nearest(target)?;
    if !ctx.allow_roots.iter().any(|r| t.starts_with(r)) {
        bail!(
            "target moved outside the allowlist between validation and write: {}",
            target.display()
        );
    }
    Ok(())
}

/// Rolling 3-deep backup: `.bak.1` newest. Forensic trail only — RECOVERY goes
/// through the manifest (reinstall/revert), not by hand-restoring .baks (R8).
fn rotate_bak(target: &Path) -> Result<()> {
    let bak = |n: u32| PathBuf::from(format!("{}.bak.{n}", target.display()));
    let _ = fs::remove_file(bak(3));
    for n in [2u32, 1] {
        if bak(n).exists() {
            fs::rename(bak(n), bak(n + 1)).with_context(|| format!("rotate {}", bak(n).display()))?;
        }
    }
    fs::copy(target, bak(1)).with_context(|| format!("backup {}", target.display()))?;
    Ok(())
}

/// After removing a file, prune now-empty parent directories, stopping at (never
/// removing) any allowlist root.
fn remove_empty_parents(target: &Path, roots: &[PathBuf]) {
    let mut dir = target.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if roots.contains(&d) {
            break;
        }
        match fs::read_dir(&d) {
            Ok(mut it) => {
                if it.next().is_some() {
                    break; // not empty
                }
            }
            Err(_) => break,
        }
        if fs::remove_dir(&d).is_err() {
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_tagged_enum_rejects_half_legal_entries() {
        // generator with a source field → parse error
        let bad = r#"
[[artifact]]
stage = "skills"
type = "generator"
generator = "claude-advisory"
source = "nope.md"
target = "$CLAUDE_HOME/rules/x.md"
"#;
        assert!(toml::from_str::<ManifestFile>(bad).is_err());
        // file without source → parse error
        let bad = r#"
[[artifact]]
stage = "skills"
type = "file"
target = "$CLAUDE_HOME/skills/x.md"
"#;
        assert!(toml::from_str::<ManifestFile>(bad).is_err());
        // block without marker → parse error
        let bad = r#"
[[artifact]]
stage = "full"
type = "block"
source = "x.md"
target = "$CODEX_HOME/AGENTS.md"
"#;
        assert!(toml::from_str::<ManifestFile>(bad).is_err());
        // unknown type → parse error
        let bad = r#"
[[artifact]]
stage = "skills"
type = "symlink"
source = "x"
target = "$CLAUDE_HOME/x"
"#;
        assert!(toml::from_str::<ManifestFile>(bad).is_err());
    }

    #[test]
    fn manifest_parses_grok_hooks_generator_and_rejects_a_half_legal_one() {
        let good = r#"
[[artifact]]
stage = "full"
type = "generator"
generator = "grok-hooks"
target = "$GROK_HOME/hooks/flint.json"
"#;
        let parsed = toml::from_str::<ManifestFile>(good).unwrap();
        let artifact = &parsed.artifacts[0];
        assert_eq!(artifact.stage(), Stage::Full);
        assert_eq!(artifact.target(), "$GROK_HOME/hooks/flint.json");
        assert_eq!(artifact.type_str(), "generator");
        assert_eq!(
            artifact.marker(),
            None,
            "the grok wiring is a whole-file artifact — it owns no marked block"
        );
        // half-legal: a generator carrying a source field is a parse error, not a
        // half-honored entry.
        let with_source = good.replace("target =", "source = \"skills/a/SKILL.md\"\ntarget =");
        assert!(toml::from_str::<ManifestFile>(&with_source).is_err());
        // an unknown generator name must refuse, never silently install nothing.
        assert!(toml::from_str::<ManifestFile>(&good.replace("grok-hooks", "grok-wiring")).is_err());
    }

    #[test]
    fn manifest_codex_hook_is_strict() {
        let good = r#"
[[artifact]]
stage = "full"
type = "codex-hook"
hook = "flint-session-sync"
target = "$CODEX_HOME/hooks.json"
event = "SessionStart"
matcher = "startup|resume"
timeout = 30
"#;
        let parsed = toml::from_str::<ManifestFile>(good).unwrap();
        let artifact = &parsed.artifacts[0];
        assert_eq!(artifact.stage(), Stage::Full);
        assert_eq!(artifact.target(), "$CODEX_HOME/hooks.json");
        assert_eq!(artifact.type_str(), "codex-hook");
        assert_eq!(artifact.marker(), Some("flint-session-sync"));

        let missing_timeout = good.replace("timeout = 30\n", "");
        assert!(toml::from_str::<ManifestFile>(&missing_timeout).is_err());

        let unknown = good.replace("hook = \"flint-session-sync\"", "hook = \"other\"");
        assert!(toml::from_str::<ManifestFile>(&unknown).is_err());
    }

    #[test]
    fn upsert_block_roundtrip_and_corruption() {
        let doc = "# mine\n\n<!-- notes:begin -->\nold\n<!-- notes:end -->\ntail\n";
        let updated = upsert_block(doc, "notes", "new").unwrap();
        assert!(updated.contains("new") && !updated.contains("old"));
        assert!(updated.starts_with("# mine"));
        assert!(updated.contains("tail"));
        // removal keeps user content
        let removed = upsert_block(&updated, "notes", "").unwrap();
        assert!(!removed.contains("<!-- notes:begin -->"));
        assert!(removed.contains("# mine") && removed.contains("tail"));
        // duplicated markers refuse
        let corrupt = "<!-- m:begin -->\na\n<!-- m:end -->\n<!-- m:begin -->\nb\n<!-- m:end -->\n";
        assert!(upsert_block(corrupt, "m", "x").is_err());
    }

    #[test]
    fn normalize_ignores_trailing_ws_and_eol() {
        assert_eq!(normalize("a  \r\nb\r\n"), normalize("a\nb\n\n\n"));
        assert_ne!(normalize("a\nb\n"), normalize("a\nc\n"));
    }
}
