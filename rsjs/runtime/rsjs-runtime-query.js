// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// rsjs query + mutation runtime — page-scoped QueryClient + projection
// helpers + mutation registry + cross-island invalidation.
//
// M8 (queries) + M9 (mutations) implementation, plain ESM, named
// exports.
//
// Sibling module to rsjs-runtime.js. Loaded only by islands that
// declare at least one Rust `query_with_initial(...)`. Targets <= 7 KB minified +
// gzipped; we keep the body compact-but-readable (no minification
// happens in this file; that is the deploy pipeline's job).
//
// Public surface (also see PLAN § Cross-framework interop):
//
//     import {
//         rsjs,
//         register_query,
//         rebind_query,
//         dispose_query,
//         dispose_page_query_state,
//     } from '/rsjs-runtime-query.js';
//
//     // Cross-framework consumers (React, Vue, vanilla) subscribe
//     // via the typed handle the runtime exposes:
//     const handle = rsjs.query(['cart', userId]);
//     const off = handle.subscribe((value) => {...});
//     const snap = handle.getSnapshot();
//     handle.invalidate();
//
//     // Per-island registration (emit_js calls this once per
//     // QueryDecl):
//     const q = register_query({
//         island: 'stockticker',
//         query_id: 0,
//         key: ['stock', 'AAPL'],
//         fetcher: '/api/stocks/AAPL',
//         options: {
//             interval_ms: 5000,
//             refetch_on_focus: true,
//             download_progress: false,
//             stream_text: false,
//             retry_count: 3,
//             stale_time_ms: 2000,
//         },
//     });
//     // q exposes: data(), status(), is_loading(), is_success(),
//     // is_error(), is_refetching(), is_stale(), error(),
//     // data_updated_at(), error_updated_at(), fetch_count(),
//     // download_loaded(), download_total(), download_percent().
//
// The page-scoped QueryClient owns one cache entry per unique
// stringified key, deduplicates server fetches (per the dehydrated
// blob), deduplicates in-flight client fetches, and notifies every
// subscriber when an entry's value changes. Three islands subscribed
// to ['stock', 'AAPL'] share one cache entry; one fetch covers all
// of them.

// ---------------------------------------------------------------------------
// Cache key normalisation
// ---------------------------------------------------------------------------

/**
 * Structural equality for fetcher / runner descriptors. Functions are
 * compatible only when they are the same function object. Static request
 * descriptors are normalized separately before query registration compares
 * them, so a bare URL and its equivalent `{url}` form agree.
 */
function shallow_eq_fetcher(a, b) {
    if (a === b) return true;
    if (typeof a === 'function' && typeof b === 'function') return false;
    if (typeof a === 'function' || typeof b === 'function') return false;
    if (typeof a === 'string' && typeof b === 'string') return a === b;
    if (typeof a === 'string' || typeof b === 'string') return false;
    if (a == null || b == null) return false;
    if (typeof a !== 'object' || typeof b !== 'object') return false;
    return deep_eq(a, b);
}

/**
 * Shallow equality for option bags (the `options` field on
 * register_query / register_mutation calls). Same shape: deep-eq on
 * objects, structural equality on primitives.
 */
function shallow_eq_options(a, b) {
    return deep_eq(a || {}, b || {});
}

const QUERY_OBSERVER_OPTION_KEYS = new Set([
    'stale_time_ms',
    'interval_ms',
    'refetch_on_focus',
    'refetch_on_reconnect',
]);

function query_fetch_options(options) {
    const fetch_options = {};
    for (const [name, value] of Object.entries(options || {})) {
        if (!QUERY_OBSERVER_OPTION_KEYS.has(name)) {
            fetch_options[name] = value;
        }
    }
    // Canonicalize the currently supported entry-wide defaults. Generated
    // registrations normally spell these fields out, while hand-written
    // integrations often omit false/zero values; those forms have identical
    // fetch semantics and must not conflict merely because their object shapes
    // differ.
    fetch_options.download_progress = options?.download_progress === true;
    fetch_options.stream_text = options?.stream_text === true;
    fetch_options.retry_count = Math.max(
        0,
        Math.trunc(Number(options?.retry_count) || 0),
    );
    return fetch_options;
}

function query_observer_options(options) {
    options = options || {};
    const stale_time_ms = Math.max(
        0,
        Math.trunc(Number(options.stale_time_ms) || 0),
    );
    const interval_ms = Math.max(
        0,
        Math.trunc(Number(options.interval_ms) || 0),
    );
    return {
        stale_time_ms,
        interval_ms: interval_ms > 0 ? interval_ms : null,
        refetch_on_focus: options.refetch_on_focus === true,
        refetch_on_reconnect: options.refetch_on_reconnect === true,
    };
}

function query_fetch_semantics(fetcher, options) {
    return {
        fetcher:
            typeof fetcher === 'function'
                ? fetcher
                : parse_request_config(fetcher, 'GET', 'rsjs.register_query'),
        options,
    };
}

/**
 * Stringify a structural cache key. We use JSON.stringify for the
 * common case (arrays of primitives); keys that contain reference-
 * typed values (objects) are stringified recursively in a stable
 * (sorted) order so two equivalent keys collapse to the same entry.
 */
function stringify_key(key) {
    if (key == null) return 'null';
    if (Array.isArray(key)) {
        return '[' + key.map(stringify_key).join(',') + ']';
    }
    if (typeof key === 'object') {
        const keys = Object.keys(key).sort();
        const parts = [];
        for (const k of keys) {
            parts.push(JSON.stringify(k) + ':' + stringify_key(key[k]));
        }
        return '{' + parts.join(',') + '}';
    }
    // `JSON.stringify` throws on a BigInt. A wide shared field reaching a
    // query key is now ordinary (shared fields hydrate as BigInt whenever the
    // Rust type is in the i64 family), so serialize it explicitly. The `n`
    // suffix keeps `1n` and `"1"` distinct cache keys.
    if (typeof key === 'bigint') return key.toString() + 'n';
    return JSON.stringify(key);
}

/**
 * `JSON.stringify` replacer that renders BigInt as its decimal string.
 *
 * This matches the wire codec, where wide integers already travel as JSON
 * strings because JSON numbers round past 2^53. Without it, putting a wide
 * shared field into a request body throws
 * `TypeError: Do not know how to serialize a BigInt`.
 */
function bigint_safe_replacer(_key, value) {
    return typeof value === 'bigint' ? value.toString() : value;
}

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

/**
 * State machine slot for a single cache entry. Tracks both the
 * typed value and the metadata accessors expose (status, timestamps,
 * fetch counts).
 */
function make_entry(initial) {
    return {
        data: initial?.data,
        error: initial?.error,
        // status: 'idle' | 'loading' | 'success' | 'error' | 'refetching'
        status: initial?.data !== undefined ? 'success' : 'idle',
        is_stale: false,
        data_updated_at: initial?.data_updated_at ?? 0,
        error_updated_at: initial?.error_updated_at ?? 0,
        fetch_count: initial?.data !== undefined ? 1 : 0,
        download_loaded: 0,
        download_total: null,
        download_percent: 0,
        // Internal: subscribers and in-flight fetch promise (so the
        // SWR dedup property holds across re-entry).
        subscribers: new Set(),
        observers: new Set(),
        active_observers: new Set(),
        in_flight: null,
        in_flight_controller: null,
        // Entry-wide fetch semantics. Observer policy remains independent,
        // while one entry-level scheduler combines the active policies.
        fetcher: null,
        options: null,
        fetch_semantics: null,
        policy_interval_handle: null,
        policy_interval_ms: null,
        policy_focus_listener: null,
        policy_focus_target: null,
        policy_reconnect_listener: null,
        policy_reconnect_target: null,
        generation: QUERY_GENERATION,
        disposed: false,
        // `undefined`: pristine cache stub that may still accept an SSR seed.
        // string: data currently comes from that ranked SSR seed.
        // `null`: client/refetch/mutation-owned data; late SSR must not clobber it.
        _rsjs_seed_rank: undefined,
    };
}

// ---------------------------------------------------------------------------
// QueryClient (singleton per page)
// ---------------------------------------------------------------------------

let QUERY_GENERATION = 0;
const CACHE = new Map();
const QUERY_HANDLE_OBSERVERS = new WeakMap();
// Page-level query seed table: structural cache key -> { state, rank }.
// Framework SSR can emit one inert JSON script for all query_with_initial(...)
// seeds so repeated islands sharing a key hydrate from one parsed table.
const PAGE_DEHYDRATED = new Map();
// A streamed boundary or client-navigation fragment may add another seed
// table after the initial document was parsed. Remember the exact source seen
// for each element instead of using a page-global "parsed once" latch, so
// later tables and in-place morph updates are consumed incrementally.
let PAGE_DEHYDRATED_TABLE_SOURCES = new WeakMap();
// Two-level fallback dehydration index: island id -> query id -> { data, ... }.
// Macro-emitted registrations pass their hydrated root so the runtime can read
// the adjacent per-instance blob. This map remains for public `rsjs.query(...)`
// consumers and older generated bundles that do not pass a root.
const DEHYDRATED = new Map();
// Two-level fallback mutation metadata index: island id -> mutation id ->
// { key?, runner?, invalidates?, options? }. New generated bundles pass the
// hydrated root and use the adjacent per-instance blob; the global map remains
// for older bundles and non-rooted test harnesses.
const MUTATION_METADATA = new Map();

