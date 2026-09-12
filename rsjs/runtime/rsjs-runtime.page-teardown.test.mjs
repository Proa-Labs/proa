// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Regression tests for the page-lifecycle teardown the client router drives.
// Run with:
//
//     node rsjs/runtime/rsjs-runtime.page-teardown.test.mjs
//
// C7 / C13: a soft navigation replaces one container, not the page. The
// runtime must offer a teardown scoped to that container, because islands in
// the persistent shell (header, nav, cart badge) are never re-hydrated after a
// soft navigation: island modules call hydrate() once at import, and scripts
// inside patched HTML never execute. A page-wide dispose_hydrated() therefore
// turns the shell into inert markup for the rest of the session.
//
// C26: teardown is a lifecycle boundary. One island whose cleanup throws must
// not skip the remaining islands or the page-store release that follows it.

import {
    acquire_shared_state,
    dispose_hydrated,
    dispose_hydrated_subtree,
    dispose_page_shared_state,
    hydrate,
} from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function make_node(tag) {
    const node = {
        tagName: tag,
        isConnected: true,
        parentNode: null,
        childNodes: [],
        attributes: {},
        appendChild(child) {
            child.parentNode = node;
            node.childNodes.push(child);
            return child;
        },
        // Mirrors the real Node.contains(): the node itself counts.
        contains(other) {
            let cursor = other;
            while (cursor != null) {
                if (cursor === node) return true;
                cursor = cursor.parentNode;
            }
            return false;
        },
        getAttribute(name) {
            return Object.prototype.hasOwnProperty.call(node.attributes, name)
                ? node.attributes[name]
                : null;
        },
    };
    return node;
}

function island_root(name, blob) {
    const root = make_node('rsjs-island');
    root.attributes['data-rsjs-island-root'] = name;
    root.nextElementSibling = {
        tagName: 'SCRIPT',
        textContent: JSON.stringify(blob ?? { signals: [], shared: [] }),
        getAttribute(attribute) {
            if (attribute === 'type') return 'application/json';
            if (attribute === 'data-rsjs-island') return name;
            return null;
        },
    };
    return root;
}

// Detach a subtree the way `target.replaceChildren(...)` does: the removed
// nodes stay reachable from JS but report isConnected === false.
function detach_children(container) {
    for (const child of container.childNodes) {
        child.isConnected = false;
        child.parentNode = null;
    }
    container.childNodes = [];
}

const previous_document = globalThis.document;

