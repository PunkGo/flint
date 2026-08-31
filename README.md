# Flint

<p align="center">
  <img src="assets/key-visual.png" alt="A small figure carrying a torch into the dark" width="420">
</p>

> 秉火前行。 — *Carry the fire forward.*

## Who this is for

One person running coding agents — Claude Code, Codex, Grok — who wants their own rules
enforced across all of them. Local and personal: one binary, plain files under `~/.flint`.
No server, no daemon, no account, no telemetry.

## Setup

Requires **Rust 1.85+**. There is no published release channel; the git tree is the
distribution.

```sh
cargo install --git https://github.com/PunkGo/flint flint-cli
```

Or paste this to your coding agent, which does the whole thing:

```text
Read https://github.com/PunkGo/flint/blob/main/SETUP.md and set Flint up on this
machine. Follow it exactly. Stop at the signing step and hand me that command —
signing is mine.
```

[`SETUP.md`](SETUP.md) is the full procedure. Rules take effect only when **you** sign
them; an agent never signs. Setup merges a hook into the config of each agent you name
(`~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.grok/hooks/flint.json`), backing up
each first.

## How it works

Signed markdown in `~/.flint/canon/rules/` → `flint compile` renders per-agent hook
wiring → the agent's PreToolUse hook runs `flint hook`, which verifies the signature,
matches the call, and returns `warn` (proceeds), `critique` (blocked, recovery path) or
`block` (hard freeze) → a receipt lands in a local log: rule id, verdict, timestamp, never
the command itself.

## What enforces where

| Rule kind | Claude Code | Codex | Grok |
|---|---|---|---|
| `command` — regex over a shell command | **blocks** | **blocks** | **blocks** |
| `path` — glob over a write target | **blocks** | advisory only | **blocks** |
| `advisory` — guidance into agent context | compiled in | compiled in | not compiled |

A `path` rule on Codex is guidance, not a gate; Grok never receives advisory rules. The
`command` row blocks everywhere. Which envelope each harness honours, and when that was
measured, is in [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Verify enforcement

A receipt records Flint's judgment, not the harness's obedience — the two came apart for
two weeks on one machine, with identical-looking receipts. Enforcement is proven only by
watching a command fail to run, which is why setup ends with a live-fire probe.

## Using it day to day

```sh
flint law list                 # rules in the working tree, and their status
flint law accept --name <id>   # sign one in            (yours to run, not your agent's)
flint canon list               # the signed set actually in force
flint canon pick               # re-sign after editing a rule
flint pit mark "<gist>"        # capture a wall as you hit it
flint knowledge review         # triage captures; nothing is ever auto-promoted
flint memory capture "<note>"  # optional vault, if wired with --with-memory
flint fleet add --pubkey <k>   # trust another machine's public key
```

All take `--config ~/.flint/flint.toml`. Fields and config keys:
[`docs/reference.md`](docs/reference.md).

## Status

Public at v0.1.3. CI covers Linux, macOS and Windows. Claude Code and Grok enforcement is
live; the Codex adapter is partial. In daily use on the author's machines for months; as of
this release **nobody else has installed it**, and the install path has been walked by hand
only on macOS. Most useful thing to report back: whether your harness actually refused to
run the command.

## Documentation

- [`SETUP.md`](SETUP.md) — install and operate, written for the agent doing it
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — internals, boundaries, per-harness measurements
- [`docs/reference.md`](docs/reference.md) — every rule field and `flint.toml` key
- [`docs/whitepaper.md`](docs/whitepaper.md) — why this exists
- [`examples/laws/`](examples/laws/) — sample rules to copy, read, and sign

## Security

Threat boundaries, and what Flint explicitly cannot defend, are in
[`SECURITY.md`](SECURITY.md).

## Contributing

Build, test and guardrails are in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT © PunkGo
