---
schema: flint/v2
id: lsp-over-grep-sweep
type: rule
kind: command
status: proposed
version: 2
created: 2026-08-08
description: Note blind recursive text sweeps whose target cannot be classified from the command line.
source.kind: human
scope: global
tags: navigation
pattern: '((^|[;&\n]|\$\(|\x60|\|\|)[[:space:]]*([A-Za-z_][A-Za-z_0-9]*=("[^"\n]*"|\x27[^\x27\n]*\x27|[^[:space:];&|]*)[[:space:]]+|(xargs|sudo|command|nice|time|git|env|if|then|elif|else|do|while|until)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"][^[:space:];&|]*)[[:space:]]+)?)*|![[:space:]]+)*([^[:space:];&|]*/)?((grep|egrep|fgrep)[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"[:space:];&|\n][^[:space:];&|\n]*|-[^-[:space:];&|\n][^[:space:];&|\n]*|--[^[:space:];&|\n]+)[[:space:]]+)*(-[a-zA-Z]*[rR][a-zA-Z]*|--recursive|--dereference-recursive)([[:space:]]|$)|(rg|ag|ack)[[:space:]]+((-[tTgmjABCr]|--type|--type-not|--glob|--max-count|--threads|--context|--replace|--encoding|--color)[[:space:]]+[^-][^[:space:];&|]*[[:space:]]+|-[^[:space:];&|]+[[:space:]]+)*("[^"\n]*"|\x27[^\x27\n]*\x27|[^-][^[:space:];&|]*|--[[:space:]]+[^[:space:];&|\n]+)([[:space:]]+-[^[:space:];&|]+([[:space:]]+[0-9]+)?)*([[:space:]]+\.\.?/?)?[[:space:]]*([;&|)\n]|$)))'
response: warn
suggestion: 'If you are hunting a code symbol, use LSP (goToDefinition / findReferences / workspaceSymbol) or scope the search to an explicit file. If this is a plain-text hunt, carry on — the call proceeds either way.'
---
This search is recursive (an explicit -r flag, or rg / ag / ack with no target, which
recurse by default) over a target the command line cannot classify as code or text.
The gate cannot see the filesystem, only the command — so instead of guessing, it lets
the action RUN and tells you: if you are navigating source code, LSP is semantic
(types, scopes, real references) where a text sweep matches strings; if you are
searching plain text, carry on. This passage is receipted either way. Companion to
lsp-over-grep, which speaks with a more specific message when the target is visibly
source code; both are notes now — neither blocks.
