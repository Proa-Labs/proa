// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Smoke tests for rsjs query runtime regressions. Run with:
//
//     node rsjs/runtime/rsjs-runtime-query.smoke.mjs
//
// Each test exits the process with a non-zero status on failure so
// CI / a manual run can detect breakage. The file is deliberately
// dependency-free (no Jest / Vitest) so `node --check` validates it
// in isolation.
//
// Some tests need a `document` shim because the runtime walks
// `<script type="application/json" data-rsjs-island-queries=...>`
// blobs at registration time; the shim exposes the minimum surface
// area the runtime touches.

function install_document_shim(blobs, page_query_seed_table = null) {
    const elements = blobs.map(({ island, payload, kind = 'queries' }) => {
        const attrs = new Map();
        attrs.set(`data-rsjs-island-${kind}`, island);
        return {
            textContent: JSON.stringify(payload),
            getAttribute: (k) => attrs.get(k) ?? null,
            hasAttribute: (k) => attrs.has(k),
            setAttribute: (k, v) => attrs.set(k, v),
        };
    });
    const page_query_seeds = [];
    const append_page_query_seed = (payload) => {
        const element = {
            textContent: JSON.stringify(payload),
        };
        page_query_seeds.push(element);
        return element;
    };
    if (page_query_seed_table != null) {
        append_page_query_seed(page_query_seed_table);
    }
    globalThis.document = {
        getElementById(id) {
            return id === 'rsjs-query-seed-table' ? (page_query_seeds[0] ?? null) : null;
        },
        querySelectorAll(selector) {
            if (selector.includes('id="rsjs-query-seed-table"')) {
                return page_query_seeds;
            }
            if (selector.includes('data-rsjs-island-queries')) {
                return elements.filter((el) => el.hasAttribute('data-rsjs-island-queries'));
            }
            if (selector.includes('data-rsjs-island-mutations')) {
                return elements.filter((el) => el.hasAttribute('data-rsjs-island-mutations'));
            }
            if (selector.startsWith('script')) return elements;
            return [];
        },
    };
    return { append_page_query_seed };
}

const document_shim = install_document_shim([
    {
        island: 'stub_then_register_island',
        payload: { '0': { data: 99, data_updated_at: 1 } },
    },
], {
    queries: [
        {
            key: ['page_seed_query'],
            state: { key: ['page_seed_query'], data: 'PAGE', data_updated_at: 7 },
        },
        {
            key: ['page_seed_register'],
            state: { key: ['page_seed_register'], data: 'REGISTER', data_updated_at: 8 },
        },
        {
            key: ['runtime_key_query', 'AAPL'],
            state: { key: ['runtime_key_query', 'AAPL'], data: 'AAPL', data_updated_at: 9 },
        },
    ],
});

const { rsjs, register_query, register_mutation, set_data } = await import(
    './rsjs-runtime-query.js'
);

function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        process.exit(1);
    }
}

function state_rank(state) {
    return JSON.stringify(state);
}

// `QueryValue::as_i64()` has one stable browser representation: an in-range
// BigInt. Safe JSON Numbers are promoted; fractional, unsafe, and out-of-range
// values use the same zero fallback as Rust's serde_json projection.
assert(rsjs.__as_i64(7) === 7n, 'safe integral Number should promote to i64 BigInt');
assert(rsjs.__as_i64(-7n) === -7n, 'in-range BigInt should stay exact');
assert(
    rsjs.__as_i64('9007199254740993') === 9007199254740993n,
    'canonical decimal strings should preserve integers beyond Number precision',
);
assert(
    rsjs.__as_i64('-9223372036854775808') === -9223372036854775808n,
    'canonical decimal i64::MIN string should be accepted',
);
assert(
    rsjs.__as_i64('9223372036854775807') === 9223372036854775807n,
    'canonical decimal i64::MAX string should be accepted',
);
assert(
    rsjs.__as_i64(-9223372036854775808n) === -9223372036854775808n,
    'i64::MIN should be accepted',
);
assert(
    rsjs.__as_i64(9223372036854775807n) === 9223372036854775807n,
    'i64::MAX should be accepted',
);
for (const invalid of [
    1.5,
    Number.NaN,
    Number.POSITIVE_INFINITY,
    Number.MAX_SAFE_INTEGER + 1,
    -9223372036854775809n,
    9223372036854775808n,
    '',
    '-0',
    '+1',
    '01',
    ' 1',
    '1.0',
    '-9223372036854775809',
    '9223372036854775808',
]) {
    assert(rsjs.__as_i64(invalid) === 0n, `invalid i64 projection should fall back: ${invalid}`);
}

