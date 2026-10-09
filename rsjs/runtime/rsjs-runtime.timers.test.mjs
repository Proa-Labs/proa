// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for the sanctioned timer primitives interval() and debounce().
// Run with:
//
//     node rsjs/runtime/rsjs-runtime.timers.test.mjs
//
// Uses real (short) timers; each wait is generous enough to be stable
// on a loaded CI machine while keeping the whole file under a second.

import { interval, debounce } from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

// --- interval ---------------------------------------------------------------

{
    let ticks = 0;
    const dispose = interval(20, () => {
        ticks += 1;
    });
    await sleep(90);
    dispose();
    const at_dispose = ticks;
    assert(at_dispose >= 2, `interval fired repeatedly (got ${at_dispose})`);
    await sleep(60);
    assert(ticks === at_dispose, 'disposed interval stops firing');
    dispose(); // double-dispose is a no-op
}

// --- debounce: trailing edge, latest args -----------------------------------

{
    const calls = [];
    const search = debounce((q) => {
        calls.push(q);
    }, 30);
    search('a');
    search('ab');
    search('abc');
    assert(calls.length === 0, 'debounce does not fire on the leading edge');
    await sleep(80);
    assert(calls.length === 1, `burst collapses to one call (got ${calls.length})`);
    assert(calls[0] === 'abc', 'debounce delivers the most recent arguments');

    search('next');
    await sleep(80);
    assert(calls.length === 2 && calls[1] === 'next', 'debounce fires again after settling');
}

// --- debounce: dispose cancels pending invocation ---------------------------

{
    let fired = 0;
    const handler = debounce(() => {
        fired += 1;
    }, 30);
    handler();
    handler._dispose();
    await sleep(80);
    assert(fired === 0, 'dispose cancels the pending trailing call');
}

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.timers.test.mjs: all assertions passed');
