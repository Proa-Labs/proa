// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// rsjs base runtime — public TypeScript declarations.
//
// This file documents the M2 base-runtime public JS contract
// exposed by `/rsjs-runtime.js`. The shape mirrors the named
// exports from the JS module: signal, read_signal_initials,
// signal_initial, bind_text, bind_event, bind_input, bind_submit,
// bind_attr, bind_conditional, hydrate, dispose_hydrated.
//
// Cross-framework consumers (React, Vue, Svelte, vanilla) embed
// these primitives directly and benefit from compile-time type
// checking; the rsjs proc macro emits calls into the same
// vocabulary so the .d.ts also serves as a reference for what the
// generated JS expects.

/**
 * Cleanup callback returned by [`hydrate`]. Framework adapters can
 * call this on island unmount to undo side effects the runtime
 * registered. Disposers are idempotent: calling one more than once
 * has the same effect as calling it once.
 */
export type Disposer = () => void;

/**
 * Synchronous owner initializer used by hydration strategies.
 *
 * Generated initializers push each resource disposer into `resources`
 * immediately after creation. The runtime owns that stack so a later
 * initialization failure can unwind partial listeners, effects, and
 * subscriptions in reverse order before leaving the SSR owner inert. A
 * returned disposer is additive: owner teardown runs it first and still drains
 * the runtime-owned stack. Returning a callback that calls `dispose_all` on
 * this same array remains supported and does not double-run resources.
 */
export type HydrationInit = (
    root: HTMLElement,
    resources: Disposer[],
) => void | Disposer;

export type HydrationStrategy = (
    name: string,
    init: HydrationInit,
    ...args: unknown[]
) => Disposer;

export interface RsjsActivationErrorContext {
    response_epoch: number;
    navigation_generation: number;
    owner_id: string;
    asset_id: string;
    phase: "import" | "dependency" | "init";
}

/**
 * Structured protocol error for a failed generation-scoped owner activation.
 * The associated SSR owner remains inert.
 */
export class RsjsActivationError extends Error {
    readonly code: "rsjs/owner-activation-failed";
    readonly response_epoch: number;
    readonly navigation_generation: number;
    readonly owner_id: string;
    readonly asset_id: string;
    readonly phase: "import" | "dependency" | "init";
    readonly context: Readonly<RsjsActivationErrorContext>;
}

/**
 * Reactive cell. Created by [`signal`].
 *
 * The runtime tracks dependencies via the currently-running
 * effect: a `.get()` call inside an effect's body registers the
 * effect as a subscriber, so a subsequent `.set(...)` re-runs
 * the effect.
 */
export interface Signal<T> {
    /**
     * Read the current value. If called inside a tracked effect,
     * registers that effect as a subscriber so future `.set` calls
     * re-run it.
     */
    get(): T;

    /**
     * Write a new value, notifying every current subscriber.
     * No-op when `next` is `Object.is`-equal to the previous value.
     */
    set(next: T): void;

    /**
     * Convenience wrapper for `.set(fn(prev_value))`. The closure
     * runs untracked: reading other signals inside `fn` does not
     * subscribe the calling effect to them.
     */
    update(fn: (prev: T) => T): void;

    /**
     * Read the current value without registering a subscription.
     */
    peek(): T;
}

/**
 * Internal handle returned by the runtime's effect machinery.
 *
 * Exposed in the type surface only because the binding helpers
 * accept caller-provided dep arrays whose elements are signals
 * and the runtime calls `signal._subscribe(effect)` against them.
 * Library consumers normally do not touch `Effect` directly.
 */
export interface Effect {
    /** Set of signals this effect currently subscribes to. */
    deps: Set<unknown>;
    /** Re-run the effect body. Idempotent w.r.t. dep tracking. */
    run(): void;
}

/**
 * Query / mutation handles can participate in bindings by exposing a
 * lightweight subscription method. The runtime accepts these as
 * additional dependencies for expressions that read non-signal state.
 */
export interface SubscribableHandle {
    subscribe(listener: () => void): unknown;
}

