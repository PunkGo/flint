//! Canon — the markdown rule source (PIVOT Contract A · reframe-and-diff §3.6).
//!
//! A Canon rule is a markdown file: a strict `---`-fenced frontmatter (flat
//! `key: value` lines) + a body that IS the agent-facing message (the dialectical
//! critique / deny reason). Canon REPLACES the on-ledger `gate_rule` events as the
//! source of the sovereign [`TouchstonePolicy`]; the judge core (`touchstone`) is
//! unchanged — only where the policy comes from changes.
//!
//! FAIL-CLOSED (reframe-and-diff §3.6 / codex r3): the parser is FALLIBLE and the
//! reducer returns `Result`. ANY malformed rule -> `Err` for the WHOLE Canon — never
//! "skip the bad rule and serve the rest" (in current-state markdown there is no older
//! valid version to fall back to, so dropping a malformed deny = the deny silently
//! vanishes = `Affirm` by absence = fail-OPEN). The OLD `projector::project_touchstone_policy`
//! skipped malformed `gate_rule` versions (sound only because the ledger replayed an
//! earlier valid version); that infallible-skip habit is DELIBERATELY NOT carried over.
//! A malformed Canon -> reducer `Err` -> the hook's existing "gate error -> fail-closed
//! deny (block mode)" path. The sovereign-facing `flint canon lint` surfaces the same
//! `Err` BEFORE a pick, so malformed rules never reach a signed manifest.
//!
//! Frontmatter is hand-parsed (no YAML dep): the schema is small + flat, and a strict
//! hand parser gives full control of the fail-closed behavior. Values may be wrapped in
//! matching single/double quotes (stripped literally — NO escape processing, so a regex
//! `pattern: '\b(grep)\b'` keeps its backslashes). Unknown keys are REJECTED (a typo'd
//! field is a loud error, not a silently-dropped field).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::glob::glob_is_wellformed;
use crate::touchstone::{
    pattern_compiles, AdvisoryRule, GateRule, Matcher, Response, Reversibility, TouchstonePolicy,
};

/// Evidence maturity of a rule (reframe-and-diff §3.7). Parsed + stored in P1 but NOT
/// yet load-bearing: in P1 a rule bears weight because it is in the signed manifest
/// (§3.5). The tier-derivation gate (a hand-written `reproduced` with no Forge promotion
/// record is treated as `prose`) is P2. Ordered: prose < attested < anchored < reproduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum EvidenceTier {
    #[default]
    Prose,
    Attested,
    Anchored,
    Reproduced,
}

impl EvidenceTier {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "prose" => Some(Self::Prose),
            "attested" => Some(Self::Attested),
            "anchored" => Some(Self::Anchored),
            "reproduced" => Some(Self::Reproduced),
            _ => None,
        }
    }
}

/// Provenance/authorship of a rule (schema v1 `source.kind`). Attribution ONLY —
/// deliberately NOT an evidence grade or a load-bearing signal (that would revive
/// the frozen evidence_tier addon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Human,
    Observation,
    Claude,
    Codex,
    Reading,
}

impl SourceKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "human" => Some(Self::Human),
            "observation" => Some(Self::Observation),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "reading" => Some(Self::Reading),
            _ => None,
        }
    }
}

/// Lifecycle status of a rule (self-contained spec §2 — the sovereignty model). A default
/// law is PROPOSED at `flint init` (written, but unsigned — it does NOT bear weight); the
/// sovereign's `flint law accept` signs it = the first real `pick` (propose≠pick from t=0).
/// `disabled` is a SEPARATE state (accepted, then turned off — proposed≠disabled); `removed`
/// is an auditable tombstone (a deletion is recorded, never a silent vanish, §6).
///
/// ABSENT status defaults to [`Status::Accepted`]: pre-existing signed canon (and any rule
/// written before this field) bears weight by default — omitting `status` must NEVER
/// silently disable a rule (fail-safe: a rule is enforced unless explicitly held back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// Written by `init`, not yet signed — does not bear weight until `flint law accept`.
    Proposed,
    /// Signed and enforced (the default when the field is absent).
    #[default]
    Accepted,
    /// Was accepted, now turned off — signed (auditable) but not enforced.
    Disabled,
    /// Removed — a signed tombstone recording the deletion; not enforced.
    Removed,
}

impl Status {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "proposed" => Some(Self::Proposed),
            "accepted" => Some(Self::Accepted),
            "disabled" => Some(Self::Disabled),
            "removed" => Some(Self::Removed),
            _ => None,
        }
    }

    /// The canonical frontmatter word.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Accepted => "accepted",
            Self::Disabled => "disabled",
            Self::Removed => "removed",
        }
    }

    /// Does this rule contribute to the ACTIVE policy? accepted + proposed do (proposed is
    /// validated by `lint`, but never reaches the signed set — `pick` excludes it); disabled
    /// + removed do not (inert).
    pub fn is_active(self) -> bool {
        matches!(self, Self::Accepted | Self::Proposed)
    }

    /// May `pick` sign this rule into the manifest? Everything EXCEPT proposed (accepted is
    /// enforced; disabled / removed are signed as auditable inactive records). A proposed law
    /// stays unsigned until `flint law accept` flips it — that IS the sovereignty boundary.
    pub fn is_signable(self) -> bool {
        !matches!(self, Self::Proposed)
    }
}

/// schema v1 provenance metadata for a rule. NONE of it reaches the judge
/// ([`GateRule`]) — it serves `canon lint`, `canon list`, and (later, Plan 2)
/// Striker scope-filtering. Kept deliberately thin (no evidence-grading / energy /
/// veto fields — those are frozen addons).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMeta {
    /// The schema tag, e.g. `flint/v1`.
    pub schema: String,
    /// Human-readable revision number (defaults to 1). NOT the manifest epoch.
    pub version: u32,
    /// ISO date `YYYY-MM-DD`, if declared.
    pub created: Option<String>,
    /// One-line third-person "what + when" (routing / injection text).
    pub description: Option<String>,
    /// Who authored it (attribution).
    pub source_kind: Option<SourceKind>,
    /// A traceable pointer (doc path / session id / commit / URL).
    pub source_ref: Option<String>,
    /// Effect-range selectors (`global` / `project:x` / `agent:claude` / ...).
    pub scope: Vec<String>,
    /// Rule ids this one supersedes.
    pub supersedes: Vec<String>,
    /// Free-form retrieval tags.
    pub tags: Vec<String>,
}

impl Default for RuleMeta {
    fn default() -> Self {
        Self {
            schema: String::new(),
            version: 1,
            created: None,
            description: None,
            source_kind: None,
            source_ref: None,
            scope: Vec::new(),
            supersedes: Vec::new(),
            tags: Vec::new(),
        }
    }
}

