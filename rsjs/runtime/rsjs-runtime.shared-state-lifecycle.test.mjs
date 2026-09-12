// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    acquire_shared_state,
    bind_text,
    dispose_page_shared_state,
    hydrate,
    install_page_shared_state,
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

const binding = {
    scope_id: 31,
    type_key: 'test.lifecycle',
    schema_hash: 'sha256:lifecycle',
    rust_type: 'LifecycleState',
};

function root() {
    const name = 'shared_state_lifecycle';
    return {
        nextElementSibling: {
            tagName: 'SCRIPT',
            textContent: JSON.stringify({ signals: [], shared: [binding] }),
            getAttribute(attribute) {
                if (attribute === 'type') return 'application/json';
                if (attribute === 'data-rsjs-island') return name;
                return null;
            },
        },
        getAttribute(attribute) {
            if (attribute === 'data-rsjs-island-root') return name;
            return null;
        },
    };
}

function install(label) {
    install_page_shared_state({
        abi_version: 1,
        stores: [{ ...binding, state: { label } }],
    });
}

dispose_page_shared_state();
install('old');

const old_store = acquire_shared_state(root(), 0);
const old_field = old_store.field('label');
const old_node = { textContent: '' };
const dispose_old_binding = bind_text(
    old_node,
    () => old_field.get(),
    [old_field],
);
assert(old_node.textContent === 'old', 'old generation binds its initial value');

old_field.set('queued-old-update');
dispose_page_shared_state();
await tick();
assert(
    old_node.textContent === 'old',
    'page teardown cancels pending SharedState DOM work',
);

for (const [operation, invoke] of [
    ['store field lookup', () => old_store.field('label')],
    ['field read', () => old_field.get()],
    ['field write', () => old_field.set('after-dispose')],
]) {
    let error;
    try {
        invoke();
    } catch (caught) {
        error = caught;
    }
    assert(
        error?.code === 'rsjs/shared-state-disposed',
        `${operation} fails with the disposed-generation error`,
    );
}

// A resource disposer retained by an owner remains idempotent after the page
// lifecycle has already severed its field subscription.
dispose_old_binding();
dispose_old_binding();

install('new');
const new_store = acquire_shared_state(root(), 0);
const new_field = new_store.field('label');
const new_node = { textContent: '' };
const dispose_new_binding = bind_text(
    new_node,
    () => new_field.get(),
    [new_field],
);
new_field.set('new-update');
await tick();
assert(
    new_node.textContent === 'new-update',
    'a replacement generation schedules normally after old pending work is cancelled',
);

dispose_new_binding();
dispose_page_shared_state();

// A malformed page-wide schema table is validated before an unrelated owner
// initializer can attach any client behavior.
const conflict_name = 'shared_state_schema_conflict';
const conflict_root = {
    getAttribute(attribute) {
        if (attribute === 'data-rsjs-island-root') return conflict_name;
        return null;
    },
};
const conflict_seed = {
    isConnected: true,
    textContent: JSON.stringify({
        abi_version: 1,
        stores: [
            { ...binding, state: { label: 'one' } },
            {
                ...binding,
                scope_id: 32,
                schema_hash: 'sha256:other',
                state: { label: 'two' },
            },
        ],
    }),
    getAttribute() {
        return null;
    },
};
const previous_document = globalThis.document;
const previous_console_error = console.error;
let reported_install_error;
globalThis.document = {
    readyState: 'complete',
    querySelectorAll(selector) {
        if (selector === '#rsjs-shared-state-seed-table') return [conflict_seed];
        if (
            selector ===
            `rsjs-island[data-rsjs-island-root="${conflict_name}"]`
        ) {
            return [conflict_root];
        }
        return [];
    },
};
console.error = (_message, error) => {
    reported_install_error = error;
};
let initializer_started = false;
const dispose_conflict_hydration = hydrate(conflict_name, () => {
    initializer_started = true;
});
dispose_conflict_hydration();
console.error = previous_console_error;
if (previous_document === undefined) {
    delete globalThis.document;
} else {
    globalThis.document = previous_document;
}
assert(
    initializer_started === false,
    'schema conflict is rejected before the owner initializer runs',
);
assert(
    reported_install_error?.code === 'rsjs/shared-state-type-key-conflict',
    'pre-activation schema conflict keeps its structured protocol error',
);
dispose_page_shared_state();

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.shared-state-lifecycle.test.mjs: all assertions passed');