/** Every page/fragment query seed table currently present in document order. */
function page_query_seed_tables() {
    if (typeof document === 'undefined') return [];
    if (typeof document.querySelectorAll === 'function') {
        let found = null;
        try {
            found = document.querySelectorAll(
                'script[type="application/json"][id="rsjs-query-seed-table"]',
            );
        } catch (_) {
            found = null;
        }
        if (found != null && found.length > 0) {
            return Array.from(found);
        }
    }
    if (typeof document.getElementById !== 'function') return [];
    const table = document.getElementById('rsjs-query-seed-table');
    return table == null ? [] : [table];
}

function compare_utf8(left, right) {
    const left_bytes = new TextEncoder().encode(left);
    const right_bytes = new TextEncoder().encode(right);
    const length = Math.min(left_bytes.length, right_bytes.length);
    for (let index = 0; index < length; index += 1) {
        if (left_bytes[index] < right_bytes[index]) return -1;
        if (left_bytes[index] > right_bytes[index]) return 1;
    }
    if (left_bytes.length < right_bytes.length) return -1;
    if (left_bytes.length > right_bytes.length) return 1;
    return 0;
}

function normalized_seed_rank(rank, state) {
    // New servers include the exact raw serialized state. `compare_utf8`
    // therefore matches Rust's `String::cmp`, including non-ASCII states.
    if (typeof rank === 'string') {
        return rank;
    }
    // Compatibility with seed tables emitted before ranks were added. Object
    // property order is preserved by JSON.parse/JSON.stringify, making this
    // deterministic even though escaped/non-ASCII legacy states cannot always
    // reproduce the original Rust byte sequence exactly.
    return JSON.stringify(state);
}

function reconcile_ranked_ssr_seed(skey, state, rank) {
    const entry = CACHE.get(skey);
    if (!entry || entry._rsjs_seed_rank === null || entry.in_flight) return;
    if (
        typeof entry._rsjs_seed_rank === 'string' &&
        compare_utf8(entry._rsjs_seed_rank, rank) <= 0
    ) {
        return;
    }

    entry.data = state.data;
    entry.error = state.error;
    entry.status =
        state.data !== undefined ? 'success' : state.error !== undefined ? 'error' : 'idle';
    entry.is_stale = Boolean(state.is_stale);
    entry.data_updated_at = state.data_updated_at ?? 0;
    entry.error_updated_at = state.error_updated_at ?? 0;
    entry.fetch_count = state.fetch_count ?? (state.data !== undefined ? 1 : 0);
    entry.download_loaded = state.download_loaded ?? 0;
    entry.download_total = state.download_total ?? null;
    entry.download_percent = state.download_percent ?? 0;
    entry._rsjs_seed_rank = rank;
    if (entry.subscribers.size > 0) {
        notify(entry);
    }
}

function install_page_query_seed(key, state, rank) {
    const skey = stringify_key(key);
    const current = PAGE_DEHYDRATED.get(skey);
    if (current && compare_utf8(current.rank, rank) <= 0) return;
    PAGE_DEHYDRATED.set(skey, { state, rank });
    reconcile_ranked_ssr_seed(skey, state, rank);
}

/** Parse every new or changed page/fragment query seed table. */
function seed_page_query_table() {
    for (const blob of page_query_seed_tables()) {
        const source = blob.textContent || '{}';
        if (PAGE_DEHYDRATED_TABLE_SOURCES.get(blob) === source) continue;
        try {
            const parsed = JSON.parse(source);
            const entries = Array.isArray(parsed) ? parsed : parsed.queries;
            if (!Array.isArray(entries)) continue;
            for (const entry of entries) {
                let key;
                let state;
                if (Array.isArray(entry)) {
                    key = entry[0];
                    state = entry[1];
                } else if (entry && typeof entry === 'object') {
                    key = entry.key;
                    state = entry.state;
                }
                if (key === undefined || !state || typeof state !== 'object') continue;
                const rank = normalized_seed_rank(
                    entry && typeof entry === 'object' && !Array.isArray(entry)
                        ? entry.rank
                        : undefined,
                    state,
                );
                if (state.key === undefined) {
                    state = { ...state, key };
                }
                install_page_query_seed(key, state, rank);
            }
            PAGE_DEHYDRATED_TABLE_SOURCES.set(blob, source);
        } catch (e) {
            console.warn('rsjs: failed to parse page-level query seed table', e);
        }
    }
}

function page_dehydrated_seed_for_key(key) {
    if (key === undefined) return undefined;
    seed_page_query_table();
    return PAGE_DEHYDRATED.get(stringify_key(key));
}

/**
 * Walk every <script type="application/json"
 * data-rsjs-island-queries="<asset_id>"> blob the server emitted
 * and stash its contents in the global fallback map. Macro-emitted
 * query registrations pass the hydrated root and read the adjacent
 * per-instance blob instead; this fallback supports public
 * `rsjs.query(...)` consumers and older generated bundles.
 *
 * Idempotent: re-running is a no-op after marking each blob seeded.
 */
function seed_dehydrated_state() {
    if (typeof document === 'undefined') return;
    const blobs = document.querySelectorAll(
        'script[type="application/json"][data-rsjs-island-queries]',
    );
    // Global fallback is keyed by asset id alone. This is fine for
    // public `rsjs.query(...)` consumers that only need one seed per
    // island asset, but multiple blobs for the same asset are
    // ambiguous. Macro-emitted registrations avoid this path by
    // passing their hydrated root.
    const seen_islands = new Set();
    for (const blob of blobs) {
        if (blob.hasAttribute('data-rsjs-seeded')) continue;
        const island = blob.getAttribute('data-rsjs-island-queries');
        if (seen_islands.has(island)) {
            console.warn(
                `rsjs: multiple dehydrated query blobs for island "${island}". ` +
                    `The public rsjs.query() fallback is keyed per island asset, ` +
                    `so the last global seed wins. Macro-generated query ` +
                    `registrations use their adjacent per-instance blob instead.`,
            );
        }
        seen_islands.add(island);
        try {
            const parsed = JSON.parse(blob.textContent);
            DEHYDRATED.set(island, parsed);
        } catch (e) {
            console.warn(
                `rsjs: failed to parse dehydrated query state for ${island}`,
                e,
            );
        }
        blob.setAttribute('data-rsjs-seeded', '1');
    }
}

function dehydrated_state_for(island, query_id, root) {
    const instance_state = root ? dehydrated_state_for_root(root, island) : undefined;
    if (instance_state && instance_state[String(query_id)]) {
        return instance_state[String(query_id)];
    }
    if (!root) {
        const island_state = DEHYDRATED.get(island);
        return island_state && island_state[String(query_id)]
            ? island_state[String(query_id)]
            : undefined;
    }
    return undefined;
}

function dehydrated_state_for_root(root, island) {
    const blob = query_state_script_for(root, island);
    if (!blob) return undefined;
    if (blob.__rsjs_query_state !== undefined) {
        return blob.__rsjs_query_state;
    }
    try {
        blob.__rsjs_query_state = JSON.parse(blob.textContent || '{}');
        return blob.__rsjs_query_state;
    } catch (e) {
        console.warn(
            `rsjs: failed to parse dehydrated query state for ${island}`,
            e,
        );
        blob.__rsjs_query_state = undefined;
        return undefined;
    }
}

function query_state_script_for(root, island) {
    if (!root || typeof root.getAttribute !== 'function') {
        return null;
    }
    let next = root.nextElementSibling;
    while (
        next &&
        next.tagName === 'SCRIPT' &&
        next.getAttribute('type') === 'application/json'
    ) {
        if (next.getAttribute('data-rsjs-island-queries') === island) {
            return next;
        }
        next = next.nextElementSibling;
    }
    return null;
}

function has_own(obj, key) {
    return Object.prototype.hasOwnProperty.call(obj || {}, key);
}

/**
 * Walk every <script type="application/json"
 * data-rsjs-island-mutations="<asset_id>"> metadata blob and stash
 * its contents in the global fallback map. Macro-emitted mutation
 * registrations pass the hydrated root and read their adjacent blob
 * instead, preserving per-instance metadata when the same island
 * asset renders more than once.
 */
function seed_mutation_metadata() {
    if (typeof document === 'undefined') return;
    const blobs = document.querySelectorAll(
        'script[type="application/json"][data-rsjs-island-mutations]',
    );
    const seen_islands = new Set();
    for (const blob of blobs) {
        if (blob.hasAttribute('data-rsjs-mutations-seeded')) continue;
        const island = blob.getAttribute('data-rsjs-island-mutations');
        if (seen_islands.has(island)) {
            console.warn(
                `rsjs: multiple mutation metadata blobs for island "${island}". ` +
                    `The fallback register_mutation() path is keyed per island asset, ` +
                    `so the last global seed wins. Macro-generated mutation ` +
                    `registrations use their adjacent per-instance blob instead.`,
            );
        }
        seen_islands.add(island);
        try {
            const parsed = JSON.parse(blob.textContent || '{}');
            MUTATION_METADATA.set(island, parsed);
        } catch (e) {
            console.warn(
                `rsjs: failed to parse mutation metadata for ${island}`,
                e,
            );
        }
        blob.setAttribute('data-rsjs-mutations-seeded', '1');
    }
}

function mutation_metadata_for(island, mutation_id, root) {
    if (island == null || mutation_id == null) return undefined;
    const key = String(mutation_id);
    const instance_metadata = root ? mutation_metadata_for_root(root, island) : undefined;
    if (instance_metadata && instance_metadata[key]) {
        return instance_metadata[key];
    }
    if (!root) {
        seed_mutation_metadata();
        const island_metadata = MUTATION_METADATA.get(island);
        return island_metadata && island_metadata[key] ? island_metadata[key] : undefined;
    }
    return undefined;
}

