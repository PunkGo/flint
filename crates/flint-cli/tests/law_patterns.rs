//! Pattern contract for the SHIPPED lsp-over-grep law family — the regex is data
//! (law frontmatter), so its behavior is pinned here against the REAL shipped bytes
//! (`include_str!`), not a re-typed copy. Fixtures encode the 2026-08-08 false-positive
//! harvest (six "command merely MENTIONS a grep-containing filename" hits on mac, plus
//! the cross-segment span and over-wide dir list forms) and the v5 contract:
//!
//!   - `lsp-over-grep` (warn, flint/v2): fires when a grep-family tool is used AS A
//!     COMMAND and a code signal (source extension / strong source dir) appears in the
//!     SAME `;`/`&`/newline-delimited segment. Pipes do not split a segment (a pipeline
//!     is one dataflow). The call PROCEEDS — the law recommends LSP, it does not block,
//!     and it carries no exempt arm (nothing to bypass once nothing blocks).
//!   - `lsp-over-grep-sweep` (warn, flint/v2): the undecidable bucket — recursive or
//!     bare default-recursive searches whose target cannot be classified from the
//!     command line. Non-blocking: the action proceeds, the agent is told, receipted.
//!   - judge reporting: when both match, ONE verdict is reported — the deterministic
//!     total order picks the smallest `(scope, rule_id)`, so the specific law speaks
//!     and the generic sweep never doubles up.

use std::collections::BTreeMap;
use std::path::PathBuf;

use flint_core::canon::{parse_rule, reduce, ParsedRule};
use flint_core::touchstone::{exempted_rules, judge, Action, Response, TouchstonePolicy, Verdict};

const LSP_LAW: &str = include_str!("../../../examples/laws/lsp-over-grep.md");
const SWEEP_LAW: &str = include_str!("../../../examples/laws/lsp-over-grep-sweep.md");

fn policy() -> TouchstonePolicy {
    let mut files: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    files.insert(PathBuf::from("rules/lsp-over-grep.md"), LSP_LAW.as_bytes().to_vec());
    files.insert(PathBuf::from("rules/lsp-over-grep-sweep.md"), SWEEP_LAW.as_bytes().to_vec());
    reduce(&files).expect("both shipped laws reduce into a policy")
}

fn bash(command: &str) -> Action {
    Action {
        tool_kind: "exec".into(),
        scopes: vec!["exec:bash".into()],
        command: Some(command.into()),
        ..Default::default()
    }
}

fn verdict_tag(cmd: &str) -> &'static str {
    judge(&policy(), &bash(cmd)).tag()
}

// ── law shape ─────────────────────────────────────────────────────────────────

#[test]
fn shipped_laws_parse_with_expected_tiers() {
    let lsp = match parse_rule("rules/lsp-over-grep.md", LSP_LAW).expect("lsp law parses") {
        ParsedRule::Gate(cr) => cr,
        ParsedRule::Advisory(_) => panic!("lsp-over-grep is a gate rule"),
    };
    assert_eq!(lsp.rule.id, "lsp-over-grep");
    assert_eq!(lsp.rule.response, Response::Warn, "the law recommends, it does not block");
    assert_eq!(lsp.meta.schema, "flint/v2", "warn is v2-only vocabulary");

    let sweep = match parse_rule("rules/lsp-over-grep-sweep.md", SWEEP_LAW).expect("sweep law parses") {
        ParsedRule::Gate(cr) => cr,
        ParsedRule::Advisory(_) => panic!("lsp-over-grep-sweep is a gate rule"),
    };
    assert_eq!(sweep.rule.id, "lsp-over-grep-sweep");
    assert_eq!(sweep.rule.response, Response::Warn, "the undecidable bucket is non-blocking");
    assert_eq!(sweep.meta.schema, "flint/v2", "warn is v2-only vocabulary");
}

// ── warn: true positives the tightening must NOT lose ─────────────────────────

