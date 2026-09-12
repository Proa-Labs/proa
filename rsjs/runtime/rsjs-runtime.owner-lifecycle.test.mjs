// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Owner/structural-region lifecycle regressions. Run with:
//
//     node rsjs/runtime/rsjs-runtime.owner-lifecycle.test.mjs

import {
    bind_list,
    dispose_all,
    dispose_hydrated,
    hydrate,
    hydrate_when_visible,
    signal,
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

function make_node(tag = 'div', attributes = {}) {
    const node = {
        tagName: tag.toUpperCase(),
        attributes: { ...attributes },
        parentNode: null,
        parentElement: null,
        childNodes: [],
        nextSibling: null,
        isConnected: true,
        get firstChild() {
            return node.childNodes[0] ?? null;
        },
        getAttribute(name) {
            return Object.prototype.hasOwnProperty.call(node.attributes, name)
                ? node.attributes[name]
                : null;
        },
        contains(other) {
            let current = other;
            while (current != null) {
                if (current === node) return true;
                current = current.parentNode ?? current.parentElement ?? null;
            }
            return false;
        },
        appendChild(child) {
            return node.insertBefore(child, null);
        },
        insertBefore(child, reference) {
            if (child.parentNode != null) {
                child.parentNode.removeChild(child);
            }
            const index =
                reference == null ? node.childNodes.length : node.childNodes.indexOf(reference);
            if (index < 0) throw new Error('insertBefore reference is not a child');
            node.childNodes.splice(index, 0, child);
            child.parentNode = node;
            child.parentElement = node;
            child.isConnected = node.isConnected;
            relink_children(node);
            return child;
        },
        removeChild(child) {
            const index = node.childNodes.indexOf(child);
            if (index < 0) throw new Error('removeChild target is not a child');
            node.childNodes.splice(index, 1);
            child.parentNode = null;
            child.parentElement = null;
            child.nextSibling = null;
            mark_connected(child, false);
            relink_children(node);
            return child;
        },
    };
    return node;
}

function relink_children(parent) {
    for (let i = 0; i < parent.childNodes.length; i += 1) {
        parent.childNodes[i].nextSibling = parent.childNodes[i + 1] ?? null;
    }
}

function mark_connected(node, connected) {
    node.isConnected = connected;
    for (const child of node.childNodes ?? []) {
        mark_connected(child, connected);
    }
}

function island_root(name) {
    return make_node('rsjs-island', {
        'data-rsjs-island-root': name,
    });
}

const previous_document = globalThis.document;
const previous_intersection_observer = globalThis.IntersectionObserver;
const roots = new Map();
const body = make_node('body');

globalThis.document = {
    readyState: 'complete',
    body,
    documentElement: body,
    addEventListener() {},
    getElementById() {
        return null;
    },
    querySelectorAll(selector) {
        if (selector === '#rsjs-shared-state-seed-table') return [];
        const current = /^rsjs-island\[data-rsjs-island-root="([^"]+)"\]$/.exec(
            selector,
        );
        if (current != null) {
            return roots.get(current[1]) ?? [];
        }
        return [];
    },
};

dispose_hydrated();

