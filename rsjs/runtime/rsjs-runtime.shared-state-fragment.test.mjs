// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Regression tests for how the page store table decides it has gone stale.
// Run with:
//
//     node rsjs/runtime/rsjs-runtime.shared-state-fragment.test.mjs
//
// C17: a fragment response (a partial render patched into a live page) carries
// its own copy of the seed table, so the document ends up with two elements
// holding the id. That is not a navigation. Adopting the fragment's table
// would rebuild every page store from an SSR snapshot while already-hydrated
// islands keep hard references to the discarded cells, splitting one logical
// scope into two disjoint cells that never see each other's writes.
//
// C30: a morphing patcher keeps the seed script node and rewrites its text in
// place, so node identity cannot detect the new page. The parsed content can.
// Both directions matter: a rewritten table must reseed, and an untouched one
// must keep the live (written-to) stores for late hydration.

import {
    acquire_shared_state,
    dispose_page_shared_state,
    install_page_shared_state_from_document,
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

function island_root() {
    return {
        isConnected: true,
        nextElementSibling: {
            tagName: 'SCRIPT',
            textContent: JSON.stringify({ signals: [], shared: [nav_binding] }),
            getAttribute(attribute) {
                if (attribute === 'type') return 'application/json';
                if (attribute === 'data-rsjs-island') return 'nav_island';
                return null;
            },
        },
        getAttribute(attribute) {
            return attribute === 'data-rsjs-island-root' ? 'nav_island' : null;
        },
    };
}

function seed_table_json(value) {
    return JSON.stringify({
        abi_version: 1,
        stores: [{ ...nav_binding, state: { value } }],
    });
}

/** `scope` mirrors the emitter's `data-rsjs-seed-scope` attribute. */
function seed_script(value, scope) {
    return {
        tagName: 'SCRIPT',
        textContent: seed_table_json(value),
        isConnected: true,
        getAttribute(attribute) {
            if (attribute === 'data-rsjs-seed-scope') return scope ?? null;
            return null;
        },
    };
}

const previous_document = globalThis.document;
// Seed tables in document order. getElementById returns the first match, the
// way a real DOM does when an id is duplicated.
let tables = [];

try {
    globalThis.document = {
        readyState: 'complete',
        getElementById(id) {
            return id === 'rsjs-shared-state-seed-table' ? (tables[0] ?? null) : null;
        },
        querySelectorAll(selector) {
            return selector === '#rsjs-shared-state-seed-table' ? tables.slice() : [];
        },
    };

    // ------------------------------------------------------------------
    // C17: a fragment's own seed table must not wipe the live page stores.
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    const shell = seed_script('shell-seed');
    tables = [shell];

    const header = acquire_shared_state(island_root(), 0);
    assert(header.field('value').get() === 'shell-seed', 'the header seeds from the page table');
    header.field('value').set('user-edited');

    // A fragment is patched into a container that sits earlier in document
    // order than the shell's trailing seed script, so getElementById now
    // returns the fragment's copy. Today's emitters ship it unmarked.
    tables = [seed_script('fragment-seed'), shell];

    const panel = acquire_shared_state(island_root(), 0);
    assert(
        panel.field('value').get() === 'user-edited',
        'an island hydrated after a fragment patch joins the live cell, not the fragment snapshot',
    );
    assert(
        panel.field('value') === header.field('value'),
        'the fragment patch leaves one cell per scope instead of splitting it in two',
    );
    panel.field('value').set('panel-wrote');
    assert(
        header.field('value').get() === 'panel-wrote',
        'both islands still observe each other after a fragment patch',
    );

    // ------------------------------------------------------------------
    // C17, marked fragments: when nothing is parsed yet, the document's own
    // table wins over a fragment that happens to come first.
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    tables = [seed_script('fragment-seed', 'fragment'), seed_script('shell-seed')];
    assert(
        acquire_shared_state(island_root(), 0).field('value').get() === 'shell-seed',
        'a marked fragment table is skipped when the document has its own table',
    );

    // A fragment injected into a page that has no table of its own is still
    // the only seed its islands can use.
    dispose_page_shared_state();
    tables = [seed_script('fragment-seed', 'fragment')];
    assert(
        acquire_shared_state(island_root(), 0).field('value').get() === 'fragment-seed',
        'a fragment table still seeds the scopes its own islands need',
    );

    // ------------------------------------------------------------------
    // C30 forward: a morphing patch rewrites the seed script in place.
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    const morphed = seed_script('page-a');
    tables = [morphed];

    const page_a = acquire_shared_state(island_root(), 0);
    assert(page_a.field('value').get() === 'page-a', 'page A seeds from page A\'s table');
    page_a.field('value').set('page-a-live');

    // idiomorph and friends match on the stable id, keep the node, and swap
    // its text. No dispose_hydrated() call happens.
    morphed.textContent = seed_table_json('page-b');
    install_page_shared_state_from_document();
    assert(
        acquire_shared_state(island_root(), 0).field('value').get() === 'page-b',
        'a morphing navigation that rewrites the seed script reseeds from the new table',
    );

    // The same holds without the explicit lifecycle call, for adapters that
    // rely on lazy seeding alone.
    dispose_page_shared_state();
    const lazy = seed_script('page-a');
    tables = [lazy];
    acquire_shared_state(island_root(), 0).field('value').set('page-a-live');
    lazy.textContent = seed_table_json('page-b');
    assert(
        acquire_shared_state(island_root(), 0).field('value').get() === 'page-b',
        'lazy seeding also notices a rewritten seed script',
    );

    // ------------------------------------------------------------------
    // C30 backward: an untouched page keeps its live stores.
    // ------------------------------------------------------------------
    dispose_page_shared_state();
    tables = [seed_script('seed')];
    const early = acquire_shared_state(island_root(), 0);
    early.field('value').set('written');
    install_page_shared_state_from_document();
    assert(
        acquire_shared_state(island_root(), 0).field('value').get() === 'written',
        'late hydration on an untouched page observes written state, not the SSR seed',
    );

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
console.log('rsjs-runtime.shared-state-fragment.test.mjs: all assertions passed');
