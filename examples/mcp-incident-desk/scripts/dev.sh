#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/watch-lock.sh"

# Use an installed CLI, or this checkout's cargo alias when developing Proa.
proa_cli=(proa)
if ! command -v proa >/dev/null 2>&1; then
  proa_cli=(cargo proa)
fi

# ---------------------------------------------------------------------------
# Watch and rebuild
#
# This example owns its HTTPS shell and strict CSP; it does not embed Proa's
# browser reload client. The watcher rebuilds assets and restarts the server,
# but refresh the browser manually after an edit.
# ---------------------------------------------------------------------------

PROA_DEV_RELOAD="${PROA_DEV_RELOAD:-off}"

# Port for the reload event stream. Empty lets the operating system pick a
# free one. Pin it only if you want it to be predictable.
PROA_DEV_RELOAD_PORT="${PROA_DEV_RELOAD_PORT:-}"

# on/off. On swaps stylesheets without navigating when only stylesheets
# changed. Turn it off if your stylesheets are compiled into the binary rather
# than served from disk.
PROA_DEV_CSS_HOT_SWAP="${PROA_DEV_CSS_HOT_SWAP:-on}"

# Quiet window, in milliseconds, that a change must survive before a rebuild
# starts. Raise it if a slow formatter or code generator triggers extra cycles.
PROA_DEV_DEBOUNCE_MS="${PROA_DEV_DEBOUNCE_MS:-300}"

# Accept the spellings people actually reach for. Silently treating `false` as
# "on" is how someone ends up convinced the setting does nothing.
proa_dev_is_off() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    0 | false | no | off) return 0 ;;
    1 | true | yes | on) return 1 ;;
    *)
      echo "dev.sh: ignoring unrecognized value '$1' for $2; using the default." >&2
      return 1
      ;;
  esac
}

proa_dev_require_number() {
  if [[ ! "$1" =~ ^[0-9]+$ ]]; then
    echo "dev.sh: $2 must be a number, got '$1'." >&2
    exit 1
  fi
}

dev_args=()
if proa_dev_is_off "$PROA_DEV_RELOAD" PROA_DEV_RELOAD; then
  dev_args+=(--no-reload)
fi
if proa_dev_is_off "$PROA_DEV_CSS_HOT_SWAP" PROA_DEV_CSS_HOT_SWAP; then
  dev_args+=(--no-css-hot-swap)
fi
if [[ -n "$PROA_DEV_RELOAD_PORT" ]]; then
  proa_dev_require_number "$PROA_DEV_RELOAD_PORT" PROA_DEV_RELOAD_PORT
  dev_args+=(--reload-port "$PROA_DEV_RELOAD_PORT")
fi
if [[ -n "$PROA_DEV_DEBOUNCE_MS" ]]; then
  proa_dev_require_number "$PROA_DEV_DEBOUNCE_MS" PROA_DEV_DEBOUNCE_MS
  dev_args+=(--debounce-ms "$PROA_DEV_DEBOUNCE_MS")
fi

# Cargo normally shares one target directory across every package in a
# workspace. Give this site its own target so sibling dev servers can rebuild
# concurrently instead of waiting on the same Cargo build lock.
export CARGO_TARGET_DIR="${PROA_DEV_CARGO_TARGET_DIR:-${CARGO_TARGET_DIR:-$ROOT/target/watch/dev}}"

# Keep one watcher per site and supervise the whole process group so Ctrl-C
# stops Proa, Cargo, and the running server before releasing the lock.
#
# `${dev_args[@]+...}` expands an empty array safely under `set -u`, which
# bash 3.2 (still the system bash on macOS) otherwise treats as unbound.
proa_watch_lock_acquire "dev" "$ROOT"
proa_watch_run "${proa_cli[@]}" dev --root "$ROOT" ${dev_args[@]+"${dev_args[@]}"} "$@"
