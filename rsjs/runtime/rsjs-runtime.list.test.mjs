// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for bind_list keyed reconciliation. Run with:
//
//     node rsjs/runtime/rsjs-runtime.list.test.mjs
//
// Uses a minimal fake container implementing the DOM subset bind_list
// touches: firstChild, nextSibling, parentNode, insertBefore (with
// move semantics), removeChild, and getAttribute for adoption.

import { signal, bind_list } from './rsjs-runtime.js';

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

function capture_uncaught(run) {
    return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
            process.removeListener('uncaughtException', on_error);
            reject(new Error('expected an uncaught exception'));
        }, 1_000);
        const on_error = (error) => {
            clearTimeout(timeout);
            resolve(error);
        };
        process.once('uncaughtException', on_error);
        try {
            run();
        } catch (error) {
            process.removeListener('uncaughtException', on_error);
            clearTimeout(timeout);
            resolve(error);
        }
    });
}

function make_node(label, key) {
    const attrs = new Map();
    if (key != null) {
        attrs.set('data-rsjs-key', key);
    }
    return {
        label,
        parentNode: null,
        nextSibling: null,
        getAttribute(name) {
            return attrs.has(name) ? attrs.get(name) : null;
        },
    };
}

function make_container(...initial_children) {
    const container = {
        firstChild: null,
        insertBefore(node, ref) {
            if (node.parentNode === container) {
                container.removeChild(node);
            }
            node.parentNode = container;
            if (ref == null) {
                let cursor = container.firstChild;
                if (cursor == null) {
                    container.firstChild = node;
                } else {
                    while (cursor.nextSibling != null) {
                        cursor = cursor.nextSibling;
                    }
                    cursor.nextSibling = node;
                }
                node.nextSibling = null;
                return node;
            }
            if (container.firstChild === ref) {
                node.nextSibling = ref;
                container.firstChild = node;
                return node;
            }
            let cursor = container.firstChild;
            while (cursor != null && cursor.nextSibling !== ref) {
                cursor = cursor.nextSibling;
            }
            if (cursor == null) {
                throw new Error('insertBefore: ref is not a child');
            }
            node.nextSibling = ref;
            cursor.nextSibling = node;
            return node;
        },
        removeChild(node) {
            if (node.parentNode !== container) {
                throw new Error('removeChild: node is not a child');
            }
            if (container.firstChild === node) {
                container.firstChild = node.nextSibling;
            } else {
                let cursor = container.firstChild;
                while (cursor != null && cursor.nextSibling !== node) {
                    cursor = cursor.nextSibling;
                }
                if (cursor == null) {
                    throw new Error('removeChild: node not found');
                }
                cursor.nextSibling = node.nextSibling;
            }
            node.parentNode = null;
            node.nextSibling = null;
            return node;
        },
    };
    for (const child of initial_children) {
        container.insertBefore(child, null);
    }
    return container;
}

function labels(container) {
    const out = [];
    let cursor = container.firstChild;
    while (cursor != null) {
        out.push(cursor.label);
        cursor = cursor.nextSibling;
    }
    return out.join(',');
}

// --- create, insert, remove, reorder ----------------------------------------

