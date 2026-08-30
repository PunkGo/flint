# Sample laws

Rules grown from the author's real practice — offered as **samples, not defaults**.
Flint ships the mechanism; it never ships, blesses, or pre-installs rule content.

Each file is a plain-markdown law in the canon rule format, carrying
`status: proposed`: copying one into your canon gives it **zero weight** until you
read it and sign it yourself.

```sh
cp examples/laws/secret-zero.md ~/.flint/canon/rules/
flint law list   --config ~/.flint/flint.toml
flint law accept --all --config ~/.flint/flint.toml --key ~/.flint/keys/sovereign_ed25519
```

Two properties are enforced by the test suite: every sample parses clean, and every
sample is `proposed` — a sample may never claim to already bear weight. Edit them,
take some, take none: your judgment, your signature.
