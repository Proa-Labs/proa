// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for the microtask-batched signal scheduler. Run with:
//
//     node rsjs/runtime/rsjs-runtime.batching.test.mjs
//
// The runtime is dependency-free and the scheduler is a pure-JS
// concern (no DOM needed), so these tests just import the runtime,
// drive `signal()` writes, await microtask ticks, and assert on
// effect-run counts.
//
// We rely on the host's native `queueMicrotask`. `await Promise.resolve()`
// yields to the microtask queue, so a single `await` is sufficient to
// drain one flush; we use a small helper for readability.

import { batch, signal } from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

// Drain one microtask tick. The runtime's `schedule()` calls
// `queueMicrotask(flush)`, which runs after the currently executing
// JS plus any already-resolved promise reactions. Awaiting a resolved
// promise yields to the microtask queue, so by the time we resume
// the flush has already executed.
//
// IMPORTANT: this is NOT an async function. Wrapping `await
// Promise.resolve()` in `async function tick()` adds an extra
// microtask hop (the async function's own resolution), which would
// drain *two* flushes per call and mask "next-tick" behaviour. We
// expose the bare promise instead so each `await tick()` advances
// exactly one microtask.
function tick() {
    return Promise.resolve();
}

// Build an effect handle compatible with the runtime's internal
// shape: `{ deps: Set, run: Function }`. We subscribe via the
// signal's private `_subscribe` so the test does not depend on
// `bind_text` / DOM bindings.
function make_effect(fn) {
    return { deps: new Set(), run: fn };
}

// -------------------------------------------------------------------
// Test 1: three signal writes in one tick collapse to one effect run.
// -------------------------------------------------------------------
{
    const a = signal(0);
    const b = signal(0);
    const c = signal(0);
    let runs = 0;
    const eff = make_effect(() => {
        runs += 1;
    });
    a._subscribe(eff);
    b._subscribe(eff);
    c._subscribe(eff);

    runs = 0; // ignore any setup-phase triggering; we only count post-write runs

    a.set(1);
    b.set(2);
    c.set(3);

    // Synchronous reads must observe the latest values *before* the flush.
    assert(a.get() === 1, 'test 1: a.get() should observe latest value synchronously');
    assert(b.get() === 2, 'test 1: b.get() should observe latest value synchronously');
    assert(c.get() === 3, 'test 1: c.get() should observe latest value synchronously');

    // No effect should have run yet; flush is still pending on a microtask.
    assert(runs === 0, `test 1: expected 0 runs before tick, got ${runs}`);

    await tick();

    assert(
        runs === 1,
        `test 1: expected exactly 1 effect run after batched writes, got ${runs}`,
    );
}

// -------------------------------------------------------------------
// Test 2: writes from two separate ticks produce two flushes.
// -------------------------------------------------------------------
{
    const a = signal(0);
    let runs = 0;
    const eff = make_effect(() => {
        runs += 1;
    });
    a._subscribe(eff);

    runs = 0;

    a.set(1);
    await tick();
    const after_first = runs;

    a.set(2);
    await tick();
    const after_second = runs;

    assert(
        after_first === 1,
        `test 2: expected 1 run after first tick, got ${after_first}`,
    );
    assert(
        after_second === 2,
        `test 2: expected 2 total runs after second tick, got ${after_second}`,
    );
}

// -------------------------------------------------------------------
// Test 3: a write inside an effect (during flush) runs on the NEXT
// tick rather than recursing.
// -------------------------------------------------------------------
{
    const trigger = signal(0);
    const downstream = signal(0);

    let trigger_runs = 0;
    let downstream_runs = 0;

    // Effect on `trigger`: when it fires, it writes to `downstream`.
    // If the scheduler recursed, `downstream_eff` would run *during*
    // this same flush, before we yield. Instead, we expect it to be
    // queued for the next tick.
    const trigger_eff = make_effect(() => {
        trigger_runs += 1;
        downstream.set(downstream.peek() + 1);
    });
    const downstream_eff = make_effect(() => {
        downstream_runs += 1;
    });

    trigger._subscribe(trigger_eff);
    downstream._subscribe(downstream_eff);

    trigger_runs = 0;
    downstream_runs = 0;

    trigger.set(1);
    await tick();

    // First flush: trigger_eff runs, writes to downstream, downstream_eff
    // is queued for the next tick.
    assert(
        trigger_runs === 1,
        `test 3: expected 1 trigger run after first tick, got ${trigger_runs}`,
    );
    assert(
        downstream_runs === 0,
        `test 3: write-during-flush should NOT run downstream effect in same tick; got ${downstream_runs} runs`,
    );

    await tick();

    // Second flush: downstream_eff runs.
    assert(
        downstream_runs === 1,
        `test 3: expected downstream effect to run on next tick, got ${downstream_runs}`,
    );
    // Trigger should NOT re-run; only downstream was written this tick.
    assert(
        trigger_runs === 1,
        `test 3: trigger effect should not re-run, got ${trigger_runs}`,
    );
}

