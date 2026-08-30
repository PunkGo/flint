//! PIP-0001 core-freeze smoke gate (suite plan v4.3 WP3).
//!
//! flint-core is the judge face. It must never grow knowledge-management organs
//! (memory buckets, retro/curation, auto-promotion, mining) — those belong to the
//! private memory instance, not the enforcement layer. This gate is a SMOKE ALARM:
//! crude word-root assertions over code lines, deliberately 拦粗不拦精. It exists to
//! make accidental scope creep loud, not to adjudicate intent — a tripped root with a
//! legitimate reason is amended HERE, in review, with a grandfather entry.
//!
//! Scope: `crates/flint-core/src/**/*.rs` code lines (comments stripped — docs and
//! comments may legally NAME these concepts). `flint-cli/src/install.rs` is explicitly
//! OUT of scope: the installer legally reads `instance.toml` (it is the "carry it over"
//! action, not the judge). Additionally, the hook entrypoints (`flint-cli/src/hook.rs`,
//! `cross_vendor.rs`) must never reference `instance.toml` at all — the judge/hook
//! path must not even know that file exists (plan §2 / R9).
//!
//! Full rationale + amendment protocol: docs/PIP-0001-core-freeze.md.

use std::fs;
use std::path::{Path, PathBuf};

/// Banned word-roots in flint-core code lines. Matched case-insensitively with
/// alphanumeric boundaries (`_` COUNTS as a boundary so `fn retro_scan` trips).
const BANNED_ROOTS: &[&str] = &[
    "casefiles",
    "briefs",
    "portals",
    "archive",
    "retro",
    "promote",
    "classify",
    "distill",
    "instance.toml",
];

/// `maps` is banned only in bucket/path form (`maps/`): the bare word is everyday
/// Rust and English (`.map(`, `fn block_maps_to_deny`, "maps X into Y").
const BANNED_PATH_FORMS: &[&str] = &["maps/"];

/// Dormant outer ring, frozen as-is (外圈冻结休眠): these (file, root) pairs predate
/// the gate and are grandfathered. Growing NEW uses of the root in OTHER files still
/// trips. Removing the ring is a separate, deliberate surgery — not this gate's job.
const GRANDFATHERED: &[(&str, &str)] = &[("forge.rs", "promote")];

struct Finding {
    file: String,
    line_no: usize,
    root: &'static str,
    line: String,
}

/// Strip `//` line/doc comments. Crude by design (a `//` inside a string literal cuts
/// the rest of the line — that only under-matches, acceptable for a smoke alarm).
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// Case-insensitive word-root search: the match must not be flanked by [a-zA-Z0-9].
/// `_` is a boundary on purpose — identifiers like `retro_scan` must trip.
fn hits_root(code: &str, root: &str) -> bool {
    let hay = code.to_ascii_lowercase();
    let needle = root.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = hay[from..].find(&needle) {
        let start = from + pos;
        let end = start + needle.len();
        let left_ok = start == 0 || !is_word_byte(hay.as_bytes()[start - 1]);
        let right_ok = end == hay.len() || !is_word_byte(hay.as_bytes()[end]);
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn scan_source(file_label: &str, text: &str) -> Vec<Finding> {
    let file_name = Path::new(file_label)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_label.to_string());
    let mut findings = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let code = code_part(raw);
        if code.trim().is_empty() {
            continue;
        }
        for root in BANNED_ROOTS {
            if GRANDFATHERED.contains(&(file_name.as_str(), root)) {
                continue;
            }
            if hits_root(code, root) {
                findings.push(Finding {
                    file: file_label.to_string(),
                    line_no: idx + 1,
                    root,
                    line: raw.trim_end().to_string(),
                });
            }
        }
        for form in BANNED_PATH_FORMS {
            // path forms match as plain substrings of the code part (case-insensitive),
            // with a word boundary on the left only (`maps/` should trip inside a
            // longer path like `"vault/maps/x.md"`).
            let hay = code.to_ascii_lowercase();
            let mut from = 0;
            while let Some(pos) = hay[from..].find(form) {
                let start = from + pos;
                let left_ok = start == 0 || !is_word_byte(hay.as_bytes()[start - 1]);
                if left_ok {
                    findings.push(Finding {
                        file: file_label.to_string(),
                        line_no: idx + 1,
                        root: "maps/",
                        line: raw.trim_end().to_string(),
                    });
                    break;
                }
                from = start + 1;
            }
        }
    }
    findings
}