function script_element(attrs, payload, nextElementSibling = null) {
    const attr_map = new Map(Object.entries(attrs));
    return {
        tagName: 'SCRIPT',
        textContent: JSON.stringify(payload),
        nextElementSibling,
        getAttribute: (k) => attr_map.get(k) ?? null,
        hasAttribute: (k) => attr_map.has(k),
        setAttribute: (k, v) => attr_map.set(k, v),
    };
}

function root_with_query_blob(island, payload) {
    return root_with_blobs(island, { queries: payload });
}

function root_with_blobs(island, { queries, mutations }) {
    let next = null;
    if (mutations !== undefined) {
        next = script_element(
            { type: 'application/json', 'data-rsjs-island-mutations': island },
            mutations,
            next,
        );
    }
    if (queries !== undefined) {
        next = script_element(
            { type: 'application/json', 'data-rsjs-island-queries': island },
            queries,
            next,
        );
    }
    const signals = script_element(
        { type: 'application/json', 'data-rsjs-island': island },
        { signals: [] },
        next,
    );
    return {
        nextElementSibling: signals,
        getAttribute: (k) => (k === 'id' ? island : null),
    };
}

// Page-level query seed table: public handles and generated registrations can
// hydrate by structural key without reparsing each adjacent island blob.
{
    const handle = rsjs.query(['page_seed_query']);
    assert(handle.data() === 'PAGE', `expected page-level seed, got ${handle.data()}`);
    assert(
        handle.data_updated_at() === 7,
        `expected page-level data_updated_at 7, got ${handle.data_updated_at()}`,
    );

    const registered = register_query({
        island: 'page_seed_register_island',
        query_id: 0,
        key: ['page_seed_register'],
        fetcher: '/api/page-seed-register',
        options: {},
    });
    assert(
        registered.data() === 'REGISTER',
        `expected register_query page seed, got ${registered.data()}`,
    );
}

// Out-of-order boundaries and client-navigation fragments can append another
// page-level table after the runtime has already consumed the document table.
// The next query lookup must discover that late table without reloading the
// runtime module.
{
    document_shim.append_page_query_seed({
        queries: [
            {
                key: ['late_fragment_seed'],
                state: {
                    key: ['late_fragment_seed'],
                    data: 'LATE',
                    data_updated_at: 10,
                },
            },
        ],
    });
    const handle = rsjs.query(['late_fragment_seed']);
    assert(handle.data() === 'LATE', `expected late fragment seed, got ${handle.data()}`);
}

