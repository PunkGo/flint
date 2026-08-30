# Changelog

## 0.1.3 — 2026-08-30

- **Agent-first docs**: [`SETUP.md`](SETUP.md) is the install-and-operate manual written
  for the agent doing the setup — a setup-state block it fills in first, every step
  ending on a check, an executable idempotent hook-merge recipe, a read-only live-fire
  probe with a per-harness expect/SKIP table, and a signing hand-off it may not perform
  itself. Reviewed by two outside models; their findings on executability, safety and
  overstatement are folded in.
- **`--version` told the truth only sometimes**: the build script watched `.git/HEAD`
  and `.git/index`, which an unstaged edit never touches — so a dirty tree could build
  a binary claiming to be clean. It now watches (and diffs) the source paths that
  actually enter the binary.
- SECURITY: two boundaries stated plainly — "only the human signs" is a discipline, not
  a technical control (the agent runs as you and can read the key), and the receipt log
  is append-only in how Flint writes it, not tamper-proof storage.
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
- `flint --version` carries the build git hash.

## 0.1.0 – 0.1.1 — 2026-06 → 2026-08

- Initial construction: the judge (Touchstone), the signed Canon and law lifecycle
  (`init` → `law accept` → `disable`/`remove`), the per-harness compiler
  (Claude Code, Codex), the suite installer, the embedded-law release attestation,
  the pit/knowledge capture loop, bring-your-own memory, and the fleet keyring.
