#!/usr/bin/env bash
# Wire flint's PreToolUse hook into the harnesses you name.
#
# `flint compile` prints wiring; something has to merge it into configs that belong to
# other tools. That merge used to live as a shell block inside SETUP.md, where it could
# not be tested and every review round found another hole in it. It lives here instead,
# covered by scripts/check-setup-doc.py.
#
#   scripts/wire-harness.sh --config ~/.flint/flint.toml claude codex
#   scripts/wire-harness.sh --config ~/.flint/flint.toml --check claude
#
# What it guarantees, per harness:
#   - the config is backed up before anything is written, or nothing is written;
#   - the merged result is validated BEFORE it replaces the live file;
#   - flint's own hook is replaced, and hooks belonging to other tools are kept, even
#     when they share an entry with flint's;
#   - the written file is read back and checked: right harness named, and for Codex both
#     the Bash and apply_patch matchers present;
#   - any failure leaves the original config untouched and exits non-zero.

set -euo pipefail

CFG=""; CHECK=0; HARNESSES=()
FLINT="${FLINT:-flint}"

usage() { echo "usage: $0 --config <flint.toml> [--check] <claude|codex|grok>..." >&2; exit 2; }

while [ $# -gt 0 ]; do
  case "$1" in
    --config) CFG="${2:-}"; shift 2 ;;
    --check)  CHECK=1; shift ;;
    -h|--help) usage ;;
    -*) echo "unknown option: $1" >&2; usage ;;
    *) HARNESSES+=("$1"); shift ;;
  esac
done
[ -n "$CFG" ] || usage
[ ${#HARNESSES[@]} -gt 0 ] || usage
command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 2; }

# compile prints the hook JSON, and for codex an AGENTS.md block after it. Take the
# first JSON value; ignore whatever follows.
first_json() {
  python3 -c 'import json,sys; print(json.dumps(json.JSONDecoder().raw_decode(sys.stdin.read().strip())[0], indent=2))'
}

target_for() {
  case "$1" in
    claude) echo "$HOME/.claude/settings.json" ;;
    codex)  echo "$HOME/.codex/hooks.json" ;;
    grok)   echo "$HOME/.grok/hooks/flint.json" ;;
    *)      return 1 ;;
  esac
}

# Read back what was written and prove it is what this harness needs.
verify() {
  local harness="$1" file="$2"
  python3 - "$harness" "$file" <<'PY'
import json, sys
harness, path = sys.argv[1], sys.argv[2]
d = json.load(open(path))
entries = d.get("hooks", {}).get("PreToolUse", [])
flint = [h for e in entries for h in e.get("hooks", []) if "hook --harness" in h.get("command", "")]
if not flint:
    sys.exit(f"{path}: no flint hook present after write")
named = {c.split("--harness ")[1].split()[0] for c in (h["command"] for h in flint) if "--harness " in c}
if named != {harness}:
    sys.exit(f"{path}: wiring names {sorted(named)}, expected [{harness}]")
if harness == "codex":
    matchers = {e.get("matcher") for e in entries if any("hook --harness" in h.get("command", "") for h in e.get("hooks", []))}
    if matchers != {"Bash", "apply_patch"}:
        sys.exit(f"{path}: codex needs both Bash and apply_patch matchers, found {sorted(matchers)}")
PY
}

rc=0
for harness in "${HARNESSES[@]}"; do
  if ! target="$(target_for "$harness")"; then
    echo "$harness: unknown harness" >&2; rc=1; continue
  fi

  frag="$(mktemp)"; merged="$(mktemp)"
  trap 'rm -f "$frag" "$merged"' EXIT

  if ! "$FLINT" compile --harness "$harness" --config "$CFG" | grep -v '^#' | first_json > "$frag"; then
    echo "$harness: compile failed — $target untouched" >&2; rc=1; continue
  fi

  if [ "$harness" = grok ]; then
    cp "$frag" "$merged"                    # grok's hook file is flint's alone
  else
    if [ -s "$target" ]; then
      if ! jq --slurpfile frag "$frag" '
            .hooks.PreToolUse = (
              [ (.hooks.PreToolUse // [])[]
                # strip flint from INSIDE an entry: another tool may share it.
                # "hook --harness" is the marker — grok quotes the binary path, so a
                # substring like "flint hook" misses it.
                | .hooks = [ (.hooks // [])[] | select(((.command // "") | contains("hook --harness")) | not) ]
                | select((.hooks | length) > 0) ]
              + $frag[0].hooks.PreToolUse )
          ' "$target" > "$merged"; then
        echo "$harness: merge failed — $target untouched" >&2; rc=1; continue
      fi
    else
      cp "$frag" "$merged"                  # nothing there yet: the fragment is the file
    fi
  fi

  # validate the CANDIDATE before it replaces anything live
  if ! python3 -m json.tool "$merged" >/dev/null 2>&1; then
    echo "$harness: merged result is not valid JSON — $target untouched" >&2; rc=1; continue
  fi

  if [ "$CHECK" = 1 ]; then
    echo "$harness: would write $target"; continue
  fi

  mkdir -p "$(dirname "$target")"
  if [ -f "$target" ]; then
    if ! cp -p "$target" "$target.bak-$(date +%Y%m%d-%H%M%S)"; then
      echo "$harness: could not back up $target — refusing to write" >&2; rc=1; continue
    fi
  fi
  cat "$merged" > "$target"                 # through the existing inode: keeps its mode

  if verify "$harness" "$target"; then
    echo "$harness: wired -> $target"
  else
    echo "$harness: WRITTEN BUT WRONG — restore from the .bak- file beside it" >&2; rc=1
  fi
done

exit $rc