function mutation_metadata_for_root(root, island) {
    const blob = mutation_state_script_for(root, island);
    if (!blob) return undefined;
    if (blob.__rsjs_mutation_metadata !== undefined) {
        return blob.__rsjs_mutation_metadata;
    }
    try {
        blob.__rsjs_mutation_metadata = JSON.parse(blob.textContent || '{}');
        return blob.__rsjs_mutation_metadata;
    } catch (e) {
        console.warn(
            `rsjs: failed to parse mutation metadata for ${island}`,
            e,
        );
        blob.__rsjs_mutation_metadata = undefined;
        return undefined;
    }
}

function mutation_state_script_for(root, island) {
    if (!root || typeof root.getAttribute !== 'function') {
        return null;
    }
    let next = root.nextElementSibling;
    while (
        next &&
        next.tagName === 'SCRIPT' &&
        next.getAttribute('type') === 'application/json'
    ) {
        if (next.getAttribute('data-rsjs-island-mutations') === island) {
            return next;
        }
        next = next.nextElementSibling;
    }
    return null;
}

/**
 * Notify every subscriber of an entry that its value changed. Each
 * subscriber receives the entry's current data (the .data() projection
 * is what most consumers want; the handle exposes raw status etc.).
 */
function notify(entry) {
    const snapshot = Array.from(entry.subscribers);
    for (const fn of snapshot) {
        try {
            fn(entry.data);
        } catch (e) {
            // A misbehaving subscriber must not break the runtime.
            console.warn('rsjs: query subscriber threw', e);
        }
    }
}

const NO_REQUEST_BODY = Symbol('rsjs.no_request_body');

/**
 * Parse a query fetcher / mutation runner config. Accepts a bare URL
 * string, a JSON object string emitted by the Rust macro, or an
 * already-object descriptor. Always returns a normalised shape.
 */
function parse_request_config(raw, default_method, label) {
    if (typeof raw === 'function') {
        raw = raw();
    }
    let cfg = raw;
    if (typeof raw === 'string') {
        const trimmed = raw.trimStart();
        const first = trimmed.charAt(0);
        if (first === '{' || first === '[') {
            try {
                cfg = JSON.parse(raw);
            } catch (e) {
                throw new Error(
                    `${label}: malformed JSON request config; ` +
                        `got: ${raw.slice(0, 50)}; ` +
                        `expected an object {url, method?, ...}`,
                );
            }
            if (!cfg || typeof cfg !== 'object' || Array.isArray(cfg)) {
                throw new Error(
                    `${label}: malformed JSON request config; ` +
                        `got: ${raw.slice(0, 50)}; ` +
                        `expected an object {url, method?, ...}`,
                );
            }
        } else {
            cfg = { url: raw };
        }
    }
    if (!cfg || typeof cfg !== 'object') {
        cfg = { url: '', method: default_method };
    }
    return {
        url: cfg.url || '',
        method: String(cfg.method || default_method).toUpperCase(),
        credentials: cfg.credentials || 'same-origin',
        headers: cfg.headers || {},
        body: cfg.body,
        response: cfg.response || 'json',
        timeout_ms: cfg.timeout_ms || 0,
    };
}

function clone_headers(headers) {
    if (Array.isArray(headers)) {
        return headers.map((entry) => [entry[0], entry[1]]);
    }
    return { ...(headers || {}) };
}

function has_header(headers, name) {
    const needle = String(name).toLowerCase();
    if (Array.isArray(headers)) {
        return headers.some((entry) => String(entry[0]).toLowerCase() === needle);
    }
    return Object.keys(headers || {}).some((key) => key.toLowerCase() === needle);
}

function set_header(headers, name, value) {
    if (Array.isArray(headers)) {
        headers.push([name, value]);
    } else {
        headers[name] = value;
    }
}

function method_allows_body(method) {
    return method !== 'GET' && method !== 'HEAD';
}

function request_body_from_config(config) {
    if (config == null) return null;
    if (typeof config === 'object' && typeof config.kind === 'string') {
        const value = config.value ?? '';
        switch (config.kind) {
            case 'json':
                return { body: String(value), content_type: 'application/json' };
            case 'text':
                return { body: String(value), content_type: 'text/plain;charset=UTF-8' };
            case 'bytes_base64':
                return {
                    body: decode_base64_to_bytes(String(value)),
                    content_type: 'application/octet-stream',
                };
            case 'bytes_base64url':
                return {
                    body: decode_base64url_to_bytes(String(value)),
                    content_type: 'application/octet-stream',
                };
            default:
                throw new Error(`rsjs request body kind is not supported: ${config.kind}`);
        }
    }
    if (typeof config === 'object') {
        return {
            body: JSON.stringify(config, bigint_safe_replacer),
            content_type: 'application/json',
        };
    }
    return { body: config, content_type: null };
}

function decode_base64_to_bytes(value) {
    const binary = atob(value);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
        bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
}

function decode_base64url_to_bytes(value) {
    let base64 = value.replace(/-/g, '+').replace(/_/g, '/');
    const pad = base64.length % 4;
    if (pad) base64 += '='.repeat(4 - pad);
    return decode_base64_to_bytes(base64);
}

function build_fetch_init(cfg, fallback_json_body) {
    const headers = clone_headers(cfg.headers);
    const init = {
        method: cfg.method,
        credentials: cfg.credentials,
        headers,
    };
    if (!method_allows_body(init.method)) {
        return init;
    }

    const configured = request_body_from_config(cfg.body);
    if (configured) {
        init.body = configured.body;
        if (configured.content_type && !has_header(headers, 'Content-Type')) {
            set_header(headers, 'Content-Type', configured.content_type);
        }
        return init;
    }

    if (fallback_json_body !== NO_REQUEST_BODY) {
        init.body = JSON.stringify(fallback_json_body, bigint_safe_replacer);
        if (!has_header(headers, 'Content-Type')) {
            set_header(headers, 'Content-Type', 'application/json');
        }
    }
    return init;
}

async function fetch_with_timeout(url, init, timeout_ms, external_signal) {
    if (typeof AbortController === 'undefined') {
        return await fetch(
            url,
            external_signal ? { ...init, signal: external_signal } : init,
        );
    }
    if (!timeout_ms && !external_signal) {
        return await fetch(url, init);
    }

    const controller = new AbortController();
    const abort_from_external = () => {
        controller.abort(external_signal?.reason);
    };
    if (external_signal?.aborted) {
        abort_from_external();
    } else if (typeof external_signal?.addEventListener === 'function') {
        external_signal.addEventListener('abort', abort_from_external, {
            once: true,
        });
    }
    const timer = timeout_ms
        ? setTimeout(() => controller.abort(), timeout_ms)
        : null;
    try {
        return await fetch(url, { ...init, signal: controller.signal });
    } finally {
        if (timer != null) clearTimeout(timer);
        if (typeof external_signal?.removeEventListener === 'function') {
            external_signal.removeEventListener('abort', abort_from_external);
        }
    }
}

async function decode_response(res, kind) {
    switch (String(kind || 'json').toLowerCase()) {
        case 'empty':
            return null;
        case 'bytes':
            return new Uint8Array(await res.arrayBuffer());
        case 'text':
            return await res.text();
        case 'json': {
            const text = await res.text();
            return text ? JSON.parse(text) : null;
        }
        default:
            throw new Error(`rsjs response decoder is not supported: ${kind}`);
    }
}

function parse_content_length(res) {
    const raw =
        res.headers && typeof res.headers.get === 'function'
            ? res.headers.get('content-length')
            : null;
    if (raw == null || raw === '') return null;
    const total = Number(raw);
    return Number.isFinite(total) && total >= 0 ? total : null;
}

async function read_response_bytes_with_progress(res, on_progress) {
    const total = parse_content_length(res);
    let loaded = 0;
    if (on_progress) on_progress(loaded, total);

    const reader =
        res.body && typeof res.body.getReader === 'function'
            ? res.body.getReader()
            : null;
    if (!reader) {
        const bytes = new Uint8Array(await res.arrayBuffer());
        if (on_progress) on_progress(bytes.byteLength, total);
        return bytes;
    }

    const chunks = [];
    try {
        for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            const chunk = value instanceof Uint8Array ? value : new Uint8Array(value || []);
            loaded += chunk.byteLength;
            chunks.push(chunk);
            if (on_progress) on_progress(loaded, total);
        }
    } finally {
        if (typeof reader.releaseLock === 'function') {
            reader.releaseLock();
        }
    }

    const bytes = new Uint8Array(loaded);
    let offset = 0;
    for (const chunk of chunks) {
        bytes.set(chunk, offset);
        offset += chunk.byteLength;
    }
    return bytes;
}

function decode_response_bytes(bytes, kind) {
    switch (String(kind || 'json').toLowerCase()) {
        case 'empty':
            return null;
        case 'bytes':
            return bytes;
        case 'text':
            return new TextDecoder().decode(bytes);
        case 'json': {
            const text = new TextDecoder().decode(bytes);
            return text ? JSON.parse(text) : null;
        }
        default:
            throw new Error(`rsjs response decoder is not supported: ${kind}`);
    }
}

