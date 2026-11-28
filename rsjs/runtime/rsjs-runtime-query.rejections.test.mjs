// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for the rejection contract of query and mutation requests. A failed
// request is recorded in query or mutation state, so a request promise that
// nobody awaits must not raise an unhandled rejection. A caller that awaits or
// chains the promise must still receive the rejection. Run with:
//
//     node rsjs/runtime/rsjs-runtime-query.rejections.test.mjs

import {
    dispose_page_query_state,
    register_mutation,
    register_query,
    rsjs,
    set_data,
} from './rsjs-runtime-query.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

// Node reports a rejection as unhandled once the microtask queue drains with
// no handler attached, which is the same rule a browser applies before it
// fires `unhandledrejection` and logs `Uncaught (in promise)`.
const unhandled = [];
process.on('unhandledRejection', (reason) => {
    unhandled.push(reason);
});

const real_set_timeout = globalThis.setTimeout;
const previous_fetch = globalThis.fetch;
const previous_set_interval = globalThis.setInterval;
const previous_clear_interval = globalThis.clearInterval;

function turn() {
    return new Promise((resolve) => real_set_timeout(resolve, 0));
}

/** Wait for `done()`, then give Node one more turn to report rejections. */
async function settle(done) {
    for (let attempt = 0; attempt < 400 && !done(); attempt += 1) {
        await turn();
    }
    await turn();
    await turn();
}

function take_unhandled() {
    return unhandled.splice(0, unhandled.length);
}

function describe(reasons) {
    return reasons.map((reason) => reason?.message ?? String(reason)).join(' | ');
}

function json_response(status, body) {
    const text = body === undefined ? '' : JSON.stringify(body);
    return {
        ok: status >= 200 && status < 300,
        status,
        async text() {
            return text;
        },
        async arrayBuffer() {
            return new TextEncoder().encode(text).buffer;
        },
    };
}

// One route table stands in for the server. A route returns a response, or a
// promise that never resolves and rejects with an `AbortError` on abort, the
// way a browser fetch behaves when its signal fires.
const routes = new Map();
const requests = [];

function install_fetch() {
    globalThis.fetch = (url, init = {}) => {
        requests.push({ url, method: init.method ?? 'GET' });
        const route = routes.get(url);
        if (route === undefined) {
            return Promise.resolve(json_response(404));
        }
        if (route === 'hang') {
            return new Promise((_resolve, reject) => {
                const abort = () => {
                    const error = new Error('signal is aborted without reason');
                    error.name = 'AbortError';
                    reject(error);
                };
                if (init.signal?.aborted) {
                    abort();
                } else {
                    init.signal?.addEventListener('abort', abort, { once: true });
                }
            });
        }
        return Promise.resolve(route());
    };
}

function install_intervals() {
    const intervals = new Set();
    globalThis.setInterval = (callback, delay) => {
        const handle = { callback, delay };
        intervals.add(handle);
        return handle;
    };
    globalThis.clearInterval = (handle) => {
        intervals.delete(handle);
    };
    return intervals;
}

