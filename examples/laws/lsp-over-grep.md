---
schema: flint/v1
id: lsp-over-grep
type: rule
kind: command
status: proposed
version: 4
created: 2026-07-06
description: Route source-code symbol navigation to LSP, not grep.
source.kind: human
scope: global
tags: navigation, iron-law
pattern: '((^|[;&|\n]|\$\(|\x60)[[:space:]]*([A-Za-z_][A-Za-z_0-9]*=("[^"\n]*"|\x27[^\x27\n]*\x27|[^[:space:];&|]*)[[:space:]]+|(xargs|sudo|command|nice|time|git|env|if|then|elif|else|do|while|until)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"][^[:space:];&|]*)[[:space:]]+)?)*|![[:space:]]+)*([^[:space:];&|]*/)?(grep|egrep|fgrep|rg|ag|ack)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+)*(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27";&|[:space:]\n][^[:space:];&|\n]*|--[[:space:]]+[^\x27";&|[:space:]\n][^[:space:];&|\n]*)[[:space:]]+|((-e|-f)[^[:space:];&|]+|--(regexp|file)=[^[:space:];&|]+)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+)*)([^;&|\x27"\n]|\|[^;&|\n]|&[0-9]|\x27[^\x27\n]*\x27|"[^"\n]*")*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/|"[^"\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^"\n]*"|\x27[^\x27\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^\x27\n]*\x27)|(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/|"[^"\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^"\n]*"|\x27[^\x27\n]*(\.(rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|kt|kts|c|cpp|cc|cxx|h|hpp|hxx|swift|rb|scala|cs|m|mm|dart|zig)\b|\b(src|crates|pkg|internal|cmd|lib)/)[^\x27\n]*\x27)([^;&|\x27"\n]|\|[^;&|\n]|&[0-9]|\x27[^\x27\n]*\x27|"[^"\n]*")*(\||\$\(|\x60)[[:space:]]*([A-Za-z_][A-Za-z_0-9]*=("[^"\n]*"|\x27[^\x27\n]*\x27|[^[:space:];&|]*)[[:space:]]+|(xargs|sudo|command|nice|time|git|env|if|then|elif|else|do|while|until)[[:space:]]+(-[^[:space:];&|]+[[:space:]]+(("[^"\n]*"|\x27[^\x27\n]*\x27|[^-\x27"][^[:space:];&|]*)[[:space:]]+)?)*|![[:space:]]+)*([^[:space:];&|]*/)?(grep|egrep|fgrep|rg|ag|ack)([[:space:]]|$))'
exempt: '(^|[;&|(\n]|\$\(|\x60)[[:space:]]*(FLINT_LSP_BYPASS=[a-z0-9][a-z0-9-]{3,}[[:space:]]|\$env:FLINT_LSP_BYPASS[[:space:]]*=[[:space:]]*("|\x27)?[a-z0-9][a-z0-9-]{3,}("|\x27|[[:space:]]|;))'
response: critique
suggestion: 'Use LSP (goToDefinition / findReferences / workspaceSymbol) for code symbols. For a legitimate text grep over source files (string literal / log line / comment / TODO marker / piped non-source output / a throwaway repo with no LSP workspace) declare the downgrade WITH ITS REASON as the value: prefix the command with FLINT_LSP_BYPASS=reason-slug (4+ chars, e.g. =stdout-grep, =string-literal, =no-lsp-workspace); on PowerShell write $env:FLINT_LSP_BYPASS=reason-slug; ahead of the command in the same command text. A bare =1 does not exempt. Every declared downgrade lands in the obs log as an exempt receipt.'
---
Code navigation defaults to LSP (goToDefinition / findReferences / workspaceSymbol),
not grep. This command runs a grep-family tool AS A COMMAND against a visible code
signal — a source extension or a strong source directory (src/ crates/ pkg/ internal/
cmd/ lib/) — in the same command segment. LSP is semantic (types, scopes, real
references) where grep text-matches (false positives on same-named symbols / string
literals / comments; misses dynamic imports / re-exports / generics). grep stays the
right tool for plain text (README / comments / logs / commit messages) and non-LSP
formats (TOML / JSON / YAML / shell) — those never match this rule. v4 narrows the
aperture, not the strength: the tool must sit in command position (a filename that
merely CONTAINS the word grep no longer fires), the tool and the code signal must
share one segment (text delimited by semicolons, ampersands, newlines, or the
double-pipe or-separator — a signal in one command no longer pairs with a grep in the
next; single pipes stay joined, a pipeline is one dataflow, and fd redirects like 2>&1
do not split), and the directory list keeps only unambiguous source roots. The
recursive-with-no-classifiable-target shape is covered by the non-blocking companion
lsp-over-grep-sweep. The path-producer fast-path (find src | xargs grep) still fires
here by design — declare it via the exempt when legitimate. Known differentials of
the regex tier (this matcher is not a shell parser): the FIRST non-flag argument is
the search pattern and is opaque — a pattern that merely looks like code is data —
while later arguments are targets, and a quoted target still counts as a code signal.
The anchors themselves are quote-blind — prose containing a pipe plus a grep-shaped
call can still fire, and a crafted quoted token that mimics the exempt can suppress a
real critique. The second is
a deliberate dodge, not an authorization: every exempt passage lands in the obs log as
a countable receipt, which is exactly where bypass habit is audited.