{
    const container = make_container();
    const items = signal([
        { id: 'a', text: 'alpha' },
        { id: 'b', text: 'beta' },
    ]);
    let created = 0;
    let disposed = [];
    bind_list(container, () => items.get(), {
        key: (item) => item.id,
        create(item) {
            created += 1;
            const node = make_node(item.text);
            return {
                node,
                update(next) {
                    node.label = next.text;
                },
                dispose() {
                    disposed.push(item.id);
                },
            };
        },
        deps: [items],
    });

    assert(labels(container) === 'alpha,beta', 'initial items render in order');
    assert(created === 2, 'one create per initial item');

    // Append and prepend in one write.
    items.set([
        { id: 'c', text: 'gamma' },
        { id: 'a', text: 'alpha' },
        { id: 'b', text: 'beta' },
        { id: 'd', text: 'delta' },
    ]);
    await tick();
    assert(labels(container) === 'gamma,alpha,beta,delta', 'insert at both ends');
    assert(created === 4, 'surviving keys are not re-created');

    // Reorder only: no creates, no disposes.
    items.set([
        { id: 'd', text: 'delta' },
        { id: 'b', text: 'beta' },
        { id: 'a', text: 'alpha' },
        { id: 'c', text: 'gamma' },
    ]);
    await tick();
    assert(labels(container) === 'delta,beta,alpha,gamma', 'keyed reorder moves nodes');
    assert(created === 4, 'reorder creates nothing');
    assert(disposed.length === 0, 'reorder disposes nothing');

    // Remove two.
    items.set([
        { id: 'b', text: 'beta' },
        { id: 'c', text: 'gamma' },
    ]);
    await tick();
    assert(labels(container) === 'beta,gamma', 'removed items leave the DOM');
    assert(disposed.sort().join(',') === 'a,d', 'removed items run their disposers');

    // In-place update via identity change.
    items.set([
        { id: 'b', text: 'BETA' },
        { id: 'c', text: 'gamma' },
    ]);
    await tick();
    assert(labels(container) === 'BETA,gamma', 'identity change calls update in place');
    assert(created === 4, 'update does not re-create');
}

// --- SSR adoption ------------------------------------------------------------

{
    const ssr_a = make_node('ssr-alpha', 'a');
    const ssr_b = make_node('ssr-beta', 'b');
    const ssr_stale = make_node('ssr-stale', 'zombie');
    const container = make_container(ssr_a, ssr_b, ssr_stale);

    const items = signal([
        { id: 'a', text: 'alpha' },
        { id: 'b', text: 'beta' },
        { id: 'c', text: 'gamma' },
    ]);
    let adopted = 0;
    let created = 0;
    bind_list(container, () => items.get(), {
        key: (item) => item.id,
        create(item) {
            created += 1;
            return make_node(item.text);
        },
        adopt(node) {
            adopted += 1;
            return node;
        },
        deps: [items],
    });

    assert(adopted === 2, 'server-rendered children with matching keys are adopted');
    assert(created === 1, 'only the unseeded item is created');
    assert(labels(container) === 'ssr-alpha,ssr-beta,gamma', 'adopted nodes keep their DOM, stale markup is dropped');
}

// --- duplicate keys ----------------------------------------------------------

{
    const container = make_container();
    const items = signal([
        { id: 'x', text: 'one' },
        { id: 'x', text: 'two' },
    ]);
    let error = null;
    try {
        bind_list(container, () => items.get(), {
            key: (item) => item.id,
            create: (item) => make_node(item.text),
        });
    } catch (caught) {
        error = caught;
    }
    assert(
        error?.code === 'rsjs/duplicate-list-key' &&
            error?.source === 'live item sequence' &&
            error?.key === 'x',
        'an initial duplicate live key throws a structured list-key error',
    );
    assert(labels(container) === '', 'an initial duplicate live sequence mutates no DOM');

    items.set([{ id: 'valid', text: 'must stay inert' }]);
    await tick();
    assert(
        labels(container) === '',
        'a failed initial binding unsubscribes before it can receive later writes',
    );
}