/// A parsed Canon rule: the runtime [`GateRule`], its schema-v1 provenance
/// [`RuleMeta`], plus its declared evidence tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonRule {
    pub rule: GateRule,
    pub meta: RuleMeta,
    pub tier: EvidenceTier,
}

/// The result of parsing one Canon markdown entry: a runtime gate rule
/// (kind:command|path) or an advisory guideline (kind:advisory). `reduce` routes each
/// into the policy's `rules` / `advisory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedRule {
    Gate(CanonRule),
    Advisory(AdvisoryRule),
}

/// Why a Canon file / set is inadmissible. Every variant is fail-closed (the rule, and
/// thus the whole Canon, does not bear weight). Carries the offending file/id for lint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonError {
    NoFrontmatter { file: String },
    UnterminatedFrontmatter { file: String },
    MissingField { file: String, field: &'static str },
    UnknownKey { file: String, key: String },
    DuplicateKey { file: String, key: String },
    BadValue { file: String, field: &'static str, value: String },
    BadGlob { file: String, glob: String },
    BadPattern { file: String, field: &'static str },
    EmptyMessage { file: String },
    /// Two rules share an `id` — non-deterministic precedence, rejected (no silent override).
    DuplicateId { id: String, first: String, second: String },
    /// A reserved schema-v1 carrier/kind whose behavior isn't wired yet (advisory
    /// = Plan 2, pit = Plan 3). Fail-closed: never silently ignored.
    NotYetSupported { file: String, what: String },
    /// A `supersedes` entry names an id that no rule in the Canon defines
    /// (a dangling relation — the knowledge graph must not rot).
    DanglingRef { file: String, id: String },
    /// A schema-v1 rule uses a `response` word whose MEANING changed in v2. A signature
    /// authenticates bytes, not parser semantics, so silently re-reading the old bytes
    /// under the new vocabulary would change what a signed law does without the sovereign
    /// signing anything. Fail closed until they migrate and re-sign, deliberately.
    DriftedResponse { file: String, word: String },
}

impl std::fmt::Display for CanonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonError::NoFrontmatter { file } => write!(f, "{file}: missing `---` frontmatter"),
            CanonError::UnterminatedFrontmatter { file } => write!(f, "{file}: frontmatter not closed by `---`"),
            CanonError::MissingField { file, field } => write!(f, "{file}: missing required field `{field}`"),
            CanonError::UnknownKey { file, key } => write!(f, "{file}: unknown frontmatter key `{key}`"),
            CanonError::DuplicateKey { file, key } => write!(f, "{file}: duplicate frontmatter key `{key}`"),
            CanonError::BadValue { file, field, value } => write!(f, "{file}: field `{field}` has bad value `{value}`"),
            CanonError::BadGlob { file, glob } => write!(f, "{file}: malformed scope glob `{glob}`"),
            CanonError::BadPattern { file, field } => write!(f, "{file}: field `{field}` is not a valid regex"),
            CanonError::EmptyMessage { file } => write!(f, "{file}: empty body (the rule message is the body)"),
            CanonError::DuplicateId { id, first, second } => {
                write!(f, "duplicate rule id `{id}` in `{first}` and `{second}`")
            }
            CanonError::NotYetSupported { file, what } => {
                write!(f, "{file}: {what} is not supported yet")
            }
            CanonError::DanglingRef { file, id } => {
                write!(f, "{file}: supersedes an unknown rule id `{id}`")
            }
            CanonError::DriftedResponse { file, word } => write!(
                f,
                "{file}: `response: {word}` under `schema: flint/v1` no longer has a settled \
                 meaning. In v1 `{word}` BLOCKED the action (exit 2); in v2 it lets the action \
                 through. Your signature covers the bytes, not this parser's reading of them, \
                 so flint will not guess which one you meant. Fix it explicitly, then re-sign \
                 (`flint canon pick`): to keep BLOCKING, set `response: critique` (schema may \
                 stay `flint/v1`); to get the new non-blocking tier, set `schema: flint/v2` and \
                 keep `response: {word}`."
            ),
        }
    }
}

impl std::error::Error for CanonError {}

/// The recognized frontmatter keys. Unknown keys are rejected (fail-closed).
const KNOWN_KEYS: &[&str] = &[
    // identity + carrier
    "schema", "id", "type", "kind", "status",
    // matcher (command / path)
    "glob", "pattern", "exempt", "response", "reversibility", "suggestion",
    // provenance (schema v1)
    "version", "created", "description", "source.kind", "source.ref", "scope", "supersedes", "tags",
    // advisory (kind:advisory)
    "trigger",
    // dormant (forge) — parsed, not load-bearing
    "evidence_tier",
];

