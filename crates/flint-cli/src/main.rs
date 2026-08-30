//! flint — carry the fire forward.
//!
//! A small, harness-agnostic sovereign judgment loop for AI agents. Your boundaries +
//! judgment, injected at the agent's runtime, owned and portable. (PIVOT: the old
//! embedded-kernel ledger is gone — rules live as a SIGNED markdown Canon, enforced via
//! commodity hooks.)

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use flint_core::canon;
use flint_core::config::FlintConfig;
use flint_core::forge;
use flint_core::harness::Harness;
use flint_core::striker::{self, CompileParams};
use flint_core::trust;

mod capture;
mod cross_vendor;
mod codex_hook;
mod fleet;
mod hook;
mod init;
mod install;
mod knowledge;

#[derive(Parser)]
#[command(name = "flint", version = concat!(env!("CARGO_PKG_VERSION"), env!("FLINT_VERSION_SUFFIX")), about = "Carry the fire forward — a sovereign judgment loop for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run as a harness PreToolUse hook: read a hook JSON on stdin, judge it against the
    /// signed Canon, record a redacted receipt, and (in --mode block) enforce the verdict.
    Hook {
        /// claude | codex | grok — selects the input adapter AND the verdict envelope.
        #[arg(long)]
        harness: String,
        /// Path to flint.toml (trust + canon_root). Pinned with the harness install.
        #[arg(long)]
        config: PathBuf,
        /// record (default, never blocks) | block (enforce warn / critique / deny).
        #[arg(long, default_value = "record")]
        mode: String,
        /// closed (default, deny on gate error) | open.
        #[arg(long = "fail-mode", default_value = "closed")]
        fail_mode: String,
        /// Print the active signed policy and exit (no stdin read).
        #[arg(long)]
        show: bool,
    },
    /// Compile the per-harness hook wiring (Contract B). Prints the config fragment to
    /// stdout for you to install. For codex, also prints the AGENTS.md scope advisory.
    Compile {
        #[arg(long)]
        harness: String,
        #[arg(long)]
        config: PathBuf,
        /// block (default — the installed gate ENFORCES the verdict) | record (advisory,
        /// dogfood-first). The bare `flint hook` CLI defaults to record; the COMPILED
        /// wiring defaults to block so a fresh install enforces (codex P1).
        #[arg(long, default_value = "block")]
        mode: String,
        /// If set, WRITE the advisory files into this repo dir (claude:
        /// `.claude/rules/flint-advisory.md`; codex: the marked block in `AGENTS.md`)
        /// instead of only printing. Hook wiring is still printed (installed once).
        #[arg(long = "target-dir")]
        target_dir: Option<PathBuf>,
    },
    /// Canon management (the markdown rule source).
    Canon {
        #[command(subcommand)]
        cmd: CanonCmd,
    },
    /// Law lifecycle (self-contained spec §2): the sovereign accepts / disables / removes
    /// individual laws. A default law is PROPOSED at `flint init` (written, unsigned); `accept`
    /// signs it with your key = the first real pick (propose≠pick). `disable` / `remove` turn
    /// an accepted law off / tombstone it. Every mutation re-signs the Canon.
    Law {
        #[command(subcommand)]
        cmd: LawCmd,
    },
    /// Forge: the load-bearing gate (run a rule's discrimination fixture to earn the
    /// `reproduced` evidence tier; 入库 != 承重, reframe-and-diff §3.7).
    Forge {
        #[command(subcommand)]
        cmd: ForgeCmd,
    },
    /// Energy budget (P3): accumulate REAL measured tokens into the sidecar the hook reads.
    /// SOURCE (corrected 2026-06-28 vs jack.db empirics): the universal source is the
    /// TRANSCRIPT (`transcript_path`, the harness's own file given to every hook) — it
    /// carries per-turn tokens and IS parseable in practice (punkgo-jack scans it at scale).
    /// Wire a Stop/PostToolUse hook to read the new turn's tokens from the transcript tail
    /// and call `flint budget record`. OTel / Agent SDK / `claude -p --json .usage` also
    /// work; an existing jack.db is an optional shortcut. The PreToolUse hook itself carries
    /// no token field — but the transcript it points to does. flint never fabricates tokens.
    Budget {
        #[command(subcommand)]
        cmd: BudgetCmd,
    },
    /// Pit: the knowledge store (Plan 3). Mark a wall in the moment (hot); save a gist
    /// into a full note (cold). Pits are knowledge — never signed / judged / injected.
    /// To ENFORCE what a pit teaches, write a rule and `canon pick` (no auto-promotion).
    Pit {
        #[command(subcommand)]
        cmd: PitCmd,
    },
    /// Install the flint suite from a manifest (skills, generated advisory, marked
    /// blocks) — Striker's "carry it over" action for the whole suite. Idempotent
    /// diff-writes, installed.lock for honest removal, targets confined to
    /// ~/.claude, ~/.codex, ~/.flint. The judge/hook path never runs through here.
    Install {
        /// Path to the suite manifest. Defaults to scripts/manifest.toml (run from
        /// the flint repo root, as scripts/bootstrap.* does).
        #[arg(long, default_value = "scripts/manifest.toml")]
        manifest: PathBuf,
        /// skills (workflow skills only, default) | full (also harness bindings —
        /// generator/block entries rendered from the signed canon).
        #[arg(long, default_value = "skills")]
        stage: String,
        /// Path to flint.toml — required when generator entries are in scope (they
        /// render from the signed canon).
        #[arg(long)]
        config: Option<PathBuf>,
        /// Report drift (normalized textual diff + pending removals), write nothing.
        /// Exit 1 when out of sync.
        #[arg(long)]
        check: bool,
        /// Dry-run: print would-write / would-remove / would-skip, write nothing.
        #[arg(long)]
        plan: bool,
        /// SessionStart auto-sync mode: skip (exit 0, one-line warning) when the
        /// flint repo is dirty or not on main; any failure warns instead of blocking.
        #[arg(long)]
        quiet: bool,
        /// Git HEAD approved by the explicit install that rendered this SessionStart hook.
        /// Only valid with --quiet.
        #[arg(long)]
        expected_repo_head: Option<String>,
    },
    /// Bring-your-own memory (spec §4): capture / read the OPT-IN vault (an Obsidian/wiki
    /// folder or a flint scaffold). Memory is knowledge — never signed / judged / injected.
    /// The vault path is `[memory] vault` in flint.toml (`flint init --with-memory`).
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// Knowledge (升维, knowledge-layer spec skeleton §7): review the raw-ore gists captured
    /// across your stores, then PROMOTE the ones worth keeping into durable 精矿 notes (人裁 —
    /// never auto). Promoting is the owner's decision; a promoted note is bare markdown you own
    /// (`knowledge_root` in flint.toml). Knowledge is never signed / judged / injected.
    Knowledge {
        #[command(subcommand)]
        cmd: KnowledgeCmd,
    },
    /// Bootstrap a fresh flint home (spec §2): generate the sovereign signing key, write the
    /// default law pack (8 Iron Laws + lsp-over-grep-sweep) as PROPOSED (unsigned), and write
    /// flint.toml. Nothing bears weight until you `flint law accept` — init proposes, it never
    /// decides for you.
    Init {
        /// The flint home (default ~/.flint).
        #[arg(long)]
        home: Option<PathBuf>,
        /// The instance namespace (manifest scope — anti cross-instance replay).
        #[arg(long, default_value = "local")]
        scope: String,
        /// The flint binary path recorded in the config (default: this executable).
        #[arg(long = "flint-bin")]
        flint_bin: Option<String>,
        /// Also wire an opt-in memory vault (spec §4).
        #[arg(long = "with-memory")]
        with_memory: bool,
        /// The memory vault path (with --with-memory; default <home>/memory).
        #[arg(long)]
        vault: Option<PathBuf>,
    },
    /// Sovereign key custody (spec §2): back up / restore the signing key. The private key
    /// never leaves the machine except via an explicit `export` you run.
    Key {
        #[command(subcommand)]
        cmd: KeyCmd,
    },
    /// Fleet keyring (spec §8): manage the cross-machine trust set. Add another machine's
    /// PUBLIC key and a Canon it signs verifies here too (sign once, the whole fleet enforces);
    /// remove it to revoke. Private keys never move — only public keys join the set.
    Fleet {
        #[command(subcommand)]
        cmd: FleetCmd,
    },
}

