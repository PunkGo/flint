//! Flint runtime config (PIVOT reframe · config-over-code).
//!
//! `flint.toml` carries the entrypoint-boundary-protected runtime wiring: where the
//! signed Canon lives, the pinned sovereign trust (root of trust, §3.5.1), the flint
//! binary path (for `flint compile`'s emitted hook configs), and the obs-log path. It is
//! installed WITH the harness (NOT read from agent-writable Canon) — its protection is
//! the entrypoint-boundary tier (§3.5: OS-protected strong / discipline weak).
//!
//! The OLD kernel config (Backend{Embedded,Daemon}, ledger state_dir) is gone — there is
//! no kernel.

use std::path::PathBuf;

use serde::Deserialize;

use crate::trust::SovereignTrust;

/// The pinned sovereign trust block (root of trust, §3.5.1).
#[derive(Debug, Clone, Deserialize)]
pub struct TrustConfig {
    /// OpenSSH `allowed_signers` file pinning the sovereign key(s).
    pub allowed_signers: PathBuf,
    /// The signer principal/identity (a principal in `allowed_signers`).
    pub signer_identity: String,
    /// This instance's namespace (anti cross-instance replay; must equal manifest.scope).
    pub scope: String,
    /// Anti-rollback floor. Strong tier advances this in agent-unwritable durable state;
    /// weak tier leaves it static (§3.5: rollback below this is rejected, above is §12.6).
    #[serde(default)]
    pub min_epoch: u64,
    /// Optional PERSISTENT anti-rollback floor file (a durable high-water epoch). When set,
    /// `canon pick` advances it to the freshly-signed epoch, and the hook floors the accepted
    /// epoch at `max(min_epoch, this)` — so a `git checkout` of an OLD but validly-signed
    /// manifest is rejected even when `min_epoch` was never hand-bumped (the common case,
    /// since it defaults to 0). WEAK TIER (§3.5): a same-UID agent can edit this file just as
    /// it can checkout an old manifest, so at that tier it is defense-in-depth (raising the
    /// cost of a silent rollback), not a hard boundary — real protection needs the file on
    /// agent-unwritable durable state (a different UID / a read-only mount). Its always-on
    /// value: the rollback floor advances automatically instead of relying on a manual bump.
    #[serde(default)]
    pub epoch_floor: Option<PathBuf>,
}

/// The energy-budget block (P3 · §5 P3). Cumulative REAL tokens fed into the judgment gate.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BudgetConfig {
    /// The accumulator sidecar a Stop/PostToolUse hook writes via `flint budget record`.
    /// `None` disables energy entirely.
    #[serde(default)]
    pub sidecar: Option<PathBuf>,
    /// Cumulative-token threshold at/over which an otherwise-Affirm action is raised to a
    /// recoverable Critique (veto-only soft pressure). `0` (default) disables the fold.
    #[serde(default)]
    pub critique_threshold: u64,
}

/// Memory block (self-contained spec §4). Opt-in bring-your-own store: a vault directory
/// (an existing Obsidian/wiki/notes folder, or a flint scaffold). DEFAULT OFF — the gate is
/// the product; memory is the `flint init --with-memory` add-on.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MemoryConfig {
    /// The vault directory. `None` disables the `flint memory` commands (config-over-code:
    /// the store location is configuration, never a hardcoded default).
    #[serde(default)]
    pub vault: Option<PathBuf>,
}

/// One raw-ore store (粗矿厂, knowledge-layer spec skeleton §5): a markdown folder holding an
/// `inbox.md` and notes, where captured gists land. `flint init` scaffolds a DEFAULT local
/// store; a user ATTACHES external stores (an Obsidian/wiki folder) later. Auto-capture writes
/// to the ONE `active` store (no per-capture routing tax); `flint knowledge review` reads the
/// UNION of every store's inbox. This unifies the legacy `pits_root` and `[memory]` vault into
/// one 1:N abstraction.
#[derive(Debug, Clone, Deserialize)]
pub struct OreStoreConfig {
    /// The store directory (an existing Obsidian/wiki vault, or a flint scaffold).
    pub path: PathBuf,
    /// The auto-capture write target. Exactly one store is active; if none is flagged the
    /// FIRST listed wins (stores-but-no-active is not an error — a lenient default, not a gate).
    #[serde(default)]
    pub active: bool,
    /// A short label for listings. Defaults to the directory basename.
    #[serde(default)]
    pub label: Option<String>,
}