// A later invalid proposal validates every key before updating an existing
// record, creating a new one, removing an old one, or moving DOM.
{
    const container = make_container();
    const items = signal([
        { id: 'a', text: 'alpha' },
        { id: 'b', text: 'beta' },
    ]);
    let created = 0;
    let updated = 0;
    let disposed = 0;
    const dispose = bind_list(container, () => items.get(), {
        key: (item) => item.id,
        create(item) {
            created += 1;
            const node = make_node(item.text);
            return {
                node,
                update(next) {
                    updated += 1;
                    node.label = next.text;
                },
                dispose() {
                    disposed += 1;
                },
            };
        },
        deps: [items],
    });

    const error = await capture_uncaught(() => {
        items.set([
            { id: 'a', text: 'ALPHA' },
            { id: 'a', text: 'duplicate' },
            { id: 'c', text: 'gamma' },
        ]);
    });
    assert(
        error?.code === 'rsjs/duplicate-list-key' &&
            error?.source === 'live item sequence' &&
            error?.key === 'a',
        'a later duplicate live key fails the scheduled reconciliation',
    );
    assert(
        labels(container) === 'alpha,beta',
        'a later duplicate proposal preserves the previously committed DOM',
    );
    assert(
        created === 2 && updated === 0 && disposed === 0,
        'a later duplicate proposal applies no create, update, or dispose prefix',
    );

    dispose();
    dispose();
    assert(
        disposed === 2,
        'list disposal remains once-only after a failed later reconciliation',
    );
}

// Duplicate SSR adoption markers are protocol-invalid even when the live item
// sequence itself is unique. Validate the complete marker set before adopt,
// create, stale-node removal, or placement.
{
    const ssr_first = make_node('ssr-first', 'same');
    const ssr_second = make_node('ssr-second', 'same');
    const container = make_container(ssr_first, ssr_second);
    let adopted = 0;
    let created = 0;
    let error = null;
    try {
        bind_list(container, () => [{ id: 'same', text: 'live' }], {
            key: (item) => item.id,
            create(item) {
                created += 1;
                return make_node(item.text);
            },
            adopt(node) {
                adopted += 1;
                return node;
            },
            deps: [],
        });
    } catch (caught) {
        error = caught;
    }
    assert(
        error?.code === 'rsjs/duplicate-list-key' &&
            error?.source === 'SSR marker set' &&
            error?.key === 'same',
        'duplicate SSR adoption markers throw a structured list-key error',
    );
    assert(
        labels(container) === 'ssr-first,ssr-second' &&
            adopted === 0 &&
            created === 0,
        'duplicate SSR markers leave the complete server DOM unclaimed and untouched',
    );
}

// --- disposal stops updates and runs item disposers ---------------------------

{
    const container = make_container();
    const items = signal([{ id: 'a', text: 'alpha' }]);
    const disposed = [];
    const dispose = bind_list(container, () => items.get(), {
        key: (item) => item.id,
        create(item) {
            const node = make_node(item.text);
            return { node, dispose: () => disposed.push(item.id) };
        },
        deps: [items],
    });
    dispose();
    assert(disposed.join(',') === 'a', 'binding disposal runs item disposers');
    items.set([{ id: 'a', text: 'alpha' }, { id: 'b', text: 'beta' }]);
    await tick();
    assert(labels(container) === 'alpha', 'disposed binding stops reconciling');
}

// --- query/mutation handle notifications rerun the list getter ----------------

{
    const container = make_container();
    let query_items = [{ id: 'a', text: 'alpha' }];
    const listeners = new Set();
    const handle = {
        subscribe(listener) {
            listeners.add(listener);
            return () => listeners.delete(listener);
        },
    };
    const dispose = bind_list(container, () => query_items, {
        key: (item) => item.id,
        create: (item) => make_node(item.text),
        extra_handles: [handle],
    });

    assert(labels(container) === 'alpha', 'extra handle list renders initially');
    query_items = [
        { id: 'a', text: 'alpha' },
        { id: 'b', text: 'beta' },
    ];
    for (const listener of listeners) {
        listener();
    }
    assert(labels(container) === 'alpha,beta', 'extra handle notification rerenders list');

    dispose();
    query_items = [{ id: 'c', text: 'gamma' }];
    for (const listener of listeners) {
        listener();
    }
    assert(labels(container) === 'alpha,beta', 'disposed extra handle list stops reconciling');
}

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.list.test.mjs: all assertions passed');
