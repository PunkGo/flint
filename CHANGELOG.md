# Changelog

## 0.1.3 — 2026-08-30

- **Mechanism/content decoupled**: the binary no longer ships any rule content.
  `flint init` scaffolds an **empty** canon; the former default laws live on as
  plain samples in `examples/laws/` (copy → review → sign). The `attest`
  subsystem (embedded-law release attestation + maintainer key) is retired with
  the embedded content it existed to vouch for.
- Example pack: `authority-first` replaces `portal-first` (the stateless
  `AUTHORITY.toml` convention supersedes stateful portal documents).
- Open-source readiness: `SECURITY.md` (honest threat boundary), `CONTRIBUTING.md`
  (guardrails), CI (test + clippy on Linux/macOS/Windows), README hero + "who this
  is for" + Grok in the status line.
- Source comments scrubbed of private machine/vocabulary references; measurement
  provenance kept (dates + versions).

## 0.1.2 — 2026-08-20

- Third harness adapter: **Grok** (xAI Grok Build). Measured envelope: Grok honours
  `{"decision":"deny"}` and ignores `permissionDecision`; per-platform hook wiring
  (PowerShell call operator on Windows); Windows path normalization for glob scopes.
- `flint --version` carries the build git hash (`+dirty` on uncommitted trees).

## 0.1.0 – 0.1.1 — 2026-06 → 2026-08

- Initial construction: the judge (Touchstone), the signed Canon and law lifecycle
  (`init` → `law accept` → `disable`/`remove`), the per-harness compiler
  (Claude Code, Codex), the suite installer, the embedded-law release attestation,
  the pit/knowledge capture loop, bring-your-own memory, and the fleet keyring.