/**
 * Create a new reactive signal cell seeded with `initial`.
 *
 * The cell exposes `.get()`, `.set(v)`, `.update(fn)`, and
 * `.peek()`. Equality on `.set` uses `Object.is`, so primitives
 * compare by value and reference types compare by identity.
 */
/** Compiler-owned text! layout; arguments are rendered as raw DOM text. */
export function format_text(template: string, plan: string, ...values: unknown[]): string;
export function format_text_attr(template: string, plan: string, ...values: unknown[]): string;

export function signal<T>(initial: T): Signal<T>;

export type NarrowIntegerDomain = "i32" | "u32";
export type WideIntegerDomain = "i64" | "u64";
export type IntegerDomain = NarrowIntegerDomain | WideIntegerDomain;

/** Create a signal that validates every write in one Rust integer domain. */
export function integer_signal(
    initial: number,
    domain: NarrowIntegerDomain,
): Signal<number>;
export function integer_signal(
    initial: bigint,
    domain: WideIntegerDomain,
): Signal<bigint>;

/**
 * Read half of the Signal interface, as returned by `memo()`.
 * Listing a memo in a binding's `deps` array works exactly like
 * listing a plain signal.
 */
export interface ReadonlySignal<T> {
    /** Read the current derived value (subscribes a tracked effect). */
    get(): T;
    /** Read the current derived value without subscribing. */
    peek(): T;
    /**
     * Unsubscribe the recompute effect from every input. Island init
     * code pushes this into the island's disposer list.
     */
    _dispose(): void;
}

/**
 * Create a read-only derived cell whose value is `fn()` re-evaluated
 * whenever its inputs change. With `deps` supplied, inputs are
 * subscribed explicitly (the compile-time dep-list path); without it,
 * a tracked first run discovers them. Recomputation is microtask
 * batched; downstream subscribers only run when the derived value
 * actually changed (`Object.is`).
 */
export function memo<T>(
    fn: () => T,
    deps?: ReadonlyArray<Signal<unknown>>,
): ReadonlySignal<T>;

/**
 * Run `fn` every `ms` milliseconds until the returned disposer runs.
 * Island init code pushes the disposer into the island's disposer
 * list so teardown clears the interval.
 */
export function interval(ms: number, fn: () => void): Disposer;

/**
 * Debounced function wrapper (trailing edge). Carries a `_dispose`
 * property that cancels any pending invocation; island init code
 * pushes it into the island's disposer list.
 */
export interface DebouncedFunction<A extends unknown[]> {
    (...args: A): void;
    _dispose: Disposer;
}

/**
 * Wrap `fn` so calls collapse until `delay_ms` milliseconds pass without a
 * new call; the wrapper then invokes `fn` with the most recent
 * arguments.
 */
export function debounce<A extends unknown[]>(
    fn: (...args: A) => void,
    delay_ms: number,
): DebouncedFunction<A>;

/**
 * Item record returned by bind_list's `create` / `adopt` callbacks.
 * Returning a bare Node is equivalent to `{ node }`.
 */
export interface ListItemRecord<T> {
    node: Node;
    /** Refresh the node when the item value changes identity. */
    update?(item: T, index: number): void;
    /** Tear down item-level bindings when the item leaves the list. */
    dispose?(): void;
}

export interface BindListOptions<T> {
    /**
     * Stable string key per item. Required. Duplicate keys throw before the
     * reconciliation mutates item state or DOM.
     *
     * The current runtime canonicalizes with `String(...)`; typed
     * string-versus-integer identity awaits the compiler/SSR key-tagging
     * protocol.
     */
    key(item: T, index: number): string;
    /** Build the DOM for a new item. Required. */
    create(item: T, index: number): Node | ListItemRecord<T>;
    /**
     * Pair a server-rendered child carrying `data-rsjs-key` with its
     * initial item instead of re-creating it.
     */
    adopt?(node: Node, item: T, index: number): Node | ListItemRecord<T>;
    /** Explicit dep array (the compile-time dep-list path). */
    deps?: ReadonlyArray<Signal<unknown>>;
    /** Extra query / mutation handles observed by the list getter. */
    extra_handles?: ReadonlyArray<SubscribableHandle>;
}

