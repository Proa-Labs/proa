// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// rsjs runtime — signal scheduler + DOM bindings + hydration entry.
//
// Stage-1 (M2) implementation. Plain ESM, named exports.
//
// Public API:
//
//   import {
//       signal, bind_text, bind_event, bind_input, bind_submit,
//       bind_conditional, read_signal_initials, signal_initial, hydrate,
//   } from '/rsjs-runtime.js';
//
//   - signal(initial)            → reactive cell with .get(), .set(v), .update(fn).
//   - read_signal_initials(root) → parse this island instance's adjacent
//                                  dehydrated signal-state JSON blob.
//   - signal_initial(initials, index, fallback)
//                                → read one initial value, preserving explicit
//                                  JSON null and falling back only when absent.
//   - signal_initial_wide(initials, index, fallback)
//                                → signal_initial for wide-int (i64-family)
//                                  signals: decodes the codec's JSON-string
//                                  wire form to BigInt so values past 2^53
//                                  stay exact.
//   - signal_initial_typed(initials, index, fallback, ty)
//                                → recursively validate/decode fixed-width
//                                  integer leaves inside Option/Vec/object
//                                  signal seeds.
//   - num_add / num_sub / num_mul / num_div / num_div_int / num_rem /
//     num_eq / num_ne (a, b)
//                                → legacy representation-dispatching helpers
//                                  for page-shared fields. New fixed-width
//                                  arithmetic requires an exact compiler
//                                  domain; equality may still use num_eq/ne
//                                  for representation-pinned shared values.
//   - bind_text(node, getter, deps, extra_handles, text_kind)
//                                → keep node.textContent in sync with getter().
//                                  `deps` is the array of signals to subscribe to;
//                                  the rsjs JS emitter computes it via the
//                                  dep_collector pass on the IR. A compiler-
//                                  proven float passes `text_kind =
//                                  "rust-float"` so infinities keep Rust's
//                                  `inf` / `-inf` Display spelling.
//   - bind_event(node, name, handler)
//                                → addEventListener wrapper. Trivial; defined
//                                  for symmetry with bind_text and so the
//                                  emitted JS uses one consistent vocabulary.
//                                  Kept exported for backward compat; the
//                                  emitter routes non-submit events through
//                                  `bind_delegated_events` as of M-OPT2.
//   - bind_delegated_events(root, eventName, handlers)
//                                → one listener per event-name on the island
//                                  root; dispatches to per-handler functions
//                                  by reading `data-rsjs-handler` on the
//                                  click target's closest matching ancestor.
//                                  Replaces N `bind_event` + N `walk_path`
//                                  calls with a single listener registration
//                                  during hydrate.
//   - bind_document_event(owner, name, handler)
//                                → document-level outside-click / key listener.
//   - bind_window_event(owner, name, handler)
//                                → window-level history/storage/resize listener.
//   - bind_input(node, property, signal)
//                                → two-way binding between a form control's DOM
//                                  property (`value`, `checked`) and a signal.
//                                  User edits flow into the signal on `input`
//                                  and `change`; signal writes flow back to the
//                                  DOM via a tracked subscription. M5.
//   - bind_submit(node, handler) → form-submit binding that calls
//                                  `event.preventDefault()` before invoking
//                                  the handler, so the browser does not
//                                  navigate. M5.
//   - bind_conditional(getter, deps, then_nodes, else_nodes)
//                                → toggle visibility of two sibling lists of
//                                  nodes based on a boolean predicate. When
//                                  getter() is truthy, every node in
//                                  then_nodes is shown and every node in
//                                  else_nodes is hidden; falsy flips the
//                                  pair. The runtime hides via `display: none`
//                                  rather than removing nodes from the DOM,
//                                  so any signal subscriptions inside the
//                                  hidden subtree stay live (M4 fixtures
//                                  don't have nested bindings yet, but this
//                                  keeps the door open for M5 forms).
//   - hydrate(name, init)        → look up rendered <rsjs-island data-rsjs-island-root="...">
//                                  element(s) and run init(root) against
//                                  every match. Returns a Disposer
//                                  (`() => void`) that framework adapters
//                                  can call on island unmount.
//   - hydrate_when_visible / hydrate_when_idle / hydrate_on_interaction /
//     hydrate_when_media
//                                → strategy variants that defer init(root)
//                                  until the root is visible, idle time is
//                                  available, the user first interacts, or a
//                                  media query matches.
//                                  Pending work is tracked so returned
//                                  disposers and dispose_hydrated() cancel it.
//
//                                  Production behaviour: when no marker is
//                                  found, hydrate logs an error to console
//                                  and returns without binding. (Falling
//                                  back to document.body silently masks
//                                  SSR pipeline bugs.)
//
//                                  Development behaviour: in dev mode, the
//                                  no-markers branch falls back to
//                                  document.body so the manual smoke test
//                                  (drop emitted JS into an index.html,
//                                  click buttons) keeps working without a
//                                  server emitter to write the markers.
//                                  Dev mode is auto-detected (see the
//                                  `is_dev_mode()` JSDoc below for the
//                                  resolution order); the explicit override
//                                  is `globalThis.__RSJS_DEV__`:
//                                      <script>globalThis.__RSJS_DEV__ = true;</script>
//                                  When NODE_ENV is set, the runtime treats
//                                  anything other than "production" as dev.
//
//                                  Disposer contract: init(root) may return a
//                                  cleanup callback. hydrate collects one per
//                                  island instance and returns a parent
//                                  disposer that runs them all. Rehydrating the
//                                  same island root disposes the previous tree
//                                  first; route adapters can also call
//                                  dispose_hydrated().
//
// Manual smoke test (no server required):
//
//   1. Create an index.html that loads this file as an ES module and the
//      emitted Counter JS (also as a module). Inside <body>, hand-write the
//      DOM the server emitter will eventually produce, including the markers
//      that the emit_js output references:
//
//          <rsjs-island data-rsjs-island-root="counter">
//              <div>
//                  <button data-rsjs-handler="1">-</button>
//                  <span   data-rsjs-bind="2">0</span>
//                  <button data-rsjs-handler="3">+</button>
//              </div>
//          </rsjs-island>
//
//   2. Open the file in a browser (or `python3 -m http.server` if the
//      browser refuses file:// modules). Click the buttons. The span text
//      should update.
//
// The runtime is intentionally small. ~1-2 KB minified for v0.1; readability
// is prioritised over byte count here. Minification is a separate pass.

// ---------------------------------------------------------------------------
// Signal scheduler
// ---------------------------------------------------------------------------

// The currently-running effect, or null. When a signal's .get() is called,
// it adds the running effect to its subscriber set so future writes notify it.
let CURRENT_EFFECT = null;
const HYDRATED_ROOTS = new WeakMap();
const HYDRATED_RECORDS = new Set();
const PENDING_HYDRATIONS = new Set();
const HYDRATION_REGISTRATIONS = new Map();
const NAVIGATION_REHYDRATION_DOCUMENTS = new WeakSet();
let DETACHED_ROOT_OBSERVER = null;

// A page may import the runtime through both a framework-resolved URL and the
// canonical specifier embedded in generated island ESM. Those URLs are
// distinct module instances even when they serve identical bytes, so managed
// import authorization cannot live in module-local maps. Keep the small
// ticket registry on a Symbol.for singleton; this is protocol state, not
// application state, and every runtime alias must observe the same tickets.
const MANAGED_MODULE_REGISTRY_SYMBOL =
    typeof Symbol === 'undefined'
        ? null
        : Symbol.for('rsjs.managed-module-registry.v1');

function managed_module_registry() {
    if (
        typeof globalThis === 'undefined' ||
        MANAGED_MODULE_REGISTRY_SYMBOL === null
    ) {
        return {
            loads_by_id: new Map(),
            loads_by_key: new Map(),
            init_contexts: new WeakMap(),
            root_ids: new WeakMap(),
            next_load_id: 1,
            next_root_id: 1,
            active_authorization: null,
        };
    }
    const current = globalThis[MANAGED_MODULE_REGISTRY_SYMBOL];
    if (
        current !== null &&
        typeof current === 'object' &&
        current.loads_by_id instanceof Map &&
        current.loads_by_key instanceof Map &&
        current.root_ids instanceof WeakMap
    ) {
        if (!(current.init_contexts instanceof WeakMap)) {
            current.init_contexts = new WeakMap();
        }
        return current;
    }
    const registry = {
        loads_by_id: new Map(),
        loads_by_key: new Map(),
        init_contexts: new WeakMap(),
        root_ids: new WeakMap(),
        next_load_id: 1,
        next_root_id: 1,
        active_authorization: null,
    };
    globalThis[MANAGED_MODULE_REGISTRY_SYMBOL] = registry;
    return registry;
}

const MANAGED_MODULE_REGISTRY = managed_module_registry();
const MANAGED_MODULE_LOADS_BY_ID = MANAGED_MODULE_REGISTRY.loads_by_id;
const MANAGED_MODULE_LOADS_BY_KEY = MANAGED_MODULE_REGISTRY.loads_by_key;
const MANAGED_MODULE_INIT_CONTEXTS = MANAGED_MODULE_REGISTRY.init_contexts;
const MANAGED_MODULE_RUNTIME_INSTANCE = {};

// ---------------------------------------------------------------------------
// Microtask-batched scheduler
// ---------------------------------------------------------------------------
//
// `signal.set()` enqueues subscribers into `pending_effects` instead of
// running them synchronously. The first enqueue per tick schedules a
// `queueMicrotask(flush)`; the flush drains the set, so multiple writes
// to one or more signals in the same tick collapse to one effect run
// per subscriber. Synchronous reads (`.get()` / `.peek()`) still see
// the latest value; only the DOM commit is deferred. Without this,
// a handler that writes 3 signals causes 3 effect runs and 3 DOM
// commits per click. This is also a prerequisite for `signal_array`,
// where push / splice / replace will write many slots in a tight loop.
//
// Write-during-flush: an effect that calls `signal.set()` schedules the
// downstream subscriber for the NEXT microtask. We snapshot-and-clear
// `pending_effects` before iterating, so re-entrant writes don't
// recurse within a single flush.
const pending_effects = new Set();
let scheduled = false;
let batch_depth = 0;
// Non-null only while one owner initializer is running synchronously. Form
// bindings adopt dirty DOM state immediately but defer their first
// signal-to-DOM commit until every control in the owner has reconciled.
let ACTIVE_OWNER_INITIALIZATION = null;

function schedule_flush() {
    if (scheduled || batch_depth !== 0 || pending_effects.size === 0) {
        return;
    }
    scheduled = true;
    queueMicrotask(flush);
}

function schedule(effect) {
    if (effect?.active === false) {
        return;
    }
    pending_effects.add(effect);
    schedule_flush();
}

function flush() {
    const batch = Array.from(pending_effects);
    pending_effects.clear();
    scheduled = false;
    for (const effect of batch) {
        if (effect?.active !== false) {
            effect.run();
        }
    }
}

/**
 * Run a group of synchronous writes as one scheduler batch.
 *
 * Reads observe every write immediately. Nested batches share the outer
 * boundary, and all subscriber/effect/DOM work is deduplicated into the next
 * microtask after that outer callback returns. A throw preserves prior writes
 * and is rethrown after scheduler state is restored. Async callbacks and
 * Promise-returning callbacks are rejected because a batch cannot span await.
 */
export function batch(run) {
    if (typeof run !== 'function') {
        throw new TypeError('rsjs/batch: expected a synchronous callback');
    }
    // Reject native async callbacks before invoking them. The thenable check
    // below also catches ordinary functions that return a Promise, but such a
    // function may already have performed a synchronous prefix before its
    // return value reveals the mistake.
    if (Object.prototype.toString.call(run) === '[object AsyncFunction]') {
        throw new TypeError('rsjs/batch: async callbacks are not supported');
    }

    batch_depth += 1;
    try {
        const result = run();
        if (
            result !== null &&
            (typeof result === 'object' || typeof result === 'function') &&
            typeof result.then === 'function'
        ) {
            throw new TypeError(
                'rsjs/batch: callbacks must complete synchronously and cannot return a Promise',
            );
        }
        return result;
    } finally {
        batch_depth -= 1;
        if (batch_depth === 0) {
            schedule_flush();
        }
    }
}

function rust_float_text(value) {
    if (value === Infinity) return 'inf';
    if (value === -Infinity) return '-inf';
    return value == null ? '' : String(value);
}

/** Compiler-owned text! layout; values remain raw text for DOM bindings. */
export function format_text(_template, plan, ...values) {
    return render_text_plan(plan, values, false);
}

/** Attribute slots preserve Rust AttrValue's bare boolean semantics. */
export function format_text_attr(_template, plan, ...values) {
    return render_text_plan(plan, values, true);
}

function render_text_plan(plan, values, attribute) {
    return JSON.parse(plan).map((part) => {
        if (Object.hasOwn(part, 'text')) return part.text;
        const value = values[part.arg];
        if (!Object.hasOwn(part, 'width')) {
            if (attribute && part.attr_bool && typeof value === 'boolean') return '';
            return part.float || typeof value === 'number' ? rust_float_text(value) : String(value ?? '');
        }
        let text;
        let sign = '';
        let prefix = '';
        if (part.domain !== null) {
            let integer = BigInt(value);
            if (part.kind !== 0 && integer < 0n) {
                integer = BigInt.asUintN(part.domain.endsWith('32') ? 32 : 64, integer);
            }
            if (integer < 0n) {
                sign = '-';
                integer = -integer;
            } else if (part.plus) {
                sign = '+';
            }
            const radix = [10, 2, 8, 16, 16][part.kind];
            text = integer.toString(radix);
            if (part.kind === 4) text = text.toUpperCase();
            if (part.alternate) prefix = ['', '0b', '0o', '0x', '0x'][part.kind];
        } else {
            text = String(value ?? '');
            if (part.precision !== null) text = Array.from(text).slice(0, part.precision).join('');
        }
        const padding = Math.max(0, part.width - Array.from(sign + prefix + text).length);
        if (part.zero && part.domain !== null) return sign + prefix + '0'.repeat(padding) + text;
        text = sign + prefix + text;
        const align = part.align || (part.domain === null ? 1 : 2);
        const left = align === 1 ? 0 : align === 2 ? padding : Math.floor(padding / 2);
        return part.fill.repeat(left) + text + part.fill.repeat(padding - left);
    }).join('');
}

/**
 * Create a new reactive signal cell.
 *
 * Signals expose a tiny API:
 *   - .get()       — read the current value, registering CURRENT_EFFECT
 *                    (if any) as a subscriber.
 *   - .set(v)      — write a new value, notifying every current subscriber.
 *                    No-op if v is === to the previous value (reference
 *                    equality; primitives compare by value).
 *   - .update(fn)  — convenience wrapper for .set(fn(prev_value)). Reads
 *                    the previous value WITHOUT registering a subscription
 *                    (the closure body runs untracked).
 *   - .peek()      — read the current value without subscribing. Used by
 *                    .update internally and exposed for the rare case where
 *                    a binding wants the value without creating a dep.
 */
export function signal(initial) {
    let value = initial;
    const subscribers = new Set();
    const cell = {
        get() {
            if (CURRENT_EFFECT !== null) {
                subscribers.add(CURRENT_EFFECT);
                CURRENT_EFFECT.deps.add(cell);
            }
            return value;
        },
        set(next) {
            if (Object.is(value, next)) {
                return;
            }
            value = next;
            // Enqueue every subscriber for the next microtask flush
            // instead of running them inline. The synchronous read
            // path (`get()` / `peek()`) still observes `next`
            // immediately; only the DOM commit is deferred. See the
            // scheduler comment block above for the rationale and
            // write-during-flush semantics.
            for (const effect of subscribers) {
                schedule(effect);
            }
        },
        update(fn) {
            cell.set(fn(value));
        },
        peek() {
            return value;
        },
        // Internal: subscribe an effect explicitly. Used by bind_text when
        // the dep list is supplied at registration time so we do not have to
        // run the body once to discover deps.
        _subscribe(effect) {
            subscribers.add(effect);
            effect.active = effect.active !== false;
            effect.deps.add(cell);
            return () => {
                cell._unsubscribe(effect);
            };
        },
        _unsubscribe(effect) {
            subscribers.delete(effect);
            effect.deps.delete(cell);
        },
        // Internal page-lifecycle hook. SharedState fields use this to sever
        // every owner subscription and cancel already-queued work before the
        // field cell becomes invalid.
        _dispose_subscribers() {
            for (const effect of Array.from(subscribers)) {
                pending_effects.delete(effect);
                dispose_effect(effect);
            }
            subscribers.clear();
        },
    };
    return cell;
}

const INTEGER_DOMAIN_BOUNDS = Object.freeze({
    i32: Object.freeze([-2147483648n, 2147483647n, false]),
    u32: Object.freeze([0n, 4294967295n, false]),
    i64: Object.freeze([-9223372036854775808n, 9223372036854775807n, true]),
    u64: Object.freeze([0n, 18446744073709551615n, true]),
});

function integer_domain(domain) {
    const bounds = INTEGER_DOMAIN_BOUNDS[domain];
    if (bounds === undefined) {
        throw new TypeError(`rsjs checked integer uses unsupported domain ${String(domain)}`);
    }
    return bounds;
}

function integer_bigint(value, domain) {
    const [, , wide] = integer_domain(domain);
    if (wide ? typeof value !== 'bigint' : typeof value !== 'number' || !Number.isInteger(value)) {
        throw new TypeError(
            `rsjs checked ${domain} value must be a ${wide ? 'BigInt' : 'Number'} integer, got ${typeof value}`,
        );
    }
    const integer = wide ? value : BigInt(value);
    const [min, max] = integer_domain(domain);
    if (integer < min || integer > max) {
        throw new RangeError(
            `rsjs checked ${domain} value ${integer} is outside ${min}..=${max}`,
        );
    }
    return integer;
}

function integer_result(value, domain, operation) {
    const [min, max, wide] = integer_domain(domain);
    if (value < min || value > max) {
        throw new RangeError(`rsjs checked ${domain} ${operation} overflow`);
    }
    // Number-backed integer domains intentionally canonicalize zero. Rust has
    // one integer zero, while JavaScript's -0 is observable through Object.is
    // and would otherwise create spurious signal writes.
    return wide ? value : Number(value === 0n ? 0n : value);
}

function checked_integer_operands(a, b, domain) {
    return [integer_bigint(a, domain), integer_bigint(b, domain)];
}

/** Deterministic Rust fixed-width integer `+`. */
export function checked_int_add(a, b, domain) {
    const [lhs, rhs] = checked_integer_operands(a, b, domain);
    return integer_result(lhs + rhs, domain, 'add');
}

/** Deterministic Rust fixed-width integer `-`. */
export function checked_int_sub(a, b, domain) {
    const [lhs, rhs] = checked_integer_operands(a, b, domain);
    return integer_result(lhs - rhs, domain, 'sub');
}

/** Deterministic Rust fixed-width integer `*`. */
export function checked_int_mul(a, b, domain) {
    const [lhs, rhs] = checked_integer_operands(a, b, domain);
    return integer_result(lhs * rhs, domain, 'mul');
}

function checked_integer_divisor(lhs, rhs, domain, operation) {
    if (rhs === 0n) {
        throw new RangeError(`rsjs checked ${domain} ${operation} by zero`);
    }
    const [min] = integer_domain(domain);
    if ((domain === 'i32' || domain === 'i64') && lhs === min && rhs === -1n) {
        throw new RangeError(`rsjs checked ${domain} ${operation} overflow`);
    }
}

/** Deterministic Rust fixed-width integer `/`, truncating toward zero. */
export function checked_int_div(a, b, domain) {
    const [lhs, rhs] = checked_integer_operands(a, b, domain);
    checked_integer_divisor(lhs, rhs, domain, 'division');
    return integer_result(lhs / rhs, domain, 'division');
}

/** Deterministic Rust fixed-width integer `%`. */
export function checked_int_rem(a, b, domain) {
    const [lhs, rhs] = checked_integer_operands(a, b, domain);
    checked_integer_divisor(lhs, rhs, domain, 'remainder');
    return integer_result(lhs % rhs, domain, 'remainder');
}

