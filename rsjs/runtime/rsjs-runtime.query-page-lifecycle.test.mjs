// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Query cache integration with the canonical RSJS page lifecycle. Run with:
//
//     node rsjs/runtime/rsjs-runtime.query-page-lifecycle.test.mjs

// Load the optional query module first to prove registration does not depend
// on module-script evaluation order.
const {
    get_entry,
    register_query,
    rsjs,
    set_data,
} = await import('./rsjs-runtime-query.js');
const {
    dispose_hydrated,
} = await import('./rsjs-runtime.js');

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

const page_runtime = globalThis[Symbol.for('rsjs.page-runtime')];
assert(page_runtime?.abi_version === 1, 'core runtime should publish its page bridge');
assert(
    typeof page_runtime?.registerPageDisposer === 'function',
    'page bridge should accept optional generation-owned runtime resources',
);

const first_key = ['query-page-lifecycle', 'first'];
set_data(first_key, { generation: 1 });
const first = register_query({
    island: 'query-page-lifecycle',
    query_id: 0,
    key: first_key,
    fetcher: '/api/query-page-lifecycle',
    options: { stale_time_ms: 60_000 },
});
const off_first = first.subscribe(() => {});
const first_entry = get_entry(first_key);

// Container patches preserve page-global query data for persistent shell
// owners. Targeted island teardown is likewise not a generation boundary.
assert(
    page_runtime.disposeSubtree({}) === 0,
    'empty subtree teardown should have no hydration records',
);
assert(
    get_entry(first_key) === first_entry && first.data()?.generation === 1,
    'subtree disposal must retain query identity and data',
);
dispose_hydrated('unrelated-island');
assert(
    get_entry(first_key) === first_entry,
    'targeted island disposal must retain page query state',
);

// The router bridge delegates its full-page transition to the canonical
// unfiltered disposal path, which runs optional generation disposers.
page_runtime.disposePage();
assert(
    get_entry(first_key) === undefined,
    'router full-page teardown should clear the prior query generation',
);
let stale_handle_error;
try {
    first.subscribe(() => {});
} catch (error) {
    stale_handle_error = error;
}
assert(
    stale_handle_error?.message.includes('query-disposed'),
    'router full-page teardown should invalidate prior-generation handles',
);
off_first();

// Direct callers of the canonical full teardown receive the same behavior.
const second_key = ['query-page-lifecycle', 'second'];
set_data(second_key, { generation: 2 });
const second = register_query({
    island: 'query-page-lifecycle',
    query_id: 1,
    key: second_key,
    fetcher: '/api/query-page-lifecycle',
    options: { stale_time_ms: 60_000 },
});
const off_second = second.subscribe(() => {});
assert(
    rsjs.query(second_key).data()?.generation === 2,
    'replacement generation should start with independent query data',
);
dispose_hydrated();
assert(
    get_entry(second_key) === undefined,
    'direct unfiltered teardown should clear page query state',
);
off_second();

if (failed > 0) process.exit(1);
console.log('All query/page lifecycle integration tests passed.');
