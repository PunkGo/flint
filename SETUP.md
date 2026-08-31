# SETUP.md — installing and operating Flint, written for agents

You are an agent, and your operator wants Flint installed or working on this machine.
This file is the manual for that job. A human can follow it too, with one exception
called out where it happens (step 6 must run inside an agent session).

The ideas behind Flint are in [`README.md`](README.md); the map of the code is in
[`ARCHITECTURE.md`](ARCHITECTURE.md). Working on Flint's own source? That is
[`CONTRIBUTING.md`](CONTRIBUTING.md)'s territory.

## What you are installing

Flint is the mechanism by which **your operator's rules — signed by them — govern your
tool calls**: warn, critique, or block, at the moment of action. A full install puts
this and only this outside the repo you cloned:

| Artifact | Where | What it is |
|---|---|---|
| one binary, `flint` | wherever you put it on `PATH` | judge + compiler + CLI; no server, no daemon, no telemetry, no network calls |
| a flint home | `~/.flint` by default | the operator's Ed25519 key (`keys/`, dir `0700`, key `0600`), the rule canon (`canon/rules/`, plain markdown), `flint.toml`, the receipt log, the capture inbox |
| hook wiring | only the harness configs the operator names — `~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.grok/hooks/flint.json` | a PreToolUse entry pointing that harness at the `flint hook` judge |
| advisory text (optional) | `.claude/rules/flint-advisory.md`, a marked block in `~/.codex/AGENTS.md` | `advisory`-kind rules compiled into agent context — guidance, not a gate |