/** Deterministic Rust fixed-width signed unary negation. */
export function checked_int_neg(value, domain) {
    if (domain !== 'i32' && domain !== 'i64') {
        throw new TypeError(`rsjs checked unary negation is not defined for ${domain}`);
    }
    return integer_result(-integer_bigint(value, domain), domain, 'negation');
}

/**
 * Create a signal whose state always retains one proven Rust integer domain.
 *
 * Coercion and range validation happen before the underlying cell mutates, so
 * a failed `.set(...)` or `.update(...)` leaves both state and subscribers
 * untouched. Wide domains stay BigInt-backed even after in-range literal or
 * event-derived writes.
 */
export function integer_signal(initial, domain) {
    const [, , wide] = integer_domain(domain);
    const normalize = (value) => {
        const integer = integer_bigint(value, domain);
        return wide ? integer : Number(integer === 0n ? 0n : integer);
    };
    const inner = signal(normalize(initial));
    return {
        get: inner.get,
        set(next) {
            inner.set(normalize(next));
        },
        update(fn) {
            const next = fn(inner.peek());
            inner.set(normalize(next));
        },
        peek: inner.peek,
        _subscribe: inner._subscribe,
        _unsubscribe: inner._unsubscribe,
    };
}

/**
 * Create a read-only derived cell whose value is `fn()` re-evaluated
 * whenever its inputs change.
 *
 * The result has the read half of the Signal interface (`.get()`,
 * `.peek()`, `_subscribe` / `_unsubscribe`) so bindings can list a
 * memo in their `deps` array exactly like a plain signal.
 * `.set()` / `.update()` exist but throw: derived state has no
 * writable identity, and failing loudly beats silently desyncing
 * the memo from its inputs.
 *
 * Dependency wiring follows bind_text:
 *   - `deps` supplied (the compile-time list from
 *     analysis::dep_collector): subscribe explicitly, then evaluate
 *     once untracked to seed the value.
 *   - `deps` omitted: tracked first run discovers the inputs.
 *
 * Recomputation is microtask-batched like every other effect, and the
 * inner cell's `Object.is` equality means downstream subscribers only
 * run when the derived value actually changed. A write to an input
 * during flush N recomputes during flush N; dependents of the memo
 * observe the new value at flush N+1 (write-during-flush semantics,
 * see the scheduler comment block).
 *
 * Returns the cell; its `_dispose()` unsubscribes the recompute effect
 * from every input. Island init code must push `_dispose` into the
 * island's disposer list alongside binding disposers.
 */
export function memo(fn, deps) {
    const inner = signal(undefined);
    const apply = () => {
        inner.set(fn());
    };
    let dispose;
    if (Array.isArray(deps)) {
        const effect = { active: true, deps: new Set(), run: apply };
        dispose = collect_reactive_disposers(effect, deps, null, apply);
        try {
            apply();
        } catch (error) {
            dispose();
            throw error;
        }
    } else {
        const effect = tracked_effect(apply);
        dispose = once_disposer(() => dispose_effect(effect));
    }
    return {
        get: inner.get,
        peek: inner.peek,
        set() {
            throw new Error(
                'rsjs: memo cells are read-only; write to one of the source signals instead',
            );
        },
        update() {
            throw new Error(
                'rsjs: memo cells are read-only; write to one of the source signals instead',
            );
        },
        _subscribe: inner._subscribe,
        _unsubscribe: inner._unsubscribe,
        _dispose: dispose,
    };
}

// ---------------------------------------------------------------------------
// Timers
// ---------------------------------------------------------------------------
//
// rsjs does not expose raw setTimeout / setInterval to island code; the
// sanctioned surface is these two primitives, both of which tie their
// pending work to the island's disposer list so a removed island never
// fires a stale callback. Covered by rsjs-runtime.timers.test.mjs.

/**
 * Run `fn` every `ms` milliseconds until disposed.
 *
 * Returns a disposer; island init code pushes it into the island's
 * disposer list so teardown clears the interval. `fn` runs untracked
 * (it is not an effect); signals it writes notify their subscribers
 * through the normal microtask-batched scheduler.
 */
export function interval(ms, fn) {
    const id = setInterval(fn, ms);
    return once_disposer(() => {
        clearInterval(id);
    });
}

/**
 * Wrap `fn` so calls collapse until `delay_ms` milliseconds pass without a
 * new call; the wrapper then invokes `fn` with the most recent
 * arguments. The trailing edge only: there is no leading-edge mode in
 * v0.1.
 *
 * The returned function carries a `_dispose` property that cancels any
 * pending invocation; island init code pushes it into the island's
 * disposer list so teardown never fires a stale handler.
 */
export function debounce(fn, delay_ms) {
    let id = null;
    const wrapped = (...args) => {
        if (id !== null) {
            clearTimeout(id);
        }
        id = setTimeout(() => {
            id = null;
            fn(...args);
        }, delay_ms);
    };
    wrapped._dispose = once_disposer(() => {
        if (id !== null) {
            clearTimeout(id);
            id = null;
        }
    });
    return wrapped;
}

const EMPTY_SIGNAL_INITIALS = Object.freeze([]);
const EMPTY_SHARED_BINDINGS = Object.freeze([]);
let ISLAND_STATE_CACHE = new WeakMap();

function read_island_state(root) {
    if (root == null || (typeof root !== 'object' && typeof root !== 'function')) {
        return null;
    }
    if (ISLAND_STATE_CACHE.has(root)) {
        return ISLAND_STATE_CACHE.get(root);
    }
    const script = signal_state_script_for(root);
    if (script === null) {
        ISLAND_STATE_CACHE.set(root, null);
        return null;
    }
    try {
        const parsed = JSON.parse(script.textContent || '{}');
        const state = parsed && typeof parsed === 'object' ? parsed : null;
        ISLAND_STATE_CACHE.set(root, state);
        return state;
    } catch (err) {
        console.warn('rsjs: failed to parse island state blob', err);
        ISLAND_STATE_CACHE.set(root, null);
        return null;
    }
}

/**
 * Read this island instance's signal seed array from the adjacent
 * `<script type="application/json" data-rsjs-island="...">` blob.
 * Missing or malformed blobs fall back to an empty array so emitted
 * islands can still run in dev/smoke contexts where only the JS was
 * loaded.
 */
export function read_signal_initials(root) {
    const parsed = read_island_state(root);
    if (parsed && Array.isArray(parsed.signals)) {
        return parsed.signals;
    }
    return EMPTY_SIGNAL_INITIALS;
}

/** Read the shared-provider requirements resolved for this island instance. */
export function read_shared_state_bindings(root) {
    const parsed = read_island_state(root);
    if (parsed && Array.isArray(parsed.shared)) {
        return parsed.shared;
    }
    return EMPTY_SHARED_BINDINGS;
}

// ---------------------------------------------------------------------------
// Typed page-scoped shared state
// ---------------------------------------------------------------------------

const SHARED_STATE_ABI_VERSION = 1;
// Lifecycle bridge version is intentionally independent: routers depend on
// disposePage/installPage, not on the internal seed-table wire shape.
const PAGE_RUNTIME_ABI_VERSION = 1;
const PAGE_GENERATION_DISPOSERS_SYMBOL =
    typeof Symbol === 'undefined'
        ? null
        : Symbol.for('rsjs.page-generation-disposers');
const PAGE_GENERATION_STATE_SYMBOL =
    typeof Symbol === 'undefined'
        ? null
        : Symbol.for('rsjs.page-generation-state');

function page_generation_state() {
    if (
        typeof globalThis === 'undefined' ||
        PAGE_GENERATION_STATE_SYMBOL === null
    ) {
        return { response_epoch: 1, navigation_generation: 0 };
    }
    const current = globalThis[PAGE_GENERATION_STATE_SYMBOL];
    if (
        current !== null &&
        typeof current === 'object' &&
        Number.isSafeInteger(current.response_epoch) &&
        Number.isSafeInteger(current.navigation_generation)
    ) {
        return current;
    }
    const state = { response_epoch: 1, navigation_generation: 0 };
    globalThis[PAGE_GENERATION_STATE_SYMBOL] = state;
    return state;
}

const PAGE_GENERATION_STATE = page_generation_state();

function capture_page_generation() {
    return {
        response_epoch: PAGE_GENERATION_STATE.response_epoch,
        navigation_generation: PAGE_GENERATION_STATE.navigation_generation,
    };
}

function page_generation_is_current(generation) {
    return (
        generation !== null &&
        typeof generation === 'object' &&
        generation.response_epoch === PAGE_GENERATION_STATE.response_epoch &&
        generation.navigation_generation ===
            PAGE_GENERATION_STATE.navigation_generation
    );
}

function advance_page_generation() {
    PAGE_GENERATION_STATE.navigation_generation += 1;
    if (!Number.isSafeInteger(PAGE_GENERATION_STATE.navigation_generation)) {
        PAGE_GENERATION_STATE.response_epoch += 1;
        PAGE_GENERATION_STATE.navigation_generation = 0;
    }
}

function page_generation_disposer_registry() {
    if (
        typeof globalThis === 'undefined' ||
        PAGE_GENERATION_DISPOSERS_SYMBOL === null
    ) {
        return new Set();
    }
    const current = globalThis[PAGE_GENERATION_DISPOSERS_SYMBOL];
    if (current instanceof Set) return current;
    const registry = new Set();
    globalThis[PAGE_GENERATION_DISPOSERS_SYMBOL] = registry;
    return registry;
}

// Optional runtime modules (queries today, other page-scoped resources later)
// register here without forcing the always-loaded core runtime to import them.
// The Symbol-backed registry makes module evaluation order irrelevant.
const PAGE_GENERATION_DISPOSERS = page_generation_disposer_registry();

/**
 * Register a resource owned by the complete response/navigation generation.
 * The returned function unregisters it; registration itself persists across
 * page transitions because the optional runtime module remains evaluated.
 */
export function register_page_generation_disposer(dispose) {
    if (typeof dispose !== 'function') {
        throw new TypeError(
            'rsjs/page-generation-disposer: expected a disposer function',
        );
    }
    PAGE_GENERATION_DISPOSERS.add(dispose);
    let active = true;
    return () => {
        if (!active) return;
        active = false;
        PAGE_GENERATION_DISPOSERS.delete(dispose);
    };
}

function dispose_page_generation_resources() {
    for (const dispose of Array.from(PAGE_GENERATION_DISPOSERS)) {
        try {
            dispose();
        } catch (error) {
            report_hydration_error('page generation resource teardown', error);
        }
    }
}

let PAGE_SHARED_STORES = new Map();
let PAGE_SHARED_TABLE_PARSED = false;
// The DOM element the current store table was parsed from. Page stores are
// page-scoped: when this element leaves the document (a DOM-replacement
// navigation that relies on the runtime's automatic detached-root pruning
// instead of an explicit dispose_hydrated() call), the stores it seeded are
// stale and must be rebuilt from the new page's seed table. Null when the
// table was installed programmatically; adapters that install their own
// table object also own its teardown.
let PAGE_SHARED_TABLE_ANCHOR = null;
// The exact JSON text the anchor held when its stores were installed. A
// morphing patcher (idiomorph and friends) keeps the seed script node and
// rewrites its text in place, so node identity alone cannot tell page A from
// page B; the parsed content can. Null whenever there is no anchor.
let PAGE_SHARED_TABLE_SOURCE = null;

const PAGE_SEED_TABLE_ID = 'rsjs-shared-state-seed-table';
// Marks a seed table that describes an injected fragment instead of the
// document. See is_fragment_seed_table().
const SEED_TABLE_SCOPE_ATTRIBUTE = 'data-rsjs-seed-scope';

export class RsjsSharedStateError extends Error {
    constructor(code, message, details = {}) {
        super(`rsjs/${code}: ${message}`);
        this.name = 'RsjsSharedStateError';
        this.code = `rsjs/${code}`;
        this.details = details;
    }
}

function shared_state_error(code, message, details) {
    return new RsjsSharedStateError(code, message, details);
}

/**
 * Whether a seed value listed in a store's `wide` array can be decoded to
 * BigInt. Wide-int fields (the Rust i64 family) travel as JSON strings
 * because JSON numbers round past 2^53; `null` (an absent `Option`) and
 * already-BigInt values pass through, and bare integer numbers are accepted
 * defensively for hand-written tables.
 */
function is_decodable_wide_seed_value(value) {
    if (value === null || typeof value === 'bigint') {
        return true;
    }
    if (
        typeof value !== 'string' &&
        (typeof value !== 'number' || !Number.isSafeInteger(value))
    ) {
        return false;
    }
    try {
        BigInt(value);
        return true;
    } catch (_) {
        return false;
    }
}

/**
 * Sentinel returned by `coerce_to_repr` for a write that has no image in the
 * cell's representation. Such a write is dropped rather than stored: mid-typing
 * an input can legitimately hold `"-"` on the way to `"-5"`, and storing that
 * string would poison the cell for every other island on the page.
 */
const REPR_REJECT = Symbol('rsjs.repr-reject');

/**
 * Coerce a value into a shared field cell's pinned representation.
 *
 * `null` / `undefined` pass through: an absent `Option` is a legitimate value
 * of any representation.
 */
function coerce_to_repr(next, repr) {
    if (next === null || next === undefined) return next;
    const t = typeof next;
    if (t === repr) return next;
    if (repr === 'bigint') {
        if (t === 'number') return Number.isInteger(next) ? BigInt(next) : REPR_REJECT;
        if (t === 'boolean') return next ? 1n : 0n;
        if (t === 'string') {
            const trimmed = next.trim();
            if (trimmed === '') return 0n;
            try {
                return BigInt(trimmed);
            } catch (_) {
                return REPR_REJECT;
            }
        }
        return REPR_REJECT;
    }
    if (repr === 'number') {
        if (t === 'bigint') return Number(next);
        if (t === 'boolean') return next ? 1 : 0;
        if (t === 'string') {
            const trimmed = next.trim();
            if (trimmed === '') return 0;
            const parsed = Number(trimmed);
            return Number.isNaN(parsed) ? REPR_REJECT : parsed;
        }
        return REPR_REJECT;
    }
    return next;
}

/**
 * A shared field cell whose representation is pinned by the page seed.
 *
 * This is what makes the compiler's `num_*` dispatch sound. The island JS was
 * compiled without knowing whether the browser holds this field as a BigInt or
 * a Number, so the cell must guarantee that whatever is *in* it keeps the
 * representation the seed established. Every write goes through `set`
 * (`update`, `batch`, and `bind_input` all call it), so pinning `set` pins the
 * cell.
 *
 * `repr` comes from the seed's `wide_paths`, not from `typeof initial`: an
 * `Option<i64>` seeded `null` is `typeof "object"` and would otherwise never be
 * pinned, and the first `set(5)` would lodge a Number in a BigInt field.
 */
function shared_field_cell(initial, repr, nested_wide_paths, identity) {
    const cell = signal(initial);
    const nested = nested_wide_paths ?? [];
    const base_get = cell.get;
    const base_peek = cell.peek;
    const base_set = cell.set;
    const base_subscribe = cell._subscribe;
    let active = true;
    const assert_live = () => {
        if (!active) {
            throw shared_state_error(
                'shared-state-disposed',
                `provider scope ${String(identity.scope_id)} belongs to a disposed page generation`,
                identity,
            );
        }
    };
    const set_live_value = (next) => {
        if (repr === 'bigint' || repr === 'number') {
            const coerced = coerce_to_repr(next, repr);
            if (coerced !== REPR_REJECT) {
                base_set(coerced);
            }
            return;
        }
        // A whole-object or whole-array write into a field that contains wide
        // integers must re-decode them, or the nested cells silently revert to
        // Numbers and lose exactness past 2^53. The seed already names those
        // paths; `decode_wide_path` is the same walk the initial seed used.
        let decoded = next;
        for (const path of nested) {
            try {
                decoded = decode_wide_path(decoded, path, 0);
            } catch (_) {
                // A value the wide walk cannot decode is stored as written:
                // dropping a structural write would lose the user's data, and
                // the `num_*` helpers stay correct on Numbers.
                decoded = next;
                break;
            }
        }
        base_set(decoded);
    };
    cell.get = () => {
        assert_live();
        return base_get();
    };
    cell.peek = () => {
        assert_live();
        return base_peek();
    };
    cell.set = (next) => {
        assert_live();
        set_live_value(next);
    };
    cell.update = (update) => {
        assert_live();
        cell.set(update(base_peek()));
    };
    cell._subscribe = (effect) => {
        assert_live();
        return base_subscribe(effect);
    };
    cell._dispose_shared_state = () => {
        if (!active) return;
        active = false;
        cell._dispose_subscribers();
    };
    if (repr === 'bigint' || repr === 'number') {
        cell.__rsjs_repr = repr;
    }
    return cell;
}

/** Decode one wide-int seed value validated by is_decodable_wide_seed_value. */
function decode_wide_seed_value(value) {
    if (value === null || typeof value === 'bigint') {
        return value;
    }
    return BigInt(value);
}

/** Normalize v1's top-level `wide` names and the recursive `wide_paths` form. */
function normalize_wide_paths(seed, scope_key) {
    if (seed.wide !== undefined && seed.wide_paths !== undefined) {
        throw shared_state_error(
            'shared-state-invalid-seed',
            `scope ${scope_key} carries both "wide" and "wide_paths" metadata`,
            { scope_id: seed.scope_id, type_key: seed.type_key },
        );
    }
    if (seed.wide !== undefined) {
        if (!Array.isArray(seed.wide) || seed.wide.some((name) => typeof name !== 'string')) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `scope ${scope_key} must encode legacy wide fields as "wide":["field",...]`,
                { scope_id: seed.scope_id, type_key: seed.type_key },
            );
        }
        return seed.wide.map((name) => [name]);
    }
    if (seed.wide_paths === undefined) return [];
    if (!Array.isArray(seed.wide_paths)) {
        throw shared_state_error(
            'shared-state-invalid-seed',
            `scope ${scope_key} carries a "wide_paths" entry that is not an array`,
            { scope_id: seed.scope_id, type_key: seed.type_key },
        );
    }
    for (const path of seed.wide_paths) {
        if (
            !Array.isArray(path) ||
            path.length === 0 ||
            path.some((segment) => typeof segment !== 'string' || segment.length === 0)
        ) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `scope ${scope_key} has an invalid wide path; every path must contain ` +
                    'one or more non-empty string segments',
                { scope_id: seed.scope_id, type_key: seed.type_key, path },
            );
        }
    }
    return seed.wide_paths;
}

function canonical_shared_state_seed_value(value, path, stack) {
    if (value === null) {
        return 'null';
    }
    if (typeof value === 'string') {
        return `string:${JSON.stringify(value)}`;
    }
    if (typeof value === 'boolean') {
        return value ? 'boolean:true' : 'boolean:false';
    }
    if (typeof value === 'number') {
        if (!Number.isFinite(value)) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `shared-state seed value at ${path} is not a finite JSON number`,
            );
        }
        return `number:${JSON.stringify(Object.is(value, -0) ? 0 : value)}`;
    }
    // BigInt is accepted defensively by the wide-value decoder for tables
    // installed programmatically, even though the JSON wire representation is
    // a string. Tag it here so it cannot collide with an ordinary string.
    if (typeof value === 'bigint') {
        return `bigint:${value.toString()}`;
    }
    if (typeof value !== 'object') {
        throw shared_state_error(
            'shared-state-invalid-seed',
            `shared-state seed value at ${path} cannot be represented in JSON`,
        );
    }
    if (stack.has(value)) {
        throw shared_state_error(
            'shared-state-invalid-seed',
            `shared-state seed value at ${path} contains a cycle`,
        );
    }
    stack.add(value);
    try {
        if (Array.isArray(value)) {
            return (
                'array:[' +
                value
                    .map((item, index) =>
                        canonical_shared_state_seed_value(
                            item,
                            `${path}[${index}]`,
                            stack,
                        ),
                    )
                    .join(',') +
                ']'
            );
        }
        if (Object.prototype.toString.call(value) !== '[object Object]') {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `shared-state seed value at ${path} is not a plain object`,
            );
        }
        return (
            'object:{' +
            Object.keys(value)
                .sort()
                .map(
                    (key) =>
                        `${JSON.stringify(key)}:${canonical_shared_state_seed_value(
                            value[key],
                            `${path}.${key}`,
                            stack,
                        )}`,
                )
                .join(',') +
            '}'
        );
    } finally {
        stack.delete(value);
    }
}

