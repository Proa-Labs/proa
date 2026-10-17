#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
apt-get update -qq
apt-get install -y -qq --no-install-recommends build-essential ca-certificates curl libssl-dev pkg-config python3 time zstd
curl --fail --location --silent --show-error --retry 3 --proto '=https' --tlsv1.2 \
  --output /usr/local/bin/tailwindcss \
  https://github.com/tailwindlabs/tailwindcss/releases/download/v3.4.18/tailwindcss-linux-x64
printf '%s  %s\n' d37c46e4ff1e96c64804380e6c0db2b6ee24aafaafeabd605ca791223b06bbb3 \
  /usr/local/bin/tailwindcss | sha256sum --check --strict
chmod 0755 /usr/local/bin/tailwindcss
