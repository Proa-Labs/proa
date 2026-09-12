#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# A portable single-instance guard for long-running watch scripts.
#
# `mkdir` is the atomic operation. The PID file lets a later invocation
# distinguish an active owner from a stale lock left by an unclean shutdown.

_PROA_WATCH_LOCK_DIR=""
_PROA_WATCH_LOCK_OWNER_PID=""
_PROA_WATCH_CHILD_PID=""
_PROA_WATCH_CHILD_PGID=""

proa_watch_lock_release() {
  local owner_pid=""

  if [[ -z "$_PROA_WATCH_LOCK_DIR" ]] || [[ ! -d "$_PROA_WATCH_LOCK_DIR" ]]; then
    return
  fi

  if [[ -r "$_PROA_WATCH_LOCK_DIR/pid" ]]; then
    IFS= read -r owner_pid <"$_PROA_WATCH_LOCK_DIR/pid" || true
  fi

  # Never remove a lock that another process acquired after this one.
  if [[ "$owner_pid" != "$_PROA_WATCH_LOCK_OWNER_PID" ]]; then
    return
  fi

  rm -f "$_PROA_WATCH_LOCK_DIR/pid"
  rmdir "$_PROA_WATCH_LOCK_DIR" 2>/dev/null || true
}

proa_watch_lock_handle_signal() {
  local signal_name="$1"
  local exit_status="$2"

  # Do not let a second terminal signal interrupt cleanup and strand a child.
  trap '' HUP INT TERM
  proa_watch_stop_child_group "$signal_name"
  exit "$exit_status"
}

proa_watch_child_group_is_alive() {
  local child_pgid="$1"

  [[ "$child_pgid" =~ ^[0-9]+$ ]] &&
    kill -0 -- "-$child_pgid" 2>/dev/null
}

proa_watch_escalate_child_group() {
  local child_pgid="$1"
  local attempt

  # Give the watcher a brief chance to handle the original signal, then make
  # shutdown bounded even when it or a descendant ignores INT/TERM.
  for attempt in {1..5}; do
    sleep 0.05
    proa_watch_child_group_is_alive "$child_pgid" || return
  done
  kill -TERM -- "-$child_pgid" 2>/dev/null || return

  for attempt in {1..5}; do
    sleep 0.05
    proa_watch_child_group_is_alive "$child_pgid" || return
  done
  kill -KILL -- "-$child_pgid" 2>/dev/null || true
}

proa_watch_stop_child_group() {
  local signal_name="$1"
  local child_pid="$_PROA_WATCH_CHILD_PID"
  local child_pgid="$_PROA_WATCH_CHILD_PGID"
  local escalation_pid=""

  if proa_watch_child_group_is_alive "$child_pgid"; then
    kill "-$signal_name" -- "-$child_pgid" 2>/dev/null || true
    proa_watch_escalate_child_group "$child_pgid" &
    escalation_pid="$!"
  fi

  if [[ "$child_pid" =~ ^[0-9]+$ ]]; then
    wait "$child_pid" 2>/dev/null || true
  fi
  if [[ "$escalation_pid" =~ ^[0-9]+$ ]]; then
    wait "$escalation_pid" 2>/dev/null || true
  fi

  _PROA_WATCH_CHILD_PID=""
  _PROA_WATCH_CHILD_PGID=""
}

proa_watch_run() {
  local exit_status restore_monitor=0

  # A separate process group lets Ctrl-C reach this supervisor first. It can
  # then stop the watcher and every command it has spawned together.
  if [[ "$-" != *m* ]]; then
    set -m
    restore_monitor=1
  fi
  "$@" &
  _PROA_WATCH_CHILD_PID="$!"
  _PROA_WATCH_CHILD_PGID="$_PROA_WATCH_CHILD_PID"
  if ((restore_monitor)); then
    set +m
  fi

  # Once the supervisor exists, make it the advertised lock owner. If this
  # wrapper is killed uncleanly, a surviving watcher still keeps the lock live.
  if [[ -n "$_PROA_WATCH_LOCK_DIR" ]] && [[ -d "$_PROA_WATCH_LOCK_DIR" ]]; then
    if ! printf '%s\n' "$_PROA_WATCH_CHILD_PID" >"$_PROA_WATCH_LOCK_DIR/pid"; then
      proa_watch_stop_child_group TERM
      echo "Could not update the watcher lock owner." >&2
      return 1
    fi
    _PROA_WATCH_LOCK_OWNER_PID="$_PROA_WATCH_CHILD_PID"
  fi

  if wait "$_PROA_WATCH_CHILD_PID"; then
    exit_status=0
  else
    exit_status=$?
  fi
  _PROA_WATCH_CHILD_PID=""
  _PROA_WATCH_CHILD_PGID=""
  return "$exit_status"
}

proa_watch_lock_acquire() {
  local lock_name="$1"
  local workspace_root="$2"
  local lock_root="${PROA_WATCH_LOCK_ROOT:-$workspace_root/target/watch-locks}"
  local lock_dir="$lock_root/$lock_name.lock"
  local owner_pid=""
  local stale_dir=""
  local attempt=0

  mkdir -p "$lock_root"

  while ((attempt < 3)); do
    attempt=$((attempt + 1))

    if mkdir "$lock_dir" 2>/dev/null; then
      if ! printf '%s\n' "$$" >"$lock_dir/pid"; then
        rmdir "$lock_dir" 2>/dev/null || true
        echo "Could not record the $lock_name watcher lock owner." >&2
        return 1
      fi

      _PROA_WATCH_LOCK_DIR="$lock_dir"
      _PROA_WATCH_LOCK_OWNER_PID="$$"
      trap proa_watch_lock_release EXIT
      trap 'proa_watch_lock_handle_signal HUP 129' HUP
      trap 'proa_watch_lock_handle_signal INT 130' INT
      trap 'proa_watch_lock_handle_signal TERM 143' TERM
      return
    fi

    owner_pid=""
    if [[ -r "$lock_dir/pid" ]]; then
      IFS= read -r owner_pid <"$lock_dir/pid" || true
    else
      # Give a process that just created the directory time to write its PID.
      sleep 1
      if [[ -r "$lock_dir/pid" ]]; then
        IFS= read -r owner_pid <"$lock_dir/pid" || true
      fi
    fi

    if [[ "$owner_pid" =~ ^[0-9]+$ ]] && kill -0 "$owner_pid" 2>/dev/null; then
      echo "$lock_name watcher is already running (PID $owner_pid)." >&2
      echo "Stop that watcher before starting another instance." >&2
      return 1
    fi

    # Rename first so two processes cannot both reclaim the same stale lock.
    stale_dir="$lock_dir.stale.$$.$RANDOM"
    if mv "$lock_dir" "$stale_dir" 2>/dev/null; then
      rm -f "$stale_dir/pid"
      if ! rmdir "$stale_dir" 2>/dev/null; then
        echo "Stale watcher lock contains unexpected files: $stale_dir" >&2
        return 1
      fi
    fi
  done

  echo "Could not acquire the $lock_name watcher lock at $lock_dir." >&2
  return 1
}
