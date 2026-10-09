// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    acquire_shared_state,
    batch,
    bind_text,
    memo,
    dispose_hydrated,
    dispose_page_shared_state,
    install_page_shared_state,
    read_shared_state_bindings,
    signal_initial_wide,
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

function root(binding) {
    const name = `shared_test_${binding.scope_id}`;
    const script = {
        tagName: 'SCRIPT',
        textContent: JSON.stringify({ signals: [], shared: [binding] }),
        getAttribute(attribute) {
            if (attribute === 'type') return 'application/json';
            if (attribute === 'data-rsjs-island') return name;
            return null;
        },
    };
    return {
        nextElementSibling: script,
        getAttribute(attribute) {
            if (attribute === 'data-rsjs-island-root') return name;
            return null;
        },
    };
}

const outer_binding = {
    scope_id: 10,
    type_key: 'test.workbench',
    schema_hash: 'sha256:abc',
    rust_type: 'WorkbenchState',
};
const inner_binding = {
    ...outer_binding,
    scope_id: 11,
};

dispose_page_shared_state();

const replaceable_root = root(outer_binding);
assert(
    read_shared_state_bindings(replaceable_root)[0].scope_id === outer_binding.scope_id,
    'island metadata is initially read from the adjacent state blob',
);
replaceable_root.nextElementSibling.textContent = JSON.stringify({
    signals: [],
    shared: [inner_binding],
});
dispose_hydrated(undefined, replaceable_root);
assert(
    read_shared_state_bindings(replaceable_root)[0].scope_id === inner_binding.scope_id,
    'disposing a retained root invalidates its cached hydration metadata',
);

let abi_mismatch;
try {
    install_page_shared_state({ abi_version: 2, stores: [] });
} catch (error) {
    abi_mismatch = error;
}
assert(
    abi_mismatch?.code === 'rsjs/shared-state-abi-mismatch',
    'runtime ABI mismatch has a stable error code',
);

let invalid_seed;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [{ ...outer_binding, state: [] }],
    });
} catch (error) {
    invalid_seed = error;
}
assert(
    invalid_seed?.code === 'rsjs/shared-state-invalid-seed',
    'invalid state snapshots have a stable error code',
);

let duplicate_scope;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [
            { ...outer_binding, state: { button_bg: 'partial', selected_node: null } },
            { ...outer_binding, state: { button_bg: 'duplicate', selected_node: null } },
        ],
    });
} catch (error) {
    duplicate_scope = error;
}
assert(
    duplicate_scope?.code === 'rsjs/shared-state-duplicate-scope',
    'duplicate seed scopes have a stable error code',
);
let partial_install;
try {
    acquire_shared_state(root(outer_binding), 0);
} catch (error) {
    partial_install = error;
}
assert(
    partial_install?.code === 'rsjs/shared-state-missing-scope',
    'invalid tables install no partial stores',
);

let type_key_conflict;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [
            { ...outer_binding, state: { button_bg: 'first' } },
            {
                ...inner_binding,
                schema_hash: 'sha256:different',
                state: { button_bg: 'second' },
            },
        ],
    });
} catch (error) {
    type_key_conflict = error;
}
assert(
    type_key_conflict?.code === 'rsjs/shared-state-type-key-conflict',
    'one type_key cannot identify different schemas in separate scopes',
);
let conflict_partial_install;
try {
    acquire_shared_state(root(outer_binding), 0);
} catch (error) {
    conflict_partial_install = error;
}
assert(
    conflict_partial_install?.code === 'rsjs/shared-state-missing-scope',
    'a type-key conflict installs no partial stores',
);

let missing_binding;
try {
    acquire_shared_state(root(outer_binding), 1);
} catch (error) {
    missing_binding = error;
}
assert(
    missing_binding?.code === 'rsjs/shared-state-missing-binding',
    'missing island bindings have a stable error code',
);