// Independently completed out-of-order boundaries can carry the same
// structural key with different server snapshots. Completion order must not
// choose the cache value: the lexicographically smallest serialized state
// (the same winner selected by RsjsRenderArtifacts) replaces a prior SSR seed
// and notifies existing subscribers. Once browser code writes the cache, late
// SSR fragments no longer own it.
{
    const key = ['ooo_seed_conflict'];
    const later_lexical_state = {
        key,
        data: 'Z',
        data_updated_at: 90,
    };
    document_shim.append_page_query_seed({
        queries: [
            {
                key,
                state: later_lexical_state,
                rank: state_rank(later_lexical_state),
            },
        ],
    });
    const handle = rsjs.query(key);
    assert(handle.data() === 'Z', `expected first completed SSR seed, got ${handle.data()}`);
    const notifications = [];
    handle.subscribe((value) => notifications.push(value));

    const winning_state = {
        key,
        data: 'A',
        data_updated_at: 40,
    };
    document_shim.append_page_query_seed({
        queries: [
            {
                key,
                state: winning_state,
                rank: state_rank(winning_state),
            },
        ],
    });
    rsjs.query(key);
    assert(handle.data() === 'A', `expected lexical SSR winner, got ${handle.data()}`);
    assert(handle.status() === 'success', `expected coherent success status, got ${handle.status()}`);
    assert(handle.error() === undefined, `expected winning seed to clear error, got ${handle.error()}`);
    assert(handle.fetch_count() === 1, `expected one dehydrated fetch, got ${handle.fetch_count()}`);
    assert(
        handle.data_updated_at() === 40,
        `expected winning timestamp 40, got ${handle.data_updated_at()}`,
    );
    assert(
        notifications.at(-1) === 'A',
        `expected lexical winner notification, got ${JSON.stringify(notifications)}`,
    );

    set_data(key, 'CLIENT');
    const notification_count_after_client_write = notifications.length;
    const still_smaller_state = {
        key,
        data: '0',
        data_updated_at: 1,
    };
    document_shim.append_page_query_seed({
        queries: [
            {
                key,
                state: still_smaller_state,
                rank: state_rank(still_smaller_state),
            },
        ],
    });
    rsjs.query(key);
    assert(handle.data() === 'CLIENT', `late SSR seed clobbered client data: ${handle.data()}`);
    assert(
        notifications.length === notification_count_after_client_write,
        'late SSR seed notified subscribers after client ownership',
    );

    const forward_key = ['ooo_seed_conflict_forward'];
    const forward_winner = {
        key: forward_key,
        data: 'A',
        data_updated_at: 11,
    };
    document_shim.append_page_query_seed({
        queries: [
            {
                key: forward_key,
                state: forward_winner,
                rank: state_rank(forward_winner),
            },
        ],
    });
    const forward_handle = rsjs.query(forward_key);
    const forward_notifications = [];
    forward_handle.subscribe((value) => forward_notifications.push(value));
    const forward_loser = {
        key: forward_key,
        data: 'Z',
        data_updated_at: 99,
    };
    document_shim.append_page_query_seed({
        queries: [
            {
                key: forward_key,
                state: forward_loser,
                rank: state_rank(forward_loser),
            },
        ],
    });
    rsjs.query(forward_key);
    assert(
        forward_handle.data() === 'A',
        `A→Z completion order changed the lexical winner: ${forward_handle.data()}`,
    );
    assert(
        forward_notifications.length === 0,
        `losing SSR seed should not notify subscribers: ${JSON.stringify(forward_notifications)}`,
    );

    // UTF-16 string ordering would put the emoji surrogate before U+E000,
    // while Rust's UTF-8 byte ordering puts U+E000 (0xee...) before the emoji
    // (0xf0...). The explicit rank must preserve the server's winner.
    const unicode_key = ['ooo_seed_conflict_unicode'];
    const emoji_state = { key: unicode_key, data: '😀' };
    document_shim.append_page_query_seed({
        queries: [
            {
                key: unicode_key,
                state: emoji_state,
                rank: state_rank(emoji_state),
            },
        ],
    });
    const unicode_handle = rsjs.query(unicode_key);
    const private_use_state = { key: unicode_key, data: '\uE000' };
    document_shim.append_page_query_seed({
        queries: [
            {
                key: unicode_key,
                state: private_use_state,
                rank: state_rank(private_use_state),
            },
        ],
    });
    rsjs.query(unicode_key);
    assert(
        unicode_handle.data() === '\uE000',
        `runtime did not preserve Rust UTF-8 ordering: ${JSON.stringify(unicode_handle.data())}`,
    );
}

// Round-3 review #1: subscribers attached during the stub phase must
// see the dehydrated seed when register_query upgrades the entry.
{
    const key = ['stub_then_register_unique'];
    const handle = rsjs.query(key);
    let seen = 'not_called';
    handle.subscribe((value) => {
        seen = value;
    });
    register_query({
        island: 'stub_then_register_island',
        query_id: 0,
        key,
        fetcher: '/api/noop',
        options: {},
    });
    assert(
        seen === 99,
        `expected stub-phase subscriber to see dehydrated seed (99); got ${JSON.stringify(seen)}`,
    );
}

// Per-instance query dehydration: two rendered instances of the same island
// should seed from their adjacent blobs instead of sharing the last global
// blob for that asset id.
{
    const island = 'multi_instance_query_island';
    const root_a = root_with_query_blob(island, {
        '0': { data: 'AAPL', data_updated_at: 1 },
    });
    const root_b = root_with_query_blob(island, {
        '0': { data: 'GOOG', data_updated_at: 1 },
    });
    const handle_a = register_query({
        island,
        query_id: 0,
        root: root_a,
        key: ['multi_instance_query', 'AAPL'],
        fetcher: '/api/stock/AAPL',
        options: {},
    });
    const handle_b = register_query({
        island,
        query_id: 0,
        root: root_b,
        key: ['multi_instance_query', 'GOOG'],
        fetcher: '/api/stock/GOOG',
        options: {},
    });
    assert(handle_a.data() === 'AAPL', `expected AAPL seed, got ${handle_a.data()}`);
    assert(handle_b.data() === 'GOOG', `expected GOOG seed, got ${handle_b.data()}`);
}