try {
    // ------------------------------------------------------------------
    // C7 / C13: scoped teardown keeps the shell's islands alive.
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    dispose_hydrated();

    const body = make_node('BODY');
    const shell = body.appendChild(make_node('HEADER'));
    const container = body.appendChild(make_node('MAIN'));
    const cart_badge = shell.appendChild(island_root('cart_badge'));
    const product_list = container.appendChild(island_root('product_list'));

    const roots_by_name = {
        cart_badge: [cart_badge],
        product_list: [product_list],
    };
    globalThis.document = {
        readyState: 'complete',
        body,
        documentElement: body,
        getElementById() {
            return null;
        },
        querySelectorAll(selector) {
            for (const [name, roots] of Object.entries(roots_by_name)) {
                if (selector === `rsjs-island[data-rsjs-island-root="${name}"]`) {
                    return roots;
                }
            }
            return [];
        },
    };

    let cart_badge_alive = false;
    let product_list_alive = false;
    hydrate('cart_badge', () => {
        cart_badge_alive = true;
        return () => {
            cart_badge_alive = false;
        };
    });
    hydrate('product_list', () => {
        product_list_alive = true;
        return () => {
            product_list_alive = false;
        };
    });
    assert(cart_badge_alive && product_list_alive, 'both islands hydrate at page load');

    // Exactly what the router's `inner` patch now does: scope the teardown to
    // the container it is about to replace, then swap that container's
    // children.
    const disposed = dispose_hydrated_subtree(container);
    detach_children(container);

    assert(disposed === 1, `a container patch disposes only its own islands (got ${disposed})`);
    assert(
        product_list_alive === false,
        'the island inside the replaced container is torn down',
    );
    assert(
        cart_badge_alive === true,
        'the shell island outside the replaced container keeps running after a soft navigation',
    );

    // The shell island is still tracked, so a later real page transition still
    // tears it down.
    assert(dispose_hydrated() === 1, 'the surviving shell island is still tracked');
    assert(cart_badge_alive === false, 'a full page teardown disposes the shell island too');

    // ------------------------------------------------------------------
    // A root that already left the document is disposed by a scoped
    // teardown as well: it is due for cleanup either way.
    // ------------------------------------------------------------------
    dispose_hydrated();
    const orphan_holder = make_node('SECTION');
    const orphan = orphan_holder.appendChild(island_root('orphan_widget'));
    roots_by_name.orphan_widget = [orphan];
    let orphan_alive = false;
    hydrate('orphan_widget', () => {
        orphan_alive = true;
        return () => {
            orphan_alive = false;
        };
    });
    orphan.isConnected = false;
    dispose_hydrated_subtree(container);
    assert(orphan_alive === false, 'a detached island root is disposed by a scoped teardown');
    delete roots_by_name.orphan_widget;

    // ------------------------------------------------------------------
    // A scoped teardown must not release page-scoped shared stores: the
    // shell islands that survive it still hold those cells.
    // ------------------------------------------------------------------
    dispose_hydrated();
    dispose_page_shared_state();
    const nav_binding = {
        scope_id: 0,
        type_key: 'nav.cart',
        schema_hash: 'sha256:nav',
        rust_type: 'NavCartState',
    };
    const seed = {
        textContent: JSON.stringify({
            abi_version: 1,
            stores: [{ ...nav_binding, state: { value: 'seeded' } }],
        }),
        isConnected: true,
        getAttribute() {
            return null;
        },
    };
    globalThis.document.getElementById = (id) =>
        id === 'rsjs-shared-state-seed-table' ? seed : null;

    const shell_store_root = shell.appendChild(
        island_root('nav_island', { signals: [], shared: [nav_binding] }),
    );
    const shell_store = acquire_shared_state(shell_store_root, 0);
    shell_store.field('value').set('user-edited');
    dispose_hydrated_subtree(container);
    assert(
        shell_store.field('value').get() === 'user-edited' &&
            acquire_shared_state(
                island_root('nav_island', { signals: [], shared: [nav_binding] }),
                0,
            ).field('value').get() === 'user-edited',
        'a scoped teardown keeps page shared stores live for the surviving shell islands',
    );

    // ------------------------------------------------------------------
    // Rust-quality error for a caller that forgot the container.
    // ------------------------------------------------------------------
    let container_error = null;
    try {
        dispose_hydrated_subtree(undefined);
    } catch (error) {
        container_error = error;
    }
    assert(
        container_error instanceof TypeError &&
            container_error.message.includes('rsjs/dispose-subtree-invalid-container') &&
            container_error.message.includes('dispose_hydrated()'),
        'dispose_hydrated_subtree() rejects a missing container with an actionable message',
    );

    // ------------------------------------------------------------------
    // C26: one throwing disposer must not abort the rest of the teardown.
    // ------------------------------------------------------------------
    dispose_hydrated();
    dispose_page_shared_state();
    const throwing_root = container.appendChild(island_root('throwing_island'));
    const tidy_root = container.appendChild(island_root('tidy_island'));
    roots_by_name.throwing_island = [throwing_root];
    roots_by_name.tidy_island = [tidy_root];

    const previous_console_error = console.error;
    const reported = [];
    console.error = (...args) => {
        reported.push(args[0]);
    };
    try {
        let tidy_disposed = false;
        hydrate('throwing_island', () => () => {
            throw new TypeError('cleanup blew up');
        });
        hydrate('tidy_island', () => () => {
            tidy_disposed = true;
        });

        acquire_shared_state(
            island_root('nav_island', { signals: [], shared: [nav_binding] }),
            0,
        )
            .field('value')
            .set('page-a-live');

        let thrown = null;
        try {
            dispose_hydrated();
        } catch (error) {
            thrown = error;
        }
        assert(thrown === null, 'a throwing island disposer does not escape dispose_hydrated()');
        assert(tidy_disposed === true, 'the islands after a throwing one are still disposed');
        assert(
            reported.some(
                (message) =>
                    typeof message === 'string' &&
                    message.includes('throwing_island') &&
                    message.includes('teardown'),
            ),
            'the failing island teardown is reported instead of being swallowed',
        );

        // The page-store release is the last statement of a full teardown, so
        // it must have run: the next page seeds fresh values.
        seed.textContent = JSON.stringify({
            abi_version: 1,
            stores: [{ ...nav_binding, state: { value: 'page-b' } }],
        });
        assert(
            acquire_shared_state(
                island_root('nav_island', { signals: [], shared: [nav_binding] }),
                0,
            )
                .field('value')
                .get() === 'page-b',
            'page shared state is still released after a throwing island disposer',
        );

        // The same guarantee for the scoped teardown.
        dispose_hydrated();
        let second_disposed = false;
        hydrate('throwing_island', () => () => {
            throw new TypeError('cleanup blew up again');
        });
        hydrate('tidy_island', () => () => {
            second_disposed = true;
        });
        let scoped_thrown = null;
        try {
            dispose_hydrated_subtree(container);
        } catch (error) {
            scoped_thrown = error;
        }
        assert(
            scoped_thrown === null,
            'a throwing island disposer does not escape dispose_hydrated_subtree()',
        );
        assert(second_disposed === true, 'a scoped teardown continues past a throwing disposer');
    } finally {
        console.error = previous_console_error;
    }

    dispose_hydrated();
    dispose_page_shared_state();
} finally {
    if (previous_document === undefined) {
        delete globalThis.document;
    } else {
        globalThis.document = previous_document;
    }
}

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.page-teardown.test.mjs: all assertions passed');
