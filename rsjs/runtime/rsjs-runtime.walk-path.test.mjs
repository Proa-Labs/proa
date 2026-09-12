// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Owner-scoped hydration locator tests. Run with:
//
//     node rsjs/runtime/rsjs-runtime.walk-path.test.mjs

import { walk_path } from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function element(tagName, attrs = {}) {
    return {
        tagName: tagName.toUpperCase(),
        attrs: { ...attrs },
        children: [],
        parentElement: null,
        getAttribute(name) {
            return Object.prototype.hasOwnProperty.call(this.attrs, name)
                ? this.attrs[name]
                : null;
        },
        hasAttribute(name) {
            return Object.prototype.hasOwnProperty.call(this.attrs, name);
        },
        appendChild(child) {
            child.parentElement = this;
            this.children.push(child);
            return child;
        },
        querySelectorAll(selector) {
            const match = /^\[([a-zA-Z0-9-]+)\]$/.exec(selector);
            if (match == null) {
                throw new Error(`unsupported test selector: ${selector}`);
            }
            const attr = match[1];
            const found = [];
            const visit = (node) => {
                for (const child of node.children) {
                    if (child.hasAttribute(attr)) found.push(child);
                    visit(child);
                }
            };
            visit(this);
            return found;
        },
    };
}

// An opaque server fragment emitted two element children where codegen could
// only conservatively count one. The static path lands on the wrong element,
// so production must find the verified marker instead.
{
    delete globalThis.__RSJS_DEV__;
    const root = element('rsjs-island');
    const main = root.appendChild(element('main'));
    const nestedOwner = main.appendChild(element('rsjs-island'));
    const nestedDuplicate = nestedOwner.appendChild(
        element('span', { 'data-rsjs-bind': '7' }),
    );
    main.appendChild(element('div'));
    const target = main.appendChild(
        element('button', { 'data-rsjs-bind': '7' }),
    );

    const resolved = walk_path(root, [0, 1], 'data-rsjs-bind', '7');
    assert(
        resolved === target,
        'production fallback should resolve the shifted marker in the current owner',
    );
    assert(
        resolved !== nestedDuplicate,
        'fallback must ignore a same-numbered marker in a nested island owner',
    );
}

// Component fragments are ownership boundaries too.
{
    const root = element('div', { 'data-rsjs-component': 'parent' });
    const nestedOwner = root.appendChild(
        element('section', { 'data-rsjs-component': 'child' }),
    );
    nestedOwner.appendChild(element('span', { 'data-rsjs-attr': '3' }));
    root.appendChild(element('i'));
    const target = root.appendChild(
        element('output', { 'data-rsjs-attr': '3' }),
    );

    assert(
        walk_path(root, [1], 'data-rsjs-attr', '3') === target,
        'fragment fallback should stay inside the current component owner',
    );
}

if (failed > 0) {
    process.exitCode = 1;
} else {
    console.log('walk_path owner-scoped fallback: ok');
}
