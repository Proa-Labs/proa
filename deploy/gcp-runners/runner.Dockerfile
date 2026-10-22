# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# CI runner for Proa: ARC base + pinned Rust toolchain + link/build tools
# + sccache. Rebuild on toolchain bumps (rust-toolchain 1.95.0).
# 2.338.1 bundles Node 24.22.1, including the October 21 security fixes.
# setup-node does not update this runtime used to execute JavaScript actions.
FROM ghcr.io/actions/actions-runner:2.338.1@sha256:e275409999c37162649aa050ca573f03eab2eabcb3a1e7fcc47548de6b4f9690

RUN /home/runner/externals/node24/bin/node --version

USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
        acl haproxy openssl \
        build-essential pkg-config libssl-dev python3 curl git gh jq \
        openssh-client rsync zstd \
    && rm -rf /var/lib/apt/lists/*

# sccache, pinned + checksum-verified (release: v0.17.0 linux x86_64-musl)
RUN curl -fsSL -o /tmp/sccache.tgz \
      "https://github.com/mozilla/sccache/releases/download/v0.17.0/sccache-v0.17.0-x86_64-unknown-linux-musl.tar.gz" \
    && printf '%s  %s\n' \
      67c4a96dd237c1f518f6b36083f270f9976d516f1e57fce891755ea782e50006 \
      /tmp/sccache.tgz | sha256sum --check --status \
    && tar -xzf /tmp/sccache.tgz -C /tmp --strip-components=1 \
    && install -m 0755 /tmp/sccache /usr/local/bin/sccache \
    && rm -rf /tmp/sccache* \
    && sccache --version

USER runner
# Pinned toolchain matching the workspace; profile minimal + clippy/rustfmt.
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain 1.95.0 --profile minimal \
        --component clippy --component rustfmt \
    && ~/.cargo/bin/cargo --version
ENV PATH="/home/runner/.cargo/bin:${PATH}"
