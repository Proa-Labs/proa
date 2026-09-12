#!/bin/sh
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Start the SSR companion processes, then hand the foreground to the Rust
# server.
#
# The companions are deliberately not tied to the health of the site. Railway
# healthchecks proa_marketing, and a companion that dies degrades to the
# playground's "unavailable" message for that one engine rather than taking
# the whole deployment down with it. Exiting here on a companion failure would
# trade a broken button for a broken site.
#
# They also stay on loopback. These timings are published as a comparison
# against in-process Proa, so a companion behind a network hop would be
# charged for latency Proa never pays.
set -eu

# NODE_ENV matters for more than logging: React's development build is
# dramatically slower than production. Running the companion in dev mode would
# quietly hand Proa a benchmark win it did not earn.
export NODE_ENV=production

COMPANION_DIR=/app/companions

start_companion() {
    label=$1
    port=$2
    shift 2
    if ! [ -d "$COMPANION_DIR" ]; then
        echo "companions: $COMPANION_DIR missing, skipping $label" >&2
        return 0
    fi
    echo "companions: starting $label on :$port"
    (
        # tsx discovers tsconfig.json from the working directory. Starting in
        # the parent silently selects classic JSX and breaks components that
        # rely on the configured automatic React runtime.
        cd "$COMPANION_DIR/hono-ssr" || exit 1
        PORT="$port" exec "$@" server.ts
    ) &
}

# Set PROA_COMPANIONS=0 to run the site without them; every engine but Proa
# then reports "unavailable", which is the same state as before they shipped.
if [ "${PROA_COMPANIONS:-1}" != "0" ]; then
    start_companion "react-hono-node" "${PROA_NODE_SSR_PORT:-5100}" node --import tsx
    start_companion "react-hono-bun" "${PROA_BUN_SSR_PORT:-5200}" bun
fi

exec /usr/local/bin/proa_marketing
