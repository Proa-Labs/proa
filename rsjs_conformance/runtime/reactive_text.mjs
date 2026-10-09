// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, assetIdFromSnapshot, finish, importSnapshot, installDocument, makeElement } from './harness.mjs';

const snapshot = 'rsjs_conformance/snapshots/expressions/reactive_text.island.js';
const id = await assetIdFromSnapshot(snapshot);
const root = makeElement('rsjs-island', { id });
const section = makeElement('section');
const button = makeElement('button', { 'data-rsjs-handler': '1' });
const subtotal = makeElement('p', { 'data-rsjs-bind': '3' });
const mixed = makeElement('p', { 'data-rsjs-attr': '4' });
const slot = makeElement('span', { 'data-rsjs-bind': '6' });
const conditional = makeElement('p', { 'data-rsjs-bind': '8' });
const constants = makeElement('p', { 'data-rsjs-bind': '9' });
const ratio = makeElement('p', { 'data-rsjs-bind': '10' });
const transformed = makeElement('p', { 'data-rsjs-bind': '11', 'data-rsjs-attr': '11' });
const sampled = makeElement('p', { 'data-rsjs-bind': '12' });
const boolText = makeElement('p', { 'data-rsjs-bind': '13', 'data-rsjs-attr': '13' });
const boolAlias = makeElement('p', { 'data-rsjs-bind': '14', 'data-rsjs-attr': '14' });
mixed.textContent = 'Before / After'; // Static sibling content must stay owned by this parent.
mixed.appendChild(slot);
for (const child of [button, subtotal, mixed, conditional, constants, ratio, transformed, sampled, boolText, boolAlias]) section.appendChild(child);
root.appendChild(section);
const host = makeElement('body');
host.appendChild(root);
const seed = makeElement('script', { type: 'application/json', 'data-rsjs-island': id });
seed.textContent = '{"signals":[1,"<tag & value>",true,"fallback",0,1,0,"1",true]}';
host.appendChild(seed);
installDocument(root);
await importSnapshot(snapshot, 'reactive-text.mjs');

assert(subtotal.textContent === 'Subtotal: $1.05', `initial cents formatting: ${subtotal.textContent}`);
assert(slot.textContent === '{<tag & value>} 1 <tag & value>', `raw DOM text: ${slot.textContent}`);
assert(mixed.getAttribute('title') === 'Quantity 1', 'formatted attributes hydrate');
assert(conditional.textContent === '<tag & value> <tag & value>', 'conditional signal reads can repeat without moving their SSR values');
assert(constants.textContent === '1 05 ffffffff 0xFFFFFFFFFFFFFFFF', `literal Rust widths survive formatting: ${constants.textContent}`);
assert(transformed.getAttribute('title') === '        <tag & v', 'conditional formatted strings retain precision and width');
assert(sampled.textContent === '1 1', 'opaque String seeds are safe in conditional arguments');
assert(boolText.getAttribute('title') === 'Active: ', 'native bool attribute matches SSR');
assert(boolAlias.getAttribute('title') === 'Active: ' && boolAlias.getAttribute('data-state') === 'Active: ', 'unknown bool alias attributes match SSR');
button.dispatchEvent({ type: 'click', target: button });
await Promise.resolve();
assert(boolText.textContent === 'false', 'body text displays false');
assert(boolAlias.textContent === 'Active: false', 'body alias displays false');
assert(boolText.getAttribute('title') === 'Active: ', 'attribute suppresses false');
assert(boolAlias.getAttribute('title') === 'Active: ', 'unknown alias suppresses false');
assert(subtotal.textContent === 'Subtotal: $2.10', `updated cents formatting: ${subtotal.textContent}`);
assert(slot.textContent === '{<new & value>} 2 <new & value>', 'both signal dependencies update');
assert(mixed.getAttribute('title') === 'Quantity 2', 'formatted attributes stay reactive');
assert(mixed.textContent === 'Before / After', 'updates leave static sibling content alone');
assert(conditional.textContent === 'fallback <new & value>', 'conditional reads update to the other signal');
assert(transformed.textContent === 'FALLBACK <new & value>', 'owned string transform branches stay reactive');
assert(transformed.getAttribute('title') === '        FALLBACK', 'owned string transforms keep attribute formatting');
assert(ratio.textContent === 'Ratio inf', `positive infinity keeps Rust spelling: ${ratio.textContent}`);
assert(sampled.textContent === '<new & value> 1', 'opaque String seeds can select another live signal');
button.dispatchEvent({ type: 'click', target: button });
await Promise.resolve();
assert(boolText.textContent === 'true' && boolAlias.textContent === 'Active: true', 'body text displays true after update');
assert(boolText.getAttribute('title') === 'Active: ' && boolAlias.getAttribute('title') === 'Active: ', 'attributes suppress true after update');
assert(ratio.textContent === 'Ratio -inf', `negative infinity keeps Rust spelling: ${ratio.textContent}`);
assert(transformed.textContent === '<new & value> <new & value>', 'owned branches can switch back to borrowed signal text');
assert(sampled.textContent === '1 1', 'opaque conditional branches preserve sampled signal state');
for (let i = 0; i < 7; i++) button.dispatchEvent({ type: 'click', target: button });
await Promise.resolve();
assert(subtotal.textContent === 'Subtotal: $10.50', `batched formatting across digit widths: ${subtotal.textContent}`);
assert(constants.textContent === '10 05 ffffffff 0xFFFFFFFFFFFFFFFF', 'typed constant formatting survives updates');
finish('rsjs conformance runtime reactive text');
