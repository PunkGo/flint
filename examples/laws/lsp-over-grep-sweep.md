---
schema: flint/v2
id: lsp-over-grep-sweep
type: rule
kind: command
status: proposed
version: 1
created: 2026-08-08
description: Warn on blind recursive text sweeps whose target cannot be classified from the command line.
source.kind: human
scope: global
tags: navigation, iron-law
pattern: '((^|[;&\n]|\$\(|\x60|\|\|)[[:space:]]*([A-Za-z_][A-Za-z_0-9]*=("[^"\n]*"|\x27[^\x27\n]*\x27|[^[:space:];&|]*)[[:space:]]+|(xargs|sudo|command|nice|time|git|env|if|then|elif|else|do|while|until)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"][^[:space:];&|]*)[[:space:]]+)?)*|![[:space:]]+)*([^[:space:];&|]*/)?((grep|egrep|fgrep)[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"[:space:];&|\n][^[:space:];&|\n]*|-[^-[:space:];&|\n][^[:space:];&|\n]*|--[^[:space:];&|\n]+)[[:space:]]+)*(-[a-zA-Z]*[rR][a-zA-Z]*|--recursive|--dereference-recursive)([[:space:]]|$)|(rg|ag|ack)[[:space:]]+((-[tTgmjABCr]|--type|--type-not|--glob|--max-count|--threads|--context|--replace|--encoding|--color)[[:space:]]+[^-][^[:space:];&|]*[[:space:]]+|-[^[:space:];&|]+[[:space:]]+)*("[^"\n]*"|\x27[^\x27\n]*\x27|[^-][^[:space:];&|]*|--[[:space:]]+[^[:space:];&|\n]+)([[:space:]]+-[^[:space:];&|]+([[:space:]]+[0-9]+)?)*([[:space:]]+\.\.?/?)?[[:space:]]*([;&|)\n]|$)))'
exempt: '(^|[;&|(\n]|\$\(|\x60)[[:space:]]*(FLINT_LSP_BYPASS=[a-z0-9][a-z0-9-]{3,}[[:space:]]|\$env:FLINT_LSP_BYPASS[[:space:]]*=[[:space:]]*("|\x27)?[a-z0-9][a-z0-9-]{3,}("|\x27|[[:space:]]|;))'
response: warn
suggestion: 'If you are hunting a CODE symbol, use LSP (goToDefinition / findReferences / workspaceSymbol) or scope the search to an explicit file. If this is a plain-text hunt (logs / docs / config), it is legitimate — you may declare it with FLINT_LSP_BYPASS=reason-slug (on PowerShell: $env:FLINT_LSP_BYPASS=reason-slug; in the same command text) to silence this note and leave an exempt receipt. The action proceeds either way.'
---
This search is recursive (an explicit -r flag, or rg / ag / ack with no target, which
recurse by default) over a target the command line cannot classify as code or text.
The gate cannot see the filesystem, only the command — so instead of guessing, it lets
the action RUN and tells you: if you are navigating source code, LSP is semantic
(types, scopes, real references) where a text sweep matches strings; if you are
searching plain text, carry on. This passage is receipted either way. Companion to
lsp-over-grep, which BLOCKS when the target is visibly source code; the strongest
matching tier always wins, so this warn can never mask that critique.
