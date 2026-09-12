// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for computed() derived cells. Run with:
//
//     node rsjs/runtime/rsjs-runtime.computed.test.mjs
//
// computed() is a pure scheduler concern (no DOM needed): these tests
// drive source signals, await microtask ticks, and assert on derived
// values, recompute counts, and downstream effect runs.

import { signal, computed, bind_text } from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

// One microtask tick; see rsjs-runtime.batching.test.mjs for why this
// is a bare promise rather than an async function.
function tick() {
    return Promise.resolve();
}

// --- tracked-deps path -----------------------------------------------------

{
    const first = signal('Ada');
    const last = signal('Lovelace');
    let runs = 0;
    const full = computed(() => {
        runs += 1;
        return `${first.get()} ${last.get()}`;
    });

    assert(full.get() === 'Ada Lovelace', 'computed seeds from a tracked first run');
    assert(runs === 1, 'tracked path evaluates exactly once at creation');

    first.set('Grace');
    last.set('Hopper');
    await tick();
    assert(runs === 2, 'two source writes in one tick collapse to one recompute');
    assert(full.get() === 'Grace Hopper', 'computed sees both writes after the flush');

    full._dispose();
    first.set('Alan');
    await tick();
    assert(runs === 2, 'disposed computed no longer recomputes');
    assert(full.peek() === 'Grace Hopper', 'disposed computed keeps its last value');
}

// --- explicit-deps path (the compile-time dep-list shape) ------------------

{
    const count = signal(2);
    let runs = 0;
    const doubled = computed(() => {
        runs += 1;
        return count.get() * 2;
    }, [count]);

    assert(doubled.get() === 4, 'explicit-deps computed seeds at creation');
    count.set(5);
    await tick();
    assert(doubled.get() === 10, 'explicit-deps computed recomputes on source write');
    assert(runs === 2, 'explicit-deps path runs once per flush');
}

// --- equality short-circuit ------------------------------------------------

{
    const n = signal(1);
    const parity = computed(() => (n.get() % 2 === 0 ? 'even' : 'odd'));

    let downstream_runs = 0;
    const probe = { active: true, deps: new Set(), run: () => { downstream_runs += 1; } };
    parity._subscribe(probe);

    n.set(3); // odd -> odd: derived value unchanged
    await tick();
    await tick();
    assert(downstream_runs === 0, 'unchanged derived value does not notify subscribers');

    n.set(4); // odd -> even
    await tick(); // computed recomputes
    await tick(); // downstream observes the inner write (write-during-flush)
    assert(downstream_runs === 1, 'changed derived value notifies subscribers once');
    assert(parity.get() === 'even', 'derived value updated');
}

// --- composes with bind_text deps ------------------------------------------

{
    const price = signal(10);
    const total = computed(() => `$${price.get() * 3}`, [price]);
    const node = { textContent: '' };
    bind_text(node, () => total.get(), [total]);
    assert(node.textContent === '$30', 'bind_text seeds from a computed dep');

    price.set(20);
    await tick(); // computed recomputes
    await tick(); // text binding observes the computed write
    assert(node.textContent === '$60', 'bind_text re-applies when the computed changes');
}

// --- read-only surface -----------------------------------------------------

{
    const c = computed(() => 1);
    let threw = false;
    try {
        c.set(2);
    } catch {
        threw = true;
    }
    assert(threw, 'computed.set throws');
    threw = false;
    try {
        c.update((v) => v + 1);
    } catch {
        threw = true;
    }
    assert(threw, 'computed.update throws');
}

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.computed.test.mjs: all assertions passed');
