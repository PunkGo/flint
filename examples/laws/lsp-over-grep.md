---
schema: flint/v2
id: lsp-over-grep
type: rule
kind: command
version: 5
status: proposed
created: 2026-07-06
description: Recommend LSP for source-code symbol navigation instead of grep, without blocking.
source.kind: human
scope: global
tags: navigation
pattern: '((^|[;&|\n]|\$\(|\x60)[[:space:]]*([A-Za-z_][A-Za-z_0-9]*=("[^"\n]*"|\x27[^\x27\n]*\x27|[^[:space:];&|]*)[[:space:]]+|(xargs|sudo|command|nice|time|git|env|if|then|elif|else|do|while|until)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"][^[:space:];&|]*)[[:space:]]+)?)*|![[:space:]]+)*([^[:space:];&|]*/)?(grep|egrep|fgrep|rg|ag|ack)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+)*(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27";&|[:space:]\n][^[:space:];&|\n]*|--[[:space:]]+[^\x27";&|[:space:]\n][^[:space:];&|\n]*)[[:space:]]+|((-e|-f)[^[:space:];&|]+|--(regexp|file)=[^[:space:];&|]+)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+)*)([^;&|\x27"\n]|\|[^;&|\n]|&[0-9]|\x27[^\x27\n]*\x27|"[^"\n]*")*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/|"[^"\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^"\n]*"|\x27[^\x27\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^\x27\n]*\x27)|(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/|"[^"\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^"\n]*"|\x27[^\x27\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^\x27\n]*\x27)([^;&|\x27"\n]|\|[^;&|\n]|&[0-9]|\x27[^\x27\n]*\x27|"[^"\n]*")*(\||\$\(|\x60)[[:space:]]*([A-Za-z_][A-Za-z_0-9]*=("[^"\n]*"|\x27[^\x27\n]*\x27|[^[:space:];&|]*)[[:space:]]+|(xargs|sudo|command|nice|time|git|env|if|then|elif|else|do|while|until)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"][^[:space:];&|]*)[[:space:]]+)?)*|![[:space:]]+)*([^[:space:];&|]*/)?(grep|egrep|fgrep|rg|ag|ack)([[:space:]]|$))'
response: warn
suggestion: 'Use LSP (goToDefinition / findReferences / workspaceSymbol) when the target is a symbol; for a plain-text hunt this note is informational and the call proceeds.'
---
Symbol navigation belongs to LSP: goToDefinition / findReferences / workspaceSymbol answer with types, scopes and real references, where a text match returns same-named symbols, string literals and comments and misses dynamic imports and re-exports.

This command greps a visible code signal — a source extension, or a strong source root (src/ crates/ pkg/ internal/ cmd/ lib/) in the same segment. That signal is why the note is worth stating; it is not a verdict on intent, and the call proceeds either way. Hunting a symbol: reach for LSP first. Hunting text (a literal, a log line, a TODO): carry on.

The recursive-with-no-classifiable-target shape is covered by the companion note lsp-over-grep-sweep. grep stays the right tool for prose and non-source formats (TOML / JSON / YAML / shell) — those never match this rule. This is a regex tier, not a shell parser: the first non-flag argument is the search pattern, later arguments are targets, and a quoted target still counts as a code signal.