async function read_response_text_stream(res, on_text, on_progress) {
    const total = parse_content_length(res);
    let loaded = 0;
    let text = '';
    const decoder = new TextDecoder();
    if (on_progress) on_progress(loaded, total);
    if (on_text) on_text(text);

    const reader =
        res.body && typeof res.body.getReader === 'function'
            ? res.body.getReader()
            : null;
    if (!reader) {
        text = await res.text();
        if (on_text) on_text(text);
        if (on_progress) on_progress(new TextEncoder().encode(text).byteLength, total);
        return text;
    }

    try {
        for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            const chunk = value instanceof Uint8Array ? value : new Uint8Array(value || []);
            loaded += chunk.byteLength;
            text += decoder.decode(chunk, { stream: true });
            if (on_text) on_text(text);
            if (on_progress) on_progress(loaded, total);
        }
        const tail = decoder.decode();
        if (tail) {
            text += tail;
            if (on_text) on_text(text);
        }
    } finally {
        if (typeof reader.releaseLock === 'function') {
            reader.releaseLock();
        }
    }
    return text;
}

/**
 * Run a fetch with the configured retry policy. Resolves to the
 * decoded response body on success, rejects (after the retry budget)
 * on failure.
 */
async function execute_fetch(fetcher, options, hooks) {
    options = options || {};
    hooks = hooks || {};
    const cfg = parse_request_config(fetcher, 'GET', 'rsjs.register_query');
    let attempts = 0;
    const max = (options.retry_count || 0) + 1;
    let delay_ms = 250;
    while (attempts < max) {
        attempts += 1;
        try {
            const init = build_fetch_init(cfg, NO_REQUEST_BODY);
            const res = await fetch_with_timeout(
                cfg.url,
                init,
                cfg.timeout_ms,
                hooks.signal,
            );
            if (!res.ok) {
                throw new Error(`rsjs query fetch failed: ${res.status}`);
            }
            if (options.stream_text) {
                if (String(cfg.response || 'json').toLowerCase() !== 'text') {
                    throw new Error(
                        'rsjs query stream_text requires FetchRequest.response = ResponseKind::Text',
                    );
                }
                return await read_response_text_stream(
                    res,
                    hooks.on_stream_text,
                    hooks.on_progress,
                );
            }
            if (options.download_progress) {
                if (String(cfg.response || 'json').toLowerCase() === 'empty') {
                    if (hooks.on_progress) hooks.on_progress(0, parse_content_length(res));
                    return null;
                }
                const bytes = await read_response_bytes_with_progress(res, hooks.on_progress);
                return decode_response_bytes(bytes, cfg.response);
            }
            return await decode_response(res, cfg.response);
        } catch (e) {
            if (hooks.signal?.aborted) throw e;
            if (attempts >= max) throw e;
            await new Promise((r) => setTimeout(r, delay_ms));
            // Exponential backoff capped at 30s.
            delay_ms = Math.min(delay_ms * 2, 30_000);
        }
    }
}

function reset_download_progress(entry) {
    if (!entry.options || !entry.options.download_progress) return;
    entry.download_loaded = 0;
    entry.download_total = null;
    entry.download_percent = 0;
}

function set_download_progress(entry, loaded, total) {
    entry.download_loaded = Number.isFinite(loaded) && loaded > 0 ? loaded : 0;
    entry.download_total = Number.isFinite(total) && total >= 0 ? total : null;
    entry.download_percent =
        entry.download_total && entry.download_total > 0
            ? Math.min(1, entry.download_loaded / entry.download_total)
            : 0;
    notify(entry);
}

function reset_stream_text(entry) {
    if (!entry.options || !entry.options.stream_text) return;
    entry.data = '';
}

function set_stream_text(entry, text) {
    entry.data = String(text ?? '');
    notify(entry);
}

function claim_client_query_entry(entry) {
    // Once browser work has touched this cache slot, asynchronously arriving
    // SSR fragments may still improve the page-level seed registry for future
    // consumers but must not roll this live entry backward.
    entry._rsjs_seed_rank = null;
}

/**
 * Begin (or join) a fetch for `entry`. Returns the in-flight
 * promise so callers can await completion if they want to.
 */
function refetch(entry) {
    if (entry.in_flight) return entry.in_flight;
    if (entry.disposed || entry.generation !== QUERY_GENERATION) {
        return Promise.reject(
            new Error(
                'rsjs/query-disposed: this query handle belongs to a disposed page generation',
            ),
        );
    }
    if (entry.fetch_semantics === null) {
        return Promise.reject(
            new Error(
                'rsjs/query-unregistered: this cache entry has no fetch specification',
            ),
        );
    }
    claim_client_query_entry(entry);
    const generation = entry.generation;
    const controller =
        typeof AbortController === 'undefined' ? null : new AbortController();
    entry.in_flight_controller = controller;
    const had_data = entry.data !== undefined;
    entry.status = had_data ? 'refetching' : 'loading';
    reset_download_progress(entry);
    reset_stream_text(entry);
    notify(entry);
    entry.in_flight = execute_fetch(
        entry.fetcher,
        entry.options,
        {
            on_progress:
                entry.options && entry.options.download_progress
                    ? (loaded, total) => set_download_progress(entry, loaded, total)
                    : null,
            on_stream_text:
                entry.options && entry.options.stream_text
                    ? (text) => set_stream_text(entry, text)
                    : null,
            signal: controller?.signal,
        },
    ).then(
        (data) => {
            if (entry.disposed || generation !== QUERY_GENERATION) {
                return data;
            }
            entry.data = data;
            entry.error = undefined;
            entry.status = 'success';
            entry.is_stale = false;
            entry.data_updated_at = Date.now();
            entry.fetch_count += 1;
            entry.in_flight = null;
            entry.in_flight_controller = null;
            notify(entry);
            return data;
        },
        (error) => {
            if (entry.disposed || generation !== QUERY_GENERATION) {
                throw error;
            }
            entry.error = error;
            entry.status = 'error';
            entry.error_updated_at = Date.now();
            entry.fetch_count += 1;
            entry.in_flight = null;
            entry.in_flight_controller = null;
            notify(entry);
            throw error;
        },
    );
    return entry.in_flight;
}

/**
 * Mark an entry stale; schedule a refetch on the next read or
 * subscriber registration. M8 collapses the simple "invalidate"
 * model into "mark stale + refetch immediately if any subscribers
 * exist." Mutations (M9) flow through the same hook.
 */
function invalidate(entry) {
    claim_client_query_entry(entry);
    entry.is_stale = true;
    if (entry.fetch_semantics !== null && entry.subscribers.size > 0) {
        refetch_in_background(entry);
    }
}

function observer_is_stale(entry, observer) {
    if (entry.is_stale) return true;
    const stale_ms = observer?.options?.stale_time_ms;
    if (stale_ms == null || !entry.data_updated_at) return false;
    return Date.now() - entry.data_updated_at >= stale_ms;
}

function observer_needs_fetch(entry, observer) {
    return entry.data === undefined || observer_is_stale(entry, observer);
}

/**
 * Policy-driven refetches surface failures through query state/subscribers.
 * Attach a rejection handler as well so a failed interval/focus/invalidation
 * request does not become an unhandled promise rejection. Explicit
 * `handle.refetch()` calls still return the rejecting promise to their caller.
 */
function refetch_in_background(entry) {
    const request = refetch(entry);
    if (request && typeof request.catch === 'function') {
        request.catch(() => {});
    }
    return request;
}

function refetch_stale_if_observed(entry, observer) {
    if (
        entry.fetch_semantics !== null &&
        entry.subscribers.size > 0 &&
        (observer === undefined
            ? entry.is_stale
            : observer.subscribers.size > 0 &&
              observer.entry === entry &&
              observer_needs_fetch(entry, observer))
    ) {
        refetch_in_background(entry);
    }
}

function active_query_observers(entry) {
    return Array.from(entry.active_observers).filter(
        (observer) =>
            observer.entry === entry &&
            !observer.disposed &&
            observer.subscribers.size > 0,
    );
}

function stop_query_entry_policy(entry) {
    if (entry.policy_interval_handle != null) {
        clearInterval(entry.policy_interval_handle);
        entry.policy_interval_handle = null;
    }
    entry.policy_interval_ms = null;
    if (entry.policy_focus_listener != null) {
        if (
            entry.policy_focus_target != null &&
            typeof entry.policy_focus_target.removeEventListener === 'function'
        ) {
            entry.policy_focus_target.removeEventListener(
                'focus',
                entry.policy_focus_listener,
            );
        }
        entry.policy_focus_listener = null;
        entry.policy_focus_target = null;
    }
    if (entry.policy_reconnect_listener != null) {
        if (
            entry.policy_reconnect_target != null &&
            typeof entry.policy_reconnect_target.removeEventListener === 'function'
        ) {
            entry.policy_reconnect_target.removeEventListener(
                'online',
                entry.policy_reconnect_listener,
            );
        }
        entry.policy_reconnect_listener = null;
        entry.policy_reconnect_target = null;
    }
}

/**
 * Combine active observer policy without choosing a declaration-order winner.
 * One entry timer uses the minimum positive interval. Focus and reconnect each
 * use one listener and refetch when the union of enabled observers contains a
 * stale observer.
 */