#[derive(Subcommand)]
enum FleetCmd {
    /// Trust another machine's public key (its Canon signatures then verify here).
    Add {
        #[arg(long)]
        config: PathBuf,
        /// The other machine's public key file (its `<home>/keys/sovereign_ed25519.pub`).
        #[arg(long)]
        pubkey: PathBuf,
        /// A human label for the machine (defaults to the key's own comment).
        #[arg(long)]
        label: Option<String>,
    },
    /// List the fleet trust set.
    List {
        #[arg(long)]
        config: PathBuf,
    },
    /// Revoke a machine — remove its key by `--label` or `--pubkey`.
    Remove {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        pubkey: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Back up the sovereign key (+ its `.pub`) into a directory. Refuses to export an
    /// already-exposed key; copies are written 0600.
    Export {
        #[arg(long)]
        key: PathBuf,
        #[arg(long = "to")]
        to: PathBuf,
    },
    /// Restore a sovereign key from a backup to a fresh key path (0600; refuses to clobber).
    Import {
        #[arg(long = "from")]
        from: PathBuf,
        #[arg(long = "to")]
        to: PathBuf,
    },
}

#[derive(Subcommand)]
enum MemoryCmd {
    /// Create a fresh generic vault (inbox + orientation stub) at the configured vault path.
    /// Idempotent — never clobbers an existing vault.
    Scaffold {
        #[arg(long)]
        config: PathBuf,
    },
    /// Append a one-line hot-capture gist to the vault inbox.
    Capture {
        #[arg(long)]
        config: PathBuf,
        /// The one-line gist (what you learned).
        gist: String,
    },
    /// List the pending inbox gists.
    List {
        #[arg(long)]
        config: PathBuf,
    },
    /// Print the vault orientation doc (PORTAL / README), if any.
    Orient {
        #[arg(long)]
        config: PathBuf,
    },
    /// Resolve a `source.ref` (vault-relative) to its content, or report absence.
    Resolve {
        #[arg(long)]
        config: PathBuf,
        #[arg(long = "ref")]
        reference: String,
    },
}

#[derive(Subcommand)]
enum PitCmd {
    /// Append a one-line gist to the inbox — the HOT mark, called the moment you hit a
    /// wall (non-interrupting; write it from what's in front of you, no transcript scrape).
    Mark {
        #[arg(long)]
        config: PathBuf,
        /// The one-line gist (what bit you).
        gist: String,
    },
    /// List pending inbox gists + saved pit notes.
    List {
        #[arg(long)]
        config: PathBuf,
    },
    /// Scaffold a cold-store pit draft `<id>.md` (you then expand the body). `--desc` a
    /// one-liner, `--seed` a gist to prime the body. Does NOT consume the inbox — you read
    /// it and decide what is worth keeping.
    Save {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        desc: Option<String>,
        #[arg(long)]
        seed: Option<String>,
    },
}

#[derive(Subcommand)]
enum KnowledgeCmd {
    /// Review pending raw-ore gists across ALL configured stores (the §5 union) — read-only.
    /// Each gist shows its store label + index, for `promote` / `toss`.
    Review {
        #[arg(long)]
        config: PathBuf,
    },
    /// Promote a raw gist into a durable 精矿 note (人裁 — you decide it bears keeping). Reads
    /// gist `--index` from store `--from` as the seed, writes `<id>.md` to the knowledge store,
    /// and resolves that gist out of the inbox. `--title` defaults to the id; `--body` overrides
    /// the note body (else the gist seeds it). Or promote free text with just `--body` (no
    /// `--from`/`--index`).
    Promote {
        #[arg(long)]
        config: PathBuf,
        /// The note id / filename slug (`<id>.md`).
        #[arg(long)]
        id: String,
        /// The ore store to pull the gist from (its label from `knowledge review`).
        #[arg(long = "from")]
        from: Option<String>,
        /// Select the gist by its EXACT text (stable — survives inbox shifts; preferred).
        /// Mutually exclusive with `--index`: two selectors that disagree must never be
        /// silently reconciled into one of them.
        #[arg(long, conflicts_with = "index")]
        gist: Option<String>,
        /// Select the gist by position within the store (from `knowledge review`; positional —
        /// re-review first, as indices shift as gists are resolved). Prefer `--gist`.
        #[arg(long)]
        index: Option<usize>,
        /// The note title (default: the id).
        #[arg(long)]
        title: Option<String>,
        /// The note body (default: the pulled gist text).
        #[arg(long)]
        body: Option<String>,
    },
    /// Toss a raw gist — resolve it out of the inbox WITHOUT promoting (not worth keeping).
    Toss {
        #[arg(long)]
        config: PathBuf,
        #[arg(long = "from")]
        from: String,
        /// Select by EXACT gist text (stable; preferred). Mutually exclusive with `--index`.
        #[arg(long, conflicts_with = "index")]
        gist: Option<String>,
        /// Select by position (positional — re-review first). Prefer `--gist`.
        #[arg(long)]
        index: Option<usize>,
    },
    /// List the promoted 精矿 notes in the knowledge store.
    List {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Subcommand)]
enum BudgetCmd {
    /// Add measured tokens to the running total (call from your token source).
    Record {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        tokens: u64,
    },
    /// Print the cumulative tokens.
    Show {
        #[arg(long)]
        config: PathBuf,
    },
    /// Reset the accumulator to 0 (call from a SessionStart hook).
    Reset {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Subcommand)]
enum ForgeCmd {
    /// Run the load-bearing gate for one rule against its `fixtures/<id>.md` discrimination
    /// fixture. On success writes `promotion/<id>.md` (run `canon pick` to sign it in).
    Promote {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        rule: String,
    },
    /// Verifier-ceiling report: for each working-tree rule, does it have a discrimination
    /// fixture, and does it discriminate? (How many prose rules admit an executable
    /// falsifier — reframe-and-diff §7, the riskiest unknown.)
    Ceiling {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Subcommand)]
enum LawCmd {
    /// List the working-tree laws with their lifecycle status (proposed / accepted / disabled
    /// / removed) — what bears weight and what is only proposed.
    List {
        #[arg(long)]
        config: PathBuf,
    },
    /// Print one law's full markdown (find by its `id`, not filename).
    Show {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        name: String,
    },
    /// Accept a proposed law — sign it with your sovereign key (the first real pick). With
    /// `--all`, accept every currently-proposed law in one signed pick.
    Accept {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        key: PathBuf,
        /// The law id to accept. Omit with `--all`.
        #[arg(long)]
        name: Option<String>,
        /// Accept every proposed law at once.
        #[arg(long)]
        all: bool,
    },
    /// Disable an accepted law (turn it off; it stays signed as an auditable record), re-sign.
    Disable {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        name: String,
    },
    /// Remove a law — record a signed tombstone (an auditable deletion, never a silent
    /// vanish), re-sign.
    Remove {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum CanonCmd {
    /// Lint the WORKING-TREE rules (unsigned) — the pre-pick gate. Catches malformed rules
    /// BEFORE they can be signed into a manifest (reframe-and-diff §3.6 lint-gate). Exits
    /// non-zero on any error.
    Lint {
        #[arg(long)]
        config: PathBuf,
    },
    /// List the active SIGNED policy (verifies the manifest first).
    List {
        #[arg(long)]
        config: PathBuf,
    },
    /// Pick: lint the working-tree rules, build a fresh signed manifest (epoch+1), and sign
    /// it with the sovereign key. This is the ONE sovereign write that makes rules
    /// load-bearing (§3.5: signature = the root of trust).
    Pick {
        #[arg(long)]
        config: PathBuf,
        /// The sovereign SSH signing key (private). Strong tier: a hardware-backed key.
        #[arg(long)]
        key: PathBuf,
        /// Manifest epoch. Defaults to (current signed epoch + 1), or 1 if none.
        #[arg(long)]
        epoch: Option<u64>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Hook { harness, config, mode, fail_mode, show } => run_hook_cmd(&harness, &config, &mode, &fail_mode, show),
        Commands::Compile { harness, config, mode, target_dir } => {
            run_compile(&harness, &config, &mode, target_dir.as_deref())
        }
        Commands::Canon { cmd } => match cmd {
            CanonCmd::Lint { config } => run_lint(&config),
            CanonCmd::List { config } => run_list(&config),
            CanonCmd::Pick { config, key, epoch } => run_pick(&config, &key, epoch),
        },
        Commands::Law { cmd } => match cmd {
            LawCmd::List { config } => run_law_list(&config),
            LawCmd::Show { config, name } => run_law_show(&config, &name),
            LawCmd::Accept { config, key, name, all } => run_law_accept(&config, &key, name.as_deref(), all),
            LawCmd::Disable { config, key, name } => run_law_transition(&config, &key, &name, canon::Status::Disabled, "disable"),
            LawCmd::Remove { config, key, name } => run_law_transition(&config, &key, &name, canon::Status::Removed, "remove"),
        },
        Commands::Forge { cmd } => match cmd {
            ForgeCmd::Promote { config, rule } => run_forge_promote(&config, &rule),
            ForgeCmd::Ceiling { config } => run_forge_ceiling(&config),
        },
        Commands::Budget { cmd } => match cmd {
            BudgetCmd::Record { config, tokens } => run_budget(&config, Some(tokens), false),
            BudgetCmd::Show { config } => run_budget(&config, None, false),
            BudgetCmd::Reset { config } => run_budget(&config, None, true),
        },
        Commands::Pit { cmd } => match cmd {
            PitCmd::Mark { config, gist } => run_pit_mark(&config, &gist),
            PitCmd::List { config } => run_pit_list(&config),
            PitCmd::Save { config, id, desc, seed } => {
                run_pit_save(&config, &id, desc.as_deref(), seed.as_deref())
            }
        },
        Commands::Install {
            manifest,
            stage,
            config,
            check,
            plan,
            quiet,
            expected_repo_head,
        } => {
            let stage = match stage.as_str() {
                "skills" => install::Stage::Skills,
                "full" => install::Stage::Full,
                other => anyhow::bail!("unknown --stage `{other}` (skills | full)"),
            };
            let code = install::run_install(&install::InstallArgs {
                manifest,
                stage,
                config,
                check,
                plan,
                quiet,
                expected_repo_head,
            })?;
            std::process::exit(code);
        }
        Commands::Memory { cmd } => match cmd {
            MemoryCmd::Scaffold { config } => run_memory_scaffold(&config),
            MemoryCmd::Capture { config, gist } => run_memory_capture(&config, &gist),
            MemoryCmd::List { config } => run_memory_list(&config),
            MemoryCmd::Orient { config } => run_memory_orient(&config),
            MemoryCmd::Resolve { config, reference } => run_memory_resolve(&config, &reference),
        },
        Commands::Knowledge { cmd } => match cmd {
            KnowledgeCmd::Review { config } => run_knowledge_review(&config),
            KnowledgeCmd::Promote { config, id, from, gist, index, title, body } => run_knowledge_promote(
                &config,
                &id,
                from.as_deref(),
                index,
                gist.as_deref(),
                title.as_deref(),
                body.as_deref(),
            ),
            KnowledgeCmd::Toss { config, from, gist, index } => {
                run_knowledge_toss(&config, &from, index, gist.as_deref())
            }
            KnowledgeCmd::List { config } => run_knowledge_list(&config),
        },
        Commands::Init { home, scope, flint_bin, with_memory, vault } => {
            let home = match home {
                Some(h) => h,
                None => {
                    let base = std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .ok_or_else(|| anyhow::anyhow!("cannot resolve home (HOME/USERPROFILE unset) — pass --home"))?;
                    PathBuf::from(base).join(".flint")
                }
            };
            init::run_init(&init::InitArgs { home, scope, flint_bin, with_memory, vault })
        }
        Commands::Key { cmd } => match cmd {
            KeyCmd::Export { key, to } => init::export_key(&key, &to),
            KeyCmd::Import { from, to } => init::import_key(&from, &to),
        },
        Commands::Fleet { cmd } => match cmd {
            FleetCmd::Add { config, pubkey, label } => {
                fleet::add(&FlintConfig::load(&config)?, &pubkey, label.as_deref())
            }
            FleetCmd::List { config } => fleet::list(&FlintConfig::load(&config)?),
            FleetCmd::Remove { config, label, pubkey } => {
                fleet::remove(&FlintConfig::load(&config)?, label.as_deref(), pubkey.as_deref())
            }
        },
    }
}

/// The configured memory vault, or a loud error (config-over-code: the store is opt-in via
/// `[memory] vault`, never a hardcoded path — parity with `pits_root`).
fn memory_vault(cfg: &FlintConfig) -> Result<flint_core::memory::FsVault> {
    let root = cfg
        .memory
        .vault
        .clone()
        .ok_or_else(|| anyhow::anyhow!("no `[memory] vault` configured — set `[memory] vault` in flint.toml, then `flint memory scaffold`"))?;
    Ok(flint_core::memory::FsVault::new(root))
}

fn run_memory_scaffold(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let vault = memory_vault(&cfg)?;
    flint_core::memory::scaffold_vault(vault.root())?;
    println!("memory: scaffolded vault at {}", vault.root().display());
    Ok(())
}

fn run_memory_capture(config: &Path, gist: &str) -> Result<()> {
    use flint_core::memory::MemoryPort;
    let cfg = FlintConfig::load(config)?;
    let n = memory_vault(&cfg)?.capture(gist)?;
    println!("captured. inbox now has {n} pending gist(s) — `flint memory list` to review.");
    Ok(())
}

fn run_memory_list(config: &Path) -> Result<()> {
    use flint_core::memory::MemoryPort;
    let cfg = FlintConfig::load(config)?;
    let gists = memory_vault(&cfg)?.list_inbox()?;
    println!("INBOX\t{} pending gist(s)", gists.len());
    for g in &gists {
        println!("  - {g}");
    }
    Ok(())
}

fn run_memory_orient(config: &Path) -> Result<()> {
    use flint_core::memory::MemoryPort;
    let cfg = FlintConfig::load(config)?;
    match memory_vault(&cfg)?.read_orientation()? {
        Some(text) => print!("{text}"),
        None => println!("memory: no orientation doc (PORTAL.md / README.md) in the vault yet"),
    }
    Ok(())
}

fn run_memory_resolve(config: &Path, reference: &str) -> Result<()> {
    use flint_core::memory::MemoryPort;
    let cfg = FlintConfig::load(config)?;
    match memory_vault(&cfg)?.resolve_source(reference)? {
        Some(text) => print!("{text}"),
        None => {
            eprintln!("memory: no vault file for source.ref `{reference}`");
            std::process::exit(1);
        }
    }
    Ok(())
}

// --- knowledge (升维, spec skeleton §7): review raw ore → promote 精矿 (人裁, never auto) ---

/// The configured knowledge store (精矿, §7), or a loud error (config-over-code: `knowledge_root`
/// is opt-in, never a hardcoded path — parity with `pits_root` / the memory vault).
fn knowledge_store(cfg: &FlintConfig) -> Result<flint_core::memory::FsVault> {
    let root = cfg.knowledge_root.clone().ok_or_else(|| {
        anyhow::anyhow!("no `knowledge_root` configured in flint.toml (set it, then `flint knowledge promote`)")
    })?;
    Ok(flint_core::memory::FsVault::new(root))
}

/// Current UNIX seconds for the promote date stamp — the wall clock, never fabricated.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve `--from <label>` + a selector to the store's [`FsVault`] and the EXACT gist text.
/// Loud on ambiguity or a miss — a mis-selection must never silently act on the wrong gist:
///   - an ambiguous `label` (two stores share it) is an error, never a silent first-match (codex P1);
///   - `--gist <text>` selects by EXACT text — stable across inbox shifts, so a stale index can
///     never silently promote/toss the wrong gist (codex P1). `--index <n>` stays for quick use.
fn resolve_selection(
    cfg: &FlintConfig,
    label: &str,
    index: Option<usize>,
    gist: Option<&str>,
) -> Result<(flint_core::memory::FsVault, String)> {
    use flint_core::memory::MemoryPort;
    let stores = cfg.ore_stores();
    let matches: Vec<_> = stores.iter().filter(|s| s.label == label).collect();
    let store = match matches.as_slice() {
        [] => anyhow::bail!("no ore store labeled `{label}` — see `flint knowledge review`"),
        [s] => *s,
        _ => anyhow::bail!(
            "ambiguous store label `{label}` ({} stores share it) — give each a unique `label` in [[ore_store]]",
            matches.len()
        ),
    };
    let vault = flint_core::memory::FsVault::new(store.path.clone());
    let gists = vault.list_inbox()?;
    let text = match (gist, index) {
        // Contradictory selectors. `clap` refuses this at the CLI boundary (`conflicts_with`);
        // this arm keeps the function itself total, so a future caller cannot reintroduce the
        // silent `(Some(g), _)` reconciliation that ignored `--index` without a word.
        (Some(_), Some(i)) => {
            anyhow::bail!("--gist and --index select the same gist two different ways — pass one, not both (got --index {i})")
        }
        (Some(g), None) => {
            let hits: Vec<&String> = gists.iter().filter(|x| x.as_str() == g).collect();
            match hits.as_slice() {
                [] => anyhow::bail!("no pending gist `{g}` in `{label}` — `flint knowledge review` for the current list"),
                [t] => (*t).clone(),
                _ => anyhow::bail!("`{g}` matches {} gists in `{label}` — ambiguous; resolve one first", hits.len()),
            }
        }
        (None, Some(i)) => gists
            .get(i)
            .ok_or_else(|| anyhow::anyhow!("store `{label}` has no gist at index {i} (it has {})", gists.len()))?
            .clone(),
        (None, None) => {
            anyhow::bail!("select a gist with `--gist \"<text>\"` (stable) or `--index <n>` (positional — re-review first)")
        }
    };
    Ok((vault, text))
}

fn run_knowledge_review(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let stores = cfg.ore_stores();
    if stores.is_empty() {
        println!("no ore stores configured — set `pits_root` / `[memory] vault` / `[[ore_store]]` in flint.toml");
        return Ok(());
    }
    let pending = knowledge::pending_across(&stores);
    let total = knowledge::total_pending(&pending);
    println!("REVIEW\t{total} pending gist(s) across {} store(s)", pending.len());
    for sp in &pending {
        let tag = if sp.active { " (active)" } else { "" };
        match &sp.gists {
            Ok(gs) => {
                println!("  {}{}\t{} gist(s)", sp.label, tag, gs.len());
                for (i, g) in gs.iter().enumerate() {
                    println!("    [{i}] {g}");
                }
            }
            Err(e) => println!("  {}{}\t<unreadable: {e}>", sp.label, tag),
        }
    }
    if total > 0 {
        println!("\npromote: flint knowledge promote --config <c> --from <label> --gist \"<exact text>\" --id <slug>");
    }
    Ok(())
}

fn run_knowledge_promote(
    config: &Path,
    id: &str,
    from: Option<&str>,
    index: Option<usize>,
    gist: Option<&str>,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<()> {
    use crate::knowledge::{format_note, iso_date};
    use flint_core::memory::MemoryPort;
    let cfg = FlintConfig::load(config)?;
    let ks = knowledge_store(&cfg)?;

    // Seed + source + (optional) the inbox gist to resolve out AFTER a successful write.
    let (seed_body, source, resolve): (String, String, Option<(flint_core::memory::FsVault, String)>) =
        match from {
            Some(label) => {
                let (vault, g) = resolve_selection(&cfg, label, index, gist)?;
                let seed = body.map(str::to_string).unwrap_or_else(|| g.clone());
                // Record the store AND the exact gist. The gist is the note's provenance: with
                // `--body` the note text is rewritten, so without it the durable note keeps no
                // link at all to the raw ore it resolved out. An index would not do — it shifts.
                // (Gists are whitespace-collapsed to one line at capture, so this stays one line.)
                (seed, format!("{label} · gist: {g}"), Some((vault, g)))
            }
            None => {
                let b = body.ok_or_else(|| {
                    anyhow::anyhow!("promote needs `--from <label>` with `--gist \"<text>\"` / `--index <n>` (a pending gist), or `--body <text>` (free text)")
                })?;
                (b.to_string(), "manual".to_string(), None)
            }
        };

    let title = title.unwrap_or(id);
    let note = format_note(title, &seed_body, &source, &iso_date(now_secs()));
    let path = ks.write_durable(id, &note)?;
    println!("promoted → {}", path.display());

    // Echo the exact gist acted on (never silent, codex P1), then resolve it out. Best-effort:
    // the note is already durable, so a resolve failure warns but never fails the promote.
    if let Some((vault, g)) = resolve {
        println!("  gist: {g}");
        match vault.resolve_inbox(&g) {
            Ok(true) => println!("  resolved it out of `{}`'s inbox", from.unwrap_or("?")),
            Ok(false) => println!("  note: gist not found in inbox (already triaged?)"),
            Err(e) => eprintln!("  warning: promoted, but could not resolve the gist out: {e}"),
        }
    }
    Ok(())
}

fn run_knowledge_toss(config: &Path, from: &str, index: Option<usize>, gist: Option<&str>) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let (vault, g) = resolve_selection(&cfg, from, index, gist)?;
    if vault.resolve_inbox(&g)? {
        println!("tossed from `{from}`: {g}");
    } else {
        println!("nothing tossed — gist not found (already triaged?)");
    }
    Ok(())
}

fn run_knowledge_list(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let ks = knowledge_store(&cfg)?;
    let root = ks.root();
    let mut ids: Vec<String> = Vec::new();
    match std::fs::read_dir(root) {
        Ok(rd) => {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|x| x.to_str()) == Some("md") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        // Only promoted notes — skip a scaffold's own inbox / README.
                        if stem != "inbox" && stem != "README" {
                            ids.push(stem.to_string());
                        }
                    }
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {} // no notes promoted yet
        Err(e) => return Err(anyhow::anyhow!("read knowledge store {}: {e}", root.display())),
    }
    ids.sort();
    println!("KNOWLEDGE\t{} note(s) @ {}", ids.len(), root.display());
    for id in &ids {
        println!("  - {id}");
    }
    Ok(())
}

/// The ACTIVE raw-ore store (§5) — the unified capture target AND the `flint pit` verbs' store.
/// Resolves via `ore_stores()`, so a legacy `pits_root` config AND an explicit `[[ore_store]]`
/// config both work (config-over-code — never a hardcoded path). This is what `flint pit mark`
/// writes to, and what the L1 capture nudge tells the agent to feed (codex P1: `pit mark` must
/// follow the active store, not the legacy `pits_root` alone).
fn active_ore_root(cfg: &FlintConfig) -> Result<flint_core::config::ResolvedOreStore> {
    cfg.active_ore_store().ok_or_else(|| {
        anyhow::anyhow!("no ore store configured — set `pits_root` / `[memory] vault` / `[[ore_store]]` in flint.toml")
    })
}

fn run_pit_mark(config: &Path, gist: &str) -> Result<()> {
    use flint_core::memory::MemoryPort;
    if gist.trim().is_empty() {
        anyhow::bail!("empty gist — a mark is a one-line note of what bit you");
    }
    let cfg = FlintConfig::load(config)?;
    let store = active_ore_root(&cfg)?;
    // Capture into the ACTIVE ore store's inbox (same inbox.md format as `flint memory capture`);
    // a legacy pits_root-only config resolves to the same place.
    let n = flint_core::memory::FsVault::new(store.path.clone()).capture(gist)?;
    println!("marked. `{}` inbox now has {n} pending gist(s) — `flint knowledge review` to triage.", store.label);
    Ok(())
}

fn run_pit_list(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let root = active_ore_root(&cfg)?.path;
    let inbox_path = root.join("inbox.md");
    // Distinguish "no inbox yet" (fine → 0) from "exists but unreadable" — the latter must
    // be surfaced, never silently reported as 0 pending while real marks are hidden by an
    // IO/UTF-8 error (codex WP1 P2).
    let inbox = match std::fs::read_to_string(&inbox_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => anyhow::bail!("inbox {} is unreadable: {e}", inbox_path.display()),
    };
    let pending = flint_core::pit::count_inbox(&inbox);
    println!("INBOX\t{pending} pending gist(s)");
    for l in inbox.lines() {
        let t = l.trim_start();
        if t.starts_with("- ") && t.trim().len() > 2 {
            println!("  {t}");
        }
    }
    let mut saved: Vec<flint_core::pit::Pit> = Vec::new();
    if root.exists() {
        for entry in std::fs::read_dir(&root).with_context(|| format!("read_dir {}", root.display()))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "md") {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem == "inbox" {
                    continue;
                }
                // Surface an unreadable pit file instead of parsing it as a blank note —
                // a knowledge store must not silently hide/corrupt owner notes (codex WP1 P2).
                match std::fs::read_to_string(&path) {
                    Ok(text) => saved.push(flint_core::pit::parse_pit(stem, &text)),
                    Err(e) => eprintln!("warning: pit {} unreadable: {e}", path.display()),
                }
            }
        }
    }
    saved.sort_by(|a, b| a.id.cmp(&b.id));
    println!("SAVED\t{} pit note(s)", saved.len());
    for p in &saved {
        println!("PIT\t{}\t{}", p.id, p.description);
    }
    Ok(())
}

