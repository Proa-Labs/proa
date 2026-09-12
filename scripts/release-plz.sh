#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

RELEASE_PLZ_VERSION="${RELEASE_PLZ_VERSION:-0.3.158}"
REGISTRY="crates-io"
CONFIG="${PROA_RELEASE_PLZ_CONFIG:-release-plz.toml}"

usage() {
  cat <<USAGE
Usage:
  scripts/release-plz.sh install
  scripts/release-plz.sh release-pr [release-plz args...]
  scripts/release-plz.sh release [release-plz args...]
  scripts/release-plz.sh update [release-plz args...]
  scripts/release-plz.sh set-version [release-plz args...]

Environment:
  RELEASE_PLZ_VERSION         release-plz crate version to install (default: ${RELEASE_PLZ_VERSION})
  PROA_RELEASE_PLZ_CONFIG     release-plz config path (default: ${CONFIG})
  GIT_TOKEN or GITHUB_TOKEN   token used by release-plz for GitHub operations
  CARGO_REGISTRY_TOKEN token used by Cargo for crates.io
USAGE
}

have_release_plz() {
  command -v release-plz >/dev/null 2>&1
}

release_plz_version_matches() {
  have_release_plz && release-plz --version | grep -qx "release-plz ${RELEASE_PLZ_VERSION}"
}

install_release_plz() {
  if release_plz_version_matches; then
    echo "release-plz ${RELEASE_PLZ_VERSION} already installed"
    return
  fi

  cargo install release-plz --locked --version "$RELEASE_PLZ_VERSION"
}

require_release_plz() {
  if ! have_release_plz; then
    cat >&2 <<EOF
release-plz is not installed.

Run:
  scripts/release-plz.sh install

or put release-plz ${RELEASE_PLZ_VERSION} on PATH.
EOF
    exit 127
  fi
}

run_release_plz() {
  local command="$1"
  shift

  # Keep credentials out of local validation subprocesses. They are restored
  # only after every preflight (and, for publish, the final main-tip check) has
  # succeeded.
  local release_git_token="${GIT_TOKEN:-${GITHUB_TOKEN:-}}"
  local release_registry_token="${CARGO_REGISTRY_TOKEN:-}"
  unset GIT_TOKEN GITHUB_TOKEN CARGO_REGISTRY_TOKEN

  cd "$REPO_ROOT"
  scripts/check_publish_policy.py
  python3 scripts/check_crate_archives.py

  require_release_plz

  if [[ "$command" == "release" && "${GITHUB_ACTIONS:-false}" == "true" ]]; then
    local expected_sha="${PROA_RELEASE_EXPECTED_SHA:-}"
    local repository="${PROA_RELEASE_REPOSITORY:-${GITHUB_REPOSITORY:-}}"
    local current_sha

    if [[ ! "$expected_sha" =~ ^[0-9a-fA-F]{40}$ ]]; then
      echo "release attestation requires PROA_RELEASE_EXPECTED_SHA to be a full commit SHA" >&2
      exit 1
    fi
    if [[ ! "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
      echo "release attestation requires PROA_RELEASE_REPOSITORY or GITHUB_REPOSITORY as owner/repo" >&2
      exit 1
    fi
    if [[ -z "$release_git_token" ]]; then
      echo "release attestation requires GIT_TOKEN or GITHUB_TOKEN" >&2
      exit 1
    fi

    current_sha="$(GH_TOKEN="$release_git_token" gh api \
      --header 'Accept: application/vnd.github+json' \
      --header 'X-GitHub-Api-Version: 2026-03-10' \
      "repos/$repository/git/ref/heads/main" \
      --jq '.object.sha')"
    if [[ ! "$current_sha" =~ ^[0-9a-fA-F]{40}$ ]]; then
      echo "release attestation did not receive a full main commit SHA" >&2
      exit 1
    fi
    if [[ "$current_sha" != "$expected_sha" ]]; then
      echo "release candidate is stale: expected main=$expected_sha, current main=$current_sha" >&2
      exit 1
    fi
    echo "release_attestation revision=$expected_sha status=current"
  fi

  if [[ -n "$release_git_token" ]]; then
    export GIT_TOKEN="$release_git_token"
  fi
  if [[ -n "$release_registry_token" ]]; then
    export CARGO_REGISTRY_TOKEN="$release_registry_token"
  fi

  if [[ "$command" == "set-version" ]]; then
    exec release-plz "$command" --config "$CONFIG" "$@"
  fi
  exec release-plz "$command" --config "$CONFIG" --registry "$REGISTRY" "$@"
}

if [[ $# -eq 0 ]]; then
  usage
  exit 2
fi

case "$1" in
  -h | --help | help)
    usage
    ;;
  install)
    install_release_plz
    ;;
  release | release-pr | update | set-version)
    command="$1"
    shift
    run_release_plz "$command" "$@"
    ;;
  *)
    echo "unknown release-plz command: $1" >&2
    usage >&2
    exit 2
    ;;
esac