/**
 * Keyed list rendering over a reactive array. The binding owns the
 * element children of `container`: it inserts, moves, and removes
 * them to mirror `getter()`. Keyed reconciliation: removals dispose,
 * surviving keys update in place, and one forward pass moves nodes
 * only when out of position. Duplicate live keys or duplicate adoptable SSR
 * `data-rsjs-key` markers throw without applying a valid prefix.
 */
export function bind_list<T>(
    container: Element,
    getter: () => ReadonlyArray<T>,
    options: BindListOptions<T>,
): Disposer;

/**
 * Consume a disposer stack in reverse order. Every entry is removed before it
 * runs, so retaining the array after teardown does not retain owner resources
 * and calling `dispose_all` again is a no-op.
 */
export function dispose_all(
    disposers: Disposer[],
    scope?: string,
): void;

/**
 * Read a rendered island instance's signal seed array from its
 * adjacent `<script type="application/json" data-rsjs-island="...">`
 * state blob. Missing or malformed state returns an empty array.
 */
export function read_signal_initials(root: HTMLElement): ReadonlyArray<unknown>;

export interface SharedStateBinding {
    scope_id: number;
    type_key: string;
    schema_hash: string;
    rust_type: string;
}

export interface SharedStateStore {
    readonly scope_id: number;
    readonly type_key: string;
    readonly schema_hash: string;
    field<T = unknown>(name: string): Signal<T>;
}

export interface SharedStateSeed {
    scope_id: number;
    type_key: string;
    schema_hash: string;
    rust_type: string;
    state: Record<string, unknown>;
    /**
     * Recursive paths whose values are wide integers (the Rust i64 family)
     * serialized as JSON strings. `"*"` traverses every array element.
     */
    wide_paths?: ReadonlyArray<ReadonlyArray<string>>;
    /** @deprecated Legacy top-level form; use one-segment `wide_paths`. */
    wide?: ReadonlyArray<string>;
}

export interface SharedStateSeedTable {
    abi_version: number;
    stores: ReadonlyArray<SharedStateSeed>;
}

export class RsjsSharedStateError extends Error {
    /**
     * `code` is the bare error code (for example
     * `"shared-state-missing-scope"`); the constructor prefixes it with
     * `rsjs/` for both the stored `code` property and the message.
     */
    constructor(code: string, message: string, details?: Record<string, unknown>);
    readonly code: string;
    readonly details: Record<string, unknown>;
}

export function read_shared_state_bindings(
    root: HTMLElement,
): ReadonlyArray<SharedStateBinding>;

export function install_page_shared_state(table: SharedStateSeedTable): void;

/** Install the current document's inert page seed after a client route swap. */
export function install_page_shared_state_from_document(): void;

export function acquire_shared_state(root: HTMLElement, slot: number): SharedStateStore;

/**
 * Destroy every SharedState store in the current page generation, sever field
 * subscriptions, cancel their pending work, and invalidate existing store and
 * field handles. Later use of an invalid handle throws
 * `rsjs/shared-state-disposed`.
 */
export function dispose_page_shared_state(): void;

/**
 * Group synchronous signal writes into one deduplicated next-microtask flush.
 * Reads see writes immediately, nested batches share the outer boundary, and
 * a thrown callback preserves prior writes without poisoning the scheduler.
 * Async and Promise-returning callbacks are rejected at runtime.
 */
export function batch<T>(run: () => T): T;

/**
 * Return `initials[index]` when present, otherwise `fallback`.
 * Unlike `??`, this preserves explicit JSON `null` seed values.
 */
export function signal_initial<T>(
    initials: ReadonlyArray<unknown>,
    index: number,
    fallback: T,
): T;

export type SignalSeedType =
    | { readonly kind: "i32" | "u32" | "i64" | "u64" }
    | { readonly kind: "option"; readonly inner: SignalSeedType }
    | { readonly kind: "vec"; readonly inner: SignalSeedType }
    | {
        readonly kind: "object";
        readonly fields: ReadonlyArray<{
            readonly name: string;
            readonly ty: SignalSeedType;
        }>;
    }
    | { readonly kind: string };