fn run_pit_save(config: &Path, id: &str, desc: Option<&str>, seed: Option<&str>) -> Result<()> {
    if !flint_core::pit::is_safe_id(id) {
        // A pit id is a filename SLUG, not a path — saying only "no separators" reads as
        // "flint can't handle my OS's paths" (measured on Windows, 2026-07-11). Name the model and
        // suggest the slug; the owner re-runs with it (never applied automatically).
        let hint = flint_core::pit::suggest_id(id)
            .map(|s| format!(" — did you mean `{s}`?"))
            .unwrap_or_default();
        anyhow::bail!(
            "a pit id is a filename slug, not a path: ascii letters / digits / `-` / `_` only (e.g. `r2-key-expiry`). got `{id}`{hint}"
        );
    }
    let cfg = FlintConfig::load(config)?;
    let root = active_ore_root(&cfg)?.path;
    std::fs::create_dir_all(&root).with_context(|| format!("create ore store {}", root.display()))?;
    let path = root.join(format!("{id}.md"));
    // create_new (O_CREAT|O_EXCL): fails if the path already exists — INCLUDING a
    // (possibly dangling) symlink a same-UID attacker could plant in pits_root to
    // redirect the write outside it. This closes both the "already exists" case and the
    // symlink-follow / TOCTOU that a bare `exists()` + `write` has (codex WP1 P1).
    use std::io::Write;
    let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("pit `{id}` already exists at {} — edit it directly", path.display());
        }
        Err(e) => return Err(anyhow::Error::new(e).context(format!("create {}", path.display()))),
    };
    f.write_all(flint_core::pit::scaffold(id, desc, seed).as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    println!(
        "saved draft {}. Expand the body — it's knowledge you own. To ENFORCE it, write a rule and `flint canon pick` (pits never auto-promote).",
        path.display()
    );
    Ok(())
}

