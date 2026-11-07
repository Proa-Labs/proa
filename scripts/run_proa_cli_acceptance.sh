#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

# proa-cli integration targets, partitioned across the main shards. Every
# target must appear exactly once; --validate checks that against cargo
# metadata so a new tests/*.rs cannot silently escape CI.
readonly -a MAIN_SHARD_1=(
  rsjs_explicit_island_static_scalar
  rsjs_shared_state_async_provider
  lifecycle_run_test
  lifecycle_start
)
readonly -a MAIN_SHARD_2=(
  lifecycle_check
  rsjs_linked_site_codegen
  rsjs_build_wrapper
  cli
  content_coverage
  live_lint
  markdown_syntax
  rsjs_final
)
readonly -a MAIN_SHARD_3=(
  rsjs_linked_helper_acceptance
  rsjs_shared_state_consumer
  lifecycle_build
)
readonly -a MAIN_SHARD_4=(
  rsjs_shared_state_provider_seed
  rsjs_pipeline
  lifecycle_dev
  rsjs_descriptor_cache
  rsjs_unit_projection
)

# Applications with real RSJS islands need the linked `proa test` pipeline.
# Pizza is a workspace member; ORIGIN is generated from the packaged scaffold.
# Keep each app's integration tests in one binary so the descriptor/final
# compilation runs once. The generated ORIGIN app also runs its library tests
# with plain Cargo.
readonly -a EXAMPLES=(pizza atlas)

# The pull-request proa-cli targets. PRs changing the linked pipeline must
# exercise cache recovery, complete carriers, mapping/projection freshness and
# generated runtime behavior. `cargo test` runs test binaries one after
# another, so the `pr` fan-out runs these as two concurrent halves (pr-1a and
# pr-1b) that share one prebuilt target directory. The halves are balanced on
# measured binary run times (helper acceptance ~106 s, build wrapper ~42 s,
# pipeline ~41 s, unit projection ~38 s, site codegen ~32 s, the rest under
# 5 s). Both lifecycle tests stay together because they bind network ports.
readonly -a PR_SHARD_1A=(
  rsjs_linked_helper_acceptance
  rsjs_linked_site_codegen
  rsjs_descriptor_cache
  lifecycle_dev
  lifecycle_start
)
readonly -a PR_SHARD_1B=(
  rsjs_build_wrapper
  rsjs_pipeline
  rsjs_unit_projection
  rsjs_final
  live_lint
  markdown_syntax
)
readonly -a PR_SHARD_1=("${PR_SHARD_1A[@]}" "${PR_SHARD_1B[@]}")

all_main_targets() {
  printf '%s\n' \
    "${MAIN_SHARD_1[@]}" \
    "${MAIN_SHARD_2[@]}" \
    "${MAIN_SHARD_3[@]}" \
    "${MAIN_SHARD_4[@]}"
}

discovered_integration_targets() {
  cargo metadata --locked --no-deps --format-version=1 |
    python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
package = next(package for package in metadata["packages"] if package["name"] == "proa-cli")
names = sorted(
    target["name"]
    for target in package["targets"]
    if "test" in target["kind"]
)
print("\n".join(names))
'
}

validate_main_partition() {
  local assigned discovered
  assigned="$(all_main_targets | LC_ALL=C sort)"
  discovered="$(discovered_integration_targets)"
  if [[ "$assigned" == "$discovered" ]]; then
    return
  fi

  echo "proa-cli acceptance shards do not cover every integration target exactly once" >&2
  diff -u \
    <(printf '%s\n' "$discovered") \
    <(printf '%s\n' "$assigned") >&2 || true
  return 1
}