function recompute_query_entry_policy(entry) {
    const observers = active_query_observers(entry);
    if (
        entry.disposed ||
        entry.fetch_semantics === null ||
        observers.length === 0
    ) {
        stop_query_entry_policy(entry);
        return;
    }

    const intervals = observers
        .map((observer) => observer.options.interval_ms)
        .filter((interval) => interval != null && interval > 0);
    const minimum_interval =
        intervals.length === 0 ? null : Math.min(...intervals);
    if (minimum_interval !== entry.policy_interval_ms) {
        if (entry.policy_interval_handle != null) {
            clearInterval(entry.policy_interval_handle);
            entry.policy_interval_handle = null;
        }
        entry.policy_interval_ms = minimum_interval;
        if (minimum_interval != null) {
            entry.policy_interval_handle = setInterval(() => {
                if (
                    entry.disposed ||
                    entry.in_flight ||
                    active_query_observers(entry).length === 0
                ) {
                    return;
                }
                refetch_in_background(entry);
            }, minimum_interval);
        }
    }

    const focus_enabled = observers.some(
        (observer) => observer.options.refetch_on_focus,
    );
    if (
        focus_enabled &&
        entry.policy_focus_listener == null &&
        typeof window !== 'undefined' &&
        typeof window.addEventListener === 'function'
    ) {
        entry.policy_focus_target = window;
        entry.policy_focus_listener = () => {
            const should_refetch = active_query_observers(entry).some(
                (observer) =>
                    observer.options.refetch_on_focus &&
                    observer_is_stale(entry, observer),
            );
            if (should_refetch) refetch_in_background(entry);
        };
        entry.policy_focus_target.addEventListener(
            'focus',
            entry.policy_focus_listener,
        );
    } else if (!focus_enabled && entry.policy_focus_listener != null) {
        entry.policy_focus_target?.removeEventListener(
            'focus',
            entry.policy_focus_listener,
        );
        entry.policy_focus_listener = null;
        entry.policy_focus_target = null;
    }

    const reconnect_enabled = observers.some(
        (observer) => observer.options.refetch_on_reconnect,
    );
    if (
        reconnect_enabled &&
        entry.policy_reconnect_listener == null &&
        typeof window !== 'undefined' &&
        typeof window.addEventListener === 'function'
    ) {
        entry.policy_reconnect_target = window;
        entry.policy_reconnect_listener = () => {
            const should_refetch = active_query_observers(entry).some(
                (observer) =>
                    observer.options.refetch_on_reconnect &&
                    observer_is_stale(entry, observer),
            );
            if (should_refetch) refetch_in_background(entry);
        };
        entry.policy_reconnect_target.addEventListener(
            'online',
            entry.policy_reconnect_listener,
        );
    } else if (
        !reconnect_enabled &&
        entry.policy_reconnect_listener != null
    ) {
        entry.policy_reconnect_target?.removeEventListener(
            'online',
            entry.policy_reconnect_listener,
        );
        entry.policy_reconnect_listener = null;
        entry.policy_reconnect_target = null;
    }
}

// ---------------------------------------------------------------------------
// Per-island registration (called by emit_js output)
// ---------------------------------------------------------------------------

/**
 * Register an island-declared query. Looks up (or creates) the
 * shared cache entry, seeds it from the page-level query table by
 * structural key, creates one observer with the configured scheduling
 * policy, and returns a typed handle the island's bindings read from.
 * Active handles contribute to one entry-level scheduler; final observer
 * removal stops scheduling without discarding the generation-local cache.
 * Runtime-initialized queries may carry only their effective structural key
 * in the adjacent blob when the key depends on server locals that static
 * island JS cannot evaluate; older blobs that still carry full data remain a
 * fallback.
 *
 * Multiple islands declaring the same key share one entry and in-flight
 * fetch. Request/retry/decoder semantics must agree. Freshness, interval,
 * focus, and reconnect behavior are observer-local and may differ between
 * handles.
 */
function resolve_query_registration(
    { island, query_id, key, fetcher, options, root, live_key = false },
    rebinding = false,
) {
    if (!root) {
        seed_dehydrated_state();
    }

    if (rebinding && key == null) {
        throw new Error(
            'rsjs/query-rebind-key: a live query rebind requires its current structural key',
        );
    }

    const static_seed = page_dehydrated_seed_for_key(key);
    const static_initial = static_seed?.state;
    // Adjacent dehydration describes only the SSR key. A live rebind must
    // never move that seed to a different identity.
    const adjacent_initial = rebinding
        ? undefined
        : dehydrated_state_for(island, query_id, root);
    // A late-hydrating live query may already evaluate to a key different
    // from the adjacent SSR locator. Preserve that SSR value under its
    // original identity, but never attach the current request snapshot to
    // the old entry. The generated live registration binds directly to
    // `key`; its immediate watcher then becomes a same-key no-op unless the
    // dependency changes during initialization.
    if (
        live_key &&
        !rebinding &&
        adjacent_initial &&
        adjacent_initial.key !== undefined
    ) {
        install_page_query_seed(
            adjacent_initial.key,
            adjacent_initial,
            normalized_seed_rank(adjacent_initial.rank, adjacent_initial),
        );
    }
    const locator = static_initial || (live_key ? undefined : adjacent_initial);
    const effective_key = rebinding || live_key
        ? key
        : locator && locator.key !== undefined
          ? locator.key
          : key;
    const initial_seed = page_dehydrated_seed_for_key(effective_key) || static_seed;
    const initial = initial_seed?.state || (live_key ? undefined : adjacent_initial);
    const skey = stringify_key(effective_key);
    const fetch_options = query_fetch_options(options);
    const observer_options = query_observer_options(options);
    const fetch_semantics = query_fetch_semantics(fetcher, fetch_options);
    let entry = CACHE.get(skey);
    if (entry && entry.fetch_semantics !== null) {
        // One cache identity has one request/decoder contract. Scheduling
        // options are intentionally excluded: every observer owns those.
        if (!shallow_eq_options(entry.fetch_semantics, fetch_semantics)) {
            throw new Error(
                `rsjs.query: key ${skey} has incompatible fetch semantics. ` +
                    `Observers may vary stale_time_ms, interval_ms, ` +
                    `refetch_on_focus, and refetch_on_reconnect, but a shared ` +
                    `key must use one fetcher/retry/decoder contract.`,
            );
        }
    }
    // Split "create if missing" from "install if no fetcher yet" so a
    // stub entry created by an earlier `rsjs.query(key)` call (cross-
    // framework consumer running before our register_query) gets
    // upgraded with the fetcher / options / dehydrated seed instead of
    // staying permanently unfetchable. Mirrors the mutation path in
    // register_mutation below.
    if (!entry) {
        entry = make_entry(undefined);
        // Stash the raw structural key so M9's invalidation walker
        // can prefix-match without re-parsing the stringified form.
        entry._raw_key = effective_key;
        CACHE.set(skey, entry);
    }
    if (entry.fetch_semantics === null) {
        // Pull the dehydrated state for this (island, query_id) and
        // seed any fields the stub entry didn't already populate. We
        // only overwrite when the stub has nothing useful: a real
        // value (set via `set_data` before registration, say) wins
        // over the SSR snapshot.
        if (initial_seed !== undefined) {
            reconcile_ranked_ssr_seed(skey, initial_seed.state, initial_seed.rank);
        } else if (initial !== undefined) {
            const seeded = entry.data === undefined && initial.data !== undefined;
            if (seeded) {
                entry.data = initial.data;
                entry.status = 'success';
                entry.fetch_count = 1;
            }
            if (!entry.data_updated_at && initial.data_updated_at) {
                entry.data_updated_at = initial.data_updated_at;
            }
            if (!entry.error_updated_at && initial.error_updated_at) {
                entry.error_updated_at = initial.error_updated_at;
            }
            // Round-3 review #1: a stub entry created by an earlier
            // `rsjs.query(key).subscribe(cb)` call has subscribers
            // attached before this registration ran. Notify them so
            // the seed value lands instead of waiting for the next
            // refetch / mutation.
            if (seeded && entry.subscribers.size > 0) {
                notify(entry);
            }
        }
        entry.fetcher = fetcher;
        entry.options = fetch_options;
        entry.fetch_semantics = fetch_semantics;

        for (const observer of entry.active_observers) {
            refetch_stale_if_observed(entry, observer);
        }
        recompute_query_entry_policy(entry);
        // `stale_time_ms` is computed on demand in `is_stale()` (see
        // make_handle below) rather than via a one-shot setTimeout at
        // registration. The earlier timer-based approach failed to
        // re-arm: after firing once, `entry.is_stale` stayed true
        // forever and subsequent refetches (which reset is_stale to
        // false) were not retracked. The on-demand model compares
        // `Date.now() - entry.data_updated_at` against the threshold
        // every read, so it naturally re-arms after every successful
        // refetch.
    }

    return { entry, observer_options };
}

export function register_query(options) {
    const { entry, observer_options } = resolve_query_registration(options);
    return make_handle(entry, observer_options);
}

/**
 * Build the typed accessor surface for an observer. Accessors resolve its
 * mutable current entry so a live-key rebind does not replace the handle.
 */
