// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for one-way attribute bindings. Run with:
//
//     node rsjs/runtime/rsjs-runtime.attr.test.mjs

import { bind_attr, signal } from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

function tick() {
    return Promise.resolve();
}

function make_element() {
    const attrs = new Map();
    return {
        disabled: false,
        value: '',
        setAttribute(name, value) {
            attrs.set(name, String(value));
        },
        removeAttribute(name) {
            attrs.delete(name);
        },
        getAttribute(name) {
            return attrs.has(name) ? attrs.get(name) : null;
        },
        hasAttribute(name) {
            return attrs.has(name);
        },
    };
}

{
    const el = make_element();
    const open = signal(false);
    bind_attr(el, 'aria-expanded', 'value', () => open.get(), [open]);

    assert(el.getAttribute('aria-expanded') === 'false', 'value attr stringifies false');
    open.set(true);
    await tick();
    assert(el.getAttribute('aria-expanded') === 'true', 'value attr updates to true');
}

{
    const el = make_element();
    const disabled = signal(false);
    bind_attr(el, 'disabled', 'boolean', () => disabled.get(), [disabled]);

    assert(!el.hasAttribute('disabled'), 'boolean attr starts absent');
    assert(el.disabled === false, 'boolean attr mirrors property false');
    disabled.set(true);
    await tick();
    assert(el.hasAttribute('disabled'), 'boolean attr becomes present');
    assert(el.disabled === true, 'boolean attr mirrors property true');
    disabled.set(false);
    await tick();
    assert(!el.hasAttribute('disabled'), 'boolean attr is removed after false');
    assert(el.disabled === false, 'boolean property returns false');
}

{
    const el = make_element();
    const value = signal('initial');
    bind_attr(el, 'value', 'value', () => value.get(), [value]);

    assert(el.getAttribute('value') === 'initial', 'value attr starts with signal value');
    assert(el.value === 'initial', 'value property starts with signal value');
    el.value = 'typed';
    value.set('reset');
    await tick();
    assert(el.getAttribute('value') === 'reset', 'value attr updates after signal reset');
    assert(el.value === 'reset', 'value property updates after user edits');
}

{
    const el = make_element();
    const url = signal('/docs');
    bind_attr(el, 'href', 'url', () => url.get(), [url]);

    assert(el.getAttribute('href') === '/docs', 'url attr starts with safe relative URL');

    for (const dangerous of [
        'javascript:alert(1)',
        'VBScript:MsgBox',
        'java\tscript:alert(1)',
        '&#106;avascript:alert(1)',
        '&#0000000106;avascript:alert(1)',
        '&#x6a;avascript:alert(1)',
        'data:text/html,<script>alert(1)</script>',
        'data:image/svg+xml,<svg/>',
    ]) {
        url.set(dangerous);
        await tick();
        assert(el.getAttribute('href') === '', `url attr neutralizes ${dangerous}`);
    }

    url.set('data:image/png;base64,iVBORw0KGgo=');
    await tick();
    assert(el.getAttribute('href') === 'data:image/png;base64,iVBORw0KGgo=', 'url attr allows image data URL');
}

if (failed > 0) {
    process.exit(1);
}

console.log('All attribute binding tests passed.');
