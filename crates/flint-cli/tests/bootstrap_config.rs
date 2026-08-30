//! WP-G P0 regression: the bootstrap entrypoints must pass `--config` to
//! `flint install`, or a `--stage full` run aborts inside the binary
//! ("manifest has generator entries in scope — pass --config"). The preflight
//! (`canon list`) already named `flint-global.toml`; the install hand-off
//! dropped it, so `bootstrap --stage full` was a guaranteed-error command —
//! the one full-stage path that was never exercised live (live stage=full runs
//! come from the SessionStart hook command, which does pass --config).
//!
//! `.sh` and `.ps1` are deliberately dumb mirrors ("semantics live in the
//! binary"); a config that appears in one entrypoint but not the other, or in
//! preflight but not the install hand-off, is the exact silent drift this
//! guards. We assert on script TEXT (not a live run) so the regression is
//! caught without a cargo build + a signed canon in the test.

use std::fs;
use std::path::{Path, PathBuf};

fn scripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts")
}

fn read_script(name: &str) -> String {
    let path = scripts_dir().join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Skip comment lines (`#` in both sh and PowerShell) so the matchers anchor on the real
/// command, never a comment above it that happens to mention the same tokens (a comment
/// documenting "the install line passes --stage and --config" must not retarget the test).
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// The line that hands off to `flint install` — the one carrying both `install`
/// and `--stage` (distinct from the `canon list` preflight line).
fn install_exec_line(script: &str, name: &str) -> String {
    script
        .lines()
        .find(|l| !is_comment(l) && l.contains("install") && l.contains("--stage"))
        .unwrap_or_else(|| panic!("no `install ... --stage` hand-off line found in {name}"))
        .to_string()
}

/// The stage-full preflight line — the `canon list` that already used --config.
fn preflight_line(script: &str, name: &str) -> String {
    script
        .lines()
        .find(|l| !is_comment(l) && l.contains("canon") && l.contains("list"))
        .unwrap_or_else(|| panic!("no `canon list` preflight line found in {name}"))
        .to_string()
}

/// The whitespace-delimited token immediately after `--config` on a command line — the
/// config argument itself (a `$VAR`, not the `--manifest`'s `.toml` path).
fn config_arg(line: &str) -> Option<String> {
    let mut it = line.split_whitespace();
    while let Some(tok) = it.next() {
        if tok == "--config" {
            return it.next().map(str::to_string);
        }
    }
    None
}

#[test]
fn bootstrap_default_instance_root_is_memory_in_both_scripts() {
    // sh/ps1 parity: the --instance default clone root must agree across the two
    // entrypoints — nothing else pins them together.
    let sh = read_script("bootstrap.sh");
    assert!(
        sh.contains("${INSTANCE_ROOT:-$HOME/memory}"),
        "bootstrap.sh default instance root must be $HOME/memory"
    );
    let ps1 = read_script("bootstrap.ps1");
    assert!(
        ps1.contains(r#"Join-Path $env:USERPROFILE "memory""#),
        "bootstrap.ps1 default instance root must be USERPROFILE\\memory"
    );
}

#[test]
fn bootstrap_sh_install_passes_config() {
    let text = read_script("bootstrap.sh");
    let line = install_exec_line(&text, "bootstrap.sh");
    assert!(
        line.contains("--config"),
        "bootstrap.sh install hand-off must pass --config (stage=full requires it, \
         WP-G P0):\n  {line}"
    );
}

#[test]
fn bootstrap_ps1_install_passes_config() {
    let text = read_script("bootstrap.ps1");
    let line = install_exec_line(&text, "bootstrap.ps1");
    assert!(
        line.contains("--config"),
        "bootstrap.ps1 install hand-off must pass --config (stage=full requires it, \
         WP-G P0):\n  {line}"
    );
}

/// Preflight and install must draw from the SAME config. Both scripts route the
/// config through a single token/variable so the two hand-offs cannot drift; we
/// assert both the preflight and the install line reference `--config`, and that
/// the config path token (`flint-global.toml`) appears in each entrypoint.
#[test]
fn bootstrap_preflight_and_install_share_one_config() {
    for name in ["bootstrap.sh", "bootstrap.ps1"] {
        let text = read_script(name);
        let preflight = preflight_line(&text, name);
        let install = install_exec_line(&text, name);
        assert!(
            preflight.contains("--config"),
            "{name}: preflight (canon list) must pass --config:\n  {preflight}"
        );
        assert!(
            install.contains("--config"),
            "{name}: install hand-off must pass --config:\n  {install}"
        );
        // Preflight and install must pass the SAME --config token, and it must be a
        // variable (not a literal `.toml`), so the two hand-offs cannot silently point at
        // DIFFERENT configs (P2 — the two must be one). We compare the --config ARG, not
        // the whole line (which also carries the `--manifest` .toml path).
        let p_cfg = config_arg(&preflight).unwrap_or_else(|| panic!("{name}: preflight has no --config arg:\n  {preflight}"));
        let i_cfg = config_arg(&install).unwrap_or_else(|| panic!("{name}: install has no --config arg:\n  {install}"));
        assert_eq!(
            p_cfg, i_cfg,
            "{name}: preflight and install must pass the SAME --config token (single source)"
        );
        assert!(
            !i_cfg.contains(".toml"),
            "{name}: --config must route through the shared variable, not a literal .toml path (got `{i_cfg}`)"
        );
        assert!(
            text.contains("flint-global.toml"),
            "{name}: the shared config variable must resolve to flint-global.toml (single source)"
        );
    }
}
