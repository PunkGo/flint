**What this changes, and why.**

**Checks** (CI runs the first two on Linux, macOS and Windows):

- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] Docs updated if behaviour changed — including `--help` text, which is documentation

**Guardrails** ([`CONTRIBUTING.md`](https://github.com/PunkGo/flint/blob/main/CONTRIBUTING.md) has the full list). Tick the ones
your change touches, and say how it stays inside them:

- [ ] Does not add a fifth action
- [ ] Does not let anything but a human sign a rule
- [ ] Does not grow `flint-core` toward a memory system (PIP-0001 / `freeze_gate`)
- [ ] Keeps failures closed, and receipts redacted

**Measured claims.** If you pinned harness behaviour, say where and when you measured it
— a claim about how a harness reacts to a hook needs a real session behind it.