#[test]
fn code_greps_still_speak() {
    for cmd in [
        // extension signal, tool at command start
        "grep -rn fold_veto crates/flint-core/src/touchstone.rs",
        // strong dir signal
        "rg -n Warn src/",
        // pipeline: source content flows into grep (pipe does not split the segment)
        "cat src/main.rs | grep judge",
        // path-producer into xargs (known over-catch, declared-downgrade escape exists)
        "find src -name *.rs | xargs grep TODO",
        // git grep is the same offense
        "git grep -n canon src/",
        // env-assign prefix does not hide command position
        "RUST_LOG=debug rg pattern src/lib.rs",
        // second segment pairs tool + signal WITHIN itself
        "cargo build && grep foo src/main.rs",
        // subshell position
        "echo $(grep foo src/main.rs)",
        // wrapper flags that take a VALUE (codex probe: these were misses)
        "git -C repo grep foo src/main.rs",
        "sudo -u root grep foo src/main.rs",
        // quoted env-assign value with a space
        "RUST_LOG=\"trace spans\" rg foo src/main.rs",
        // absolute / relative path to the tool binary
        "/usr/bin/rg foo src/main.rs",
        // fd redirects must not break same-segment pairing (&1 is not a separator)
        "cat src/main.rs 2>&1 | grep foo",
        // more legitimate command prefixes (codex round 2: these were misses)
        "env RUST_LOG=debug rg foo src/main.rs",
        "git -C 'repo dir' grep foo src/main.rs",
        "if grep -q foo src/main.rs; then echo hit; fi",
        // a quoted metacharacter is pattern data, not a segment boundary
        "grep ';' src/main.rs",
        // a QUOTED target is still a target — only the pattern argument is opaque
        // (codex round 3: these were misses after the quote-atomicity fix)
        "rg foo \"src/main.rs\"",
        "cat \"src/main.rs\" | grep foo",
        // shell negation is a command prefix
        "if ! grep -q foo src/main.rs; then exit 1; fi",
        // a directory target after a plain pattern
        "rg unused crates/",
        // the pattern can ride an attached option — the next token is then a TARGET
        // (codex round 4)
        "grep -efoo src/main.rs",
        "grep -fpatterns.txt src/main.rs",
        "rg --regexp=foo src/main.rs",
        // -- terminates options; a dash-prefixed pattern may follow, target still seen
        "grep -- -r src/main.rs",
        "rg -- -foo src/main.rs",
    ] {
        assert_eq!(verdict_tag(cmd), "warn", "must speak: {cmd}");
    }
}

// ── the 2026-08-08 false-positive forms must affirm ───────────────────────────

#[test]
fn mentioning_a_grep_named_file_is_not_grepping() {
    // The six-FP form: `grep` as a SUBSTRING of a filename argument, never a command.
    for cmd in [
        "cat crates/flint-cli/laws/lsp-over-grep.md",
        "git add crates/flint-cli/laws/lsp-over-grep.md",
        "cp crates/flint-cli/laws/lsp-over-grep.md /tmp/backup.md",
        "vim src/notes/lsp-over-grep-v4-design.md",
        "echo grep src/main.rs inside prose is not a command",
    ] {
        assert_eq!(verdict_tag(cmd), "affirm", "filename mention must not fire: {cmd}");
    }
}

#[test]
fn cross_segment_pairing_does_not_fire() {
    // v3 span bug: a code signal in segment A + a grep in segment B fired. Segments are
    // delimited by `;`, `&`, and newline; each must pair tool + signal on its own.
    for cmd in [
        "grep foo x.txt && cat src/main.rs",
        "cargo build --manifest-path crates/flint-cli/Cargo.toml; grep ERROR build.log",
        "rg foo notes.txt\ncat src/main.rs",
        // || is a command separator like &&, not a pipe (codex probe: this fired)
        "grep foo notes.txt || cat src/main.rs",
        // quoted prose is not a subshell — a bare ( is no longer command position
        "echo '(grep foo src/main.rs)'",
    ] {
        assert_eq!(verdict_tag(cmd), "affirm", "cross-segment must not fire: {cmd}");
    }
}

#[test]
fn narrowed_dir_list_and_text_greps_affirm() {
    for cmd in [
        // plain-text targets (the legitimate grep half of the law)
        "grep TODO README.md",
        "rg error logs/app.log",
        "grep foo notes.jsonl",
        // `app` left the dir list: too common in doc paths
        "rg heading docs/app/page.md",
        // pipe filters over command output are not code navigation
        "ps aux | grep flint",
        "cargo test 2>&1 | rg FAILED",
        "history | grep cargo",
        // a QUOTED search pattern is opaque data — a code-looking string searched
        // in a text file is not code navigation (codex round 2: these fired)
        "rg 'main.rs' README.md",
        "rg 'src/' README.md",
    ] {
        assert_eq!(verdict_tag(cmd), "affirm", "must affirm: {cmd}");
    }
}

// ── warn: the undecidable bucket ──────────────────────────────────────────────

