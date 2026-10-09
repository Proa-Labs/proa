// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for observer-local query scheduling and cleanup. Run with:
//
//     node rsjs/runtime/rsjs-runtime-query.lifecycle.test.mjs

import {
    dispose_page_query_state,
    dispose_query,
    get_entry,
    register_query,
    rebind_query,
    rsjs,
    set_data,
} from './rsjs-runtime-query.js';
import { memo, signal } from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

function install_window() {
    const listeners = new Map();
    const win = {
        addEventListener(name, handler) {
            if (!listeners.has(name)) listeners.set(name, []);
            listeners.get(name).push(handler);
        },
        removeEventListener(name, handler) {
            const current = listeners.get(name) ?? [];
            listeners.set(
                name,
                current.filter((entry) => entry !== handler),
            );
        },
        listener_count(name) {
            return (listeners.get(name) ?? []).length;
        },
        dispatch(name) {
            for (const handler of [...(listeners.get(name) ?? [])]) {
                handler({ type: name });
            }
        },
    };
    globalThis.window = win;
    return win;
}

function install_timers() {
    const intervals = new Set();
    let next_id = 0;
    globalThis.setInterval = (callback, delay) => {
        const handle = { id: next_id, callback, delay };
        next_id += 1;
        intervals.add(handle);
        return handle;
    };
    globalThis.clearInterval = (handle) => {
        intervals.delete(handle);
    };
    return intervals;
}

const previous_window = globalThis.window;
const previous_set_interval = globalThis.setInterval;
const previous_clear_interval = globalThis.clearInterval;
const previous_fetch = globalThis.fetch;
const previous_date_now = Date.now;