fn rs_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => panic!("freeze gate: read_dir {}: {e}", dir.display()),
    };
    for entry in entries {
        let entry = entry.expect("freeze gate: dir entry");
        let path = entry.path();
        if path.is_dir() {
            rs_files_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn scan_tree(dir: &Path) -> Vec<Finding> {
    let mut files = Vec::new();
    rs_files_under(dir, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "freeze gate: no .rs files under {} — scan scope broken",
        dir.display()
    );
    let mut findings = Vec::new();
    for f in files {
        let text = fs::read_to_string(&f)
            .unwrap_or_else(|e| panic!("freeze gate: read {}: {e}", f.display()));
        findings.extend(scan_source(&f.display().to_string(), &text));
    }
    findings
}

fn render(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("  {}:{} [{}] {}", f.file, f.line_no, f.root, f.line))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// The gate itself: the current tree must be green.
// ---------------------------------------------------------------------------

#[test]
fn core_src_is_free_of_banned_roots() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let findings = scan_tree(&src);
    assert!(
        findings.is_empty(),
        "PIP-0001 core freeze tripped — flint-core/src grew a banned word-root.\n\
         Either this is scope creep (remove it) or a deliberate, reviewed exception\n\
         (add a GRANDFATHERED entry + amend docs/PIP-0001-core-freeze.md):\n{}",
        render(&findings)
    );
}

/// The judge/hook entrypoints must not reference instance.toml even in comments-adjacent
/// code — the hook path must not know the file exists (R9: config brick isolation).
#[test]
fn hook_entrypoints_never_reference_instance_toml() {
    let cli_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../flint-cli/src");
    for entry in ["hook.rs", "cross_vendor.rs"] {
        let path = cli_src.join(entry);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("freeze gate: read {}: {e}", path.display()));
        let findings: Vec<_> = scan_source(&path.display().to_string(), &text)
            .into_iter()
            .filter(|f| f.root == "instance.toml")
            .collect();
        assert!(
            findings.is_empty(),
            "hook path references instance.toml (judge/hook must never read it):\n{}",
            render(&findings)
        );
    }
}

// ---------------------------------------------------------------------------
// Scanner acceptance: injection trips, legal references don't (plan WP3 验收).
// ---------------------------------------------------------------------------

#[test]
fn injected_identifier_trips() {
    let src = "fn retro_scan(log: &str) -> usize { log.len() }\n";
    let f = scan_source("synthetic.rs", src);
    assert_eq!(f.len(), 1, "fn retro_scan must trip the gate");
    assert_eq!(f[0].root, "retro");

    let src = "let casefiles_dir = std::path::PathBuf::from(root);\n";
    assert!(!scan_source("synthetic.rs", src).is_empty(), "casefiles identifier must trip");

    let src = "cfg.load(\"instance.toml\")?;\n";
    assert!(!scan_source("synthetic.rs", src).is_empty(), "instance.toml in code must trip");
}

#[test]
fn comments_and_docs_do_not_trip() {
    let src = "\
//! Pits are curated into casefiles by the OWNER, never by this crate (retro lives
//! in the memory instance). See also: briefs, portals, archive, instance.toml.
// promote/classify/distill are banned roots — documented here legally.
fn judge() {} // maps the action to a verdict, then archives nothing
";
    let f = scan_source("synthetic.rs", src);
    assert!(
        f.is_empty(),
        "comments/docs legally naming banned roots must NOT trip:\n{}",
        render(&f)
    );
}

#[test]
fn bare_maps_is_legal_but_bucket_path_trips() {
    let legal = "fn block_maps_to_deny_warn_maps_to_critique() {}\nlet x = items.iter().map(|i| i);\n";
    assert!(
        scan_source("synthetic.rs", legal).is_empty(),
        "bare `maps`/`map` words must not trip (everyday Rust/English)"
    );

    let bucket = "let p = format!(\"{root}/maps/index.md\");\n";
    let f = scan_source("synthetic.rs", bucket);
    assert_eq!(f.len(), 1, "maps/ bucket path form must trip");
    assert_eq!(f[0].root, "maps/");
}

#[test]
fn grandfather_is_file_scoped_not_global() {
    // promote in forge.rs is grandfathered (dormant outer ring)…
    let f = scan_source("crates/flint-core/src/forge.rs", "pub fn promote() {}\n");
    assert!(f.is_empty(), "forge.rs promote is grandfathered");
    // …but the SAME root in any other file still trips.
    let f = scan_source("crates/flint-core/src/canon.rs", "pub fn promote() {}\n");
    assert_eq!(f.len(), 1, "promote outside forge.rs must trip");
}