function shared_state_seed_fingerprint(seed, wide_paths) {
    const canonical_wide_paths = wide_paths
        .map((path) => path.slice())
        .sort((left, right) => {
            const left_json = JSON.stringify(left);
            const right_json = JSON.stringify(right);
            return left_json < right_json ? -1 : left_json > right_json ? 1 : 0;
        });
    return (
        canonical_shared_state_seed_value(seed.state, 'state', new Set()) +
        `;wide_paths:${JSON.stringify(canonical_wide_paths)}`
    );
}

function validate_wide_path(value, path, index, seed) {
    // A null encountered before the terminal is an absent Option at some
    // level of the codec. There is no integer payload to decode.
    if (value === null) return;
    if (index === path.length) {
        if (!is_decodable_wide_seed_value(value)) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `scope ${String(seed.scope_id)} wide path ${JSON.stringify(path)} ends at a ` +
                    'value that cannot be decoded to BigInt; expected an integer JSON string or null',
                { scope_id: seed.scope_id, type_key: seed.type_key, path },
            );
        }
        return;
    }

    const segment = path[index];
    if (segment === '*') {
        if (!Array.isArray(value)) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `scope ${String(seed.scope_id)} wide path ${JSON.stringify(path)} expected an ` +
                    `array at segment ${index}`,
                { scope_id: seed.scope_id, type_key: seed.type_key, path },
            );
        }
        for (const item of value) validate_wide_path(item, path, index + 1, seed);
        return;
    }
    if (
        !value ||
        typeof value !== 'object' ||
        Array.isArray(value) ||
        !Object.prototype.hasOwnProperty.call(value, segment)
    ) {
        throw shared_state_error(
            'shared-state-invalid-seed',
            `scope ${String(seed.scope_id)} wide path ${JSON.stringify(path)} cannot resolve ` +
                `field ${JSON.stringify(segment)}`,
            { scope_id: seed.scope_id, type_key: seed.type_key, path, field: segment },
        );
    }
    validate_wide_path(value[segment], path, index + 1, seed);
}

function decode_wide_path(value, path, index) {
    if (value === null) return null;
    if (index === path.length) return decode_wide_seed_value(value);
    const segment = path[index];
    if (segment === '*') {
        return value.map((item) => decode_wide_path(item, path, index + 1));
    }
    return {
        ...value,
        [segment]: decode_wide_path(value[segment], path, index + 1),
    };
}

/**
 * Install one page seed table. Existing live stores win: calling this after a
 * field write never resets that field to its SSR value. Framework adapters may
 * call this explicitly for client-side route transitions; ordinary full-page
 * hydration installs lazily from the inert JSON script.
 *
 * Seed format: each store carries `scope_id`, `type_key`, `schema_hash`,
 * `rust_type`, and a `state` object. A store whose schema has wide-int
 * fields (the Rust i64 family, whose wire form is a JSON string) also
 * carries `wide_paths: [["field", "*", "nested"], ...]`; `"*"` traverses
 * array elements. Legacy top-level `wide` names are accepted as one-segment
 * paths. This
 * mirrors the `signal_initial_wide` convention for island-local wide
 * signals, so client arithmetic on a wide shared field keeps exact integer
 * semantics instead of concatenating strings.
 */
export function install_page_shared_state(table) {
    reset_page_shared_state_if_stale();
    if (!table || typeof table !== 'object') {
        throw shared_state_error(
            'shared-state-invalid-table',
            'the page shared-state seed must be an object',
        );
    }
    if (table.abi_version !== SHARED_STATE_ABI_VERSION) {
        throw shared_state_error(
            'shared-state-abi-mismatch',
            `expected ABI ${SHARED_STATE_ABI_VERSION}, received ${String(table.abi_version)}`,
            { expected: SHARED_STATE_ABI_VERSION, actual: table.abi_version },
        );
    }
    if (!Array.isArray(table.stores)) {
        throw shared_state_error(
            'shared-state-invalid-table',
            'the page shared-state seed is missing its stores array',
        );
    }

    // Validate the complete table before allocating any cells. Hydration must
    // never observe a partially installed page if a later seed is malformed.
    const incoming = new Map();
    const wide_paths_by_scope = new Map();
    const schemas_by_type_key = new Map();
    for (const store of PAGE_SHARED_STORES.values()) {
        const registered = schemas_by_type_key.get(store.type_key);
        if (registered !== undefined && registered !== store.schema_hash) {
            throw shared_state_error(
                'shared-state-type-key-conflict',
                `protocol key ${JSON.stringify(store.type_key)} is already installed with ` +
                    'more than one schema; dispose the previous page before installing a new one',
                { type_key: store.type_key },
            );
        }
        schemas_by_type_key.set(store.type_key, store.schema_hash);
    }
    for (const seed of table.stores) {
        if (!seed || typeof seed !== 'object') {
            throw shared_state_error(
                'shared-state-invalid-seed',
                'a shared-state store seed is not an object',
            );
        }
        if (!Number.isSafeInteger(seed.scope_id) || seed.scope_id < 0) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                'a shared-state store seed has an invalid scope_id',
                { scope_id: seed.scope_id, type_key: seed.type_key },
            );
        }
        if (
            typeof seed.type_key !== 'string' ||
            typeof seed.schema_hash !== 'string' ||
            typeof seed.rust_type !== 'string'
        ) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `scope ${String(seed.scope_id)} is missing string schema metadata`,
                { scope_id: seed.scope_id },
            );
        }
        const scope_key = String(seed.scope_id);
        if (incoming.has(scope_key)) {
            throw shared_state_error(
                'shared-state-duplicate-scope',
                `scope ${scope_key} appears more than once in the same page seed table`,
                { scope_id: seed.scope_id, type_key: seed.type_key },
            );
        }
        if (!seed.state || typeof seed.state !== 'object' || Array.isArray(seed.state)) {
            throw shared_state_error(
                'shared-state-invalid-seed',
                `scope ${scope_key} does not contain an object state snapshot`,
                { scope_id: seed.scope_id, type_key: seed.type_key },
            );
        }
        const existing = PAGE_SHARED_STORES.get(scope_key);
        if (existing) {
            if (
                existing.type_key !== seed.type_key ||
                existing.schema_hash !== seed.schema_hash
            ) {
                throw shared_state_error(
                    'shared-state-scope-collision',
                    `scope ${scope_key} was registered with incompatible schema metadata`,
                    { scope_id: seed.scope_id, type_key: seed.type_key },
                );
            }
        }
        const registered_schema = schemas_by_type_key.get(seed.type_key);
        if (registered_schema !== undefined && registered_schema !== seed.schema_hash) {
            throw shared_state_error(
                'shared-state-type-key-conflict',
                `protocol key ${JSON.stringify(seed.type_key)} identifies both ` +
                    `${registered_schema} and ${seed.schema_hash}; one type_key must name ` +
                    'exactly one wire schema on a page',
                {
                    scope_id: seed.scope_id,
                    type_key: seed.type_key,
                    registered_schema,
                    incoming_schema: seed.schema_hash,
                },
            );
        }
        schemas_by_type_key.set(seed.type_key, seed.schema_hash);
        const wide_paths = normalize_wide_paths(seed, scope_key);
        for (const path of wide_paths) validate_wide_path(seed.state, path, 0, seed);
        const seed_fingerprint = shared_state_seed_fingerprint(seed, wide_paths);
        if (existing && existing.seed_fingerprint !== seed_fingerprint) {
            throw shared_state_error(
                'shared-state-seed-conflict',
                `scope ${scope_key} was published with a different canonical initial seed`,
                { scope_id: seed.scope_id, type_key: seed.type_key },
            );
        }
        wide_paths_by_scope.set(scope_key, wide_paths);
        incoming.set(scope_key, { seed, seed_fingerprint });
    }

    for (const [scope_key, incoming_store] of incoming) {
        if (PAGE_SHARED_STORES.has(scope_key)) continue;
        const { seed, seed_fingerprint } = incoming_store;
        const fields = new Map();
        let decoded_state = seed.state;
        for (const path of wide_paths_by_scope.get(scope_key) ?? []) {
            decoded_state = decode_wide_path(decoded_state, path, 0);
        }
        // Pin each top-level field cell to the representation the seed
        // established. The compiled island JS never asks which one it is; the
        // `num_*` helpers dispatch on it at run time, which is only sound if
        // the cell cannot drift.
        const scope_wide_paths = wide_paths_by_scope.get(scope_key) ?? [];
        const top_level_wide = new Set(
            scope_wide_paths.filter((path) => path.length === 1).map((path) => path[0]),
        );
        // Wide paths *inside* a field, with the field segment stripped: a
        // whole-object write to that field has to re-decode them.
        const nested_wide = new Map();
        for (const path of scope_wide_paths) {
            if (path.length < 2) continue;
            const [field, ...rest] = path;
            if (!nested_wide.has(field)) nested_wide.set(field, []);
            nested_wide.get(field).push(rest);
        }
        for (const [name, value] of Object.entries(decoded_state)) {
            const repr = top_level_wide.has(name)
                ? 'bigint'
                : typeof value === 'number'
                  ? 'number'
                  : undefined;
            fields.set(
                name,
                shared_field_cell(value, repr, nested_wide.get(name), {
                    scope_id: seed.scope_id,
                    type_key: seed.type_key,
                    field: name,
                }),
            );
        }
        PAGE_SHARED_STORES.set(scope_key, {
            active: true,
            scope_id: seed.scope_id,
            type_key: seed.type_key,
            schema_hash: seed.schema_hash,
            rust_type: seed.rust_type,
            seed_fingerprint,
            fields,
        });
    }
    PAGE_SHARED_TABLE_PARSED = true;
}

/**
 * Whether a seed-table element describes an injected fragment rather than the
 * document it now lives in.
 *
 * A fragment response (a partial render, an `X-Proa-Client` payload patched
 * into a container) carries its own copy of the seed table so the islands it
 * ships can find their provider scopes. That table describes the fragment
 * only: it is never the document's table, so it must not become the staleness
 * anchor and must not be mistaken for evidence that the page navigated.
 * Emitters mark it with `data-rsjs-seed-scope="fragment"`; a missing attribute
 * (or `"page"`) means a full-document table, which keeps older SSR output
 * working unchanged.
 */
function is_fragment_seed_table(element) {
    return (
        element != null &&
        typeof element.getAttribute === 'function' &&
        element.getAttribute(SEED_TABLE_SCOPE_ATTRIBUTE) === 'fragment'
    );
}

/**
 * Every seed-table element currently in the document, in tree order.
 *
 * `getElementById` returns only the first match, and a page that has taken a
 * fragment patch legitimately holds more than one element with this id, so
 * prefer `querySelectorAll` when the host document implements it.
 */
function seed_table_elements() {
    if (typeof document === 'undefined') return [];
    if (typeof document.querySelectorAll === 'function') {
        let found = null;
        try {
            found = document.querySelectorAll(`#${PAGE_SEED_TABLE_ID}`);
        } catch (error) {
            found = null;
        }
        if (found != null && found.length > 0) {
            return Array.from(found);
        }
    }
    if (typeof document.getElementById !== 'function') return [];
    const element = document.getElementById(PAGE_SEED_TABLE_ID);
    return element == null ? [] : [element];
}

/** The document's own seed table: the first one not marked as a fragment. */
function page_seed_table_of(elements) {
    for (const element of elements) {
        if (!is_fragment_seed_table(element)) return element;
    }
    return null;
}

/**
 * Whether the installed page store table no longer describes what is on the
 * page.
 *
 * Two shapes count as stale, and nothing else does:
 *
 * 1. The anchor left the document. SPA adapters may replace the whole DOM and
 *    rely on the runtime's automatic detached-root pruning instead of calling
 *    dispose_hydrated(); the seed script goes with the old DOM.
 * 2. The anchor is still the same node but its JSON changed. A morphing
 *    patcher keeps the script element (the stable `id` is exactly its match
 *    key) and rewrites `textContent` in place, so identity is not enough.
 *
 * A *second* element carrying the same id is deliberately not staleness: a
 * fragment patched into the live page brings its own seed table, and adopting
 * it would throw away stores that already-hydrated islands hold references to,
 * splitting one logical scope into two disjoint cells.
 */
function page_seed_table_is_stale() {
    const anchor = PAGE_SHARED_TABLE_ANCHOR;
    if (typeof anchor.isConnected === 'boolean' && !anchor.isConnected) return true;
    if (typeof anchor.textContent !== 'string') return false;
    return (anchor.textContent || '{}') !== PAGE_SHARED_TABLE_SOURCE;
}

/**
 * Drop the page store table when the document it was parsed from moved on.
 *
 * Scope ids restart at 0 on every server render, so without this check the
 * next page's islands would silently bind the previous page's stores (same
 * type_key, same schema) and its fresh seed table would never install. While
 * the anchor element stays connected and unchanged the stores stay live,
 * which is what keeps late hydration on an untouched page working.
 */
function reset_page_shared_state_if_stale() {
    if (!PAGE_SHARED_TABLE_PARSED || PAGE_SHARED_TABLE_ANCHOR === null) return;
    if (!page_seed_table_is_stale()) return;
    dispose_shared_state_stores();
    PAGE_SHARED_TABLE_PARSED = false;
    PAGE_SHARED_TABLE_ANCHOR = null;
    PAGE_SHARED_TABLE_SOURCE = null;
}

function seed_page_shared_state_table() {
    reset_page_shared_state_if_stale();
    if (PAGE_SHARED_TABLE_PARSED) return;
    const elements = seed_table_elements();
    if (elements.length === 0) return;
    const page_table = page_seed_table_of(elements);
    // With no document table at all, a fragment's table is the only seed
    // available; install it so the fragment's islands can resolve their
    // scopes, but leave the anchor null so a later document table is not
    // judged against a fragment's contents.
    const script = page_table ?? elements[0];
    const source = script.textContent || '{}';
    let parsed;
    try {
        parsed = JSON.parse(source);
    } catch (error) {
        throw shared_state_error(
            'shared-state-seed-decode-failed',
            'the page shared-state seed table is not valid JSON',
            { cause: error },
        );
    }
    install_page_shared_state(parsed);
    if (page_table === null) return;
    PAGE_SHARED_TABLE_ANCHOR = script;
    PAGE_SHARED_TABLE_SOURCE = source;
}

/** Install the current document's inert page seed after a client route swap. */
export function install_page_shared_state_from_document() {
    seed_page_shared_state_table();
}

/**
 * Acquire the exact provider scope SSR resolved for this island instance.
 * `slot` is the compile-time store slot in the adjacent island state blob.
 */
export function acquire_shared_state(root, slot) {
    seed_page_shared_state_table();
    const bindings = read_shared_state_bindings(root);
    const binding = bindings[slot];
    if (!binding || typeof binding !== 'object') {
        throw shared_state_error(
            'shared-state-missing-binding',
            `island hydration metadata has no shared-state binding at slot ${slot}`,
            { slot },
        );
    }
    const scope_key = String(binding.scope_id);
    const store = PAGE_SHARED_STORES.get(scope_key);
    if (!store) {
        throw shared_state_error(
            'shared-state-missing-scope',
            `provider scope ${scope_key} is absent from the page seed table`,
            { slot, scope_id: binding.scope_id, type_key: binding.type_key },
        );
    }
    if (
        store.type_key !== binding.type_key ||
        store.schema_hash !== binding.schema_hash
    ) {
        throw shared_state_error(
            'shared-state-schema-mismatch',
            `provider scope ${scope_key} does not match this island's compiled schema`,
            {
                slot,
                scope_id: binding.scope_id,
                expected_type_key: binding.type_key,
                actual_type_key: store.type_key,
                expected_schema: binding.schema_hash,
                actual_schema: store.schema_hash,
            },
        );
    }
    return {
        scope_id: store.scope_id,
        type_key: store.type_key,
        schema_hash: store.schema_hash,
        field(name) {
            if (!store.active) {
                throw shared_state_error(
                    'shared-state-disposed',
                    `provider scope ${scope_key} belongs to a disposed page generation`,
                    { scope_id: store.scope_id, type_key: store.type_key, field: name },
                );
            }
            const field = store.fields.get(name);
            if (!field) {
                throw shared_state_error(
                    'shared-state-missing-field',
                    `schema ${store.type_key} has no seeded field ${JSON.stringify(name)}`,
                    { scope_id: store.scope_id, type_key: store.type_key, field: name },
                );
            }
            return field;
        },
    };
}

function dispose_shared_state_stores() {
    const stores = PAGE_SHARED_STORES;
    PAGE_SHARED_STORES = new Map();
    for (const store of stores.values()) {
        store.active = false;
        for (const field of store.fields.values()) {
            field._dispose_shared_state();
        }
        store.fields.clear();
    }
}

/** Reset page-owned shared cells during an explicit client-side page teardown. */
export function dispose_page_shared_state() {
    dispose_shared_state_stores();
    PAGE_SHARED_TABLE_PARSED = false;
    PAGE_SHARED_TABLE_ANCHOR = null;
    PAGE_SHARED_TABLE_SOURCE = null;
    ISLAND_STATE_CACHE = new WeakMap();
}

export function signal_initial(initials, index, fallback) {
    if (
        Array.isArray(initials) &&
        Object.prototype.hasOwnProperty.call(initials, index)
    ) {
        return initials[index];
    }
    return fallback;
}

function decode_typed_signal_seed(value, ty, path) {
    if (!ty || typeof ty !== 'object') {
        throw new TypeError(`rsjs typed hydration schema at ${path} is invalid`);
    }
    switch (ty.kind) {
        case 'i32':
        case 'u32': {
            const integer = integer_bigint(value, ty.kind);
            return Number(integer === 0n ? 0n : integer);
        }
        case 'i64':
        case 'u64': {
            if (
                typeof value !== 'string'
                || !/^(?:0|-?[1-9][0-9]*)$/.test(value)
            ) {
                throw new TypeError(
                    `rsjs ${ty.kind} hydration seed at ${path} must be a canonical decimal JSON string`,
                );
            }
            const integer = BigInt(value);
            const [min, max] = integer_domain(ty.kind);
            if (integer < min || integer > max) {
                throw new RangeError(
                    `rsjs checked ${ty.kind} value ${integer} at ${path} is outside ${min}..=${max}`,
                );
            }
            return integer;
        }
        case 'option':
            return value === null
                ? null
                : decode_typed_signal_seed(value, ty.inner, `${path}.some`);
        case 'vec':
            if (!Array.isArray(value)) {
                throw new TypeError(`rsjs Vec hydration seed at ${path} must be a JSON array`);
            }
            return value.map((entry, index) =>
                decode_typed_signal_seed(entry, ty.inner, `${path}[${index}]`));
        case 'object': {
            if (value === null || typeof value !== 'object' || Array.isArray(value)) {
                throw new TypeError(`rsjs object hydration seed at ${path} must be a JSON object`);
            }
            const decoded = { ...value };
            for (const field of ty.fields ?? []) {
                if (Object.prototype.hasOwnProperty.call(value, field.name)) {
                    decoded[field.name] = decode_typed_signal_seed(
                        value[field.name],
                        field.ty,
                        `${path}.${field.name}`,
                    );
                }
            }
            return decoded;
        }
        default:
            // Non-integer leaves keep their JSON representation. Nominal and
            // unknown shapes cannot contain a recursively provable integer
            // domain and therefore never select this decoder on their own.
            return value;
    }
}

/**
 * Decode a compound, statically typed signal seed before it enters reactive
 * state. Option/Vec/object containers preserve their shape while every nested
 * i32/u32/i64/u64 leaf is validated in its Rust domain; wide leaves become
 * BigInt from the codec's canonical decimal JSON string.
 */