fn run_budget(config: &Path, record: Option<u64>, reset: bool) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let sidecar = cfg
        .budget
        .sidecar
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no [budget] sidecar configured in flint.toml"))?;
    if reset {
        flint_core::budget::reset(sidecar)?;
        println!("budget reset to 0");
    } else if let Some(tokens) = record {
        let total = flint_core::budget::add(sidecar, tokens)?;
        let thr = cfg.budget.critique_threshold;
        let over = thr > 0 && total >= thr;
        println!("budget: +{tokens} -> {total} tokens{}", if over { " (AT/OVER threshold — actions now critique)" } else { "" });
    } else {
        println!("budget: {} tokens (threshold {})", flint_core::budget::read_cumulative(sidecar), cfg.budget.critique_threshold);
    }
    Ok(())
}

/// `flint hook` — special-cased so a gate-SETUP failure (config load / stdin) in
/// block+closed emits a deny rather than exiting non-zero with no verdict (an agent could
/// otherwise trigger a setup failure to fail the gate open).
fn run_hook_cmd(harness: &str, config: &Path, mode: &str, fail_mode: &str, show: bool) -> Result<()> {
    let mode = hook::Mode::parse(mode)?;
    let fail_mode = hook::FailMode::parse(fail_mode)?;
    let harness = Harness::parse(harness).map_err(|e| anyhow::anyhow!(e))?;

    let outcome = (|| -> Result<i32> {
        let cfg = FlintConfig::load(config)?;
        if show {
            hook::show_policy(&cfg).map_err(|e| anyhow::anyhow!(e))?;
            Ok(0)
        } else {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(hook::run_hook(&cfg, harness, mode, fail_mode, &buf))
        }
    })();
    match outcome {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            if show {
                eprintln!("flint hook --show: {e}");
            } else {
                hook::deny_on_setup_failure(harness, mode, fail_mode, &e.to_string());
            }
            Ok(())
        }
    }
}

