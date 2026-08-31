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

**To install it, paste this to your coding agent** — Claude Code, Codex, Grok:

```text
Read https://github.com/PunkGo/flint/blob/main/SETUP.md and set Flint up on this
machine. Follow it exactly. Stop at the signing step and hand me that command —
signing is mine.
```

It clones, builds, wires your harnesses, and proves the gate really blocks a command.
The one thing it will not do is sign: that command comes back to you.

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

A rules file — `CLAUDE.md`, `AGENTS.md`, `.cursorrules` — does the first two actions and
stops there, because context is advice: read when convenient, drifting as the session
fills, checked by nothing. Flint keeps that layer, and adds the two it cannot reach.

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

## Setup

[`SETUP.md`](SETUP.md) is the whole procedure — install, wiring, day-to-day, uninstall.
It is written for the agent doing the work (that is what the paste block above is), and
it reads as a human runbook just as well: every step is a command plus the check that
proves it worked, and the one step no agent may take is marked.

You need **Rust 1.85+**. There is no published release channel yet — the git tree is the
distribution, and `cargo install` compiles it, which for a gate is the point: you run
what you can read.

It touches three things: a binary on your `PATH`, a `~/.flint` directory holding your key,
your rules and your receipts, and one hook entry merged into the config of each agent you
name — backed up first. Removing it is those three in reverse.

```sh
cargo install --git https://github.com/PunkGo/flint flint-cli   # puts `flint` on PATH
```

That gets you the binary alone. Clone instead if you want the sample rules and the
workflow skills with it. The binary is native to one OS/arch — install it on each
machine you use; your rule `.md` files are the portable part.

### What a rule looks like

Your canon is a folder of files like this — plain text you can read, diff, and carry:

```markdown
---
schema: flint/v1
id: no-secrets-dir
type: rule
kind: path
description: Never write into secrets/ — use a vault or a keychain pointer.
glob: secrets/**
response: block
reversibility: irreversible
---
Do not write into secrets/. Use a vault / Keychain pointer.
```

(`kind: command` matches a regex over a shell command; `kind: advisory` is a guideline
compiled into agent context rather than a gate.) Sample rules grown from real practice
live in [`examples/laws/`](examples/laws/) — copy, read, then sign: nothing bears weight
until you accept it (`propose ≠ pick`, the sovereignty line). Every field, and every
`flint.toml` key, is tabulated in [`docs/reference.md`](docs/reference.md).

## Using it day to day

- **Enforcement is automatic.** Once wired, every matching tool call is judged — no
  per-call ceremony. Hooks load at session start, so a fresh wiring takes effect in the
  next session, and what each harness can actually enforce is the matrix below.
- **A rule grows from practice.** Hit a wall, capture it, and when a note earns it, turn it
  into a rule — `flint canon pick` again to sign the change in.
- **Capture walls, then refine them into knowledge.** While you work, walls and
  worth-keeping notes land as one-line gists — the agent marks them itself by default (the
  `[capture] auto_mine` nudge; turn it off any time), or you run `flint pit mark --config ~/.flint/flint.toml "<gist>"`.
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

Private keys never move — only public keys join the set. Both `add` and `remove` change
who can sign rules enforced here, so both are decisions worth making deliberately.

## What enforces where

| Rule kind | Claude Code | Codex | Grok |
|---|---|---|---|
| `command` (regex over a shell command) | **block** (PreToolUse hook) | **block** (PreToolUse hook) | **block** (PreToolUse hook) |
| `path` (glob over a write target) | **block** (PreToolUse hook) | advisory (`AGENTS.md`) — apply_patch enforcement is flaky | **block** (PreToolUse hook, `write`/`search_replace`) |
| `advisory` (guidance into agent context) | `.claude/rules/flint-advisory.md` | `AGENTS.md` block | — (not compiled; Grok user rules are a prompt-layer surface) |

Claude Code is the only harness where all three rule kinds enforce, which is why it is
the worked example in [`SETUP.md`](SETUP.md) — coverage, not preference. Wire whichever of
the three you actually use; the wiring differs per harness and is not interchangeable.

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
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the full status table.

**Maturity, plainly.** Flint has governed the author's own agents daily for months, on
macOS and Windows. As of this first public release, nobody else has installed it. The code
is tested on Linux, macOS and Windows in CI, but the install path has only been walked by
hand on macOS. If you are trying it now you are early — and the most useful thing you can
report back is the one thing a receipt cannot tell you: whether your harness actually
refused to run the command.

Part of the [PunkGo](https://punkgo.ai) family.

## License

MIT © PunkGo
