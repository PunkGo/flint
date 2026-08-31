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
| context files | `<target-dir>/rules/flint-advisory.md` for Claude, a marked block in `<target-dir>/AGENTS.md` for Codex — `$HOME/.claude` and `$HOME/.codex` for a machine-wide install | `advisory` rules as guidance, and on Codex the `path`-rule governance too, since those do not enforce through its hook |

Cloning and building also leave a source tree and a `target/` directory in the repo;
the optional workflow-skills suite adds `~/.flint/installed.lock`. Removal is
[Uninstall](#uninstall) below.

## Before you start — brief the operator

Get their go-ahead on all of this before touching anything:

1. **Prerequisites** — Rust **1.85+** (`rustc --version`), `git`, `ssh-keygen`, and a
   JSON toolchain for step 5 (`jq` and `python3`). If any is missing, agree how
   to install it before proceeding; do not install a toolchain unasked.
2. **Which harnesses to wire** — Claude Code, Codex, Grok, or a subset. Those config
   files gain a Flint entry; you back them up first.
3. **One step is personally theirs** — signing (step 4). You will stop and hand them a
   command. Setup does not continue until they have run it.
4. **Rules are their choice** — the canon starts empty. They pick from `examples/laws/`
   or write their own. Nothing is ever enforced that they did not sign.
5. **What is NOT part of a basic install** — say so, and only do these if asked: the
   workflow-skills suite, a memory vault, fleet trust for a second machine.
6. **Is Flint already here?** Run `command -v flint`, and list the home the operator
   intends to use (`$FLINT_HOME`, not necessarily `~/.flint`) first. If either
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
independent flint homes. Every key `flint.toml` accepts is tabulated in
[`docs/reference.md`](docs/reference.md) — read it there rather than guessing, since an
unknown key is a hard load error.

## Install

Each step ends on a check. Run the check; move on only when it holds.

**0. Preflight.**

```sh
rustc --version && git --version && command -v ssh-keygen && command -v jq && command -v python3
```

*Done when* `rustc` prints **1.85 or newer** — read the number, do not just check that the
command succeeded — `git` prints a version, and the last three print a path. (`ssh-keygen`
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

They may also want a rule of their own: the complete frontmatter field
table is [`docs/reference.md`](docs/reference.md). Two traps it names — an
unrecognized key fails the whole canon closed, and an omitted `status:` counts as
`accepted` — are the ones worth reading before you draft anything.

Fewer is better on a first install. These samples are one person's rules and carry that
person's taste; the operator has felt none of the walls they came from yet. Propose a
small set they can judge — `secret-zero` is the usual first pick — and let them add more
when a need shows up in their own work.

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

**5. Wire the harnesses.** `flint compile` **prints** wiring; it never writes a harness
config. The loop below is the whole step: it iterates `$HARNESSES` — the list the
operator actually named in the setup state, not a fixed set — and for each one backs up,
merges, and validates. The fragments are **not interchangeable**: Codex's carries two
matchers (`Bash` and `apply_patch`) where Claude's carries one, and each names its own
harness to the judge, so a Claude fragment in Codex's file leaves `apply_patch` ungated
and the adapter reading the wrong wire format, silently.

```sh
set -o pipefail        # a compile that fails mid-pipeline must not look like success

for HARNESS in $HARNESSES; do
  case "$HARNESS" in
    claude) TARGET="$HOME/.claude/settings.json" ;;
    codex)  TARGET="$HOME/.codex/hooks.json" ;;
    grok)   TARGET="$HOME/.grok/hooks/flint.json" ;;
    *)      echo "unknown harness: $HARNESS" >&2; continue ;;
  esac
  mkdir -p "$(dirname "$TARGET")"
  [ -f "$TARGET" ] && cp -p "$TARGET" "$TARGET.bak-$(date +%Y%m%d-%H%M%S)"

  # compile into a temp file first: never redirect straight onto the operator's config,
  # because the shell truncates the target before the command that fills it has run.
  # For codex, compile prints an AGENTS.md block after the JSON — take the first JSON
  # value and ignore what follows.
  FRAG="$(mktemp)"
  if ! flint compile --harness "$HARNESS" --config "$CFG" | grep -v '^#' \
       | python3 -c 'import json,sys; print(json.dumps(json.JSONDecoder().raw_decode(sys.stdin.read().strip())[0], indent=2))' > "$FRAG"; then
    echo "compile failed for $HARNESS — config untouched" >&2; continue
  fi

  if [ "$HARNESS" = grok ]; then
    cat "$FRAG" > "$TARGET"          # grok's hook file is flint's alone
  else
    [ -s "$TARGET" ] || echo '{}' > "$TARGET"
    MERGED="$(mktemp)"
    # Drop flint hooks from INSIDE each entry rather than dropping whole entries: another
    # tool's hook can share an entry with flint's, and removing the entry removes theirs.
    jq --slurpfile frag "$FRAG" '
      .hooks.PreToolUse = (
        [ (.hooks.PreToolUse // [])[]
          | .hooks = [ (.hooks // [])[] | select(((.command // "") | contains("flint hook")) | not) ]
          | select((.hooks | length) > 0) ]
        + $frag[0].hooks.PreToolUse )
    ' "$TARGET" > "$MERGED" || { echo "merge failed for $HARNESS — config untouched" >&2; continue; }
    cat "$MERGED" > "$TARGET"        # write through the existing inode: keeps its permissions
  fi

  python3 -m json.tool "$TARGET" > /dev/null && echo "$HARNESS: wired, valid JSON"
done
```

Then write the context files. This is **not** conditional on having signed `advisory`-kind
rules: on Codex the same `AGENTS.md` block is where **`path` rules are governed** (they do
not enforce through the hook there), so a Codex operator who signed only path rules and
skipped this step would have no path governance at all. It writes a marked block and
leaves the rest of an existing file alone. Grok has no advisory surface, so a Grok-only
install has nothing to write here:

```sh
for HARNESS in $HARNESSES; do
  case "$HARNESS" in
    claude) flint compile --harness claude --config "$CFG" --target-dir "$HOME/.claude" ;;
    codex)  flint compile --harness codex  --config "$CFG" --target-dir "$HOME/.codex" ;;
    grok)   echo "grok: advisory rules are not compiled for this harness" ;;
  esac
done
```

*Done when*, **for every harness in `$HARNESSES`**: the target parses as valid JSON; every
`flint hook` command in it names that same harness (`--harness codex` in Codex's file,
never `claude`); Codex's file carries both the `Bash` and `apply_patch` matchers; any hook
belonging to another tool is still present; and a `.bak-` file exists if the config did.
That proves the file is well-formed and wired — not that the harness obeys it. Step 6 is
the only thing that proves that.

**6. Live-fire proof — once per harness.** **receipt ≠ enforcement:** a receipt records
Flint's judgment, not the harness's obedience. Enforcement is proven only by watching a
command not run. Run this in a **new** session of *each* wired harness — hooks load at
session start, so the session that installed them cannot test itself, and a pass on one
harness says nothing about another. If you are the installing agent, hand this step to
the operator with the probe pair written out.

Two properties make a probe worth running. It must be **harmless if it executes**, since
a broken gate will execute it; and it must **print a marker when it executes**, or
"nothing happened" is indistinguishable from "the command ran and had nothing to say".
Pair every gated probe with a `printf` in the same command so the shell prints the marker
if and only if the whole call got through:

```sh
printf 'CONTROL_OK\n'                                    # matches no rule — must print

# gated: pick the form matching a rule the operator actually signed.
# secret-zero (the usual first pick) — the rule matches the TEXT, so echoing is enough:
printf 'postgres://user:notarealpassword@localhost/db\n'; printf 'GATED_PROBE_RAN\n'

# lsp-over-grep, if that was signed instead:
grep -rn PROBE src/main.rs; printf 'GATED_PROBE_RAN\n'
```

*Done when*, in each wired harness: `CONTROL_OK` printed, `GATED_PROBE_RAN` did **not**,
and the rule's own reason text came back as the block message. If `CONTROL_OK` never
printed, the session or shell is broken and this run proves nothing — fix that before
concluding anything.

Two cases need a different reading, and neither is a failure:

- **A `warn`-tier rule proceeds by design.** `GATED_PROBE_RAN` *will* print. What you are
  checking is that the rule's text reached you as context (and on Grok, that text is
  measured-undelivered — the receipt is the only evidence there).
- **A `path` rule cannot be probed read-only**, because it is a write that triggers it.
  Probe it in a throwaway directory with a disposable filename inside the glob
  (`secrets/flint-probe.tmp`, say). If the gate holds, the file does not exist afterwards;
  if it does exist, that file *is* the finding. Delete it either way. On Codex, skip this
  — path rules ride advisory there and will not block, which is expected, not a fault.

If a harness has no signed rule whose kind it can enforce (a Codex install carrying only
`path` rules, a Grok install carrying only `advisory` ones), say so plainly instead of
declaring the install proven: nothing was demonstrated for that harness.

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
inside `~/.claude` / `~/.codex` / `~/.grok` / `~/.flint`, and records
`~/.flint/installed.lock` so removal is honest. The workflow skills themselves install for
Claude Code and Codex; a Grok-only operator gets the gate, not the skills. `--stage full` additionally renders harness bindings from the signed
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
2. Delete the flint home (`$FLINT_HOME` — `~/.flint` only if that is what they chose).
   **This destroys the operator's signing key and their receipt log — irreversible. Offer to copy `canon/rules/` out first: those are
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
  mid-session takes effect in the next one. Check the target file parses, and holds one
  `flint hook` command per matcher its harness needs — one for Claude and Grok, **two for
  Codex** (`Bash` and `apply_patch`). Trimming Codex down to a single entry is how you
  silently ungate `apply_patch`.
