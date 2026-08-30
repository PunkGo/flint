# Flint

<p align="center">
  <img src="assets/key-visual.png" alt="A small figure carrying a torch into the dark" width="420">
</p>

> 秉火前行。 — *Carry the fire forward.*

**Flint keeps your judgment yours in the age of autonomous agents.** Agents can now
read, write, decide, and ship end to end, faster than you can check. That is a gift. The
quiet cost is the one muscle you stop using: judgment. Flint is a small, local,
harness-agnostic layer that keeps the rules you follow **explicit, portable, enforced at
the moment of action, and revised only when you say so** — so your judgment stays with
you, not with the tool.

## The four actions

1. **Write it down.** The rules you follow become your own versioned plain text — not a
   vendor's opaque memory feature you can't read, export, or audit.
2. **Carry it across agents.** One rule source compiles into the native format each agent
   already reads (Claude Code's session rules, Codex's `AGENTS.md`). Your judgment follows
   *you*, not the tool. Switch agents and it comes along.
3. **Enforce at the moment of action.** When an agent is about to act, the applicable
   rules are brought to the spot: **allow, warn, or block — before the action lands**, not
   after.
4. **Revise only when you say so.** Rules change through *your* review, on *your* evidence.
   Never silently, never by the model editing its own constitution. You sign the change;
   nothing bears weight until you do.

In plain words: **a folder of your rules, a thing that compiles them into each agent, a
gate that judges by your rules at the moment of action, and a receipt that shows you which
rule fired.**

## What Flint does not do

Flint does not decide *for* you, and it does not judge whether your rules are wise, correct,
or good. It keeps your judgment **explicit, portable, enforced, and revisable** — and stops
there. A nod through Flint means *"I allowed this,"* never *"I was right."* The system
guarantees only that what is applied is **yours**, was **recorded**, and can be **taken
back**.

You will be wrong sometimes — and the mistake will be yours, visible, and reversible. That
is not a weakness; it is the whole point: a place to be wrong on your own terms, instead of
drifting unknowingly into someone else's defaults.

## Who this is for

One person running coding agents — Claude Code, Codex, Grok — who wants their own rules
enforced across all of them. Flint is a **local, personal** tool: one binary, plain files
under `~/.flint`, no server, no daemon, no telemetry, no account. It is not a team policy
engine and not a cloud service, and it will not become one.

## Design in one breath

- **Signed rule source (Canon).** Your rules are markdown files with a small, strict
  frontmatter. They bear weight only when you sign them with your sovereign Ed25519 key
  (`flint canon pick`). A malformed rule fails the whole set closed — it never silently
  half-applies.
- **Rides commodity hooks, not a kernel.** Flint compiles thin wiring that points each
  harness's existing PreToolUse hook at one judge. There is no server and no daemon to
  trust; the binary reads your signed Canon and decides locally.
- **Redacted, local receipts.** Every judged action leaves a receipt in an append-only
  local log — which rule fired, what the verdict was — with the raw command **never**
  written down (a gate that blocks a secret must not record it).
- **Three response tiers, and the word means the thing.** A rule's `response:` is one of:

  | `response:` | What happens to the call | Agent hears | Receipt |
  |---|---|---|---|
  | `warn` | **proceeds** | yes, as context | `warn` |
  | `critique` | **blocked**, with a recovery path | yes, as the blocking reason | `critique` |
  | `block` | **blocked**, a hard freeze, no recovery path offered | yes, as a denial | `deny` |

  An irreversible action escalates any tier to a hard freeze. A call that matches nothing
  is simply not spoken to — that is an absence of a rule, never an authorization, so Flint
  has no way to *approve* anything. (The judge's internal verdict has a fourth value,
  `affirm`, for exactly that no-rule-matched case.)

  `warn` requires `schema: flint/v2`. It is the one word whose meaning ever changed — in
  `flint/v1` it *blocked* — and a signature covers your bytes, not this parser's reading of
  them. So Flint refuses to guess: a `flint/v1` rule saying `warn` fails closed with an
  error telling you which word you want. Upgrading Flint can make a gate shout; it can
  never make one quietly stop gating.
- **Plain files, no lock-in.** What you can verify, you can carry.

The constitution — the few invariants Flint will never let change — lives in
[`docs/whitepaper.md`](docs/whitepaper.md).

## Install

Flint is a Rust workspace (`flint-core` + `flint-cli`, binary `flint`). Requires **Rust
1.85+**. The compiled binary is native to one OS/arch — build it on each machine you use;
your rule `.md` files are portable, the binary is not.

**1. Build.**

```sh
git clone https://github.com/PunkGo/flint && cd flint
cargo build --release          # -> target/release/flint
```

**2. Initialize your flint home.** One command generates the sovereign signing key (Ed25519;
the private key never leaves this box, `keys/` is `0700`, the key `0600`) and writes
`flint.toml`. The canon starts **empty** on purpose: flint ships the mechanism, never the
rules — rules are yours:

```sh
target/release/flint init                 # ~/.flint by default; --home <dir> to override
```

Add `--with-memory` to also wire an opt-in vault (point it at an existing Obsidian / wiki /
notes folder, or let it scaffold one — see [Bring your own memory](#bring-your-own-memory)),
or `--scope <name>` to namespace this instance against cross-instance replay.

**3. Pick your laws.** [`examples/laws/`](examples/laws/) holds sample rules grown from real
practice (secret-zero, lsp-over-grep, verify-before-claiming, …) — plain markdown, all
`status: proposed`. Copy the ones you actually want, read them, then sign. Nothing bears
weight until you accept it (`propose ≠ pick`, the sovereignty line):

```sh
cp examples/laws/secret-zero.md ~/.flint/canon/rules/
flint law list   --config ~/.flint/flint.toml     # proposed vs accepted, at a glance
flint law accept --all --config ~/.flint/flint.toml --key ~/.flint/keys/sovereign_ed25519
```

`accept --all` signs every proposed law in one pick; accept them one at a time with
`--name <id>`. `flint law disable` / `remove` later turn an accepted law off (an auditable,
re-signed record — never a silent vanish; a `remove` leaves a signed tombstone).

**4. Author your own rules** — drop a markdown file under `~/.flint/canon/rules/`,
then lint + pick to sign it in:

```markdown
---
schema: flint/v1
id: no-secrets-dir
type: rule
kind: path
glob: secrets/**
response: block
reversibility: irreversible
---
Do not write into secrets/. Use a vault / Keychain pointer.
```

```sh
flint canon lint --config ~/.flint/flint.toml
flint canon pick --config ~/.flint/flint.toml --key ~/.flint/keys/sovereign_ed25519
```

(`kind: command` matches a regex over a shell command instead; `kind: advisory` is a guideline
compiled into agent context rather than a gate.)

**5. Wire it into your agents.** `compile` prints the hook fragment to merge:

```sh
# Claude Code — merge the printed JSON into ~/.claude/settings.json
flint compile --harness claude --config ~/.flint/flint.toml

# Codex — prints ~/.codex/hooks.json wiring + the AGENTS.md advisory block
flint compile --harness codex --config ~/.flint/flint.toml

# Grok (xAI Grok Build) — prints the ~/.grok/hooks/flint.json wiring
flint compile --harness grok --config ~/.flint/flint.toml
```

`compile --target-dir <dir>` also writes the advisory files (Claude `.claude/rules/`, Codex
`AGENTS.md`) directly. That's it — the gate is live on your next agent session.

Tip: put the binary on your `PATH` — e.g.
`ln -s "$PWD/target/release/flint" /usr/local/bin/flint` (or copy it anywhere your shell
finds it). The examples below assume `flint` resolves.

## Suite bootstrap (workflow skills)

Beyond the gate, Flint ships its workflow skills (`/flint-knowledge`, `/flint-status`)
from a single source: `skills/` in this repo, declared in `scripts/manifest.toml`,
installed by the binary itself — no interpreter involved.

```sh
# macOS / Linux / WSL
scripts/bootstrap.sh                    # build + `flint install --stage skills`
# with a private memory-instance repo (cloned + pointer written if absent):
scripts/bootstrap.sh --instance git@github.com:you/your-memory.git
```

```powershell
# Windows native
scripts\bootstrap.ps1                   # same, PowerShell entrypoint
scripts\bootstrap.ps1 -Instance git@github.com:you/your-memory.git
```

`--instance` is opt-in and one-time: it clones YOUR private memory repo (default
`~/memory`, override with `--root`) and writes `~/.flint/instance.toml` so
`$INSTANCE`-sourced manifest entries resolve. The URL is passed in — never baked
into this repo. Without it the suite still installs; instance-sourced entries
skip with a warning.

Both are thin glue over the same three moves: `cargo build --release`, a signed-canon
preflight (stage `full` only), then `flint install`. All semantics live in the binary:
idempotent diff-writes, an `installed.lock` under `~/.flint` for honest removal,
targets confined to `~/.claude` / `~/.codex` / `~/.flint`. Useful directly:

```sh
flint install --check      # drift report (hand-edited / missing / pending removal)
flint install --plan       # dry run — prints what would change, writes nothing
```

`--stage full` (harness bindings rendered from your signed canon) is staged work —
refuse-on-unsigned-canon is already enforced. On a machine that may run either WSL or
native Windows, both entrypoints work against the same repo checkout; build natively in
each runtime (the binary is per-OS, your `.md` rules and skills are portable).

### Codex SessionStart suite sync

`flint install --stage full` installs a manifest-owned `SessionStart` hook for
`startup|resume`. The explicit install records the current Git `HEAD` in the hook
command; SessionStart re-runs `flint install --quiet --stage full` only when the
local Flint repository is clean, on `main`, and still at that approved commit. It
does not pull Git, rebuild, upgrade the Flint binary, or adopt a newer commit.
After first install—or after the repo moves to a commit you want to approve—run the
non-quiet full install again, then review and trust the new hook hash with Codex
`/hooks`.

## Using it day to day

- **Enforcement is automatic.** Once wired, every matching tool call is judged. You do
  nothing.
- **A rule grows from practice.** Hit a wall, capture it, and when a note earns it, turn it
  into a rule — `flint canon pick` again to sign the change in.
- **Capture walls, then refine them into knowledge.** While you work, walls and
  worth-keeping notes land as one-line gists — the agent marks them itself by default (the
  `[capture] auto_mine` nudge; turn it off any time), or you run `flint pit mark "<gist>"`.
  Later, triage the inbox: `flint knowledge review` shows what's pending across your ore
  stores, `flint knowledge promote` keeps a gist as a durable note (bare markdown you own),
  `flint knowledge toss` drops the noise. The `/flint-knowledge` skill walks the triage.
  Nothing is auto-promoted — a note is *knowledge*, never a rule; to enforce it, write a
  rule and `flint canon pick`.
- **See which rule fired.** `flint canon list` for the active signed set; the obs log for
  per-action receipts.

## Bring your own memory

Flint's product is the *gate* — memory is an **opt-in add-on it does not own**. Point
`flint init --with-memory --vault <dir>` at any folder of markdown (an existing Obsidian
vault, a wiki, a plain notes dir) and the `flint memory` verbs read and append it; leave it
off and nothing changes. The vault is knowledge, never signed or judged — flint owns no
memory vault of its own, so what you write stays plain text you can carry to any tool, never
locked in a vendor's opaque memory or a server you don't control.

```sh
flint memory capture --config ~/.flint/flint.toml "what you just learned"
flint memory list    --config ~/.flint/flint.toml
```

## Across your machines

Your rules are portable markdown, but a signature is per-key. The **fleet keyring** lets a
Canon signed on one machine verify on another without moving a private key: add the other
machine's PUBLIC key to the trust set, and its signatures verify here too (sign once, the
whole fleet enforces). Remove it to revoke.

```sh
# machine B runs `flint init`, then hands you its public key
flint fleet add    --config ~/.flint/flint.toml --pubkey machine-b.pub --label laptop
flint fleet list   --config ~/.flint/flint.toml
flint fleet remove --config ~/.flint/flint.toml --label laptop
```

Private keys never move — only public keys join the set.

## What enforces where

| Rule kind | Claude Code | Codex | Grok |
|---|---|---|---|
| `command` (regex over a shell command) | **block** (PreToolUse hook) | **block** (PreToolUse hook) | **block** (PreToolUse hook) |
| `path` (glob over a write target) | **block** (PreToolUse hook) | advisory (`AGENTS.md`) — apply_patch enforcement is flaky | **block** (PreToolUse hook, `write`/`search_replace`) |
| `advisory` (guidance into agent context) | `.claude/rules/flint-advisory.md` | `AGENTS.md` block | — (not compiled; Grok user rules are a prompt-layer surface) |

On Claude Code and Codex both blocking tiers reach the agent through the hook's
`permissionDecision` JSON, and a `warn` through `additionalContext`. Grok honours a different
envelope — `{"decision":"deny","reason":…}` — and ignores `permissionDecision` entirely
(measured 2026-08-20 on grok 1.0.5), so the gate forks the output shape per harness; on Grok
a `warn` still proceeds and is receipted, but its text is measured-undelivered to the model.
Flint deliberately does **not** block via a non-zero exit
code, even though that is the documented contract: measured 2026-08-08, codex on Windows
classifies a non-zero PreToolUse exit as a hook *error*, delivers nothing to the model, and
runs the command — while the identical binary and verdict block correctly on macOS. The JSON
channel was verified to block and to deliver its full reason on Claude Code/macOS,
codex/macOS and codex/Windows. Every judged action leaves a redacted receipt (`flint canon
list` shows the active signed set; the obs log shows per-action receipts). The binary is
per-OS; your `.md` rules and skills are portable.

A receipt records the **judgment**, not the harness's **obedience** — that gap is exactly how
the Windows behaviour hid for two weeks behind receipts identical to the ones on a machine
where the gate really was blocking. Enforcement is only ever proven by watching a command
not run.

## Status

The judge, the signed Canon, the law lifecycle (`init` → `law accept`), the compiler, the
pit store, bring-your-own memory, and the fleet keyring run end to end. Claude Code enforcement (command + path + advisory) is live; Grok enforcement
(command + path) is live on macOS and Windows; the Codex adapter is partial (command rules
enforce; file-scope governance rides `AGENTS.md` advisory). See
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the full status table. Part of the
[PunkGo](https://punkgo.ai) family.

## License

MIT © PunkGo