/// A resolved ore store: path + display label + whether it is the active write target.
/// Produced by [`FlintConfig::ore_stores`] from EITHER explicit `[[ore_store]]` entries OR the
/// legacy `pits_root`/`memory.vault` fallback — so callers never branch on the config shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOreStore {
    pub path: PathBuf,
    pub label: String,
    pub active: bool,
}

/// Capture defaults (knowledge-layer spec skeleton §3, L1). flint's OWN self-shipped governance
/// — how the agent feeds the capture loop — NOT the owner's judgment (that is the signed Canon).
/// It is opt-OUT: `auto_mine = true` by default ships the capture nudge into agent context; set
/// it false and not one word of it reaches the agent. flint governs only its OWN workflow here,
/// it never adjudicates the owner's actions — that stays opt-in Canon (the sovereignty line, and
/// the reason opt-out is safe: a default you can always turn off).
#[derive(Debug, Clone, Deserialize)]
pub struct CaptureConfig {
    /// Ship flint's capture nudge (mark walls) into agent context. Opt-out; default true.
    #[serde(default = "default_true")]
    pub auto_mine: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self { auto_mine: true }
    }
}

fn default_true() -> bool {
    true
}

/// Cross-vendor judge block (P4 · §3.7 L2 / §8.4 #5). The veto-only second opinion on a
/// Forge promotion. DEFAULT OFF — which model + when to consult is the owner's call.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct JudgeConfig {
    /// Enable the cross-vendor (codex) veto on the Forge promotion cold path.
    #[serde(default)]
    pub cross_vendor: bool,
    /// Optional model override for the cross-vendor judge.
    #[serde(default)]
    pub model: Option<String>,
}

/// Top-level Flint runtime config.
#[derive(Debug, Clone, Deserialize)]
pub struct FlintConfig {
    /// The installed flint binary path (for `flint compile`'s emitted hook commands).
    pub flint_bin: String,
    /// The signed Canon root (contains `CANON.manifest` + `.sig` + `rules/`).
    pub canon_root: PathBuf,
    /// The workspace root the relative scope globs are written against — the directory the
    /// harness runs the PreToolUse hook in. A harness reports file paths ABSOLUTELY
    /// (`/abs/.../knowledge/secret/key`, the normal Claude case) or with `..`; the gate
    /// resolves them against this root before matching the relative globs (codex P1 r2). When
    /// unset it defaults to the hook process's current working directory (which the harness
    /// sets to the project root). Pin it explicitly to be independent of cwd.
    #[serde(default)]
    pub workspace_root: Option<PathBuf>,
    /// Append-only observation log (redacted receipts; the Forge write-path feed, P2).
    /// `None` disables recording.
    #[serde(default)]
    pub obs_log: Option<PathBuf>,
    /// The pit knowledge store (Plan 3). A directory holding `inbox.md` (hot marks) and
    /// `<id>.md` cold-store notes. Pits are NOT signed / judged / injected — knowledge,
    /// not rules. `None` disables the `flint pit` commands (config-over-code: the store
    /// location is configuration, never hardcoded).
    #[serde(default)]
    pub pits_root: Option<PathBuf>,
    /// Energy budget (P3). Defaults to disabled (no sidecar / zero threshold).
    #[serde(default)]
    pub budget: BudgetConfig,
    /// Cross-vendor judge (P4). Defaults to disabled.
    #[serde(default)]
    pub judge: JudgeConfig,
    /// Bring-your-own memory (spec §4). Defaults to disabled (no vault).
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Capture defaults (spec skeleton §3, L1 auto-mine). Defaults to on (opt-out).
    #[serde(default)]
    pub capture: CaptureConfig,
    /// Raw-ore stores (knowledge-layer spec skeleton §5). When empty, synthesized for
    /// backward-compat from the legacy single `pits_root` (active) + `memory.vault` (source),
    /// so a pre-ore_store config keeps working unchanged. See [`FlintConfig::ore_stores`].
    #[serde(default)]
    pub ore_store: Vec<OreStoreConfig>,
    /// The knowledge store (精矿, spec skeleton §7): where PROMOTED notes are written — a bare
    /// markdown folder (git-md, take-away; `flint rm` → notes survive). `None` disables
    /// `flint knowledge promote/list` (config-over-code, parity with `pits_root`). `flint init`
    /// sets `<home>/knowledge`. DISTINCT from the raw-ore stores: ore is captured, 精矿 is what
    /// the owner PROMOTED (人裁) out of it.
    #[serde(default)]
    pub knowledge_root: Option<PathBuf>,
    pub trust: TrustConfig,
}