fn run_compile(harness: &str, config: &Path, mode: &str, target_dir: Option<&Path>) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    // Validate the mode here (reuse the hook's parser) so a bad `--mode` is a loud error,
    // not a string that silently lands in the emitted wiring.
    hook::Mode::parse(mode)?;
    let params = CompileParams {
        flint_bin: cfg.flint_bin.clone(),
        config_path: config.display().to_string(),
        mode: mode.to_string(),
    };
    let h = Harness::parse(harness).map_err(|e| anyhow::anyhow!(e))?;
    match h {
        Harness::Claude => {
            let wiring = serde_json::to_string_pretty(&striker::claude_settings(&params))?;
            match target_dir {
                None => println!("{wiring}"),
                Some(dir) => {
                    // Wiring is installed once (operator merges); advisory rules are the
                    // part that changes with Canon, so those we WRITE.
                    println!("# merge into .claude/settings.json:\n{wiring}");
                    let policy = hook::load_policy(&cfg).map_err(|e| anyhow::anyhow!("gate error: {e}"))?;
                    // Append flint's opt-out capture default (L1); off → base untouched.
                    let body = capture::append_advisory(&striker::claude_advisory_rules(&policy), cfg.capture.auto_mine);
                    let rules_path = dir.join(".claude").join("rules").join("flint-advisory.md");
                    if body.is_empty() {
                        // no advisory rules — remove any stale managed file so Claude
                        // stops loading outdated guidance after a Canon update (codex P2-2).
                        if rules_path.exists() {
                            std::fs::remove_file(&rules_path)?;
                            println!("# removed stale {}", rules_path.display());
                        } else {
                            println!("# no advisory rules — nothing to write");
                        }
                    } else {
                        if let Some(parent) = rules_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&rules_path, &body)?;
                        println!("# wrote {}", rules_path.display());
                    }
                }
            }
        }
        Harness::Codex => {
            let hooks = serde_json::to_string_pretty(&striker::codex_hooks(&params))?;
            println!("# ~/.codex/hooks.json (or [[hooks.PreToolUse]] in config.toml):");
            println!("{hooks}");
            // The scope + advisory rules need the signed policy (codex apply_patch enforcement
            // is flaky — these go to AGENTS.md as guidance).
            match target_dir {
                None => {
                    // print-only: a load failure just means no advisory is printed.
                    let md = hook::load_policy(&cfg)
                        .map(|p| capture::append_advisory(&striker::codex_agents_md(&p), cfg.capture.auto_mine))
                        .unwrap_or_default();
                    if !md.is_empty() {
                        println!("\n# --- append to AGENTS.md ---\n{md}");
                    }
                }
                Some(dir) => {
                    // target_dir WRITES — a load failure must FAIL LOUDLY, not silently
                    // remove the flint block via an empty upsert (codex P2-1).
                    let policy = hook::load_policy(&cfg).map_err(|e| anyhow::anyhow!("gate error: {e}"))?;
                    let md = capture::append_advisory(&striker::codex_agents_md(&policy), cfg.capture.auto_mine);
                    let agents_path = dir.join("AGENTS.md");
                    let existing = std::fs::read_to_string(&agents_path).unwrap_or_default();
                    let updated = striker::upsert_marked_block(&existing, &md);
                    std::fs::write(&agents_path, &updated)?;
                    println!("# updated flint block in {}", agents_path.display());
                }
            }
        }
        Harness::Grok => {
            // Wiring only. No advisory is compiled for Grok: advisory is the prompt layer,
            // and Grok's own measured PreToolUse channel does not deliver hook text to the
            // model at all — out of scope here.
            let wiring = serde_json::to_string_pretty(&striker::grok_hooks(&params))?;
            println!("# write to ~/.grok/hooks/flint.json:");
            println!("{wiring}");
        }
    }
    Ok(())
}

