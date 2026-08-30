# SETUP.md — installing and operating Flint, written for agents

You are an agent, and your operator wants Flint installed or working on this machine.
This file is the single install-and-operate manual; every command in it works equally
when a human runs it by hand. The ideas behind Flint are in [`README.md`](README.md);
the map of the code is in [`ARCHITECTURE.md`](ARCHITECTURE.md). Working on Flint's own
source code? That is [`CONTRIBUTING.md`](CONTRIBUTING.md)'s territory.

## What you are installing

Flint is the mechanism by which **your operator's rules — signed by them — govern your
tool calls**: allow, warn, or block, at the moment of action. Installing it puts exactly
this on the machine, and nothing else:

| Artifact | Where | What it is |
|---|---|---|
| one binary, `flint` | built into `target/release/`, linked onto `PATH` | judge + compiler + CLI; no server, no daemon, no telemetry |
| a flint home | `~/.flint` (or `--home <dir>`) | the operator's Ed25519 key (`keys/`, 0700/0600), the rule canon (`canon/rules/`, plain markdown), config, receipt log, capture inbox |
| hook fragments | the harness configs the operator names (`~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.grok/hooks/flint.json`) | a PreToolUse line pointing each harness at the `flint hook` judge |

Uninstalling is the mirror: remove the fragments, delete `~/.flint`, drop the binary.

## Before you start — brief the operator

Surface these to the operator and get their go-ahead before touching anything:

1. **Prerequisites**: Rust **1.85+** toolchain (`rustc --version`), `git`, and
   `ssh-keygen` (signing shells out to it). If Rust is missing, agree with the operator
   on how to install it before proceeding.
2. **Which harnesses to wire** — Claude Code, Codex, Grok, or a subset. Their config
   files will gain a flint fragment (merged, not overwritten).
3. **One step is personally theirs**: signing. Midway through setup you will hand them
   one command to run themselves, and setup pauses until they do.
4. **Rules are their choice**: the canon starts empty; they pick from `examples/laws/`
   or write their own. Nothing is enforced that they did not sign.

## The sovereignty line — read this first

**propose ≠ pick.** You may propose anything: draft rules, copy samples, lint, wire hooks,
verify, troubleshoot. **Picking — the signature — is the operator's hand on the key.**
The two signing commands, `flint law accept` and `flint canon pick`, are theirs to run
personally: your job is to prepare everything, hand them the exact command verbatim, and
wait. This holds even if they offer you permission in chat — an agent that signs is a
model editing its own constitution, the one thing Flint exists to prevent. Decline,
explain in one line, and hand the command back.

The same shape governs bypasses: a rule's exempt mechanism (declaring a legitimate
downgrade, e.g. via an env-var prefix the rule text names) is for cases the *operator*
directs or the rule text itself sanctions — and every exempt passage lands in the receipt
log, where bypass habits are audited. When a gate blocks you, the right move is in
[Working under Flint](#working-under-flint) below.

## Install

Each step ends on a check. Run the check; move on only when it holds.

**0. Preflight.** `rustc --version` (needs 1.85+), `git --version`, `ssh-keygen` present
(signing shells out to it). *Done when all three print versions.*

**1. Build.**

```sh
git clone https://github.com/PunkGo/flint && cd flint
cargo build --release
```

*Done when* `./target/release/flint --version` prints `flint 0.1.x (<git-hash>)`.

**2. PATH.** Symlink or copy `target/release/flint` somewhere the shell finds it
(e.g. `ln -s "$PWD/target/release/flint" /usr/local/bin/flint`). *Done when* `flint
--version` resolves from any directory.

**3. Init.** `flint init` (default home `~/.flint`; `--home <dir>` to override,
`--with-memory` to wire an opt-in markdown vault, `--scope <name>` to namespace the
instance). This generates the operator's sovereign Ed25519 key (private key never leaves
the machine) and scaffolds an **empty** canon — Flint ships the mechanism, never the
rules. *Done when* the output shows `canon ... (empty — rules are yours ...)`.

**4. Rules.** Read the frontmatter `description:` line of each sample in
[`examples/laws/`](examples/laws/), present the list to the operator, and copy the ones
they choose into `~/.flint/canon/rules/` (they can also write their own — the authoring
format is in `examples/laws/README.md` and the README). *Done when* `flint law list
--config ~/.flint/flint.toml` shows each chosen rule as `proposed`.