/** Decode fixed-width integer leaves nested in a typed signal seed. */
export function signal_initial_typed<T>(
    initials: ReadonlyArray<unknown>,
    index: number,
    fallback: T,
    ty: SignalSeedType,
): T;

/**
 * `signal_initial` for wide-int (i64-family) signals: decodes the codec's
 * JSON-string wire form to BigInt so values past 2^53 stay exact. Number
 * seeds are accepted defensively; non-decodable seeds fall back.
 */
export function signal_initial_wide(
    initials: ReadonlyArray<unknown>,
    index: number,
    fallback: bigint | null,
): bigint | null;

/** Decode and validate one fixed-width Rust integer hydration seed. */
export function signal_initial_integer(
    initials: ReadonlyArray<unknown>,
    index: number,
    fallback: number,
    domain: NarrowIntegerDomain,
): number;
export function signal_initial_integer(
    initials: ReadonlyArray<unknown>,
    index: number,
    fallback: bigint,
    domain: WideIntegerDomain,
): bigint;

export function checked_int_add(
    a: number,
    b: number,
    domain: NarrowIntegerDomain,
): number;
export function checked_int_add(
    a: bigint,
    b: bigint,
    domain: WideIntegerDomain,
): bigint;
export function checked_int_sub(
    a: number,
    b: number,
    domain: NarrowIntegerDomain,
): number;
export function checked_int_sub(
    a: bigint,
    b: bigint,
    domain: WideIntegerDomain,
): bigint;
export function checked_int_mul(
    a: number,
    b: number,
    domain: NarrowIntegerDomain,
): number;
export function checked_int_mul(
    a: bigint,
    b: bigint,
    domain: WideIntegerDomain,
): bigint;
export function checked_int_div(
    a: number,
    b: number,
    domain: NarrowIntegerDomain,
): number;
export function checked_int_div(
    a: bigint,
    b: bigint,
    domain: WideIntegerDomain,
): bigint;
export function checked_int_rem(
    a: number,
    b: number,
    domain: NarrowIntegerDomain,
): number;
export function checked_int_rem(
    a: bigint,
    b: bigint,
    domain: WideIntegerDomain,
): bigint;
export function checked_int_neg(
    value: number,
    domain: "i32",
): number;
export function checked_int_neg(
    value: bigint,
    domain: "i64",
): bigint;

/**
 * Representation-dispatching arithmetic for page-shared fields.
 *
 * A shared field's JS representation (BigInt or Number) is pinned by the page
 * seed's `wide_paths`, which a proc macro cannot resolve, so the island
 * compiler emits these helpers instead of bare operators. Each dispatches on
 * `typeof` and widens a Number operand to BigInt when the other side is one,
 * throwing `RsjsSharedStateError('rsjs/mixed-width-arithmetic')` for a
 * fractional Number that has no BigInt image.
 *
 * Ordering comparisons have no helper: JS already defines mixed BigInt/Number
 * ordering correctly.
 */
export function num_add(a: unknown, b: unknown): unknown;
export function num_sub(a: unknown, b: unknown): unknown;
export function num_mul(a: unknown, b: unknown): unknown;
/** Rust float `/`: never truncates. */
export function num_div(a: unknown, b: unknown): unknown;
/** Rust integer `/`: truncates toward zero for both representations. */
export function num_div_int(a: unknown, b: unknown): unknown;
export function num_rem(a: unknown, b: unknown): unknown;
/** Rust `==`: `num_eq(5n, 5)` is `true`, unlike `5n === 5`. */
export function num_eq(a: unknown, b: unknown): boolean;
/** Rust `!=`. */
export function num_ne(a: unknown, b: unknown): boolean;

/**
 * Keep `node.textContent` in sync with `getter()`.
 *
 * When `deps` is supplied, the runtime registers explicit
 * subscriptions against each signal in the array and runs the
 * binding once eagerly. This is the path the rsjs JS emitter
 * uses; the dep list comes from the IR's dep_collector pass.
 *
 * When `deps` is omitted (or not an array), the runtime falls
 * back to a tracked first execution that discovers the deps
 * dynamically via the currently-running effect mechanism.
 *
 * A null `node` logs a warning and skips the binding (the rest
 * of the island still wires up).
 *
 * The compiler passes `text_kind: "rust-float"` only for expressions
 * proven to be f32/f64. In that mode infinities use Rust `Display`
 * spelling (`inf` / `-inf`) rather than JavaScript spelling.
 */
