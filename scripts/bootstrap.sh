#!/bin/sh
# flint suite bootstrap — thin glue, no logic (suite plan v4.3 E2).
# Prereq (README): git clone <repo> && cd flint
# Usage: scripts/bootstrap.sh [--stage skills|full] [--instance <git-url>] [--root <dir>]
#        (defaults: --stage skills; --root $HOME/memory when --instance is given)
#
# Moves, nothing else: build the binary, provision the memory-instance pointer
# (opt-in via --instance: clone if absent + write ~/.flint/instance.toml),
# preflight (stage full only), hand off to `flint install`. All real semantics
# (manifest, lock, idempotent writes, allowlist) live in the binary — this script
# must stay dumb so both .sh and .ps1 entrypoints cannot drift apart in behavior.
#
# The instance URL is YOUR private repo — passed in, never baked into this open
# repo. Without --instance the suite still installs; $INSTANCE-sourced entries
# skip with a warning (you get enforcement + skills, no memory bindings).
set -eu

STAGE="skills"
INSTANCE_URL=""
INSTANCE_ROOT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --stage)    STAGE="${2:?--stage needs skills|full}"; shift 2 ;;
    --instance) INSTANCE_URL="${2:?--instance needs a git url}"; shift 2 ;;
    --root)     INSTANCE_ROOT="${2:?--root needs a directory}"; shift 2 ;;
    *) echo "usage: scripts/bootstrap.sh [--stage skills|full] [--instance <git-url>] [--root <dir>]" >&2; exit 2 ;;
  esac
done

REPO_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

cargo build --release --manifest-path "$REPO_DIR/Cargo.toml"
BIN="$REPO_DIR/target/release/flint"

# ---- memory-instance provisioning (opt-in, one-time, idempotent) ----
INSTANCE_TOML="$HOME/.flint/instance.toml"
if [ -n "$INSTANCE_URL" ]; then
  if [ -f "$INSTANCE_TOML" ]; then
    echo "bootstrap: $INSTANCE_TOML already exists — leaving your pointer untouched"
  else
    ROOT="${INSTANCE_ROOT:-$HOME/memory}"
    if [ ! -d "$ROOT/.git" ]; then
      git clone "$INSTANCE_URL" "$ROOT"
    else
      echo "bootstrap: $ROOT already a git repo — skipping clone, writing pointer only"
    fi
    mkdir -p "$HOME/.flint"
    MACHINE="$(hostname -s 2>/dev/null || hostname)"
    printf '# flint memory-instance pointer (written by bootstrap; judge/hook never read this).\nroot    = "%s"\nmachine = "%s"\nrepo    = "%s"\n' \
      "$ROOT" "$MACHINE" "$REPO_DIR" > "$INSTANCE_TOML"
    echo "bootstrap: wrote $INSTANCE_TOML (root=$ROOT, machine=$MACHINE)"
  fi
fi

# The runtime config the suite renders from — the single source for BOTH the
# full-stage preflight AND the install hand-off. WP-G P0: the hand-off used to
# omit --config, so a full-stage bootstrap always aborted inside the binary
# ("generator entries in scope — pass --config"). Skills stage never reads it
# (--config is required only when generator/codex-hook entries are in scope), so
# passing it unconditionally is safe and keeps one code path.
FLINT_CONFIG="$HOME/.flint/flint-global.toml"

# Stage full renders harness bindings from the signed Canon: refuse to generate
# cmd: artifacts on an unsigned/invalid canon (Eng-2). Skills stage carries no
# canon-derived content and skips the check.
if [ "$STAGE" = "full" ]; then
  if ! "$BIN" canon list --config "$FLINT_CONFIG" >/dev/null; then
    echo "bootstrap: canon not signed/valid — run 'flint canon pick' first (stage full renders from the signed canon)" >&2
    exit 1
  fi
fi

exec "$BIN" install --manifest "$REPO_DIR/scripts/manifest.toml" --stage "$STAGE" --config "$FLINT_CONFIG"
