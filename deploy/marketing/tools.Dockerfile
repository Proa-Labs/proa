# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Build these native amd64 tools once with tools.yaml, then pin their published
# digests in cloudbuild.yaml and the profiling configuration.
FROM rust:1.95.0-slim-bookworm@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082 AS compile
COPY deploy/marketing/install-tools.sh /tmp/install-tools.sh
# rust-toolchain.toml requests both components even for a plain cargo build.
RUN rustup component add clippy rustfmt && bash /tmp/install-tools.sh \
    && rm /tmp/install-tools.sh && rm -rf /var/lib/apt/lists/*

# stable is the minimal Cloud SDK image; slim contains additional packages.
FROM gcr.io/google.com/cloudsdktool/google-cloud-cli:stable@sha256:ee2115362cc5e05064ace2398593dc2489444fda581db3f61410a84c318f3066 AS cache
# gcloud bundles its own certificates; Docker/buildx need the OS trust store.
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends ca-certificates zstd \
    && rm -rf /var/lib/apt/lists/*
COPY --from=gcr.io/cloud-builders/docker:24.0.9@sha256:9fcfbddaf676527b2be4a6473bb37ed028a88bc55bc5cd9dcc9db4f7d9bcebee /usr/bin/docker /usr/bin/docker
# Cloud Build currently exposes Docker API 1.41. buildx 0.31 requires >=1.44.
COPY --from=docker/buildx-bin:0.29.1@sha256:6f7e32af76c687d477b9c271fcec24f49971ca6a6c980ad4352ed72dc75de617 /buildx /usr/local/lib/docker/cli-plugins/docker-buildx
RUN gcloud version && zstd --version && docker --version && docker buildx version