export function bind_text(
    node: Node | null,
    getter: () => unknown,
    deps?: ReadonlyArray<Signal<unknown>>,
    extra_handles?: ReadonlyArray<SubscribableHandle>,
    text_kind?: "rust-float",
): Disposer;

/**
 * `addEventListener` wrapper kept in the binding vocabulary so
 * the emit_js output reads uniformly. A null `node` logs a
 * warning and skips.
 *
 * As of OPTIMIZATIONS item #2 the emit_js codegen routes non-submit
 * events through `bind_delegated_events` instead. The export is kept
 * for backward compatibility with hand-written runtime callers.
 */
export function bind_event(
    node: EventTarget | null,
    name: string,
    handler: (event: Event) => void,
): Disposer;

/**
 * Document-level event registration. The listener attaches to
 * `owner.ownerDocument`, while the handler observes `owner` as the
 * rsjs current target for `event.current_target_*()` helpers.
 */
export function bind_document_event(
    owner: Element | null,
    name: string,
    handler: (event: Event) => void,
): Disposer;

/**
 * Window-level event registration. The listener attaches to
 * `owner.ownerDocument.defaultView`, while the handler observes
 * `owner` as the rsjs current target for `event.current_target_*()`
 * helpers.
 */
export function bind_window_event(
    owner: Element | null,
    name: string,
    handler: (event: Event) => void,
): Disposer;

/**
 * Delegated event registration. One listener attaches to `root`;
 * each fired event walks `closest('[data-rsjs-handler]')` from the
 * target and dispatches to the matching entry in `handlers`. Keys
 * are the stringified NodeId values the SSR layer writes into the
 * `data-rsjs-handler` attribute. A null `root` logs a warning and
 * skips.
 */
export function bind_delegated_events(
    root: Element | null,
    eventName: string,
    handlers: Map<string, (event: Event) => void>,
): Disposer;

/**
 * Two-way binding between a form-control DOM property and a
 * signal.
 *
 * `property` is the DOM property name to read/write: `"value"`
 * for text inputs, textareas, and `<select>`s; `"checked"` for
 * checkboxes / radios. User edits flow into the signal on
 * `input` and `change`; signal updates flow back to the DOM via
 * a tracked subscription. Before that subscription commits, a
 * property that differs from its SSR/default baseline wins and is
 * adopted into the signal. Re-entrant writes are guarded by an
 * `Object.is` comparison against `signal.peek()`.
 *
 * `preserve_dirty` defaults to true for first hydration. Linked lifecycle
 * remounts pass false so a retained control that still differs from its
 * immutable SSR default cannot overwrite the freshly seeded child signal.
 *
 * A null `node` logs a warning and skips.
 */
export function bind_input<T>(
    node: (HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement | HTMLElement) | null,
    property: 'value' | 'checked' | 'selected' | 'textContent' | string,
    signal: Signal<T>,
    ssr_baseline?: unknown,
    preserve_dirty?: boolean,
): Disposer;

/**
 * One-way binding between a reactive expression and an HTML attribute.
 *
 * `kind: "boolean"` uses HTML boolean-attribute presence/absence
 * semantics. `kind: "value"` stringifies non-null values; `null` /
 * `undefined` remove the attribute. `kind: "url"` stringifies values
 * after neutralizing dangerous URL schemes to match SSR. For
 * `name === "value"`, rsjs also mirrors the DOM property so controlled
 * form fields update after user edits.
 */
export function bind_attr(
    node: Element | null,
    name: string,
    kind: 'value' | 'boolean' | 'url',
    getter: () => unknown,
    deps?: ReadonlyArray<Signal<unknown>>,
    extra_handles?: ReadonlyArray<SubscribableHandle>,
): Disposer;