function make_handle(entry, options = query_observer_options()) {
    const observer = {
        entry,
        options,
        subscribers: new Set(),
        disposed: false,
    };
    entry.observers.add(observer);
    const handle = {
        data: () => observer.entry.data,
        status: () => observer.entry.status,
        is_loading: () => observer.entry.status === 'loading',
        is_success: () => observer.entry.status === 'success',
        is_error: () => observer.entry.status === 'error',
        is_refetching: () => observer.entry.status === 'refetching',
        // Staleness is computed on demand: an entry is stale if its
        // `is_stale` flag has been imperatively set (e.g. via
        // `invalidate()`), or if its `stale_time_ms` policy threshold
        // has elapsed since the last successful fetch. Computing this
        // on every read (rather than via a one-shot timer) means the
        // staleness window naturally re-arms after every refetch.
        is_stale: () => observer_is_stale(observer.entry, observer),
        error: () => observer.entry.error,
        data_updated_at: () => observer.entry.data_updated_at,
        error_updated_at: () => observer.entry.error_updated_at,
        fetch_count: () => observer.entry.fetch_count,
        download_loaded: () => observer.entry.download_loaded,
        download_total: () => observer.entry.download_total,
        download_percent: () => observer.entry.download_percent,
        // Imperative actions (M8). Mutations land in M9.
        refetch: () => refetch(observer.entry),
        invalidate: () => invalidate(observer.entry),
        // Subscription surface used by bind_text / bind_conditional
        // and by cross-framework consumers.
        subscribe: (cb) => {
            if (observer.disposed) {
                throw new Error(
                    'rsjs/query-disposed: cannot subscribe a handle from a disposed page generation',
                );
            }
            let active = true;
            const subscription = (value) => {
                if (active) cb(value);
            };
            const current_entry = observer.entry;
            current_entry.subscribers.add(subscription);
            observer.subscribers.add(subscription);
            current_entry.active_observers.add(observer);
            recompute_query_entry_policy(current_entry);
            refetch_stale_if_observed(current_entry, observer);
            return () => {
                if (!active) return;
                active = false;
                const subscribed_entry = observer.entry;
                subscribed_entry.subscribers.delete(subscription);
                observer.subscribers.delete(subscription);
                if (observer.subscribers.size === 0) {
                    subscribed_entry.active_observers.delete(observer);
                }
                recompute_query_entry_policy(subscribed_entry);
            };
        },
        getSnapshot: () => observer.entry.data,
    };
    QUERY_HANDLE_OBSERVERS.set(handle, observer);
    return handle;
}

/**
 * Dispose one query registration observer without evicting its page-scoped
 * cache entry.
 *
 * Generated component fragments call this when their mounted instance leaves
 * a reactive subtree. Every subscription owned by the handle is removed from
 * its current entry, observer-local policy is recomputed, and later
 * subscribe/rebind attempts fail through the ordinary disposed-handle checks.
 * Cache data and in-flight work remain available to other observers until the
 * page-generation disposer runs.
 *
 * Returns true only for the first disposal. Keeping the WeakMap record makes a
 * repeated call, including one after page teardown, an idempotent false.
 */
export function dispose_query(handle) {
    const observer = QUERY_HANDLE_OBSERVERS.get(handle);
    if (!observer) {
        throw new TypeError(
            'rsjs/query-dispose-handle: dispose_query requires a query handle from this runtime',
        );
    }
    if (observer.disposed) {
        return false;
    }

    observer.disposed = true;
    const entry = observer.entry;
    entry.active_observers.delete(observer);
    entry.observers.delete(observer);
    for (const subscription of observer.subscribers) {
        entry.subscribers.delete(subscription);
    }
    observer.subscribers.clear();
    recompute_query_entry_policy(entry);
    return true;
}

/**
 * Rebind a generated live-query handle to a newly evaluated structural key.
 * The handle object remains stable. Existing subscriptions move atomically,
 * old in-flight work remains owned by the old cache entry, and every
 * imperative method resolves the observer's current entry at call time.
 *
 * This is an internal compiler/runtime primitive. Generated code is
 * responsible for coalescing synchronous dependency writes before calling it.
 */
export function rebind_query(handle, options) {
    const observer = QUERY_HANDLE_OBSERVERS.get(handle);
    if (!observer || observer.disposed) {
        throw new Error(
            'rsjs/query-rebind-handle: rebind_query requires a live query handle from this page generation',
        );
    }

    const { entry: next_entry, observer_options } =
        resolve_query_registration(options, true);
    const previous_entry = observer.entry;
    observer.options = observer_options;
    if (next_entry === previous_entry) {
        recompute_query_entry_policy(previous_entry);
        refetch_stale_if_observed(previous_entry, observer);
        return false;
    }

    previous_entry.active_observers.delete(observer);
    previous_entry.observers.delete(observer);
    for (const subscription of observer.subscribers) {
        previous_entry.subscribers.delete(subscription);
    }
    recompute_query_entry_policy(previous_entry);

    observer.entry = next_entry;
    next_entry.observers.add(observer);
    const active = observer.subscribers.size > 0;
    if (active && observer_needs_fetch(next_entry, observer)) {
        // Start/join before attaching the moved callbacks. Existing observers
        // see the entry transition; the moved observer receives one explicit
        // notification for the identity change below.
        refetch_in_background(next_entry);
    }
    for (const subscription of observer.subscribers) {
        next_entry.subscribers.add(subscription);
    }
    if (active) next_entry.active_observers.add(observer);
    recompute_query_entry_policy(next_entry);

    if (active) {
        for (const subscription of Array.from(observer.subscribers)) {
            subscription(next_entry.data);
        }
    }
    return true;
}

/**
 * Dispose the complete page-generation query/mutation registry.
 *
 * Cache entries deliberately survive the departure of their final observer so
 * a later owner in the same page generation can reuse data and an already
 * started request may still populate that entry. This explicit generation
 * boundary is the deterministic retention/GC point: it cancels scheduling and
 * query requests, invalidates old handles, and clears all page-owned state.
 */
export function dispose_page_query_state() {
    QUERY_GENERATION += 1;
    for (const entry of CACHE.values()) {
        entry.disposed = true;
        stop_query_entry_policy(entry);
        if (entry.in_flight_controller != null) {
            entry.in_flight_controller.abort();
            entry.in_flight_controller = null;
        }
        for (const observer of entry.observers) {
            observer.disposed = true;
            observer.subscribers.clear();
        }
        entry.observers.clear();
        entry.active_observers.clear();
        entry.subscribers.clear();
        entry.in_flight = null;
    }
    CACHE.clear();
    PAGE_DEHYDRATED.clear();
    DEHYDRATED.clear();
    MUTATION_METADATA.clear();
    PAGE_DEHYDRATED_TABLE_SOURCES = new WeakMap();

    // Mutation requests are not currently abortable, but clearing their
    // subscribers/registry generation-fences any late completion from the next
    // page. Query request cancellation above is exact.
    for (const entry of MUTATIONS.values()) {
        entry.subscribers.clear();
    }
    MUTATIONS.clear();
}

// ---------------------------------------------------------------------------
// Cross-framework public API
// ---------------------------------------------------------------------------

/**
 * Public typed surface for cross-framework consumers (React via
 * useSyncExternalStore, Vue, Svelte, vanilla). See PLAN § Cross-
 * framework interop.
 *
 * Stability: this is an M8 stable public contract; breaking it
 * requires a major version bump.
 */
export const rsjs = {
    /** Look up (or create-if-missing) a handle for the given key. */
    query(key) {
        seed_dehydrated_state();
        const initial_seed = page_dehydrated_seed_for_key(key);
        const skey = stringify_key(key);
        let entry = CACHE.get(skey);
        if (!entry) {
            entry = make_entry(undefined);
            // Round-3 review #2: stash the raw structural key so the
            // M9 invalidation walker can prefix-match without
            // reparsing. Mirrors register_query / register_mutation.
            entry._raw_key = key;
            CACHE.set(skey, entry);
            if (initial_seed !== undefined) {
                reconcile_ranked_ssr_seed(skey, initial_seed.state, initial_seed.rank);
            }
        }
        return make_handle(entry);
    },

    // Projection helpers used by emit_js's lowering. Exposed on the
    // public namespace so consumers writing custom JS can reuse the
    // same shape-tolerant primitives (e.g. when bridging through
    // useSyncExternalStore).

    /**
     * Walk `target` along `path` (an array of segments). Returns
     * `null` on any null/undefined / missing-key step.
     */
    __project(target, path) {
        let v = target;
        for (const seg of path) {
            if (v == null) return null;
            v = v[seg];
        }
        return v ?? null;
    },

    /**
     * Cast helpers: each returns the projected value if and only if
     * its JS shape matches the asserted type. Otherwise returns the
     * type-appropriate empty per PLAN § Projection over query state.
     */
    __as_str(v) {
        return typeof v === 'string' ? v : '';
    },
    __as_f64(v) {
        return typeof v === 'number' ? v : 0.0;
    },
    __as_i64(v) {
        let integer;
        if (typeof v === 'bigint') {
            integer = v;
        } else if (typeof v === 'number' && Number.isSafeInteger(v)) {
            integer = BigInt(v);
        } else if (typeof v === 'string' && /^(?:0|-?[1-9][0-9]*)$/.test(v)) {
            integer = BigInt(v);
        } else {
            return 0n;
        }
        return integer >= -9223372036854775808n && integer <= 9223372036854775807n
            ? integer
            : 0n;
    },
    __as_bool(v) {
        return typeof v === 'boolean' ? v : false;
    },
    __as_array(v) {
        return Array.isArray(v) ? v : [];
    },
    __as_object(v) {
        return v != null && typeof v === 'object' && !Array.isArray(v)
            ? v
            : {};
    },

    /**
     * Predicates. Match the PLAN's narrow-predicate vocabulary;
     * never JS truthiness.
     */
    __is_present(v) {
        return v != null;
    },
    __eq(target, value) {
        return deep_eq(target, value);
    },
    __neq(target, value) {
        return !deep_eq(target, value);
    },
    __coalesce(target, fallback) {
        return target == null ? fallback : target;
    },
};

/**
 * Structural deep-equality used by the `eq` / `neq` predicates.
 * Compares numbers / strings / bools by value; arrays and objects
 * by deep walk.
 *
 * Round-3 review #7: cycle detection. A naive deep walk recurses
 * forever on user data that contains back-references (caches, tree
 * structures with parent pointers). The `seen` WeakSet records every
 * object/array we are currently inspecting; revisiting one returns
 * `true` (assume equal — conservative; throwing would be hostile to
 * cache-style structures that legitimately emit cycles). Entries are
 * removed on exit so independent siblings aren't penalised.
 */