**5. Hand over the signature.** Give the operator this command verbatim and wait:

```sh
flint law accept --all --config ~/.flint/flint.toml --key ~/.flint/keys/sovereign_ed25519
```

*Done when* the operator has run it and `flint canon list --config ~/.flint/flint.toml`
lists the accepted rules. (Before the first signature that command exits non-zero with
`no signed` — that is the design: nothing bears weight yet.)

**6. Wire the harnesses.** For each harness the operator uses:

```sh
flint compile --harness claude --config ~/.flint/flint.toml   # merge into ~/.claude/settings.json
flint compile --harness codex  --config ~/.flint/flint.toml   # ~/.codex/hooks.json + AGENTS.md block
flint compile --harness grok   --config ~/.flint/flint.toml   # ~/.grok/hooks/flint.json
```

`compile` prints the hook fragment; merge it into the file its comment names.
`--target-dir <dir>` writes the advisory files (Claude `.claude/rules/`, Codex
`AGENTS.md`) directly. What each harness enforces for each rule kind is the
["What enforces where" table in the README](README.md#what-enforces-where).
*Done when* each named config file contains the flint fragment.

**7. Live-fire proof.** **receipt ≠ enforcement**: a receipt records Flint's judgment,
not the harness's obedience — enforcement is only ever proven by watching a command not
run. In a **new** agent session (hooks load at session start), run a control probe that
matches no rule (`printf CONTROL_OK` — must print), then a probe that matches an accepted
gate rule and would print a marker if executed. *Done when* the control marker printed,
the gated probe's marker did not, and the rule's reason text reached you as the block
message.*

For the optional workflow-skills suite: `scripts/bootstrap.sh` (Windows:
`scripts\bootstrap.ps1`) builds and runs `flint install`; `flint install --check` reports
drift, `--plan` is a dry run. Install writes only inside `~/.claude` / `~/.codex` /
`~/.flint` and records an `installed.lock` for honest removal. `--instance <git-url>` is
opt-in and one-time: it clones the operator's private memory repo and writes
`~/.flint/instance.toml`; without it, instance-sourced entries skip with a warning.
`flint install --stage full` also installs a Codex `SessionStart` hook pinned to the
current Git `HEAD`: the quiet auto-sync re-runs only when the repo is clean, on `main`,
and still at that approved commit (it never pulls, rebuilds, or adopts a newer commit) —
after the repo moves, run the non-quiet full install again and have the operator re-trust
the hook in Codex `/hooks`.

## Working under Flint

- **A block is the operator's judgment landing, not an obstacle.** `warn` proceeds and
  hands you context; `critique` blocks and names a recovery path — follow it (fix the
  action so it satisfies the rule, or use the exempt form the rule text itself offers,
  with its reason); `block` is a hard freeze — stop and tell the operator. Satisfy the
  rule rather than rephrasing the command until the matcher misses: every verdict is
  receipted, and dodges read exactly like what they are.
- **Which list is truth:** `flint law list` reads the working tree; `flint canon list`
  reads the signed manifest — **enforcement follows only the signed manifest.** When in
  doubt about whether a rule governs, trust `canon list`.
- **Editing rules:** edit the `.md`, then hand the operator the re-sign (`flint canon
  pick --config … --key …`). Between edit and re-sign the whole set fails closed — so
  prepare the edit and the hand-off in one motion, and expect gate rules to take effect
  on the next hook call after the pick (no reinstall needed). Advisory rules additionally
  need `flint install` after signing to reach agent context.
- **Capture walls.** When you hit a wall or learn something worth keeping, append it as a
  one-line gist: `flint pit mark --config ~/.flint/flint.toml "<gist>"`. Nothing is
  auto-promoted; the operator triages with `flint knowledge review`.
- The verb surface is discoverable: `flint --help`, and `--help` on any subcommand.

## Troubleshooting

- **Every call suddenly denied** → a rule file changed after signing (hash mismatch →
  fail-closed). Hand the operator a re-pick.
- **A `flint/v1` rule saying `warn` refuses to load** → by design: `warn` changed meaning
  between schemas, and a signature covers bytes, not readings. The error names the fix
  (`schema: flint/v2`); edit, then hand over the re-sign.
- **Blocked but the command ran anyway (Windows/Codex path rules)** → expected: see the
  [enforcement matrix](README.md#what-enforces-where) — path rules on Codex ride advisory,
  and blocking rides the JSON envelope, never exit codes.