/// Parse one Canon markdown rule file (`file` is its path, for error messages).
/// FALLIBLE: any structural / value error -> `Err` (the rule does not bear weight).
pub fn parse_rule(file: &str, text: &str) -> Result<ParsedRule, CanonError> {
    let (fm, body) = split_frontmatter(file, text)?;
    let fields = parse_frontmatter(file, &fm)?;

    let get = |k: &'static str| fields.get(k).map(String::as_str);
    let require = |k: &'static str| get(k).ok_or(CanonError::MissingField { file: file.into(), field: k });

    let id = require("id")?.to_string();
    if !is_kebab(&id) {
        return Err(CanonError::BadValue { file: file.into(), field: "id", value: id });
    }

    // schema tag — required, pins the frontmatter contract. v2 exists only because one
    // word's MEANING changed (see `DriftedResponse`); everything else is identical, so a
    // canon may freely mix v1 and v2 files.
    let schema = require("schema")?.to_string();
    if schema != "flint/v1" && schema != "flint/v2" {
        return Err(CanonError::BadValue { file: file.into(), field: "schema", value: schema });
    }

    // carrier (`type`): rule = a constraint; pit = knowledge (Plan 3, fail-closed).
    match require("type")? {
        "rule" => {}
        "pit" => return Err(CanonError::NotYetSupported { file: file.into(), what: "type: pit (Plan 3)".into() }),
        other => return Err(CanonError::BadValue { file: file.into(), field: "type", value: other.into() }),
    }

    // The body IS the agent-facing text (gate: the critique/deny message; advisory: the
    // guidance prose). Shared by both carriers, so it is read before the kind split.
    let message = body.trim().to_string();
    if message.is_empty() {
        return Err(CanonError::EmptyMessage { file: file.into() });
    }

    let kind = require("kind")?;

    // advisory is NOT a gate — no matcher, no verdict. Handle + return early.
    if kind == "advisory" {
        return parse_advisory(file, id, message, schema, &fields).map(ParsedRule::Advisory);
    }

    // kind: the gate matcher detail. pit is reserved (Plan 3 — fail-closed).
    let matcher = match kind {
        "path" => {
            let glob = require("glob")?.to_string();
            if !glob_is_wellformed(&glob) {
                return Err(CanonError::BadGlob { file: file.into(), glob });
            }
            Matcher::Path { glob }
        }
        "command" => {
            let pattern = require("pattern")?.to_string();
            if !pattern_compiles(&pattern) {
                return Err(CanonError::BadPattern { file: file.into(), field: "pattern" });
            }
            let exempt = match get("exempt") {
                Some(ex) => {
                    if !pattern_compiles(ex) {
                        return Err(CanonError::BadPattern { file: file.into(), field: "exempt" });
                    }
                    Some(ex.to_string())
                }
                None => None,
            };
            Matcher::Command { pattern, exempt }
        }
        "pit" => return Err(CanonError::NotYetSupported { file: file.into(), what: "kind: pit (Plan 3)".into() }),
        other => return Err(CanonError::BadValue { file: file.into(), field: "kind", value: other.into() }),
    };

    // response: the canonical user-facing words, mapped to the internal verdict. One word
    // per tier and no aliases — `warn` meaning "blocks the call" is exactly the mismatch
    // this vocabulary exists to prevent.
    //
    // `warn` is SCHEMA-GATED. It is the one word whose meaning moved (v1: blocks; v2:
    // proceeds), and a signature covers bytes rather than this parser's reading of them —
    // so accepting a v1 `warn` under the new meaning would silently change what an
    // already-signed law does. v1 `warn` therefore fails closed until the sovereign
    // migrates and re-signs. Nothing else drifted: `block` is unchanged, and `critique`
    // was a hard error in v1 so no signed v1 file can contain it.
    let response = match require("response")? {
        "warn" if schema == "flint/v1" => {
            return Err(CanonError::DriftedResponse { file: file.into(), word: "warn".into() })
        }
        "warn" => Response::Warn,
        "critique" => Response::Critique,
        "block" => Response::Deny,
        other => return Err(CanonError::BadValue { file: file.into(), field: "response", value: other.into() }),
    };

    let reversibility = match get("reversibility") {
        None | Some("reversible") => Reversibility::Reversible,
        Some("irreversible") => Reversibility::Irreversible,
        Some(other) => {
            return Err(CanonError::BadValue { file: file.into(), field: "reversibility", value: other.into() })
        }
    };

    let tier = match get("evidence_tier") {
        None => EvidenceTier::Prose,
        Some(s) => EvidenceTier::parse(s)
            .ok_or_else(|| CanonError::BadValue { file: file.into(), field: "evidence_tier", value: s.into() })?,
    };

    let suggestion = get("suggestion").unwrap_or("").to_string();

    let meta = parse_meta(file, schema, &fields)?;

    Ok(ParsedRule::Gate(CanonRule {
        rule: GateRule { id, matcher, response, reversibility, message, suggestion },
        meta,
        tier,
    }))
}

/// Read ONLY a rule's lifecycle [`Status`] from its frontmatter, without a full typed parse.
/// Used at the `reduce` / `pick` boundary (which rules bear weight / get signed) — kept out
/// of [`parse_rule`] so the runtime judge types stay free of lifecycle metadata (the same
/// "NONE of it reaches the judge" discipline as [`RuleMeta`]).
///
/// Fail-safe defaults, fail-CLOSED on a real typo: an ABSENT `status`, or a file whose
/// frontmatter can't even be fenced, defaults to [`Status::Accepted`] (a rule bears weight
/// unless explicitly held back; a structural error is then surfaced by `parse_rule`). But a
/// PRESENT-yet-unrecognized `status: bogus` is a loud `BadValue` — a typo must never silently
/// leave a law enforced-when-you-meant-disabled (or vice-versa).
pub fn rule_status(file: &str, text: &str) -> Result<Status, CanonError> {
    let Ok((fm, _)) = split_frontmatter(file, text) else {
        return Ok(Status::Accepted); // no/broken fence — let parse_rule report the real error.
    };
    for raw in fm.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == "status" {
                let val = strip_quotes(v.trim());
                return Status::parse(&val)
                    .ok_or_else(|| CanonError::BadValue { file: file.into(), field: "status", value: val });
            }
        }
    }
    Ok(Status::Accepted)
}

/// Parse the schema-v1 provenance fields into [`RuleMeta`]. `schema` is already
/// validated by the caller. Value-level lint (kebab id / ISO date / scope
/// allowlist) is layered on in a later task; here we parse + type-check only.
fn parse_meta(file: &str, schema: String, fields: &BTreeMap<String, String>) -> Result<RuleMeta, CanonError> {
    let get = |k: &str| fields.get(k).map(String::as_str);
    let bad = |field: &'static str, value: &str| CanonError::BadValue {
        file: file.into(),
        field,
        value: value.into(),
    };

    let version = match get("version") {
        None => 1,
        Some(s) => match s.parse::<u32>() {
            Ok(v) if v >= 1 => v,
            _ => return Err(bad("version", s)),
        },
    };
    let created = match get("created") {
        None => None,
        Some(s) if is_iso_date(s) => Some(s.to_string()),
        Some(s) => return Err(bad("created", s)),
    };
    let source_kind = match get("source.kind") {
        None => None,
        Some(s) => Some(SourceKind::parse(s).ok_or_else(|| bad("source.kind", s))?),
    };
    let scope = get("scope").map(split_csv).unwrap_or_default();
    for sel in &scope {
        if !is_valid_scope_selector(sel) {
            return Err(bad("scope", sel));
        }
    }
    Ok(RuleMeta {
        schema,
        version,
        created,
        description: get("description").map(String::from),
        source_kind,
        source_ref: get("source.ref").map(String::from),
        scope,
        supersedes: get("supersedes").map(split_csv).unwrap_or_default(),
        tags: get("tags").map(split_csv).unwrap_or_default(),
    })
}

/// Parse a `kind: advisory` entry into an [`AdvisoryRule`]. advisory rules are guidance
/// (no matcher, no verdict), so any gate-only field is a mutex error. `id` / `message` /
/// `schema` are already validated by the caller.
fn parse_advisory(
    file: &str,
    id: String,
    message: String,
    schema: String,
    fields: &BTreeMap<String, String>,
) -> Result<AdvisoryRule, CanonError> {
    let get = |k: &str| fields.get(k).map(String::as_str);
    // advisory carries no gate machinery — a gate-only field present is a mutex error.
    for field in ["pattern", "glob", "exempt", "reversibility", "response", "suggestion"] {
        if get(field).is_some() {
            return Err(CanonError::BadValue {
                file: file.into(),
                field,
                value: "not allowed on kind: advisory".into(),
            });
        }
    }
    // Reuse the provenance parser (validates version/created/source.kind/scope/...).
    let meta = parse_meta(file, schema, fields)?;
    let description = meta
        .description
        .clone()
        .ok_or(CanonError::MissingField { file: file.into(), field: "description" })?;
    let trigger = get("trigger").map(split_csv).unwrap_or_default();
    if trigger.is_empty() {
        return Err(CanonError::MissingField { file: file.into(), field: "trigger" });
    }
    Ok(AdvisoryRule { id, description, message, trigger, scope: meta.scope })
}

