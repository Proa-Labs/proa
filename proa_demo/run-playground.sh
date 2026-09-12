#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo ""
echo "  ╔══════════════════════════════════════╗"
echo "  ║       Proa Playground Setup          ║"
echo "  ╚══════════════════════════════════════╝"
echo ""

# Check Rust toolchain
if ! command -v cargo &>/dev/null; then
  echo "  Error: cargo not found. Install Rust: https://rustup.rs"
  exit 1
fi

# Check Node/pnpm — needed for the React and Next.js comparison engines.
# Proa rendering works without them, but the A/B comparison won't.
HAS_NODE=false
HAS_PNPM=false
HAS_NPM=false
if command -v node &>/dev/null; then HAS_NODE=true; fi
if command -v pnpm &>/dev/null; then HAS_PNPM=true; fi
if command -v npm &>/dev/null; then HAS_NPM=true; fi

if ! $HAS_NODE; then
  echo "  Warning: node not found — React (Node), React SSR, and Next.js comparisons will be unavailable"
  echo "           Install Node: https://nodejs.org"
fi
if ! $HAS_PNPM; then
  echo "  Warning: pnpm not found — React dependency install will be skipped"
  echo "           Install pnpm: npm install -g pnpm"
fi

HAS_BUN=false
if command -v bun &>/dev/null; then HAS_BUN=true; fi
if ! $HAS_BUN; then
  echo "  Warning: bun not found — React (Bun) comparison will be unavailable"
  echo "           Install Bun: https://bun.sh"
fi
echo ""

# 1. Install shared React components if needed
if $HAS_PNPM; then
  SHARED_DIR="$SCRIPT_DIR/react-ssr-shared"
  if [ -d "$SHARED_DIR" ] && [ ! -d "$SHARED_DIR/node_modules" ]; then
    echo "  Installing shared React SSR dependencies..."
    cd "$SHARED_DIR" && pnpm install --silent 2>/dev/null && cd "$ROOT_DIR"
  fi

  # 2. Install Hono SSR server if needed
  HONO_DIR="$SCRIPT_DIR/hono-ssr"
  if [ -d "$HONO_DIR" ] && [ ! -d "$HONO_DIR/node_modules" ]; then
    echo "  Installing Hono SSR dependencies..."
    cd "$HONO_DIR" && pnpm install --silent 2>/dev/null && cd "$ROOT_DIR"
  fi
fi

if $HAS_NODE; then
  MARKO_DIR="$SCRIPT_DIR/marko-server"
  if [ -d "$MARKO_DIR" ]; then
    if [ ! -d "$MARKO_DIR/node_modules" ]; then
      if $HAS_NPM; then
        echo "  Installing Marko SSR dependencies..."
        cd "$MARKO_DIR" && npm install --silent && cd "$ROOT_DIR"
      else
        echo "  Warning: npm not found — Marko dependency install will be skipped"
      fi
    fi
  fi

  REACT_SSR_DIR="$SCRIPT_DIR/react-ssr"
  if [ -d "$REACT_SSR_DIR" ]; then
    if [ ! -d "$REACT_SSR_DIR/node_modules" ]; then
      if $HAS_NPM; then
        echo "  Installing React SSR dependencies..."
        cd "$REACT_SSR_DIR" && npm install --silent && cd "$ROOT_DIR"
      else
        echo "  Warning: npm not found — React SSR dependency install will be skipped"
      fi
    fi
  fi

  NEXT_DIR="$SCRIPT_DIR/next-ssr"
  if [ -d "$NEXT_DIR" ]; then
    if [ ! -d "$NEXT_DIR/node_modules" ]; then
      echo "  Installing Next.js SSR dependencies..."
      cd "$NEXT_DIR" && npm install --silent 2>/dev/null && cd "$ROOT_DIR"
    fi
    NEXT_BUILD_ID="$NEXT_DIR/.next/BUILD_ID"
    NEXT_BUILD_STALE=false
    if [ -f "$NEXT_BUILD_ID" ]; then
      if find \
        "$NEXT_DIR/app" \
        "$NEXT_DIR/lib" \
        "$NEXT_DIR/next.config.mjs" \
        "$SCRIPT_DIR/react-ssr/page.tsx" \
        "$SCRIPT_DIR/react-ssr-shared/src" \
        -type f -newer "$NEXT_BUILD_ID" 2>/dev/null | grep -q .; then
        NEXT_BUILD_STALE=true
      fi
    fi
    if [ ! -f "$NEXT_BUILD_ID" ] || [ "$NEXT_BUILD_STALE" = true ]; then
      echo "  Building Next.js SSR demo..."
      cd "$NEXT_DIR" && npm run build --silent && cd "$ROOT_DIR"
    fi
  fi
fi

# 3. Build the Rust playground
echo "  Building Proa playground (release)..."
cd "$ROOT_DIR"
cargo build -p proa_demo --bin proa-playground --features server --release 2>&1 | grep -v "^warning" || true
echo ""

# 4. Run the binary directly (faster than cargo run)
echo "  Starting playground..."
echo ""
exec "$ROOT_DIR/target/release/proa-playground"
