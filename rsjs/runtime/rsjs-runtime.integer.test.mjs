// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Fixed-width integer runtime parity. Run with:
//
//     node rsjs/runtime/rsjs-runtime.integer.test.mjs

import {
    checked_int_add,
    checked_int_div,
    checked_int_mul,
    checked_int_neg,
    checked_int_rem,
    checked_int_sub,
    integer_signal,
    signal_initial_integer,
    signal_initial_typed,
} from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function throws(run, message) {
    try {
        run();
        assert(false, `${message}: expected an exception`);
    } catch (_) {
        // Expected.
    }
}

{
    const value = integer_signal(2147483647, 'i32');
    throws(
        () => value.update((current) => checked_int_add(current, 1, 'i32')),
        'i32 checked add overflow',
    );
    assert(value.get() === 2147483647, 'failed checked update must be atomic');
    throws(() => value.set(2147483648), 'i32 out-of-range set');
    assert(value.get() === 2147483647, 'failed set must leave state unchanged');
}

{
    const optionI64 = { kind: 'option', inner: { kind: 'i64' } };
    assert(
        signal_initial_typed(['9007199254740993'], 0, null, optionI64)
            === 9007199254740993n,
        'Option<i64>::Some hydration decodes exactly past 2^53',
    );
    assert(
        signal_initial_typed([null], 0, 7n, optionI64) === null,
        'Option<i64>::None hydration remains null',
    );
    const nested = {
        kind: 'vec',
        inner: { kind: 'option', inner: { kind: 'u64' } },
    };
    const decoded = signal_initial_typed(
        [['18446744073709551615', null]],
        0,
        [],
        nested,
    );
    assert(
        decoded[0] === 18446744073709551615n && decoded[1] === null,
        'Vec<Option<u64>> hydration recursively decodes wide leaves',
    );
    throws(
        () => signal_initial_typed(['09007199254740993'], 0, null, optionI64),
        'nested wide hydration rejects non-canonical decimal strings',
    );
}

{
    const value = integer_signal(-2147483648, 'i32');
    for (const [name, operation] of [
        ['division by zero', (current) => checked_int_div(current, 0, 'i32')],
        ['remainder by zero', (current) => checked_int_rem(current, 0, 'i32')],
        ['MIN / -1', (current) => checked_int_div(current, -1, 'i32')],
        ['MIN % -1', (current) => checked_int_rem(current, -1, 'i32')],
        ['MIN negation', (current) => checked_int_neg(current, 'i32')],
    ]) {
        throws(() => value.update(operation), name);
        assert(value.get() === -2147483648, `${name} must not mutate state`);
    }
}

{
    const value = integer_signal(0, 'u32');
    assert(checked_int_sub(4, 3, 'u32') === 1, 'u32 subtraction should stay Number');
    assert(checked_int_mul(65535, 65535, 'u32') === 4294836225, 'u32 multiply');
    throws(() => checked_int_sub(0, 1, 'u32'), 'u32 underflow');
    throws(() => checked_int_neg(0, 'u32'), 'unsigned negation');
    value.set(0);
    assert(!Object.is(value.get(), -0), 'integer signals canonicalize negative zero');
    assert(
        !Object.is(checked_int_neg(0, 'i32'), -0),
        'checked narrow negation canonicalizes negative zero',
    );
}

{
    const value = integer_signal(1n, 'i64');
    value.set(2n);
    value.update(() => 3n);
    assert(typeof value.get() === 'bigint' && value.get() === 3n, 'i64 writes retain BigInt');
    assert(
        checked_int_add(9007199254740993n, 1n, 'i64') === 9007199254740994n,
        'in-range i64 arithmetic stays exact past 2^53',
    );
    throws(() => value.set(4), 'i64 Number write');
    assert(value.get() === 3n, 'cross-representation write must be atomic');
}

{
    // Shared fields use the same emitted checked expression before `.set`.
    // Model the shared cell's mutation contract directly: an overflowing
    // expression must throw before `set` runs, leaving the page-scoped value
    // unchanged.
    let sharedValue = 9223372036854775807n;
    const shared = {
        get: () => sharedValue,
        set: (next) => {
            sharedValue = next;
        },
    };
    throws(
        () => shared.set(checked_int_add(shared.get(), 1n, 'i64')),
        'direct shared i64 set overflow',
    );
    assert(
        shared.get() === 9223372036854775807n,
        'failed direct shared set must remain atomic',
    );
}

{
    // `rsjs::div_int::<i64>(shared.get(), divisor.get())` emits this exact
    // checked expression before the page-scoped cell's `.set`. Both Rust
    // failure modes must throw before mutation.
    let sharedValue = -9223372036854775808n;
    const shared = {
        get: () => sharedValue,
        set: (next) => {
            sharedValue = next;
        },
    };
    throws(
        () => shared.set(checked_int_div(shared.get(), -1n, 'i64')),
        'direct shared i64 MIN / -1 overflow',
    );
    assert(
        shared.get() === -9223372036854775808n,
        'MIN / -1 must fail before shared mutation',
    );
    throws(
        () => shared.set(checked_int_div(shared.get(), 0n, 'i64')),
        'direct shared i64 division by zero',
    );
    assert(
        shared.get() === -9223372036854775808n,
        'division by zero must fail before shared mutation',
    );
}

{
    const max = 18446744073709551615n;
    const value = integer_signal(max, 'u64');
    assert(value.get() === max, 'u64::MAX is accepted exactly');
    throws(
        () => value.update((current) => checked_int_add(current, 1n, 'u64')),
        'u64 overflow',
    );
    assert(value.get() === max, 'u64 overflow must leave state unchanged');
    throws(() => value.set(-1n), 'negative u64 write');
    assert(value.get() === max, 'negative u64 rejection must be atomic');
}

{
    assert(
        signal_initial_integer(['9223372036854775807'], 0, 0n, 'i64')
            === 9223372036854775807n,
        'wide hydration decodes decimal JSON strings',
    );
    assert(
        signal_initial_integer([4294967295], 0, 0, 'u32') === 4294967295,
        'narrow hydration accepts JSON Numbers',
    );
    assert(
        signal_initial_integer([], 0, -9223372036854775808n, 'i64')
            === -9223372036854775808n,
        'wide fallback accepts i64::MIN BigInt',
    );
    throws(
        () => signal_initial_integer([1], 0, 0n, 'i64'),
        'wide hydration rejects JSON Number',
    );
    throws(
        () => signal_initial_integer(['1'], 0, 0, 'i32'),
        'narrow hydration rejects JSON string',
    );
}

{
    throws(() => checked_int_add(1n, 1n, 'i32'), 'narrow op rejects BigInt');
    throws(() => checked_int_add(1, 1, 'i64'), 'wide op rejects Number');
}

if (failed > 0) {
    process.exitCode = 1;
} else {
    console.log('rsjs fixed-width integer runtime tests passed');
}