Cloning and building also leave a source tree and a `target/` directory in the repo;
the optional workflow-skills suite adds `~/.flint/installed.lock`. Removal is
[Uninstall](#uninstall) below.

## Before you start — brief the operator

Get their go-ahead on all of this before touching anything:

1. **Prerequisites** — Rust **1.85+** (`rustc --version`), `git`, `ssh-keygen`, and a
   JSON tool for step 5 (`jq`, or you edit JSON yourself). If any is missing, agree how
   to install it before proceeding; do not install a toolchain unasked.
2. **Which harnesses to wire** — Claude Code, Codex, Grok, or a subset. Those config
   files gain a Flint entry; you back them up first.
3. **One step is personally theirs** — signing (step 4). You will stop and hand them a
   command. Setup does not continue until they have run it.
4. **Rules are their choice** — the canon starts empty. They pick from `examples/laws/`
   or write their own. Nothing is ever enforced that they did not sign.
5. **What is NOT part of a basic install** — say so, and only do these if asked: the
   workflow-skills suite, a memory vault, fleet trust for a second machine.
6. **Is Flint already here?** Run `command -v flint` and `ls ~/.flint` first. If either
   exists, this is a re-install or an upgrade: report what you found and ask how to
   proceed rather than running `init` blind. (`init` is idempotent and never overwrites
   an existing key or config — but the operator should know which case they are in.)

## The sovereignty line — read this first

**propose ≠ pick.** You may propose anything: draft rules, copy samples, lint, wire
hooks, verify, troubleshoot. **Picking — the signature — is the operator's hand on the
key.** The signing commands (`flint law accept`, `flint canon pick`) are theirs to run
personally: prepare everything, hand them the exact command, and wait. This holds even
if they offer you permission in chat — an agent that signs is a model editing its own
constitution, the one thing Flint exists to prevent. Decline, say why in one line, and
hand the command back.

Nothing in the software stops you from running those commands: you run as the operator
and the key is readable by you. That is the point — this is a discipline, not a
sandbox, and it holds because you keep it.

Bypasses follow the same shape. Some rules name an exempt form (an env-var prefix, a
declared reason). Use it only when the rule text's own stated condition is actually
true of what you are doing, or when the operator directs it — and say which of the two
it was. Every exempt passage lands in the receipt log, where bypass habits are audited.
When a gate blocks you, the move is in [Working under Flint](#working-under-flint).

## Setup state — fill this in before you touch anything

Every command below uses these. Resolve them once, from the operator's answers, and
substitute them literally; do not assume `~/.flint` if they chose another home.

```sh
REPO="$HOME/src/flint"          # where you cloned it
FLINT_HOME="$HOME/.flint"       # `init --home` if the operator wants another
CFG="$FLINT_HOME/flint.toml"    # written by init
KEY="$FLINT_HOME/keys/sovereign_ed25519"
HARNESSES="claude codex grok"   # only the ones the operator named
```

`--scope <name>` (an instance namespace that stops one machine's signed canon being
replayed on another) defaults to `local`; leave it unless the operator runs several
independent flint homes.

## Install

Each step ends on a check. Run the check; move on only when it holds.

**0. Preflight.**

```sh
rustc --version && git --version && command -v ssh-keygen && command -v jq
```

*Done when* the first two print versions and the last two print a path. (`ssh-keygen`
has no portable `--version`; presence is the check.)

**1. Install the binary.** Both paths compile from source — that is deliberate for a
tool like this: you run a gate you built yourself, from a tree you can read. `cargo
install` handles placement, so there is no symlink or `sudo` to reason about: `flint`
lands in `~/.cargo/bin`, which a Rust toolchain already puts on `PATH`.

Full — what the rest of this manual assumes, because it needs the repo for the sample
laws (and, if wanted, the suite manifest):

```sh
git clone https://github.com/PunkGo/flint "$REPO" && cd "$REPO"
cargo install --path crates/flint-cli --locked
```

Binary only — no samples, no suite; the operator writes their own rules:

```sh
cargo install --git https://github.com/PunkGo/flint flint-cli --locked
```

*Done when* `flint --version` prints `flint <version> (<git-hash>)` from a directory
other than the repo. Upgrading later is the same command with `--force`; there is no
published release channel yet, so the git tree is the distribution.

**2. Init.**

```sh
flint init --home "$FLINT_HOME"          # add --scope <name> only if agreed
```

This generates the operator's sovereign Ed25519 key (the private key never leaves this
machine) and scaffolds an **empty** canon — Flint ships the mechanism, never the rules.
It never overwrites an existing key or config.

*Done when* the output line `canon ... (empty — rules are yours ...)` appears and
`ls "$FLINT_HOME/canon/rules"` is empty.

**3. Propose rules.** Read the frontmatter `description:` of each sample in
[`examples/laws/`](examples/laws/), present that list to the operator in their own
terms, and copy **only** what they choose:

```sh
cp "$REPO"/examples/laws/<chosen>.md "$FLINT_HOME/canon/rules/"
flint law list --config "$CFG"
```

They may also want their own rule; the format is in `examples/laws/README.md`.

*Done when* `law list` shows every chosen rule as `proposed` **and nothing else** —
the canon directory must contain exactly what the operator agreed to, because the next
step signs what is there, not what you remember proposing.

**4. Hand over the signature.** Read back the exact `proposed` ids from step 3 so the
operator knows what they are signing, then give them the command and **wait**:

```sh
# one rule at a time — the honest default, and --name is not repeatable
flint law accept --name <rule-id> --config "$CFG" --key "$KEY"

# --all only when `law list` shows exactly the agreed set and nothing more
flint law accept --all --config "$CFG" --key "$KEY"
```

*Done when* the operator has run it and `flint canon list --config "$CFG"` prints the
accepted rules. (Before the first signature that command exits non-zero with `no
signed` — by design: nothing bears weight yet.) `canon list` proves a valid signature
exists, not who typed the command; that part is the discipline above.

`law accept` is `canon pick` scoped to one law's lifecycle — both end in the same act,
a re-signed `CANON.manifest`. Use `law accept` for adopting a rule, `canon pick` after
editing rule files.

**5. Wire the harnesses.** `flint compile` **prints** wiring; it does not write harness
configs. Back up, merge, validate — per harness the operator named.

Grok has its own file, so it is a plain write:

```sh
mkdir -p ~/.grok/hooks
flint compile --harness grok --config "$CFG" | grep -v '^#' > ~/.grok/hooks/flint.json
```

Claude Code and Codex share a config with other tools, so merge instead of overwrite.
This is idempotent — it drops any previous Flint entry before adding the current one:

```sh
TARGET="$HOME/.claude/settings.json"    # Codex: "$HOME/.codex/hooks.json"
FRAG="$(mktemp)"
flint compile --harness claude --config "$CFG" | grep -v '^#' > "$FRAG"
if [ -f "$TARGET" ]; then cp "$TARGET" "$TARGET.bak-$(date +%Y%m%d-%H%M%S)"; else mkdir -p "$(dirname "$TARGET")"; echo '{}' > "$TARGET"; fi
jq --slurpfile frag "$FRAG" '
  .hooks.PreToolUse = (
    [ (.hooks.PreToolUse // [])[]
      | select([ (.hooks // [])[].command ] | any(contains("flint hook")) | not) ]
    + $frag[0].hooks.PreToolUse )
' "$TARGET" > "$TARGET.new" && mv "$TARGET.new" "$TARGET"
python3 -m json.tool "$TARGET" > /dev/null && echo "valid JSON"
```

If the operator signed any `advisory`-kind rules, also write the advisory text — this
is what puts the guidance into agent context:

```sh
flint compile --harness claude --config "$CFG" --target-dir "$HOME/.claude"
flint compile --harness codex  --config "$CFG" --target-dir "$HOME/.codex"
```

*Done when* each target parses as valid JSON, contains exactly one `flint hook` entry,
and the backup file exists. That proves the file is well-formed and wired — not that
the harness obeys it. Step 6 proves that.

**6. Live-fire proof.** **receipt ≠ enforcement:** a receipt records Flint's judgment,
not the harness's obedience. Enforcement is proven only by watching a command not run.

This step must run inside a **new** agent session of the wired harness (hooks load at
session start, so the session that did the install cannot test itself). If you are the
installing agent, hand the operator this step and have them start a fresh session.

The probe must be **read-only by construction** — you are deliberately running a
command that will execute if the gate is broken, so it must be harmless when it does.
Never probe with a write, a delete, or anything touching real paths.

With `lsp-over-grep` accepted (a `command` rule; its probe is a read-only search):

```sh
printf 'CONTROL_OK\n'                    # control: matches no rule — must print
grep -n TODO src/main.rs                 # gated: matches the rule — must NOT run
```

For a different rule, build the probe the same way: the shortest read-only command that
its `pattern` matches. Expected outcome per rule kind and harness:

| Rule kind | Claude Code | Codex | Grok |
|---|---|---|---|
| `command` | blocks, reason delivered | blocks, reason delivered | blocks, reason delivered |
| `path` | blocks | **SKIP — advisory only, will not block** | blocks (`write` / `search_replace`) |
| `advisory` | **SKIP — guidance, never a gate** | **SKIP** | **SKIP — not compiled for Grok** |

*Done when* `CONTROL_OK` printed, the gated probe did **not** run, and the rule's reason
text came back as the block message. A `warn`-tier rule proceeds by design (and on Grok
its text is measured-undelivered) — do not read either as a failure. If the control
probe did not print, the session or shell is broken and this run proves nothing: fix
that before drawing any conclusion.

## Working under Flint

- **A block is the operator's judgment landing, not an obstacle.** `warn` proceeds and
  hands you context; `critique` blocks and names a recovery path — follow it; `block` is
  a hard freeze — stop and tell the operator. Satisfy the rule rather than rephrasing
  the command until the matcher misses: every verdict is receipted, and dodges read
  exactly like what they are.
- **Which list is truth:** `flint law list` reads the working tree; `flint canon list`
  reads the signed manifest — **enforcement follows only the signed manifest.**
- **Editing rules:** editing a signed rule file breaks its hash, and the whole set fails
  closed until it is re-signed — which freezes every governed session on this machine.
  So do not edit the live canon speculatively: prepare the edit only when the operator is
  ready to review and sign it, then hand over `flint canon pick --config "$CFG" --key
  "$KEY"` in the same breath. Gate rules take effect on the next hook call after the
  pick; advisory rules additionally need their `compile --target-dir` re-run to reach
  agent context.
- **Capture walls.** When you hit a wall or learn something worth keeping:
  `flint pit mark --config "$CFG" "<gist>"`. Nothing is auto-promoted; the operator
  triages with `flint knowledge review`.
- The verb surface is discoverable: `flint --help`, and `--help` on any subcommand.

## Optional — only when the operator asks

### A second machine

Rule `.md` files are portable; a signature is per-key; the binary is per-OS (build it
natively on each machine). Two ways to run the same rules elsewhere:

- **Each machine signs for itself.** Copy the rule files over, run this install there,
  and the operator signs there too. Prove the two agree by comparing the `file <sha256>`
  lines of both `CANON.manifest`s — byte-for-byte; equal file *counts* prove nothing.
- **Fleet keyring.** Register machine B's PUBLIC key in the trust set here, and a Canon
  signed there verifies here too — sign once, the whole fleet enforces.

```sh
# on machine B, `flint init` writes <home>/keys/sovereign_ed25519.pub — copy that file here
flint fleet add    --config "$CFG" --pubkey machine-b.pub --label laptop
flint fleet list   --config "$CFG"
flint fleet remove --config "$CFG" --label laptop     # revoke
```

Private keys never move between machines — only public keys join the set. **Both `add`
and `remove` change who can sign rules that are enforced here: propose, name the
machine, and let the operator confirm before you run either.** A `remove` can
immediately invalidate a canon that machine signed.

### Memory vault

Flint's product is the gate; memory is an add-on it does not own — flint keeps no vault
of its own, so point it at markdown the operator already has (an Obsidian vault, a wiki,
a notes folder), or let it scaffold a plain one:

```sh
flint init --home "$FLINT_HOME" --with-memory --vault <dir>   # also valid on an existing home
flint memory capture --config "$CFG" "what you just learned"
flint memory list    --config "$CFG"
```

The vault is knowledge: never signed, never judged, never turned into a rule by anything
but the operator writing one. `flint memory scaffold` / `orient` / `resolve` complete the
verbs.

### Workflow-skills suite

Flint can install its own workflow skills (`/flint-knowledge`, `/flint-status`) from
`scripts/manifest.toml`:

```sh
cd "$REPO" && scripts/bootstrap.sh     # Windows: scripts\bootstrap.ps1
flint install --check                  # drift report
flint install --plan                   # dry run, writes nothing
```

Run `flint install` from the repo root — the manifest paths are relative. It writes only
inside `~/.claude` / `~/.codex` / `~/.flint`, and records `~/.flint/installed.lock` so
removal is honest. `--stage full` additionally renders harness bindings from the signed
canon and installs a Codex `SessionStart` hook pinned to the repo's current Git `HEAD`:
that quiet auto-sync re-runs only while the repo is clean, on `main`, and still at the
approved commit — it never pulls, rebuilds, or adopts a newer commit. After the repo
moves, re-run the non-quiet full install and have the operator re-trust the hook in
Codex `/hooks`.

## Uninstall

There is no `flint uninstall` verb — removal is three explicit moves, each auditable.
**Step 2 is irreversible; get the operator's confirmation before it.**

1. Remove the Flint entry from every harness config it was merged into
   (`~/.claude/settings.json`, `~/.codex/hooks.json`, delete `~/.grok/hooks/flint.json`),
   plus the advisory files if written. Suite artifacts are listed in
   `~/.flint/installed.lock`; dropping an entry from the manifest and re-running
   `flint install` removes that artifact for you.
2. Delete the flint home (`~/.flint`). **This destroys the operator's signing key and
   their receipt log — irreversible. Offer to copy `canon/rules/` out first: those are
   their rules, in portable plain text that outlives Flint.**
3. Remove the binary from `PATH`, and the cloned repo if they want it gone.

## Troubleshooting

- **Every call suddenly denied** → a rule file changed after signing (hash mismatch →
  fail-closed, by design). Hand the operator a re-pick.
- **A `flint/v1` rule saying `warn` refuses to load** → by design: `warn` changed meaning
  between schemas, and a signature covers bytes, not readings. The error names the fix
  (`schema: flint/v2`); edit, then hand over the re-sign.
- **A path rule did not block on Codex** → expected, not a bug: path rules ride
  `AGENTS.md` advisory there. See the
  [enforcement matrix](README.md#what-enforces-where).
- **The hook does not fire at all** → hooks load at session start; a config merged
  mid-session takes effect in the next one. Check the target file parses and holds
  exactly one `flint hook` entry.
