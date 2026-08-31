---
name: Bug report
about: Something Flint did, or failed to do
labels: bug
---

**What happened, and what you expected instead.**

**The environment** — these four decide most Flint bugs:

- `flint --version` (paste it whole; the git hash is the part that identifies the build):
- OS and architecture:
- Harness and its version (Claude Code / Codex / Grok):
- Rule kind involved (`command` / `path` / `advisory`), and its `response:` tier:

**Did the gate judge, or did the harness disobey?** These are different bugs, and the
receipt tells them apart:

- What `flint canon list --config <your config>` shows for the rule:
- What the obs log recorded for the action (rule id + verdict), if anything:
- Whether the command actually ran:

A receipt with no block means the harness ignored the verdict. No receipt at all means
the hook never fired. Both are useful — just say which you saw.

**Smallest way to reproduce it.** A rule file (redacted as needed) plus the command you
ran beats a description.
