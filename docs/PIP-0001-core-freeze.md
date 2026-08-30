# PIP-0001 — flint-core freeze (ADR)

- **Status:** accepted (2026-07-11)
- **Enforced by:** `crates/flint-core/tests/freeze_gate.rs` (runs in `cargo test`)

## Context

flint is a small local enforcement layer: your rules, compiled to every agent you use,
enforced at the moment of action, revised only when you say so. The strongest pull on a
codebase like this is scope creep toward a *memory system* — buckets, retrospectives,
auto-promotion of notes into rules, mining logs for "insights". Every one of those
organs belongs to the owner's private memory instance (a separate, git-synced knowledge
repo), not to the judge. History has already shown the failure mode: an earlier
construction round grew exactly these organs and had to be cut back.

## Decision

`flint-core` (the judge face) is **frozen against knowledge-management growth**:

1. A smoke-alarm test bans these word-roots from code lines (comments and docs may name
   them) in `crates/flint-core/src/**`:
   `casefiles · briefs · portals · archive · retro · promote · classify · distill ·
   instance.toml`, plus the bucket path form `maps/` (the bare word `maps` is everyday
   Rust/English and stays legal).
2. The hook entrypoints (`flint-cli/src/hook.rs`, `cross_vendor.rs`) must never
   reference `instance.toml` — the judge/hook path must not know that file exists. The
   installer (`flint-cli` install path) legally reads it: carrying artifacts over is the
   "take it with you" action, not judgment.
3. The dormant outer ring (`forge.rs` and its `promote` API) predates this gate and is
   grandfathered **as-is**. It stays frozen: no new callers, no new capability. Removing
   it is a separate deliberate surgery, out of this gate's scope.

The gate is deliberately crude (拦粗不拦精): it makes accidental growth loud; it does
not adjudicate intent.

## Consequences

- Any PR that introduces a banned root into `flint-core/src` code fails `cargo test`
  with a pointer to this document.
- A legitimate exception is added consciously: a reviewed `GRANDFATHERED` entry in
  `freeze_gate.rs` **plus** an amendment note here — never a silent weakening of the
  scan.
- Binary changes stay reviewable: the freeze keeps the judge small enough that "what
  does the enforcement layer do" remains a one-sitting read.

## Amendments

- (none yet)