install_page_shared_state({
    abi_version: 1,
    stores: [
        { ...outer_binding, state: { button_bg: 'green', selected_node: null } },
        { ...inner_binding, state: { button_bg: 'blue', selected_node: 7 } },
    ],
});

// Independent island roots sharing one resolved scope acquire the same cells.
const toolbar = acquire_shared_state(root(outer_binding), 0);
const preview = acquire_shared_state(root(outer_binding), 0);
assert(toolbar.field('button_bg') === preview.field('button_bg'), 'same scope shares one field cell');
const renamed_rust_type = acquire_shared_state(
    root({ ...outer_binding, rust_type: 'renamed::WorkbenchState' }),
    0,
);
assert(
    renamed_rust_type.field('button_bg') === toolbar.field('button_bg'),
    'Rust diagnostic names are not part of browser protocol identity',
);

const node = { textContent: '' };
bind_text(node, () => preview.field('button_bg').get(), [preview.field('button_bg')]);
assert(node.textContent === 'green', 'shared field seeds a binding');
toolbar.field('button_bg').set('purple');
assert(preview.field('button_bg').get() === 'purple', 'writes are synchronously readable');
await tick();
assert(node.textContent === 'purple', 'writes update another island after the batched flush');

// A computation only subscribes to the field it reads.
let memo_runs = 0;
const label = memo(() => {
    memo_runs += 1;
    return `color:${preview.field('button_bg').get()}`;
}, [preview.field('button_bg')]);
preview.field('selected_node').set(9);
await tick();
assert(memo_runs === 1, 'unrelated shared field does not invalidate memo');
preview.field('button_bg').set('orange');
await tick();
assert(memo_runs === 2, 'read shared field invalidates memo');
assert(label.get() === 'color:orange', 'memo observes the new shared field');

// Nested batches across independent owners sharing one provider commit the
// subscriber once. Both owners see every write synchronously; only subscriber
// delivery waits for the scheduler's outermost microtask commit.
const toolbar_bg = toolbar.field('button_bg');
const toolbar_selection = toolbar.field('selected_node');
const preview_bg = preview.field('button_bg');
const preview_selection = preview.field('selected_node');
let cross_owner_runs = 0;
const cross_owner_snapshots = [];
const cross_owner = memo(() => {
    cross_owner_runs += 1;
    const snapshot = [preview_bg.get(), preview_selection.get()];
    cross_owner_snapshots.push(snapshot);
    return snapshot;
}, [preview_bg, preview_selection]);
batch(() => {
    toolbar_bg.set('first');
    assert(preview_bg.get() === 'first', 'outer shared-state write is synchronously cross-owner');

    batch(() => {
        toolbar_selection.set(42);
        toolbar_bg.set('second');
        assert(
            preview_selection.get() === 42,
            'nested shared-state write is synchronously cross-owner',
        );
        assert(preview_bg.get() === 'second', 'nested shared-state overwrite is visible');
        assert(cross_owner_runs === 1, 'nested shared-state batch does not flush its subscriber');
    });

    assert(cross_owner_runs === 1, 'outer shared-state callback keeps subscribers deferred');
});
assert(toolbar_bg.get() === 'second', 'shared-state batch retains the final value');
assert(cross_owner_runs === 1, 'shared-state batch does not flush synchronously');
await tick();
assert(cross_owner_runs === 2, 'nested shared-state batch commits each dependent once');
assert(
    cross_owner_snapshots.length === 2
        && cross_owner_snapshots[1][0] === 'second'
        && cross_owner_snapshots[1][1] === 42,
    `cross-owner subscriber sees only final batch values: ${JSON.stringify(cross_owner_snapshots)}`,
);
assert(node.textContent === 'second', 'cross-owner DOM subscriber observes the committed value');