function deep_eq(a, b, seen) {
    if (Object.is(a, b)) return true;
    // A BigInt and a Number denoting the same integer are equal for rsjs's
    // purposes: shared fields are representation-pinned by the page seed, and
    // `rsjs.__eq(5n, 5)` must agree with the compiled `num_eq(5n, 5)`.
    if (
        (typeof a === 'bigint' && typeof b === 'number') ||
        (typeof a === 'number' && typeof b === 'bigint')
    ) {
        // eslint-disable-next-line eqeqeq
        return a == b;
    }
    if (typeof a !== typeof b) return false;
    if (a == null || b == null) return false;
    if (Array.isArray(a) && Array.isArray(b)) {
        if (a.length !== b.length) return false;
        const s = seen || new WeakSet();
        if (s.has(a) || s.has(b)) return true;
        s.add(a);
        s.add(b);
        try {
            for (let i = 0; i < a.length; i += 1) {
                if (!deep_eq(a[i], b[i], s)) return false;
            }
            return true;
        } finally {
            s.delete(a);
            s.delete(b);
        }
    }
    if (typeof a === 'object' && typeof b === 'object') {
        const ka = Object.keys(a);
        const kb = Object.keys(b);
        if (ka.length !== kb.length) return false;
        const s = seen || new WeakSet();
        if (s.has(a) || s.has(b)) return true;
        s.add(a);
        s.add(b);
        try {
            for (const k of ka) {
                if (!deep_eq(a[k], b[k], s)) return false;
            }
            return true;
        } finally {
            s.delete(a);
            s.delete(b);
        }
    }
    return false;
}

// ---------------------------------------------------------------------------
// M9 hooks (left in place so the mutation runtime can plug in
// without re-opening this module)
// ---------------------------------------------------------------------------

/**
 * Look up an existing entry by key. Returns undefined if the key
 * has no cache entry. Used by the M9 mutation runtime to invalidate
 * affected keys without going through `rsjs.query()` (which would
 * lazily create a stub entry).
 */
export function get_entry(key) {
    return CACHE.get(stringify_key(key));
}

/**
 * Iterate every cache entry. **Internal**: used by the M9 mutation
 * runtime's `invalidates: [keys]` walker. Not part of the stable
 * public surface; entries are yielded by reference so cross-
 * framework consumers could mutate them. The double-underscore
 * prefix marks the bridge as runtime-internal.
 */
function __iter_entries() {
    return CACHE.entries();
}

/**
 * Imperatively replace an entry's data. Used by M9 optimistic
 * updates: a mutation's `optimistic` block calls `set_data(key,
 * next)` synchronously to snap the UI into the optimistic shape,
 * then rolls back on rejection.
 *
 * Cost: O(structure) `deep_eq` walk on every write. For large query
 * results (deeply-nested objects / long arrays) this is non-trivial.
 * The trade-off is avoiding a duplicate `notify` on the rare
 * optimistic-rollback equality case (rollback to a value that
 * happens to match the post-mutation value). v0.2 may switch to
 * `Object.is` if subscribers prove tolerant of the occasional
 * double-notify; the structural compare is the safer default for v0.1
 * because re-rendering large query results can be expensive on the
 * subscriber side.
 */
export function set_data(key, data) {
    const skey = stringify_key(key);
    let entry = CACHE.get(skey);
    if (!entry) {
        entry = make_entry({ data, data_updated_at: Date.now() });
        claim_client_query_entry(entry);
        CACHE.set(skey, entry);
        notify(entry);
        return;
    }
    claim_client_query_entry(entry);
    // Short-circuit when the new value is structurally equal to the
    // previous value. This matters most for optimistic-update
    // rollback: if `prev === optimistic_value` (e.g. the user set
    // the cart to its existing contents), the rollback path used to
    // fire two notifications back-to-back. Subscribers that drive
    // expensive renders would re-paint twice for no net change.
    if (deep_eq(entry.data, data)) {
        // Still bump status / timestamp so the entry no longer reads
        // as `loading`, but skip the subscriber walk.
        entry.status = 'success';
        entry.data_updated_at = Date.now();
        return;
    }
    entry.data = data;
    entry.status = 'success';
    entry.data_updated_at = Date.now();
    notify(entry);
}

// ---------------------------------------------------------------------------
// M9: mutations + cross-island invalidation
// ---------------------------------------------------------------------------

// Page-scoped mutation registry. One slot per unique stringified key.
// Like the query cache, three islands declaring the same mutation
// key share one state slot; firing `run()` from any of them updates
// every subscriber.
const MUTATIONS = new Map();

/**
 * State machine slot for a single mutation. Tracks the four
 * canonical states (`idle` / `pending` / `success` / `error`) plus
 * the most recent data / error payload.
 */
function make_mutation_entry() {
    return {
        // status: 'idle' | 'pending' | 'success' | 'error'
        status: 'idle',
        data: undefined,
        error: undefined,
        subscribers: new Set(),
        // Configured policy + runner (set on first registration).
        runner: null,
        invalidates: null,
        options: null,
        // Concurrent run() coalescence: while a run is in flight,
        // every additional run() call returns the same Promise.
        in_flight_promise: null,
    };
}

/**
 * Notify every subscriber of a mutation entry that its state changed.
 * Each subscriber receives the full state object so React's
 * useSyncExternalStore (and equivalent) can shallow-compare.
 */
function notify_mutation(entry) {
    const snapshot = {
        status: entry.status,
        data: entry.data,
        error: entry.error,
    };
    const subs = Array.from(entry.subscribers);
    for (const fn of subs) {
        try {
            fn(snapshot);
        } catch (e) {
            console.warn('rsjs: mutation subscriber threw', e);
        }
    }
}

function parse_runner(runner) {
    return parse_request_config(runner, 'POST', 'rsjs.register_mutation');
}

/**
 * Execute a mutation with the configured retry policy. Resolves to
 * the decoded response body on success, rejects (after the retry
 * budget) on failure.
 */
async function execute_mutation(runner, args, options) {
    const cfg = parse_runner(runner);
    let attempts = 0;
    const max = (options.retry_count || 0) + 1;
    let delay_ms = 250;
    while (attempts < max) {
        attempts += 1;
        try {
            // Round-4 review #2: encode `args` verbatim as a single
            // argument when no fixed request body is configured. The
            // previous shape collapsed `m.run([1,2,3])` and
            // `m.run(1,2,3)` to the same body, contradicting the .d.ts
            // which types `run(args: Args)` as a single value.
            const init = build_fetch_init(cfg, args);
            const res = await fetch_with_timeout(cfg.url, init, cfg.timeout_ms);
            if (!res.ok) {
                throw new Error(`rsjs mutation failed: ${res.status}`);
            }
            return await decode_response(res, cfg.response);
        } catch (e) {
            if (attempts >= max) throw e;
            await new Promise((r) => setTimeout(r, delay_ms));
            delay_ms = Math.min(delay_ms * 2, 30_000);
        }
    }
}

/**
 * Walk every cache entry and invalidate any whose key matches the
 * mutation's `invalidates: QueryKey[]` list. v0.1 matching rule:
 * an entry matches a target key iff the target is a prefix of the
 * entry's key (so target `["cart"]` matches `["cart", userId]`).
 * Bare strings are treated as a single-element prefix.
 *
 * `strategy` is `'refetch'` (default) or `'reset'`. `'reset'` snaps
 * the entry to `idle` and clears its data; `'refetch'` re-runs the
 * client fetcher.
 *
 * Edge cases:
 * - A target key with no matching cache entry: silently no-op
 *   (the user may have invalidated a key no live island subscribes
 *   to; we don't want a console warning to flood the page).
 * - A matching entry with no subscribers: still invalidated, but
 *   `refetch` defers until a subscriber registers (the existing
 *   `invalidate(entry)` helper already encodes this).
 */
function invalidate_keys(targets, strategy) {
    const target_keys = targets.map((t) => {
        if (typeof t === 'string') return [t];
        if (Array.isArray(t)) return t;
        return [t];
    });
    for (const [skey, entry] of __iter_entries()) {
        // Reverse-engineer the entry's structural key from its
        // stringified form. We cached the raw key on the entry the
        // first time it was registered; fall back to JSON.parse for
        // entries we have only the stringified form for.
        const entry_key = entry._raw_key ?? safe_parse_key(skey);
        if (!entry_key) continue;
        if (any_prefix_matches(target_keys, entry_key)) {
            if (strategy === 'reset') {
                claim_client_query_entry(entry);
                entry.data = undefined;
                entry.status = 'idle';
                entry.fetch_count = 0;
                notify(entry);
            } else {
                invalidate(entry);
            }
        }
    }
}

/**
 * Check whether `entry_key` starts with any of the `targets`
 * (each itself an array of segments).
 */
function any_prefix_matches(targets, entry_key) {
    const entry_arr = Array.isArray(entry_key) ? entry_key : [entry_key];
    for (const target of targets) {
        const t_arr = Array.isArray(target) ? target : [target];
        if (t_arr.length > entry_arr.length) continue;
        let ok = true;
        for (let i = 0; i < t_arr.length; i += 1) {
            if (!deep_eq(t_arr[i], entry_arr[i])) {
                ok = false;
                break;
            }
        }
        if (ok) return true;
    }
    return false;
}

/**
 * Best-effort recovery of a structural key from its stringified
 * form. The stringify_key helper produces JSON-ish output for the
 * common (array-of-primitives) case; we lean on JSON.parse here.
 */
