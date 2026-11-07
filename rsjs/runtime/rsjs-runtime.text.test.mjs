// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { format_text, format_text_attr, signal, bind_text } from './rsjs-runtime.js';

const integer = (domain, extra = {}) => ({ arg: 0, domain, fill: ' ', align: 0,
    plus: false, alternate: false, zero: false, width: 0, precision: null, kind: 0, ...extra });
const render = (part, value) => format_text('', JSON.stringify([part]), value);

// Rust fmt parity: padding includes signs/prefixes, signed radix output uses
// the source width, and string precision/width count Unicode scalars.
for (const [part, value, expected] of [
    [integer('u32', { zero: true, width: 2 }), 5, '05'],
    [integer('i32', { zero: true, width: 6, plus: true }), -5, '-00005'],
    [integer('u32', { zero: true, width: 6, plus: true }), 5, '+00005'],
    [integer('u32', { kind: 3, alternate: true, zero: true, width: 8 }), 42, '0x00002a'],
    [integer('i32', { kind: 3 }), -1, 'ffffffff'],
    [integer('i64', { kind: 4, alternate: true }), -1n, '0xFFFFFFFFFFFFFFFF'],
    [integer('u64'), 18446744073709551615n, '18446744073709551615'],
    [integer('i64'), -9223372036854775808n, '-9223372036854775808'],
    [integer('u32', { kind: 1, alternate: true }), 5, '0b101'],
    [integer('u32', { kind: 2, alternate: true }), 9, '0o11'],
    [integer(null, { width: 6, align: 3, fill: '·', precision: 2 }), 'é🙂x', '··é🙂··'],
    [integer(null, { width: 5, align: 3 }), 'ab', ' ab  '],
]) assert.equal(render(part, value), expected);

for (const [value, expected] of [[Infinity, 'inf'], [-Infinity, '-inf'], [NaN, 'NaN'], [1.25, '1.25']]) {
    assert.equal(render({ arg: 0, float: true }, value), expected);
}
// Bare `{}` floats keep proa's server spelling (JS shortest text), unchanged
// by formatted-float support.
for (const [value, expected] of [[5, '5'], [-0, '0'], [1e-7, '1e-7'], [1e21, '1e+21']]) {
    assert.equal(render({ arg: 0, float: true }, value), expected);
}

// Formatted floats (`{:.1}`, `{:+}`, `{:08.2}`, `{:^9.1}`): expected strings
// were produced by rustc's core::fmt. Precision rounds the exact binary value
// half-to-even, unlike Number#toFixed (2.5 -> "2", 0.125 -> "0.12"); shortest
// output is plain positional with ties rounded up; f32 parts narrow the value
// with Math.fround first, since f32 seeds arrive as shortest decimal text.
const float = (extra = {}) => ({ arg: 0, string: false, domain: null, float: true, f32: false,
    fill: ' ', align: 0, plus: false, alternate: false, zero: false, width: 0, precision: null,
    kind: 0, ...extra });