export function signal_initial_typed(initials, index, fallback, ty) {
    const has_seed =
        Array.isArray(initials)
        && Object.prototype.hasOwnProperty.call(initials, index);
    if (!has_seed) {
        return fallback;
    }
    return decode_typed_signal_seed(initials[index], ty, `signals[${index}]`);
}

/**
 * Legacy/boundary-codec hydration for wide Rust integers. ClientEncode may
 * transport i64, u64, i128, u128, isize, and usize as decimal JSON strings,
 * because JSON numbers round past 2^53. Fixed-width client arithmetic is
 * intentionally narrower: only i32, u32, i64, and u64 are supported, and new
 * typed signals use `signal_initial_integer` plus `integer_signal` to validate
 * their exact domain. This helper remains for transport-only and older
 * descriptor paths.
 *
 * Number seeds are accepted defensively (a hand-written blob may use
 * a bare number); non-decodable seeds fall back to the compiled
 * BigInt literal rather than poisoning the signal with a mixed type.
 */
/**
 * Widen a Number-, boolean-, or string-shaped operand to BigInt so it can meet
 * a BigInt operand under an arithmetic operator.
 *
 * A fractional Number has no BigInt image, so it throws a named rsjs error
 * rather than silently truncating. That is the one runtime failure this design
 * accepts in exchange for not requiring the compiler to know a Rust width.
 */
function widen(v) {
    if (typeof v === 'bigint') return v;
    if (typeof v === 'number') {
        if (!Number.isInteger(v)) {
            throw shared_state_error(
                'mixed-width-arithmetic',
                `cannot combine a BigInt-backed shared value with the fractional Number ${v}`,
            );
        }
        return BigInt(v);
    }
    if (typeof v === 'boolean') return v ? 1n : 0n;
    if (typeof v === 'string' && v.trim() !== '') {
        try {
            return BigInt(v.trim());
        } catch (_) {
            /* fall through to the error below */
        }
    }
    throw shared_state_error('mixed-width-arithmetic', `cannot widen ${typeof v} to BigInt`);
}

/** `+` over operands whose representation is decided at run time. */
export function num_add(a, b) {
    if (typeof a === 'bigint') return typeof b === 'bigint' ? a + b : a + widen(b);
    if (typeof b === 'bigint') return widen(a) + b;
    // Number + Number, or JS string concatenation, exactly as a bare `+` would.
    return a + b;
}

/** `-` over operands whose representation is decided at run time. */
export function num_sub(a, b) {
    if (typeof a === 'bigint') return typeof b === 'bigint' ? a - b : a - widen(b);
    if (typeof b === 'bigint') return widen(a) - b;
    return a - b;
}

/** `*` over operands whose representation is decided at run time. */
export function num_mul(a, b) {
    if (typeof a === 'bigint') return typeof b === 'bigint' ? a * b : a * widen(b);
    if (typeof b === 'bigint') return widen(a) * b;
    return a * b;
}

/**
 * Rust integer `/`: truncates toward zero for both representations.
 *
 * `Math.trunc` throws on a BigInt argument, which is exactly why the compiled
 * `Math.trunc(a / b)` form cannot be used when either operand might be one.
 * BigInt `/` already truncates toward zero natively (`-7n / 2n === -3n`).
 */
export function num_div_int(a, b) {
    if (typeof a === 'bigint') return typeof b === 'bigint' ? a / b : a / widen(b);
    if (typeof b === 'bigint') return widen(a) / b;
    return Math.trunc(a / b);
}

/** Rust float `/`: never truncates. */
export function num_div(a, b) {
    if (typeof a === 'bigint' || typeof b === 'bigint') return Number(a) / Number(b);
    return a / b;
}

/** `%` over operands whose representation is decided at run time. */
export function num_rem(a, b) {
    if (typeof a === 'bigint') return typeof b === 'bigint' ? a % b : a % widen(b);
    if (typeof b === 'bigint') return widen(a) % b;
    return a % b;
}

/**
 * `==` over operands whose representation is decided at run time.
 *
 * A bare `===` is wrong here: `5n === 5` is `false` while the Rust `5 == 5`
 * this lowered from is `true`. Loose `==` gets the BigInt/Number cross-compare
 * right, and it is used *only* for that pair, so `null == undefined` and the
 * other loose-equality surprises stay unreachable.
 */
export function num_eq(a, b) {
    const ta = typeof a;
    const tb = typeof b;
    if (ta === tb) return a === b;
    if ((ta === 'bigint' && tb === 'number') || (ta === 'number' && tb === 'bigint')) {
        // eslint-disable-next-line eqeqeq
        return a == b;
    }
    return false;
}

/** `!=` over operands whose representation is decided at run time. */
export function num_ne(a, b) {
    return !num_eq(a, b);
}

export function signal_initial_wide(initials, index, fallback) {
    const value = signal_initial(initials, index, fallback);
    if (typeof value === 'bigint' || value === null) {
        return value;
    }
    if (
        typeof value === 'string' ||
        (typeof value === 'number' && Number.isSafeInteger(value))
    ) {
        try {
            return BigInt(value);
        } catch (err) {
            console.warn('rsjs: wide-int signal seed is not an integer; using fallback', err);
            return fallback;
        }
    }
    console.warn('rsjs: wide-int signal seed has unexpected type; using fallback', typeof value);
    return fallback;
}

/**
 * Decode and validate one fixed-width integer seed in its proven Rust domain.
 * Unlike the legacy representation-only wide decoder, this rejects malformed
 * and out-of-domain values instead of letting them enter typed state.
 */
export function signal_initial_integer(initials, index, fallback, domain) {
    const [, , wide] = integer_domain(domain);
    const has_seed =
        Array.isArray(initials) &&
        Object.prototype.hasOwnProperty.call(initials, index);
    const value = has_seed ? initials[index] : fallback;
    let integer;
    if (has_seed && wide) {
        if (typeof value !== 'string' || !/^(?:0|-?[1-9][0-9]*)$/.test(value)) {
            throw new TypeError(
                `rsjs ${domain} hydration seed must be a decimal JSON string`,
            );
        }
        integer = BigInt(value);
        const [min, max] = integer_domain(domain);
        if (integer < min || integer > max) {
            throw new RangeError(
                `rsjs checked ${domain} value ${integer} is outside ${min}..=${max}`,
            );
        }
    } else {
        // Narrow JSON seeds and compile-time fallbacks already have their
        // runtime representation. Reuse the strict operation validator so a
        // malformed blob cannot smuggle a cross-width value into typed state.
        integer = integer_bigint(value, domain);
    }
    return wide ? integer : Number(integer === 0n ? 0n : integer);
}

function signal_state_script_for(root) {
    if (!root || typeof root.getAttribute !== 'function') {
        return null;
    }
    // The SSR emitter tags the seed blob with the island asset id and
    // places it as the wrapper's immediate next sibling. Match the seed
    // name against the wrapper's `id` (legacy emitters) or its
    // `data-rsjs-island-root` (the attribute the macro actually writes).
    // Adjacency plus the exact-name check keeps pairing as strict as the
    // id-only form; without the island-root fallback, every island whose
    // signal initials are runtime values (no compile-time literal
    // fallback in the island JS) hydrates from an empty seed.
    const names = [
        root.getAttribute('id'),
        root.getAttribute('data-rsjs-island-root'),
    ].filter(Boolean);
    if (names.length === 0) {
        return null;
    }
    const next = root.nextElementSibling;
    if (
        next &&
        next.tagName === 'SCRIPT' &&
        next.getAttribute('type') === 'application/json' &&
        names.includes(next.getAttribute('data-rsjs-island'))
    ) {
        return next;
    }
    return null;
}

/**
 * Run `fn` and capture every signal it reads. Returns an effect handle whose
 * .run() method re-executes `fn`. Used by bind_text when no dep list is
 * supplied (so we discover the deps by tracking the first execution).
 */
function tracked_effect(fn) {
    // We intentionally do not unsubscribe from previous deps before
    // re-running: signals are pinned to the island's lifetime in v0.1, and
    // re-reading the same dep just re-adds the effect to its subscriber
    // set (a Set, so duplicates are free). When the runtime grows
    // memo() / cleanup() in M4+ this needs revisiting.
    const effect = {
        active: true,
        deps: new Set(),
        run() {
            if (effect.active === false) {
                return;
            }
            const previous = CURRENT_EFFECT;
            CURRENT_EFFECT = effect;
            try {
                fn();
            } finally {
                CURRENT_EFFECT = previous;
            }
        },
    };
    try {
        effect.run();
    } catch (error) {
        // A binding that fails during its tracked first run has not returned a
        // disposer to the owner yet. Unsubscribe the partially discovered
        // dependency set here so owner-init rollback cannot leave a live
        // effect behind.
        dispose_effect(effect);
        throw error;
    }
    return effect;
}

function dispose_effect(effect) {
    if (effect == null) {
        return;
    }
    effect.active = false;
    pending_effects.delete(effect);
    for (const dep of Array.from(effect.deps ?? [])) {
        if (typeof dep?._unsubscribe === 'function') {
            dep._unsubscribe(effect);
        } else if (typeof dep?._subscribe === 'function') {
            // Older signal cells exposed only `_subscribe`. The cleanup
            // path below removes the back-reference even if the dep cannot
            // delete the subscriber from its private set.
            effect.deps.delete(dep);
        }
    }
}

function collect_reactive_disposers(effect, deps, extra_handles, apply) {
    const disposers = [];
    try {
        if (Array.isArray(deps)) {
            for (const dep of deps) {
                if (dep && typeof dep._subscribe === 'function') {
                    const unsubscribe = dep._subscribe(effect);
                    if (typeof unsubscribe === 'function') {
                        disposers.push(unsubscribe);
                    }
                }
            }
        }
        if (Array.isArray(extra_handles)) {
            for (const handle of extra_handles) {
                if (handle && typeof handle.subscribe === 'function') {
                    const unsubscribe = handle.subscribe(apply);
                    if (typeof unsubscribe === 'function') {
                        disposers.push(unsubscribe);
                    }
                }
            }
        }
    } catch (error) {
        effect.active = false;
        pending_effects.delete(effect);
        dispose_all(disposers, 'binding subscription rollback');
        effect.deps.clear();
        throw error;
    }
    return once_disposer(() => {
        effect.active = false;
        pending_effects.delete(effect);
        dispose_all(disposers, 'binding subscription');
        effect.deps.clear();
    });
}

// ---------------------------------------------------------------------------
// DOM bindings
// ---------------------------------------------------------------------------

/**
 * Keep `node.textContent` in sync with `getter()`.
 *
 * If `deps` is supplied, the runtime registers explicit subscriptions
 * without an initial tracked run. This is the path the emit_js output
 * uses; the dep list is computed at compile time by analysis::dep_collector.
 *
 * Round-3 review #4: bindings that read `query.data()` /
 * `mutation.is_pending()` (and other accessors that walk a query or
 * mutation handle's state slot) need the runtime to subscribe to
 * those handles too, otherwise the binding applies once at hydration
 * and never updates when the query refetches or the mutation runs.
 *
 *   - `deps` (optional): array of signal slots; subscribe via
 *     `dep._subscribe(effect)`.
 *   - `extra_handles` (optional): array of query / mutation handles,
 *     each exposing a `.subscribe(cb)` method that takes a plain
 *     callback (returns the unsubscribe). The emitter passes
 *     `__rsjs_query_<id>` / `__rsjs_mutation_<id>` references here.
 *   - `text_kind` (optional): `"rust-float"` when the compiler proves
 *     the getter is an f32/f64. This preserves Rust's `Display`
 *     spelling for positive and negative infinity (`inf` / `-inf`)
 *     instead of JavaScript's `Infinity` / `-Infinity`.
 *
 * If both `deps` and `extra_handles` are omitted (or not arrays),
 * the runtime falls back to a tracked first execution that discovers
 * the deps dynamically.
 */
export function bind_text(node, getter, deps, extra_handles, text_kind) {
    if (node == null) {
        // Permissive: a missing marker (e.g. during the M2 smoke test where
        // the developer hand-wrote the HTML and forgot a data-rsjs-bind
        // attribute) should warn loudly but not throw. Throwing here would
        // make the rest of the island fail to wire up.
        console.warn('rsjs: bind_text called with null node; binding skipped');
        return noop_disposer;
    }
    const apply = () => {
        const value = getter();
        node.textContent = text_kind === 'rust-float'
            ? rust_float_text(value)
            : value == null ? '' : String(value);
    };
    const has_signals = Array.isArray(deps);
    const has_handles = Array.isArray(extra_handles) && extra_handles.length > 0;
    if (has_signals || has_handles) {
        const effect = { active: true, deps: new Set(), run: apply };
        const dispose = collect_reactive_disposers(effect, deps, extra_handles, apply);
        try {
            apply();
        } catch (error) {
            dispose();
            throw error;
        }
        return dispose;
    } else {
        const effect = tracked_effect(apply);
        return once_disposer(() => dispose_effect(effect));
    }
}

/**
 * `addEventListener` wrapper. Exists so the emit_js vocabulary stays
 * uniform (every binding goes through a `bind_*` call) and so the runtime
 * has a single place to add cross-cutting concerns later (telemetry,
 * passive-listener defaults, etc.).
 *
 * As of OPTIMIZATIONS.md item #2, the JS emitter no longer calls this
 * helper for islands with non-submit event bindings; it routes those
 * through `bind_delegated_events` instead, which collapses N
 * per-handler `addEventListener` calls into one per event-name. The
 * export is preserved for backward compatibility with hand-written
 * runtime callers and for `bind_submit`-adjacent emit paths.
 */
export function bind_event(node, name, handler) {
    if (node == null) {
        console.warn(`rsjs: bind_event(${JSON.stringify(name)}) called with null node; binding skipped`);
        return noop_disposer;
    }
    const listener = (event) => with_rsjs_current_target(event, node, handler);
    node.addEventListener(name, listener);
    return once_disposer(() => node.removeEventListener(name, listener));
}

/**
 * Document-level event registration for island-local handlers that need
 * to observe clicks, pointer events, or keys outside their own subtree.
 *
 * `owner` is the source element from the rsjs template. The listener attaches
 * to `owner.ownerDocument`, exposes that `Document` as the RSJS current target,
 * and retains `owner` separately for compiler-owned per-node payload lookup.
 */
export function bind_document_event(owner, name, handler) {
    if (owner == null) {
        console.warn(`rsjs: bind_document_event(${JSON.stringify(name)}) called with null owner; binding skipped`);
        return noop_disposer;
    }
    const doc = owner.ownerDocument ?? document;
    const listener = (event) => with_rsjs_current_target(event, doc, handler, owner);
    doc.addEventListener(name, listener);
    return once_disposer(() => doc.removeEventListener(name, listener));
}

/**
 * Bind a window-level event for one island source element.
 *
 * This covers APIs whose events fire on `window` rather than bubbling
 * through the island root: `popstate`, `hashchange`, `storage`, and
 * viewport `resize`, and page `scroll`. The handler sees that `Window` as the
 * RSJS current target while compiler-owned payloads still resolve from `owner`.
 */
export function bind_window_event(owner, name, handler) {
    if (owner == null) {
        console.warn(`rsjs: bind_window_event(${JSON.stringify(name)}) called with null owner; binding skipped`);
        return noop_disposer;
    }
    const win = owner.ownerDocument?.defaultView ?? window;
    const listener = (event) => with_rsjs_current_target(event, win, handler, owner);
    win.addEventListener(name, listener);
    return once_disposer(() => win.removeEventListener(name, listener));
}

function with_rsjs_current_target(event, target, handler, owner = target) {
    const prev_target = event.__rsjsCurrentTarget;
    const prev_owner = event.__rsjsOwnerTarget;
    event.__rsjsCurrentTarget = target;
    event.__rsjsOwnerTarget = owner;
    try {
        handler(event);
    } finally {
        if (prev_target === undefined) {
            delete event.__rsjsCurrentTarget;
        } else {
            event.__rsjsCurrentTarget = prev_target;
        }
        if (prev_owner === undefined) {
            delete event.__rsjsOwnerTarget;
        } else {
            event.__rsjsOwnerTarget = prev_owner;
        }
    }
}

/**
 * Delegated event-listener registration. One listener attaches to the
 * island root and dispatches to the right handler based on the
 * `data-rsjs-handler` marker attribute carried by the SSR-rendered
 * element nearest to the click target.
 *
 * `handlers` is a `Map<string, (event: Event) => void>` keyed by the
 * stringified `NodeId` (matching the `data-rsjs-handler` attribute
 * value the server emits).
 *
 * Why delegation: an island with N handlers used to register N
 * listeners during hydration, plus N `walk_path` lookups to find the
 * target nodes. Delegation collapses that to one listener and zero
 * lookups per event-name, regardless of N. For a page with 60 islands
 * carrying ~5 click handlers each, that drops 300 listener
 * registrations to 60 (one per island root), and removes 300
 * `walk_path` calls from the hydrate critical path.
 *
 * Bubbling note: we use `closest('[data-rsjs-handler]')` so that a
 * click on a nested element inside a handler-bearing element (e.g. an
 * `<svg>` icon inside a `<button on_click=...>`) still resolves to
 * the parent handler. The containment and owner checks ensure we
 * never fire for a marker outside this owner, including a
 * same-numbered marker in a nested component fragment or explicit
 * island.
 */
export function bind_delegated_events(root, eventName, handlers) {
    if (root == null) {
        console.warn(
            `rsjs: bind_delegated_events(${JSON.stringify(eventName)}) called with null root; binding skipped`,
        );
        return noop_disposer;
    }
    const options = delegated_event_options(eventName);
    const listener = (event) => {
        let target = event.target;
        // `event.target` may be a Text node or other non-Element; walk
        // up to the nearest Element before calling `closest`.
        if (target != null && target.nodeType !== 1) {
            target = target.parentElement;
        }
        if (target == null || typeof target.closest !== 'function') return;
        const match = target.closest('[data-rsjs-handler]');
        if (match == null || !root.contains(match)) return;
        if (marker_owner(match) !== marker_owner(root)) return;
        const handler = handlers.get(match.getAttribute('data-rsjs-handler'));
        if (handler != null) with_rsjs_current_target(event, match, handler);
    };
    root.addEventListener(eventName, listener, options);
    return once_disposer(() => root.removeEventListener(eventName, listener, options));
}

function delegated_event_options(eventName) {
    // Some high-value UI events do not bubble, so delegated root
    // listeners must observe them in the capture phase.
    return eventName === 'focus' || eventName === 'blur' || eventName === 'scroll'
        || eventName === 'pointerenter' || eventName === 'pointerleave'
        ? { capture: true }
        : false;
}

/**
 * Two-way binding between a form-control DOM property and a signal.
 *
 *   - `property` is the DOM property name to read/write: `"value"` for
 *     text inputs, textareas, and `<select>`s; `"checked"` for
 *     checkboxes / radios.
 *   - User edits flow signal-ward via `input` and `change` events.
 *     Both fire so we cover the full keyboard / paste / programmatic
 *     surface. The runtime guards against re-entrant writes (a signal
 *     update we just emitted echoing back through the input listener)
 *     by comparing the would-be next value against the signal's current
 *     value before calling `.set()`.
 *   - Signal updates flow back via a tracked subscription that mirrors
 *     `node[property] = signal.get()` whenever the signal changes.
 *
 * Mirrors `bind_text` in that the DOM is the side-effect target and
 * the signal is the source of truth, but adds the user-event listener.
 * `preserve_dirty` is true for an owner's first mount so edits made before
 * hydration win. Lifecycle remounts pass false: the retained SSR control may
 * still look dirty relative to its immutable default, but child-owned state
 * from the new mount must not be overwritten by that stale DOM value.
 */