// Late hydration acquires the live cell and cannot restore the SSR seed.
const inspector = acquire_shared_state(root(outer_binding), 0);
assert(inspector.field('button_bg').get() === 'second', 'late island sees live value, not seed');
install_page_shared_state({
    abi_version: 1,
    stores: [{ ...outer_binding, state: { selected_node: null, button_bg: 'green' } }],
});
assert(
    inspector.field('button_bg').get() === 'second',
    'canonical equal seed reinstall never overwrites live cells',
);
let seed_conflict;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [{ ...outer_binding, state: { button_bg: 'stale', selected_node: null } }],
    });
} catch (error) {
    seed_conflict = error;
}
assert(
    seed_conflict?.code === 'rsjs/shared-state-seed-conflict',
    'repeated publication of one scope rejects a different canonical initial seed',
);
assert(
    inspector.field('button_bg').get() === 'second',
    'a conflicting repeated seed does not alter the live store',
);

// Nested/sibling scope ids remain independent even with the same type key.
const nested = acquire_shared_state(root(inner_binding), 0);
assert(nested.field('button_bg').get() === 'blue', 'different provider scope has independent seed');
nested.field('button_bg').set('cyan');
assert(toolbar.field('button_bg').get() === 'second', 'nested write does not affect outer scope');

// Schema failures are structured and keep the calling island in control.
let mismatch;
try {
    acquire_shared_state(root({ ...outer_binding, schema_hash: 'sha256:wrong' }), 0);
} catch (error) {
    mismatch = error;
}
assert(mismatch?.code === 'rsjs/shared-state-schema-mismatch', 'schema mismatch has stable code');

let missing_field;
try {
    toolbar.field('not_a_field');
} catch (error) {
    missing_field = error;
}
assert(
    missing_field?.code === 'rsjs/shared-state-missing-field',
    'missing schema fields have a stable error code',
);

let scope_collision;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [
            {
                ...outer_binding,
                schema_hash: 'sha256:replacement',
                state: { button_bg: 'replacement', selected_node: null },
            },
        ],
    });
} catch (error) {
    scope_collision = error;
}
assert(
    scope_collision?.code === 'rsjs/shared-state-scope-collision',
    'reusing a live scope with incompatible metadata has a stable error code',
);
assert(
    toolbar.field('button_bg').get() === 'second',
    'scope collisions do not alter the existing live store',
);

label._dispose();
cross_owner._dispose();
dispose_hydrated();

let disposed_scope;
try {
    acquire_shared_state(root(outer_binding), 0);
} catch (error) {
    disposed_scope = error;
}
assert(disposed_scope?.code === 'rsjs/shared-state-missing-scope', 'page teardown releases stores');

const page_runtime = globalThis[Symbol.for('rsjs.page-runtime')];
assert(page_runtime?.abi_version === 1, 'runtime publishes the page lifecycle bridge');
const previous_document = globalThis.document;
globalThis.document = {
    getElementById(id) {
        if (id !== 'rsjs-shared-state-seed-table') return null;
        return {
            textContent: JSON.stringify({
                abi_version: 1,
                stores: [
                    {
                        ...outer_binding,
                        state: { button_bg: 'route-green', selected_node: null },
                    },
                ],
            }),
        };
    },
};
page_runtime.installPage();
assert(
    acquire_shared_state(root(outer_binding), 0).field('button_bg').get() === 'route-green',
    'router bridge installs the replacement page seed before hydration',
);
page_runtime.disposePage();
if (previous_document === undefined) {
    delete globalThis.document;
} else {
    globalThis.document = previous_document;
}

