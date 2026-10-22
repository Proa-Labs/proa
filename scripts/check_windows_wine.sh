#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# amd64 also works through Docker Desktop's CPU translation on Apple Silicon.
# The checkout is read-only; Rust artifacts and registry downloads use separate
# named volumes. No host Cargo credentials or Docker socket enter the container.
docker build --platform linux/amd64 -t proa-windows-wine \
    -f "$repo_root/scripts/windows-wine/Dockerfile" "$repo_root/scripts/windows-wine"
docker run --rm --platform linux/amd64 \
    --mount "type=bind,source=$repo_root,target=/src,readonly" \
    --mount type=volume,source=proa-windows-wine-target,target=/t \
    --mount type=volume,source=proa-windows-wine-registry,target=/usr/local/cargo/registry \
    proa-windows-wine bash /src/scripts/windows-wine/check.sh