fn run_lint(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let files = collect_rule_files(&cfg.canon_root)?;
    match canon::reduce(&files) {
        Ok(policy) => {
            println!("canon lint: OK — {} rule(s) parse cleanly", policy.rules.len());
            Ok(())
        }
        Err(e) => {
            eprintln!("canon lint: FAIL — {e}");
            std::process::exit(1);
        }
    }
}

fn run_list(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let tiered = hook::load_canon_tiered(&cfg).map_err(|e| anyhow::anyhow!("canon list: {e}"))?;
    for (rule, tier) in tiered {
        println!(
            "RULE\t{}\t{}\t{}\t{:?}\t{}",
            rule.id,
            rule.matcher.describe(),
            rule.response.tag(),
            tier,
            rule.message.lines().next().unwrap_or("")
        );
    }
    Ok(())
}

fn run_forge_promote(config: &Path, rule_id: &str) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    // Load the working-tree rule (the candidate) + its discrimination fixture.
    let files = collect_rule_files(&cfg.canon_root)?;
    let policy = canon::reduce(&files).map_err(|e| anyhow::anyhow!("canon lint FAIL: {e}"))?;
    let rule = policy
        .rules
        .iter()
        .find(|r| r.id == rule_id)
        .ok_or_else(|| anyhow::anyhow!("no rule with id `{rule_id}` in {}/rules", cfg.canon_root.display()))?;
    let fixture_path = cfg.canon_root.join(format!("fixtures/{rule_id}.md"));
    let fixture_bytes = std::fs::read(&fixture_path)
        .with_context(|| format!("read discrimination fixture {}", fixture_path.display()))?;
    let fixture = forge::parse_fixture(std::str::from_utf8(&fixture_bytes).unwrap_or(""))
        .ok_or_else(|| anyhow::anyhow!("malformed fixture {}", fixture_path.display()))?;
    // L2 cross-vendor judge (P4 §3.7): veto-only, default off (§8.4 #5 — the owner toggles
    // `[judge] cross_vendor`). On the cold promotion path only (a per-action shell-out
    // would be too slow for the hot hook).
    let veto = cross_vendor::CodexVeto::new(cfg.judge.cross_vendor, cfg.judge.model.clone());
    if cfg.judge.cross_vendor {
        eprintln!("forge promote: consulting cross-vendor judge (veto-only)...");
    }
    match forge::promote(rule, &fixture, &fixture_bytes, &veto) {
        forge::Promotion::Reproduced(rec) => {
            let prom_dir = cfg.canon_root.join("promotion");
            std::fs::create_dir_all(&prom_dir)?;
            let prom_path = prom_dir.join(format!("{rule_id}.md"));
            std::fs::write(&prom_path, rec.to_markdown())?;
            println!(
                "promoted `{rule_id}` -> reproduced ({} bad / {} good). Wrote {}. Run `flint canon pick` to sign it in.",
                rec.bad_cases, rec.good_cases, prom_path.display()
            );
            Ok(())
        }
        forge::Promotion::Rejected { reason, .. } => {
            eprintln!("forge promote: `{rule_id}` NOT promoted — {reason}");
            std::process::exit(1);
        }
    }
}

fn run_forge_ceiling(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let files = collect_rule_files(&cfg.canon_root)?;
    let policy = canon::reduce(&files).map_err(|e| anyhow::anyhow!("canon lint FAIL: {e}"))?;
    let mut promotable = 0usize;
    for rule in &policy.rules {
        let fixture_path = cfg.canon_root.join(format!("fixtures/{}.md", rule.id));
        let status = match std::fs::read(&fixture_path) {
            Ok(bytes) => match forge::parse_fixture(std::str::from_utf8(&bytes).unwrap_or("")) {
                Some(fx) => match forge::promote_default(rule, &fx, &bytes) {
                    forge::Promotion::Reproduced(_) => {
                        promotable += 1;
                        "DISCRIMINATES (reproduced)".to_string()
                    }
                    forge::Promotion::Rejected { reason, .. } => format!("has fixture but FAILS: {reason}"),
                },
                None => "fixture present but MALFORMED".to_string(),
            },
            Err(_) => "no fixture (prose only)".to_string(),
        };
        println!("{}\t{}", rule.id, status);
    }
    println!(
        "\nverifier ceiling: {}/{} rule(s) admit an executable discrimination fixture (reproduced); the rest stay prose.",
        promotable,
        policy.rules.len()
    );
    Ok(())
}

fn run_pick(config: &Path, key: &Path, epoch: Option<u64>) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let (epoch, count) = sign_and_place(&cfg, key, epoch)?;
    println!(
        "picked: signed {} ({count} file(s), epoch {epoch})",
        cfg.canon_root.join("CANON.manifest").display()
    );
    Ok(())
}

