#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/watch-lock.sh"

TAILWIND_VERSION="${TAILWIND_VERSION:-3.4.18}"
export CARGO_TARGET_DIR="${PROA_MARKETING_CARGO_TARGET_DIR:-${CARGO_TARGET_DIR:-$ROOT/target/watch/proa-marketing}}"

tailwind_asset() {
  local os arch ext

  case "$(uname -s)" in
    Darwin) os="macos" ;;
    Linux) os="linux" ;;
    MINGW* | MSYS* | CYGWIN*) os="windows" ;;
    *)
      echo "Unsupported OS for Tailwind standalone CLI: $(uname -s)" >&2
      exit 1
      ;;
  esac

  case "$(uname -m)" in
    arm64 | aarch64) arch="arm64" ;;
    x86_64 | amd64) arch="x64" ;;
    *)
      echo "Unsupported architecture for Tailwind standalone CLI: $(uname -m)" >&2
      exit 1
      ;;
  esac

  if [[ "$os" == "windows" ]]; then
    ext=".exe"
  else
    ext=""
  fi

  printf 'tailwindcss-%s-%s%s\n' "$os" "$arch" "$ext"
}

prepare_cached_tailwind() {
  local candidate="$1"

  if [[ "$(uname -s)" == "Darwin" ]] && [[ "$candidate" == "$ROOT"/.tools/* ]] && command -v xattr >/dev/null 2>&1; then
    xattr -c "$candidate" 2>/dev/null || true
  fi
}

install_tailwind() {
  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to install the Tailwind standalone CLI. Install curl or set TAILWIND_BIN." >&2
    exit 1
  fi

  local asset dir dest tmp url
  asset="$(tailwind_asset)"
  dir="$ROOT/.tools/tailwindcss/v$TAILWIND_VERSION"
  dest="$dir/$asset"
  tmp="$dest.tmp"
  url="https://github.com/tailwindlabs/tailwindcss/releases/download/v$TAILWIND_VERSION/$asset"

  mkdir -p "$dir"
  echo "Installing Tailwind CSS standalone CLI v$TAILWIND_VERSION..." >&2
  if ! curl -fsSL --retry 3 -o "$tmp" "$url"; then
    rm -f "$tmp"
    echo "Failed to download Tailwind standalone CLI from $url" >&2
    exit 1
  fi
  if ! chmod +x "$tmp" || ! mv "$tmp" "$dest"; then
    rm -f "$tmp"
    echo "Failed to install Tailwind standalone CLI at $dest" >&2
    exit 1
  fi
  prepare_cached_tailwind "$dest"
  printf '%s\n' "$dest"
}

find_tailwind() {
  if [[ -n "${TAILWIND_BIN:-}" ]]; then
    printf '%s\n' "$TAILWIND_BIN"
    return
  fi

  local asset cached
  asset="$(tailwind_asset)"
  cached="$ROOT/.tools/tailwindcss/v$TAILWIND_VERSION/$asset"

  for candidate in \
    "$cached" \
    "$ROOT/node_modules/.bin/tailwindcss"
  do
    if [[ -x "$candidate" ]]; then
      prepare_cached_tailwind "$candidate"
      printf '%s\n' "$candidate"
      return
    fi
  done

  if command -v tailwindcss >/dev/null 2>&1; then
    command -v tailwindcss
    return
  fi

  install_tailwind
}

build_css() (
  local output status tailwind tmp
  tailwind="$(find_tailwind)"
  output="$ROOT/proa_marketing/public/styles.css"
  mkdir -p "$CARGO_TARGET_DIR/asset-tmp"
  tmp="$(mktemp "$CARGO_TARGET_DIR/asset-tmp/marketing-styles.XXXXXX")"
  trap '[[ -z "$tmp" ]] || rm -f -- "$tmp"' EXIT

  if (
      cd "$ROOT/proa_marketing"
      "$tailwind" \
        -c styles/tailwind.config.cjs \
        -i styles/input.css \
        -o "$tmp" \
        --minify
    ); then
    :
  else
    status=$?
    return "$status"
  fi

  if [[ -f "$output" ]] && cmp -s -- "$tmp" "$output"; then
    return
  fi

  chmod 0644 "$tmp"
  mv -f "$tmp" "$output"
  tmp=""
)

marketing_port() {
  if [[ "${PORT:-}" =~ ^[0-9]+$ ]]; then
    printf '%s\n' "$PORT"
  else
    printf '5555\n'
  fi
}

port_in_use() {
  local port="$1"
  command -v lsof >/dev/null 2>&1 || return 1
  lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
}

assert_marketing_port_available() {
  local port next_port
  port="$(marketing_port)"
  next_port="$((port + 1))"

  if port_in_use "$port"; then
    echo "Port $port is already in use. proa_marketing defaults to PORT=5555." >&2
    echo "Inspect it with: lsof -nP -iTCP:$port -sTCP:LISTEN" >&2
    echo "Stop that process or run on another port: PORT=$next_port bash scripts/watch-marketing.sh" >&2
    exit 1
  fi

  echo "Starting proa_marketing on ${HOST:-0.0.0.0}:$port" >&2
}

if [[ "${PROA_MARKETING_BUILD_CSS_ONLY:-}" == "1" ]]; then
  build_css
  exit 0
fi

if [[ "${PROA_MARKETING_WATCH_RUN:-}" == "1" ]]; then
  build_css
  assert_marketing_port_available
  # `cargo proa run`, not `cargo run`: the RSJS descriptor pass emits
  # `__rsjs_unlinked_helper_edge_N` placeholders at client-helper call sites, and
  # only the linked pipeline resolves them into real helper exports. A plain
  # cargo build serves those placeholders to the browser, where hydration dies
  # with `ReferenceError: __rsjs_unlinked_helper_edge_N is not defined`.
  exec cargo proa run --root "$ROOT" -p proa_marketing
fi

if ! cargo watch --version >/dev/null 2>&1; then
  echo "cargo-watch is required. Install it with: cargo install cargo-watch" >&2
  exit 1
fi

proa_watch_lock_acquire "proa-marketing" "$ROOT"
assert_marketing_port_available
build_css

# Watch paths are explicit, so repo-wide ignore-file discovery is unnecessary.
# Keep this shell alive so its EXIT trap releases the single-instance lock.
proa_watch_run cargo watch \
  --no-vcs-ignores \
  --no-dot-ignores \
  --clear \
  -w proa_marketing/src \
  -w proa_marketing/build.rs \
  -w proa_marketing/Cargo.toml \
  -w proa_marketing/styles \
  -w proa_marketing/public \
  -w proa_docs/proa_docs/src \
  -w proa_docs/proa_docs_build/src \
  -w Cargo.toml \
  -w Cargo.lock \
  -i target \
  -i proa_marketing/public/styles.css \
  -i 'proa_marketing/public/styles.css.tmp.*' \
  -i proa_marketing/public/docs.js \
  -s "PROA_MARKETING_WATCH_RUN=1 '$ROOT/scripts/watch-marketing.sh'"