// Wide-int shared values arrive as JSON strings. Recursive codec paths must
// seed BigInt values through nested structs and Vec elements.
dispose_page_shared_state();
const cart_binding = {
    scope_id: 20,
    type_key: 'test.cart',
    schema_hash: 'sha256:cart',
    rust_type: 'CartState',
};
const cart_table = {
    abi_version: 1,
    stores: [
        {
            ...cart_binding,
            state: {
                count: '8',
                total_cents: '9007199254740993',
                coupon: null,
                label: '8',
            },
            wide_paths: [['count'], ['total_cents'], ['coupon']],
        },
    ],
};
install_page_shared_state(cart_table);
const cart = acquire_shared_state(root(cart_binding), 0);
assert(cart.field('count').get() === 8n, 'wide field seeds as BigInt, not as the wire string');
assert(
    cart.field('total_cents').get() === 9007199254740993n,
    'wide seeding stays exact past 2^53',
);
assert(cart.field('coupon').get() === null, 'a null Option seed passes through unconverted');
assert(cart.field('label').get() === '8', 'fields outside wide_paths keep their string value');
cart.field('count').update((c) => c + 1n);
assert(
    cart.field('count').get() === 9n,
    'update(c => c + 1n) round-trips numerically instead of concatenating',
);

// Late hydration preservation: reinstalling the page table never resets a
// live wide cell back to its SSR seed, and a late island sees the BigInt.
install_page_shared_state(cart_table);
assert(cart.field('count').get() === 9n, 'reinstall preserves the live wide value');
const late_cart = acquire_shared_state(root(cart_binding), 0);
assert(late_cart.field('count').get() === 9n, 'late hydration observes the live BigInt value');

const nested_cart_binding = {
    scope_id: 24,
    type_key: 'test.nested-cart',
    schema_hash: 'sha256:nested-cart',
    rust_type: 'NestedCartState',
};
install_page_shared_state({
    abi_version: 1,
    stores: [
        {
            ...nested_cart_binding,
            state: {
                items: [
                    { cents: '9007199254740993', quantity: 1 },
                    { cents: '-7', quantity: 2 },
                ],
                summary: { outstanding_cents: '42' },
            },
            wide_paths: [
                ['items', '*', 'cents'],
                ['summary', 'outstanding_cents'],
            ],
        },
    ],
});
const nested_cart = acquire_shared_state(root(nested_cart_binding), 0);
assert(
    nested_cart.field('items').get()[0].cents === 9007199254740993n &&
        nested_cart.field('items').get()[1].cents === -7n,
    'wide paths decode every Vec element without losing precision',
);
assert(
    nested_cart.field('summary').get().outstanding_cents === 42n,
    'wide paths decode nested ClientProps fields',
);

// Malformed wide metadata is rejected during validation, before any store
// is installed.
let non_integer_wide;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [
            {
                ...cart_binding,
                scope_id: 21,
                state: { count: 'not-an-integer' },
                wide: ['count'],
            },
        ],
    });
} catch (error) {
    non_integer_wide = error;
}
assert(
    non_integer_wide?.code === 'rsjs/shared-state-invalid-seed',
    'a wide field that cannot decode to BigInt has a stable error code',
);
let ghost_wide;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [
            { ...cart_binding, scope_id: 22, state: { count: '1' }, wide: ['ghost'] },
        ],
    });
} catch (error) {
    ghost_wide = error;
}
assert(
    ghost_wide?.code === 'rsjs/shared-state-invalid-seed',
    'a wide entry naming a missing state field has a stable error code',
);

let unsafe_number_wide;
try {
    install_page_shared_state({
        abi_version: 1,
        stores: [
            {
                ...cart_binding,
                scope_id: 23,
                state: { count: 9007199254740992 },
                wide: ['count'],
            },
        ],
    });
} catch (error) {
    unsafe_number_wide = error;
}
assert(
    unsafe_number_wide?.code === 'rsjs/shared-state-invalid-seed',
    'an unsafe JSON number is rejected before BigInt can preserve the wrong integer',
);
assert(
    signal_initial_wide([9007199254740992], 0, 7n) === 7n,
    'island-local wide signals also reject unsafe Number seeds',
);
dispose_page_shared_state();

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log('rsjs-runtime.shared-state.test.mjs: all assertions passed');