/**
 * Form-submit binding. Calls `event.preventDefault()` before
 * dispatching to `handler` so the browser does not navigate.
 * A null `node` logs a warning and skips.
 */
export function bind_submit(
    node: HTMLFormElement | null,
    handler: (event: SubmitEvent) => void,
): Disposer;

/**
 * Toggle visibility of two sibling lists of nodes based on a
 * boolean predicate. `then_nodes` is shown and `else_nodes` is
 * hidden when `getter()` is truthy; the pair flips when it is
 * falsy. Visibility is implemented via `style.display`, not
 * removal, so any subscriptions inside the hidden subtree stay
 * live.
 *
 * `deps` is the explicit dep array the JS emitter computed; if
 * omitted, the runtime falls back to a tracked first execution.
 * Initial predicate failure throws and rolls back registration.
 * Later failures are reported and preserve the last visibility state
 * so other subscribers to the same predicate can still fail closed.
 *
 * Null entries in either node list are tolerated and skipped.
 */
export function bind_conditional(
    getter: () => boolean,
    deps: ReadonlyArray<Signal<unknown>> | undefined,
    then_nodes: ReadonlyArray<HTMLElement | null>,
    else_nodes: ReadonlyArray<HTMLElement | null>,
    extra_handles?: ReadonlyArray<SubscribableHandle>,
): Disposer;

/**
 * Exact parent predicate exported for a linked component lifecycle.
 *
 * The producer owns `get` and the dependency lists; consumers must not
 * reconstruct the predicate from canonical IDs.
 */
export interface ComponentLifecycleControl {
    get(): boolean;
    deps: ReadonlyArray<{
        _subscribe(effect: Effect): Disposer;
    }>;
    handles: ReadonlyArray<{
        subscribe(listener: () => void): Disposer;
    }>;
}

/**
 * Mount or dispose one linked component group as its parent predicate changes.
 *
 * Initial predicate/mount failures throw so owner initialization can unwind
 * atomically. Later failures are reported under `scope`, leave the group
 * inactive, and may retry on a later dependency notification. Teardown
 * unsubscribes before disposing the active group and never removes SSR DOM.
 */
export function bind_component_lifecycle(
    control: ComponentLifecycleControl,
    branch: "then" | "else",
    mount: () => Disposer,
    scope: string,
): Disposer;

/**
 * Look up the rendered island root and invoke `init(root)`.
 *
 * Lookup strategy:
 *   1. Find every `<rsjs-island data-rsjs-island-root="<name>">` element.
 *   2. If no element is found in dev mode, log a warning and call
 *      `init(document.body)` so manual smoke tests still work
 *      without the server emitter.
 *   3. If no element is found in production mode, log an error and
 *      skip hydration.
 *
 * Hydration is synchronous when the DOM is ready. If the runtime is
 * loaded while the document is still parsing, `hydrate` defers until
 * `DOMContentLoaded`; the returned [`Disposer`] cancels the pending
 * deferred run when called before the listener fires.
 *
 * Returns a [`Disposer`] framework adapters can call on island
 * unmount. The disposer tears down child binding subscriptions and
 * DOM listeners returned by generated island init code, and cancels
 * pending DOMContentLoaded hydration for the deferred path.
 * Starting a newer hydration for the same island/root cancels older
 * pending strategy work before the new init is registered.
 */
export function hydrate(
    name: string,
    init: HydrationInit,
): Disposer;

/**
 * Hydrate matching island roots when they first become visible.
 *
 * Uses `IntersectionObserver` when available and falls back to immediate
 * hydration when the browser does not provide it. The returned disposer
 * cancels unobserved roots and disposes any roots that already hydrated.
 */
export function hydrate_when_visible(
    name: string,
    init: HydrationInit,
    options?: IntersectionObserverInit,
): Disposer;

/**
 * Hydrate matching island roots during an idle callback.
 *
 * Falls back to `setTimeout(..., 0)` when `requestIdleCallback` is not
 * available. The returned disposer cancels pending idle work and disposes
 * any roots that already hydrated.
 */
export function hydrate_when_idle(
    name: string,
    init: HydrationInit,
    options?: { timeout?: number },
): Disposer;

