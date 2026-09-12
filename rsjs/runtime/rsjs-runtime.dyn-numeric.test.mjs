// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Regression suite for representation-agnostic shared-state arithmetic.
//
// The island compiler cannot resolve a shared field's Rust type, so it emits
// `num_*` helper calls instead of bare JS operators and lets the page seed
// decide whether the value is a BigInt or a Number. Two invariants make that
// sound, and both are exercised here:
//
//   1. The helpers agree with Rust semantics for every representation pairing.
//   2. A shared field cell cannot drift off the representation the seed pinned,
//      no matter which write path reaches it (`set`, `update`, `batch`,
//      `bind_input`).
//
// Invariant 2 is also the fix for the `bind_input` poisoning bug: a number
// input used to write the raw DOM string into a BigInt-backed, page-scoped
// cell, corrupting that field for every island on the page.

import {
    acquire_shared_state,
    batch,
    bind_input,
    dispose_page_shared_state,
    install_page_shared_state,
    num_add,
    num_div,
    num_div_int,
    num_eq,
    num_mul,
    num_ne,
    num_rem,
    num_sub,
} from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function tick() {
    return Promise.resolve();
}

function root(binding) {
    const name = `dyn_test_${binding.scope_id}`;
    const script = {
        tagName: 'SCRIPT',
        textContent: JSON.stringify({ signals: [], shared: [binding] }),
        getAttribute(attribute) {
            if (attribute === 'type') return 'application/json';
            if (attribute === 'data-rsjs-island') return name;
            return null;
        },
    };
    return {
        nextElementSibling: script,
        getAttribute(attribute) {
            if (attribute === 'data-rsjs-island-root') return name;
            return null;
        },
    };
}

function input_node(initial) {
    const listeners = new Map();
    return {
        value: initial,
        addEventListener(name, fn) {
            listeners.set(name, fn);
        },
        removeEventListener(name) {
            listeners.delete(name);
        },
        type_text(next) {
            this.value = next;
            listeners.get('input')?.();
        },
    };
}

// ---------------------------------------------------------------------------
// 1. Helper semantics
// ---------------------------------------------------------------------------

assert(num_add(3, 1) === 4, 'Number + Number stays a Number');
assert(
    num_add(9007199254740993n, 1) === 9007199254740994n,
    'a BigInt operand widens the Number literal instead of throwing a TypeError',
);
assert(num_add(1, 9007199254740993n) === 9007199254740994n, 'widening works on either side');
assert(num_add('8', 1) === '81', 'string concatenation still behaves like bare `+`');
assert(num_sub(5n, 2) === 3n, 'num_sub widens');
assert(num_mul(5n, 2) === 10n, 'num_mul widens');
assert(num_rem(7n, 2) === 1n, 'num_rem widens');

// Rust integer division truncates toward zero for both representations, and
// `Math.trunc` throws on a BigInt, which is exactly why this is a helper.
assert(num_div_int(-7, 2) === -3, 'Number integer division truncates toward zero');
assert(num_div_int(-7n, 2n) === -3n, 'BigInt integer division truncates toward zero');
assert(num_div_int(-7n, 2) === -3n, 'mixed integer division truncates toward zero');
assert(num_div(7, 2) === 3.5, 'float division does not truncate');
assert(num_div(7n, 2n) === 3.5, 'float division over BigInts does not truncate');

// A bare `===` would make `count.get() == 0` permanently false against a
// BigInt-backed cell.
assert(num_eq(5n, 5) === true, 'num_eq compares across representations');
assert(num_eq(5, 5) === true, 'num_eq compares Numbers');
assert(num_eq(5, '5') === false, 'num_eq does not coerce strings');
assert(num_eq(null, undefined) === false, 'num_eq does not inherit loose null equality');
assert(num_ne(5n, 6) === true, 'num_ne is the negation of num_eq');

let fractional;
try {
    num_add(9007199254740993n, 1.5);
} catch (error) {
    fractional = error;
}
assert(
    fractional?.code === 'rsjs/mixed-width-arithmetic',
    'a fractional Number meeting a BigInt is a named rsjs error, not a silent truncation',
);

