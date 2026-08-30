---
schema: flint/v1
id: authority-first
type: rule
kind: advisory
status: proposed
version: 1
created: 2026-08-20
description: At session start in a project directory, read AUTHORITY.toml — the repo-root pointer-level source of truth — before any other action; fall back to the memory index when absent.
source.kind: human
scope: global
trigger: session-start, project-dir, resume, continue, next-step, compaction-recovery, handoff, new-agent-onboarding, generate-authority, dead-pointer
tags: iron-law, session-init, ssot
---
AUTHORITY.toml is a stateless manifest at the repo root: the single source of truth for exactly three things — authority bindings (which file answers for architecture / agent instructions / task state / runbooks), the [recovery] read_order, and where state lives. It carries no state itself; it points at where state lives, which is why it cannot rot the way stateful portals did. It is TOML, not prose, so every agent of every model parses the same facts. When a session starts in a project directory and AUTHORITY.toml exists, the first action is the BOOT read: the manifest itself, the file its task_state pointer names, and the project memory index. The full [recovery] read_order fires only on recovery signals — compaction recovery (a summary marker at the top of the transcript), a cold handoff, or a new agent taking over. Action-bound, not phrase-bound: any start signal triggers it — "continue", "next step", a project name, a pronoun referencing current working state. Whether to read is not your judgment call.

When no AUTHORITY.toml exists, fall back to the project memory index (and the latest checkpoint if under 24h), and you MAY propose generating one from the minimal shape below. The generated file is a proposal: the human reviews the diff before it becomes truth — the registry that defines what is true is the last file a model may write for itself. Pointer duty: when generating or editing AUTHORITY.toml, mechanically verify every pointer resolves before handing it over; when reading, a dead pointer is a finding to report, never a line to skip silently. SSOT discipline: each pointed file is the sole home of its facts, and nothing restates a fact outside its home — the manifest exists to kill the two-truths disease.

Minimal shape:

```toml
[authority]
architecture_file = "ARCHITECTURE.md"
agent_instructions_file = "AGENTS.md"
task_state_file = "docs/PLAN.md"
runbook_index_file = "runbooks/README.md"

[recovery]
read_order = ["AGENTS.md", "ARCHITECTURE.md", "docs/PLAN.md", "runbooks/README.md"]

[state]
directory = ".state"
```

Extra bindings are allowed; the manifest stays stateless regardless. Not when: cwd is not a project dir, mid-session after the first read, or the user explicitly said to skip.