// Runtime-initialized queries can carry their effective key in the per-instance
// blob. This covers `query_with_initial(...)` keys that use server locals the
// static island JS cannot evaluate.
{
    const island = 'runtime_key_query_island';
    const key = ['runtime_key_query', 'AAPL'];
    const root = root_with_query_blob(island, {
        '0': { key },
    });
    const public_handle = rsjs.query(key);
    assert(
        public_handle.getSnapshot() === 'AAPL',
        'public handle should be pre-seeded from the page-level query table',
    );
    const registered = register_query({
        island,
        query_id: 0,
        root,
        key: null,
        fetcher: '/api/runtime-key-query/AAPL',
        options: {},
    });
    assert(registered.data() === 'AAPL', `expected runtime-key seed, got ${registered.data()}`);
    assert(
        registered.data_updated_at() === 9,
        `expected runtime-key page seed timestamp, got ${registered.data_updated_at()}`,
    );
    assert(public_handle.getSnapshot() === 'AAPL', 'public handle should share runtime-key entry');
}

// Round-4 review #1: a query that errored, then succeeded, must clear
// entry.error so error()/is_error() are mutually consistent. Also,
// fetch_count must increment on every resolution (success + error)
// per the .d.ts contract.
{
    let call = 0;
    globalThis.fetch = async () => {
        call += 1;
        if (call === 1) {
            throw new Error('boom');
        }
        return {
            ok: true,
            status: 200,
            async text() {
                return '{"ok":true}';
            },
        };
    };

    const key = ['err_then_success_unique'];
    const handle = rsjs.query(key);
    register_query({
        island: 'err_then_success_island',
        query_id: 0,
        key,
        fetcher: '/api/sometimes',
        options: {},
    });

    try {
        await handle.refetch();
    } catch (_) {
        // expected: first attempt fails
    }
    assert(handle.is_error() === true, 'expected is_error() === true after first failed refetch');
    assert(handle.error() instanceof Error, 'expected error() to expose the rejection');
    assert(
        handle.fetch_count() === 1,
        `expected fetch_count() === 1 after one failed fetch, got ${handle.fetch_count()}`,
    );

    await handle.refetch();
    assert(
        handle.is_error() === false,
        'expected is_error() === false after a successful refetch',
    );
    assert(
        handle.error() === undefined,
        `expected error() === undefined after successful refetch, got ${String(handle.error())}`,
    );
    assert(
        handle.fetch_count() === 2,
        `expected fetch_count() === 2 (success + error), got ${handle.fetch_count()}`,
    );
}

// Rich request descriptors: a query can POST a static non-JSON body
// with headers/credentials/timeout and decode a text response.
{
    let seen;
    globalThis.fetch = async (url, init) => {
        seen = { url, init };
        return {
            ok: true,
            status: 200,
            async text() {
                return 'accepted';
            },
        };
    };

    const key = ['rich_request_unique'];
    const handle = rsjs.query(key);
    register_query({
        island: 'rich_request_island',
        query_id: 0,
        key,
        fetcher:
            '{"url":"/api/rich","method":"POST","credentials":"include",' +
            '"headers":[["Accept","text/plain"]],"body":{"kind":"text","value":"ping"},' +
            '"response":"text","timeout_ms":1000}',
        options: {},
    });

    const data = await handle.refetch();
    assert(data === 'accepted', `expected text response, got ${JSON.stringify(data)}`);
    assert(seen.url === '/api/rich', `expected rich URL, got ${seen.url}`);
    assert(seen.init.method === 'POST', `expected POST, got ${seen.init.method}`);
    assert(
        seen.init.credentials === 'include',
        `expected include credentials, got ${seen.init.credentials}`,
    );
    assert(seen.init.body === 'ping', `expected static text body, got ${seen.init.body}`);
    assert(
        Array.isArray(seen.init.headers),
        `expected header pairs to stay as an array, got ${JSON.stringify(seen.init.headers)}`,
    );
    assert(
        seen.init.headers.some(
            ([name, value]) => name === 'Accept' && value === 'text/plain',
        ),
        `expected Accept header pair, got ${JSON.stringify(seen.init.headers)}`,
    );
}

