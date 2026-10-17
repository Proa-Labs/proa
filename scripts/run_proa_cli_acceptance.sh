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
)
readonly -a MAIN_SHARD_2=(
  lifecycle_check
  rsjs_linked_site_codegen
  rsjs_build_wrapper
  cli
  content_coverage
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

# Workspace examples with real RSJS islands. Plain `cargo test` correctly
# rejects their integration targets at link time, so they run through the
# linked `proa test` pipeline here. Each example is one linked build
# (descriptor plus final target) of a few hundred crates from a cold runner,
# which is why each example keeps every integration test in a single test
# binary: `proa test` accepts one --test per invocation and shares nothing
# between invocations. Lib unit tests are a separate matter: they link no
# island rlib from a dependency, so the plain-cargo `test (libraries)` lane
# runs them and this script must not duplicate them.
readonly -a EXAMPLES=(pizza atlas)

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

run_example() {
  case "$1" in
    pizza)
      cargo run --locked -p proa-cli --bin proa -- \
        test --root . --locked \
        -p proa_webmcp_pizza --test e2e
      ;;
    atlas)
      cargo run --locked -p proa-cli --bin proa -- \
        test --root . --locked \
        -p proa_origin_routing_atlas --test atlas
      ;;
    *)
      echo "unknown example: $1" >&2
      return 2
      ;;
  esac
}

# Run several shards concurrently in this one process and report them as one
# result, so a pull request shows a single check instead of one per shard.
# Each shard is a full re-invocation of this script writing to its own log;
# the logs are replayed in collapsible groups once everything has finished.
# Cargo serializes the shared target/debug builds of proa-cli itself, but the
# linked descriptor/final builds live in per-invocation directories and run
# side by side.
run_shards_in_parallel() {
  local logs
  logs="$(mktemp -d)"
  local -a modes=("$@")
  local mode
  for mode in "${modes[@]}"; do
    (
      started="$(date +%s)"
      status=0
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
  echo "usage: $0 {--validate|pr|pr-1|pr-2|pr-3|main-all|main-1|main-2|main-3|main-4|main-5}" >&2
}

readonly shard="${1:-}"
validate_main_partition

# Pull requests run `pr`: the proa-cli pipeline acceptance suite and one shard
# per example, concurrently in one job so the pull request shows one check.
# Serially these took about 15 minutes on one runner. Main fans the proa-cli
# targets out four ways and gives the examples a fifth shard; the
# single-runner VPS rollback path runs everything in one job.
tests=()
examples=()
case "$shard" in
  --validate)
    exit 0
    ;;
  pr)
    run_shards_in_parallel pr-1 pr-2 pr-3
    exit $?
    ;;
  pr-1)
    # PRs changing the linked pipeline must exercise cache recovery, complete
    # carriers, mapping/projection freshness and generated runtime behavior.
    tests=(
      rsjs_linked_helper_acceptance
      rsjs_build_wrapper
      rsjs_descriptor_cache
      rsjs_final
      rsjs_linked_site_codegen
      rsjs_unit_projection
      rsjs_pipeline
      lifecycle_dev
    )
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

# The linked pipeline's descriptor/final compilation must execute its wrapper
# and side effects. A compiler-cache hit could bypass both, so acceptance runs
# never inherit a wrapper or a cache backend from the invoking environment.
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
unset SCCACHE_GHA_ENABLED SCCACHE_GHA_RW_MODE
unset SCCACHE_GCS_BUCKET SCCACHE_GCS_RW_MODE

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