export function bind_input(
    node,
    property,
    cell,
    ssr_baseline,
    preserve_dirty = true,
) {
    if (node == null) {
        console.warn('rsjs: bind_input called with null node; binding skipped');
        return noop_disposer;
    }
    // Round-3 review #8: the parameter is renamed from `signal` to
    // `cell` so it does not shadow the module-level `signal` export.
    // Pull the user's edit into the cell. `checked` is a boolean, every other
    // property we surface is a string.
    //
    // For an island-local cell the raw string is stored as-is: the IR layer is
    // responsible for any type translation when an integer-typed signal binds
    // to a text input (M7+). A *page-shared* field cell is different: it is
    // representation-pinned by `shared_field_cell`, so its `set` coerces the
    // DOM string into the seed's representation. Without that, one number
    // input would replace a BigInt-backed field with the string `"5"` for
    // every island on the page bound to the same provider scope.
    const read_from_dom = () =>
        property === 'checked' || property === 'selected'
            ? !!node[property]
            : node[property];
    const on_user_edit = () => {
        const next = read_from_dom();
        if (!Object.is(cell.peek(), next)) {
            cell.set(next);
        }
    };

    // Users can edit a control before a deferred owner activates. Compare the
    // live property to the browser's immutable SSR/default baseline before
    // installing signal-to-DOM effects. Dirty DOM wins; untouched DOM takes
    // the current cell value (which may already have changed through
    // SharedState in another owner).
    if (
        preserve_dirty &&
        form_control_is_dirty(node, property, ssr_baseline)
    ) {
        const next = read_from_dom();
        if (!Object.is(cell.peek(), next)) {
            cell.set(next);
        }
    }

    // Mirror signal -> DOM via an effect that re-applies the property
    // every time the cell changes. We pre-seed `node[property]` to
    // the current cell value so the rendered HTML and the signal's
    // initial value agree on first run, even when the SSR-time output
    // forgot to write the attribute.
    const apply = () => {
        const value = cell.get();
        if (property === 'checked' || property === 'selected') {
            // Boolean properties: still skip the write when the DOM
            // already matches. Even though there's no cursor state to
            // preserve, an unconditional write can cause a brief
            // visual flicker when the SSR-rendered initial value
            // disagrees with the signal's initial value (browsers may
            // re-paint the control on every property assignment).
            const next = !!value;
            if (node[property] !== next) {
                node[property] = next;
            }
        } else {
            // A representation-pinned cell rejects or normalizes what the user
            // typed, so the cell's value and the DOM text can legitimately
            // differ while still *denoting* the same value ("007" -> 7n). Only
            // mirror back when they denote different values; otherwise every
            // keystroke would rewrite the field mid-typing and move the caret.
            if (cell.__rsjs_repr !== undefined) {
                const from_dom = coerce_to_repr(node[property], cell.__rsjs_repr);
                if (from_dom !== REPR_REJECT && Object.is(from_dom, value)) {
                    return;
                }
            }
            const next = value == null ? '' : String(value);
            // Skip the write when the DOM already matches. This
            // preserves user state across signal-triggered re-renders:
            //   - <input>:    text-input cursor / IME composition
            //   - <textarea>: caret position + native scroll offset
            //   - <select>:   open-listbox selection focus
            // All three use the `value` property, so the same
            // identity check covers the whole form-control surface.
            //
            // Contenteditable elements (property === 'textContent')
            // also flow through this branch. They don't have a
            // stable cursor model the runtime can preserve through a
            // textContent reassignment (the browser collapses any
            // selection on every overwrite), so we still skip the
            // no-op write but the user-visible benefit is just
            // avoiding redundant DOM thrash, not cursor preservation.
            if (node[property] !== next) {
                node[property] = next;
            }
        }
    };
    const effect = { active: true, deps: new Set(), run: apply };
    let unsubscribe = null;
    const dispose = once_disposer(() => {
        effect.active = false;
        pending_effects.delete(effect);
        node.removeEventListener('input', on_user_edit);
        node.removeEventListener('change', on_user_edit);
        if (typeof unsubscribe === 'function') {
            unsubscribe();
        } else if (typeof cell._unsubscribe === 'function') {
            // A custom cell may have registered the effect and then thrown
            // before returning its disposer. Remove that partial subscription
            // through the signal-compatible fallback when one is available.
            cell._unsubscribe(effect);
        }
        effect.deps.clear();
    });
    try {
        node.addEventListener('input', on_user_edit);
        node.addEventListener('change', on_user_edit);
        unsubscribe = cell._subscribe(effect);
        if (ACTIVE_OWNER_INITIALIZATION !== null) {
            ACTIVE_OWNER_INITIALIZATION.form_commits.push(apply);
        } else {
            apply();
        }
    } catch (error) {
        dispose();
        throw error;
    }
    return dispose;
}

function form_control_is_dirty(node, property, ssr_baseline) {
    if (property === 'checked' && 'defaultChecked' in node) {
        return !!node.checked !== !!node.defaultChecked;
    }
    if (property === 'selected' && 'defaultSelected' in node) {
        return !!node.selected !== !!node.defaultSelected;
    }
    if (property === 'value') {
        const options = node.options;
        if (options != null && typeof options.length === 'number') {
            for (let index = 0; index < options.length; index += 1) {
                const option = options[index];
                if (
                    option != null &&
                    'selected' in option &&
                    'defaultSelected' in option &&
                    !!option.selected !== !!option.defaultSelected
                ) {
                    return true;
                }
            }
            return false;
        }
        if ('defaultValue' in node) {
            return String(node.value ?? '') !== String(node.defaultValue ?? '');
        }
        return false;
    }
    if (property === 'textContent') {
        let baseline = ssr_baseline;
        if (
            baseline === undefined &&
            node != null &&
            typeof node.getAttribute === 'function'
        ) {
            baseline = node.getAttribute('data-rsjs-ssr-text');
        }
        return baseline !== undefined && baseline !== null
            ? String(node.textContent ?? '') !== String(baseline)
            : false;
    }
    return false;
}

/**
 * Keep an HTML attribute in sync with a reactive expression.
 *
 * `kind === "boolean"` writes by presence/absence (`disabled`,
 * `hidden`, `required`, ...). Other attributes are string-valued:
 * `null` / `undefined` remove the attribute, all other values are
 * stringified. This deliberately treats `false` on ARIA/data attrs as
 * the string `"false"` rather than absence, which is the accessible
 * shape expected by attrs like `aria-expanded`.
 */
export function bind_attr(node, name, kind, getter, deps, extra_handles) {
    if (node == null) {
        console.warn(
            `rsjs: bind_attr(${JSON.stringify(name)}) called with null node; binding skipped`
        );
        return noop_disposer;
    }
    const apply = () => {
        const value = getter();
        if (kind === 'boolean') {
            const on = !!value;
            if (on) {
                node.setAttribute(name, '');
            } else {
                node.removeAttribute(name);
            }
            if (name in node) {
                try {
                    node[name] = on;
                } catch (_) {
                    // Some reflected properties are read-only on some elements.
                }
            }
            return;
        }
        if (value == null) {
            node.removeAttribute(name);
        } else {
            // Defense-in-depth: never install an event-handler attribute
            // (`onclick`, `onerror`, `onmouseover`, ...) via setAttribute.
            // Doing so executes the string value as JavaScript with no
            // quoting/escaping, an injection sink. The SSR-side `#[rsjs]`
            // macro already rejects dynamic `on*` bindings, so reaching this
            // branch means a codegen path regressed. Refuse and warn rather
            // than write attacker-controlled script. Attribute removal above
            // stays allowed (clearing a handler is safe).
            if (/^on/i.test(name)) {
                if (is_dev_mode()) {
                    console.warn(
                        `rsjs: refusing to set event-handler attribute ${JSON.stringify(
                            name
                        )} via setAttribute (would execute as JavaScript). ` +
                            `Dynamic on* handlers are not supported; use a bound event listener instead.`
                    );
                }
                return;
            }
            const next = String(value);
            node.setAttribute(name, kind === 'url' && has_dangerous_url_scheme(next) ? '' : next);
        }
        if (name === 'value' && 'value' in node) {
            try {
                node.value = value == null ? '' : String(value);
            } catch (_) {
                // Some element implementations expose read-only value properties.
            }
        }
    };
    const has_signals = Array.isArray(deps);
    const has_handles = Array.isArray(extra_handles) && extra_handles.length > 0;
    if (has_signals || has_handles) {
        const effect = { active: true, deps: new Set(), run: apply };
        const dispose = collect_reactive_disposers(effect, deps, extra_handles, apply);
        try {
            apply();
        } catch (error) {
            dispose();
            throw error;
        }
        return dispose;
    } else {
        const effect = tracked_effect(apply);
        return once_disposer(() => dispose_effect(effect));
    }
}

function has_dangerous_url_scheme(url) {
    const normalized = normalize_url_scheme_prefix(String(url ?? ''));
    if (normalized.startsWith('javascript:') || normalized.startsWith('vbscript:')) {
        return true;
    }
    if (!normalized.startsWith('data:')) {
        return false;
    }
    const rest = normalized.slice('data:'.length);
    return (
        rest.startsWith('text/html') ||
        rest.startsWith('application/xhtml+xml') ||
        rest.startsWith('application/xml') ||
        rest.startsWith('image/svg+xml')
    );
}

function normalize_url_scheme_prefix(input) {
    let out = '';
    for (let i = 0; i < input.length && out.length < 96; ) {
        const decoded = decode_numeric_entity_at(input, i);
        let code;
        if (decoded) {
            code = decoded.code;
            i = decoded.next;
        } else {
            code = input.charCodeAt(i);
            i += 1;
        }
        if (code <= 0x20) continue;
        if (code > 0x7f) {
            out += '\ufffd';
        } else {
            out += String.fromCharCode(code).toLowerCase();
        }
        if (out.includes(':') && !out.startsWith('data:')) {
            break;
        }
    }
    return out;
}

function decode_numeric_entity_at(input, index) {
    if (input.charCodeAt(index) !== 0x26 || input.charCodeAt(index + 1) !== 0x23) {
        return null;
    }
    let i = index + 2;
    let radix = 10;
    if (input.charCodeAt(i) === 0x78 || input.charCodeAt(i) === 0x58) {
        radix = 16;
        i += 1;
    }
    const start = i;
    while (i < input.length) {
        const code = input.charCodeAt(i);
        const is_digit =
            radix === 16
                ? (code >= 0x30 && code <= 0x39) ||
                  (code >= 0x61 && code <= 0x66) ||
                  (code >= 0x41 && code <= 0x46)
                : code >= 0x30 && code <= 0x39;
        if (!is_digit) break;
        i += 1;
    }
    if (i === start) {
        return null;
    }
    const raw = input.slice(start, i);
    if (input.charCodeAt(i) === 0x3b) {
        i += 1;
    }
    const parsed = Number.parseInt(raw, radix);
    if (!Number.isFinite(parsed) || parsed < 0) {
        return { code: 0xfffd, next: i };
    }
    return { code: parsed === 0 ? 0xfffd : parsed, next: i };
}

/**
 * Form-submit binding. Calls `event.preventDefault()` before
 * dispatching to the handler so the browser does not navigate; the
 * handler can then run signal updates synchronously and (later) kick
 * off async work.
 *
 * Symmetrical with `bind_event`: the only practical difference is the
 * `preventDefault` call, but giving submit its own runtime entry-point
 * keeps the emit_js output self-documenting (a `bind_submit(...)` line
 * in the generated JS unambiguously means "form submit; we suppress
 * the default action").
 */
export function bind_submit(node, handler) {
    if (node == null) {
        console.warn('rsjs: bind_submit called with null node; binding skipped');
        return noop_disposer;
    }
    const listener = (event) => {
        event.preventDefault();
        with_rsjs_current_target(event, node, handler);
    };
    node.addEventListener('submit', listener);
    return once_disposer(() => node.removeEventListener('submit', listener));
}

/**
 * Toggle visibility of two sibling lists of nodes based on a boolean
 * predicate.
 *
 * `then_nodes` is shown and `else_nodes` is hidden when `getter()` is
 * truthy; the pair flips when it is falsy. Nodes are not removed from
 * the DOM; we set `style.display = 'none'` (or restore the previous
 * inline value) so subscriptions inside the hidden subtree stay live.
 *
 * `deps` is the explicit dep array the JS emitter computed via
 * dep_collector on the predicate ExprId. Symmetric with bind_text.
 * An initial getter failure throws and rolls back registration. A later
 * failure is reported and leaves the last visibility state in place so this
 * subscriber cannot starve a linked component-lifecycle subscriber to the
 * same predicate.
 *
 * Null nodes are tolerated: a missing marker (e.g. the user wrote a
 * branch element but the M3+ emitter forgot a marker for it) logs a
 * warning and is skipped, the rest of the binding still wires up.
 */
export function bind_conditional(getter, deps, then_nodes, else_nodes, extra_handles) {
    const live_then = (then_nodes || []).filter((n) => n != null);
    const live_else = (else_nodes || []).filter((n) => n != null);
    if (live_then.length === 0 && live_else.length === 0) {
        console.warn('rsjs: bind_conditional called with no resolved nodes; binding skipped');
        return noop_disposer;
    }
    // Capture the original `display` so we can restore it on show. We
    // read it once at registration; if it's the empty string (the
    // default), restoring '' lets the cascaded stylesheet rules apply.
    //
    // SSR pre-hides the inactive branch with inline `display:none` so
    // the page doesn't flicker before hydration. We must NOT treat
    // that pre-applied 'none' as the natural display value — if we did,
    // restoring it on show would re-hide the branch and toggling would
    // be a one-way trip. Map 'none' back to '' so the cascaded default
    // applies when the branch is shown.
    const original_display = new Map();
    for (const n of [...live_then, ...live_else]) {
        const cur = n.style.display || '';
        original_display.set(n, cur === 'none' ? '' : cur);
    }
    const apply = () => {
        const truthy = !!getter();
        const show = truthy ? live_then : live_else;
        const hide = truthy ? live_else : live_then;
        for (const n of show) {
            n.style.display = original_display.get(n);
        }
        for (const n of hide) {
            n.style.display = 'none';
        }
    };
    // Round-3 review #4: same query/mutation-handle plumbing as
    // bind_text. Conditional branches whose predicate observes
    // `query.data() != null` or `mutation.is_pending()` need to
    // toggle visibility when those state slots change.
    const has_signals = Array.isArray(deps);
    const has_handles = Array.isArray(extra_handles) && extra_handles.length > 0;
    let initializing = true;
    const apply_transition = () => {
        try {
            apply();
        } catch (error) {
            if (initializing) {
                throw error;
            }
            // Conditional visibility and linked component lifetime may
            // subscribe to the same predicate. Contain later visibility
            // failures so one effect cannot abort the scheduler before the
            // lifecycle effect gets a chance to fail closed.
            report_hydration_error('conditional binding transition', error);
        }
    };
    if (has_signals || has_handles) {
        const effect = {
            active: true,
            deps: new Set(),
            run: apply_transition,
        };
        const dispose = collect_reactive_disposers(
            effect,
            deps,
            extra_handles,
            apply_transition,
        );
        try {
            apply_transition();
            initializing = false;
        } catch (error) {
            initializing = false;
            dispose();
            throw error;
        }
        return dispose;
    } else {
        let effect;
        try {
            effect = tracked_effect(apply_transition);
            initializing = false;
        } catch (error) {
            initializing = false;
            throw error;
        }
        return once_disposer(() => dispose_effect(effect));
    }
}

/**
 * Mount and dispose one linked component subtree with a parent conditional.
 *
 * `control` is emitted by the parent fragment and carries the exact producer
 * predicate plus its reactive dependencies:
 *
 *     { get: () => boolean, deps: Signal[], handles: Subscribable[] }
 *
 * The first evaluation is part of owner initialization and therefore throws
 * on predicate or mount failure so the owner can unwind atomically. After
 * registration, transition failures are scoped, reported, and contained: the
 * group is left inactive and may retry on a later dependency notification.
 * Disposing this binding unsubscribes first, then tears down the active group.
 * Component DOM is server-owned and deliberately remains in place throughout.
 */
export function bind_component_lifecycle(control, branch, mount, scope) {
    if (control == null || typeof control !== 'object') {
        throw new TypeError(
            'rsjs/component-lifecycle: control must be an object',
        );
    }
    if (typeof control.get !== 'function') {
        throw new TypeError(
            'rsjs/component-lifecycle: control.get must be a function',
        );
    }
    if (!Array.isArray(control.deps)) {
        throw new TypeError(
            'rsjs/component-lifecycle: control.deps must be an array',
        );
    }
    for (const dep of control.deps) {
        if (dep == null || typeof dep._subscribe !== 'function') {
            throw new TypeError(
                'rsjs/component-lifecycle: every control dependency must be a signal',
            );
        }
    }
    if (!Array.isArray(control.handles)) {
        throw new TypeError(
            'rsjs/component-lifecycle: control.handles must be an array',
        );
    }
    for (const handle of control.handles) {
        if (handle == null || typeof handle.subscribe !== 'function') {
            throw new TypeError(
                'rsjs/component-lifecycle: every extra handle must be subscribable',
            );
        }
    }
    if (branch !== 'then' && branch !== 'else') {
        throw new TypeError(
            'rsjs/component-lifecycle: branch must be "then" or "else"',
        );
    }
    if (typeof mount !== 'function') {
        throw new TypeError(
            'rsjs/component-lifecycle: mount must be a function',
        );
    }
    if (typeof scope !== 'string' || scope.length === 0) {
        throw new TypeError(
            'rsjs/component-lifecycle: scope must be a non-empty string',
        );
    }

    let active_disposer = null;
    let disposed = false;
    let initializing = true;
    const lifecycle_deps = control.deps.map((dep) => ({
        _subscribe(effect) {
            const unsubscribe = dep._subscribe(effect);
            if (typeof unsubscribe !== 'function') {
                if (typeof dep._unsubscribe === 'function') {
                    dep._unsubscribe(effect);
                }
                throw new TypeError(
                    'rsjs/component-lifecycle: signal subscription must return a disposer',
                );
            }
            return unsubscribe;
        },
    }));
    const lifecycle_handles = control.handles.map((handle) => ({
        subscribe(listener) {
            const unsubscribe = handle.subscribe(listener);
            if (typeof unsubscribe !== 'function') {
                throw new TypeError(
                    'rsjs/component-lifecycle: handle subscription must return a disposer',
                );
            }
            return unsubscribe;
        },
    }));

    const deactivate = () => {
        const current = active_disposer;
        active_disposer = null;
        if (typeof current !== 'function') {
            return;
        }
        try {
            current();
        } catch (error) {
            report_hydration_error(`${scope} active-group teardown`, error);
        }
    };

    const transition = (strict) => {
        if (disposed) {
            return;
        }
        try {
            const predicate = control.get();
            if (typeof predicate !== 'boolean') {
                throw new TypeError(
                    'rsjs/component-lifecycle: control.get must return a boolean',
                );
            }
            const should_mount =
                branch === 'then' ? predicate : !predicate;
            if (!should_mount) {
                deactivate();
                return;
            }
            if (active_disposer !== null) {
                return;
            }
            const mounted = mount();
            if (typeof mounted !== 'function') {
                throw new TypeError(
                    'rsjs/component-lifecycle: mount must return a disposer',
                );
            }
            active_disposer = once_disposer(mounted);
        } catch (error) {
            deactivate();
            if (strict) {
                throw error;
            }
            report_hydration_error(`${scope} transition`, error);
        }
    };

    const effect = {
        active: true,
        deps: new Set(),
        run() {
            transition(initializing);
        },
    };
    let unsubscribe = noop_disposer;
    try {
        unsubscribe = collect_reactive_disposers(
            effect,
            lifecycle_deps,
            lifecycle_handles,
            () => transition(initializing),
        );
        transition(true);
        initializing = false;
    } catch (error) {
        initializing = false;
        unsubscribe();
        deactivate();
        throw error;
    }

    return once_disposer(() => {
        disposed = true;
        // Stop future transitions before the active subtree releases any
        // callbacks, effects, or handles that could themselves notify.
        unsubscribe();
        deactivate();
    });
}