function safe_parse_key(skey) {
    try {
        return JSON.parse(skey);
    } catch (_) {
        return null;
    }
}

/**
 * Register an island-declared mutation. Looks up (or creates) the
 * shared mutation slot, attaches the runner config + policy, and
 * returns a typed handle the island bindings (and cross-framework
 * consumers) read from. Generated islands pass their root so static
 * server-emitted mutation metadata can override the fallback JS
 * fields without losing signal-backed fields the server blob cannot
 * encode.
 *
 * Multiple islands declaring the same key share the same state slot;
 * only the first registration installs the runner / policy.
 */
export function register_mutation({ island, mutation_id, key, runner, invalidates, options, root }) {
    const metadata = mutation_metadata_for(island, mutation_id, root);
    const effective_key = has_own(metadata, 'key') ? metadata.key : key;
    const effective_runner = has_own(metadata, 'runner') ? metadata.runner : runner;
    const effective_invalidates = has_own(metadata, 'invalidates')
        ? metadata.invalidates
        : invalidates;
    const effective_options = has_own(metadata, 'options') ? metadata.options : options;

    const skey = stringify_key(effective_key);
    let entry = MUTATIONS.get(skey);
    if (entry && entry.runner) {
        // Same cross-island contract as register_query: islands
        // sharing a mutation key must agree on the runner +
        // invalidates + options. Mismatches almost always indicate a
        // bug (e.g. two islands targeting different endpoints).
        const invalidates_eq = deep_eq(entry.invalidates || [], effective_invalidates || []);
        if (
            !shallow_eq_fetcher(entry.runner, effective_runner) ||
            !invalidates_eq ||
            !shallow_eq_options(entry.options, effective_options || {})
        ) {
            // Round-3 review #9: escalate to console.error (see the
            // matching note in register_query above).
            console.error(
                `rsjs.mutation: key ${skey} registered twice with different runner/invalidates/options. ` +
                    `Keeping first registration; second registration ignored. ` +
                    `(Two islands sharing a mutation key must use identical runner + invalidates + options.)`,
            );
        }
    }
    if (!entry) {
        entry = make_mutation_entry();
        entry._raw_key = effective_key;
        MUTATIONS.set(skey, entry);
    }
    // First-registration-wins for the runner config. Subsequent
    // registrations are a no-op (the cross-island contract: every
    // island agrees on the runner, like every island agrees on the
    // query fetcher).
    if (!entry.runner) {
        entry.runner = effective_runner;
        entry.invalidates = effective_invalidates || [];
        entry.options = effective_options || {};
    }
    // Touch the island id so future debug surfaces (e.g. devtools)
    // can correlate a mutation back to the islands that declared it.
    // Round-3 review #10: v0.1 keeps these as plain Sets that grow
    // monotonically as islands re-register on each navigation; v0.2
    // will switch to a WeakSet of registration tokens so unmounted
    // islands drop out automatically. The leak is bounded by the
    // number of distinct islands * mutations on the page, which is
    // small in practice.
    entry._islands = entry._islands || new Set();
    entry._islands.add(island);
    entry._mutation_ids = entry._mutation_ids || new Set();
    entry._mutation_ids.add(mutation_id);

    return make_mutation_handle(entry);
}

/**
 * Build the typed accessor surface for a mutation entry. Each
 * accessor is a closure over the entry so consumers can call them
 * as methods on the handle.
 */
function make_mutation_handle(entry) {
    return {
        // Public mutation-handle contract.
        run: (args) => run_mutation_entry(entry, args),
        subscribe: (cb) => {
            entry.subscribers.add(cb);
            return () => entry.subscribers.delete(cb);
        },
        getSnapshot: () => ({
            status: entry.status,
            data: entry.data,
            error: entry.error,
        }),
        // Per-accessor methods used by emit_js's MutationRead lowering.
        status: () => entry.status,
        is_idle: () => entry.status === 'idle',
        is_pending: () => entry.status === 'pending',
        is_success: () => entry.status === 'success',
        is_error: () => entry.status === 'error',
        data: () => entry.data,
        error: () => entry.error,
        // Internal: identity hook used by `rsjs.mutation()` for
        // cross-framework consumers (so React's useSyncExternalStore
        // gets a stable reference).
        _entry: entry,
    };
}

/**
 * Fire a mutation. Snapshots the previous data for every
 * invalidation target (so optimistic updates can roll back on
 * rejection), applies the optimistic update if configured, executes
 * the runner, and on success walks the invalidation list.
 *
 * Concurrency contract: concurrent `run()` calls on the same
 * mutation key coalesce to a single in-flight request; every
 * caller receives the same Promise. Without this, two simultaneous
 * `m.run(args)` calls would fire two HTTP requests, only the last
 * `notify_mutation` would stick, and the rollback path could run
 * twice (once per snapshot taken). Coalescing keeps the runtime
 * correct under double-submits.
 */
function run_mutation_entry(entry, args) {
    if (entry.status === 'pending' && entry.in_flight_promise) {
        return entry.in_flight_promise;
    }
    const promise = (async () => {
        // Round-4 review #2: mutation.run takes a single argument. The
        // value is forwarded verbatim to the runner / optimistic update
        // callback / fetch body. Earlier revisions split arrays into
        // positional args, which made `m.run([1,2,3])` and
        // `m.run(1,2,3)` indistinguishable and contradicted the .d.ts.
        // Users who want positional args should pass a tuple/object.
        entry.status = 'pending';
        entry.error = undefined;
        notify_mutation(entry);

        // Snapshot current data for every invalidation target so we can
        // roll back optimistic updates on rejection.
        const rollback = [];
        if (entry.options && entry.options.optimistic_update !== undefined) {
            for (const target of entry.invalidates || []) {
                const tk = typeof target === 'string' ? [target] : target;
                const cache_entry = get_entry(tk);
                if (cache_entry) {
                    rollback.push({ key: tk, prev: cache_entry.data });
                }
            }
            // Apply the optimistic update to every invalidation target.
            // The optimistic value can be a function (called with args)
            // or a literal value the runtime substitutes verbatim.
            const opt = entry.options.optimistic_update;
            const opt_value = typeof opt === 'function' ? opt(args) : opt;
            for (const target of entry.invalidates || []) {
                const tk = typeof target === 'string' ? [target] : target;
                set_data(tk, opt_value);
            }
        }

        try {
            const data = await execute_mutation(entry.runner, args, entry.options || {});
            entry.status = 'success';
            entry.data = data;
            entry.error = undefined;
            notify_mutation(entry);

            // Invalidate / refetch every matching cache entry.
            const strategy =
                (entry.options && entry.options.on_success_invalidate_strategy) || 'refetch';
            if (entry.invalidates && entry.invalidates.length > 0) {
                invalidate_keys(entry.invalidates, strategy);
            }
            return data;
        } catch (err) {
            // Roll back optimistic updates on rejection.
            for (const { key, prev } of rollback) {
                set_data(key, prev);
            }
            entry.status = 'error';
            entry.error = err;
            notify_mutation(entry);
            throw err;
        } finally {
            entry.in_flight_promise = null;
        }
    })();
    entry.in_flight_promise = promise;
    return promise;
}

/**
 * Public mutation lookup. Symmetrical with `rsjs.query(key)`: the
 * caller provides the structural mutation key, the runtime returns
 * a handle whose `run` / `subscribe` / `getSnapshot` methods remain
 * stable for the lifetime of the entry.
 *
 * If no island has registered a mutation under this key yet, the
 * runtime creates a stub state slot in the `idle` state. The first
 * island registration to follow installs the runner config without
 * disturbing existing subscribers.
 */
function rsjs_mutation(key) {
    const skey = stringify_key(key);
    let entry = MUTATIONS.get(skey);
    if (!entry) {
        entry = make_mutation_entry();
        entry._raw_key = key;
        MUTATIONS.set(skey, entry);
    }
    return make_mutation_handle(entry);
}

// Splice the mutation API into the public namespace.
rsjs.mutation = rsjs_mutation;

/**
 * Internal hook called by emit_js's HandlerStatement::RunMutation
 * lowering. Looks up the mutation by id (for the local island) and
 * fires `run()`.
 *
 * Exposed because the per-island JS the emitter produces references
 * the mutation handle by its dense local id (`__rsjs_mutation_<N>`),
 * not by its structural key. The mapping is established at
 * registration time; this helper is the bridge for emit_js's
 * imperative-fire shape.
 */
rsjs.__run_mutation = (handle, args) => {
    if (!handle || typeof handle.run !== 'function') {
        console.warn('rsjs: __run_mutation called with non-handle', handle);
        return;
    }
    return handle.run(args);
};

// The query runtime is optional, so the core page runtime must not statically
// import it. Register the generation disposer through the additive router
// bridge when available. The shared Symbol-backed Set is the order-independent
// fallback when this module evaluates before the core runtime.
function register_query_page_generation_disposer() {
    if (typeof globalThis === 'undefined' || typeof Symbol === 'undefined') {
        return;
    }
    const page_runtime = globalThis[Symbol.for('rsjs.page-runtime')];
    if (typeof page_runtime?.registerPageDisposer === 'function') {
        page_runtime.registerPageDisposer(dispose_page_query_state);
        return;
    }

    const registry_symbol = Symbol.for('rsjs.page-generation-disposers');
    let registry = globalThis[registry_symbol];
    if (!(registry instanceof Set)) {
        registry = new Set();
        globalThis[registry_symbol] = registry;
    }
    registry.add(dispose_page_query_state);
}

register_query_page_generation_disposer();