try {
    globalThis.fetch = async () => ({
        ok: true,
        status: 200,
        async text() {
            return JSON.stringify({ ok: true });
        },
    });

    {
        const win = install_window();
        const intervals = install_timers();
        const handle = register_query({
            island: 'query-lifecycle',
            query_id: 0,
            key: ['query-lifecycle', 'direct'],
            fetcher: '/api/query-lifecycle/direct',
            options: {
                interval_ms: 1000,
                refetch_on_focus: true,
                refetch_on_reconnect: true,
            },
        });

        assert(intervals.size === 0, 'query policy should wait for subscribers before installing interval');
        assert(win.listener_count('focus') === 0, 'query policy should wait for subscribers before installing focus listener');
        assert(win.listener_count('online') === 0, 'query policy should wait for subscribers before installing reconnect listener');

        const off_a = handle.subscribe(() => {});
        const off_b = handle.subscribe(() => {});
        assert(intervals.size === 1, 'query subscriber should install one interval');
        assert(win.listener_count('focus') === 1, 'query subscriber should install one focus listener');
        assert(win.listener_count('online') === 1, 'query subscriber should install one reconnect listener');

        off_a();
        assert(intervals.size === 1, 'one observer should keep its policy while it still has a subscriber');
        assert(win.listener_count('focus') === 1, 'one observer should keep its focus listener while subscribed');

        off_b();
        off_b();
        assert(intervals.size === 0, 'query unsubscribe should clear the interval once');
        assert(win.listener_count('focus') === 0, 'query unsubscribe should remove the focus listener once');
        assert(win.listener_count('online') === 0, 'query unsubscribe should remove the reconnect listener once');

        const off_again = handle.subscribe(() => {});
        assert(intervals.size === 1, 'query resubscribe should reinstall interval');
        assert(win.listener_count('focus') === 1, 'query resubscribe should reinstall focus listener');
        assert(win.listener_count('online') === 1, 'query resubscribe should reinstall reconnect listener');
        off_again();
    }

    // Component-instance disposal removes the complete registration observer
    // while retaining the page-scoped cache entry. Repeated mount/unmount
    // cycles must not accumulate dormant observers, subscriptions, timers, or
    // global policy listeners.
    {
        const win = install_window();
        const intervals = install_timers();
        const key = ['query-lifecycle', 'instance-dispose'];
        set_data(key, { retained: true });

        for (let iteration = 0; iteration < 4; iteration += 1) {
            const handle = register_query({
                island: `query-instance-dispose-${iteration}`,
                query_id: 0,
                key,
                fetcher: '/api/query-lifecycle/instance-dispose',
                options: {
                    interval_ms: 1000,
                    stale_time_ms: 1_000_000,
                    refetch_on_focus: true,
                    refetch_on_reconnect: true,
                },
            });
            const entry = get_entry(key);
            assert(
                entry.observers.size === 1,
                `mount ${iteration} should own exactly one query observer`,
            );

            const off_first = handle.subscribe(() => {});
            const off_second = handle.subscribe(() => {});
            assert(
                entry.active_observers.size === 1 && entry.subscribers.size === 2,
                `mount ${iteration} should attach one active observer with both subscriptions`,
            );
            assert(intervals.size === 1, `mount ${iteration} should own one interval`);
            assert(
                win.listener_count('focus') === 1 &&
                    win.listener_count('online') === 1,
                `mount ${iteration} should own one focus and reconnect listener`,
            );

            off_first();
            assert(
                entry.active_observers.size === 1 && entry.subscribers.size === 1,
                `mount ${iteration} should remain active until disposal clears its final subscription`,
            );
            assert(
                dispose_query(handle) === true,
                `mount ${iteration} should dispose its observer exactly once`,
            );
            assert(
                entry.observers.size === 0 &&
                    entry.active_observers.size === 0 &&
                    entry.subscribers.size === 0,
                `mount ${iteration} disposal should detach all observer state`,
            );
            assert(intervals.size === 0, `mount ${iteration} disposal should clear its interval`);
            assert(
                win.listener_count('focus') === 0 &&
                    win.listener_count('online') === 0,
                `mount ${iteration} disposal should clear its policy listeners`,
            );

            // A previously returned unsubscribe and the generated disposer may
            // race during subtree teardown. Both paths must remain harmless.
            off_second();
            assert(
                dispose_query(handle) === false,
                `mount ${iteration} repeated disposal should be idempotent`,
            );

            let subscribe_error;
            try {
                handle.subscribe(() => {});
            } catch (error) {
                subscribe_error = error;
            }
            assert(
                subscribe_error?.message.includes('query-disposed'),
                `mount ${iteration} disposed handle should reject subscriptions`,
            );

            let rebind_error;
            try {
                rebind_query(handle, {
                    island: `query-instance-dispose-${iteration}`,
                    query_id: 0,
                    key,
                    fetcher: '/api/query-lifecycle/instance-dispose',
                    options: { stale_time_ms: 1_000_000 },
                });
            } catch (error) {
                rebind_error = error;
            }
            assert(
                rebind_error?.message.includes('query-rebind-handle'),
                `mount ${iteration} disposed handle should reject rebinding`,
            );
        }

        assert(
            get_entry(key)?.data?.retained === true,
            'instance disposal should retain cache data until page teardown',
        );
    }

    // A live-key observer belongs only to its current entry. Disposal after a
    // rebind must detach the moved subscriptions and policy from that entry,
    // without evicting either cached identity.
    {
        const win = install_window();
        const intervals = install_timers();
        const old_key = ['query-lifecycle', 'dispose-rebind', 'old'];
        const current_key = ['query-lifecycle', 'dispose-rebind', 'current'];
        set_data(old_key, { identity: 'old' });
        set_data(current_key, { identity: 'current' });

        const registration = (key) => ({
            island: 'query-dispose-rebind',
            query_id: 0,
            key,
            live_key: true,
            fetcher: '/api/query-lifecycle/dispose-rebind',
            options: {
                interval_ms: 500,
                stale_time_ms: 1_000_000,
                refetch_on_focus: true,
            },
        });
        const handle = register_query(registration(old_key));
        const off = handle.subscribe(() => {});
        const old_entry = get_entry(old_key);
        assert(
            old_entry.observers.size === 1 &&
                old_entry.active_observers.size === 1 &&
                old_entry.subscribers.size === 1,
            'the initial live-key entry should own the observer and subscription',
        );

        assert(
            rebind_query(handle, registration(current_key)) === true,
            'the live-key observer should move to its current identity',
        );
        const current_entry = get_entry(current_key);
        assert(
            old_entry.observers.size === 0 &&
                old_entry.active_observers.size === 0 &&
                old_entry.subscribers.size === 0,
            'rebind should detach the observer completely from its old entry',
        );
        assert(
            current_entry.observers.size === 1 &&
                current_entry.active_observers.size === 1 &&
                current_entry.subscribers.size === 1,
            'rebind should attach the observer and subscription to its current entry',
        );
        assert(intervals.size === 1, 'the rebound current entry should own one interval');
        assert(win.listener_count('focus') === 1, 'the rebound current entry should own one focus listener');

        assert(dispose_query(handle) === true, 'disposal after rebind should succeed once');
        assert(
            current_entry.observers.size === 0 &&
                current_entry.active_observers.size === 0 &&
                current_entry.subscribers.size === 0,
            'disposal after rebind should detach the current entry',
        );
        assert(intervals.size === 0, 'disposal after rebind should clear the current interval');
        assert(win.listener_count('focus') === 0, 'disposal after rebind should clear the current listener');
        assert(
            old_entry.data?.identity === 'old' &&
                current_entry.data?.identity === 'current',
            'disposal after rebind should retain both cache identities',
        );
        off();
        assert(dispose_query(handle) === false, 'rebound handle disposal should remain idempotent');
    }

    // Same-key registrations retain independent observer policy while one
    // entry scheduler combines the minimum interval and trigger union.
    // Removing one observer must recompute rather than tear down the other.
    {
        const win = install_window();
        const intervals = install_timers();
        const key = ['query-lifecycle', 'observer-local'];
        const first = register_query({
            island: 'query-lifecycle-first',
            query_id: 0,
            key,
            fetcher: '/api/query-lifecycle/observer-local',
            options: {
                interval_ms: 100,
                stale_time_ms: 500,
                refetch_on_focus: true,
            },
        });
        const diagnostics = [];
        const previous_warn = console.warn;
        const previous_error = console.error;
        let second;
        try {
            console.warn = (...args) => diagnostics.push(['warn', ...args]);
            console.error = (...args) => diagnostics.push(['error', ...args]);
            second = register_query({
                island: 'query-lifecycle-second',
                query_id: 0,
                key,
                // Equivalent to the URL shorthand above after normalization.
                fetcher: { url: '/api/query-lifecycle/observer-local' },
                options: {
                    interval_ms: 250,
                    stale_time_ms: 2_000,
                    refetch_on_reconnect: true,
                    retry_count: 0,
                    download_progress: false,
                    stream_text: false,
                },
            });
        } finally {
            console.warn = previous_warn;
            console.error = previous_error;
        }
        assert(
            diagnostics.length === 0,
            `observer-local policy differences should not warn: ${JSON.stringify(diagnostics)}`,
        );

        const off_first = first.subscribe(() => {});
        const off_second = second.subscribe(() => {});
        assert(intervals.size === 1, 'same-key observers should share one entry timer');
        assert(
            [...intervals][0].delay === 100,
            'the entry timer should use the minimum positive active interval',
        );
        assert(win.listener_count('focus') === 1, 'only the focus-enabled observer should install a focus listener');
        assert(win.listener_count('online') === 1, 'only the reconnect-enabled observer should install an online listener');

        off_first();
        assert(intervals.size === 1, 'removing one observer should preserve the other interval');
        assert(
            [...intervals][0].delay === 250,
            'removing the fastest observer should rearm at the remaining interval',
        );
        assert(win.listener_count('focus') === 0, 'removing the focus observer should remove only its listener');
        assert(win.listener_count('online') === 1, 'the reconnect observer should remain active');

        off_second();
        assert(intervals.size === 0, 'removing the final observer should clear all intervals');
        assert(win.listener_count('online') === 0, 'removing the final observer should clear reconnect listeners');
    }

    // Public cache handles created before registration remain ordinary
    // subscribers. They receive the eventual seed/update, but they do not
    // silently inherit a later declaration's observer policy.
    {
        const win = install_window();
        const intervals = install_timers();
        const key = ['query-lifecycle', 'late-registration'];
        const public_handle = rsjs.query(key);
        const off_public = public_handle.subscribe(() => {});

        assert(intervals.size === 0, 'stub query subscriber should not install interval before registration');
        assert(win.listener_count('focus') === 0, 'stub query subscriber should not install focus listener before registration');

        const registered_handle = register_query({
            island: 'query-lifecycle',
            query_id: 1,
            key,
            fetcher: '/api/query-lifecycle/late-registration',
            options: { interval_ms: 250, refetch_on_focus: true },
        });

        assert(intervals.size === 0, 'registration should not transfer policy to an existing public handle');
        assert(win.listener_count('focus') === 0, 'registration should wait for its own observer to subscribe');

        const off_registered = registered_handle.subscribe(() => {});
        assert(intervals.size === 1, 'the registered observer should install its interval when subscribed');
        assert(win.listener_count('focus') === 1, 'the registered observer should install its focus listener when subscribed');

        off_registered();
        assert(intervals.size === 0, 'registered observer cleanup should not depend on unrelated public subscribers');
        assert(win.listener_count('focus') === 0, 'registered observer cleanup should remove its focus listener');
        off_public();
    }

    // Focus/reconnect policies consult the observer's own stale threshold.
    // If several stale observers trigger together, the entry-wide in-flight
    // promise still deduplicates the network request.
    {
        const win = install_window();
        install_timers();
        let now = 10_000;
        Date.now = () => now;
        const key = ['query-lifecycle', 'stale-triggers'];
        set_data(key, { version: 0 });

        let fetch_count = 0;
        const pending = [];
        globalThis.fetch = () => {
            fetch_count += 1;
            return new Promise((resolve) => pending.push(resolve));
        };
        const make_response = (version) => ({
            ok: true,
            status: 200,
            async text() {
                return JSON.stringify({ version });
            },
        });

        const fast = register_query({
            island: 'query-lifecycle-fast',
            query_id: 0,
            key,
            fetcher: '/api/query-lifecycle/stale-triggers',
            options: {
                stale_time_ms: 100,
                refetch_on_focus: true,
            },
        });
        const slow = register_query({
            island: 'query-lifecycle-slow',
            query_id: 0,
            key,
            fetcher: '/api/query-lifecycle/stale-triggers',
            options: {
                stale_time_ms: 500,
                refetch_on_focus: true,
                refetch_on_reconnect: true,
            },
        });
        const off_fast = fast.subscribe(() => {});
        const off_slow = slow.subscribe(() => {});
        assert(
            win.listener_count('focus') === 1,
            'same-key focus observers should share one entry listener',
        );
        assert(
            win.listener_count('online') === 1,
            'the reconnect-policy union should install one entry listener',
        );

        now += 50;
        win.dispatch('focus');
        assert(fetch_count === 0, 'focus should not refetch while every observer considers data fresh');

        now += 100;
        assert(fast.is_stale() === true, 'fast observer should use its own stale threshold');
        assert(slow.is_stale() === false, 'slow observer should remain fresh for the same entry');
        win.dispatch('focus');
        win.dispatch('focus');
        assert(
            fetch_count === 1,
            `repeated stale focus triggers should join one in-flight fetch; got ${fetch_count}`,
        );
        const joined_focus = fast.refetch();
        pending.shift()(make_response(1));
        await joined_focus;

        now += 200;
        win.dispatch('online');
        assert(
            fetch_count === 1,
            `reconnect should not refetch before either observer is stale again; got ${fetch_count}`,
        );

        now += 400;
        win.dispatch('online');
        assert(fetch_count === 2, 'stale reconnect observers should start one shared fetch');
        const joined_reconnect = slow.refetch();
        pending.shift()(make_response(2));
        await joined_reconnect;

        off_fast();
        off_slow();
    }

    // A live query handle is a stable observer proxy. Rebinding moves its
    // subscriptions to a new identity, keeps old in-flight work isolated, and
    // makes imperative methods target the current entry.
    {
        install_window();
        install_timers();
        let now = 50_000;
        Date.now = () => now;
        const old_key = ['query-lifecycle', 'live-key', 'old'];
        const middle_key = ['query-lifecycle', 'live-key', 'middle'];
        const current_key = ['query-lifecycle', 'live-key', 'current'];
        const cached_key = ['query-lifecycle', 'live-key', 'cached'];
        set_data(old_key, { version: 'old' });

        const pending = [];
        let fetch_count = 0;
        globalThis.fetch = (url) => {
            fetch_count += 1;
            return new Promise((resolve) => pending.push({ url, resolve }));
        };
        const response = (version) => ({
            ok: true,
            status: 200,
            async text() {
                return JSON.stringify({ version });
            },
        });

        const handle = register_query({
            island: 'query-live-key',
            query_id: 0,
            key: old_key,
            fetcher: '/api/query-lifecycle/live-key',
            options: { stale_time_ms: 10_000 },
        });
        const notifications = [];
        const off = handle.subscribe((value) => notifications.push(value));
        assert(fetch_count === 0, 'fresh initial identity should not refetch');

        const moved_to_middle = rebind_query(handle, {
            island: 'query-live-key',
            query_id: 0,
            key: middle_key,
            fetcher: '/api/query-lifecycle/live-key',
            options: { stale_time_ms: 10_000 },
        });
        assert(moved_to_middle === true, 'changed live key should move the observer');
        assert(fetch_count === 1, 'an uncached rebound identity should start one fetch');
        assert(handle.status() === 'loading', 'uncached rebound identity should expose loading');
        assert(
            notifications.length === 1 && notifications[0] === undefined,
            'rebind should emit exactly one identity-change notification',
        );
        const middle_request = handle.refetch();

        const moved_to_current = rebind_query(handle, {
            island: 'query-live-key',
            query_id: 0,
            key: current_key,
            fetcher: '/api/query-lifecycle/live-key',
            options: { stale_time_ms: 10_000 },
        });
        assert(moved_to_current === true, 'a second changed key should move again');
        assert(fetch_count === 2, 'the current uncached identity should start its own fetch');
        const current_request = handle.refetch();

        pending.shift().resolve(response('middle'));
        await middle_request;
        assert(
            handle.data() === undefined,
            'old in-flight completion must not replace the rebound handle snapshot',
        );
        assert(
            notifications.length === 2,
            'old in-flight completion must not notify the moved observer',
        );
        assert(
            rsjs.query(middle_key).data()?.version === 'middle',
            'old request should still populate its own cache identity',
        );

        pending.shift().resolve(response('current'));
        await current_request;
        assert(
            handle.data()?.version === 'current',
            'current in-flight completion should update the stable proxy',
        );
        assert(
            notifications.at(-1)?.version === 'current',
            'current entry completion should notify the moved subscription',
        );

        const before_old_write = notifications.length;
        set_data(middle_key, { version: 'middle-2' });
        assert(
            notifications.length === before_old_write,
            'writes to a previous identity must not notify the moved observer',
        );

        set_data(cached_key, { version: 'cached' });
        const fetches_before_cached = fetch_count;
        const notifications_before_cached = notifications.length;
        const moved_to_cached = rebind_query(handle, {
            island: 'query-live-key',
            query_id: 0,
            key: cached_key,
            fetcher: '/api/query-lifecycle/live-key',
            options: { stale_time_ms: 10_000 },
        });
        assert(moved_to_cached === true, 'a cached key should still move identity');
        assert(
            fetch_count === fetches_before_cached,
            'fresh cached target should be used without a network request',
        );
        assert(handle.data()?.version === 'cached', 'fresh cached target should be visible immediately');
        assert(
            notifications.length === notifications_before_cached + 1 &&
                notifications.at(-1)?.version === 'cached',
            'cached rebind should emit one observer notification',
        );

        const unchanged = rebind_query(handle, {
            island: 'query-live-key',
            query_id: 0,
            key: cached_key,
            fetcher: '/api/query-lifecycle/live-key',
            options: { stale_time_ms: 10_000 },
        });
        assert(unchanged === false, 'equal key/spec rebind should be a no-op');

        let rebind_conflict;
        try {
            rebind_query(handle, {
                island: 'query-live-key',
                query_id: 0,
                key: cached_key,
                fetcher: '/api/query-lifecycle/incompatible-live-key',
                options: { stale_time_ms: 10_000 },
            });
        } catch (error) {
            rebind_conflict = error;
        }
        assert(
            rebind_conflict?.message.includes('incompatible fetch semantics'),
            'same-key ABI change should fail before moving the observer',
        );
        assert(
            handle.data()?.version === 'cached',
            'failed rebind should leave the observer on its previous entry',
        );
        off();

        assert(
            get_entry(cached_key)?.data?.version === 'cached',
            'final observer departure should retain cache data for this page generation',
        );
    }

    // Generated live-key owners use a memo watcher over the compiler's
    // deduplicated signal dependency array. The core scheduler must collapse
    // same-tick writes to the final structural identity, and owner disposal
    // must stop all later rebinds.
    {
        install_window();
        install_timers();
        const selected = signal('old');
        const key_for = (value) => ['query-lifecycle', 'generated-live-key', value];
        set_data(key_for('old'), { version: 'old' });

        const pending = [];
        let fetch_count = 0;
        globalThis.fetch = (url) => {
            fetch_count += 1;
            return new Promise((resolve) => pending.push({ url, resolve }));
        };
        const response = (version) => ({
            ok: true,
            status: 200,
            async text() {
                return JSON.stringify({ version });
            },
        });
        const registration = () => ({
            island: 'query-generated-live-key',
            query_id: 0,
            key: key_for(selected.get()),
            live_key: true,
            fetcher: '/api/query-lifecycle/generated-live-key',
            options: { stale_time_ms: 10_000 },
        });
        const handle = register_query(registration());
        const off = handle.subscribe(() => {});
        const watcher = memo(() => {
            rebind_query(handle, registration());
            return null;
        }, [selected]);

        selected.set('middle');
        selected.set('current');
        await Promise.resolve();
        assert(
            fetch_count === 1,
            `same-tick key writes should start one final-key fetch; got ${fetch_count}`,
        );
        assert(
            get_entry(key_for('middle')) === undefined,
            'coalesced watcher must never create the intermediate cache identity',
        );
        const current_request = handle.refetch();
        pending.shift().resolve(response('current'));
        await current_request;
        assert(
            handle.data()?.version === 'current',
            'generated watcher should expose the final rebound identity',
        );

        watcher._dispose();
        selected.set('after-dispose');
        await Promise.resolve();
        assert(
            get_entry(key_for('after-dispose')) === undefined &&
                handle.data()?.version === 'current',
            'disposing the generated watcher must prevent later key rebinds',
        );
        off();
    }

    // Late hydration of a live key must preserve the adjacent SSR seed under
    // its original identity while binding the observer and request snapshot
    // directly to the key currently evaluated in the browser.
    {
        install_window();
        install_timers();
        const old_key = ['query-lifecycle', 'late-live-key', 'old'];
        const current_key = ['query-lifecycle', 'late-live-key', 'current'];
        const island = 'query-late-live-key';
        const blob = {
            tagName: 'SCRIPT',
            textContent: JSON.stringify({
                0: {
                    key: old_key,
                    data: { version: 'ssr-old' },
                    data_updated_at: 1,
                },
            }),
            nextElementSibling: null,
            getAttribute(name) {
                if (name === 'type') return 'application/json';
                if (name === 'data-rsjs-island-queries') return island;
                return null;
            },
        };
        const root = {
            nextElementSibling: blob,
            getAttribute() {
                return null;
            },
        };
        set_data(current_key, { version: 'browser-current' });

        const live = register_query({
            island,
            query_id: 0,
            root,
            key: current_key,
            live_key: true,
            fetcher: '/api/query-lifecycle/late-live-key/current',
            options: { stale_time_ms: 10_000 },
        });
        assert(
            live.data()?.version === 'browser-current',
            'late live registration should bind directly to the browser-current key',
        );

        let old;
        let old_registration_error;
        try {
            old = register_query({
                island: 'query-late-live-key-old-owner',
                query_id: 0,
                key: old_key,
                fetcher: '/api/query-lifecycle/late-live-key/old',
                options: { stale_time_ms: 10_000 },
            });
        } catch (error) {
            old_registration_error = error;
        }
        assert(
            old_registration_error === undefined,
            `current request semantics must not be installed on the SSR key: ${old_registration_error}`,
        );
        assert(
            old?.data()?.version === 'ssr-old',
            'the adjacent SSR seed should remain available under its original key',
        );
    }

    // Scheduling differences are valid, but entry-wide request/retry/decoder
    // differences for one structural key fail before a second observer is
    // installed.
    {
        install_window();
        install_timers();
        const key = ['query-lifecycle', 'fetch-conflict'];
        register_query({
            island: 'query-lifecycle-compatible',
            query_id: 0,
            key,
            fetcher: '/api/query-lifecycle/fetch-conflict',
            options: { stale_time_ms: 10, retry_count: 1 },
        });

        let error;
        try {
            register_query({
                island: 'query-lifecycle-incompatible',
                query_id: 0,
                key,
                fetcher: '/api/query-lifecycle/different-endpoint',
                options: { stale_time_ms: 1_000, retry_count: 1 },
            });
        } catch (caught) {
            error = caught;
        }
        assert(error instanceof Error, 'incompatible same-key fetch semantics should throw');
        assert(
            error?.message.includes('incompatible fetch semantics'),
            `fetch conflict should have a focused diagnostic, got ${error?.message}`,
        );

        let retry_error;
        try {
            register_query({
                island: 'query-lifecycle-incompatible-retry',
                query_id: 0,
                key,
                fetcher: '/api/query-lifecycle/fetch-conflict',
                options: { stale_time_ms: 5_000, retry_count: 2 },
            });
        } catch (caught) {
            retry_error = caught;
        }
        assert(
            retry_error instanceof Error,
            'same-key retry differences should remain an entry-wide conflict',
        );
    }

    // Page-generation teardown is the deterministic query-cache GC boundary.
    // It removes policy machinery, aborts requests, invalidates handles, and
    // lets a new generation reuse the same structural key without old state.
    {
        const win = install_window();
        const intervals = install_timers();
        const key = ['query-lifecycle', 'page-generation'];
        let abort_count = 0;
        let request_count = 0;
        globalThis.fetch = (_url, init) =>
            new Promise((_resolve, reject) => {
                request_count += 1;
                init.signal?.addEventListener(
                    'abort',
                    () => {
                        abort_count += 1;
                        reject(new Error('query generation disposed'));
                    },
                    { once: true },
                );
            });

        const handle = register_query({
            island: 'query-page-generation',
            query_id: 0,
            key,
            fetcher: '/api/query-lifecycle/page-generation',
            options: {
                interval_ms: 250,
                refetch_on_focus: true,
                refetch_on_reconnect: true,
                retry_count: 3,
            },
        });
        const off = handle.subscribe(() => {});
        const request = handle.refetch();
        assert(intervals.size === 1, 'active generation should own its entry timer');
        assert(win.listener_count('focus') === 1, 'active generation should own its focus listener');
        assert(win.listener_count('online') === 1, 'active generation should own its reconnect listener');

        dispose_page_query_state();
        dispose_page_query_state();
        await request.catch(() => {});
        assert(abort_count === 1, 'generation teardown should abort its in-flight query');
        assert(
            request_count === 1,
            'generation abort should not enter the ordinary retry/backoff policy',
        );
        assert(intervals.size === 0, 'generation teardown should clear query timers');
        assert(win.listener_count('focus') === 0, 'generation teardown should remove focus listeners');
        assert(win.listener_count('online') === 0, 'generation teardown should remove reconnect listeners');
        assert(get_entry(key) === undefined, 'generation teardown should clear the query cache');

        let subscribe_error;
        try {
            handle.subscribe(() => {});
        } catch (error) {
            subscribe_error = error;
        }
        assert(
            subscribe_error?.message.includes('query-disposed'),
            'old-generation handles should reject new subscriptions',
        );
        assert(
            dispose_query(handle) === false,
            'per-handle disposal after page teardown should be an idempotent no-op',
        );
        off();

        set_data(key, { generation: 'next' });
        assert(
            rsjs.query(key).data()?.generation === 'next',
            'a new generation should receive a clean cache identity',
        );
        dispose_page_query_state();
    }
} finally {
    if (previous_window === undefined) {
        delete globalThis.window;
    } else {
        globalThis.window = previous_window;
    }
    globalThis.setInterval = previous_set_interval;
    globalThis.clearInterval = previous_clear_interval;
    globalThis.fetch = previous_fetch;
    Date.now = previous_date_now;
}

if (failed > 0) {
    process.exit(1);
}

console.log('All query lifecycle tests passed.');