/**
 * Keyed list rendering over a reactive array. Reconciliation and teardown
 * are covered by rsjs-runtime.list.test.mjs.
 *
 * The binding OWNS the element children of `container`: it inserts,
 * moves, and removes them to mirror `getter()`. Do not mix other
 * static children into the same container.
 *
 * `options`:
 *   - `key(item, index)`   — REQUIRED. Stable string key per item.
 *     Duplicate keys fail the complete reconciliation before any item
 *     state or DOM is changed.
 *   - `create(item, index)` — REQUIRED. Build the DOM for a new item.
 *     May return a bare node, or `{ node, update?, dispose? }` where
 *     `update(item, index)` refreshes the node when the item value
 *     changes identity and `dispose()` tears down item-level bindings
 *     when the item leaves the list.
 *   - `adopt(node, item, index)` — optional. Pair a server-rendered
 *     child carrying `data-rsjs-key` with its initial item instead of
 *     re-creating it. Same return contract as `create`. Children
 *     without a matching key are removed on the first apply.
 *   - `deps` — optional explicit signal array (the compile-time
 *     dep-list path); omitted means a tracked first run discovers
 *     the deps.
 *   - `extra_handles` — optional query / mutation handles whose
 *     `.subscribe(cb)` notifications should re-run reconciliation
 *     when the getter reads non-signal reactive state.
 *
 * Reconciliation is keyed and order-preserving: removals dispose,
 * existing keys update in place when `Object.is(prev, next)` fails,
 * and a single forward pass moves nodes with `insertBefore` only when
 * they are out of position. Disposal of the binding stops updates and
 * runs every item disposer but leaves the current DOM in place, the
 * same contract as the other bind_* primitives.
 *
 * This runtime slice canonicalizes keys with `String(...)`. Consequently the
 * string `"1"` and numeric `1` currently collide. Preserving their distinct
 * typed identities requires the pending compiler/SSR key-tagging protocol.
 */
export function bind_list(container, getter, options) {
    if (container == null) {
        console.warn('rsjs: bind_list called with null container; binding skipped');
        return noop_disposer;
    }
    if (typeof options?.key !== 'function' || typeof options?.create !== 'function') {
        console.warn('rsjs: bind_list requires key() and create() options; binding skipped');
        return noop_disposer;
    }
    const { key: key_of, create, adopt, deps, extra_handles } = options;

    const normalize = (built, item) => {
        if (built == null) {
            return null;
        }
        if (built.node != null) {
            return { node: built.node, update: built.update, dispose: built.dispose, item };
        }
        return { node: built, update: undefined, dispose: undefined, item };
    };

    const duplicate_key_error = (source, key) => {
        const error = new Error(
            `rsjs/duplicate-list-key: bind_list ${source} contains duplicate key ${JSON.stringify(key)}`,
        );
        error.code = 'rsjs/duplicate-list-key';
        error.key = key;
        error.source = source;
        return error;
    };

    // Server-rendered children annotated with data-rsjs-key wait here
    // until the first apply pairs them with their initial items.
    const adoptable = new Map();
    if (typeof adopt === 'function') {
        const markers = [];
        let child = container.firstChild;
        while (child != null) {
            const k = typeof child.getAttribute === 'function'
                ? child.getAttribute('data-rsjs-key')
                : null;
            if (k != null) {
                markers.push([String(k), child]);
            }
            child = child.nextSibling;
        }
        const seen_markers = new Set();
        for (const [key] of markers) {
            if (seen_markers.has(key)) {
                throw duplicate_key_error('SSR marker set', key);
            }
            seen_markers.add(key);
        }
        for (const [key, node] of markers) {
            adoptable.set(key, node);
        }
    }

    let records = [];
    const by_key = new Map();

    const apply = () => {
        const items = getter();
        const list = Array.isArray(items) ? items : [];
        const keyed_items = [];
        const seen = new Set();
        // Canonicalize and validate the complete proposal before looking up,
        // updating, creating, adopting, disposing, or moving any record.
        // This is what makes a later invalid write atomic with respect to the
        // previously committed list.
        for (let i = 0; i < list.length; i += 1) {
            const item = list[i];
            const key = String(key_of(item, i));
            if (seen.has(key)) {
                throw duplicate_key_error('live item sequence', key);
            }
            seen.add(key);
            keyed_items.push({ item, index: i, key });
        }

        const desired = [];

        for (const { item, index, key: k } of keyed_items) {
            let record = by_key.get(k);
            if (record !== undefined) {
                if (!Object.is(record.item, item)) {
                    record.item = item;
                    if (typeof record.update === 'function') {
                        record.update(item, index);
                    }
                }
            } else {
                const ssr_node = adoptable.get(k);
                if (ssr_node !== undefined) {
                    adoptable.delete(k);
                    record = normalize(adopt(ssr_node, item, index), item) ?? normalize(ssr_node, item);
                } else {
                    record = normalize(create(item, index), item);
                }
                if (record == null) {
                    continue;
                }
                by_key.set(k, record);
            }
            record.key = k;
            desired.push(record);
        }

        // Removal pass: anything keyed that is no longer wanted leaves
        // the DOM and runs its disposer.
        for (const record of records) {
            if (!seen.has(record.key)) {
                by_key.delete(record.key);
                // A structural binding owns the complete lifetime of the
                // removed range. Tear down independently tracked owners inside
                // that range synchronously, including deferred activation,
                // before either item cleanup or DOM removal can strand them.
                dispose_hydrations_in_owned_subtree(record.node, true);
                if (typeof record.dispose === 'function') {
                    try {
                        record.dispose();
                    } catch (error) {
                        report_hydration_error('bind_list item dispose', error);
                    }
                }
                if (record.node.parentNode === container) {
                    container.removeChild(record.node);
                }
            }
        }
        // Server-rendered children whose key never matched an item are
        // stale markup; drop them on the first apply.
        for (const node of adoptable.values()) {
            dispose_hydrations_in_owned_subtree(node, true);
            container.removeChild(node);
        }
        adoptable.clear();

        // Placement pass: one forward walk; insertBefore only when a
        // node is not already in position. insertBefore moves nodes
        // that are already connected, so this handles insert and
        // reorder uniformly.
        let prev = null;
        for (const record of desired) {
            const ref = prev === null ? container.firstChild : prev.node.nextSibling;
            if (ref !== record.node) {
                container.insertBefore(record.node, ref ?? null);
            }
            prev = record;
        }
        records = desired;
    };

    const dispose_items = () => {
        for (const record of records) {
            if (typeof record.dispose === 'function') {
                try {
                    record.dispose();
                } catch (error) {
                    report_hydration_error('bind_list item dispose', error);
                }
            }
        }
    };

    const has_signals = Array.isArray(deps);
    const has_handles = Array.isArray(extra_handles) && extra_handles.length > 0;
    if (has_signals || has_handles) {
        const effect = { active: true, deps: new Set(), run: apply };
        const dispose = collect_reactive_disposers(effect, deps, extra_handles, apply);
        try {
            apply();
        } catch (error) {
            // `bind_list` has not returned its disposer to the owner yet.
            // Unsubscribe eagerly so the owner's partial-init rollback cannot
            // leave this failed binding scheduled.
            dispose();
            throw error;
        }
        return once_disposer(() => {
            dispose();
            dispose_items();
        });
    }
    const effect = tracked_effect(apply);
    return once_disposer(() => {
        dispose_effect(effect);
        dispose_items();
    });
}

export function report_hydration_error(scope, error) {
    if (typeof console !== 'undefined' && typeof console.error === 'function') {
        console.error(`rsjs: ${scope} failed`, error);
    }
}

export function dispose_all(disposers, scope = 'disposer') {
    if (!Array.isArray(disposers)) {
        return;
    }
    // Consume the stack as it is unwound. Besides releasing closures and DOM
    // references promptly, this makes a raw generated `dispose_all(resources)`
    // callback idempotent when the hydration runtime also owns the same stack.
    while (disposers.length > 0) {
        const dispose = disposers.pop();
        if (typeof dispose !== 'function') {
            continue;
        }
        try {
            dispose();
        } catch (error) {
            report_hydration_error(`${scope} dispose`, error);
        }
    }
}

/**
 * Seal the synchronous initialization resource stack into one terminal owner
 * disposer.
 *
 * Generated initializers currently return a callback that itself drains the
 * same `resources` array. Hand-written/runtime adapters may instead return an
 * independent disposer. The owner contract must cover both without leaking
 * the runtime stack or double-running a callback that was also pushed into it.
 */
function owner_resource_disposer(name, resources, returned_disposer) {
    const wrapped_by_resource = new Map();
    for (let i = 0; i < resources.length; i += 1) {
        const resource = resources[i];
        if (typeof resource !== 'function') {
            continue;
        }
        let wrapped = wrapped_by_resource.get(resource);
        if (wrapped == null) {
            wrapped = once_disposer(resource);
            wrapped_by_resource.set(resource, wrapped);
        }
        resources[i] = wrapped;
    }

    const returned =
        typeof returned_disposer === 'function'
            ? wrapped_by_resource.get(returned_disposer) ??
              once_disposer(returned_disposer)
            : noop_disposer;

    return once_disposer(() => {
        let returned_error = null;
        try {
            // A returned cleanup is the last resource created by init, so it
            // runs before the earlier stack entries.
            returned();
        } catch (error) {
            returned_error = error;
        } finally {
            dispose_all(resources, `island "${name}" resources`);
        }
        if (returned_error !== null) {
            throw returned_error;
        }
    });
}

// ---------------------------------------------------------------------------
// Hydration entry
// ---------------------------------------------------------------------------

/**
 * Resolve "is this a dev build?" for the no-markers fallback at
 * hydrate time. Resolution order (first match wins):
 *
 *   1. `globalThis.__RSJS_DEV__` — explicit override. Set this to
 *      `true` (or `false`) to force a specific behaviour. Honoured
 *      whenever the property is set (even to `false`); only `null`
 *      / `undefined` falls through to the next step.
 *   2. `process.env.NODE_ENV` — Node / bundler convention. Anything
 *      other than `"production"` is treated as dev.
 *
 * `import.meta.env.DEV` (Vite / modern bundlers) is intentionally
 * not auto-detected: `import.meta` is a syntax error in older
 * CommonJS bundlers, so referring to it here would break those
 * pipelines at parse time. Consumers using such bundlers can wire
 * the flag through manually:
 *
 *     globalThis.__RSJS_DEV__ = import.meta.env.DEV;
 *
 * before importing `rsjs-runtime.js`.
 */
function is_dev_mode() {
    if (typeof globalThis !== 'undefined' && globalThis.__RSJS_DEV__ != null) {
        return globalThis.__RSJS_DEV__ === true;
    }
    if (
        typeof process !== 'undefined' &&
        process.env &&
        process.env.NODE_ENV !== undefined
    ) {
        return process.env.NODE_ENV !== 'production';
    }
    return false;
}

/**
 * Look up the rendered island root and invoke `init(root)`.
 *
 * Lookup strategy:
 *   1. Find every `<rsjs-island data-rsjs-island-root="<name>">` element. The server
 *      emitter writes the asset id onto `data-rsjs-island-root=` (matching the convention
 *      shared with `emit_rust`); the JS string passed to `hydrate` is the
 *      same asset id.
 *   2. If no element is found, log a warning and fall back to
 *      document.body. This keeps the manual smoke test working when the
 *      developer's index.html lacks the wrapper element.
 *
 * Hydration runs synchronously when the DOM is ready. If the runtime
 * is loaded eagerly (e.g. an emitted island JS file imported from a
 * `<script type="module">` in `<head>` while the body is still being
 * parsed), hydrate() defers until DOMContentLoaded so it observes the
 * island's marker element. Belt-and-braces: the recommended pattern
 * is still to load the runtime + island JS at the END of `<body>`, but
 * the deferral means a misplaced `<script>` doesn't silently miss its
 * island.
 */
/**
 * Walk a DOM index path from `root` to a target node and return it.
 *
 * Replaces per-marker `root.querySelector(...)` calls during hydration
 * with an O(depth) child-index walk. The codegen knows the IR's tree
 * shape at build time, so each marker is reachable as a fixed sequence
 * of `.children[i]` accesses. No selector parsing, no attribute scan.
 *
 * The walked node is always checked against its marker. On mismatch, the
 * runtime performs an owner-scoped marker lookup. This is the correctness path
 * for opaque server fragments whose output has `0..N` element cardinality:
 * the common exact-shape case keeps the O(depth) walk, while a shifted sibling
 * still hydrates correctly in production. Dev mode additionally logs the
 * mismatch.
 *
 * Hoisted into the runtime (rather than inlined per-island) so 60
 * islands on a page pay the helper cost once, not 60 times. With the
 * helper in the runtime the per-island JS only ships call sites:
 *
 *     import { walk_path } from "/rsjs-runtime.js";
 *     const __node_2 = walk_path(root, [0, 1], "data-rsjs-bind", "2");
 *
 * Each call site is ~70 bytes raw vs. ~700 bytes for an inlined helper.
 */
export function walk_path(root, path, marker_attr, marker_value) {
    let node = root;
    for (let i = 0; i < path.length; i++) {
        if (node == null || node.children == null) {
            node = null;
            break;
        }
        node = node.children[path[i]];
    }
    // Production spot-check: cheap getAttribute comparison on the
    // landed node. If the SSR markup drifted from the codegen's
    // expected shape (extra wrapper div, hot-reload mismatch, third-
    // party DOM mutation before hydrate), the walked node is the
    // *wrong* element — binding event handlers to it would silently
    // misdirect clicks. Resolve the verified marker inside this owner instead.
    //
    // Prior versions only ran this check under __RSJS_DEV__, so the prod path
    // could hand back a confidently-wrong DOM reference. The check costs one
    // getAttribute per hydrated binding and runs once at hydrate, not on every
    // event.
    const mismatch =
        node == null ||
        node.getAttribute == null ||
        node.getAttribute(marker_attr) !== marker_value;
    if (mismatch) {
        if (typeof globalThis !== 'undefined' && globalThis.__RSJS_DEV__ === true) {
            console.error(
                'rsjs: index-walk hydration mismatch for [' +
                    marker_attr +
                    '="' +
                    marker_value +
                    '"]; falling back to owner-scoped marker lookup. Path was',
                path,
            );
        }
        return find_owned_marker(root, marker_attr, marker_value);
    }
    return node;
}

const RSJS_COMPONENT_OWNER_ATTR = 'data-rsjs-component';

/**
 * Return the closest independent RSJS owner for `node`.
 *
 * Avoiding a compound CSS selector here keeps the ownership check usable in
 * small DOM adapters as well as browsers. Component fragments and explicit
 * islands both establish ownership boundaries; a parent fallback must never
 * capture a same-numbered marker inside either nested owner.
 */
function marker_owner(node) {
    let current = node;
    while (current != null) {
        const tag =
            typeof current.tagName === 'string'
                ? current.tagName.toLowerCase()
                : '';
        if (
            tag === 'rsjs-island' ||
            current.hasAttribute?.(RSJS_COMPONENT_OWNER_ATTR)
        ) {
            return current;
        }
        current = current.parentElement ?? null;
    }
    return null;
}

/**
 * Find one marker belonging to `root`'s owner, excluding nested component and
 * island owners. Attribute values are compared directly rather than embedded
 * in a CSS selector, so generated ids never need selector escaping.
 */
function find_owned_marker(root, marker_attr, marker_value) {
    if (root == null || typeof root.querySelectorAll !== 'function') {
        return null;
    }
    const owner = marker_owner(root);
    const candidates = root.querySelectorAll('[' + marker_attr + ']');
    for (const candidate of candidates) {
        if (
            candidate.getAttribute?.(marker_attr) === marker_value &&
            marker_owner(candidate) === owner
        ) {
            return candidate;
        }
    }
    return null;
}

/**
 * Structured owner-activation failure reported by managed island imports.
 *
 * The SSR owner stays inert when this error is produced. Context fields are
 * intentionally data-only so framework diagnostics can group failures without
 * parsing a human message.
 */
export class RsjsActivationError extends Error {
    constructor(context, cause) {
        super(
            `rsjs/owner-activation-failed: ${context.phase} failed for owner ` +
                `${JSON.stringify(context.owner_id)} (asset ${JSON.stringify(
                    context.asset_id,
                )})`,
            cause === undefined ? undefined : { cause },
        );
        this.name = 'RsjsActivationError';
        this.code = 'rsjs/owner-activation-failed';
        this.response_epoch = context.response_epoch;
        this.navigation_generation = context.navigation_generation;
        this.owner_id = context.owner_id;
        this.asset_id = context.asset_id;
        this.phase = context.phase;
        this.context = Object.freeze({
            response_epoch: context.response_epoch,
            navigation_generation: context.navigation_generation,
            owner_id: context.owner_id,
            asset_id: context.asset_id,
            phase: context.phase,
        });
    }
}

function managed_module_owner_id(asset_id, root) {
    if (root != null && typeof root.getAttribute === 'function') {
        return (
            root.getAttribute('data-rsjs-owner-id') ??
            root.getAttribute('data-rsjs-owner') ??
            root.getAttribute('data-rsjs-island-root') ??
            asset_id
        );
    }
    return asset_id;
}

function managed_module_initializer(ticket, init) {
    const context = {
        response_epoch: ticket.generation.response_epoch,
        navigation_generation: ticket.generation.navigation_generation,
        asset_id: ticket.asset_id,
        owner_id: ticket.owner_id,
        failed: false,
    };
    const wrapped = (root, resources) => init(root, resources);
    // This WeakMap belongs to the Symbol.for-backed managed-module registry,
    // so the framework runtime can recognize an initializer registered by a
    // generated module that imported the same runtime bytes through another
    // URL/module instance.
    MANAGED_MODULE_INIT_CONTEXTS.set(wrapped, context);
    return { init: wrapped, context };
}

function normalize_managed_module_url(url) {
    return new URL(String(url), import.meta.url).href;
}

function managed_module_url(module_url, load_id) {
    const url = new URL(normalize_managed_module_url(module_url));
    url.searchParams.set('rsjs-load', load_id);
    return url.href;
}

function managed_module_root_token(root) {
    if (
        root == null ||
        (typeof root !== 'object' && typeof root !== 'function')
    ) {
        return 'asset';
    }
    let id = MANAGED_MODULE_REGISTRY.root_ids.get(root);
    if (id == null) {
        id = MANAGED_MODULE_REGISTRY.next_root_id++;
        MANAGED_MODULE_REGISTRY.root_ids.set(root, id);
    }
    return `root-${id}`;
}

function managed_module_load_key(
    asset_id,
    module_url,
    generation,
    root_token,
) {
    return (
        `${generation.response_epoch}:${generation.navigation_generation}:` +
        `${asset_id}:${root_token}:${normalize_managed_module_url(module_url)}`
    );
}

function managed_module_load_is_current(ticket) {
    return (
        ticket != null &&
        ticket.status !== 'cancelled' &&
        ticket.status !== 'failed' &&
        page_generation_is_current(ticket.generation)
    );
}

function remove_managed_module_ticket(ticket, { remove_key = false } = {}) {
    if (MANAGED_MODULE_LOADS_BY_ID.get(ticket.id) === ticket) {
        MANAGED_MODULE_LOADS_BY_ID.delete(ticket.id);
    }
    if (
        remove_key &&
        MANAGED_MODULE_LOADS_BY_KEY.get(ticket.key) === ticket
    ) {
        MANAGED_MODULE_LOADS_BY_KEY.delete(ticket.key);
    }
}

function release_managed_module_ticket_resources(ticket) {
    ticket.root = null;
    ticket.registration = null;
    ticket.activation_start = null;
}

function managed_module_failure(ticket, phase, cause) {
    if (ticket.status === 'failed' || ticket.status === 'cancelled') {
        return;
    }
    ticket.status = 'failed';
    release_managed_module_ticket_resources(ticket);
    remove_managed_module_ticket(ticket);
    const error = new RsjsActivationError(
        {
            ...ticket.generation,
            owner_id: ticket.owner_id,
            asset_id: ticket.asset_id,
            phase,
        },
        cause,
    );
    report_hydration_error(
        `owner ${JSON.stringify(ticket.owner_id)} activation`,
        error,
    );
}