// -------------------------------------------------------------------
// Bonus: same signal written multiple times in one tick still only
// runs each subscriber once.
// -------------------------------------------------------------------
{
    const a = signal(0);
    let runs = 0;
    const eff = make_effect(() => {
        runs += 1;
    });
    a._subscribe(eff);
    runs = 0;

    a.set(1);
    a.set(2);
    a.set(3);
    a.set(4);

    assert(a.get() === 4, 'bonus: latest value must be observable synchronously');
    await tick();
    assert(runs === 1, `bonus: expected 1 run for repeated writes to same signal, got ${runs}`);
}

// -------------------------------------------------------------------
// Public batch contract: nested batches share one outermost commit.
//
// Writes are visible synchronously throughout the nested calls, but
// subscribers do not run at an inner boundary or when the outer callback
// returns. The scheduler commits the final values once in its next microtask.
// -------------------------------------------------------------------
{
    const left = signal(0);
    const right = signal(0);
    let runs = 0;
    const snapshots = [];
    const eff = make_effect(() => {
        runs += 1;
        snapshots.push([left.peek(), right.peek()]);
    });
    left._subscribe(eff);
    right._subscribe(eff);

    const result = batch(() => {
        left.set(1);
        assert(left.get() === 1, 'nested batch: outer write is synchronously readable');

        const inner_result = batch(() => {
            right.set(2);
            left.set(3);
            assert(left.get() === 3, 'nested batch: inner overwrite is synchronously readable');
            assert(right.get() === 2, 'nested batch: inner write is synchronously readable');
            assert(runs === 0, 'nested batch: inner boundary must not flush subscribers');
            return 'inner';
        });

        assert(inner_result === 'inner', 'nested batch: inner callback return value is preserved');
        assert(runs === 0, 'nested batch: subscribers remain deferred inside outer callback');
        assert(left.get() === 3, 'nested batch: outer callback sees the final inner value');
        return 'outer';
    });

    assert(result === 'outer', 'nested batch: outer callback return value is preserved');
    assert(runs === 0, 'nested batch: returning from outer batch does not flush synchronously');

    await tick();

    assert(runs === 1, `nested batch: expected one outermost flush, got ${runs}`);
    assert(
        snapshots.length === 1 && snapshots[0][0] === 3 && snapshots[0][1] === 2,
        `nested batch: subscriber should observe only final values, got ${JSON.stringify(snapshots)}`,
    );

    await tick();
    assert(runs === 1, 'nested batch: no extra flush is scheduled after the outermost commit');
}

// -------------------------------------------------------------------
// Public batch contract: an exception is rethrown without poisoning the
// scheduler. Writes made before the throw still commit once, and a later
// batch schedules and commits normally.
// -------------------------------------------------------------------
{
    const value = signal(0);
    let runs = 0;
    const snapshots = [];
    const eff = make_effect(() => {
        runs += 1;
        snapshots.push(value.peek());
    });
    value._subscribe(eff);

    const sentinel = new Error('batch sentinel');
    let caught;
    try {
        batch(() => {
            value.set(1);
            batch(() => {
                value.set(2);
                throw sentinel;
            });
        });
    } catch (error) {
        caught = error;
    }

    assert(caught === sentinel, 'throwing batch: the original exception is rethrown');
    assert(value.get() === 2, 'throwing batch: writes before the exception stay visible');
    assert(runs === 0, 'throwing batch: exception handling does not force a sync flush');

    await tick();

    assert(runs === 1, `throwing batch: queued writes should commit once, got ${runs}`);
    assert(
        snapshots.length === 1 && snapshots[0] === 2,
        `throwing batch: first commit should observe the last pre-throw value, got ${snapshots}`,
    );

    batch(() => {
        value.set(3);
    });
    assert(value.get() === 3, 'throwing batch: a later batch writes normally');
    assert(runs === 1, 'throwing batch: later writes remain deferred');

    await tick();

    assert(runs === 2, `throwing batch: scheduler should recover for later work, got ${runs}`);
    assert(
        snapshots.length === 2 && snapshots[1] === 3,
        `throwing batch: recovered scheduler should commit the later value, got ${snapshots}`,
    );
}

// -------------------------------------------------------------------
// Public batch contract: a batch is synchronous and never spans await.
// Native async callbacks are rejected before invocation; a non-async
// callback that returns a thenable is rejected when that result is observed.
// -------------------------------------------------------------------
{
    let native_async_started = false;
    let native_async_error;
    try {
        batch(async () => {
            native_async_started = true;
        });
    } catch (error) {
        native_async_error = error;
    }
    assert(
        native_async_error instanceof TypeError,
        'async batch: a native async callback is rejected',
    );
    assert(
        native_async_started === false,
        'async batch: a native async callback is rejected before invocation',
    );

    let promise_error;
    try {
        batch(() => Promise.resolve('later'));
    } catch (error) {
        promise_error = error;
    }
    assert(
        promise_error instanceof TypeError,
        'async batch: a Promise-returning callback is rejected',
    );
}

if (failed > 0) {
    console.error(`\n${failed} test(s) failed.`);
    process.exit(1);
}
console.log('All batching tests passed.');
