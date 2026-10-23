#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

RELEASE_PLZ_VERSION="0.3.174"
SEMVER_CHECKS_VERSION="0.51.0"
# The release tools need newer Rust than the application workspace. Select it
# only for installation; preflights and application builds keep rust-toolchain.toml.
RELEASE_TOOLS_TOOLCHAIN="1.97.0"
REGISTRY="crates-io"
CONFIG="$REPO_ROOT/release-plz.toml"

usage() {
  cat <<USAGE
Usage:
  scripts/release-plz.sh install
  scripts/release-plz.sh check
  scripts/release-plz.sh release-pr [release-plz args...]
  scripts/release-plz.sh release-dry-run [release-plz args...]
  scripts/release-plz.sh release [release-plz args...]
  scripts/release-plz.sh update [release-plz args...]
  scripts/release-plz.sh set-version [release-plz args...]

Environment:
  GIT_TOKEN or GITHUB_TOKEN   token used by release-plz for GitHub operations
  CARGO_REGISTRY_TOKEN        token used by Cargo for crates.io publication
  PROA_RELEASE_EXPECTED_SHA   optional expected HEAD (set by CI)

Publication also accepts CARGO_REGISTRIES_CRATES_IO_TOKEN. Credentials saved
by cargo login are not used; load the token into the environment explicitly.

Pinned tools: release-plz ${RELEASE_PLZ_VERSION}, cargo-semver-checks ${SEMVER_CHECKS_VERSION}.
Installation uses Rust ${RELEASE_TOOLS_TOOLCHAIN}; application builds keep rust-toolchain.toml.
Local and CI releases use this wrapper and release-plz.toml. Live release-pr
and release commands require a clean checkout of the current origin/main.
USAGE
}

tool_version_matches() {
  local tool="$1" version="$2"
  command -v "$tool" >/dev/null 2>&1 &&
    "$tool" --version | grep -Fxq "$tool $version"
}

require_tool() {
  local tool="$1" version="$2"
  if ! tool_version_matches "$tool" "$version"; then
    echo "$tool $version is required. Run scripts/release-plz.sh install and ensure Cargo's bin directory is first on PATH." >&2
    exit 1
  fi
}

install_tool() {
  local tool="$1" version="$2"
  if ! tool_version_matches "$tool" "$version"; then
    cargo +"$RELEASE_TOOLS_TOOLCHAIN" install "$tool" --locked --version "$version"
  fi
  require_tool "$tool" "$version"
}

verify_release_source() {
  local release_git_token="$1"
  local head expected_sha repository origin_url origin_repository current_sha status
  head="$(git rev-parse HEAD)"
  expected_sha="${PROA_RELEASE_EXPECTED_SHA:-$head}"
  if [[ ! "$expected_sha" =~ ^[0-9a-fA-F]{40}$ || "$head" != "$expected_sha" ]]; then
    echo "release source does not match PROA_RELEASE_EXPECTED_SHA" >&2
    exit 1
  fi
  status="$(git status --porcelain)"
  if [[ -n "$status" ]]; then
    echo "release requires a clean checkout, including untracked files" >&2
    exit 1
  fi

  origin_url="$(git remote get-url origin)"
  case "$origin_url" in
    https://github.com/*) origin_repository="${origin_url#https://github.com/}" ;;
    git@github.com:*) origin_repository="${origin_url#git@github.com:}" ;;
    ssh://git@github.com/*) origin_repository="${origin_url#ssh://git@github.com/}" ;;
    *) echo "release requires a GitHub origin remote" >&2; exit 1 ;;
  esac
  origin_repository="${origin_repository%/}"
  origin_repository="${origin_repository%.git}"
  repository="${PROA_RELEASE_REPOSITORY:-${GITHUB_REPOSITORY:-$origin_repository}}"
  if [[ ! "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] ||
    [[ "$(printf '%s' "$repository" | tr '[:upper:]' '[:lower:]')" != \
       "$(printf '%s' "$origin_repository" | tr '[:upper:]' '[:lower:]')" ]]; then
    echo "release repository must match the origin remote" >&2
    exit 1
  fi

  current_sha="$(GH_TOKEN="$release_git_token" gh api \
    --header 'Accept: application/vnd.github+json' \
    --header 'X-GitHub-Api-Version: 2026-03-10' \
    "repos/$repository/git/ref/heads/main" \
    --jq '.object.sha')"
  if [[ ! "$current_sha" =~ ^[0-9a-fA-F]{40}$ || "$current_sha" != "$head" ]]; then
    echo "release candidate is stale: checkout must match the current origin/main" >&2
    exit 1
  fi
  echo "release_attestation revision=$head status=current"
}

run_release_plz() {
  local command="$1"
  shift

  local dry_run=false argument
  for argument in "$@"; do
    if [[ "$argument" == "--dry-run" ]]; then dry_run=true; fi
  done

  # Keep credentials out of validation subprocesses. Restore only what the
  # selected operation needs, after the common preflight and source check.
  local release_git_token="${GIT_TOKEN:-${GITHUB_TOKEN:-}}"
  local release_registry_token="${CARGO_REGISTRY_TOKEN:-${CARGO_REGISTRIES_CRATES_IO_TOKEN:-}}"
  unset GIT_TOKEN GITHUB_TOKEN GH_TOKEN CARGO_REGISTRY_TOKEN CARGO_REGISTRIES_CRATES_IO_TOKEN

  cd "$REPO_ROOT"
  require_tool release-plz "$RELEASE_PLZ_VERSION"
  case "$command" in
    check | release-pr | update)
      require_tool cargo-semver-checks "$SEMVER_CHECKS_VERSION"
      ;;
  esac
  if [[ "$command" == "release-pr" || ( "$command" == "release" && "$dry_run" == false ) ]]; then
    if [[ -z "$release_git_token" ]]; then
      echo "$command requires GIT_TOKEN or GITHUB_TOKEN (a PAT or GitHub App token)" >&2
      exit 1
    fi
  fi
  if [[ "$command" == "release" && "$dry_run" == false && -z "$release_registry_token" ]]; then
    echo "release requires CARGO_REGISTRY_TOKEN or CARGO_REGISTRIES_CRATES_IO_TOKEN; cargo login credentials are not used" >&2
    exit 1
  fi

  scripts/check_publish_policy.py
  python3 scripts/check_crate_archives.py

  if [[ "$command" == "check" ]]; then
    echo "release preflight passed"
    return
  fi
  if [[ "$command" == "release-pr" || ( "$command" == "release" && "$dry_run" == false ) ]]; then
    verify_release_source "$release_git_token"
  fi

  if [[ -n "$release_git_token" ]]; then
    export GIT_TOKEN="$release_git_token"
  fi
  if [[ "$command" == "release" && "$dry_run" == false ]]; then
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
    unset GIT_TOKEN GITHUB_TOKEN GH_TOKEN CARGO_REGISTRY_TOKEN CARGO_REGISTRIES_CRATES_IO_TOKEN
    if ! tool_version_matches release-plz "$RELEASE_PLZ_VERSION" ||
      ! tool_version_matches cargo-semver-checks "$SEMVER_CHECKS_VERSION"; then
      rustup toolchain install "$RELEASE_TOOLS_TOOLCHAIN" --profile minimal --no-self-update
    fi
    install_tool release-plz "$RELEASE_PLZ_VERSION"
    install_tool cargo-semver-checks "$SEMVER_CHECKS_VERSION"
    ;;
  release-dry-run)
    shift
    run_release_plz release --dry-run "$@"
    ;;
  check | release | release-pr | update | set-version)
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
