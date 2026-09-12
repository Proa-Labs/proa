#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Use an installed CLI, or this checkout's cargo alias when developing Proa.
proa_cli=(proa)
if ! command -v proa >/dev/null 2>&1; then
  proa_cli=(cargo proa)
fi

# One-shot release build. `proa build` runs the configured asset builds, the
# format/lint/site checks, and then the linked Cargo build; the command comes
# from proa.config.json, `cargo build --release` by default.
#
# The dev server keeps its own Cargo target directory under target/watch/, so
# this build and a running scripts/dev.sh do not invalidate each other.
#
# Extra arguments pass straight to `proa build`, for example:
#   bash scripts/build.sh --artifact-out dist/server
"${proa_cli[@]}" build --root "$ROOT" --bin mcp-incident-desk "$@"
