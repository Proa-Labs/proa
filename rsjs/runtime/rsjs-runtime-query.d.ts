// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// rsjs query + mutation runtime — public TypeScript declarations.
//
// This file documents the M8 (queries) + M9 (mutations) stable
// public JS contract exposed by `/rsjs-runtime-query.js`. Once
// shipped, breaking these types requires a major version bump
// (cross-framework consumers depend on stability across rsjs
// upgrades).
//
// See PLAN § Cross-framework interop and PLAN § M9 for the design
// rationale.

/**
 * Structural cache key. Matches the rsjs Rust authoring model:
 * `["stock", "AAPL"]`, `["cart", userId]`. Equality is structural,
 * not by reference.
 */
export type QueryKey = ReadonlyArray<unknown>;

/**
 * The state machine for a query. Mirrors PLAN § The state machine.
 *
 * - `idle`: cache entry has never been read (rare with SSR
 *   dehydration; only happens when a non-rsjs JS consumer calls
 *   `rsjs.query(key)` for a key the page never declared).
 * - `loading`: the very first fetch is in flight.
 * - `success`: a fetch has completed; `data` is live.
 * - `error`: the most recent fetch failed; `error` is set.
 * - `refetching`: a *subsequent* fetch is in flight (data exists,
 *   we are reconciling against the upstream).
 */
export type QueryStatus =
    | 'idle'
    | 'loading'
    | 'success'
    | 'error'
    | 'refetching';

/**
 * Typed handle returned by `rsjs.query(key)` or `register_query(...)`.
 * Each method reads the shared cache entry. Handles returned by separate
 * registrations retain their own observer-local freshness and scheduling
 * policy even when they share that entry.
 */
export interface QueryHandle<T = unknown> {
    /** Current cached value, or `undefined` if the cache has never resolved a fetch for this key. */
    data(): T | undefined;
    /** Current state-machine state. */
    status(): QueryStatus;
    /** True iff the very first fetch for this key is in flight. */
    is_loading(): boolean;
    /** True iff the cache holds a successful value. */
    is_success(): boolean;
    /** True iff the most recent fetch failed. */
    is_error(): boolean;
    /** True iff a *subsequent* fetch is in flight (data already exists). */
    is_refetching(): boolean;
    /**
     * True iff the cache entry is currently stale.
     *
     * An entry is stale when either:
     *   - it has been imperatively invalidated via `invalidate()` (or
     *     by a mutation's invalidation list), and no successful
     *     refetch has yet completed, OR
     *   - at least this handle's `stale_time_ms` threshold has
     *     elapsed since the last successful fetch.
     *
     * Staleness is observer-relative and memo on every read; this
     * handle's `stale_time_ms` window re-arms automatically after each
     * successful refetch. Global invalidation remains stale for every
     * observer.
     */
    is_stale(): boolean;
    /** Most recent error value, or `undefined` if `is_error()` is false. */
    error(): unknown | undefined;
    /** Millisecond timestamp of the last successful fetch, or `0` if never. */
    data_updated_at(): number;
    /** Millisecond timestamp of the last failed fetch, or `0` if never. */
    error_updated_at(): number;
    /** Number of fetches issued for this key (success + error). */
    fetch_count(): number;
    /** Bytes downloaded for the current or most recent tracked fetch. */
    download_loaded(): number;
    /** Total response bytes from Content-Length, or `null` when unknown. */
    download_total(): number | null;
    /** Download progress from `0.0` to `1.0` when total bytes are known. */
    download_percent(): number;

    /**
     * Manually trigger a refetch, ignoring cache freshness.
     * Resolves with the new value once the fetch completes.
     */
    refetch(): Promise<T>;

    /**
     * Mark the cache entry globally stale. If any observer is active, starts
     * or joins one immediate refetch; otherwise the next observer attachment
     * performs the fetch.
     */
    invalidate(): void;

