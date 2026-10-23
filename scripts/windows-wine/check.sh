#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
target=x86_64-pc-windows-gnu

export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR="$(mktemp -d)"
wine --version
timeout 60 wineboot --init

cargo check --locked --target "$target" -p proa-cli --all-targets

# These tests execute Windows binaries under Wine, including MoveFileExW.
cargo test --locked --target "$target" -p proa_build -p rsjs_transport --lib windows_ -- --test-threads=1
cargo test --locked --target "$target" -p proa-cli --lib cli::tests -- --test-threads=1
cargo test --locked --target "$target" -p proa-cli --lib artifact_export_replaces_the_destination_and_preserves_permissions
cargo test --locked --target "$target" -p proa-cli --lib missing_tailwind_cli_message_offers_platform_installation
cargo build --locked --target "$target" -p proa-cli --bins
wine "Z:\\t\\$target\\debug\\proa.exe" --version
wine "Z:\\t\\$target\\debug\\cargo-proa.exe" proa --version

# Launch an actual Windows PE through `proa start` in a path with spaces.
mkdir -p '/work/with spaces/target/release'
cat > /work/probe.c <<'C'
#include <stdio.h>
#include <stdlib.h>
int main(void) {
    printf("%s|%s\n", getenv("PROA_HOST"), getenv("PORT"));
    return 0;
}
C
x86_64-w64-mingw32-gcc /work/probe.c -o '/work/with spaces/target/release/probe.exe'
cd '/work/with spaces'
printf '%s\n' '{"application":{"binary":"probe"}}' > proa.config.json
output="$(wine "Z:\\t\\$target\\debug\\proa.exe" start --host 127.0.0.2 --port 8123)"
if [[ "${output//$'\r'/}" != '127.0.0.2|8123' ]]; then
    printf 'Unexpected Windows server output: %s\n' "$output" >&2
    exit 1
fi
printf 'Windows GNU/Wine portability checks passed. Native MSVC and Windows security policy still require Windows CI.\n'
