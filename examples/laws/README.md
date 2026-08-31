# Sample laws

Rules grown from the author's real practice — offered as **samples, not defaults**.
Flint ships the mechanism; it never ships, blesses, or pre-installs rule content.

Each file is a plain-markdown law in the canon rule format, carrying
`status: proposed`: copying one into your canon gives it **zero weight** until you
read it and sign it yourself.

```sh
cp examples/laws/secret-zero.md ~/.flint/canon/rules/
flint law list   --config ~/.flint/flint.toml
flint law accept --name secret-zero --config ~/.flint/flint.toml --key ~/.flint/keys/sovereign_ed25519
```

**Start with one.** These are one person's rules and they carry that person's taste:
`lsp-over-grep` is loud if you do not work through an LSP, `stop-framing-loop` encodes a
way of working. `secret-zero` is the usual first pick — least intrusive, most obviously
worth it. Add others when you have felt the need yourself, because a rule you adopted
because it came in the box is precisely the borrowed judgment Flint exists to prevent.
(`accept --all` exists, and signs everything currently proposed — reach for it once you
know that is what you want.)

Writing your own? The complete field table is [`../../docs/reference.md`](../../docs/reference.md).

Three properties are enforced by the test suite: every sample parses clean; every
sample is `proposed` — it may never claim to already bear weight; and none carries a
private pointer or an absolute home path. Edit them,
take some, take none: your judgment, your signature.
