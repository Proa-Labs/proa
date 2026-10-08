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