function cancel_managed_module_ticket(ticket) {
    if (
        ticket.status === 'activated' ||
        ticket.status === 'activating' ||
        ticket.status === 'failed' ||
        ticket.status === 'cancelled'
    ) {
        return;
    }
    ticket.status = 'cancelled';
    // A native import has no cancellation primitive and may never settle.
    // Remove its authorization immediately and release every value that can
    // retain the detached owner. If module evaluation eventually reaches
    // hydrate_from_module, the managed `rsjs-load` wrapper still fails inertly
    // because its single-use ticket no longer exists; it never falls through
    // to ordinary hydration.
    release_managed_module_ticket_resources(ticket);
    remove_managed_module_ticket(ticket, { remove_key: true });
}

function dispose_managed_module_loads() {
    for (const ticket of Array.from(MANAGED_MODULE_LOADS_BY_KEY.values())) {
        cancel_managed_module_ticket(ticket);
    }
    MANAGED_MODULE_LOADS_BY_KEY.clear();
}

function import_island_module(url) {
    return import(url);
}

function hydration_strategy_kind(strategy) {
    if (strategy === hydrate) return 'immediate';
    if (strategy === hydrate_when_visible) return 'visible';
    if (strategy === hydrate_when_idle) return 'idle';
    if (strategy === hydrate_on_interaction) return 'interaction';
    if (strategy === hydrate_when_media) return 'media';
    return null;
}

/**
 * Register one visible/idle/media module gate for the current page generation.
 *
 * The gate itself is generation-scoped, not only the import it may eventually
 * start. A router teardown can therefore invalidate an untriggered old gate
 * before replacement-page hydration scans run. Each root starts a root-scoped
 * ticket so one visible instance cannot hydrate an offscreen sibling.
 */
export function defer_island_module(
    asset_id,
    module_url,
    strategy,
    ...strategy_args
) {
    if (typeof strategy !== 'function') {
        throw new TypeError(
            'rsjs/defer-island-module: strategy must be a hydration function',
        );
    }
    const strategy_kind = hydration_strategy_kind(strategy);
    if (
        strategy_kind !== 'visible' &&
        strategy_kind !== 'idle' &&
        strategy_kind !== 'media'
    ) {
        throw new TypeError(
            'rsjs/defer-island-module: expected a visible, idle, or media strategy',
        );
    }
    const generation = capture_page_generation();
    const previous_authorization =
        MANAGED_MODULE_REGISTRY.active_authorization;
    MANAGED_MODULE_REGISTRY.active_authorization = generation;
    try {
        return strategy(
            asset_id,
            (root) => {
                if (!page_generation_is_current(generation)) {
                    return noop_disposer;
                }
                return load_island_module(
                    asset_id,
                    root,
                    module_url,
                    import_island_module,
                    strategy,
                    strategy_args,
                );
            },
            ...strategy_args,
        );
    } finally {
        MANAGED_MODULE_REGISTRY.active_authorization =
            previous_authorization;
    }
}

/**
 * Start one generation-scoped island ESM import.
 *
 * `importer` exists for deterministic runtime tests. Generated bootstraps use
 * the native dynamic-import default. The returned disposer cancels owner
 * authorization; an already-started native ESM fetch/evaluation may finish,
 * but its ticket can no longer activate anything.
 *
 * CSP note: this function uses dynamic `import()`. A nonce authorizes the
 * framework's inline module bootstrap, but deployments must additionally
 * allow the resolved runtime/island source in `script-src` or use
 * nonce-based `strict-dynamic` trust propagation.
 */
export function load_island_module(
    asset_id,
    root,
    module_url,
    importer = import_island_module,
    activation_strategy = null,
    activation_args = [],
) {
    if (typeof asset_id !== 'string' || asset_id.length === 0) {
        throw new TypeError(
            'rsjs/load-island-module: asset_id must be a non-empty string',
        );
    }
    if (typeof module_url !== 'string' || module_url.length === 0) {
        throw new TypeError(
            'rsjs/load-island-module: module_url must be a non-empty string',
        );
    }
    if (typeof importer !== 'function') {
        throw new TypeError(
            'rsjs/load-island-module: importer must be a function',
        );
    }
    if (
        activation_strategy != null &&
        hydration_strategy_kind(activation_strategy) == null
    ) {
        throw new TypeError(
            'rsjs/load-island-module: activation_strategy must be a built-in hydration function',
        );
    }
    if (!Array.isArray(activation_args)) {
        throw new TypeError(
            'rsjs/load-island-module: activation_args must be an array',
        );
    }

    const generation = capture_page_generation();
    const root_token = managed_module_root_token(root);
    const key = managed_module_load_key(
        asset_id,
        module_url,
        generation,
        root_token,
    );
    const existing = MANAGED_MODULE_LOADS_BY_KEY.get(key);
    if (existing != null) {
        if (
            existing.status === 'loading' ||
            existing.status === 'registered'
        ) {
            existing.subscribers += 1;
            return once_disposer(() => {
                existing.subscribers -= 1;
                if (existing.subscribers === 0) {
                    cancel_managed_module_ticket(existing);
                }
            });
        }
        // Activated and failed imports are terminal for this generation.
        // V1 never retries automatically.
        return noop_disposer;
    }

    const id =
        `g${generation.response_epoch}-${generation.navigation_generation}-` +
        `${MANAGED_MODULE_REGISTRY.next_load_id++}`;
    const requested_url = managed_module_url(module_url, id);
    const ticket = {
        id,
        key,
        asset_id,
        owner_id: managed_module_owner_id(asset_id, root),
        root,
        root_token,
        generation,
        requested_url,
        triggered: new URL(requested_url).searchParams.has('rsjs-triggered'),
        runtime_instance: MANAGED_MODULE_RUNTIME_INSTANCE,
        activation_kind:
            activation_strategy == null
                ? null
                : hydration_strategy_kind(activation_strategy),
        activation_start:
            activation_strategy == null
                ? null
                : (name, init) =>
                      activation_strategy(name, init, ...activation_args),
        status: 'loading',
        subscribers: 1,
        registration: null,
        consumed: false,
    };
    MANAGED_MODULE_LOADS_BY_ID.set(id, ticket);
    MANAGED_MODULE_LOADS_BY_KEY.set(key, ticket);

    let imported;
    try {
        imported = importer(requested_url);
    } catch (error) {
        managed_module_failure(ticket, 'import', error);
        return noop_disposer;
    }
    Promise.resolve(imported).then(
        () => complete_managed_module_load(ticket),
        (error) => {
            if (!managed_module_load_is_current(ticket)) {
                remove_managed_module_ticket(ticket, { remove_key: true });
                return;
            }
            managed_module_failure(ticket, 'import', error);
        },
    );

    return once_disposer(() => {
        ticket.subscribers -= 1;
        if (ticket.subscribers === 0) {
            cancel_managed_module_ticket(ticket);
        }
    });
}

/**
 * Native-import convenience for a bootstrap that owns the activation
 * strategy. The evaluated module contributes only its generated initializer;
 * the ticket-creating runtime remains authoritative for lifecycle state.
 */
export function load_island_module_for_strategy(
    asset_id,
    root,
    module_url,
    strategy,
    ...strategy_args
) {
    return load_island_module(
        asset_id,
        root,
        module_url,
        import_island_module,
        strategy,
        strategy_args,
    );
}

function complete_managed_module_load(ticket) {
    if (!managed_module_load_is_current(ticket)) {
        remove_managed_module_ticket(ticket, { remove_key: true });
        return;
    }
    if (ticket.consumed) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error('managed module ticket was consumed more than once'),
        );
        return;
    }
    const registration = ticket.registration;
    if (registration == null) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error(
                `island module did not register asset ${JSON.stringify(
                    ticket.asset_id,
                )}`,
            ),
        );
        return;
    }
    if (registration.name !== ticket.asset_id) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error(
                `island module registered asset ${JSON.stringify(
                    registration.name,
                )}, expected ${JSON.stringify(ticket.asset_id)}`,
            ),
        );
        return;
    }

    ticket.consumed = true;
    ticket.status = 'activating';
    remove_managed_module_ticket(ticket);
    const previous_authorization =
        MANAGED_MODULE_REGISTRY.active_authorization;
    MANAGED_MODULE_REGISTRY.active_authorization = ticket.generation;
    try {
        if (ticket.triggered) {
            if (
                ticket.root == null ||
                (typeof ticket.root !== 'object' &&
                    typeof ticket.root !== 'function')
            ) {
                throw new Error(
                    'triggered managed module did not retain its owner root',
                );
            }
            // Visible/idle/media gates are instance-local. Importing the
            // component module for one root must not turn the module's
            // component-wide registration into eager hydration of every
            // same-name root on the page.
            run_hydration_init(
                registration.name,
                ticket.root,
                registration.init,
            );
        } else {
            registration.start();
        }
        if (registration.init_context.failed) {
            ticket.status = 'failed';
            release_managed_module_ticket_resources(ticket);
            return;
        }
        ticket.status = 'activated';
        release_managed_module_ticket_resources(ticket);
    } catch (error) {
        managed_module_failure(ticket, 'init', error);
    } finally {
        MANAGED_MODULE_REGISTRY.active_authorization =
            previous_authorization;
    }
}

function register_managed_island_module(
    load_id,
    module_url,
    strategy,
    name,
    init,
    args,
) {
    const ticket = MANAGED_MODULE_LOADS_BY_ID.get(load_id);
    if (ticket == null || !managed_module_load_is_current(ticket)) {
        return noop_disposer;
    }
    if (ticket.consumed || ticket.registration != null) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error('managed module registered its initializer more than once'),
        );
        return noop_disposer;
    }
    if (normalize_managed_module_url(module_url) !== ticket.requested_url) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error('managed module URL did not match its import ticket'),
        );
        return noop_disposer;
    }
    const registered_kind = hydration_strategy_kind(strategy);
    if (registered_kind == null) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error('managed module registered an unknown hydration strategy'),
        );
        return noop_disposer;
    }
    if (
        ticket.activation_kind != null &&
        registered_kind !== ticket.activation_kind
    ) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error(
                `managed module registered ${registered_kind} hydration, expected ${ticket.activation_kind}`,
            ),
        );
        return noop_disposer;
    }
    if (
        ticket.activation_start == null &&
        ticket.runtime_instance !== MANAGED_MODULE_RUNTIME_INSTANCE
    ) {
        managed_module_failure(
            ticket,
            'dependency',
            new Error(
                'managed module used a different runtime instance without a ticket-owned activation strategy',
            ),
        );
        return noop_disposer;
    }
    const managed_init = managed_module_initializer(ticket, init);
    ticket.status = 'registered';
    ticket.registration = {
        name,
        init: managed_init.init,
        init_context: managed_init.context,
        start:
            ticket.activation_start == null
                ? () => strategy(name, managed_init.init, ...args)
                : () => ticket.activation_start(name, managed_init.init),
    };
    return once_disposer(() => {
        if (ticket.registration?.name === name) {
            cancel_managed_module_ticket(ticket);
        }
    });
}

/**
 * Select the hydration entry for one generated island module.
 *
 * Normal `<script type="module">` evaluation delegates to `strategy`.
 * A `rsjs-load` URL is a managed import and may only register an inert factory
 * against that exact single-use ticket. `rsjs-triggered` means a visible,
 * idle, or media gate already fired for one retained owner root. Successful
 * authorization initializes only that root; sibling instances keep their own
 * gates and tickets.
 */
export function hydrate_from_module(module_url, strategy) {
    if (typeof strategy !== 'function') {
        throw new TypeError(
            'rsjs/hydrate-from-module: strategy must be a hydration function',
        );
    }
    const url = new URL(normalize_managed_module_url(module_url));
    const load_id = url.searchParams.get('rsjs-load');
    if (load_id == null) {
        return strategy;
    }
    return (name, init, ...args) =>
        register_managed_island_module(
            load_id,
            url.href,
            strategy,
            name,
            init,
            args,
        );
}

export function hydrate(name, init) {
    return register_hydration(name, (skip_tracked) =>
        hydrate_matching_roots(
            name,
            init,
            (root) => run_hydration_init(name, root, init),
            skip_tracked,
        ),
    );
}

export function hydrate_when_visible(name, init, options) {
    return register_hydration(name, (skip_tracked) =>
        hydrate_matching_roots(
            name,
            init,
            (root) => schedule_visible_hydration(name, root, init, options),
            skip_tracked,
        ),
    );
}

export function hydrate_when_idle(name, init, options) {
    return register_hydration(name, (skip_tracked) =>
        hydrate_matching_roots(
            name,
            init,
            (root) => schedule_idle_hydration(name, root, init, options),
            skip_tracked,
        ),
    );
}

export function hydrate_on_interaction(name, init, events) {
    return register_hydration(name, (skip_tracked) =>
        hydrate_matching_roots(
            name,
            init,
            (root) => schedule_interaction_hydration(name, root, init, events),
            skip_tracked,
        ),
    );
}

export function hydrate_when_media(name, init, query) {
    return register_hydration(name, (skip_tracked) =>
        hydrate_matching_roots(
            name,
            init,
            (root) => schedule_media_hydration(name, root, init, query),
            skip_tracked,
        ),
    );
}

function register_hydration(name, start) {
    const active_authorization =
        MANAGED_MODULE_REGISTRY.active_authorization;
    const registration = {
        name,
        start,
        managed_generation:
            active_authorization == null
                ? null
                : { ...active_authorization },
    };
    HYDRATION_REGISTRATIONS.set(name, registration);
    ensure_navigation_rehydration();
    const initial_disposer = hydrate_when_dom_ready(name, () => start(false));
    return once_disposer(() => {
        initial_disposer();
        if (HYDRATION_REGISTRATIONS.get(name) === registration) {
            HYDRATION_REGISTRATIONS.delete(name);
            dispose_hydrated(name);
        }
    });
}

function ensure_navigation_rehydration() {
    if (
        typeof document === 'undefined' ||
        typeof document.addEventListener !== 'function' ||
        NAVIGATION_REHYDRATION_DOCUMENTS.has(document)
    ) {
        return;
    }
    NAVIGATION_REHYDRATION_DOCUMENTS.add(document);
    document.addEventListener('turbo:render', rehydrate_registered_islands);
    document.addEventListener('turbo:load', rehydrate_registered_islands);
    // Proa's out-of-order streaming runtime dispatches this after it has
    // inserted a completed boundary. Island ESM is intentionally evaluated
    // once per URL, so an already-registered island must rescan for roots
    // added by later boundaries instead of relying on its module to rerun.
    document.addEventListener('proa:boundary-ready', rehydrate_registered_islands);
}

function rehydrate_registered_islands() {
    for (const registration of HYDRATION_REGISTRATIONS.values()) {
        if (
            registration.managed_generation != null &&
            !page_generation_is_current(registration.managed_generation)
        ) {
            continue;
        }
        hydrate_when_dom_ready(registration.name, () => registration.start(true));
    }
}

function hydrate_when_dom_ready(name, start) {
    if (
        typeof document !== 'undefined' &&
        document.readyState === 'loading'
    ) {
        // DOM is still parsing; defer until DOMContentLoaded so
        // find_island_roots actually observes the markup. Return a
        // disposer that cancels the pending hydration if the caller
        // (e.g. a framework adapter) unmounts before DCL fires.
        // Without the cancellation, the queued listener would still
        // call `init(root)` against an island the caller has already
        // torn down. Belt-and-braces: removeEventListener + a
        // `cancelled` flag (so the handler bails even if the listener
        // somehow fires before removal lands).
        let cancelled = false;
        let child_disposer = noop_disposer;
        let record;
        const handler = () => {
            PENDING_HYDRATIONS.delete(record);
            if (cancelled) return;
            child_disposer = start();
        };
        const dispose_pending = once_disposer(() => {
            cancelled = true;
            document.removeEventListener('DOMContentLoaded', handler);
            PENDING_HYDRATIONS.delete(record);
            stop_detached_root_observer_if_idle();
            child_disposer();
        });
        dispose_existing_pending_hydration(name, null);
        record = { name, root: null, dispose: dispose_pending };
        PENDING_HYDRATIONS.add(record);
        document.addEventListener('DOMContentLoaded', handler, {
            once: true,
        });
        return dispose_pending;
    }
    return start();
}

function hydrate_matching_roots(name, init, schedule_root, skip_tracked = false) {
    const roots = find_island_roots(name);
    if (roots.length === 0) {
        // A navigation replay visits every island module the browser has ever
        // evaluated. Most of those islands are absent from the current route;
        // silently skip them instead of invoking the dev-only document.body
        // fallback or logging a production missing-root error.
        if (skip_tracked) {
            return noop_disposer;
        }
        // No markers found. In dev mode, fall back to document.body
        // so the manual smoke test keeps working without a server
        // emitter. In production, that fallback would silently mask
        // SSR pipeline bugs (e.g. the island JS loaded but the server
        // forgot to wrap the markup), so we log an error and skip.
        if (is_dev_mode()) {
            console.warn(
                `rsjs: no <rsjs-island data-rsjs-island-root="${name}"> found; hydrating against document.body (dev mode)`,
            );
            if (typeof document !== 'undefined') {
                const disposer = schedule_root(document.body);
                return typeof disposer === 'function' ? disposer : noop_disposer;
            }
        } else {
            console.error(
                `rsjs: no <rsjs-island data-rsjs-island-root="${name}"> found; hydration skipped. ` +
                    `(Set globalThis.__RSJS_DEV__ = true, or run with NODE_ENV != "production", to fall back to document.body.)`,
            );
        }
        return noop_disposer;
    }
    const disposers = [];
    for (const root of roots) {
        if (skip_tracked && hydration_is_tracked(name, root)) {
            continue;
        }
        const disposer = schedule_root(root);
        if (typeof disposer === 'function') {
            disposers.push(disposer);
        }
    }
    if (disposers.length === 0) {
        return noop_disposer;
    }
    return once_disposer(() => dispose_all(disposers, `island "${name}"`));
}

function hydration_is_tracked(name, root) {
    if (HYDRATED_ROOTS.get(root)?.has(name)) {
        return true;
    }
    for (const record of PENDING_HYDRATIONS) {
        if (record.name === name && record.root === root) {
            return true;
        }
    }
    return false;
}

function schedule_visible_hydration(name, root, init, options) {
    if (
        typeof IntersectionObserver === 'undefined' ||
        root == null ||
        typeof root !== 'object'
    ) {
        return run_hydration_init(name, root, init);
    }
    dispose_existing_pending_hydration(name, root);
    let child_disposer = noop_disposer;
    let record;
    let hydrated = false;
    let cancelled = false;
    const hydrate_root = () => {
        if (cancelled || hydrated) return;
        hydrated = true;
        PENDING_HYDRATIONS.delete(record);
        observer.disconnect();
        child_disposer = run_hydration_init(name, root, init);
    };
    let observer;
    observer = new IntersectionObserver((entries) => {
        for (const entry of entries) {
            if (
                entry &&
                entry.target === root &&
                (entry.isIntersecting || entry.intersectionRatio > 0)
            ) {
                hydrate_root();
                break;
            }
        }
    }, options || {});
    const dispose_pending = once_disposer(() => {
        cancelled = true;
        PENDING_HYDRATIONS.delete(record);
        dispose_hydrations_in_owned_subtree(root, false);
        stop_detached_root_observer_if_idle();
        observer.disconnect();
        child_disposer();
    });
    record = { name, root, dispose: dispose_pending };
    PENDING_HYDRATIONS.add(record);
    ensure_detached_root_observer();
    observer.observe(root);
    return dispose_pending;
}

function schedule_idle_hydration(name, root, init, options) {
    dispose_existing_pending_hydration(name, root);
    let child_disposer = noop_disposer;
    let record;
    let hydrated = false;
    let cancelled = false;
    let handle;
    let cancel_scheduled;
    const hydrate_root = () => {
        if (cancelled || hydrated) return;
        hydrated = true;
        PENDING_HYDRATIONS.delete(record);
        stop_detached_root_observer_if_idle();
        child_disposer = run_hydration_init(name, root, init);
    };
    const timeout = options && Number.isFinite(options.timeout) ? options.timeout : undefined;
    if (typeof requestIdleCallback === 'function') {
        handle = requestIdleCallback(hydrate_root, timeout === undefined ? undefined : { timeout });
        cancel_scheduled =
            typeof cancelIdleCallback === 'function'
                ? () => cancelIdleCallback(handle)
                : noop_disposer;
    } else {
        handle = setTimeout(hydrate_root, 0);
        cancel_scheduled = () => clearTimeout(handle);
    }
    const dispose_pending = once_disposer(() => {
        cancelled = true;
        PENDING_HYDRATIONS.delete(record);
        dispose_hydrations_in_owned_subtree(root, false);
        stop_detached_root_observer_if_idle();
        if (!hydrated) {
            cancel_scheduled();
        }
        child_disposer();
    });
    record = { name, root, dispose: dispose_pending };
    PENDING_HYDRATIONS.add(record);
    ensure_detached_root_observer();
    return dispose_pending;
}