// Download progress: when opted in, the query runtime reads the
// response body through a stream reader and publishes loaded/total/
// percent metadata on the query handle.
{
    const encoder = new TextEncoder();
    const chunks = [
        encoder.encode('{"ok"'),
        encoder.encode(':true,'),
        encoder.encode('"n":1}'),
    ];
    const total = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
    globalThis.fetch = async () => ({
        ok: true,
        status: 200,
        headers: {
            get(name) {
                return String(name).toLowerCase() === 'content-length'
                    ? String(total)
                    : null;
            },
        },
        body: new ReadableStream({
            start(controller) {
                for (const chunk of chunks) controller.enqueue(chunk);
                controller.close();
            },
        }),
    });

    const key = ['download_progress_unique'];
    const handle = rsjs.query(key);
    const seen = [];
    handle.subscribe(() => {
        seen.push([
            handle.download_loaded(),
            handle.download_total(),
            handle.download_percent(),
        ]);
    });
    register_query({
        island: 'download_progress_island',
        query_id: 0,
        key,
        fetcher: '/api/progress',
        options: { download_progress: true },
    });

    const data = await handle.refetch();
    assert(data.ok === true && data.n === 1, `expected streamed JSON body, got ${data}`);
    assert(
        handle.download_loaded() === total,
        `expected final loaded bytes ${total}, got ${handle.download_loaded()}`,
    );
    assert(
        handle.download_total() === total,
        `expected content-length total ${total}, got ${handle.download_total()}`,
    );
    assert(
        handle.download_percent() === 1,
        `expected final percent 1, got ${handle.download_percent()}`,
    );
    assert(
        seen.some(([loaded]) => loaded > 0 && loaded < total),
        `expected at least one intermediate progress notification, got ${JSON.stringify(seen)}`,
    );
}

// Streaming text: when opted in for a text response, query.data()
// updates with the accumulated body as each chunk arrives.
{
    const encoder = new TextEncoder();
    const chunks = [
        encoder.encode('hel'),
        encoder.encode('lo '),
        encoder.encode('stream'),
    ];
    globalThis.fetch = async () => ({
        ok: true,
        status: 200,
        headers: {
            get() {
                return null;
            },
        },
        body: new ReadableStream({
            start(controller) {
                for (const chunk of chunks) controller.enqueue(chunk);
                controller.close();
            },
        }),
    });

    const key = ['stream_text_unique'];
    const handle = rsjs.query(key);
    const seen = [];
    handle.subscribe(() => {
        seen.push(handle.data());
    });
    register_query({
        island: 'stream_text_island',
        query_id: 0,
        key,
        fetcher: '{"url":"/api/stream","response":"text"}',
        options: { stream_text: true },
    });

    const data = await handle.refetch();
    assert(data === 'hello stream', `expected streamed text body, got ${data}`);
    assert(
        seen.includes('hel') && seen.includes('hello ') && seen.includes('hello stream'),
        `expected progressive text snapshots, got ${JSON.stringify(seen)}`,
    );
}

