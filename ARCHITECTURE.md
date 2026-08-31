# Flint — Architecture & Status

> 秉火前行. — *Carry the fire forward.*

This doc is the map for an **agent or contributor orienting on Flint**: what the
pieces are, where the code lives, the boundaries you must not cross, and what runs
today. For installation and day-to-day use see [`README.md`](README.md); for the
constitution — the invariants Flint will never let change — see
[`docs/whitepaper.md`](docs/whitepaper.md).

Flint is a small, local, harness-agnostic layer that keeps the rules you follow
**explicit, portable, enforced at the moment of action, and revised only when you
say so**. It is a Rust workspace (`flint-core` + `flint-cli`, binary `flint`), MIT,
part of the [PunkGo](https://punkgo.ai) family — but **kernel-agnostic and
independent**: it rides commodity harness hooks, not a kernel.

## The four actions (the whole skeleton — not more, not less)

1. **Write it down.** Your rules become versioned plain-text markdown you own (the
   **Canon**), not a vendor's opaque memory.
2. **Carry it across agents.** One Canon **compiles** into each harness's native
   format (Claude Code session rules, Codex `AGENTS.md`, Grok hook wiring). Judgment
   follows *you*, not the tool.
3. **Enforce at the moment of action.** When an agent is about to act, the
   applicable rules are judged at the spot — **allow / warn / block**, before the
   action lands.
4. **Revise only when you say so.** Rules change through *your* review, on *your*
   evidence, signed with *your* key. Never silently; never by the model editing its
   own constitution.

**There is no fifth action.** The engineering task is making these four good — not
inventing a fifth. This is a design invariant, not a roadmap gap.

## Boundaries — what Flint will not do (the values, as guardrails)

Read this before changing anything. Each line is a guardrail an agent working on
Flint must hold:

- **It does not decide for you, or judge whether your rules are wise.** A nod
  through Flint means *"I allowed this,"* never *"I was right."* You will sometimes
  be wrong — the mistake is yours, visible, reversible. That is the point.
- **The constitution is bedrock.** The whitepaper invariants don't drift into
  "features." Rules grow from practice; the constitution does not.
- **The agent never auto-signs.** Auto-signing = the model owning its own
  constitution = the death of Flint. A human signs, by hand, every time. This is
  the one line that separates Flint from a plain text file.
- **Insights and external reports are reference, not gospel.** No importer, no
  mining logs into rules, no auto-distilling reports into Canon.
- **`flint-core` is frozen against memory-system growth (PIP-0001).** Buckets,
  retrospectives, auto-promotion, classify/distill — those organs belong to the
  owner's private memory instance, never to the judge. A smoke-alarm test
  (`freeze_gate.rs`) fails the build loud if a banned word-root appears in core
  source. See [`docs/PIP-0001-core-freeze.md`](docs/PIP-0001-core-freeze.md).
- **Nothing is auto-promoted.** A captured note is *knowledge*, never a rule —
  until a human writes a rule and signs it.
- **Open-source boundary.** The repo ships source, the whitepaper + PIPs, the sample
  laws in `examples/laws/`, the operating docs (README / SETUP / CONTRIBUTING /
  SECURITY / CHANGELOG / AUTHORITY.toml / `docs/reference.md`), `scripts/`, `skills/`,
  CI and assets.
  Internal design docs and the owner's private memory instance stay private (see
  `.gitignore`).

## Architecture

### Two crates

| Crate | Role |
|---|---|
| **`flint-core`** | The judge face. Kept deliberately small (PIP-0001) so *"what does the enforcement layer do"* stays a one-sitting read. |
| **`flint-cli`** | The command surface + carry-over plumbing — the "take it with you" side (compile, install, capture). Binary: `flint`. |

### `flint-core` modules

| Module | Responsibility |
|---|---|
| `canon.rs` | The signed rule source: frontmatter parse, sign/verify, epoch, the law lifecycle. |
| `touchstone.rs` | **The judge.** Match a tool call against the active Canon → `Affirm` / `Warn` / `Critique` / `Deny`. Four verdicts, three `response:` tiers — `Affirm` is "no rule matched", which no rule can request. Of the three tiers a rule can ask for, only `warn` lets the call proceed. |
| `trust.rs` | **Whose signature counts.** Ed25519 signature verification (namespace-pinned), the `allowed_signers` trust set, the fleet keyring, the anti-rollback epoch floor. A malformed or unsigned rule fails the whole set **closed** — never a silent half-apply. Key *custody* (generation, `0600`, permission re-checks) is `flint-cli`'s `init.rs`. |
| `verifier.rs` | Runs a rule's falsifier method and freezes the artifacts — **dormant ring**, serving `forge`; never consulted by a verdict. |
| `config.rs` | `flint.toml` parsing and ore-store (粗矿厂) resolution. |
| `harness.rs` | Per-harness hook-JSON shapes (Claude / Codex / Grok adapters). |
| `striker.rs` | **The compiler.** Renders per-harness hook wiring and the advisory text each agent reads — action 2, "carry it across". |
| `glob.rs` | Path-glob matching for `path` rules. |
| `pit.rs` | The pit store: the raw-ore capture inbox (mark a wall hot, save a gist cold). |
| `memory.rs` | The **bring-your-own-memory** port: an opt-in vault (scaffold / capture / list / orient / resolve). Memory is knowledge — never signed, judged, or injected. |
| `obslog.rs` | The append-only, **redacted** receipt log — which rule fired, what verdict; the raw command is never written. |
| `content_store.rs` | Content-addressed object store — **dormant ring**, used only by `verifier`. |
| `budget.rs` | Energy-budget accounting (reads real measured tokens; Flint never fabricates them). |
| `forge.rs` | Evidence-tier / load-bearing gate scaffold — a rule earns a `reproduced` tier by discriminating on a fixture. **Dormant outer ring, grandfathered & frozen by PIP-0001**: present, no new callers. |
| `model_veto.rs` | Layer-2 cross-vendor model veto — *"polygraph, not judge."* **Veto-only by construction** (can never Affirm / Authorize / Deny, nor up- or down-grade a verdict), `NoopVeto` by default: a dormant outer ring. Its very shape encodes the red line while dormant — a model may never be the judge. |