try {
    install_fetch();
    // Retry backoff starts at 250 ms. Shorten every runtime timer so the retry
    // and timeout cases settle quickly without changing their order.
    globalThis.setTimeout = (callback, delay, ...rest) =>
        real_set_timeout(callback, Math.min(Number(delay) || 0, 2), ...rest);

    // A generated event handler lowers `query.refetch()` to the bare
    // statement `handle.refetch();`. The failed response is recorded on the
    // handle, and nothing else may surface.
    {
        routes.set('/api/rejections/flaky', () => json_response(503));
        const flaky = register_query({
            island: 'rejections',
            query_id: 0,
            key: ['rejections', 'flaky'],
            fetcher: '/api/rejections/flaky',
            options: {},
        });

        flaky.refetch();
        await settle(() => flaky.status() === 'error');

        assert(flaky.status() === 'error', 'a 503 should leave the query in the error state');
        assert(
            flaky.error()?.message === 'rsjs query fetch failed: 503',
            'a 503 should be recorded as the query error',
        );
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `a failed refetch nobody awaits should not be an unhandled rejection; got: ${describe(leaked)}`,
        );
    }

    // `timeout_ms` aborts the request from inside the runtime.
    {
        routes.set('/api/rejections/slow', 'hang');
        const slow = register_query({
            island: 'rejections',
            query_id: 1,
            key: ['rejections', 'slow'],
            fetcher: { url: '/api/rejections/slow', timeout_ms: 250 },
            options: {},
        });

        slow.refetch();
        await settle(() => slow.status() === 'error');

        assert(slow.status() === 'error', 'a timeout should leave the query in the error state');
        assert(
            slow.error()?.name === 'AbortError',
            'a timeout should record the abort as the query error',
        );
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `a timed-out refetch nobody awaits should not be an unhandled rejection; got: ${describe(leaked)}`,
        );
    }

    // A generated event handler lowers `mutation.run(arg)` to the bare
    // statement `handle.run(arg);`.
    {
        routes.set('/api/rejections/fail-mut', () => json_response(422));
        const fail_mut = register_mutation({
            island: 'rejections',
            mutation_id: 0,
            key: ['rejections', 'fail-mut'],
            runner: { url: '/api/rejections/fail-mut', method: 'POST', response: 'empty' },
            invalidates: [],
            options: { retry_count: 0 },
        });

        fail_mut.run('bad');
        await settle(() => fail_mut.status() === 'error');

        assert(fail_mut.status() === 'error', 'a 422 should leave the mutation in the error state');
        assert(
            fail_mut.error()?.message === 'rsjs mutation failed: 422',
            'a 422 should be recorded as the mutation error',
        );
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `a failed mutation nobody awaits should not be an unhandled rejection; got: ${describe(leaked)}`,
        );

        rsjs.__run_mutation(fail_mut, 'bad again');
        await settle(() => fail_mut.status() === 'error');
        const leaked_hook = take_unhandled();
        assert(
            leaked_hook.length === 0,
            `rsjs.__run_mutation should follow the same contract; got: ${describe(leaked_hook)}`,
        );
    }

    // Retries run inside one request. Exhausting the budget records one error.
    {
        const before = requests.length;
        routes.set('/api/rejections/retry', () => json_response(503));
        const retried = register_query({
            island: 'rejections',
            query_id: 2,
            key: ['rejections', 'retry'],
            fetcher: '/api/rejections/retry',
            options: { retry_count: 2 },
        });

        retried.refetch();
        await settle(() => retried.status() === 'error');

        assert(
            requests.length - before === 3,
            `retry_count 2 should send three requests; sent ${requests.length - before}`,
        );
        assert(retried.status() === 'error', 'an exhausted retry budget should record the error');
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `an exhausted retry budget should not be an unhandled rejection; got: ${describe(leaked)}`,
        );
    }

    // Fetches the runtime starts for itself: the first load when a subscriber
    // attaches to an entry without data, and an interval refetch.
    {
        const intervals = install_intervals();
        routes.set('/api/rejections/policy', () => json_response(503));
        const polled = register_query({
            island: 'rejections',
            query_id: 3,
            key: ['rejections', 'policy'],
            fetcher: '/api/rejections/policy',
            options: { interval_ms: 1000 },
        });

        const off = polled.subscribe(() => {});
        await settle(() => polled.status() === 'error');
        assert(polled.fetch_count() === 1, 'subscribing without data should start the first load');
        const leaked_initial = take_unhandled();
        assert(
            leaked_initial.length === 0,
            `a failed first load should not be an unhandled rejection; got: ${describe(leaked_initial)}`,
        );

        assert(intervals.size === 1, 'the subscribed query should own one interval');
        for (const interval of intervals) interval.callback();
        await settle(() => polled.fetch_count() === 2);
        assert(polled.fetch_count() === 2, 'the interval should start a second request');
        const leaked_interval = take_unhandled();
        assert(
            leaked_interval.length === 0,
            `a failed interval refetch should not be an unhandled rejection; got: ${describe(leaked_interval)}`,
        );
        off();
    }

    // A successful mutation invalidates its target keys. The refetch that
    // follows is the runtime's own, even when the mutation itself was awaited.
    {
        let list_status = 200;
        routes.set('/api/rejections/list', () => json_response(list_status, { items: [] }));
        routes.set('/api/rejections/create', () => json_response(204));
        const list = register_query({
            island: 'rejections',
            query_id: 4,
            key: ['rejections', 'list'],
            fetcher: '/api/rejections/list',
            options: {},
        });
        const off = list.subscribe(() => {});
        await settle(() => list.status() === 'success');

        const create = register_mutation({
            island: 'rejections',
            mutation_id: 1,
            key: ['rejections', 'create'],
            runner: { url: '/api/rejections/create', method: 'POST', response: 'empty' },
            invalidates: [['rejections', 'list']],
            options: { retry_count: 0 },
        });
        list_status = 503;
        const posts_before = requests.filter((request) => request.method === 'POST').length;
        await create.run('new item');
        await settle(() => list.status() === 'error');

        assert(create.status() === 'success', 'the mutation itself should succeed');
        assert(list.status() === 'error', 'the invalidation refetch should record its failure');
        assert(
            requests.filter((request) => request.method === 'POST').length - posts_before === 1,
            'one mutation run should send exactly one request',
        );
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `a failed invalidation refetch should not be an unhandled rejection; got: ${describe(leaked)}`,
        );
        off();
    }

    // A caller that awaits the promise handles the failure at the call site.
    {
        const flaky = rsjs.query(['rejections', 'flaky']);
        let refetch_error;
        try {
            await flaky.refetch();
        } catch (error) {
            refetch_error = error;
        }
        assert(
            refetch_error?.message === 'rsjs query fetch failed: 503',
            'an awaited refetch should reject with the request failure',
        );

        const fail_mut = rsjs.mutation(['rejections', 'fail-mut']);
        let run_error;
        try {
            await fail_mut.run('bad');
        } catch (error) {
            run_error = error;
        }
        assert(
            run_error?.message === 'rsjs mutation failed: 422',
            'an awaited mutation should reject with the request failure',
        );

        let chained_error;
        await fail_mut.run('bad').then(
            () => {},
            (error) => {
                chained_error = error;
            },
        );
        assert(
            chained_error?.message === 'rsjs mutation failed: 422',
            'a chained rejection handler should receive the request failure',
        );
        await settle(() => true);
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `handled rejections should not be reported; got: ${describe(leaked)}`,
        );
    }

    // Chaining without a rejection handler creates a new promise that the
    // caller owns. Its rejection must still be reported: the runtime does not
    // swallow failures on the caller's behalf.
    {
        const flaky = rsjs.query(['rejections', 'flaky']);
        flaky.refetch().then(() => {});
        await settle(() => flaky.status() === 'error');
        const leaked_query = take_unhandled();
        assert(
            leaked_query.length === 1 &&
                leaked_query[0]?.message === 'rsjs query fetch failed: 503',
            `a refetch chained without a rejection handler should still be reported once; got: ${describe(leaked_query)}`,
        );

        const fail_mut = rsjs.mutation(['rejections', 'fail-mut']);
        fail_mut.run('bad').then(() => {});
        await settle(() => fail_mut.status() === 'error');
        const leaked_mutation = take_unhandled();
        assert(
            leaked_mutation.length === 1 &&
                leaked_mutation[0]?.message === 'rsjs mutation failed: 422',
            `a mutation chained without a rejection handler should still be reported once; got: ${describe(leaked_mutation)}`,
        );
    }

    // A request that fails before anything is recorded is not covered by the
    // contract. A refetch with no fetch specification never starts a request,
    // so the handle state cannot report it and the rejection must stay loud.
    {
        const stub = rsjs.query(['rejections', 'never-registered']);
        stub.refetch();
        await settle(() => true);
        const leaked = take_unhandled();
        assert(
            leaked.length === 1 && leaked[0]?.message.includes('rsjs/query-unregistered'),
            `a refetch that records nothing should still be reported; got: ${describe(leaked)}`,
        );
        assert(stub.status() === 'idle', 'an unregistered refetch should leave the entry idle');
    }

    // An optimistic update that throws fails the run like any other error: the
    // mutation records it and stays usable.
    {
        let fail_optimistic = true;
        const list = rsjs.query(['rejections', 'list']);
        routes.set('/api/rejections/list', () => json_response(503));
        await list.refetch().catch(() => {});
        const previous_error = list.error();
        const previous_timestamp = list.data_updated_at();
        routes.set('/api/rejections/optimistic', () => json_response(200, { ok: true }));
        const optimistic = register_mutation({
            island: 'rejections',
            mutation_id: 2,
            key: ['rejections', 'optimistic'],
            runner: { url: '/api/rejections/optimistic', method: 'POST' },
            invalidates: [['rejections', 'list']],
            options: {
                retry_count: 0,
                optimistic_update: () => {
                    if (fail_optimistic) throw new Error('optimistic update failed');
                    return { items: ['pending'] };
                },
            },
        });

        optimistic.run('first');
        await settle(() => optimistic.status() !== 'pending');
        assert(
            optimistic.status() === 'error' &&
                optimistic.error()?.message === 'optimistic update failed',
            `a throwing optimistic update should be recorded as the mutation error; status is ${optimistic.status()}`,
        );
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `a recorded optimistic failure should not be an unhandled rejection; got: ${describe(leaked)}`,
        );

        fail_optimistic = false;
        assert(list.status() === 'error' && list.error() === previous_error &&
            list.data_updated_at() === previous_timestamp, 'a throwing callback must leave the query untouched');
        routes.set('/api/rejections/optimistic', () => json_response(422));
        await optimistic.run('rejected').catch(() => {});
        assert(list.status() === 'error' && list.error() === previous_error &&
            list.data_updated_at() === previous_timestamp, 'a failed optimistic write must restore query state');
        routes.set('/api/rejections/optimistic', () => json_response(200, { ok: true }));
        routes.set('/api/rejections/list', () => json_response(200, { items: [] }));
        const result = await optimistic.run('second');
        assert(
            result?.ok === true && optimistic.status() === 'success',
            'the mutation should run again after a failed optimistic update',
        );
        await settle(() => true);
        take_unhandled();
    }

    // Overlapping writes roll back to the last durable value, even when an
    // earlier mutation fails while a later optimistic value is still visible.
    {
        const key = ['rejections', 'list'];
        const list = rsjs.query(key);
        set_data(key, 'original');
        function pending_write(id, value, target = key) {
            let finish;
            const response = new Promise((resolve) => { finish = resolve; });
            const url = '/api/rejections/write-' + id;
            routes.set(url, () => response);
            const mutation = register_mutation({
                island: 'rejections', mutation_id: id, key: ['write', id],
                runner: { url, method: 'POST' }, invalidates: [target],
                options: { retry_count: 0, optimistic_update: () => value },
            });
            const result = mutation.run().catch(() => {});
            return async (status = 422) => { finish(json_response(status)); await result; };
        }
        const fail_first = pending_write(20, 'first');
        const fail_second = pending_write(21, 'second');
        await fail_first();
        assert(list.data() === 'second', 'an earlier rejection must preserve a newer optimistic write');
        await fail_second();
        assert(list.data() === 'original', 'two rejections must restore the original value');

        const fail_again = pending_write(22, 'pending');
        set_data(key, 'updated independently');
        await fail_again();
        assert(list.data() === 'updated independently', 'rollback must preserve a newer cache write');

        const commit_first = pending_write(23, 'accepted');
        const reject_second = pending_write(24, 'rejected');
        await commit_first(204);
        assert(list.data() === 'rejected' && list.is_stale(), 'invalidation should leave the later optimistic value pending');
        await reject_second();
        assert(list.data() === 'accepted' && list.is_stale(), 'an unsubscribed query must roll back and remain stale');

        const reject_after_fetch = pending_write(25, 'temporary');
        routes.set('/api/rejections/list', () => json_response(503));
        await list.refetch().catch(() => {});
        const fetch_error = list.error();
        await reject_after_fetch();
        assert(list.data() === 'accepted' && list.status() === 'error' && list.error() === fetch_error,
            'a failed refetch must keep its error and roll back the rejected data');

        const reject_during_fetch = pending_write(26, 'temporary');
        let finish_fetch;
        routes.set('/api/rejections/list', () => new Promise((resolve) => { finish_fetch = resolve; }));
        const fetching = list.refetch();
        await reject_during_fetch();
        assert(list.data() === 'accepted' && list.status() === 'refetching', 'rollback must preserve an ongoing refetch');
        finish_fetch(json_response(200, 'fresh'));
        await fetching;
        assert(list.data() === 'fresh', 'the refetch should still publish its result');

        const fresh_key = ['rejections', 'created-by-mutation'];
        const discard_initial = pending_write(27, 'initial guess', fresh_key);
        const registered = register_query({
            island: 'rejections', query_id: 27, key: fresh_key, fetcher: '/api/rejections/flaky', options: {},
        });
        await registered.refetch().catch(() => {});
        await discard_initial();
        assert(registered.data() === undefined && registered.status() === 'error' && registered.fetch_count() === 1,
            'a query registered during the mutation must retain only its real fetch count');
    }

    // Page teardown aborts requests that are still in flight. The abort is the
    // runtime's own decision, so it must not surface either.
    {
        routes.set('/api/rejections/teardown', 'hang');
        const leaving = register_query({
            island: 'rejections',
            query_id: 5,
            key: ['rejections', 'teardown'],
            fetcher: '/api/rejections/teardown',
            options: {},
        });

        const request = leaving.refetch();
        await turn();
        dispose_page_query_state();
        await settle(() => true);
        const leaked = take_unhandled();
        assert(
            leaked.length === 0,
            `a request aborted by page teardown should not be an unhandled rejection; got: ${describe(leaked)}`,
        );

        let teardown_error;
        try {
            await request;
        } catch (error) {
            teardown_error = error;
        }
        assert(
            teardown_error?.name === 'AbortError',
            'a caller still holding the promise should observe the teardown abort',
        );
    }
} finally {
    globalThis.fetch = previous_fetch;
    globalThis.setTimeout = real_set_timeout;
    globalThis.setInterval = previous_set_interval;
    globalThis.clearInterval = previous_clear_interval;
}

if (failed > 0) {
    process.exit(1);
}

console.log('All query rejection tests passed.');