impl FlintConfig {
    /// Load + parse `flint.toml`. Fail-fast on a malformed config (an operator
    /// misconfiguration is a loud error, not a silent default).
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read flint config {}: {e}", path.display()))?;
        let cfg: FlintConfig =
            toml::from_str(&text).map_err(|e| anyhow::anyhow!("parse flint config {}: {e}", path.display()))?;
        cfg.validate()
            .map_err(|e| anyhow::anyhow!("invalid flint config {}: {e}", path.display()))?;
        Ok(cfg)
    }

    /// Schema invariants that parsing alone cannot express. Checked at load so a bad config is
    /// loud on EVERY verb (config-over-code: validate fail-fast at startup), never a surprise
    /// from whichever command happens to trip over it first.
    fn validate(&self) -> anyhow::Result<()> {
        // `active` decides where every `flint pit mark` writes. Two flagged stores used to
        // resolve to the first silently — the same silent first-match shape as the store-label
        // P1, on a more dangerous field: capture would land in one store while `knowledge
        // review` triaged from the other. The doc invariant is "exactly one store is active".
        let active: Vec<String> = self
            .ore_store
            .iter()
            .filter(|s| s.active)
            .map(|s| s.label.clone().unwrap_or_else(|| store_label(&s.path)))
            .collect();
        if active.len() > 1 {
            anyhow::bail!(
                "[[ore_store]] flags {} stores `active = true` ({}) — exactly one may be active \
                 (it is the single capture target `flint pit mark` writes to)",
                active.len(),
                active.join(", ")
            );
        }
        Ok(())
    }

    /// The [`SovereignTrust`] this config pins.
    pub fn sovereign_trust(&self) -> SovereignTrust {
        SovereignTrust {
            allowed_signers: self.trust.allowed_signers.clone(),
            signer_identity: self.trust.signer_identity.clone(),
            expected_scope: self.trust.scope.clone(),
            min_epoch: self.trust.min_epoch,
        }
    }

    /// The resolved raw-ore stores (spec skeleton §5) — the READ set for `knowledge review`,
    /// with exactly one marked `active` (the auto-capture write target) whenever any store
    /// exists. Resolution prefers explicit `[[ore_store]]` entries (active = the flagged one,
    /// else the first listed); with none, it falls back for backward-compat to the legacy
    /// `pits_root` (active) plus `memory.vault` (an additional read source). Empty when neither
    /// is configured — the `knowledge` / `pit` / `memory` commands are then disabled, mirroring
    /// the existing per-store `None`-disables-the-command rule.
    pub fn ore_stores(&self) -> Vec<ResolvedOreStore> {
        if !self.ore_store.is_empty() {
            // Explicit model wins; legacy pits_root/vault are ignored (one model, no merge).
            let active_idx = self.ore_store.iter().position(|s| s.active).unwrap_or(0);
            return self
                .ore_store
                .iter()
                .enumerate()
                .map(|(i, s)| ResolvedOreStore {
                    path: s.path.clone(),
                    label: s.label.clone().unwrap_or_else(|| store_label(&s.path)),
                    active: i == active_idx,
                })
                .collect();
        }
        // Backward-compat: the legacy single pits_root (active write target) + memory.vault
        // (an additional read source). A memory-only config keeps the vault as its write target.
        let mut out = Vec::new();
        if let Some(p) = &self.pits_root {
            out.push(ResolvedOreStore { path: p.clone(), label: store_label(p), active: true });
        }
        if let Some(v) = &self.memory.vault {
            let active = out.is_empty();
            out.push(ResolvedOreStore { path: v.clone(), label: store_label(v), active });
        }
        out
    }

    /// The active ore store (the auto-capture write target), or `None` if none is configured.
    pub fn active_ore_store(&self) -> Option<ResolvedOreStore> {
        self.ore_stores().into_iter().find(|s| s.active)
    }
}