# PROA_ACCEPTANCE_CLI names a proa executable built once by the `pr` fan-out.
# Without it (main shards, single-shard invocations) each example builds the
# CLI itself, as before.
run_example() {
  case "$1" in
    pizza)
      if [[ -n "${PROA_ACCEPTANCE_CLI:-}" ]]; then
        "$PROA_ACCEPTANCE_CLI" test --root . --locked \
          -p proa_webmcp_pizza --test e2e
      else
        cargo run --locked -p proa-cli --bin proa -- \
          test --root . --locked \
          -p proa_webmcp_pizza --test e2e
      fi
      ;;
    atlas)
      local cli="${PROA_ACCEPTANCE_CLI:-}"
      if [[ -z "$cli" ]]; then
        cargo build --locked -p proa-cli --bin proa
        cli="${CARGO_TARGET_DIR:-target}/debug/proa"
      fi
      python3 scripts/check_origin_scaffold.py --cli "$cli"
      ;;
    *)
      echo "unknown example: $1" >&2
      return 2
      ;;
  esac
}

# Compile proa-cli once before the pull-request shards fan out. Every shard
# used to build the CLI from scratch in its own target directory, three cold
# builds competing for one runner. Now `cargo test --no-run` with the full
# pr-1 target list does exactly the proa-cli halves' compile phase in their
# shared target directory (so both halves then start fully fresh and only run
# tests; Cargo holds its build-directory lock during compilation, not while
# tests execute), and the example shards execute a snapshot of that CLI
# instead of `cargo run`/`cargo build`. The snapshot lives outside every Cargo
# target directory, so no later Cargo invocation can replace it while a shard
# is executing it.
build_cli_prelude() {
  local target_dir="$1" snapshot="$2"
  local started
  started="$(date +%s)"
  local -a cargo_arguments=(test -p proa-cli --locked --no-run)
  local test_target
  for test_target in "${PR_SHARD_1[@]}"; do
    cargo_arguments+=(--test "$test_target")
  done
  CARGO_TARGET_DIR="$target_dir" cargo "${cargo_arguments[@]}"
  mkdir -p "$snapshot"
  cp "$target_dir/debug/proa" "$target_dir/debug/cargo-proa" "$snapshot/"
  echo "proa-cli built and snapshotted for the shards in $(( $(date +%s) - started ))s"
}

# Run several shards concurrently in this one process and report them as one
# result, so a pull request shows a single check instead of one per shard.
# Each shard is a full re-invocation of this script writing to its own log;
# the logs are replayed in collapsible groups once everything has finished.
# Give each shard its own outer Cargo target directory too: the linked
# pipeline's nested descriptor/final builds are keyed under it, and `cargo
# test` and `cargo run` can build different feature graphs and replace
# target/debug/proa while another shard is still executing it. Cargo's build
# lock does not cover the running CLI's lifetime or its later wrapper snapshot.
# The one exception is the pair of proa-cli halves: same package, same
# feature graph, everything prebuilt by the prelude, nothing left to replace.
shard_target_name() {
  case "$1" in
    pr-1a | pr-1b) echo pr-1 ;;
    *) echo "$1" ;;
  esac
}

run_shards_in_parallel() {
  local logs target_root
  logs="$(mktemp -d)"
  target_root="${CARGO_TARGET_DIR:-target}/acceptance-shards"
  local -a modes=("$@")
  local mode
  for mode in "${modes[@]}"; do
    (
      started="$(date +%s)"
      status=0
      CARGO_TARGET_DIR="$target_root/$(shard_target_name "$mode")" \
        "$0" "$mode" > "$logs/$mode.log" 2>&1 || status=$?
      printf '%s %s\n' "$(( $(date +%s) - started ))" "$status" > "$logs/$mode.status"
      exit "$status"
    ) &
  done

  local failed=0
  while true; do
    local pending=""
    for mode in "${modes[@]}"; do
      [[ -f "$logs/$mode.status" ]] || pending="$pending $mode"
    done
    [[ -n "$pending" ]] || break
    echo "linked acceptance shards still running:$pending"
    sleep 30
  done
  wait

  local seconds status
  for mode in "${modes[@]}"; do
    read -r seconds status < "$logs/$mode.status"
    if [[ "$status" == 0 ]]; then
      echo "::group::linked acceptance $mode: ok in ${seconds}s"
    else
      echo "::group::linked acceptance $mode: FAILED (exit $status) after ${seconds}s"
      failed=1
    fi
    cat "$logs/$mode.log"
    echo "::endgroup::"
  done
  for mode in "${modes[@]}"; do
    read -r seconds status < "$logs/$mode.status"
    printf 'linked acceptance %-6s %s (%ss)\n' "$mode" "$([[ "$status" == 0 ]] && echo ok || echo FAILED)" "$seconds"
  done
  rm -rf "$logs"
  return "$failed"
}

