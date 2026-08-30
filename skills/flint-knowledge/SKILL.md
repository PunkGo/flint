---
name: flint-knowledge
description: Triage flint's raw-ore inbox into durable knowledge — review the pending gists captured across your ore stores, then promote the keepers into 精矿 notes, toss the noise, defer the rest. A thin wrapper over the `flint knowledge` verbs; promotion is always the owner's call, never automatic. Use when the user says /flint-knowledge, "triage my pits", "review my gists", "升维", "promote a pit", "clear the flint inbox".
disable-model-invocation: true
argument-hint: "[optional: a keyword to focus the review]"
allowed-tools: Bash(~/.flint/bin/flint knowledge *), Bash(flint knowledge *), Read
---

# /flint-knowledge — triage the raw-ore inbox into durable knowledge

Capture is one funnel: while you work, walls and worth-keeping notes land as one-line
gists in your ore store(s) — either the agent marks them (`flint pit mark`, on by default
via the `[capture]` nudge) or you do. **Capture never chooses a destination.** This skill
is where you route each gist: the four exits, all human-decided. Nothing is ever
auto-promoted — a gist bears no weight until you promote it, and even then a 精矿 note is
knowledge, never an enforced rule (to enforce, write a rule and `flint canon pick`).

Config: `~/.flint/flint.toml` unless the user names another (`--config <path>`). The
promoted notes land in `knowledge_root` — bare markdown you own; delete flint and they
survive.

## The four exits (you propose, the owner decides per gist)

| Exit | Meaning | Mechanics |
|---|---|---|
| **promote** | Worth keeping | `flint knowledge promote --from <store> --gist "<exact text>" --id <slug>` — writes a 精矿 note and resolves the gist out of the inbox |
| **toss** | Not worth keeping | `flint knowledge toss --from <store> --gist "<exact text>"` — drops it from the inbox |
| **defer** | Decide later | leave it (it stays pending for the next pass) |
| **rule** | You want it machine-enforced | out of scope here — author a rule under `~/.flint/canon/rules/` and `flint canon pick`; a pit only teaches, it never auto-becomes a rule |

## Steps

1. **Review.** `flint knowledge review --config <cfg>`. It lists the pending gists across
   every ore store, each with its store `label` and `index` (the active write-target
   store is tagged). Empty → say so and stop.

2. **Propose an exit per gist.** For each pending gist, suggest promote / toss / defer with
   a one-clause reason, and for a promote a kebab-case `id` (short + specific:
   `dirty-tree-epoch-reset`, not `bug1`) and a one-line title. Present them all together so
   the owner can scan and answer once (e.g. "1 promote as X, 2 toss, 3 defer"). Never
   auto-decide — you draft, the owner picks.

3. **Execute the decisions.**
   - **promote** → `flint knowledge promote --config <cfg> --from <label> --gist "<exact
     gist text>" --id <slug> --title "<title>"`. Select by `--gist` (the exact text from
     review) — it is STABLE as other gists resolve out, unlike `--index`. The gist seeds the
     note body; the owner can edit the resulting `<knowledge_root>/<slug>.md` afterward (it
     is bare markdown). Use `--body "<text>"` to promote refined text instead of the raw gist.
   - **toss** → `flint knowledge toss --config <cfg> --from <label> --gist "<exact gist text>"`.
   - **defer** → do nothing; it stays in the inbox.
   - Prefer `--gist` over `--index`: gists shift position as you resolve them, so a stable
     text match can't act on the wrong one. `--index` remains for quick one-off use.

4. **Report.** One line: N promoted / tossed / deferred, and where the notes landed. No
   plans, no padding.

## Discipline (do not violate)
- Propose, never auto-classify or auto-promote — promotion is the owner's decision (人裁).
- A promoted note is knowledge, never a signed rule; nothing here is enforced.
- Tossing is a valid outcome. A thin honest note beats a padded one.
- The knowledge store is the owner's bare markdown — flint only writes it, never owns it.