for (const [part, value, expected] of [
    [float({ precision: 1 }), 1258291 / 1048576, '1.2'],
    [float({ precision: 1 }), 5, '5.0'],
    [float({ precision: 0 }), 2.5, '2'],
    [float({ precision: 0 }), 3.5, '4'],
    [float({ precision: 0 }), -2.5, '-2'],
    [float({ precision: 2 }), 0.125, '0.12'],
    [float({ precision: 2 }), 0.375, '0.38'],
    [float({ precision: 1 }), 1.25, '1.2'],
    [float({ precision: 1 }), 1.35, '1.4'],
    [float({ precision: 2 }), 9.995, '9.99'],
    [float({ precision: 1 }), 999.95, '1000.0'],
    [float({ precision: 1 }), -0.04, '-0.0'],
    [float({ precision: 1 }), -0, '-0.0'],
    [float({ precision: 3 }), 1e-7, '0.000'],
    [float({ precision: 1, plus: true }), 1.25, '+1.2'],
    [float({ precision: 1, plus: true }), NaN, 'NaN'],
    [float({ precision: 2, zero: true, width: 8 }), -1.5, '-0001.50'],
    [float({ precision: 2, zero: true, width: 8 }), NaN, '00000NaN'],
    [float({ precision: 2, zero: true, width: 8 }), -Infinity, '-0000inf'],
    [float({ precision: 1, width: 8 }), -0.04, '    -0.0'],
    [float({ precision: 1, width: 8, align: 1 }), 1.25, '1.2     '],
    [float({ precision: 1, width: 9, align: 3, fill: '*' }), -2.5, '**-2.5***'],
    [float({ width: 6 }), 1.5, '   1.5'],
    [float({ f32: true }), Math.fround(0.1), '0.1'],
    [float({ f32: true, precision: 5 }), Math.fround(0.1), '0.10000'],
    // f32 seeds and f32 arithmetic arrive as f64 numbers.
    [float({ f32: true, precision: 1 }), JSON.parse('0.15'), '0.2'],
    [float({ f32: true, precision: 2 }), JSON.parse('0.615'), '0.62'],
    [float({ f32: true, precision: 9 }), 0.1 + 0.2, '0.300000012'],
    [float({ f32: true, width: 12 }), 0.1 + 0.2, '         0.3'],
    // Shortest: no exponent, integral values without a fraction, signed -0,
    // exact ties rounded up (2^-25), f32 power-of-two boundaries (2^87).
    [float({ width: 1 }), 5, '5'],
    [float({ width: 1 }), -0, '-0'],
    [float({ width: 1 }), 1e21, '1000000000000000000000'],
    [float({ width: 1 }), 1e-7, '0.0000001'],
    [float({ width: 1 }), 2 ** -25, '0.000000029802322387695313'],
    [float({ f32: true, width: 1 }), 2 ** 87, '154742510000000000000000000'],
    // The only f32 whose shortest text double-rounds through f64 (all 2^31
    // positive f32 checked): the seed must narrow to 0x15ae43fd, not its
    // neighbour, and the exact interval test keeps both shortest forms apart.
    [float({ f32: true, precision: 40 }), JSON.parse('0.00000000000000000000000007038531'),
        '0.0000000000000000000000000703853069185121'],
    [float({ f32: true, plus: true }), JSON.parse('0.00000000000000000000000007038531'),
        '+0.00000000000000000000000007038531'],
    [float({ f32: true, plus: true }), new Float32Array(new Uint32Array([0x15ae43fe]).buffer)[0],
        '+0.000000000000000000000000070385313'],
    // Non-finite seeds arrive as JSON null: no digits, padding kept.
    [float({ precision: 1, width: 4 }), null, '    '],
]) assert.equal(render(part, value), expected);

assert.equal(render({ arg: 0, float: false }, Infinity), 'inf');
assert.equal(render({ arg: 0, float: false }, -Infinity), '-inf');
assert.equal(render({ arg: 0 }, 'Infinity'), 'Infinity');

// Native bool attributes write no bytes; body text and JSON-derived bools
// retain their Display / JSON text spelling.
const nativeBool = JSON.stringify([{ text: 'Active: ' }, { arg: 0, attr_bool: true }]);
const jsonBool = JSON.stringify([{ text: 'Active: ' }, { arg: 0, attr_bool: false }]);
for (const value of [true, false]) {
    assert.equal(format_text_attr('', nativeBool, value), 'Active: ');
    assert.equal(format_text('', nativeBool, value), `Active: ${value}`);
    assert.equal(format_text_attr('', jsonBool, value), `Active: ${value}`);
}
assert.equal(format_text_attr('', nativeBool, '<bool & text>'), 'Active: <bool & text>');

const count = signal(105);
const label = signal('<tag & value>');
const parts = [{ text: 'Subtotal: $' }, { arg: 0 }, { text: '.' },
    integer('u32', { arg: 1, width: 2, zero: true }), { text: ' ' }, { arg: 2 }];
const node = { textContent: '' };
bind_text(node, () => format_text('', JSON.stringify(parts),
    Math.trunc(count.get() / 100), count.get() % 100, label.get()), [count, label]);
assert.equal(node.textContent, 'Subtotal: $1.05 <tag & value>');
count.set(1050);
label.set('<new & value>');
await Promise.resolve();
assert.equal(node.textContent, 'Subtotal: $10.50 <new & value>');
console.log('rsjs-runtime.text.test.mjs: all assertions passed');
