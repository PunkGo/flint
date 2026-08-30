---
schema: flint/v1
id: verify-before-claiming
type: rule
kind: advisory
status: proposed
version: 1
created: 2026-07-06
description: Run a cheap check before asserting external state — no memory guesses, no story-building on one data point.
source.kind: human
scope: global
trigger: i-remember, should-be, looks-like, its-probably, plan-says, tests-pass, review-cleared, already-fixed, before-reporting-status
tags: iron-law, discipline
---
Before asserting any external state — code field names, library versions, bug root cause, external API shape, tool behavior, config defaults — run one cheap verification: LSP/grep, the README, a web search, or a controlled experiment. This also applies to completion claims by other actors or a past self: "the plan says X exists", "review cleared", "tests all green", "this fix should resolve it", "we already did Y" are claims, not facts, until you check the code / fixture / prior fix against truth. Inside-context review shares the author's blind spots; cross-model, outside-voice, fresh-eyes verification is the cure. Not when: code you wrote this session (state is in context), a deterministic reproducible bug (diff/bisect directly), a memory fact you verified <5 min ago this session, or an explicit user override. On violation, don't amputate prior work — stop, verify the current claim, and backtrack if it was wrong; no sunk-cost.