    /**
     * Subscribe to cache updates. The callback receives the current
     * `data` value on every successful fetch + every mutation (M9)
     * that touches this key. On a registration handle, the first
     * subscription adds that observer's interval/focus/reconnect policy
     * to the entry-level scheduler; the final unsubscribe removes it and
     * recomputes the remaining policy union. Returns an unsubscribe function.
     *
     * Use this with React's `useSyncExternalStore`, Vue's
     * `watchEffect`, Svelte's `writable` adapter, etc.
     */
    subscribe(callback: (value: T | undefined) => void): () => void;

    /**
     * Synchronous snapshot of the current cached value. Required
     * by `useSyncExternalStore`'s contract; returns the same value
     * `data()` does.
     */
    getSnapshot(): T | undefined;
}

/**
 * Public `rsjs` namespace exposed by the query runtime. Cross-
 * framework consumers reach for this by importing the runtime as
 * an ES module:
 *
 *     import { rsjs } from '/rsjs-runtime-query.js';
 *     const cart = rsjs.query(['cart', userId]);
 *     const off = cart.subscribe((value) => {...});
 *
 * The projection helpers (`__project`, `__as_*`, `__is_present`,
 * `__eq`, `__neq`, `__coalesce`) are also exposed but are
 * implementation hooks for the rsjs JS emitter; they are not
 * intended for direct use and may move into a separate namespace
 * post-1.0. The double-underscore prefix signals their internal
 * status.
 */
export interface RsjsRuntime {
    query<T = unknown>(key: QueryKey): QueryHandle<T>;
    /**
     * M9: look up (or stub-create) a typed mutation handle for a
     * structural mutation key. Symmetrical with `query()`. Stable
     * since M9; same contract-stability discipline as `query()`.
     *
     *     const m = rsjs.mutation(['update_cart', userId]);
     *     await m.run({ items: [...] });
     *     m.subscribe((state) => render(state.status));
     */
    mutation<Args = unknown, Result = unknown>(
        key: MutationKey,
    ): MutationHandle<Args, Result>;
    // Implementation hooks (see note above). Listed for parity
    // with the runtime; not part of the stable public surface.
    __project(target: unknown, path: ReadonlyArray<string>): unknown;
    __as_str(v: unknown): string;
    __as_f64(v: unknown): number;
    __as_i64(v: unknown): bigint;
    __as_bool(v: unknown): boolean;
    __as_array(v: unknown): unknown[];
    __as_object(v: unknown): Record<string, unknown>;
    __is_present(v: unknown): boolean;
    __eq(target: unknown, value: unknown): boolean;
    __neq(target: unknown, value: unknown): boolean;
    __coalesce<T>(target: T | null | undefined, fallback: T): T;
    /**
     * Internal hook called by the rsjs JS emitter's
     * `HandlerStatement::RunMutation` lowering. Not part of the
     * public surface; documented here for parity with the runtime.
     */
    __run_mutation<Args, Result>(
        handle: MutationHandle<Args, Result>,
        args: Args,
    ): Promise<Result> | undefined;
}

export const rsjs: RsjsRuntime;

/**
 * Policy owned by one registered query observer. Different observers for the
 * same structural key may use different values without changing the shared
 * cache entry's request semantics.
 *
 * Selection and notification filtering intentionally have no public v1
 * options yet. They will also be observer-local if/when a concrete API is
 * added.
 */
export interface QueryObserverOptions {
    /** Freshness threshold for this observer. Defaults to `0`. */
    stale_time_ms?: number;
    /** Positive polling interval for this observer, or null/omitted to disable polling. */
    interval_ms?: number | null;
    /** On focus, refetch when this observer considers the entry stale. */
    refetch_on_focus?: boolean;
    /** On reconnect, refetch when this observer considers the entry stale. */
    refetch_on_reconnect?: boolean;
}

/**
 * Behavior intrinsic to one shared cache entry. Every registration for a
 * structural key must agree on these fields as well as the fetcher/request
 * descriptor; a conflict throws during registration.
 */
export interface QueryFetchOptions {
    download_progress?: boolean;
    stream_text?: boolean;
    retry_count?: number;
}

export type QueryRegistrationOptions =
    QueryObserverOptions &
    QueryFetchOptions;

