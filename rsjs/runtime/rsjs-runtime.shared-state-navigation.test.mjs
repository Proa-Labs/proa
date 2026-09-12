// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Regression tests for page-lifecycle boundaries that do NOT go through an
// explicit dispose_hydrated() call. Run with:
//
//     node rsjs/runtime/rsjs-runtime.shared-state-navigation.test.mjs
//
// F15: an SPA adapter may replace the whole DOM (innerHTML swap) and rely on
// the runtime's automatic detached-root pruning. Page shared-state stores
// are keyed by scope ids that restart at 0 on every server render, so the
// stores must be tied to the seed-table element they were parsed from: when
// that element leaves the document, the next acquire must reseed from the
// new page's table instead of silently handing out the previous page's live
// values. Late hydration on an untouched page must keep observing live
// (written-to) state.
//
// F14: the per-island hydration-blob cache must be invalidated by every
// record-dispose path, including the one automatic pruning uses, so a
// morphing router that keeps the island root element but rewrites the
// adjacent state script rehydrates from the new blob.

import {
    acquire_shared_state,
    dispose_hydrated,
    dispose_page_shared_state,
    hydrate,
    install_page_shared_state_from_document,
    read_signal_initials,
} from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

const nav_binding = {
    scope_id: 0,
    type_key: 'nav.cart',
    schema_hash: 'sha256:nav',
    rust_type: 'NavCartState',
};

function island_root(name, blob_text) {
    const script = {
        tagName: 'SCRIPT',
        textContent: blob_text,
        getAttribute(attribute) {
            if (attribute === 'type') return 'application/json';
            if (attribute === 'data-rsjs-island') return name;
            return null;
        },
    };
    return {
        isConnected: true,
        nextElementSibling: script,
        getAttribute(attribute) {
            if (attribute === 'data-rsjs-island-root') return name;
            return null;
        },
    };
}

function shared_island_root(binding) {
    return island_root(
        `nav_island_${binding.scope_id}`,
        JSON.stringify({ signals: [], shared: [binding] }),
    );
}

function seed_table_json(value) {
    return JSON.stringify({
        abi_version: 1,
        stores: [{ ...nav_binding, state: { value } }],
    });
}

function seed_script(json) {
    return { textContent: json, isConnected: true };
}

const previous_document = globalThis.document;

try {
    // ------------------------------------------------------------------
    // F15 side A: DOM-replacement navigation without dispose_hydrated().
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    let current_seed_script = seed_script(seed_table_json('page-a'));
    globalThis.document = {
        readyState: 'complete',
        getElementById(id) {
            return id === 'rsjs-shared-state-seed-table' ? current_seed_script : null;
        },
        querySelectorAll() {
            return [];
        },
    };

    const store_a = acquire_shared_state(shared_island_root(nav_binding), 0);
    assert(
        store_a.field('value').get() === 'page-a',
        'page A hydration seeds from page A\'s table',
    );
    store_a.field('value').set('page-a-live');
    assert(
        acquire_shared_state(shared_island_root(nav_binding), 0).field('value').get() ===
            'page-a-live',
        'sibling islands on the same page share the live cell',
    );

    // The adapter swaps document.body.innerHTML for page B. Page A's seed
    // script is now detached; page B ships a fresh one with the same scope
    // id and schema metadata. No dispose_hydrated() call happens; only the
    // automatic detached-root pruning would run in a real browser.
    current_seed_script.isConnected = false;
    current_seed_script = seed_script(seed_table_json('page-b'));

    const store_b = acquire_shared_state(shared_island_root(nav_binding), 0);
    assert(
        store_b.field('value').get() === 'page-b',
        'after a DOM-replacement navigation the next acquire reseeds from the new table ' +
            'instead of returning the previous page\'s live value',
    );

    // ------------------------------------------------------------------
    // F15 side B: late hydration on a live, untouched page keeps stores.
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    current_seed_script = seed_script(seed_table_json('seed'));

    const early = acquire_shared_state(shared_island_root(nav_binding), 0);
    early.field('value').set('written');
    install_page_shared_state_from_document();
    const late = acquire_shared_state(shared_island_root(nav_binding), 0);
    assert(
        late.field('value').get() === 'written',
        'late hydration on a live page observes written state, not the SSR seed',
    );

    // ------------------------------------------------------------------
    // F14: the record-dispose path (the one automatic pruning invokes)
    // invalidates the cached island blob; rehydration reads the new blob.
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    const counter_root = island_root('counter', JSON.stringify({ signals: [5], shared: [] }));
    globalThis.document = {
        readyState: 'complete',
        getElementById() {
            return null;
        },
        querySelectorAll(selector) {
            return selector === 'rsjs-island[data-rsjs-island-root="counter"]'
                ? [counter_root]
                : [];
        },
    };

    const seeds_seen = [];
    let dispose = hydrate('counter', (root) => {
        seeds_seen.push(read_signal_initials(root)[0]);
    });
    assert(seeds_seen[0] === 5, 'first hydration seeds from the original blob');

    // A morphing router rewrites the adjacent script but keeps the root
    // element, then disposes. The returned disposer is the same record
    // disposer prune_detached_hydrations invokes.
    counter_root.nextElementSibling.textContent = JSON.stringify({ signals: [9], shared: [] });
    dispose();
    assert(
        read_signal_initials(counter_root)[0] === 9,
        'record dispose invalidates the cached blob so reads see the new script content',
    );

    dispose = hydrate('counter', (root) => {
        seeds_seen.push(read_signal_initials(root)[0]);
    });
    assert(seeds_seen[1] === 9, 'rehydration after dispose seeds from the NEW blob');
    dispose();
    dispose_hydrated();
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
console.log('rsjs-runtime.shared-state-navigation.test.mjs: all assertions passed');