/// A default display label for an ore store: its directory basename, or the whole path when it
/// has none (a bare root). Keeps listings readable without forcing an explicit `label`.
fn store_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let toml = r#"
flint_bin = "/usr/local/bin/flint"
canon_root = "/home/u/.flint/canon"
obs_log = "/home/u/.flint/obs.jsonl"
[trust]
allowed_signers = "/home/u/.flint/allowed_signers"
signer_identity = "flint-sovereign"
scope = "my-instance"
min_epoch = 3
"#;
        let cfg: FlintConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.flint_bin, "/usr/local/bin/flint");
        assert_eq!(cfg.canon_root, PathBuf::from("/home/u/.flint/canon"));
        assert_eq!(cfg.obs_log, Some(PathBuf::from("/home/u/.flint/obs.jsonl")));
        let t = cfg.sovereign_trust();
        assert_eq!(t.signer_identity, "flint-sovereign");
        assert_eq!(t.expected_scope, "my-instance");
        assert_eq!(t.min_epoch, 3);
    }

    /// A parseable config with `extra` appended (the store block under test).
    fn write_cfg(dir: &std::path::Path, extra: &str) -> PathBuf {
        let p = dir.join("flint.toml");
        std::fs::write(
            &p,
            format!(
                "flint_bin = \"flint\"\ncanon_root = \"canon\"\n{extra}\n\n[trust]\nallowed_signers = \"as\"\nsigner_identity = \"s\"\nscope = \"i\"\n"
            ),
        )
        .unwrap();
        p
    }

    #[test]
    fn load_refuses_two_active_ore_stores() {
        // `active` decides where EVERY `flint pit mark` writes. Two flagged stores used to
        // resolve to the first silently (`position(..).unwrap_or(0)`) — the same silent
        // first-match shape as the store-label P1, on the more dangerous field. The doc
        // invariant is "exactly one store is active", so >1 is a config error, caught at
        // load so every verb reports it, not just the one that happens to capture.
        let dir = tempfile::tempdir().unwrap();
        let p = write_cfg(
            dir.path(),
            "[[ore_store]]\npath = \"/a\"\nlabel = \"alpha\"\nactive = true\n\n\
             [[ore_store]]\npath = \"/b\"\nlabel = \"beta\"\nactive = true\n",
        );
        let err = FlintConfig::load(&p).expect_err("two active stores must be refused");
        let msg = err.to_string();
        assert!(msg.contains("active"), "error names the offending field: {msg}");
        assert!(msg.contains("alpha") && msg.contains("beta"), "error names BOTH stores: {msg}");
    }

    #[test]
    fn load_keeps_no_active_store_lenient() {
        // The documented default: stores-but-no-active is NOT an error, the first listed wins.
        // Guards the fix above from over-reaching into a gate.
        let dir = tempfile::tempdir().unwrap();
        let p = write_cfg(
            dir.path(),
            "[[ore_store]]\npath = \"/a\"\nlabel = \"alpha\"\n\n[[ore_store]]\npath = \"/b\"\nlabel = \"beta\"\n",
        );
        let cfg = FlintConfig::load(&p).expect("no-active is a lenient default, not an error");
        assert_eq!(cfg.active_ore_store().unwrap().label, "alpha");
    }

    #[test]
    fn min_epoch_and_obs_log_default() {
        let toml = r#"
flint_bin = "flint"
canon_root = "canon"
[trust]
allowed_signers = "as"
signer_identity = "s"
scope = "i"
"#;
        let cfg: FlintConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.trust.min_epoch, 0);
        assert_eq!(cfg.obs_log, None);
    }

    #[test]
    fn malformed_config_is_err() {
        assert!(toml::from_str::<FlintConfig>("not valid toml {{{").is_err());
        // missing required trust block
        assert!(toml::from_str::<FlintConfig>("flint_bin=\"x\"\ncanon_root=\"y\"\n").is_err());
    }

    // --- ore-store resolution (knowledge-layer spec skeleton §5) ---

    /// A minimal valid config with the given body appended (trust block + required fields).
    fn cfg(extra: &str) -> FlintConfig {
        let base = "flint_bin=\"flint\"\ncanon_root=\"canon\"\n";
        let trust = "[trust]\nallowed_signers=\"as\"\nsigner_identity=\"s\"\nscope=\"i\"\n";
        toml::from_str(&format!("{base}{extra}\n{trust}")).unwrap()
    }

    #[test]
    fn ore_store_explicit_active_flagged_wins() {
        let c = cfg(
            "[[ore_store]]\npath=\"/a\"\n\n[[ore_store]]\npath=\"/b\"\nactive=true\nlabel=\"obsi\"\n",
        );
        let stores = c.ore_stores();
        assert_eq!(stores.len(), 2);
        assert_eq!(stores[0].path, PathBuf::from("/a"));
        assert!(!stores[0].active);
        assert_eq!(stores[0].label, "a"); // basename default
        assert!(stores[1].active);
        assert_eq!(stores[1].label, "obsi"); // explicit label respected
        assert_eq!(c.active_ore_store().unwrap().path, PathBuf::from("/b"));
    }

    #[test]
    fn ore_store_explicit_none_active_first_wins() {
        let c = cfg("[[ore_store]]\npath=\"/a\"\n\n[[ore_store]]\npath=\"/b\"\n");
        let stores = c.ore_stores();
        assert!(stores[0].active, "first store is the default write target");
        assert!(!stores[1].active);
    }

    #[test]
    fn ore_store_explicit_two_active_only_first_is_active() {
        // `ore_stores()` is the PURE resolver and stays total — given an already-parsed config it
        // always yields exactly one active store. Two-active is rejected one layer up, at
        // `FlintConfig::load` (see `load_refuses_two_active_ore_stores`), because that is the
        // seam every production caller goes through. This test pins the resolver's totality, NOT
        // a claim that a two-active config is acceptable.
        let c = cfg("[[ore_store]]\npath=\"/a\"\nactive=true\n\n[[ore_store]]\npath=\"/b\"\nactive=true\n");
        let stores = c.ore_stores();
        assert!(stores[0].active);
        assert!(!stores[1].active);
        assert_eq!(stores.iter().filter(|s| s.active).count(), 1);
    }

    #[test]
    fn ore_store_backcompat_pits_root_only() {
        // The LIVE dogfood shape: pits_root, no [[ore_store]], no vault → one active store.
        let c = cfg("pits_root=\"/home/u/.flint/pits\"");
        let stores = c.ore_stores();
        assert_eq!(stores.len(), 1);
        assert_eq!(stores[0].path, PathBuf::from("/home/u/.flint/pits"));
        assert!(stores[0].active);
        assert_eq!(stores[0].label, "pits");
    }

    #[test]
    fn ore_store_backcompat_pits_and_vault() {
        // A fresh `init --with-memory` shape: scaffolded pits (active write) + attached vault
        // (read source) — the 1:N from day one.
        let c = cfg("pits_root=\"/h/pits\"\n[memory]\nvault=\"/h/obsidian\"");
        let stores = c.ore_stores();
        assert_eq!(stores.len(), 2);
        assert_eq!(stores[0].path, PathBuf::from("/h/pits"));
        assert!(stores[0].active, "pits is the write target");
        assert_eq!(stores[1].path, PathBuf::from("/h/obsidian"));
        assert!(!stores[1].active, "the vault is a read source, not the write target");
    }

    #[test]
    fn ore_store_backcompat_vault_only_is_active() {
        // A memory-only config (no pits_root) still has a write target: the vault.
        let c = cfg("[memory]\nvault=\"/h/obsidian\"");
        let stores = c.ore_stores();
        assert_eq!(stores.len(), 1);
        assert!(stores[0].active);
    }

    #[test]
    fn ore_store_none_configured_is_empty() {
        let c = cfg("");
        assert!(c.ore_stores().is_empty());
        assert!(c.active_ore_store().is_none());
    }

    #[test]
    fn ore_store_explicit_takes_precedence_over_legacy() {
        // With explicit [[ore_store]], the legacy pits_root/vault are IGNORED (one model wins).
        let c = cfg("pits_root=\"/legacy\"\n[[ore_store]]\npath=\"/new\"\nactive=true\n");
        let stores = c.ore_stores();
        assert_eq!(stores.len(), 1);
        assert_eq!(stores[0].path, PathBuf::from("/new"));
    }

    #[test]
    fn store_label_defaults_to_basename() {
        assert_eq!(store_label(std::path::Path::new("/a/b/pits")), "pits");
        assert_eq!(store_label(std::path::Path::new("vault")), "vault");
    }

    #[test]
    fn capture_auto_mine_defaults_on_opt_out() {
        // No [capture] block → the default is ON (opt-out: you turn it off, not on).
        let c = cfg("");
        assert!(c.capture.auto_mine, "auto_mine defaults on");
        // [capture] with no auto_mine key → still on (the field default).
        let c = cfg("[capture]");
        assert!(c.capture.auto_mine);
    }

    #[test]
    fn capture_auto_mine_explicit_false_turns_off() {
        let c = cfg("[capture]\nauto_mine = false");
        assert!(!c.capture.auto_mine, "explicit false disables it");
        let c = cfg("[capture]\nauto_mine = true");
        assert!(c.capture.auto_mine);
    }
}
