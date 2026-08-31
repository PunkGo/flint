# Contributing

Small tool, small rules. Build and test:

```sh
cargo build --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings   # CI treats warnings as errors
```

Rust 1.85+, edition 2024. All three commands must be green before a PR.

If you touch [`SETUP.md`](SETUP.md), also run:

```sh
python3 scripts/check-setup-doc.py      # needs target/release/flint built
```

That manual is a program people run, so it is checked by extracting its shell blocks and
executing them against adversarial fixtures — a Codex-only install, a config where another
tool's hook shares an entry with flint's stale one, a compile that fails midway, and a Codex canon
whose only governance is its `AGENTS.md` block. Every one of those is a defect that shipped
in an earlier version of that document because it was reviewed by being read.

## The guardrails (read before changing anything)

These are design invariants, not conventions — a PR that crosses one will be
declined regardless of code quality. The full versions live in
[`ARCHITECTURE.md`](ARCHITECTURE.md) and [`docs/whitepaper.md`](docs/whitepaper.md).

1. **Four actions, no fifth.** Write down / carry across / enforce at the moment of
   action / revise only on the owner's signature. Feature ideas that add a fifth
   action (auto-classification, auto-promotion, report mining) are out of scope.
2. **The agent never auto-signs.** Nothing may sign a rule programmatically.
3. **`flint-core` is frozen against memory-system growth** (PIP-0001). The
   `freeze_gate` test fails the build on banned word-roots in core source — that
   failure is the design working, not an obstacle to route around.
4. **Fail closed.** A malformed rule fails the whole set; blocking rides the JSON
   verdict envelope, never exit codes.
5. **Receipts stay redacted.** No change may write raw command text to the obs log.

## Changing the example laws

`examples/laws/` holds sample rules — plain markdown, never compiled into the
binary, never endorsed by it. Three properties are test-enforced (`init.rs` unit
tests): every sample parses clean under the canon rule format; every sample carries
`status: proposed` — a sample may never claim to already bear weight; and no sample
leaks a private pointer or an absolute home path.

## Scope of measurement claims

Comments citing measured harness behavior (e.g. "measured 2026-08-20 on grok 1.0.5")
are evidence, not folklore — keep the date and version when you touch one, and add
your own when you pin new harness behavior. A claim about how a harness reacts to a
hook is only admissible with a real session behind it.