export interface QueryRegistration {
    island: string;
    query_id: number;
    root?: Element | null;
    /** Static key, or null only for initial registration when adjacent SSR metadata supplies it. */
    key: QueryKey | null;
    /**
     * Internal generated-code flag. A live registration preserves adjacent
     * SSR data under the locator's original key while binding this observer
     * directly to the currently evaluated `key`.
     */
    live_key?: boolean;
    fetcher: string | { url: string };
    options?: QueryRegistrationOptions;
}

/**
 * Per-island query registration. The rsjs JS emitter calls this
 * once per declared `query(...)` to wire the cache entry up to
 * the dehydrated initial value, configured fetcher, and policy.
 * Every call creates a distinct observer handle. Active observer policies are
 * combined by one entry-level scheduler: the minimum positive interval and
 * the union of stale-enabled focus/reconnect triggers. Entry state and
 * in-flight requests remain shared by structural key.
 *
 * Observer-policy differences never conflict. A fetcher, decoder, retry,
 * streaming, or other entry-wide semantic mismatch for one key throws before
 * the second observer is installed.
 *
 * **This is an internal contract between the rsjs proc-macro and
 * the runtime.** Consumers writing custom JS should reach for
 * `rsjs.query()` instead. Macro-generated calls pass `root` so
 * dehydration is scoped to that rendered island instance.
 */
export function register_query<T = unknown>(
    opts: QueryRegistration,
): QueryHandle<T>;

/**
 * Dispose one generated query registration observer while retaining its
 * page-scoped cache entry and data for other/currently later owners.
 *
 * This removes every subscription owned by `handle`, recomputes the remaining
 * entry policy, and prevents later `subscribe` / `rebind_query` calls through
 * that handle. Returns `true` on the first disposal and `false` thereafter,
 * including after page-generation teardown.
 *
 * **This is an internal contract between generated owner code and the
 * runtime.** Cache eviction remains the responsibility of
 * `dispose_page_query_state()`.
 */
export function dispose_query<T = unknown>(
    handle: QueryHandle<T>,
): boolean;

/**
 * Internal live-key primitive used by generated owner code after it coalesces
 * synchronous key dependencies. The handle remains stable while its observer
 * moves to the new structural identity. Returns true when identity changed.
 *
 * The SSR seed remains under its original key. `opts.key` must be non-null.
 */
export function rebind_query<T = unknown>(
    handle: QueryHandle<T>,
    opts: QueryRegistration,
): boolean;

/**
 * Dispose all query/mutation state for the current response epoch/navigation
 * generation. This is the deterministic cache-retention boundary: final
 * observer departure stops scheduling but retains data until this function is
 * called. Query requests are aborted and old handles become invalid.
 *
 * The optional query module registers this automatically with the core page
 * runtime. Routers normally call the page bridge's full `disposePage()`; a
 * subtree/container patch deliberately does not invoke it.
 */
export function dispose_page_query_state(): void;

/**
 * Look up an existing entry by key, or `undefined` if no
 * registration has happened. Used by the M9 mutation runtime to
 * invalidate affected keys without lazily creating stub entries.
 */
export function get_entry(key: QueryKey): unknown | undefined;

/**
 * Imperatively replace an entry's data. Used by M9 optimistic
 * updates: a mutation's optimistic block calls `set_data(key,
 * next)` synchronously to snap the UI into the optimistic shape,
 * then rolls back on rejection.
 */
export function set_data(key: QueryKey, data: unknown): void;

// ---------------------------------------------------------------------------
// M9: mutations + cross-island invalidation
// ---------------------------------------------------------------------------

/**
 * Structural mutation key. Same shape as `QueryKey`: an array of
 * primitive parts (`["update_cart", userId]`) compared
 * structurally, not by reference.
 */
export type MutationKey = ReadonlyArray<unknown>;

/**
 * The state machine for a mutation. Mirrors PLAN § M9:
 *
 * - `idle`: no `run()` has fired since registration / last reset.
 * - `pending`: a `run()` is in flight; optimistic data (if any) is
 *   visible to subscribers.
 * - `success`: the most recent `run()` resolved; `data` is the
 *   resolved value.
 * - `error`: the most recent `run()` rejected; `error` is the
 *   thrown / rejected value; optimistic updates have been rolled
 *   back.
 */