// Static server-emitted mutation metadata can supply the effective
// key, runner, invalidation list, and policy for a generated
// register_mutation call whose JS fields are placeholders.
{
    const island = 'mutation_metadata_island';
    const mutation_key = ['metadata_mutation', 'AAPL'];
    const query_key = ['metadata_query', 'AAPL'];
    const root = root_with_blobs(island, {
        mutations: {
            '0': {
                key: mutation_key,
                runner: { url: '/api/mutation-metadata', method: 'PATCH', response: 'json' },
                invalidates: [query_key],
                options: { retry_count: 0, on_success_invalidate_strategy: 'reset' },
            },
        },
    });

    const query_handle = rsjs.query(query_key);
    set_data(query_key, { seeded: true });
    assert(query_handle.data().seeded === true, 'metadata test query should start seeded');

    const public_mutation = rsjs.mutation(mutation_key);
    const mutation_states = [];
    public_mutation.subscribe((state) => mutation_states.push(state.status));

    let seen_request;
    globalThis.fetch = async (url, init) => {
        seen_request = { url, init };
        return {
            ok: true,
            status: 200,
            async text() {
                return '{"saved":true}';
            },
        };
    };

    const m = register_mutation({
        island,
        mutation_id: 0,
        root,
        key: null,
        runner: null,
        invalidates: [],
        options: {},
    });
    const result = await m.run({ qty: 2 });
    assert(result.saved === true, `expected metadata runner result, got ${JSON.stringify(result)}`);
    assert(
        seen_request.url === '/api/mutation-metadata',
        `expected metadata URL, got ${seen_request && seen_request.url}`,
    );
    assert(
        seen_request.init.method === 'PATCH',
        `expected metadata PATCH method, got ${seen_request.init.method}`,
    );
    assert(
        seen_request.init.body === '{"qty":2}',
        `expected metadata mutation body, got ${seen_request.init.body}`,
    );
    assert(
        public_mutation.status() === 'success',
        'public mutation handle should share metadata-selected key',
    );
    assert(
        mutation_states.includes('pending') && mutation_states.includes('success'),
        `metadata mutation subscribers should see pending and success, got ${mutation_states}`,
    );
    assert(query_handle.status() === 'idle', 'metadata invalidation reset should idle the query');
    assert(query_handle.data() === undefined, 'metadata invalidation reset should clear query data');
}

// Partial metadata must not erase dynamic JS fallback fields. This
// matches signal-backed mutation keys/invalidations, where the server
// blob intentionally omits fields it cannot encode.
{
    const island = 'partial_mutation_metadata_island';
    const mutation_key = ['partial_metadata_mutation', 'AAPL'];
    const query_key = ['partial_metadata_query', 'AAPL'];
    const root = root_with_blobs(island, {
        mutations: {
            '0': {
                runner: { url: '/api/partial-mutation-metadata', method: 'POST', response: 'json' },
                options: { retry_count: 0, on_success_invalidate_strategy: 'reset' },
            },
        },
    });

    const query_handle = rsjs.query(query_key);
    set_data(query_key, { seeded: true });
    let seen_url;
    globalThis.fetch = async (url) => {
        seen_url = url;
        return {
            ok: true,
            status: 200,
            async text() {
                return '{"ok":true}';
            },
        };
    };

    const m = register_mutation({
        island,
        mutation_id: 0,
        root,
        key: mutation_key,
        runner: null,
        invalidates: [query_key],
        options: {},
    });
    await m.run({ qty: 1 });
    assert(
        seen_url === '/api/partial-mutation-metadata',
        `expected partial metadata URL, got ${seen_url}`,
    );
    assert(
        rsjs.mutation(mutation_key).status() === 'success',
        'partial metadata should keep generated JS key fallback',
    );
    assert(
        query_handle.data() === undefined && query_handle.status() === 'idle',
        'partial metadata should keep generated JS invalidates fallback',
    );
}

// Round-4 review #2: mutation.run takes a single argument; the value
// is encoded verbatim as the request body. Calling `m.run([1,2,3])`
// must reach the server as `[1,2,3]` (not `[[1,2,3]]` or `[1,2,3]`
// shared with the splat case `m.run(1,2,3)`, which is now illegal).
{

    const seen_bodies = [];
    globalThis.fetch = async (_url, init) => {
        seen_bodies.push(init && init.body);
        return {
            ok: true,
            status: 200,
            async text() {
                return '{"ok":true}';
            },
            async json() {
                return { ok: true };
            },
        };
    };

    const m_obj = register_mutation({
        key: ['shape_check_obj_unique'],
        runner: { url: '/api/shape', method: 'POST' },
        invalidates: [],
        options: {},
    });
    await m_obj.run({ items: [1, 2, 3] });
    assert(
        seen_bodies[0] === '{"items":[1,2,3]}',
        `expected object arg preserved verbatim; got body ${JSON.stringify(seen_bodies[0])}`,
    );

    const m_arr = register_mutation({
        key: ['shape_check_arr_unique'],
        runner: { url: '/api/shape', method: 'POST' },
        invalidates: [],
        options: {},
    });
    await m_arr.run([1, 2, 3]);
    assert(
        seen_bodies[1] === '[1,2,3]',
        `expected array arg preserved verbatim; got body ${JSON.stringify(seen_bodies[1])}`,
    );
}

console.log('rsjs-runtime-query.smoke: ok');
