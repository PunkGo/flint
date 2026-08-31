# Reference — rule frontmatter and `flint.toml`

The two field tables you need once you go past copying a sample: how to write a rule,
and what the config file holds. Everything here is read straight from the parser
(`crates/flint-core/src/canon.rs`) and the config type (`crates/flint-core/src/config.rs`) —
if a field is not listed here, it is not recognized.

## Writing a rule

A rule is a markdown file in `<flint home>/canon/rules/`: YAML-ish frontmatter between
`---` fences, then a body. **The body is the text the agent receives** — the blocking
reason for a gate rule, the guidance for an advisory. It may not be empty.

Two rules of the parser worth knowing before you write one:

- **An unrecognized key fails the whole set closed.** There is no "ignored extra field":
  a typo like `descripton:` is refused, and until it is fixed *nothing* in that canon is
  enforced. This is deliberate — a rule you thought you wrote is worse than no rule.
- **`status:` defaults to `accepted` when the line is absent.** A fresh rule file with no
  `status:` will be signed straight into force by the next `canon pick`. Write
  `status: proposed` while drafting.

### Fields every kind takes

| Field | Required | Value |
|---|---|---|
| `schema` | yes | `flint/v1` or `flint/v2`. One canon may mix both. `response: warn` needs v2 — see below. |
| `id` | yes | kebab-case, unique in the canon. This is the name in `law list` / `canon list` and in receipts. |
| `type` | yes | `rule`. (`pit` is reserved and refused.) |
| `kind` | yes | `command`, `path`, or `advisory`. |
| `status` | no | `proposed` / `accepted` / `disabled` / `removed`. **Absent means `accepted`.** |
| `version` | no | integer, default `1`. Your own revision counter — unrelated to the manifest epoch. |
| `created` | no | `YYYY-MM-DD`. |
| `description` | no (required for `advisory`) | one line, third person: what it governs and when. |
| `source.kind` | no | `human` / `observation` / `claude` / `codex` / `reading` — who authored it. |
| `source.ref` | no | a traceable pointer: a doc path, a session id, a commit, a URL. |
| `scope` | no | comma-separated selectors: `global`, `project:x`, `agent:claude`, … |
| `supersedes` | no | comma-separated rule ids this one replaces. |
| `tags` | no | comma-separated, free-form. |

### `kind: command` — a regex over the command text

| Field | Required | Value |
|---|---|---|
| `pattern` | yes | Regex matched against the command. Must compile (linear-time engine — no catastrophic backtracking). |
| `exempt` | no | Regex that, when it also matches, downgrades the verdict to an audited pass. This is how you offer a declared escape hatch (e.g. an env-var prefix stating a reason). Every exempt passage is receipted. |
| `response` | yes | `warn` / `critique` / `block` — see the tier table below. |
| `reversibility` | no | `reversible` (default) or `irreversible`. An irreversible action escalates any tier to a hard freeze. |
| `suggestion` | no | One line appended to the block message: what to do instead. Worth writing — it is the difference between a wall and a recovery path. |

### `kind: path` — a glob over the write target

Same as `command`, except the matcher is `glob` (required, e.g. `secrets/**`) instead of
`pattern` / `exempt`. Paths are resolved against `workspace_root` before matching, so
globs are written relative to the project root.

### `kind: advisory` — guidance compiled into agent context

Not a gate: it is never matched, never blocks, and produces no verdict. It is compiled
into the file each harness reads as standing instructions.

| Field | Required | Value |
|---|---|---|
| `description` | yes | The one-line summary the agent sees first. |
| `trigger` | yes | Comma-separated situations this applies to. Compiled into the rendered text as *"Applies when: …"*. |

An advisory that carries any gate field (`pattern`, `glob`, `exempt`, `response`,
`reversibility`, `suggestion`) is refused — the two shapes are kept mutually exclusive
so a rule cannot look like a gate while being guidance.

### The response tiers

| `response:` | The call | The agent hears | Receipt |
|---|---|---|---|
| `warn` | proceeds | yes, as context | `warn` |
| `critique` | blocked, recovery path offered | yes, as the blocking reason | `critique` |
| `block` | blocked, hard freeze | yes, as a denial | `deny` |

`warn` requires `schema: flint/v2`. It is the one word whose meaning changed — under
`flint/v1` it *blocked* — and a signature covers your bytes, not the parser's reading of
them, so a v1 file saying `warn` fails closed with an error naming the fix rather than
silently changing what an already-signed rule does.

There is no tier that means "approve". A call matching nothing is simply not spoken to.

## `flint.toml`

Written by `flint init`; loaded and validated on every verb, so a malformed config is a
loud error everywhere rather than a surprise from whichever command trips over it first.

### Top level

| Key | Required | Meaning |
|---|---|---|
| `flint_bin` | yes | Path to the installed binary, baked into the hook wiring `flint compile` emits. |
| `canon_root` | yes | The signed canon directory (`CANON.manifest`, `.sig`, `rules/`). |
| `workspace_root` | no | The root that relative `path` globs are resolved against. Defaults to the hook process's working directory (which harnesses set to the project root); pin it to be independent of cwd. |
| `obs_log` | no | Append-only receipt log. Absent disables recording. |
| `pits_root` | no | The raw-ore capture inbox. Absent disables `flint pit`. |
| `knowledge_root` | no | Where promoted notes land — plain markdown you own. Absent disables `flint knowledge promote/list`. |

### `[trust]` — the root of trust

| Key | Required | Meaning |
|---|---|---|
| `allowed_signers` | yes | OpenSSH `allowed_signers` file pinning the sovereign key (and any fleet keys). |
| `signer_identity` | yes | The principal in that file whose signature counts. |
| `scope` | yes | This instance's namespace; must equal the manifest scope. Stops one machine's signed canon being replayed on another. |
| `min_epoch` | no | Anti-rollback floor, default `0`. |
| `epoch_floor` | no | Path to a persistent high-water epoch file. `canon pick` advances it; the gate floors the accepted epoch at it, so checking out an older validly-signed manifest is refused without a manual bump. Weak tier by design — a same-UID agent can edit this file too. |

### `[memory]`, `[capture]` — opt-in behavior

| Key | Default | Meaning |
|---|---|---|
| `memory.vault` | unset | A markdown folder for the bring-your-own-memory verbs. Unset disables `flint memory`. |
| `capture.auto_mine` | `true` | Ships Flint's capture nudge into agent context, so the agent marks walls itself. Opt-out: set `false` and not one word of it reaches the agent. |

### `[[ore_store]]` — one or more capture stores

A repeatable table: `path` (required), `active` (default false — exactly one store
receives auto-capture; if none is flagged the first listed wins), `label` (defaults to
the directory name). `flint knowledge review` reads the union of every store's inbox.
When no `[[ore_store]]` is declared, one is synthesized from `pits_root` and
`memory.vault` so an older config keeps working.

### Dormant blocks

`[budget]` (`sidecar`, `critique_threshold`) and `[judge]` (`cross_vendor`, `model`)
configure the frozen outer ring described in [`ARCHITECTURE.md`](../ARCHITECTURE.md).
Both are off by default and are not part of the enforcement path.