/// Rewrite a rule file's frontmatter `status:` to `new`, preserving everything else byte for
/// byte (body, comments, key order, trailing newline). Replaces an existing `status:` line in
/// place; inserts one just before the closing fence if absent. This is the mutation behind
/// `flint law accept / disable / remove` — the only thing that flips a law's lifecycle, always
/// followed by a re-`pick` (a status change bears weight only once re-signed).
pub fn set_status(text: &str, new: Status) -> Result<String, CanonError> {
    // Validate the fence up front so a malformed file fails loudly (never a silent no-op).
    split_frontmatter("<law>", text)?;
    let status_line = format!("status: {}", new.as_str());
    let ends_nl = text.ends_with('\n');
    let mut out: Vec<String> = Vec::new();
    let mut in_fm = false;
    let mut wrote = false;
    for (idx, raw) in text.lines().enumerate() {
        if idx == 0 {
            out.push(raw.to_string()); // opening `---`
            in_fm = true;
            continue;
        }
        if in_fm && raw.trim() == "---" {
            if !wrote {
                out.push(status_line.clone()); // insert before the closing fence
                wrote = true;
            }
            out.push(raw.to_string());
            in_fm = false;
            continue;
        }
        if in_fm && !wrote {
            if let Some((k, _)) = raw.trim().split_once(':') {
                if k.trim() == "status" {
                    out.push(status_line.clone()); // replace in place
                    wrote = true;
                    continue;
                }
            }
        }
        out.push(raw.to_string());
    }
    let mut s = out.join("\n");
    if ends_nl {
        s.push('\n');
    }
    Ok(s)
}

/// Reduce a verified Canon file set into a [`TouchstonePolicy`] (Contract A · the
/// rewrite of the old `project_touchstone_policy`, now over markdown not ledger events).
/// FALLIBLE (§3.6 / codex r3): ANY malformed rule, or a duplicate id, fails the WHOLE
/// Canon -> `Err`. The caller (hook runtime) turns this into a fail-closed deny; `flint
/// canon lint` surfaces it before a pick. NEVER returns a partial policy.
///
/// Only `.md` files are treated as rules; non-`.md` files in the set (e.g. promotion
/// records / fixtures, reframe-and-diff §3.7) are ignored here (typed-role routing,
/// codex sig-iface req #7). Iteration is over the `BTreeMap` (path-sorted) so the policy
/// is a deterministic function of the file set.
pub fn reduce(files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<TouchstonePolicy, CanonError> {
    let mut rules: Vec<GateRule> = Vec::new();
    let mut advisory: Vec<AdvisoryRule> = Vec::new();
    let mut id_origin: BTreeMap<String, String> = BTreeMap::new();
    let mut pending_refs: Vec<(String, String)> = Vec::new(); // (superseded_id, declaring file)
    for (path, bytes) in files {
        if !is_rule_path(path) {
            continue;
        }
        let file = path.to_string_lossy().to_string();
        // Non-UTF8 content is malformed (fail-closed).
        let text = std::str::from_utf8(bytes)
            .map_err(|_| CanonError::BadValue { file: file.clone(), field: "<utf8>", value: "<non-utf8>".into() })?;
        // Lifecycle gate: disabled / removed rules are INERT — skipped before the typed parse
        // (an inactive rule need not be well-formed) and excluded from the id-uniqueness set
        // (so `flint law remove` then re-adding the same id does not collide with its
        // tombstone). accepted + proposed flow through: in the SIGNED set proposed never
        // appears (`pick` excludes it); in the working tree `lint` validates it here.
        if !rule_status(&file, text)?.is_active() {
            continue;
        }
        match parse_rule(&file, text)? {
            ParsedRule::Gate(cr) => {
                if let Some(first) = id_origin.get(&cr.rule.id) {
                    return Err(CanonError::DuplicateId {
                        id: cr.rule.id.clone(),
                        first: first.clone(),
                        second: file,
                    });
                }
                for sup in &cr.meta.supersedes {
                    pending_refs.push((sup.clone(), file.clone()));
                }
                id_origin.insert(cr.rule.id.clone(), file);
                rules.push(cr.rule);
            }
            ParsedRule::Advisory(adv) => {
                // ids are unique across BOTH gates and advisories (one namespace).
                if let Some(first) = id_origin.get(&adv.id) {
                    return Err(CanonError::DuplicateId {
                        id: adv.id.clone(),
                        first: first.clone(),
                        second: file,
                    });
                }
                id_origin.insert(adv.id.clone(), file);
                advisory.push(adv);
            }
        }
    }
    // Every `supersedes` entry must name a rule id defined in this Canon (no dangling
    // relation). Checked after the full id set is known — order-independent.
    for (sup, file) in pending_refs {
        if !id_origin.contains_key(&sup) {
            return Err(CanonError::DanglingRef { file, id: sup });
        }
    }
    Ok(TouchstonePolicy { rules, advisory })
}

/// A Canon rule file = a `.md` under the TOP-LEVEL `rules/` subtree (typed-role routing).
/// Promotion records / fixtures live under other top-level prefixes (`promotion/`,
/// `fixtures/`) and are not parsed as rules. ANCHORED at `rules/` (codex P2: the old
/// `contains("/rules/")` also swept `promotion/rules/x.md` etc. into the rule parser).
/// Paths here are relative to the canon root (the manifest scheme), so `rules/...` is the
/// top-level anchor. Separators are NORMALIZED first: on Windows the relative walk
/// yields `rules\x.md`, which the old `/`-only anchor silently missed — every rule
/// vanished and the gate judged with an EMPTY policy while looking healthy
/// (measured on Windows, 2026-07-11 — worst-case fail-open by omission).
fn is_rule_path(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.ends_with(".md") && s.starts_with("rules/")
}

/// Split `text` into (frontmatter, body). The file MUST start with a `---` line; the
/// frontmatter runs until the next `---` line. Line-based and fail-closed on a
/// missing/unterminated fence. The body is reconstructed `\n`-joined (the caller trims
/// it; exact trailing bytes don't matter for the message).
///
/// `pub(crate)` so the LENIENT pit parser (`pit`) can reuse the exact same fence
/// splitter (a pit with no frontmatter maps the `Err` to "body = whole text") instead
/// of hand-rolling a second one.
pub(crate) fn split_frontmatter(file: &str, text: &str) -> Result<(String, String), CanonError> {
    let mut lines = text.lines();
    match lines.next() {
        Some(l) if l.trim() == "---" => {}
        _ => return Err(CanonError::NoFrontmatter { file: file.into() }),
    }
    let mut fm = String::new();
    let mut body: Vec<&str> = Vec::new();
    let mut closed = false;
    for line in lines {
        if !closed {
            if line.trim() == "---" {
                closed = true;
                continue;
            }
            fm.push_str(line);
            fm.push('\n');
        } else {
            body.push(line);
        }
    }
    if !closed {
        return Err(CanonError::UnterminatedFrontmatter { file: file.into() });
    }
    Ok((fm, body.join("\n")))
}

/// Parse flat `key: value` frontmatter. Blank lines + `#` comments skipped. Unknown keys
/// and duplicate keys are rejected (fail-closed). Values: split on the FIRST `:`, trimmed,
/// surrounding matching quotes stripped literally (no escape processing — regex-safe).
fn parse_frontmatter(file: &str, fm: &str) -> Result<BTreeMap<String, String>, CanonError> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for raw in fm.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            return Err(CanonError::BadValue { file: file.into(), field: "<line>", value: line.into() });
        };
        let key = k.trim().to_string();
        if !KNOWN_KEYS.contains(&key.as_str()) {
            return Err(CanonError::UnknownKey { file: file.into(), key });
        }
        if out.contains_key(&key) {
            return Err(CanonError::DuplicateKey { file: file.into(), key });
        }
        out.insert(key, strip_quotes(v.trim()));
    }
    Ok(out)
}