// ---------------------------------------------------------------------------
// 2. Representation-pinned cells
// ---------------------------------------------------------------------------

const wide_binding = {
    scope_id: 40,
    type_key: 'test.dyn-cart',
    schema_hash: 'sha256:dyn',
    rust_type: 'CartState',
};

dispose_page_shared_state();
install_page_shared_state({
    abi_version: 1,
    stores: [
        {
            ...wide_binding,
            state: {
                count: '9007199254740993',
                label: 'cart',
                maybe: null,
                rows: [{ id: '9007199254740993' }],
            },
            wide_paths: [['count'], ['maybe'], ['rows', '*', 'id']],
        },
    ],
});

const store = acquire_shared_state(root(wide_binding), 0);
const count = store.field('count');
assert(count.peek() === 9007199254740993n, 'a wide seed decodes to BigInt');

count.set('5');
assert(count.peek() === 5n, 'a string write into a bigint-pinned cell coerces');
count.set(7);
assert(count.peek() === 7n, 'a Number write into a bigint-pinned cell coerces');
count.update((value) => value);
assert(count.peek() === 7n, 'update routes through the pinned setter');
batch(() => count.set('11'));
assert(count.peek() === 11n, 'batch routes through the pinned setter');
count.set('-');
assert(count.peek() === 11n, 'a write with no image in the representation is dropped, not stored');
count.set(1.5);
assert(count.peek() === 11n, 'a fractional Number is dropped rather than truncated');
assert(
    num_add(count.peek(), 1) === 12n,
    'the cell never holds a value that compiled arithmetic cannot use',
);

// An absent `Option<i64>` seeds as `null`, whose `typeof` is "object". The pin
// therefore comes from `wide_paths`, not from the seeded value's type.
const maybe = store.field('maybe');
assert(maybe.peek() === null, 'an absent Option seeds as null');
maybe.set(5);
assert(maybe.peek() === 5n, 'a null-seeded Option field is still pinned to bigint');

// A Number-backed field is pinned too, so a DOM string cannot lodge in it.
const label = store.field('label');
label.set('renamed');
assert(label.peek() === 'renamed', 'a non-numeric field is left unpinned and stores strings');

// A whole-object write into a field that contains nested wide integers must
// re-decode them. Without this the nested values silently revert to Numbers and
// lose exactness past 2^53, which is the same corruption the seed decode exists
// to prevent, just deferred to the first write.
const rows = store.field('rows');
assert(rows.peek()[0].id === 9007199254740993n, 'nested wide seeds decode');
rows.set([{ id: '9007199254740995' }, { id: 4n }]);
assert(rows.peek()[0].id === 9007199254740995n, 'a whole-array write re-decodes nested wide ints');
assert(rows.peek()[1].id === 4n, 'an already-BigInt nested value passes through');

// ---------------------------------------------------------------------------
// 3. bind_input no longer poisons a page-scoped wide cell
// ---------------------------------------------------------------------------

const node = input_node('');
const dispose_input = bind_input(node, 'value', count);
assert(node.value === '11', 'bind_input seeds the DOM from the cell');

node.type_text('5');
assert(count.peek() === 5n, 'a user edit coerces to the pinned representation');
assert(typeof count.peek() === 'bigint', 'the cell never holds the raw DOM string');

// Mid-typing states that have no image are dropped and leave the last good
// value in place, so another island reading this page-scoped field never sees
// a partial parse.
node.type_text('-');
assert(count.peek() === 5n, 'a partial edit leaves the cell at its last good value');
node.type_text('-7');
assert(count.peek() === -7n, 'completing the edit stores the negative value');

await tick();
await tick();

// "007" denotes 7n, so the cell agrees with the DOM and the effect must not
// rewrite the field under the user's caret.
node.type_text('007');
assert(count.peek() === 7n, 'leading zeros parse');
await tick();
await tick();
assert(node.value === '007', 'the mirror-back effect does not reformat text that already agrees');

dispose_input();
dispose_page_shared_state();

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.dyn-numeric.test.mjs: all assertions passed');
