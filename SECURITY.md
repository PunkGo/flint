# Security

Flint is an enforcement tool, so this file states plainly what it defends, what it
cannot defend, and how to report a hole. The same boundaries are documented in the
source (`crates/flint-core/src/trust.rs`, `crates/flint-cli/src/init.rs`) — if this
file and the code ever disagree, the code is the truth and the disagreement is a bug.

## What Flint defends

- **Rule integrity.** Rules bear weight only under your Ed25519 signature
  (`CANON.manifest` + sig). A malformed or tampered rule fails the whole set
  **closed** — the gate denies rather than judging with a partial rule set.
- **Redaction.** The raw command text is **never** written: a gate that blocks a secret
  must not write that secret to disk. Be precise about the rest, because "redacted" does
  not mean "empty" — a receipt carries the rule id, verdict, timestamp, harness, tool
  kind, the target `scopes`, and for non-command actions up to 200 characters of caller
  context. For a file write that means the path is on disk in your log. The log is
  append-only in how Flint writes it — an ordinary local file, not tamper-proof storage,
  editable by anyone who can write your home directory.
- **No smuggled content.** The binary ships **zero rule content** — `flint init`
  scaffolds an empty canon. Every rule in your canon is one you (or your agent) put
  there as `proposed` and you signed. The samples in `examples/laws/` are plain
  repo files; their provenance is git itself.
- **Install confinement.** `flint install` writes only inside `~/.claude`, `~/.codex`,
  `~/.grok` and `~/.flint`, checked against a resolved-path allowlist (symlink escape is
  tested), and records an `installed.lock` for honest removal.

## What Flint cannot defend (honest boundary)

- **A wholesale binary swap.** Flint is a local binary you build from source; if an
  attacker can replace that binary, no self-check inside it can help. Your defenses
  are upstream of Flint: read the source you build, and get the repo over a channel
  you trust.
- **Harness disobedience.** A receipt records Flint's *judgment*, not the harness's
  *obedience*. If a harness ignores the hook verdict, the obs log will look healthy
  while nothing is blocked. Enforcement is only ever proven by watching a command
  not run — see the enforcement notes in `README.md`.
- **A same-UID attacker.** The anti-rollback epoch floor is a weak tier by design:
  an agent running as your user can touch the same files you can. It is
  defense-in-depth against accidents, not a hard boundary against local malice.
- **An agent that decides to sign.** "Only the human signs" is a **discipline, not a
  technical control**: your agent runs as you, can read the `0600` key, and can invoke
  `flint law accept` exactly as you would — Flint cannot tell which of you typed it. The
  invariant is upheld by the agent-facing manual ([`SETUP.md`](SETUP.md)) and by you
  noticing, not by a sandbox. What Flint does guarantee is that every signature is
  *recorded* and every rule change is *visible* in files you can diff.
- **Your own rules.** Flint enforces what you signed. It does not judge whether
  what you signed is wise.

## Reporting a vulnerability

Email `feijiu@punkgo.ai`. Please include a minimal reproduction. There is no bug
bounty; there is a maintainer who takes fail-open bugs personally.
