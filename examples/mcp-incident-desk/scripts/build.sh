#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Use the invoking CLI, an installed CLI, or this checkout's Cargo alias.
proa_cli=(proa)
if [[ -n "${PROA_SCRIPT_EXECUTABLE:-}" ]]; then
  proa_cli=("$PROA_SCRIPT_EXECUTABLE")
elif ! command -v proa >/dev/null 2>&1; then
  proa_cli=(cargo proa)
fi

# The config launches this wrapper. The linked build runs the asset commands
# and project checks, then builds the release binary through Cargo.
#
# The dev server keeps its own Cargo target directory under target/watch/, so
# this build and a running scripts/dev.sh do not invalidate each other.
#
# Extra arguments pass straight to `proa build`, for example:
#   bash scripts/build.sh --artifact-out dist/server
target_args=(--bin mcp-incident-desk)
for arg in "$@"; do
  case "$arg" in
    --|--bin|--bin=*|--example|--example=*)
      target_args=()
      break
      ;;
  esac
done
exec "${proa_cli[@]}" build --root "$ROOT" ${target_args[@]+"${target_args[@]}"} "$@"