### `flint-cli` (the verbs)

Entry points: `main.rs` (dispatch); `hook.rs` + `codex_hook.rs` + `cross_vendor.rs`
— **the gate**: read a harness hook JSON on stdin, judge it against the signed
Canon, write a redacted receipt, and enforce the verdict in `--mode block`;
`install.rs` (suite install — idempotent diff-writes + `installed.lock` for honest
removal, targets confined to `~/.claude` / `~/.codex` / `~/.grok` / `~/.flint`);
`knowledge.rs` + `capture.rs` (the capture → refine loop); `init.rs` (bootstrap a
flint home); `fleet.rs` (cross-machine trust set). Verbs like `canon` / `law` /
`memory` / `pit` / `budget` / `compile` dispatch from here into `flint-core`.

### The lifecycle (verbs → the four actions)

```
propose        a rule .md lands in the canon — you write it, or copy a sample from
   │           examples/laws/ — unsigned (status: proposed)
   │           propose ≠ pick — nothing bears weight yet
pick / accept  you sign with your Ed25519 key: `canon pick` / `law accept`
   │
compile        per-harness hook wiring + advisory        (action 2: carry across)
   │
hook (gate)    at the moment of action: judge vs Canon → Affirm / Warn / Critique /
   │           Deny (or an audited Exempt), write a redacted receipt
   │                                                       (action 3: enforce)
revise         edit the .md + re-sign — never silent, never by the model (action 4)
```

Supporting verbs: `install` (suite); `pit` / `knowledge` / `memory` (capture →
`knowledge review` → `promote`; nothing auto-promoted); `key` / `fleet`
(custody, cross-machine trust); `budget`; and `forge` — a shipped verb of the
dormant ring, kept working but gaining no new capability.

### `AUTHORITY.toml` — the pointer-level source of truth

The repo root carries an `AUTHORITY.toml` (the *authority-first* convention): a
stateless manifest that binds which file answers for what — architecture, the
setup manual, constitution, contributing, security, runbooks — plus the `[recovery]` read_order an agent follows on cold
start or post-compaction. It points at where state lives and never carries state
itself, so it cannot rot the way stateful "portal" documents did. Every pointer
resolves from a fresh clone; machine-local state (harness memory) is declared,
not pointed. Any agent of any model parses the same facts — that is why it is
TOML, not prose.

## Status

| Area | State |
|---|---|
| Judge + signed Canon + law lifecycle (`init` → `law accept`) | **runs end to end** |
| Compiler (`compile`) + suite install (`install`) | **runs end to end** |
| Pit store, bring-your-own memory, fleet keyring | **runs end to end** |
| Knowledge layer (capture → `knowledge review` → `promote`) | **runs end to end** |
| Claude Code enforcement (command + path + advisory) | **live** (PreToolUse hook: block / critique) |
| Codex enforcement | **partial** — command rules enforce; file-scope governance rides `AGENTS.md` advisory (`apply_patch` enforcement is flaky) |
| Grok enforcement (command + path) | **live** — macOS and Windows, live-sentinel verified (PreToolUse hook, `{"decision":"deny"}` envelope — Grok ignores `permissionDecision`, measured 2026-08-20 on 1.0.5; warn text measured-undelivered; Windows wiring uses the PowerShell call operator) |
| Outer ring (`forge`, `model_veto`) | **dormant / frozen** — present, no new capability |

- **Verified live** — the gate itself — on macOS (Apple Silicon) and Windows (native +
  WSL), on the author's own machines. CI runs the suite on Linux, macOS and Windows; the
  install path has been walked by hand only on macOS. The binary
  is per-OS (build on each machine); your `.md` rules and skills are portable.
- **Tests:** the core `freeze_gate` plus CLI integration suites (`law_lifecycle`,
  `canon_hook`, `install_concurrency`, `knowledge_cli`, `rollback_floor`,
  `init_custody`, `suite_gate`, `bootstrap_config`, `fleet_keyring`, plus two suites that
  pin shipped sample regexes against the real `examples/laws/` bytes: `law_patterns`
  (the `lsp-over-grep` family) and `secret_zero` (both directions — a cleartext
  credential denies, an unexpanded `$VAR` does not). The remaining samples are
  lint-checked, not behaviour-tested.
  `cargo test` + `clippy -D warnings`.
- **Version** 0.1.3 (`flint --version` carries the build git hash, `+dirty` when the built source differs from that commit) · Rust 1.85+ · edition 2024 · MIT.

## Changing Flint without breaking it

- `freeze_gate` runs inside `cargo test` — a banned word-root in `flint-core/src`
  fails the build with a pointer to PIP-0001. Growth toward a memory system is meant
  to be **loud**.
- **Sign changes.** Nothing bears weight until it is picked with the sovereign key.
  The agent never signs — a human does.
- Keep the judge small (a one-sitting read). No fifth action.
- Respect the open-source boundary (`.gitignore`): source, whitepaper + PIPs, sample
  laws, operating docs, scripts, skills and assets ship; internal design docs stay in
  the private memory instance.

---

Pointers: [`README.md`](README.md) (install + day-to-day) ·
[`docs/whitepaper.md`](docs/whitepaper.md) (the constitution) ·
[`docs/PIP-0001-core-freeze.md`](docs/PIP-0001-core-freeze.md) (the freeze ADR).
