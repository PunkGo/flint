---
schema: flint/v1
id: stop-framing-loop
type: rule
kind: advisory
status: proposed
version: 1
created: 2026-07-06
description: Ship a file before framing again — never iterate framing/naming/scope past 3 rounds with zero commits.
source.kind: human
scope: global
trigger: framing-iterate, rethink-architecture, reframe-scope, one-more-round, naming-loop, deep-dig-one-more-layer
tags: iron-law, discipline
---
Never framing-iterate more than 3 rounds without shipping a file. Re-formulating a problem, naming, or scope while nothing has shipped destroys value past round 3 — the pull toward "deeper framing" is a failure mode, not depth. When you notice 3 rounds with 0 commits: halt the current framing branch, identify the smallest file that captures your current best understanding (300-900 words), commit it as-is even if imperfect, then iterate against real feedback — not against your own taste loop. Not when: rounds 1-2 of a genuinely new problem, a shipped file revealed a specific flaw the next round fixes, or a new external constraint (spec changed, ask changed) appeared.
