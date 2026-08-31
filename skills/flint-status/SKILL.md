---
name: flint-status
description: Receipt summary from the flint observation log — which of YOUR rules are governing, verdict/harness distribution, and the recent critique/deny detail (rule + time). Read-only. Use when the user says /flint-status, "which rule fired", "哪条 rule 在管", "flint 摘要", "receipt summary", or wants to sanity-check the gate after a config/canon change.
argument-hint: "[optional: days to look back, default 7]"
allowed-tools: Bash, Read
---

# /flint-status — see which of your rules are governing

The whitepaper's promise: a receipt tells you WHICH of your rules acted, so behavior is
never a black box. This skill is the hand-held readout of that ledger. **Read-only** —
it never writes, never changes canon or config.

**Honesty line (do not oversell):** receipts for command-shaped actions are
core-redacted — they carry `harness / rule_id / scopes / tool_kind / ts / verdict` and
NO command content. The summary can say *which rule, when, how often* — it cannot and
should not reconstruct *what command*. Don't call this "explainability".

## Inputs

- Config (`$FLINT_CFG` below): `~/.flint/flint-global.toml` if it exists, else
  `~/.flint/flint.toml` — some machines have only the latter, and the live
  gate's obs log is whichever config exists. Read `obs_log` from `$FLINT_CFG` for the log
  path. If neither config exists, or the obs log is missing/empty → say so and stop; that
  itself is a finding (gate not recording = check hooks).
- Window: `$ARGUMENTS` days if given, else 7. Receipts carry unix-seconds `ts`.
- Receipt schema (`flint-receipt-v1`, one JSON object per line):
  `{"schema","ts","harness","tool_kind","scopes":[],"context","verdict","rule_id","energy"}`.
  `rule_id` is null on `affirm` and carries the rule's id on `warn` / `critique` / `deny`
  (and on `exempt`, naming the rule that was bypassed). `context` is empty for
  command-shaped actions — that is the redaction — and otherwise holds up to 200
  characters of caller context, so do not describe the log as content-free in general.
- Verdict tags, all five: `affirm` (no rule matched) · `warn` (proceeded, with context) ·
  `critique` (blocked, recovery path) · `deny` (blocked, hard freeze) · `exempt` (a rule's
  declared escape hatch fired and was recorded). A summary that drops `warn` or `exempt`
  hides exactly the two things worth watching: the rules that only ever advise, and the
  bypasses becoming a habit.

## Steps

1. **Aggregate** over the window (use whatever tool the environment has — jq, python3,
   awk all work; do NOT require one specific tool):
   - total receipts, and per-verdict counts across all five tags: affirm / warn /
     critique / deny / exempt (they must sum to the total — if they do not, say so rather
     than papering over it);
   - per-harness counts (claude / codex / grok) — any harness at zero while the others
     flow is a wiring smell (hook not trusted / not installed), surface it;
   - per-rule counts for warn + critique + deny + exempt (which rule is actually
     governing, and which one is mostly being waved through).
2. **Recent enforcement detail**: the last 10 critique/deny receipts as a short table —
   local time, harness, rule_id, verdict. There is no command column — command text is
   never recorded — so do not invent one; `scopes` / `context` are all you have.
3. **Report** one compact block:
   - headline: `N receipts (last D days) — A affirm / W warn / C critique / D deny /
     E exempt`;
   - rule breakdown with counts;
   - harness split;
   - the recent-detail table;
   - one honest note if anything looks off (zero receipts from a live harness, a rule
     that stopped firing after a canon change, deny spikes).
4. **Never interpret beyond the ledger.** If the user asks "why did it deny X", the
   answer is the rule id + that rule's text (`flint canon list --config $FLINT_CFG`) —
   not a reconstruction of the denied content.

## Discipline
- Read-only; no writes, no config edits, no canon changes.
- Report what the receipts show, including gaps. A quiet log is a report, not an error
  to paper over.
