# syntax=docker/dockerfile:1.7
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Build from the repository root:
#   docker build -f proa_marketing/Dockerfile .

ARG BUILD_ARTIFACTS=builder
FROM rust:1.95.0-slim-bookworm@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082 AS builder

ARG TARGETARCH
ARG TAILWIND_VERSION=3.4.18

WORKDIR /incoming

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    build-essential \
    ca-certificates \
    curl \
    libssl-dev \
    pkg-config \
    python3 \
    && rm -rf /var/lib/apt/lists/*

# Keep the standalone Tailwind executable reproducible: GitHub release assets
# are mutable URLs, so verify the pinned asset before allowing the build script
# to execute it. These digests are the release-asset SHA-256 values published
# for Tailwind CSS v3.4.18.
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) \
        tailwind_asset="tailwindcss-linux-x64"; \
        tailwind_sha256="d37c46e4ff1e96c64804380e6c0db2b6ee24aafaafeabd605ca791223b06bbb3" \
        ;; \
      arm64) \
        tailwind_asset="tailwindcss-linux-arm64"; \
        tailwind_sha256="217d3901b32ed95e570b68394e7780f3c8bce9441d0040b2db77ed39cef69582" \
        ;; \
      *) \
        echo "Unsupported Docker target architecture: $TARGETARCH" >&2; \
        exit 1 \
        ;; \
    esac; \
    curl \
      --fail \
      --location \
      --proto '=https' \
      --proto-redir '=https' \
      --retry 3 \
      --show-error \
      --silent \
      --tlsv1.2 \
      --output /usr/local/bin/tailwindcss \
      "https://github.com/tailwindlabs/tailwindcss/releases/download/v${TAILWIND_VERSION}/${tailwind_asset}"; \
    echo "${tailwind_sha256}  /usr/local/bin/tailwindcss" | sha256sum --check --strict; \
    chmod 0755 /usr/local/bin/tailwindcss

COPY . .

# Remote builders receive a fresh checkout for every revision. Generate
# derived CSS, then reconcile the complete tree into a stable source path so
# Cargo can distinguish unchanged bytes from real changes. Keep the whole
# transaction under the same locked mounts; cache-mounted files are staged
# through /out for the next stage. Railway requires the non-secret service ID
# as a literal cache-key prefix and does not allow variables in mount IDs.
RUN --mount=type=cache,id=s/8a6d0a70-bfd2-4e6e-bf87-048c563a7044-/workspace/source,target=/workspace/source,sharing=locked \
    --mount=type=cache,id=s/8a6d0a70-bfd2-4e6e-bf87-048c563a7044-/workspace/target,target=/workspace/target,sharing=locked \
    --mount=type=cache,id=s/8a6d0a70-bfd2-4e6e-bf87-048c563a7044-/usr/local/cargo/registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=s/8a6d0a70-bfd2-4e6e-bf87-048c563a7044-/usr/local/cargo/git,target=/usr/local/cargo/git,sharing=locked \
    TAILWIND_BIN=/usr/local/bin/tailwindcss \
        PROA_MARKETING_BUILD_CSS_ONLY=1 \
        bash /incoming/scripts/watch-marketing.sh \
    && python3 /incoming/scripts/sync_build_context.py /incoming /workspace/source \
    && cd /workspace/source \
    && rm -rf /out \
    && install -d /out/static /out/showcase \
    && CARGO_TARGET_DIR=/workspace/target \
        cargo run --release --locked -p proa-cli --bin proa -- \
        build \
        --root /workspace/source \
        --profile release \
        --package proa_marketing \
        --bin proa_marketing \
        --locked \
        --no-check \
        --no-assets \
        --artifact-out /out/proa_marketing \
    && cp -a proa_marketing/public/. /out/static/ \
    && cp -a proa_showcase/src/espn/data/images/. /out/static/images/ \
    && cp -a proa_showcase_web/public/. /out/showcase/

# CI supplies the output of the exact same linked build. BuildKit omits the
# Rust builder above when BUILD_ARTIFACTS=prebuilt; Railway pulls the resulting
# runtime image by digest and never invokes Cargo.
FROM scratch AS prebuilt
COPY release-artifacts/ /out/

FROM ${BUILD_ARTIFACTS} AS artifacts

# ── SSR companions ──────────────────────────────────────────────────────────
#
# The playground renders Proa in-process and reaches every other engine over
# HTTP. Without a companion process those engines can only 502, which is what
# the React buttons did in this deployment.
#
# Only the Hono/React companion ships. It is one server that detects its own
# runtime (`typeof Bun !== "undefined"`), so a single install backs both the
# React (Node) and React (Bun) buttons. Marko, react-ssr, and Next.js are
# still absent and still degrade to the "unavailable" message.
#
# `hono-ssr/package.json` is installed one level *above* both source
# directories on purpose: `server.ts` imports `../react-ssr-shared/src/...`,
# and those files import bare `react`. Node resolution walks upward from each
# file, so a shared parent `node_modules` satisfies both. Installing inside
# `hono-ssr/` would leave the sibling unable to resolve React.
FROM node:22-bookworm-slim AS companions

WORKDIR /companions

COPY proa_demo/hono-ssr/package.json ./package.json
RUN npm install --no-audit --no-fund

COPY proa_demo/hono-ssr/ ./hono-ssr/
COPY proa_demo/react-ssr-shared/ ./react-ssr-shared/

# Three files the companion reads from disk at import time, all of which live
# outside the two directories above:
#
#   react-ssr-shared/src/css.ts  reads ../airbnb-shared.css, falling back to
#     the Rust showcase source tree. Ship the packaged path for the same reason.
#   react-ssr-shared/src/nike-css.ts  reads ../nike-shared.css, falling back to
#     the Rust showcase source tree. Ship the "packaged" path so the companion
#     does not need the Rust sources beside it.
#   hono-ssr/server.ts  reads ../showcase-props.json for its fixture props.
#
# Miss either and the process throws during module load, which looks exactly
# like the engine never being deployed.
COPY proa_showcase/src/airbnb/shared.css ./react-ssr-shared/airbnb-shared.css
COPY proa_showcase/src/nike/shared.css ./react-ssr-shared/nike-shared.css
COPY proa_demo/showcase-props.json ./showcase-props.json

# Fail the build rather than the deployment. A companion that cannot boot is
# indistinguishable at runtime from one that was never deployed, so prove it
# serves /health and renders every supported page here. Use the same working
# directory as the entrypoint so tsx loads the companion's JSX configuration.
# Also require the live Nike route to be byte-identical to the parity-checked
# React document; this catches
# server wiring that swaps in a different fixture after component validation.
RUN set -eu; \
    (cd hono-ssr && NODE_ENV=production PORT=5100 exec node --import tsx server.ts) & \
    pid=$!; \
    ok=""; \
    for _ in $(seq 1 60); do \
        if node -e "fetch('http://127.0.0.1:5100/health').then(r=>{if(!r.ok)process.exit(1);return r.json()}).then(j=>{console.log('companion /health ->',JSON.stringify(j))}).catch(()=>process.exit(1))" 2>/dev/null; then ok=1; break; fi; \
        sleep 0.5; \
    done; \
    test -n "$ok" || { echo "hono companion failed to serve /health under node"; exit 1; }; \
    node hono-ssr/smoke.mjs; \
    expected_nike_sha="$(NODE_ENV=production node --import tsx react-ssr-shared/dump-nike-pdp.ts | sha256sum | cut -d ' ' -f 1)"; \
    actual_nike_sha="$(node -e "fetch('http://127.0.0.1:5100/nike/product').then(r=>{if(!r.ok)process.exit(1);return r.text()}).then(html=>process.stdout.write(html)).catch(()=>process.exit(1))" | sha256sum | cut -d ' ' -f 1)"; \
    test "$actual_nike_sha" = "$expected_nike_sha" || { echo "hono Nike PDP differs from the parity fixture"; exit 1; }; \
    kill "$pid" 2>/dev/null || true; \
    wait "$pid" 2>/dev/null || true

FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime

# Package the source commit without recompiling the cached application binary.
ARG PROA_BUILD_REVISION=""
ENV PROA_BUILD_REVISION=${PROA_BUILD_REVISION}

# libstdc++6 is for the node and bun binaries copied in below, neither of
# which ships its own C++ runtime.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /app --shell /usr/sbin/nologin proa

WORKDIR /app

COPY --from=artifacts /out/proa_marketing /usr/local/bin/proa_marketing
COPY --from=artifacts /out/static /app/static
COPY --from=artifacts /out/showcase /app/showcase

# The two runtimes the Hono companion runs under. Same server, same install;
# only the interpreter differs, which is the whole point of the Node vs Bun
# comparison in the playground.
COPY --from=node:22-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=oven/bun:1-slim /usr/local/bin/bun /usr/local/bin/bun
COPY --from=companions /companions /app/companions

COPY proa_marketing/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod +x /usr/local/bin/docker-entrypoint.sh && chown -R proa:proa /app

ENV HOST=0.0.0.0 \
    RUST_LOG=info \
    STATIC_DIR=/app/static \
    SHOWCASE_PUBLIC_DIR=/app/showcase \
    PROA_CONSOLE_LOAD_TESTS=0

USER proa

CMD ["/usr/local/bin/docker-entrypoint.sh"]
