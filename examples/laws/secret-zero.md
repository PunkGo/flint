---
schema: flint/v1
id: secret-zero
type: rule
kind: command
status: proposed
version: 4
created: 2026-07-06
description: Never put a cleartext credential on the command line.
source.kind: human
scope: global
tags: security, iron-law
pattern: '(postgres(ql)?|mysql|redis|mongodb)://[^/@[:space:]]+:[^/@[:space:]$][^/@[:space:]]{5,}@|(?i:Authorization|Bearer)[[:space:]:=]+[A-Za-z0-9._\-]{20,}|\b(ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|gho_[A-Za-z0-9]{20,})|\b([a-z0-9]{0,10}sk-[A-Za-z0-9_-]{20,}|AIza[A-Za-z0-9_-]{30,}|xai-[A-Za-z0-9_-]{20,})|\bnpm_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}'
response: block
reversibility: irreversible
---
Never put a credential on the command line — it leaks into shell history, process listings, ssh/CI logs, and any error echo of argv. This command carries a cleartext secret (DB DSN with embedded password / Bearer or Authorization token / GitHub PAT ghp_·github_pat_·gho_ / AI provider key sk-·AIza·xai- / npm token / AWS access key AKIA). Fix: pass it via an env var (DATABASE_URL=… / ANTHROPIC_API_KEY=… / GH_TOKEN=…) or a Keychain / vault pointer, never the literal on argv. The DSN arm requires a password of 6+ characters, so short placeholder credentials (u:p@localhost, test:test@) do not deny, while dictionary placeholders of 6+ chars still do — this regex engine has no negative lookahead, so env vars remain the workaround. The password position must also not START with `$`: an unexpanded shell variable ($DB_PASS / ${DB_PASS}) is a pointer, not a cleartext credential, and does not deny — the rule must not punish the very practice it recommends. The accepted residual is a literal password whose first character is `$`.