usage() {
  echo "usage: $0 {--validate|pr|pr-1|pr-1a|pr-1b|pr-2|pr-3|main-all|main-1|main-2|main-3|main-4|main-5}" >&2
}

readonly shard="${1:-}"
validate_main_partition

# The linked pipeline's descriptor/final compilation must execute its wrapper
# and side effects. A compiler-cache hit could bypass both, so acceptance runs
# never inherit a wrapper or a cache backend from the invoking environment.
# Cargo fingerprints include RUSTC_WRAPPER, so this precedes the prelude too.
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
unset SCCACHE_GHA_ENABLED SCCACHE_GHA_RW_MODE
unset SCCACHE_GCS_BUCKET SCCACHE_GCS_RW_MODE

# Pull requests run `pr`: the proa-cli pipeline acceptance suite as two
# halves and one shard per example, concurrently in one job so the pull
# request shows one check. Serially these took about 15 minutes on one runner. Main fans the proa-cli
# targets out four ways and gives the examples a fifth shard; the
# single-runner VPS rollback path runs everything in one job.
tests=()
examples=()
case "$shard" in
  --validate)
    exit 0
    ;;
  pr)
    target_root="${CARGO_TARGET_DIR:-target}/acceptance-shards"
    build_cli_prelude "$target_root/pr-1" "$target_root/cli"
    PROA_ACCEPTANCE_CLI="$(cd "$target_root/cli" && pwd)/proa" \
      run_shards_in_parallel pr-1a pr-1b pr-2 pr-3
    exit $?
    ;;
  pr-1)
    tests=("${PR_SHARD_1[@]}")
    ;;
  pr-1a)
    tests=("${PR_SHARD_1A[@]}")
    ;;
  pr-1b)
    tests=("${PR_SHARD_1B[@]}")
    ;;
  pr-2)
    examples=(pizza)
    ;;
  pr-3)
    examples=(atlas)
    ;;
  main-all)
    tests=(
      "${MAIN_SHARD_1[@]}"
      "${MAIN_SHARD_2[@]}"
      "${MAIN_SHARD_3[@]}"
      "${MAIN_SHARD_4[@]}"
    )
    examples=("${EXAMPLES[@]}")
    ;;
  main-1)
    tests=("${MAIN_SHARD_1[@]}")
    ;;
  main-2)
    tests=("${MAIN_SHARD_2[@]}")
    ;;
  main-3)
    tests=("${MAIN_SHARD_3[@]}")
    ;;
  main-4)
    tests=("${MAIN_SHARD_4[@]}")
    ;;
  main-5)
    examples=("${EXAMPLES[@]}")
    ;;
  *)
    usage
    exit 2
    ;;
esac

# Several fixture applications deliberately invoke Cargo with --offline. The
# former monolithic CI job populated their crates as an incidental side effect
# of earlier workspace tests; independent acceptance jobs must do so explicitly.
cargo fetch --locked

if ((${#tests[@]} > 0)); then
  cargo_arguments=(test -p proa-cli --locked)
  for test_target in "${tests[@]}"; do
    cargo_arguments+=(--test "$test_target")
  done

  printf 'running proa-cli acceptance targets:'
  printf ' %s' "${tests[@]}"
  printf '\n'
  cargo "${cargo_arguments[@]}"
fi

for example in ${examples[@]+"${examples[@]}"}; do
  echo "running linked example acceptance: $example"
  run_example "$example"
done