export type MutationStatus = 'idle' | 'pending' | 'success' | 'error';

/**
 * The discriminated state object that subscribers and `getSnapshot`
 * consumers see. This is the public mutation-state contract.
 */
export type MutationState<Result = unknown> =
    | { status: 'idle' }
    | { status: 'pending' }
    | { status: 'success'; data: Result }
    | { status: 'error'; error: unknown };

/**
 * Typed handle returned by `rsjs.mutation(key)`. `run` fires the
 * mutation; `subscribe` and `getSnapshot` are the
 * `useSyncExternalStore` integration shape.
 */
export interface MutationHandle<Args = unknown, Result = unknown> {
    /**
     * Fire the mutation with `args`. Resolves with the runner's
     * return value on success; rejects on error.
     *
     * `run` takes a **single** argument. The value is forwarded
     * verbatim to the runner: it becomes the request body
     * (JSON-stringified), the value passed to `optimistic_update`,
     * and the value the macro lowers `m.run(x)` against. To pass
     * "multiple" inputs, wrap them in an object or tuple
     * (`m.run({ id, qty })` or `m.run([id, qty])`); the runtime
     * does not splat arrays into positional args.
     *
     * On success, every cache entry whose key matches the
     * mutation's `invalidates` list is refetched (or reset, if
     * `on_success_invalidate_strategy` is `"reset"`).
     *
     * On error, any optimistic updates applied before the runner
     * was invoked are rolled back to their pre-`run()` values.
     */
    run(args: Args): Promise<Result>;

    /**
     * Subscribe to state transitions. The callback receives the
     * full discriminated state object on every transition. Returns
     * an unsubscribe function.
     *
     * Use this with React's `useSyncExternalStore` (or the
     * equivalent in Vue / Svelte / Solid) to mirror mutation state
     * into a non-rsjs UI.
     */
    subscribe(callback: (state: MutationState<Result>) => void): () => void;

    /**
     * Synchronous snapshot of the current mutation state. Required
     * by `useSyncExternalStore`'s contract.
     */
    getSnapshot(): MutationState<Result>;

    // Convenience accessors. Each returns a primitive projection of
    // the state, mirroring the per-accessor methods the rsjs JS
    // emitter targets via Expr::MutationRead.
    /** Current state-machine state. */
    status(): MutationStatus;
    /** True iff `status === 'idle'`. */
    is_idle(): boolean;
    /** True iff `status === 'pending'`. */
    is_pending(): boolean;
    /** True iff `status === 'success'`. */
    is_success(): boolean;
    /** True iff `status === 'error'`. */
    is_error(): boolean;
    /** Most recent successful result, or `undefined`. */
    data(): Result | undefined;
    /** Most recent rejected error, or `undefined`. */
    error(): unknown | undefined;
}

/**
 * Per-island mutation registration. The rsjs JS emitter calls this
 * once per declared `mutation(...)` to wire the state slot up to
 * the runner config, the invalidation list, and the per-mutation
 * policy. Macro-generated calls pass `root` so static metadata can
 * be read from the adjacent server-emitted mutation blob; direct
 * fields remain as fallbacks for dynamic signal-backed keys.
 *
 * **This is an internal contract between the rsjs proc-macro and
 * the runtime.** Consumers writing custom JS should reach for
 * `rsjs.mutation()` instead.
 */
export function register_mutation<Args = unknown, Result = unknown>(opts: {
    island: string;
    mutation_id: number;
    root?: Element | null;
    key: MutationKey | null;
    runner:
        | string
        | {
              url: string;
              method?: string;
              body?: unknown;
              headers?: Record<string, string>;
              credentials?: 'omit' | 'same-origin' | 'include';
          }
        | null;
    invalidates: ReadonlyArray<QueryKey>;
    options: {
        retry_count: number;
        on_success_invalidate_strategy: 'refetch' | 'reset';
    };
}): MutationHandle<Args, Result>;