try {
    // A returned custom cleanup is additive to the runtime-owned stack. The
    // last-created callback runs first, stack entries unwind in reverse order,
    // and the consumed array no longer retains owner-scoped references.
    {
        const root = body.appendChild(island_root('resource-owner'));
        roots.set('resource-owner', [root]);
        const order = [];
        let captured_resources = null;
        const unregister = hydrate('resource-owner', (_root, resources) => {
            captured_resources = resources;
            resources.push(
                () => order.push('handler'),
                () => order.push('effect'),
                () => order.push('query-observer'),
                () => order.push('shared-subscription'),
                () => order.push('node-ref'),
            );
            return () => order.push('custom');
        });

        dispose_hydrated('resource-owner', root);
        dispose_hydrated('resource-owner', root);
        assert(
            order.join(',') ===
                'custom,node-ref,shared-subscription,query-observer,effect,handler',
            `owner cleanup is complete, reverse-ordered, and once-only: ${order.join(',')}`,
        );
        assert(
            captured_resources?.length === 0,
            'owner teardown consumes its resource stack and releases retained references',
        );
        unregister();
        roots.delete('resource-owner');
    }

    // Generated owners return a disposer that drains the exact array the
    // runtime passed in. Runtime ownership remains additive without running
    // those same entries twice.
    {
        const root = body.appendChild(island_root('generated-owner'));
        roots.set('generated-owner', [root]);
        let resource_runs = 0;
        const unregister = hydrate('generated-owner', (_root, resources) => {
            resources.push(() => {
                resource_runs += 1;
            });
            return () => dispose_all(resources, 'generated owner');
        });
        dispose_hydrated('generated-owner', root);
        assert(
            resource_runs === 1,
            'generated dispose_all(resources) cleanup and runtime ownership do not double-run',
        );
        unregister();
        roots.delete('generated-owner');
    }

    // A duplicate keyed-list sequence is a synchronous owner-init failure.
    // Resources installed before the list unwind once, the invalid list never
    // commits a prefix, and the failed owner is never registered.
    {
        const root = body.appendChild(island_root('duplicate-list-owner'));
        const container = root.appendChild(make_node('ul'));
        roots.set('duplicate-list-owner', [root]);
        const items = signal([
            { id: 'same', label: 'first' },
            { id: 'same', label: 'second' },
        ]);
        let earlier_cleanup = 0;
        let created = 0;
        const errors = [];
        const original_error = console.error;
        console.error = (...args) => errors.push(args);
        let unregister;
        try {
            unregister = hydrate(
                'duplicate-list-owner',
                (_hydrated_root, resources) => {
                    resources.push(() => {
                        earlier_cleanup += 1;
                    });
                    resources.push(
                        bind_list(container, () => items.get(), {
                            key: (item) => item.id,
                            create(item) {
                                created += 1;
                                return make_node('li', {
                                    'data-label': item.label,
                                });
                            },
                            deps: [items],
                        }),
                    );
                },
            );
        } finally {
            console.error = original_error;
        }
        assert(
            earlier_cleanup === 1,
            'duplicate-list activation unwinds resources installed before the list',
        );
        assert(
            created === 0 && container.childNodes.length === 0,
            'duplicate-list activation validates before creating or inserting an item',
        );
        assert(
            errors.length === 1 &&
                errors[0][0] ===
                    'rsjs: island "duplicate-list-owner" init failed' &&
                errors[0][1]?.code === 'rsjs/duplicate-list-key',
            'duplicate-list activation reports the original scoped protocol error',
        );
        assert(
            dispose_hydrated('duplicate-list-owner', root) === 0,
            'duplicate-list activation never registers a partial owner',
        );

        items.set([{ id: 'valid', label: 'must stay inert' }]);
        await tick();
        assert(
            created === 0 && container.childNodes.length === 0,
            'failed duplicate-list activation leaves no subscribed binding behind',
        );

        unregister?.();
        unregister?.();
        dispose_hydrated('duplicate-list-owner', root);
        assert(
            earlier_cleanup === 1,
            'failed activation cleanup stays idempotent across later unregister/dispose calls',
        );
        roots.delete('duplicate-list-owner');
    }

    // A failing returned cleanup still cannot skip earlier owner resources.
    {
        const root = body.appendChild(island_root('throwing-owner'));
        roots.set('throwing-owner', [root]);
        let resource_disposed = 0;
        const original_error = console.error;
        console.error = () => {};
        let unregister;
        try {
            unregister = hydrate('throwing-owner', (_root, resources) => {
                resources.push(() => {
                    resource_disposed += 1;
                });
                return () => {
                    throw new Error('terminal cleanup failed');
                };
            });
            dispose_hydrated('throwing-owner', root);
        } finally {
            console.error = original_error;
        }
        assert(
            resource_disposed === 1,
            'a throwing returned disposer still drains the runtime-owned resource stack',
        );
        unregister?.();
        roots.delete('throwing-owner');
    }

    // Explicit parent teardown is an ownership boundary: nested hydrated
    // owners unwind before the parent rather than waiting for an asynchronous
    // detached-root MutationObserver pass.
    {
        const parent = body.appendChild(island_root('parent-owner'));
        const child = parent.appendChild(island_root('child-owner'));
        roots.set('parent-owner', [parent]);
        roots.set('child-owner', [child]);
        const order = [];
        const unregister_child = hydrate('child-owner', () => () => {
            order.push('child');
        });
        const unregister_parent = hydrate('parent-owner', () => () => {
            order.push('parent');
        });

        dispose_hydrated('parent-owner', parent);
        assert(
            order.join(',') === 'child,parent',
            `nested owners dispose deepest-first: ${order.join(',')}`,
        );
        assert(
            dispose_hydrated('child-owner', child) === 0,
            'nested owner was removed from runtime tracking by the parent teardown',
        );
        unregister_child();
        unregister_parent();
        roots.delete('child-owner');
        roots.delete('parent-owner');
    }

    // The same cascade cancels deferred nested activation, including the
    // observer reference and any later callback delivery.
    {
        const observers = [];
        globalThis.IntersectionObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
                this.target = null;
                observers.push(this);
            }
            observe(target) {
                this.target = target;
            }
            disconnect() {
                this.disconnected = true;
            }
            fire() {
                this.callback([
                    {
                        target: this.target,
                        isIntersecting: true,
                        intersectionRatio: 1,
                    },
                ]);
            }
        };

        const parent = body.appendChild(island_root('activation-parent'));
        const child = parent.appendChild(island_root('activation-child'));
        roots.set('activation-parent', [parent]);
        roots.set('activation-child', [child]);
        let child_initializations = 0;
        const unregister_child = hydrate_when_visible(
            'activation-child',
            () => {
                child_initializations += 1;
            },
        );
        const unregister_parent = hydrate('activation-parent', () => {});

        dispose_hydrated('activation-parent', parent);
        observers[0].fire();
        assert(
            observers[0].disconnected === true,
            'parent teardown disconnects the nested pending activation observer',
        );
        assert(
            child_initializations === 0,
            'a callback delivered after nested pending activation disposal cannot initialize',
        );
        unregister_child();
        unregister_parent();
        roots.delete('activation-child');
        roots.delete('activation-parent');

        const pending_parent = body.appendChild(
            island_root('pending-activation-parent'),
        );
        const hydrated_child = pending_parent.appendChild(
            island_root('hydrated-child'),
        );
        roots.set('pending-activation-parent', [pending_parent]);
        roots.set('hydrated-child', [hydrated_child]);
        let parent_initializations = 0;
        let hydrated_child_disposals = 0;
        const unregister_hydrated_child = hydrate(
            'hydrated-child',
            () => () => {
                hydrated_child_disposals += 1;
            },
        );
        const unregister_pending_parent = hydrate_when_visible(
            'pending-activation-parent',
            () => {
                parent_initializations += 1;
            },
        );

        dispose_hydrated('pending-activation-parent', pending_parent);
        observers[1].fire();
        assert(
            observers[1].disconnected === true &&
                parent_initializations === 0,
            'pending parent teardown cancels its own activation',
        );
        assert(
            hydrated_child_disposals === 1,
            'pending parent teardown also disposes a nested hydrated owner',
        );
        unregister_hydrated_child();
        unregister_pending_parent();
        roots.delete('hydrated-child');
        roots.delete('pending-activation-parent');
    }

    // Structural reactive removal disposes a nested owner and the item's own
    // resources before removing the range from the DOM.
    {
        const container = body.appendChild(make_node('section'));
        const items = signal([{ id: 'row' }]);
        const order = [];
        let nested = null;
        const original_remove = container.removeChild;
        container.removeChild = (node) => {
            order.push('dom-remove');
            return original_remove(node);
        };
        const dispose_list = bind_list(container, () => items.get(), {
            key: (item) => item.id,
            create() {
                const item = make_node('article');
                nested = item.appendChild(island_root('row-child'));
                roots.set('row-child', [nested]);
                return {
                    node: item,
                    dispose() {
                        order.push('item');
                    },
                };
            },
            deps: [items],
        });
        const unregister_nested = hydrate('row-child', () => () => {
            order.push('nested-owner');
        });

        items.set([]);
        await tick();
        assert(
            order.join(',') === 'nested-owner,item,dom-remove',
            `structural removal disposes nested/item resources before DOM removal: ${order.join(',')}`,
        );
        assert(
            dispose_hydrated('row-child', nested) === 0,
            'structurally removed nested owner is no longer tracked',
        );
        dispose_list();
        unregister_nested();
        roots.delete('row-child');
    }
} finally {
    dispose_hydrated();
    if (previous_document === undefined) {
        delete globalThis.document;
    } else {
        globalThis.document = previous_document;
    }
    if (previous_intersection_observer === undefined) {
        delete globalThis.IntersectionObserver;
    } else {
        globalThis.IntersectionObserver = previous_intersection_observer;
    }
}

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.owner-lifecycle.test.mjs: all assertions passed');