/**
 * Hydrate matching island roots on first interaction with each root.
 *
 * Defaults to `pointerdown`, `keydown`, and `focusin`. The capture trigger
 * initializes synchronously, then the original event continues to the newly
 * installed delegated handler in its normal bubble phase; no synthetic event
 * is replayed. If initialization throws, resources already pushed into the
 * initializer stack are unwound and the owner remains inert. The returned
 * disposer removes pending interaction listeners and disposes any roots that
 * already hydrated.
 */
export function hydrate_on_interaction(
    name: string,
    init: HydrationInit,
    events?: ReadonlyArray<string>,
): Disposer;

/**
 * Hydrate matching island roots once a media query matches.
 *
 * Hydrates immediately when the query already matches or `matchMedia` is not
 * available. The returned disposer removes pending media listeners and
 * disposes any roots that already hydrated.
 */
export function hydrate_when_media(
    name: string,
    init: HydrationInit,
    query: string,
): Disposer;

/**
 * Register a visible/idle/media module gate owned by the current page
 * generation. Replacement-page hydration cannot replay a stale gate, and
 * every matching root retains independent trigger/import authorization.
 */
export function defer_island_module(
    asset_id: string,
    module_url: string,
    strategy: HydrationStrategy,
    ...strategy_args: unknown[]
): Disposer;

/**
 * Generation-fenced ESM loader used by framework-generated island bootstraps.
 *
 * The returned disposer revokes authorization. Native ESM evaluation may
 * finish after revocation, but its initializer can never activate that owner
 * or a replacement page generation. Because the loader uses dynamic import,
 * CSP must allow the resolved module source or propagate a nonce through
 * `strict-dynamic`; a nonce on the inline bootstrap alone is insufficient.
 */
export function load_island_module(
    asset_id: string,
    root: HTMLElement | null,
    module_url: string,
    importer?: (url: string) => PromiseLike<unknown> | unknown,
    activation_strategy?: HydrationStrategy | null,
    activation_args?: unknown[],
): Disposer;

/**
 * Load with a ticket-owner activation strategy. This keeps lifecycle records
 * in the framework runtime even when generated ESM imports an alias URL for
 * the same runtime bytes.
 */
export function load_island_module_for_strategy(
    asset_id: string,
    root: HTMLElement | null,
    module_url: string,
    strategy: HydrationStrategy,
    ...strategy_args: unknown[]
): Disposer;

/**
 * Select the normal hydration strategy for an ordinary module URL, or register
 * an inert initializer against the exact single-use `rsjs-load` ticket carried
 * by a managed module URL.
 */
export function hydrate_from_module(
    module_url: string,
    strategy: HydrationStrategy,
): HydrationStrategy;

/**
 * Dispose currently registered hydrated island roots.
 *
 * With no arguments, disposes every runtime-tracked island, including pending
 * DOMContentLoaded, visible, idle, and interaction hydration. Passing `name`
 * narrows cleanup to one island asset id; passing both `name` and `root`
 * narrows cleanup to one rendered root. Returns the number of tracked
 * hydration records disposed.
 */
/**
 * With no filters, also tears down page-scoped shared-state cells and every
 * optional runtime resource registered for the page generation.
 */
export function dispose_hydrated(name?: string, root?: HTMLElement): number;

/**
 * Internal lifecycle extension for optional runtime modules. Registered
 * disposers run only during an unfiltered/full page-generation teardown, not
 * targeted island or subtree disposal. Returns an unregister function.
 */
export function register_page_generation_disposer(
    dispose: () => void,
): () => void;

/**
 * Dispose only the islands inside `container` (plus any island root that has
 * already left the document), leaving the rest of the page running.
 *
 * This is the teardown for a container patch: islands in the persistent shell
 * are never re-hydrated after a soft navigation, so a page-wide
 * `dispose_hydrated()` would leave them as inert markup. Page-scoped shared
 * stores and query caches are kept alive for the same reason; call
 * `dispose_hydrated()` with no arguments for a real page transition. Returns
 * the number of hydration records disposed.
 */
export function dispose_hydrated_subtree(container: Element): number;