/// Lint the working-tree rules, build a signed manifest over every SIGNABLE file (proposed
/// laws excluded), sign it with the sovereign key, self-verify, and atomically place it —
/// the one sovereign write that makes rules load-bearing. Shared by `flint canon pick` AND
/// every `flint law` verb (accept / disable / remove each mutate a law then re-sign here).
/// Returns `(signed epoch, signed file count)`.
fn sign_and_place(cfg: &FlintConfig, key: &Path, epoch: Option<u64>) -> Result<(u64, usize)> {
    // (0) key custody (spec §2): re-check the sovereign key's permissions before EVERY signing
    // use — refuse to sign with a group/world-accessible key or a symlink standing in for it.
    init::assert_key_perms(key)?;
    // (1) lint-gate: malformed working-tree rules NEVER get signed (§3.6).
    let rule_files = collect_rule_files(&cfg.canon_root)?;
    canon::reduce(&rule_files).map_err(|e| anyhow::anyhow!("refusing to pick — canon lint FAIL: {e}"))?;

    // (2) build the manifest over ALL signable files (rules + promotion records + fixtures),
    // so promotion records / fixtures inherit the root of trust too (§3.7). Sorted set.
    // Auto-epoch = (current signed epoch + 1). The epoch is read from a manifest whose
    // SIGNATURE we verify (never the raw header): `CANON.manifest` is agent-writable, so a
    // bare header read would let a tampered epoch line steer the next sovereign signature
    // (e.g. u64::MAX → overflow / DoS — codex WP-C review P1). Signature verification does NOT
    // hash the listed rule files, so a DIRTY working tree (the normal pre-re-pick state) still
    // verifies — that is exactly the epoch-wart fix (§6): the last signed epoch is a property
    // of the signed manifest bytes, independent of tree edits. An absent/tampered/unsigned
    // manifest contributes 0; the config `min_epoch` floors the result so a fresh pick never
    // lands at or below the anti-rollback floor; `saturating_add` can never overflow.
    // The anti-rollback bar this pick must clear: BOTH the config min_epoch AND the persistent
    // floor (if configured). Every epoch — explicit or auto — must land STRICTLY above it, or
    // the pick installs a manifest the hook rejects as a rollback (self-lock), or collides with
    // an existing manifest at the high-water epoch (same-epoch rollback goes undetected) —
    // codex anti-rollback P1.
    let effective_floor = cfg
        .trust
        .epoch_floor
        .as_ref()
        .map(|p| trust::read_epoch_floor(p))
        .unwrap_or(0)
        .max(cfg.trust.min_epoch);
    let epoch = match epoch {
        // An explicit epoch AT OR BELOW the floor self-locks the gate (hook rejects it) or
        // collides at the high-water epoch — refuse loudly instead of signing a dead manifest.
        Some(e) if e <= effective_floor => {
            anyhow::bail!(
                "explicit --epoch {e} is at/below the anti-rollback floor {effective_floor} — \
                 the hook would reject this pick as a rollback; pick a higher epoch"
            );
        }
        Some(e) => e,
        // Auto-epoch: strictly one above max(last-signed epoch, floor). checked_add so a floor
        // at u64::MAX is a loud error, never a saturating collision at MAX (saturating_add would
        // stop being strictly monotonic there). Floors over the last-signed epoch too, so a pick
        // after a fleet-key revocation (current_signed_epoch → None) still clears the floor.
        None => current_signed_epoch(cfg)
            .unwrap_or(0)
            .max(effective_floor)
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("epoch is at u64::MAX — cannot advance the anti-rollback floor"))?,
    };
    let files: Vec<(String, Vec<u8>)> = collect_signable_files(&cfg.canon_root)?
        .into_iter()
        // A PROPOSED law is NOT signed — it bears weight only after `flint law accept` flips
        // it to accepted (propose≠pick, the sovereignty boundary, spec §2). Only rules/ files
        // carry a status; promotion/ + fixtures/ are always signable.
        .filter(|(path, bytes)| {
            if !path.starts_with("rules") {
                return true;
            }
            match std::str::from_utf8(bytes) {
                Ok(text) => canon::rule_status(&path.to_string_lossy(), text)
                    .map(canon::Status::is_signable)
                    .unwrap_or(true), // a bad status value already failed the lint gate above
                Err(_) => true,
            }
        })
        .map(|(p, b)| (p.to_string_lossy().to_string(), b))
        .collect();
    let manifest = trust::build_manifest(&cfg.trust.scope, epoch, &files);

    // (3) Sign the EXACT manifest bytes (§3.5.1 req #5). Write + sign in a PRIVATE 0700
    // random temp dir, then SELF-VERIFY the produced signature covers our exact in-memory
    // bytes BEFORE placing it into the (agent-writable) canon root — a failed verify aborts
    // before any canon-root write. A post-pick swap in the canon root is harmless (runtime
    // re-verifies the signature there).
    //
    // HONEST BOUNDARY (codex r2 P1 — same-UID temp-path signing race): `ssh-keygen -Y sign`
    // signs a FILE (no stdin sig output in OpenSSH 9.x), so a SAME-UID attacker running a
    // watcher could in principle swap the temp manifest in the write->open window and get
    // the sovereign key to sign attacker bytes (self-verify still blocks INSTALL, but the
    // signature was produced). The random 0700 temp dir is a practical mitigation, NOT a
    // same-UID security boundary. This is the §3.5 tier boundary, not a fixable code bug:
    //   - weak tier (readable key, same-UID agent): the agent can READ `--key` and forge
    //     directly, so the race is strictly dominated — moot.
    //   - FIDO/hardware key + same-UID agent: the race matters, but that is a
    //     self-inconsistent setup (hardware-protected key, unprotected process boundary).
    //   - strong tier (OS-isolated / different-UID agent): the agent cannot reach the temp
    //     dir — closed.
    // i.e. pick is a SOVEREIGN op: run it where no untrusted same-UID agent is concurrently
    // active, or under OS isolation. (reframe-and-diff §3.5.1.)
    let tmp = tempfile::tempdir().context("create temp dir for signing")?;
    let tmp_manifest = tmp.path().join("CANON.manifest");
    std::fs::write(&tmp_manifest, manifest.as_bytes()).context("write temp manifest")?;
    let status = std::process::Command::new("ssh-keygen")
        .args(["-Y", "sign", "-f"])
        .arg(key)
        .args(["-n", trust::SIG_NAMESPACE])
        .arg(&tmp_manifest)
        .status()
        .context("run ssh-keygen -Y sign")?;
    if !status.success() {
        anyhow::bail!("ssh-keygen -Y sign failed");
    }
    let tmp_sig = tmp.path().join("CANON.manifest.sig");
    trust::verify_manifest_sig(&cfg.sovereign_trust(), manifest.as_bytes(), &tmp_sig)
        .map_err(|e| anyhow::anyhow!("pick self-verify failed (signed bytes != intended, or key/allowed_signers mismatch): {e}"))?;
    // Place into the canon root via same-dir temp + atomic rename (NOT fs::copy, which
    // truncates-then-writes: a hook reading mid-copy would see a PARTIAL manifest → parse
    // error → fail-closed spurious deny). rename is atomic, so each file is always whole.
    //
    // HONEST BOUNDARY (the canon-tearing residual the reconcile assigned to WP-C): manifest
    // and sig are two separate atomic renames, so a hook firing in the µs BETWEEN them sees
    // (new manifest + old sig) or (old manifest + new sig) — a signature mismatch → a
    // TRANSIENT fail-closed deny (never a wrong Affirm — the mismatch always denies, the
    // safe direction). pick is a rare sovereign op; a single-rename pair-atomic swap (one
    // combined artifact) is the escalation if real tearing is ever observed (spec §7 →
    // WP-C note). Partial reads — the worse failure — are eliminated here.
    let sig_bytes = std::fs::read(&tmp_sig).context("read signed sig")?;
    let manifest_path = cfg.canon_root.join("CANON.manifest");
    atomic_place(&cfg.canon_root, "CANON.manifest", manifest.as_bytes())
        .with_context(|| format!("install {}", manifest_path.display()))?;
    atomic_place(&cfg.canon_root, "CANON.manifest.sig", &sig_bytes).context("install manifest sig")?;
    let _ = manifest_path;
    if epoch <= cfg.trust.min_epoch {
        eprintln!(
            "warning: epoch {epoch} <= config min_epoch {} — the hook will reject this as a rollback; \
             bump min_epoch or pick a higher epoch.",
            cfg.trust.min_epoch
        );
    }
    // Advance the persistent anti-rollback floor (if configured) to this epoch, so a later
    // checkout of an OLDER signed manifest is rejected without a manual min_epoch bump. A bump
    // failure is a warning, never a pick failure — the canon is already signed; the floor is a
    // hardening increment on top (see `[trust] epoch_floor`, weak-tier boundary).
    if let Some(floor_path) = &cfg.trust.epoch_floor {
        if let Err(e) = trust::bump_epoch_floor(floor_path, epoch) {
            eprintln!("warning: could not advance epoch floor {}: {e}", floor_path.display());
        }
    }
    Ok((epoch, files.len()))
}

/// The current SIGNED manifest's epoch, or `None`. VERIFIES the sovereign signature over the
/// manifest bytes (so a tampered epoch line is rejected — codex WP-C P1) but does NOT hash the
/// listed rule files (so a dirty working tree still yields the epoch — the §6 epoch fix). The
/// epoch is parsed from the SAME bytes whose signature verified, so there is no reread TOCTOU.
fn current_signed_epoch(cfg: &FlintConfig) -> Option<u64> {
    let bytes = std::fs::read(cfg.canon_root.join("CANON.manifest")).ok()?;
    let sig = cfg.canon_root.join("CANON.manifest.sig");
    let mut trust = cfg.sovereign_trust();
    trust.min_epoch = 0; // reading the epoch, not enforcing the floor
    trust::verify_manifest_sig(&trust, &bytes, &sig).ok()?;
    trust::parse_manifest_header(&bytes).map(|h| h.epoch)
}