function schedule_interaction_hydration(name, root, init, events) {
    if (root == null || typeof root.addEventListener !== 'function') {
        return run_hydration_init(name, root, init);
    }
    dispose_existing_pending_hydration(name, root);
    const event_names =
        Array.isArray(events) && events.length > 0
            ? events
            : ['pointerdown', 'keydown', 'focusin'];
    let child_disposer = noop_disposer;
    let record;
    let hydrated = false;
    let cancelled = false;
    const cleanup_listeners = () => {
        for (const event_name of event_names) {
            root.removeEventListener(event_name, hydrate_root, true);
        }
    };
    // This listener runs in capture. Successful initialization installs the
    // generated delegated bubble listener before the same native event reaches
    // the root again, so the triggering interaction is delivered exactly once
    // without preventDefault(), propagation suppression, or synthetic replay.
    const hydrate_root = () => {
        if (cancelled || hydrated) return;
        hydrated = true;
        PENDING_HYDRATIONS.delete(record);
        stop_detached_root_observer_if_idle();
        cleanup_listeners();
        child_disposer = run_hydration_init(name, root, init);
    };
    const dispose_pending = once_disposer(() => {
        cancelled = true;
        PENDING_HYDRATIONS.delete(record);
        dispose_hydrations_in_owned_subtree(root, false);
        stop_detached_root_observer_if_idle();
        cleanup_listeners();
        child_disposer();
    });
    record = { name, root, dispose: dispose_pending };
    PENDING_HYDRATIONS.add(record);
    ensure_detached_root_observer();
    for (const event_name of event_names) {
        root.addEventListener(event_name, hydrate_root, true);
    }
    return dispose_pending;
}

function schedule_media_hydration(name, root, init, query) {
    if (
        typeof query !== 'string' ||
        query.length === 0 ||
        typeof matchMedia !== 'function'
    ) {
        return run_hydration_init(name, root, init);
    }
    dispose_existing_pending_hydration(name, root);
    const media = matchMedia(query);
    if (!media || media.matches) {
        return run_hydration_init(name, root, init);
    }
    let child_disposer = noop_disposer;
    let record;
    let hydrated = false;
    let cancelled = false;
    const remove_listener = () => {
        if (typeof media.removeEventListener === 'function') {
            media.removeEventListener('change', on_change);
        } else if (typeof media.removeListener === 'function') {
            media.removeListener(on_change);
        }
    };
    const hydrate_root = () => {
        if (cancelled || hydrated) return;
        hydrated = true;
        PENDING_HYDRATIONS.delete(record);
        stop_detached_root_observer_if_idle();
        remove_listener();
        child_disposer = run_hydration_init(name, root, init);
    };
    const on_change = (event) => {
        if (event && event.matches) {
            hydrate_root();
        }
    };
    if (typeof media.addEventListener === 'function') {
        media.addEventListener('change', on_change);
    } else if (typeof media.addListener === 'function') {
        media.addListener(on_change);
    } else {
        return run_hydration_init(name, root, init);
    }
    const dispose_pending = once_disposer(() => {
        cancelled = true;
        PENDING_HYDRATIONS.delete(record);
        dispose_hydrations_in_owned_subtree(root, false);
        stop_detached_root_observer_if_idle();
        remove_listener();
        child_disposer();
    });
    record = { name, root, dispose: dispose_pending };
    PENDING_HYDRATIONS.add(record);
    ensure_detached_root_observer();
    return dispose_pending;
}

/**
 * Tear down one hydration record. Teardown is a lifecycle boundary: a single
 * island whose cleanup throws must not skip the remaining islands or the page
 * store release that follows, so the error is reported and the walk
 * continues. This mirrors dispose_all() and dispose_existing_hydration().
 */
function dispose_hydration_record(record) {
    ISLAND_STATE_CACHE.delete(record.root);
    try {
        record.dispose();
    } catch (error) {
        report_hydration_error(`island "${record.name}" teardown`, error);
    }
}

export function dispose_hydrated(name, root) {
    const full_page_teardown = name == null && root == null;
    if (full_page_teardown) {
        // Invalidate the generation before touching DOM records or optional
        // resources. Promise continuations queued by cleanup can therefore
        // never observe the replacement page as authorized old work.
        advance_page_generation();
        dispose_managed_module_loads();
    }
    let disposed = 0;
    for (const record of Array.from(PENDING_HYDRATIONS)) {
        if (typeof name === 'string' && record.name !== name) {
            continue;
        }
        if (root != null && record.root !== root) {
            continue;
        }
        dispose_hydration_record(record);
        disposed += 1;
    }
    for (const record of Array.from(HYDRATED_RECORDS)) {
        if (typeof name === 'string' && record.name !== name) {
            continue;
        }
        if (root != null && record.root !== root) {
            continue;
        }
        dispose_hydration_record(record);
        disposed += 1;
    }
    if (full_page_teardown) {
        // An unfiltered teardown is the page/route lifecycle boundary. Scoped
        // island disposal must keep page stores alive for sibling islands;
        // full teardown must release them so a new page may reuse scope ids
        // without inheriting stale values.
        dispose_page_shared_state();
        dispose_page_generation_resources();
    } else if (root != null && (typeof root === 'object' || typeof root === 'function')) {
        // Explicit root disposal is also the cache boundary for a router or
        // fragment system that keeps the root object and replaces its adjacent
        // hydration metadata before initializing it again.
        ISLAND_STATE_CACHE.delete(root);
    }
    return disposed;
}

/**
 * Whether `root` is the container itself or lives inside it. A root that has
 * already left the document counts too: it is due for teardown either way, and
 * a caller that just detached a subtree should not have to wait for the
 * asynchronous detached-root pruning to reclaim it.
 */
function is_root_in_subtree(container, root) {
    if (!is_watchable_root(root)) return false;
    if (root === container) return true;
    if (is_detached_root(root)) return true;
    if (typeof container.contains === 'function') {
        return container.contains(root) === true;
    }
    // Minimal DOM shims (and adapters running outside a browser) may expose
    // only parent links.
    let node = root;
    while (node != null) {
        if (node === container) return true;
        node = node.parentNode ?? node.parentElement ?? null;
    }
    return false;
}

/**
 * Return the DOM depth of `root` inside `container`, or -1 when it is not in
 * that live ownership range. Unlike router subtree disposal, this deliberately
 * does not claim unrelated roots merely because they are already detached.
 */
function owned_subtree_depth(container, root, include_container) {
    if (!is_watchable_root(container) || !is_watchable_root(root)) {
        return -1;
    }
    if (root === container) {
        return include_container ? 0 : -1;
    }
    let depth = 1;
    let node = root.parentNode ?? root.parentElement ?? null;
    while (node != null) {
        if (node === container) {
            return depth;
        }
        depth += 1;
        node = node.parentNode ?? node.parentElement ?? null;
    }
    // Full browser DOM nodes expose contains() even when a minimal adapter
    // omits parent links. Such nodes are still owned; depth 1 retains stable
    // reverse-registration ordering among them.
    if (typeof container.contains === 'function' && container.contains(root)) {
        return 1;
    }
    return -1;
}

/**
 * Dispose every pending or hydrated owner in one live structural range.
 *
 * Deepest owners run first. Equal-depth records unwind in reverse
 * registration order, matching the resource-stack rule within one owner.
 * Callers use this before removing a structurally owned node and when an outer
 * owner terminates, so detached-root MutationObserver pruning is only a
 * fallback rather than the correctness path.
 */
function dispose_hydrations_in_owned_subtree(container, include_container = false) {
    if (!is_watchable_root(container)) {
        return 0;
    }
    const candidates = [];
    let registration_order = 0;
    for (const record of PENDING_HYDRATIONS) {
        const depth = owned_subtree_depth(container, record.root, include_container);
        if (depth >= 0) {
            candidates.push({ record, depth, registration_order });
        }
        registration_order += 1;
    }
    for (const record of HYDRATED_RECORDS) {
        const depth = owned_subtree_depth(container, record.root, include_container);
        if (depth >= 0) {
            candidates.push({ record, depth, registration_order });
        }
        registration_order += 1;
    }
    candidates.sort(
        (a, b) =>
            b.depth - a.depth ||
            b.registration_order - a.registration_order,
    );

    let disposed = 0;
    for (const { record } of candidates) {
        if (
            !PENDING_HYDRATIONS.has(record) &&
            !HYDRATED_RECORDS.has(record)
        ) {
            continue;
        }
        dispose_hydration_record(record);
        disposed += 1;
    }
    return disposed;
}

/**
 * Tear down only the islands that live inside `container` (plus any island
 * root that is already detached).
 *
 * This is the teardown a router wants when it replaces one container rather
 * than the whole page: islands in the persistent shell (header, nav, cart
 * badge) keep running, because nothing re-hydrates them after a soft
 * navigation. Page-scoped shared stores are deliberately left alone for the
 * same reason, as are optional generation resources such as the query cache:
 * shell islands still hold them. Use dispose_hydrated() with no arguments for
 * a real page transition, which releases the complete generation.
 */
export function dispose_hydrated_subtree(container) {
    if (!is_watchable_root(container)) {
        throw new TypeError(
            'rsjs/dispose-subtree-invalid-container: dispose_hydrated_subtree(container) needs the ' +
                `element whose subtree is being replaced, received ${
                    container === null ? 'null' : typeof container
                }. Pass the container element the patch replaces, or call dispose_hydrated() with ` +
                'no arguments for a full page teardown.',
        );
    }
    let disposed = 0;
    for (const record of Array.from(PENDING_HYDRATIONS)) {
        if (!is_root_in_subtree(container, record.root)) {
            continue;
        }
        dispose_hydration_record(record);
        disposed += 1;
    }
    for (const record of Array.from(HYDRATED_RECORDS)) {
        if (!is_root_in_subtree(container, record.root)) {
            continue;
        }
        dispose_hydration_record(record);
        disposed += 1;
    }
    ISLAND_STATE_CACHE.delete(container);
    return disposed;
}

function report_owner_init_failure(root, init, label, cause) {
    const managed = MANAGED_MODULE_INIT_CONTEXTS.get(init);
    if (managed == null) {
        report_hydration_error(label, cause);
        return;
    }
    managed.failed = true;
    const owner_id = managed_module_owner_id(managed.asset_id, root);
    report_hydration_error(
        `owner ${JSON.stringify(owner_id)} activation`,
        new RsjsActivationError(
            {
                response_epoch: managed.response_epoch,
                navigation_generation: managed.navigation_generation,
                owner_id,
                asset_id: managed.asset_id,
                phase: 'init',
            },
            cause,
        ),
    );
}

function run_hydration_init(name, root, init) {
    // Validate and install the complete page provider table before any owner
    // initializer can attach listeners or effects. This makes a mixed schema
    // ABI or conflicting repeated seed an atomic page-protocol failure rather
    // than an error discovered halfway through one owner's activation.
    try {
        seed_page_shared_state_table();
    } catch (error) {
        report_owner_init_failure(
            root,
            init,
            `island "${name}" shared-state install`,
            error,
        );
        return noop_disposer;
    }
    dispose_existing_pending_hydration(name, root);
    dispose_existing_hydration(name, root);
    if (root != null && (typeof root === 'object' || typeof root === 'function')) {
        ISLAND_STATE_CACHE.delete(root);
    }
    // Generated initializers push every listener/effect/subscription into this
    // runtime-owned stack as soon as it is created. Keeping the stack outside
    // `init` lets us unwind a partially initialized owner when a later binding
    // throws, including interaction hydration while the triggering event is
    // still in its capture phase.
    const resources = [];
    const previous_initialization = ACTIVE_OWNER_INITIALIZATION;
    const initialization = { form_commits: [] };
    ACTIVE_OWNER_INITIALIZATION = initialization;
    try {
        const disposer = init(root, resources);
        // Dirty controls have now adopted their live DOM state. Commit the
        // final owner state to untouched controls as one synchronous batch
        // before an ordinary reactive microtask can run.
        batch(() => {
            for (const commit of initialization.form_commits) {
                commit();
            }
        });
        const child_disposer = owner_resource_disposer(
            name,
            resources,
            disposer,
        );
        return register_hydration_disposer(name, root, child_disposer);
    } catch (error) {
        dispose_all(resources, `island "${name}" init rollback`);
        report_owner_init_failure(
            root,
            init,
            `island "${name}" init`,
            error,
        );
        return noop_disposer;
    } finally {
        ACTIVE_OWNER_INITIALIZATION = previous_initialization;
    }
}

function dispose_existing_hydration(name, root) {
    if (root == null || (typeof root !== 'object' && typeof root !== 'function')) {
        return;
    }
    const previous = HYDRATED_ROOTS.get(root)?.get(name);
    if (previous == null) {
        return;
    }
    try {
        previous.dispose();
    } catch (error) {
        report_hydration_error(`island "${name}" stale hydration`, error);
    }
}

function dispose_existing_pending_hydration(name, root) {
    for (const record of Array.from(PENDING_HYDRATIONS)) {
        if (record.name === name && record.root === root) {
            record.dispose();
        }
    }
}

function register_hydration_disposer(name, root, disposer) {
    if (root == null || (typeof root !== 'object' && typeof root !== 'function')) {
        return disposer;
    }

    let by_name = HYDRATED_ROOTS.get(root);
    if (by_name == null) {
        by_name = new Map();
        HYDRATED_ROOTS.set(root, by_name);
    }

    const previous = by_name.get(name);
    if (previous != null) {
        try {
            previous.dispose();
        } catch (error) {
            report_hydration_error(`island "${name}" stale hydration`, error);
        }
        HYDRATED_ROOTS.set(root, by_name);
    }

    let record;
    const wrapped = once_disposer(() => {
        if (by_name.get(name) === record) {
            by_name.delete(name);
            if (by_name.size === 0) {
                HYDRATED_ROOTS.delete(root);
            }
        }
        HYDRATED_RECORDS.delete(record);
        // Every dispose path funnels through this wrapper, including the
        // automatic detached-root pruning that never calls
        // dispose_hydrated(). Invalidate the island's cached hydration
        // blob here so a morphing router that keeps the root element but
        // rewrites the adjacent state script reparses it on rehydration.
        ISLAND_STATE_CACHE.delete(root);
        dispose_hydrations_in_owned_subtree(root, false);
        stop_detached_root_observer_if_idle();
        disposer();
    });
    record = { name, root, dispose: wrapped };
    by_name.set(name, record);
    HYDRATED_RECORDS.add(record);
    ensure_detached_root_observer();
    return wrapped;
}

function ensure_detached_root_observer() {
    if (
        DETACHED_ROOT_OBSERVER !== null ||
        typeof MutationObserver === 'undefined' ||
        typeof document === 'undefined' ||
        !has_watchable_hydration_records()
    ) {
        return;
    }
    const target = document.documentElement ?? document.body;
    if (target == null || typeof target !== 'object') {
        return;
    }
    DETACHED_ROOT_OBSERVER = new MutationObserver(prune_detached_hydrations);
    DETACHED_ROOT_OBSERVER.observe(target, { childList: true, subtree: true });
}

function stop_detached_root_observer_if_idle() {
    if (DETACHED_ROOT_OBSERVER === null || has_watchable_hydration_records()) {
        return;
    }
    DETACHED_ROOT_OBSERVER.disconnect();
    DETACHED_ROOT_OBSERVER = null;
}

function has_watchable_hydration_records() {
    for (const record of PENDING_HYDRATIONS) {
        if (is_watchable_root(record.root)) {
            return true;
        }
    }
    for (const record of HYDRATED_RECORDS) {
        if (is_watchable_root(record.root)) {
            return true;
        }
    }
    return false;
}

function prune_detached_hydrations() {
    for (const record of Array.from(PENDING_HYDRATIONS)) {
        if (is_detached_root(record.root)) {
            dispose_hydration_record(record);
        }
    }
    for (const record of Array.from(HYDRATED_RECORDS)) {
        if (is_detached_root(record.root)) {
            dispose_hydration_record(record);
        }
    }
    stop_detached_root_observer_if_idle();
}

function is_watchable_root(root) {
    return root != null && (typeof root === 'object' || typeof root === 'function');
}

function is_detached_root(root) {
    return is_watchable_root(root) && typeof root.isConnected === 'boolean' && !root.isConnected;
}

function once_disposer(dispose) {
    let disposed = false;
    return () => {
        if (disposed) {
            return;
        }
        disposed = true;
        dispose();
    };
}

function noop_disposer() {
    /* no cleanup registered */
}

/**
 * Look up every rendered island root for `name`. Returns an empty
 * NodeList-like array when the runtime is loaded outside a DOM
 * (e.g. during `node --check` validation) or when the page contains
 * no `<rsjs-island data-rsjs-island-root="<name>">` elements at all.
 *
 * Multiple matches are normal: a page can render the same island
 * many times (a UserCard list, a row of identical Counter widgets).
 * Every match is hydrated independently.
 */
function find_island_roots(name) {
    if (typeof document === 'undefined') {
        // Node / non-DOM environment (e.g. `node --check` validation). Be
        // resilient: callers in those environments usually don't run hydrate.
        return [];
    }
    const escaped = css_escape(name);
    const roots = document.querySelectorAll(
        `rsjs-island[data-rsjs-island-root="${escaped}"]`,
    );
    if (roots.length > 0) {
        return roots;
    }
    // Backward compatibility for HTML rendered by older rsjs versions. New
    // renderers use data-rsjs-island-root so repeated island instances do not
    // duplicate DOM id attributes.
    return document.querySelectorAll(`rsjs-island[id="${escaped}"]`);
}

/**
 * Minimal CSS attribute-selector escape. We only need to make sure the
 * string can sit inside a double-quoted attribute selector. Backslashes
 * and quotes get escaped; everything else is passed through.
 */
function css_escape(s) {
    return s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

// The client router is a separately bundled package, so it cannot statically
// import the exact runtime asset selected by the page manifest. A global-symbol
// bridge keeps that integration explicit without creating a second runtime
// instance or coupling the router package to an asset URL.
//
// `disposeSubtree` and `registerPageDisposer` are additive members of ABI 1:
// routers/optional modules feature-detect them and retain their prior fallback
// when absent. Bumping the ABI would make every current router refuse a runtime
// that merely gained a capability.
if (typeof globalThis !== 'undefined' && typeof Symbol !== 'undefined') {
    const page_runtime_symbol = Symbol.for('rsjs.page-runtime');
    // The framework-resolved runtime evaluates before managed island imports.
    // A generated module may then reach the same bytes through a canonical
    // alias; that later module instance must not replace the bridge that owns
    // the authoritative hydration lifecycle records.
    if (globalThis[page_runtime_symbol] == null) {
        globalThis[page_runtime_symbol] = Object.freeze({
            abi_version: PAGE_RUNTIME_ABI_VERSION,
            // Full page transition: every island goes, page stores included.
            disposePage() {
                dispose_hydrated();
            },
            // Container patch: only the islands being removed go, and the page
            // stores stay live for the shell islands that are still mounted.
            disposeSubtree(container) {
                return dispose_hydrated_subtree(container);
            },
            // Optional runtime modules register complete-generation cleanup here.
            // Container/subtree disposal intentionally never invokes these hooks.
            registerPageDisposer(dispose) {
                return register_page_generation_disposer(dispose);
            },
            installPage() {
                install_page_shared_state_from_document();
                rehydrate_registered_islands();
            },
        });
    }
}