#[test]
fn blind_recursive_sweeps_warn() {
    for cmd in [
        // explicit recursive flag, unclassifiable target
        "grep -rn TODO .",
        "grep -R secret /etc",
        "sudo grep -r password /var",
        // rg / ag / ack are recursive BY DEFAULT with no target argument
        "rg unwrap",
        "ag console",
        "rg -i unwrap",
        "RUST_LOG=debug rg panic",
        // type-filtered and post-argument-flag shapes are still blind sweeps
        "rg -t rust TODO",
        "rg TODO --hidden",
        // || separates commands, so the rg after it is in command position
        "false || rg unwrap",
        // a pathed binary is still the tool
        "/opt/homebrew/bin/rg unwrap",
        // value-taking flags do not hide the missing target (codex rounds 2-3),
        // on either side of the pattern
        "rg --threads 4 TODO",
        "rg -m 5 TODO",
        "rg TODO --threads 4",
        // an explicit bare-dot target is still an unclassifiable recursive sweep
        "rg TODO .",
        // negation is a command prefix
        "! rg TODO",
        // a bare pattern that merely LOOKS like code is still target-less: the
        // pattern argument is opaque, so this is the sweep tier, not the gate
        "rg main.rs",
        // a dash-prefixed pattern after -- with no target is still a blind sweep
        "rg -- -foo",
        // --color takes a value: the codex agent default shape (Windows smoke,
        // 2026-08-08: this affirmed because `never` swallowed the pattern slot)
        "rg -n --color never \"unwrap\" .",
        "rg --color always TODO",
    ] {
        assert_eq!(verdict_tag(cmd), "warn", "must warn: {cmd}");
    }
}

#[test]
fn scoped_or_piped_searches_do_not_warn() {
    for cmd in [
        // explicit file target: classifiable, handled by the critique rule or free
        "rg foo README.md",
        "grep foo x.txt",
        // piped input: the tool filters stdin, nothing is swept
        "cargo test | rg FAILED",
        "journalctl | grep -r oom",
        // a long flag containing r is not a recursive flag
        "grep --word-regexp foo notes.txt",
        // -r on a non-grep tool is not a sweep
        "sort -r file.txt",
        // quoted prose is not a sweep (codex probe: this fired as warn)
        "echo '(rg unwrap)'",
        // a generic flag does not swallow the pattern: the explicit target scopes this
        "rg -e unwrap README.md",
        // -- ends option parsing: what follows is a literal pattern (codex round 2)
        "grep -- -r README.md",
        "grep -- --recursive README.md",
        // a recursive-looking token inside a quoted pattern is data
        "grep 'a -r b' README.md",
    ] {
        assert_eq!(verdict_tag(cmd), "affirm", "must not warn: {cmd}");
    }
}

#[test]
fn both_laws_matching_report_the_min_id() {
    // Both rules match a command carrying a recursive flag AND a visible code target.
    // Judge reports ONE verdict and the deterministic total order picks the smallest
    // (scope, rule_id) — the specific law speaks, the generic sweep never doubles up.
    for cmd in ["grep -rn foo src/", "sudo grep -r foo src/"] {
        let v = judge(&policy(), &bash(cmd));
        assert!(v.is_warn(), "neither tier blocks any more: {cmd}");
        match v {
            Verdict::Warn { rule_id, .. } => assert_eq!(rule_id, "lsp-over-grep", "min id wins: {cmd}"),
            other => panic!("expected a warn verdict for {cmd}, got {other:?}"),
        }
    }
}

// ── the retired exempt arm stays retired ──────────────────────────────────────

#[test]
fn the_old_bypass_prefix_is_inert() {
    // The family has no exempt arm: there is nothing to bypass once nothing blocks.
    // The old variable survives only as an ordinary environment-assignment prefix, so
    // the note still fires and no passage is receipted as an exempt downgrade.
    let p = policy();
    for cmd in [
        "FLINT_LSP_BYPASS=string-literal grep foo src/main.rs",
        "FLINT_LSP_OVER_GREP_BYPASS=string-literal grep foo src/main.rs",
        "FLINT_LSP_BYPASS=big-text-sweep rg unwrap",
        "$env:FLINT_LSP_BYPASS=\"stdout-filter\"; grep foo src/main.rs",
        "echo $(FLINT_LSP_BYPASS=big-text-sweep rg unwrap)",
        "true\nFLINT_LSP_BYPASS=stdout-filter grep foo src/main.rs",
    ] {
        let a = bash(cmd);
        assert_eq!(judge(&p, &a).tag(), "warn", "the note still fires: {cmd}");
        assert!(exempted_rules(&p, &a).is_empty(), "no exempt arm exists: {cmd}");
    }
}