/// Write `bytes` to `<dir>/<name>` via a same-dir temp file + atomic rename. The temp is on
/// the SAME filesystem as the destination (created inside `dir`), so `persist` is a rename,
/// not a cross-device copy — the destination is never observed half-written.
fn atomic_place(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("create temp in {}", dir.display()))?;
    tmp.write_all(bytes).context("write temp")?;
    tmp.flush().context("flush temp")?;
    tmp.persist(dir.join(name))
        .map_err(|e| anyhow::anyhow!("atomic rename into place: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// `flint law` — the per-law lifecycle verbs (spec §2 sovereignty model).
// ---------------------------------------------------------------------------

/// The `id` a working-tree rule file declares (find-by-id, since a filename need not match).
fn law_id(label: &str, text: &str) -> Option<String> {
    match canon::parse_rule(label, text).ok()? {
        canon::ParsedRule::Gate(cr) => Some(cr.rule.id),
        canon::ParsedRule::Advisory(a) => Some(a.id),
    }
}

/// Find the working-tree rule file whose parsed `id` == `id`. Errors on none / ambiguous.
fn find_law_file(canon_root: &Path, id: &str) -> Result<PathBuf> {
    let files = collect_rule_files(canon_root)?;
    let mut hits: Vec<PathBuf> = Vec::new();
    for (rel, bytes) in &files {
        let Ok(text) = std::str::from_utf8(bytes) else { continue };
        if law_id(&rel.to_string_lossy(), text).as_deref() == Some(id) {
            hits.push(canon_root.join(rel));
        }
    }
    match hits.len() {
        0 => anyhow::bail!("no law with id `{id}` under {}/rules", canon_root.display()),
        1 => Ok(hits.pop().expect("one hit")),
        n => anyhow::bail!("{n} rules share id `{id}` — resolve the duplicate first"),
    }
}

/// Atomically overwrite a rule file (same-dir temp + rename).
fn write_law(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| anyhow::anyhow!("law path has no parent: {}", path.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow::anyhow!("law path has no filename: {}", path.display()))?;
    atomic_place(dir, name, content.as_bytes())
}

fn run_law_list(config: &Path) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let files = collect_rule_files(&cfg.canon_root)?;
    let mut rows: Vec<(String, String, String, String)> = Vec::new(); // (status, id, kind, first msg line)
    for (rel, bytes) in &files {
        let label = rel.to_string_lossy().to_string();
        let Ok(text) = std::str::from_utf8(bytes) else {
            rows.push(("?".into(), label, "non-utf8".into(), String::new()));
            continue;
        };
        let status = canon::rule_status(&label, text).map(|s| s.as_str().to_string()).unwrap_or_else(|_| "?".into());
        match canon::parse_rule(&label, text) {
            Ok(canon::ParsedRule::Gate(cr)) => rows.push((
                status,
                cr.rule.id,
                cr.rule.matcher.describe().to_string(),
                cr.rule.message.lines().next().unwrap_or("").to_string(),
            )),
            Ok(canon::ParsedRule::Advisory(a)) => rows.push((
                status,
                a.id,
                "advisory".into(),
                a.message.lines().next().unwrap_or("").to_string(),
            )),
            Err(e) => rows.push((status, label, "MALFORMED".into(), e.to_string())),
        }
    }
    rows.sort();
    for (status, id, kind, msg) in &rows {
        println!("LAW\t{status}\t{id}\t{kind}\t{msg}");
    }
    println!("{} law(s)", rows.len());
    Ok(())
}

fn run_law_show(config: &Path, name: &str) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let path = find_law_file(&cfg.canon_root, name)?;
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    print!("{text}");
    Ok(())
}

/// Flip one law to `new` and re-sign. `verb` labels the message. `disable` / `remove` route
/// here; `accept` of a single law routes here too (via `run_law_accept`).
fn run_law_transition(config: &Path, key: &Path, id: &str, new: canon::Status, verb: &str) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    let (from, epoch) = transition_one(&cfg, key, id, new)?;
    println!("{verb}: law `{id}` {} -> {}; re-signed at epoch {epoch}", from.as_str(), new.as_str());
    Ok(())
}

/// Mutate one law's status on disk (atomic) then re-sign. Returns (previous status, new epoch).
/// ATOMIC w.r.t. signing (codex WP-C P2): if the re-sign fails (bad key, no ssh-keygen, trust
/// mismatch), the source file is rolled back to its original bytes — a failed transition leaves
/// the working tree exactly as it was, never a mutated-but-unsigned status the next pick would
/// silently commit.
fn transition_one(cfg: &FlintConfig, key: &Path, id: &str, new: canon::Status) -> Result<(canon::Status, u64)> {
    let path = find_law_file(&cfg.canon_root, id)?;
    let original = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let from = canon::rule_status(&path.to_string_lossy(), &original)?;
    if from == new {
        anyhow::bail!("law `{id}` is already {}", new.as_str());
    }
    let updated = canon::set_status(&original, new).map_err(|e| anyhow::anyhow!("rewrite status: {e}"))?;
    write_law(&path, &updated)?;
    match sign_and_place(cfg, key, None) {
        Ok((epoch, _)) => Ok((from, epoch)),
        Err(e) => {
            // best-effort rollback: restore the pre-mutation bytes so the failure is a no-op.
            if let Err(re) = write_law(&path, &original) {
                anyhow::bail!("re-sign failed ({e}) AND rollback of {} failed ({re}) — restore it by hand", path.display());
            }
            Err(e.context("re-sign failed — law status change rolled back"))
        }
    }
}

fn run_law_accept(config: &Path, key: &Path, name: Option<&str>, all: bool) -> Result<()> {
    let cfg = FlintConfig::load(config)?;
    match (name, all) {
        (Some(_), true) => anyhow::bail!("pass either --name <id> or --all, not both"),
        (None, false) => anyhow::bail!("specify --name <id> or --all"),
        (Some(id), false) => {
            let (from, epoch) = transition_one(&cfg, key, id, canon::Status::Accepted)?;
            println!("accept: law `{id}` {} -> accepted; signed at epoch {epoch}", from.as_str());
            Ok(())
        }
        (None, true) => {
            // Two-pass: validate + plan EVERY proposed law's rewrite first (a bad law aborts
            // before any disk write), then apply all, then ONE signed pick. If that pick fails,
            // roll ALL mutated files back to their originals (codex WP-C P2 — atomic w.r.t.
            // signing; a failed accept --all leaves the tree untouched).
            let files = collect_rule_files(&cfg.canon_root)?;
            let mut plan: Vec<(PathBuf, String, String, String)> = Vec::new(); // (path, original, updated, id)
            for (rel, bytes) in &files {
                let Ok(text) = std::str::from_utf8(bytes) else { continue };
                let label = rel.to_string_lossy().to_string();
                if canon::rule_status(&label, text)? == canon::Status::Proposed {
                    let updated = canon::set_status(text, canon::Status::Accepted).map_err(|e| anyhow::anyhow!("{e}"))?;
                    let id = law_id(&label, text).unwrap_or(label);
                    plan.push((cfg.canon_root.join(rel), text.to_string(), updated, id));
                }
            }
            if plan.is_empty() {
                println!("accept --all: no proposed laws to accept");
                return Ok(());
            }
            for (path, _orig, updated, _id) in &plan {
                write_law(path, updated)?;
            }
            let (epoch, count) = match sign_and_place(&cfg, key, None) {
                Ok(r) => r,
                Err(e) => {
                    for (path, orig, _u, _id) in &plan {
                        let _ = write_law(path, orig); // best-effort rollback
                    }
                    return Err(e.context("accept --all re-sign failed — all status changes rolled back"));
                }
            };
            let mut ids: Vec<&str> = plan.iter().map(|(_, _, _, id)| id.as_str()).collect();
            ids.sort_unstable();
            println!(
                "accept --all: accepted {} law(s) [{}]; signed {count} file(s) at epoch {epoch}",
                ids.len(),
                ids.join(", ")
            );
            Ok(())
        }
    }
}

/// Collect working-tree rule files: every `.md` under `<canon_root>/rules/`, keyed by its
/// path RELATIVE TO `canon_root` (so it matches the manifest path scheme).
fn collect_rule_files(canon_root: &Path) -> Result<std::collections::BTreeMap<PathBuf, Vec<u8>>> {
    let mut out = std::collections::BTreeMap::new();
    let rules_dir = canon_root.join("rules");
    if !rules_dir.exists() {
        anyhow::bail!("canon rules dir not found: {}", rules_dir.display());
    }
    walk_md(&rules_dir, canon_root, &mut out)?;
    Ok(out)
}

/// Collect every signable file: `.md` under `rules/`, `promotion/`, and `fixtures/`. These
/// all go into the signed manifest so they bear the root of trust (§3.7). `rules/` must
/// exist; the other two are optional.
fn collect_signable_files(canon_root: &Path) -> Result<std::collections::BTreeMap<PathBuf, Vec<u8>>> {
    let mut out = collect_rule_files(canon_root)?;
    for sub in ["promotion", "fixtures"] {
        let dir = canon_root.join(sub);
        if dir.exists() {
            walk_md(&dir, canon_root, &mut out)?;
        }
    }
    Ok(out)
}

fn walk_md(dir: &Path, root: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_md(&path, root, out)?;
        } else if ft.is_file() && path.extension().is_some_and(|e| e == "md") {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            out.insert(rel, bytes);
        }
    }
    Ok(())
}