/// Strip ONE layer of matching surrounding single/double quotes, literally (no escapes).
/// `pub(crate)` — reused by the lenient pit parser.
pub(crate) fn strip_quotes(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Split a comma-separated frontmatter list value into trimmed, non-empty items
/// (schema v1 list fields: `scope`, `tags`, `supersedes`). A colon inside an item
/// (e.g. `agent:claude`) is preserved — this only splits on commas. `pub(crate)` —
/// reused by the lenient pit parser for its `tags`.
pub(crate) fn split_csv(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

/// A kebab-case id: lowercase ASCII / digits / single hyphens, no leading/trailing
/// or doubled hyphen (matches the Agent Skills `name` slug rule).
fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// An ISO `YYYY-MM-DD` date shape (shape only — not calendar-validated).
fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..].iter().all(u8::is_ascii_digit)
}

/// A schema-v1 scope selector from the allowlist: `global`, `agent:claude`,
/// `agent:codex`, `project:<id>`, `os:<id>`.
fn is_valid_scope_selector(s: &str) -> bool {
    s == "global"
        || s == "agent:claude"
        || s == "agent:codex"
        || s.strip_prefix("project:").is_some_and(|r| !r.is_empty())
        || s.strip_prefix("os:").is_some_and(|r| !r.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(items: &[(&str, &str)]) -> BTreeMap<PathBuf, Vec<u8>> {
        items.iter().map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec())).collect()
    }

    /// Parse and require a gate rule (command|path); panics on advisory. Test convenience
    /// so gate tests keep asserting on CanonRule fields after parse_rule became fallible.
    fn parse_gate(file: &str, text: &str) -> CanonRule {
        match parse_rule(file, text).expect("parses") {
            ParsedRule::Gate(cr) => cr,
            ParsedRule::Advisory(_) => panic!("expected a gate rule, got advisory"),
        }
    }

    const SCOPE_RULE: &str = "---\nschema: flint/v1\nid: no-secrets\ntype: rule\nkind: path\nglob: knowledge/secret/**\nresponse: block\nreversibility: irreversible\n---\nDo not write secrets into knowledge/secret. Use the Keychain.\n";

    const CMD_RULE: &str = "---\nschema: flint/v1\nid: lsp-over-grep\ntype: rule\nkind: command\npattern: '\\b(grep|rg|ag)\\b.*\\.(rs|ts)\\b'\nexempt: 'FLINT_LSP_BYPASS=1'\nresponse: critique\nevidence_tier: reproduced\n---\nUse the LSP (goToDefinition / findReferences) for code symbols, not grep.\n";

    #[test]
    fn parses_scope_rule() {
        let r = parse_gate("rules/no-secrets.md", SCOPE_RULE);
        assert_eq!(r.rule.id, "no-secrets");
        assert_eq!(r.rule.matcher, Matcher::Path { glob: "knowledge/secret/**".into() });
        assert_eq!(r.rule.response, Response::Deny);
        assert_eq!(r.rule.reversibility, Reversibility::Irreversible);
        assert_eq!(r.tier, EvidenceTier::Prose); // defaulted
        assert!(r.rule.message.starts_with("Do not write secrets"));
    }

    #[test]
    fn parses_command_rule_with_exempt_and_tier() {
        let r = parse_gate("rules/lsp.md", CMD_RULE);
        assert_eq!(r.rule.id, "lsp-over-grep");
        match r.rule.matcher {
            Matcher::Command { pattern, exempt } => {
                assert_eq!(pattern, r"\b(grep|rg|ag)\b.*\.(rs|ts)\b"); // backslashes preserved (single-quote literal)
                assert_eq!(exempt.as_deref(), Some("FLINT_LSP_BYPASS=1"));
            }
            other => panic!("expected command matcher, got {other:?}"),
        }
        assert_eq!(r.rule.response, Response::Critique);
        assert_eq!(r.rule.reversibility, Reversibility::Reversible); // defaulted
        assert_eq!(r.tier, EvidenceTier::Reproduced);
    }

    #[test]
    fn body_is_the_message_glob_in_value_keeps_colon() {
        // `glob: exec:bash` — value split on FIRST colon keeps `exec:bash`.
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: exec:bash\nresponse: block\n---\nmsg\n";
        let r = parse_gate("rules/x.md", t);
        assert_eq!(r.rule.matcher, Matcher::Path { glob: "exec:bash".into() });
        assert_eq!(r.rule.message, "msg");
    }

    #[test]
    fn no_frontmatter_is_err() {
        assert!(matches!(parse_rule("rules/x.md", "just text\n"), Err(CanonError::NoFrontmatter { .. })));
    }

    #[test]
    fn unterminated_frontmatter_is_err() {
        assert!(matches!(
            parse_rule("rules/x.md", "---\nid: x\ntype: scope\n"),
            Err(CanonError::UnterminatedFrontmatter { .. })
        ));
    }

    #[test]
    fn missing_required_field_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nresponse: block\n---\nmsg\n"; // no glob
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::MissingField { field: "glob", .. })));
    }

    #[test]
    fn bad_glob_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: src/*\nresponse: block\n---\nmsg\n"; // src/* not wellformed
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadGlob { .. })));
    }

    #[test]
    fn bad_pattern_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: command\npattern: '('\nresponse: block\n---\nmsg\n"; // unbalanced regex
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadPattern { field: "pattern", .. })));
    }

    #[test]
    fn unknown_response_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: allow\n---\nmsg\n"; // `allow` is not a response
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "response", .. })));
    }

    #[test]
    fn unknown_key_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\nallow: true\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::UnknownKey { .. })));
    }

    #[test]
    fn empty_body_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\n\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::EmptyMessage { .. })));
    }

    #[test]
    fn reduce_collects_multiple_rules() {
        let f = files(&[("rules/a.md", SCOPE_RULE), ("rules/b.md", CMD_RULE)]);
        let policy = reduce(&f).expect("reduces");
        assert_eq!(policy.rules.len(), 2);
    }

    #[test]
    fn reduce_fails_closed_on_one_malformed() {
        // §3.6 / codex r3: ONE malformed rule fails the WHOLE Canon — never serve the rest.
        let bad = "---\nid: bad\ntype: scope\nglob: src/*\nresponse: deny\n---\nmsg\n"; // bad glob
        let f = files(&[("rules/ok.md", SCOPE_RULE), ("rules/bad.md", bad)]);
        assert!(reduce(&f).is_err(), "a single malformed rule must fail the whole Canon (no partial policy)");
    }

    #[test]
    fn reduce_rejects_duplicate_id() {
        let f = files(&[("rules/a.md", SCOPE_RULE), ("rules/a-dup.md", SCOPE_RULE)]);
        assert!(matches!(reduce(&f), Err(CanonError::DuplicateId { .. })));
    }

    #[test]
    fn reduce_ignores_non_rule_files() {
        // promotion records / fixtures (non rules/ path) are not parsed as rules (typed roles).
        let f = files(&[("rules/a.md", SCOPE_RULE), ("promotion/x.md", "garbage not a rule")]);
        let policy = reduce(&f).expect("non-rule files ignored");
        assert_eq!(policy.rules.len(), 1);
    }

    #[test]
    fn reduce_recognizes_windows_separator_rule_paths() {
        // On Windows the relative walk yields `rules\x.md`; the `/`-only anchor used to
        // silently drop EVERY rule — an empty policy judging while looking healthy
        // (measured on Windows, 2026-07-11). Both separators must route into the rule parser.
        let f = files(&[(r"rules\a.md", SCOPE_RULE)]);
        let policy = reduce(&f).expect("windows-separator rule path parses");
        assert_eq!(policy.rules.len(), 1, "rules\\a.md must be recognized as a rule");
        // …and the promotion exclusion stays anchored under both separators too.
        let f = files(&[(r"promotion\rules\x.md", "garbage not a rule")]);
        let policy = reduce(&f).expect("windows promotion path ignored");
        assert_eq!(policy.rules.len(), 0);
    }

    #[test]
    fn reduce_rejects_non_utf8_rule() {
        let mut f = files(&[("rules/a.md", SCOPE_RULE)]);
        f.insert(PathBuf::from("rules/bad.md"), vec![0xff, 0xfe, 0x00]);
        assert!(reduce(&f).is_err());
    }

    #[test]
    fn comments_and_blank_lines_skipped() {
        let t = "---\n# a comment\nschema: flint/v1\nid: x\n\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nmsg\n";
        assert!(parse_rule("rules/x.md", t).is_ok());
    }

    #[test]
    fn duplicate_key_is_err() {
        let t = "---\nschema: flint/v1\nid: x\nid: y\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::DuplicateKey { .. })));
    }

    #[test]
    fn split_csv_trims_and_drops_empty() {
        // A colon inside an item (scope selector `agent:claude`) survives — only commas split.
        assert_eq!(
            split_csv("global, agent:claude , project:flint"),
            vec!["global".to_string(), "agent:claude".to_string(), "project:flint".to_string()]
        );
        assert!(split_csv("").is_empty());
        assert!(split_csv("   ").is_empty());
        assert!(split_csv(" , ,").is_empty());
        assert_eq!(split_csv("solo"), vec!["solo".to_string()]);
    }

    #[test]
    fn rulemeta_defaults_when_unspecified() {
        // A v1 rule without provenance fields still parses; meta takes defaults.
        let r = parse_gate("rules/lsp.md", CMD_RULE);
        assert_eq!(r.meta.version, 1);
        assert!(r.meta.scope.is_empty());
        assert!(r.meta.tags.is_empty());
        assert_eq!(r.meta.source_kind, None);
        assert_eq!(r.meta.description, None);
    }

    const V1_CMD_FULL: &str = "---\nschema: flint/v1\nid: lsp-over-grep\ntype: rule\nkind: command\nversion: 2\ncreated: 2026-07-03\ndescription: Route source-code navigation to LSP, not grep.\nsource.kind: human\nsource.ref: notes/standing-orders/005-lsp-over-grep\nscope: global, agent:claude, agent:codex\ntags: navigation, iron-law\npattern: '\\bgrep\\b'\nresponse: critique\n---\nUse LSP for code symbols.\n";

    #[test]
    fn parses_v1_provenance_fields() {
        let r = parse_gate("rules/lsp.md", V1_CMD_FULL);
        assert_eq!(r.meta.schema, "flint/v1");
        assert_eq!(r.meta.version, 2);
        assert_eq!(r.meta.created.as_deref(), Some("2026-07-03"));
        assert_eq!(r.meta.description.as_deref(), Some("Route source-code navigation to LSP, not grep."));
        assert_eq!(r.meta.source_kind, Some(SourceKind::Human));
        assert_eq!(r.meta.source_ref.as_deref(), Some("notes/standing-orders/005-lsp-over-grep"));
        assert_eq!(r.meta.scope, vec!["global", "agent:claude", "agent:codex"]);
        assert_eq!(r.meta.tags, vec!["navigation", "iron-law"]);
        // This fixture models the REAL lsp-over-grep rule, which blocks — so it must say
        // `critique`. The non-blocking tier is covered by its own fixtures.
        assert_eq!(r.rule.response, Response::Critique);
    }

    #[test]
    fn missing_schema_is_err() {
        let t = "---\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::MissingField { field: "schema", .. })));
    }

    #[test]
    fn wrong_schema_is_err() {
        // v1 and v2 are the accepted contracts; anything else is a loud error (never a
        // lenient "newer must be fine" — an unknown schema is an unreadable law).
        let mk = |schema: &str| {
            format!("---\nschema: {schema}\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nmsg\n")
        };
        assert!(parse_rule("rules/x.md", &mk("flint/v1")).is_ok());
        assert!(parse_rule("rules/x.md", &mk("flint/v2")).is_ok());
        for bad in ["flint/v3", "flint/v0", "flint", "v1", ""] {
            assert!(
                matches!(parse_rule("rules/x.md", &mk(bad)), Err(CanonError::BadValue { field: "schema", .. })),
                "schema `{bad}` must be rejected"
            );
        }
    }

    #[test]
    fn advisory_rejects_gate_field() {
        // advisory carries no gate machinery — a `response` (gate-only) field is a mutex error.
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: advisory\ndescription: d\ntrigger: t\nresponse: block\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "response", .. })));
    }

    #[test]
    fn type_pit_is_not_yet_supported() {
        let t = "---\nschema: flint/v1\nid: x\ntype: pit\nkind: pit\nresponse: block\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::NotYetSupported { .. })));
    }

    #[test]
    fn the_three_response_words_map_to_three_distinct_tiers() {
        let mk = |resp: &str| {
            format!("---\nschema: flint/v2\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: {resp}\n---\nmsg\n")
        };
        assert_eq!(parse_gate("rules/x.md", &mk("block")).rule.response, Response::Deny);
        // `critique` is the blocking-with-a-recovery-path tier — the word `warn` used to
        // mean this, which made every rule labelled `warn` a lie (it exited 2).
        assert_eq!(parse_gate("rules/x.md", &mk("critique")).rule.response, Response::Critique);
        // `warn` now means what it says: the action proceeds.
        assert_eq!(parse_gate("rules/x.md", &mk("warn")).rule.response, Response::Warn);
    }

    #[test]
    fn v1_warn_is_rejected_because_its_meaning_changed_under_a_signature() {
        // THE MIGRATION GUARD. A signature authenticates BYTES, not parser semantics: the
        // same signed v1 file that used to exit 2 would now let the action through, with
        // the sovereign having re-signed nothing. Fail CLOSED and loud instead — a gate
        // that silently stops gating is the one failure this project exists to prevent.
        let v1_warn = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: warn\n---\nmsg\n";
        let err = parse_rule("rules/x.md", v1_warn).expect_err("v1 + warn must not parse");
        assert!(matches!(err, CanonError::DriftedResponse { .. }), "got {err:?}");
        // The message must be followable without reading the source.
        let text = err.to_string();
        assert!(text.contains("flint/v2"), "names the migration target: {text}");
        assert!(text.contains("critique"), "names the word that preserves blocking: {text}");
        assert!(text.contains("re-sign") || text.contains("resign"), "says to re-sign: {text}");
    }

    #[test]
    fn only_the_drifted_combination_is_rejected() {
        // Surgical: `warn` is the ONLY word whose meaning moved. Everything else a signed
        // v1 canon can legally contain still parses, so the 17 advisories and the
        // `block`-tier rules need no migration at all.
        let v1_block = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nmsg\n";
        assert_eq!(parse_gate("rules/x.md", v1_block).rule.response, Response::Deny);
        // `critique` was a hard error in v1, so NO signed v1 file can contain it — there is
        // nothing to drift, and accepting it costs nothing.
        let v1_crit = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: critique\n---\nmsg\n";
        assert_eq!(parse_gate("rules/x.md", v1_crit).rule.response, Response::Critique);
        // A v1 advisory has no `response` at all — no drift, no migration.
        let v1_adv = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: advisory\ndescription: d\ntrigger: t\n---\nmsg\n";
        assert!(parse_rule("rules/x.md", v1_adv).is_ok());
    }

    #[test]
    fn unknown_response_word_is_a_loud_error() {
        // Fail-closed on a typo: a misspelled tier must never silently pick a weaker one.
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: warnn\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "response", .. })));
    }

    #[test]
    fn bad_source_kind_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\nsource.kind: robot\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "source.kind", .. })));
    }

    #[test]
    fn bad_version_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\nversion: two\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "version", .. })));
    }

    #[test]
    fn non_kebab_id_is_err() {
        let t = "---\nschema: flint/v1\nid: Bad_Id\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "id", .. })));
    }

    #[test]
    fn bad_scope_selector_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\nscope: global, agent:gemini\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "scope", .. })));
    }

    #[test]
    fn valid_scope_selectors_parse() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\nscope: global, agent:claude, agent:codex, project:flint, os:macos\n---\nmsg\n";
        let r = parse_gate("rules/x.md", t);
        assert_eq!(r.meta.scope, vec!["global", "agent:claude", "agent:codex", "project:flint", "os:macos"]);
    }

    #[test]
    fn bad_created_date_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\ncreated: 2026-7-3\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "created", .. })));
    }

    #[test]
    fn version_zero_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: path\nglob: a/**\nresponse: block\nversion: 0\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "version", .. })));
    }

    #[test]
    fn reduce_accepts_valid_supersedes() {
        let old_rule = "---\nschema: flint/v1\nid: old-rule\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\nold\n";
        let new_rule = "---\nschema: flint/v1\nid: new-rule\ntype: rule\nkind: path\nglob: b/**\nresponse: block\nsupersedes: old-rule\n---\nnew\n";
        let f = files(&[("rules/old.md", old_rule), ("rules/new.md", new_rule)]);
        assert_eq!(reduce(&f).expect("reduces").rules.len(), 2);
    }

    #[test]
    fn reduce_rejects_dangling_supersedes() {
        let r = "---\nschema: flint/v1\nid: new-rule\ntype: rule\nkind: path\nglob: b/**\nresponse: block\nsupersedes: ghost\n---\nnew\n";
        let f = files(&[("rules/new.md", r)]);
        assert!(matches!(reduce(&f), Err(CanonError::DanglingRef { .. })));
    }

    const ADVISORY_RULE: &str = "---\nschema: flint/v1\nid: verify-before-claiming\ntype: rule\nkind: advisory\nversion: 1\ncreated: 2026-07-03\ndescription: Verify external state before asserting it.\nsource.kind: human\nscope: global, agent:claude, agent:codex\ntrigger: before-reporting-status, before-claiming-tests-pass\n---\nDo not assert a fact from memory when a cheap check is available.\n";

    #[test]
    fn parses_advisory_rule() {
        match parse_rule("rules/verify.md", ADVISORY_RULE).expect("parses") {
            ParsedRule::Advisory(a) => {
                assert_eq!(a.id, "verify-before-claiming");
                assert_eq!(a.description, "Verify external state before asserting it.");
                assert_eq!(a.trigger, vec!["before-reporting-status", "before-claiming-tests-pass"]);
                assert_eq!(a.scope, vec!["global", "agent:claude", "agent:codex"]);
                assert!(a.message.starts_with("Do not assert"));
            }
            ParsedRule::Gate(_) => panic!("expected advisory, got gate"),
        }
    }

    #[test]
    fn advisory_missing_trigger_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: advisory\ndescription: d\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::MissingField { field: "trigger", .. })));
    }

    #[test]
    fn advisory_missing_description_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: advisory\ntrigger: t\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::MissingField { field: "description", .. })));
    }

    #[test]
    fn advisory_with_glob_is_err() {
        let t = "---\nschema: flint/v1\nid: x\ntype: rule\nkind: advisory\ndescription: d\ntrigger: t\nglob: a/**\n---\nmsg\n";
        assert!(matches!(parse_rule("rules/x.md", t), Err(CanonError::BadValue { field: "glob", .. })));
    }

    #[test]
    fn reduce_collects_advisory_separately() {
        let f = files(&[("rules/gate.md", SCOPE_RULE), ("rules/adv.md", ADVISORY_RULE)]);
        let policy = reduce(&f).expect("reduces");
        assert_eq!(policy.rules.len(), 1);
        assert_eq!(policy.advisory.len(), 1);
        assert_eq!(policy.advisory[0].id, "verify-before-claiming");
    }

    #[test]
    fn duplicate_id_across_gate_and_advisory_is_err() {
        let gate = "---\nschema: flint/v1\nid: dup\ntype: rule\nkind: path\nglob: a/**\nresponse: block\n---\ng\n";
        let adv = "---\nschema: flint/v1\nid: dup\ntype: rule\nkind: advisory\ndescription: d\ntrigger: t\n---\na\n";
        let f = files(&[("rules/g.md", gate), ("rules/a.md", adv)]);
        assert!(matches!(reduce(&f), Err(CanonError::DuplicateId { .. })));
    }

    // ---- lifecycle status (self-contained spec §2 sovereignty model) ----

    fn with_status(id: &str, st: &str) -> String {
        format!("---\nschema: flint/v1\nid: {id}\ntype: rule\nkind: path\nglob: a/**\nresponse: block\nstatus: {st}\n---\nmsg\n")
    }

    #[test]
    fn rule_status_reads_all_values_and_defaults_accepted() {
        assert_eq!(rule_status("f", &with_status("x", "proposed")).unwrap(), Status::Proposed);
        assert_eq!(rule_status("f", &with_status("x", "accepted")).unwrap(), Status::Accepted);
        assert_eq!(rule_status("f", &with_status("x", "disabled")).unwrap(), Status::Disabled);
        assert_eq!(rule_status("f", &with_status("x", "removed")).unwrap(), Status::Removed);
        // absent → accepted (a rule bears weight by default — omission never disables it).
        assert_eq!(rule_status("f", SCOPE_RULE).unwrap(), Status::Accepted);
        // present-but-bogus → loud error (a typo must not silently flip enforcement).
        assert!(matches!(
            rule_status("f", &with_status("x", "bogus")),
            Err(CanonError::BadValue { field: "status", .. })
        ));
    }

    #[test]
    fn status_signable_and_active_semantics() {
        assert!(!Status::Proposed.is_signable() && Status::Proposed.is_active());
        assert!(Status::Accepted.is_signable() && Status::Accepted.is_active());
        assert!(Status::Disabled.is_signable() && !Status::Disabled.is_active());
        assert!(Status::Removed.is_signable() && !Status::Removed.is_active());
    }

    #[test]
    fn reduce_skips_disabled_and_removed() {
        let f = files(&[
            ("rules/active.md", &with_status("active", "accepted")),
            ("rules/off.md", &with_status("off", "disabled")),
            ("rules/gone.md", &with_status("gone", "removed")),
        ]);
        let p = reduce(&f).expect("reduces");
        assert_eq!(p.rules.len(), 1);
        assert_eq!(p.rules[0].id, "active");
    }

    #[test]
    fn reduce_includes_proposed_in_working_tree() {
        // `lint` reduces the WORKING tree: proposed laws are validated + counted here; they
        // are excluded from the SIGNED set only at pick time.
        let f = files(&[("rules/p.md", &with_status("p", "proposed")), ("rules/a.md", &with_status("a", "accepted"))]);
        assert_eq!(reduce(&f).expect("reduces").rules.len(), 2);
    }

    #[test]
    fn reduce_allows_reusing_a_removed_id() {
        // a `removed` tombstone with id X must not collide with a fresh accepted rule id X
        // (the tombstone is skipped BEFORE the id-uniqueness check).
        let tomb = with_status("dup", "removed");
        let fresh = "---\nschema: flint/v1\nid: dup\ntype: rule\nkind: path\nglob: b/**\nresponse: block\nstatus: accepted\n---\nnew\n";
        let f = files(&[("rules/old.md", &tomb), ("rules/new.md", fresh)]);
        let p = reduce(&f).expect("tombstone id is reusable");
        assert_eq!(p.rules.len(), 1);
        assert_eq!(p.rules[0].id, "dup");
    }

    #[test]
    fn set_status_replaces_existing_and_preserves_the_rest() {
        let before = with_status("x", "proposed");
        let after = set_status(&before, Status::Accepted).unwrap();
        assert_eq!(rule_status("f", &after).unwrap(), Status::Accepted);
        assert!(!after.contains("proposed"));
        assert!(after.contains("id: x") && after.contains("glob: a/**") && after.ends_with("msg\n"));
        assert!(parse_rule("rules/x.md", &after).is_ok()); // still a valid rule
    }

    #[test]
    fn set_status_inserts_when_absent() {
        // SCOPE_RULE has no status line (accepted by default); disabling inserts one.
        let after = set_status(SCOPE_RULE, Status::Disabled).unwrap();
        assert_eq!(rule_status("f", &after).unwrap(), Status::Disabled);
        assert!(after.contains("Do not write secrets")); // body preserved
        assert!(parse_rule("rules/x.md", &after).is_ok());
    }

    #[test]
    fn set_status_on_no_frontmatter_errs() {
        assert!(matches!(set_status("no fence here\n", Status::Accepted), Err(CanonError::NoFrontmatter { .. })));
    }

    #[test]
    fn inactive_rule_body_need_not_be_wellformed() {
        // an inactive rule is inert — skipped before parse_rule, so a malformed one bears no
        // weight and does not fail the whole Canon.
        let junk_disabled = "---\nstatus: disabled\nnot even a valid rule line\n---\nwhatever\n";
        let f = files(&[("rules/x.md", junk_disabled), ("rules/ok.md", SCOPE_RULE)]);
        assert_eq!(reduce(&f).expect("disabled skipped before parse").rules.len(), 1);
    }
}
