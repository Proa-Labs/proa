// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for hydrate() disposer lifecycle. Run with:
//
//     node rsjs/runtime/rsjs-runtime.hydration.test.mjs

import {
    bind_delegated_events,
    dispose_hydrated,
    dispose_all,
    hydrate,
    hydrate_on_interaction,
    hydrate_when_idle,
    hydrate_when_media,
    hydrate_when_visible,
} from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

function install_document({ readyState = 'complete', roots = [], legacy_roots = [] } = {}) {
    const listeners = new Map();
    const doc = {
        readyState,
        body: { name: 'body' },
        querySelectorAll(selector) {
            if (selector === 'rsjs-island[data-rsjs-island-root="counter"]') {
                return roots;
            }
            if (selector === 'rsjs-island[id="counter"]') {
                return legacy_roots;
            }
            return [];
        },
        addEventListener(name, handler, options) {
            if (!listeners.has(name)) listeners.set(name, []);
            listeners.get(name).push({ handler, once: !!options?.once });
        },
        removeEventListener(name, handler) {
            const current = listeners.get(name) ?? [];
            listeners.set(
                name,
                current.filter((entry) => entry.handler !== handler),
            );
        },
        dispatch(name) {
            if (name === 'DOMContentLoaded') {
                doc.readyState = 'complete';
            }
            const current = [...(listeners.get(name) ?? [])];
            for (const entry of current) {
                if (entry.once) {
                    doc.removeEventListener(name, entry.handler);
                }
                entry.handler();
            }
        },
        listener_count(name) {
            return (listeners.get(name) ?? []).length;
        },
    };
    globalThis.document = doc;
    return doc;
}

function make_interaction_tree() {
    function make_element(tag_name, attrs = {}) {
        const listeners = new Map();
        return {
            nodeType: 1,
            tagName: tag_name.toUpperCase(),
            attrs: { ...attrs },
            children: [],
            parentElement: null,
            getAttribute(name) {
                return Object.prototype.hasOwnProperty.call(this.attrs, name)
                    ? this.attrs[name]
                    : null;
            },
            hasAttribute(name) {
                return Object.prototype.hasOwnProperty.call(this.attrs, name);
            },
            appendChild(child) {
                child.parentElement = this;
                this.children.push(child);
                return child;
            },
            addEventListener(name, handler, options) {
                if (!listeners.has(name)) listeners.set(name, []);
                listeners.get(name).push({
                    handler,
                    capture: options === true || options?.capture === true,
                });
            },
            removeEventListener(name, handler, options) {
                const capture = options === true || options?.capture === true;
                listeners.set(
                    name,
                    (listeners.get(name) ?? []).filter(
                        (entry) =>
                            entry.handler !== handler || entry.capture !== capture,
                    ),
                );
            },
            listener_count(name, capture) {
                const entries = listeners.get(name) ?? [];
                return capture === undefined
                    ? entries.length
                    : entries.filter((entry) => entry.capture === capture).length;
            },
            closest(selector) {
                const match = /^\[([a-zA-Z0-9-]+)\]$/.exec(selector);
                if (match == null) {
                    throw new Error(`unsupported selector: ${selector}`);
                }
                let current = this;
                while (current != null) {
                    if (current.hasAttribute?.(match[1])) return current;
                    current = current.parentElement;
                }
                return null;
            },
            contains(other) {
                let current = other;
                while (current != null) {
                    if (current === this) return true;
                    current = current.parentElement;
                }
                return false;
            },
            dispatchEvent(event) {
                if (event.target == null) event.target = this;
                const path = [];
                let current = this;
                while (current != null) {
                    path.push(current);
                    current = current.parentElement;
                }
                for (const node of [...path].reverse()) {
                    for (const entry of [...(node.__listeners(event.type) ?? [])]) {
                        if (entry.capture) entry.handler(event);
                    }
                }
                for (const node of path) {
                    for (const entry of [...(node.__listeners(event.type) ?? [])]) {
                        if (!entry.capture) entry.handler(event);
                    }
                }
                return true;
            },
            __listeners(name) {
                return listeners.get(name);
            },
        };
    }

    const root = make_element('rsjs-island');
    const button = make_element('button', { 'data-rsjs-handler': '0' });
    root.appendChild(button);
    return { root, button };
}

const previous_document = globalThis.document;
const previous_intersection_observer = globalThis.IntersectionObserver;
const previous_mutation_observer = globalThis.MutationObserver;
const previous_request_idle_callback = globalThis.requestIdleCallback;
const previous_cancel_idle_callback = globalThis.cancelIdleCallback;
const previous_match_media = globalThis.matchMedia;

try {
    {
        const roots = [{ name: 'first-route' }];
        const doc = install_document({ roots });
        const hydrated = [];
        const disposed = [];

        const dispose = hydrate('counter', (root) => {
            hydrated.push(root.name);
            return () => disposed.push(root.name);
        });

        doc.dispatch('turbo:render');
        assert(
            hydrated.join(',') === 'first-route',
            'Turbo render should not rehydrate a root that is already tracked',
        );

        roots[0].isConnected = false;
        roots.splice(0, 1, { name: 'second-route' });
        doc.dispatch('turbo:render');
        assert(
            hydrated.join(',') === 'first-route,second-route',
            'Turbo render should hydrate a replacement root when its module is cached',
        );

        doc.dispatch('turbo:load');
        assert(
            hydrated.join(',') === 'first-route,second-route',
            'Turbo load should not duplicate hydration after Turbo render',
        );

        const replacement = roots[0];
        roots.splice(0, 1);
        globalThis.__RSJS_DEV__ = true;
        doc.dispatch('turbo:render');
        delete globalThis.__RSJS_DEV__;
        assert(
            hydrated.join(',') === 'first-route,second-route',
            'Turbo replay should silently skip a registered island absent from the new route',
        );
        roots.push(replacement);

        dispose();
        roots.splice(0, 1, { name: 'third-route' });
        doc.dispatch('turbo:render');
        assert(
            hydrated.join(',') === 'first-route,second-route',
            'explicit hydration disposal should unregister Turbo rehydration',
        );
        assert(
            disposed.join(',') === 'first-route,second-route',
            'explicit hydration disposal should clean initial and replacement roots',
        );
    }

    {
        const roots = [{ name: 'first' }, { name: 'second' }];
        install_document({ roots });
        const hydrated = [];
        const disposed = [];

        const dispose = hydrate('counter', (root) => {
            hydrated.push(root.name);
            return () => disposed.push(root.name);
        });

        assert(hydrated.join(',') === 'first,second', 'hydrate should initialize every matching root');
        dispose();
        dispose();
        assert(disposed.join(',') === 'second,first', 'hydrate disposer should dispose roots once in reverse order');
    }

    {
        const roots = [{ name: 'shell-root' }];
        const doc = install_document({ roots });
        const hydrated = [];

        const dispose = hydrate('counter', (root) => {
            hydrated.push(root.name);
        });
        roots.push({ name: 'streamed-boundary-root' });
        doc.dispatch('proa:boundary-ready');
        doc.dispatch('proa:boundary-ready');

        assert(
            hydrated.join(',') === 'shell-root,streamed-boundary-root',
            'an out-of-order boundary should hydrate a new root without re-executing cached island ESM',
        );
        dispose();
    }

    {
        const interaction_root = (name) => {
            const listeners = new Map();
            const root = {
                name,
                addEventListener(event_name, handler, capture) {
                    listeners.set(event_name, { handler, capture });
                },
                removeEventListener(event_name, handler, capture) {
                    const current = listeners.get(event_name);
                    if (
                        current &&
                        current.handler === handler &&
                        current.capture === capture
                    ) {
                        listeners.delete(event_name);
                    }
                },
                listener_count(event_name) {
                    return listeners.has(event_name) ? 1 : 0;
                },
                dispatch(event_name) {
                    listeners
                        .get(event_name)
                        ?.handler({ type: event_name, target: root });
                },
            };
            return root;
        };
        const first = interaction_root('first-interaction-route');
        const roots = [first];
        const doc = install_document({ roots });
        const hydrated = [];

        const dispose = hydrate_on_interaction(
            'counter',
            (root) => hydrated.push(root.name),
            ['click'],
        );
        doc.dispatch('turbo:render');
        assert(
            first.listener_count('click') === 1,
            'Turbo render should not duplicate a pending interaction trigger',
        );

        first.isConnected = false;
        const second = interaction_root('second-interaction-route');
        roots.splice(0, 1, second);
        doc.dispatch('turbo:render');
        assert(
            second.listener_count('click') === 1,
            'Turbo render should schedule a deferred island on its replacement root',
        );
        second.dispatch('click');
        assert(
            hydrated.join(',') === 'second-interaction-route',
            'replacement interaction should hydrate only after its configured event',
        );
        doc.dispatch('turbo:load');
        assert(
            second.listener_count('click') === 0 &&
                hydrated.join(',') === 'second-interaction-route',
            'Turbo load should not reschedule an already hydrated deferred island',
        );
        dispose();
    }

    {
        const legacy_roots = [{ name: 'legacy' }];
        install_document({ legacy_roots });
        const hydrated = [];

        const dispose = hydrate('counter', (root) => {
            hydrated.push(root.name);
        });

        assert(hydrated.join(',') === 'legacy', 'hydrate should still support legacy id-marked roots');
        dispose();
    }

    {
        const doc = install_document({ readyState: 'loading', roots: [{ name: 'late' }] });
        let hydrated = 0;
        const dispose = hydrate('counter', () => {
            hydrated += 1;
        });

        assert(doc.listener_count('DOMContentLoaded') === 1, 'loading hydrate should register one deferred listener');
        dispose();
        assert(doc.listener_count('DOMContentLoaded') === 0, 'deferred disposer should remove the pending listener');
        doc.dispatch('DOMContentLoaded');
        assert(hydrated === 0, 'cancelled deferred hydrate should not run init');
    }

    {
        const doc = install_document({ readyState: 'loading', roots: [{ name: 'late' }] });
        let hydrated = 0;
        hydrate('counter', () => {
            hydrated += 1;
        });

        assert(
            dispose_hydrated('counter') === 1,
            'name cleanup should cancel pending deferred hydration',
        );
        assert(
            doc.listener_count('DOMContentLoaded') === 0,
            'pending cleanup should remove the deferred listener',
        );
        doc.dispatch('DOMContentLoaded');
        assert(
            hydrated === 0,
            'pending cleanup should prevent DOMContentLoaded hydration',
        );
        assert(
            dispose_hydrated('counter') === 0,
            'pending cleanup should be idempotent after cancellation',
        );
    }

    {
        const doc = install_document({ readyState: 'loading', roots: [{ name: 'late' }] });
        const disposed = [];
        const dispose = hydrate('counter', (root) => {
            return () => disposed.push(root.name);
        });

        doc.dispatch('DOMContentLoaded');
        dispose();
        dispose();
        assert(disposed.join(',') === 'late', 'deferred child disposer should run once after hydration');
    }

    {
        const root = { name: 'same' };
        install_document({ roots: [root] });
        const events = [];

        const first = hydrate('counter', () => {
            events.push('init:first');
            return () => events.push('dispose:first');
        });
        const second = hydrate('counter', () => {
            events.push('init:second');
            return () => events.push('dispose:second');
        });

        assert(
            events.join(',') === 'init:first,dispose:first,init:second',
            'rehydrating the same root should dispose the stale tree before installing the new one',
        );
        first();
        assert(
            events.join(',') === 'init:first,dispose:first,init:second',
            'stale disposer should not dispose the newer hydration record',
        );
        second();
        second();
        assert(
            events.join(',') === 'init:first,dispose:first,init:second,dispose:second',
            'new disposer should dispose once',
        );
    }

    {
        const roots = [{ name: 'first' }, { name: 'second' }];
        install_document({ roots });
        const disposed = [];

        hydrate('counter', (root) => {
            return () => disposed.push(root.name);
        });

        assert(dispose_hydrated('counter', roots[0]) === 1, 'targeted cleanup should report one root');
        assert(disposed.join(',') === 'first', 'targeted cleanup should dispose only the requested root');
        assert(dispose_hydrated('counter') === 1, 'name cleanup should dispose the remaining root');
        assert(disposed.join(',') === 'first,second', 'name cleanup should dispose remaining matching roots');
        assert(dispose_hydrated('counter') === 0, 'cleanup should be idempotent after records are gone');
    }

    {
        const root = { name: 'route', isConnected: true };
        install_document({ roots: [root] });
        const observers = [];
        globalThis.MutationObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
                observers.push(this);
            }
            observe(target, options) {
                this.target = target;
                this.options = options;
            }
            disconnect() {
                this.disconnected = true;
            }
            trigger() {
                if (!this.disconnected) {
                    this.callback([]);
                }
            }
        };
        const disposed = [];

        hydrate('counter', (hydrated_root) => {
            assert(hydrated_root === root, 'detached root hydration should pass root');
            return () => disposed.push(root.name);
        });

        assert(observers.length === 1, 'hydrated root should install one detached-root observer');
        assert(observers[0].options.childList, 'detached-root observer should watch child removals');
        assert(observers[0].options.subtree, 'detached-root observer should watch subtree removals');
        root.isConnected = false;
        observers[0].trigger();
        assert(disposed.join(',') === 'route', 'detached root observer should dispose hydrated root');
        assert(observers[0].disconnected, 'detached-root observer should stop after last root is disposed');
        assert(dispose_hydrated('counter', root) === 0, 'detached root cleanup should remove hydration record');
    }

    {
        const roots = [{ name: 'visible' }];
        install_document({ roots });
        const observers = [];
        globalThis.IntersectionObserver = class {
            constructor(callback, options) {
                this.callback = callback;
                this.options = options;
                this.targets = [];
                this.disconnected = false;
                observers.push(this);
            }
            observe(root) {
                this.targets.push(root);
            }
            disconnect() {
                this.disconnected = true;
            }
            trigger(entry) {
                if (!this.disconnected) {
                    this.callback([entry]);
                }
            }
        };
        const hydrated = [];
        const disposed = [];

        hydrate_when_visible(
            'counter',
            (root) => {
                hydrated.push(root.name);
                return () => disposed.push(root.name);
            },
            { rootMargin: '16px' },
        );

        assert(observers.length === 1, 'visible hydration should install one observer');
        assert(observers[0].options.rootMargin === '16px', 'visible hydration should pass observer options through');
        assert(observers[0].targets[0] === roots[0], 'visible hydration should observe the island root');
        observers[0].trigger({ target: roots[0], isIntersecting: true, intersectionRatio: 1 });
        assert(hydrated.join(',') === 'visible', 'visible hydration should run when the root intersects');
        assert(observers[0].disconnected, 'visible hydration should disconnect after hydrating');
        assert(dispose_hydrated('counter', roots[0]) === 1, 'visible root cleanup should dispose hydrated root');
        assert(disposed.join(',') === 'visible', 'visible cleanup should run child disposer');
    }

    {
        const roots = [{ name: 'visible-cancelled' }];
        install_document({ roots });
        const observers = [];
        globalThis.IntersectionObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.targets = [];
                this.disconnected = false;
                observers.push(this);
            }
            observe(root) {
                this.targets.push(root);
            }
            disconnect() {
                this.disconnected = true;
            }
            trigger(entry) {
                if (!this.disconnected) {
                    this.callback([entry]);
                }
            }
        };
        let hydrated = 0;
        hydrate_when_visible('counter', () => {
            hydrated += 1;
        });

        assert(dispose_hydrated('counter', roots[0]) === 1, 'visible pending cleanup should report one root');
        assert(observers[0].disconnected, 'visible pending cleanup should disconnect observer');
        observers[0].trigger({ target: roots[0], isIntersecting: true, intersectionRatio: 1 });
        assert(hydrated === 0, 'visible pending cleanup should prevent later hydration');
    }

    {
        const root = { name: 'visible-replaced' };
        install_document({ roots: [root] });
        const observers = [];
        globalThis.IntersectionObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
                observers.push(this);
            }
            observe(target) {
                this.target = target;
            }
            disconnect() {
                this.disconnected = true;
            }
            trigger(entry) {
                if (!this.disconnected) {
                    this.callback([entry]);
                }
            }
        };
        const hydrated = [];
        const disposed = [];

        hydrate_when_visible('counter', () => {
            hydrated.push('first');
            return () => disposed.push('first');
        });
        hydrate_when_visible('counter', () => {
            hydrated.push('second');
            return () => disposed.push('second');
        });

        assert(observers.length === 2, 'replacement visible hydration should create the new observer');
        assert(observers[0].disconnected, 'replacement visible hydration should cancel stale pending observer');
        observers[0].trigger({ target: root, isIntersecting: true, intersectionRatio: 1 });
        assert(hydrated.length === 0, 'stale pending visible observer should not hydrate');
        observers[1].trigger({ target: root, isIntersecting: true, intersectionRatio: 1 });
        assert(hydrated.join(',') === 'second', 'replacement visible observer should hydrate latest request');
        assert(dispose_hydrated('counter', root) === 1, 'replacement visible cleanup should find one hydrated record');
        assert(disposed.join(',') === 'second', 'replacement visible cleanup should dispose latest request only');
    }

    {
        const root = { name: 'visible-to-immediate' };
        install_document({ roots: [root] });
        const observers = [];
        globalThis.IntersectionObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
                observers.push(this);
            }
            observe(target) {
                this.target = target;
            }
            disconnect() {
                this.disconnected = true;
            }
            trigger(entry) {
                if (!this.disconnected) {
                    this.callback([entry]);
                }
            }
        };
        const events = [];

        hydrate_when_visible('counter', () => {
            events.push('visible');
        });
        const dispose = hydrate('counter', () => {
            events.push('immediate');
            return () => events.push('dispose:immediate');
        });

        assert(observers[0].disconnected, 'immediate hydrate should cancel stale pending visible observer');
        observers[0].trigger({ target: root, isIntersecting: true, intersectionRatio: 1 });
        assert(events.join(',') === 'immediate', 'stale visible observer should not run after immediate hydration');
        dispose();
        assert(events.join(',') === 'immediate,dispose:immediate', 'immediate replacement disposer should still run');
    }

    {
        const root = { name: 'visible-detached', isConnected: true };
        install_document({ roots: [root] });
        const mutation_observers = [];
        globalThis.MutationObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
                mutation_observers.push(this);
            }
            observe() {}
            disconnect() {
                this.disconnected = true;
            }
            trigger() {
                if (!this.disconnected) {
                    this.callback([]);
                }
            }
        };
        const intersection_observers = [];
        globalThis.IntersectionObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
                intersection_observers.push(this);
            }
            observe() {}
            disconnect() {
                this.disconnected = true;
            }
            trigger(entry) {
                if (!this.disconnected) {
                    this.callback([entry]);
                }
            }
        };
        let hydrated = 0;

        hydrate_when_visible('counter', () => {
            hydrated += 1;
        });

        assert(mutation_observers.length === 1, 'pending visible root should install detached-root observer');
        root.isConnected = false;
        mutation_observers[0].trigger();
        assert(intersection_observers[0].disconnected, 'detached pending visible root should disconnect observer');
        intersection_observers[0].trigger({ target: root, isIntersecting: true, intersectionRatio: 1 });
        assert(hydrated === 0, 'detached pending visible root should not hydrate later');
        assert(dispose_hydrated('counter', root) === 0, 'detached pending cleanup should remove pending record');
    }

    {
        const roots = [{ name: 'idle' }];
        install_document({ roots });
        let idle_callback = null;
        let idle_options = null;
        let cancelled_handle = null;
        globalThis.requestIdleCallback = (callback, options) => {
            idle_callback = callback;
            idle_options = options;
            return 42;
        };
        globalThis.cancelIdleCallback = (handle) => {
            cancelled_handle = handle;
        };
        let hydrated = 0;

        hydrate_when_idle(
            'counter',
            () => {
                hydrated += 1;
            },
            { timeout: 50 },
        );

        assert(idle_options.timeout === 50, 'idle hydration should pass timeout through');
        assert(dispose_hydrated('counter', roots[0]) === 1, 'idle pending cleanup should report one root');
        assert(cancelled_handle === 42, 'idle pending cleanup should cancel idle callback');
        idle_callback();
        assert(hydrated === 0, 'idle pending cleanup should prevent later hydration');
    }

    {
        const root = { name: 'idle-detached', isConnected: true };
        install_document({ roots: [root] });
        const mutation_observers = [];
        globalThis.MutationObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
                mutation_observers.push(this);
            }
            observe() {}
            disconnect() {
                this.disconnected = true;
            }
            trigger() {
                if (!this.disconnected) {
                    this.callback([]);
                }
            }
        };
        let idle_callback = null;
        let cancelled_handle = null;
        globalThis.requestIdleCallback = (callback) => {
            idle_callback = callback;
            return 99;
        };
        globalThis.cancelIdleCallback = (handle) => {
            cancelled_handle = handle;
        };
        let hydrated = 0;

        hydrate_when_idle('counter', () => {
            hydrated += 1;
        });

        root.isConnected = false;
        mutation_observers[0].trigger();
        assert(cancelled_handle === 99, 'detached pending idle root should cancel idle callback');
        idle_callback();
        assert(hydrated === 0, 'detached pending idle root should not hydrate later');
        assert(dispose_hydrated('counter', root) === 0, 'detached pending idle cleanup should remove pending record');
    }

    {
        const { root, button } = make_interaction_tree();
        root.name = 'interaction';
        install_document({ roots: [root] });
        const phases = [];
        let handler_calls = 0;
        let handler_event = null;
        let handler_current_target = null;

        const dispose = hydrate_on_interaction(
            'counter',
            (hydrated_root, resources) => {
                assert(hydrated_root === root, 'interaction hydration should pass the island root');
                phases.push('init');
                resources.push(
                    bind_delegated_events(
                        root,
                        'click',
                        new Map([
                            [
                                '0',
                                (event) => {
                                    phases.push('handler');
                                    handler_calls += 1;
                                    handler_event = event;
                                    handler_current_target =
                                        event.__rsjsCurrentTarget;
                                },
                            ],
                        ]),
                    ),
                );
                return () => dispose_all(resources, 'interaction test');
            },
            ['click'],
        );

        assert(
            root.listener_count('click', true) === 1,
            'interaction hydration should install one capture listener',
        );
        const triggering_event = { type: 'click', target: button };
        button.dispatchEvent(triggering_event);
        assert(
            phases.join(',') === 'init,handler',
            `interaction should initialize in capture and handle during the same bubble, got ${phases.join(',')}`,
        );
        assert(
            handler_calls === 1,
            `the first interaction should reach the installed handler exactly once, got ${handler_calls}`,
        );
        assert(
            handler_event === triggering_event,
            'interaction hydration should deliver the original native event without synthetic replay',
        );
        assert(
            handler_current_target === button,
            'same-dispatch interaction should preserve delegated current-target semantics',
        );
        assert(
            root.listener_count('click', true) === 0 &&
                root.listener_count('click', false) === 1,
            'interaction hydration should replace its capture trigger with one delegated bubble listener',
        );
        dispose();
        assert(
            root.listener_count('click') === 0,
            'interaction disposer should clean the hydrated delegated listener',
        );
    }

    {
        const { root, button } = make_interaction_tree();
        root.name = 'interaction-init-failure';
        install_document({ roots: [root] });
        const cleanup_order = [];
        const logged_errors = [];
        const init_error = new Error('binding setup exploded');
        let handler_calls = 0;
        let native_calls = 0;
        root.addEventListener('click', () => {
            native_calls += 1;
        });

        const previous_console_error = console.error;
        console.error = (...args) => {
            logged_errors.push(args);
        };
        let dispose;
        try {
            dispose = hydrate_on_interaction(
                'counter',
                (_hydrated_root, resources) => {
                    resources.push(
                        bind_delegated_events(
                            root,
                            'click',
                            new Map([
                                [
                                    '0',
                                    () => {
                                        handler_calls += 1;
                                    },
                                ],
                            ]),
                        ),
                    );
                    resources.push(() => cleanup_order.push('first'));
                    resources.push(() => cleanup_order.push('second'));
                    throw init_error;
                },
                ['click'],
            );

            button.dispatchEvent({ type: 'click', target: button });
            assert(
                handler_calls === 0,
                'a failed interaction initializer must remove partially bound RSJS handlers before bubble',
            );
            assert(
                native_calls === 1,
                'a failed interaction initializer must preserve native event propagation',
            );
            assert(
                cleanup_order.join(',') === 'second,first',
                `failed initialization should unwind resources in reverse order, got ${cleanup_order.join(',')}`,
            );
            assert(
                root.listener_count('click', true) === 0 &&
                    root.listener_count('click', false) === 1,
                'failed interaction initialization should leave only the pre-existing native listener',
            );
            assert(
                dispose_hydrated('counter', root) === 0,
                'failed interaction initialization must not register a partially hydrated owner',
            );

            button.dispatchEvent({ type: 'click', target: button });
            assert(
                handler_calls === 0 && native_calls === 2,
                'a failed owner should remain inert while later native events continue',
            );
            assert(
                logged_errors.length === 1 &&
                    logged_errors[0][0] ===
                        'rsjs: island "counter" init failed' &&
                    logged_errors[0][1] === init_error,
                'interaction init failure should report the island and init phase with the original error',
            );
        } finally {
            console.error = previous_console_error;
            dispose?.();
        }
    }

    {
        const listeners = new Map();
        const root = {
            name: 'interaction-cancelled',
            addEventListener(name, handler, capture) {
                listeners.set(name, { handler, capture });
            },
            removeEventListener(name, handler, capture) {
                const current = listeners.get(name);
                if (current && current.handler === handler && current.capture === capture) {
                    listeners.delete(name);
                }
            },
            listener_count(name) {
                return listeners.has(name) ? 1 : 0;
            },
            dispatch(name) {
                listeners.get(name)?.handler({ type: name, target: root });
            },
        };
        install_document({ roots: [root] });
        let hydrated = 0;

        hydrate_on_interaction('counter', () => {
            hydrated += 1;
        }, ['click']);

        assert(dispose_hydrated('counter', root) === 1, 'interaction pending cleanup should report one root');
        assert(root.listener_count('click') === 0, 'interaction pending cleanup should remove listener');
        root.dispatch('click');
        assert(hydrated === 0, 'interaction pending cleanup should prevent later hydration');
    }

    {
        const listeners = new Map();
        const root = {
            name: 'interaction-detached',
            isConnected: true,
            addEventListener(name, handler, capture) {
                listeners.set(name, { handler, capture });
            },
            removeEventListener(name, handler, capture) {
                const current = listeners.get(name);
                if (current && current.handler === handler && current.capture === capture) {
                    listeners.delete(name);
                }
            },
            listener_count(name) {
                return listeners.has(name) ? 1 : 0;
            },
            dispatch(name) {
                listeners.get(name)?.handler({ type: name, target: root });
            },
        };
        install_document({ roots: [root] });
        const mutation_observers = [];
        globalThis.MutationObserver = class {
            constructor(callback) {
                this.callback = callback;
                mutation_observers.push(this);
            }
            observe() {}
            disconnect() {}
            trigger() {
                this.callback([]);
            }
        };
        let hydrated = 0;

        hydrate_on_interaction('counter', () => {
            hydrated += 1;
        }, ['click']);

        root.isConnected = false;
        mutation_observers[0].trigger();
        assert(root.listener_count('click') === 0, 'detached pending interaction root should remove listener');
        root.dispatch('click');
        assert(hydrated === 0, 'detached pending interaction root should not hydrate later');
        assert(dispose_hydrated('counter', root) === 0, 'detached pending interaction cleanup should remove pending record');
    }

    {
        const roots = [{ name: 'media' }];
        install_document({ roots });
        const media_lists = [];
        globalThis.matchMedia = (query) => {
            const listeners = new Set();
            const media = {
                matches: false,
                media: query,
                addEventListener(name, handler) {
                    if (name === 'change') listeners.add(handler);
                },
                removeEventListener(name, handler) {
                    if (name === 'change') listeners.delete(handler);
                },
                listener_count() {
                    return listeners.size;
                },
                trigger(matches) {
                    media.matches = matches;
                    for (const handler of [...listeners]) {
                        handler({ matches, media: query });
                    }
                },
            };
            media_lists.push(media);
            return media;
        };
        const disposed = [];

        const dispose = hydrate_when_media('counter', (root) => {
            assert(root === roots[0], 'media hydration should pass the island root');
            return () => disposed.push(root.name);
        }, '(min-width: 900px)');

        assert(media_lists[0].media === '(min-width: 900px)', 'media hydration should pass query to matchMedia');
        assert(media_lists[0].listener_count() === 1, 'media hydration should install change listener');
        media_lists[0].trigger(false);
        assert(disposed.length === 0, 'non-matching media change should not hydrate');
        media_lists[0].trigger(true);
        assert(media_lists[0].listener_count() === 0, 'media hydration should remove listener after match');
        dispose();
        assert(disposed.join(',') === 'media', 'media disposer should clean hydrated root');
    }

    {
        const roots = [{ name: 'media-cancelled' }];
        install_document({ roots });
        const media_lists = [];
        globalThis.matchMedia = () => {
            const listeners = new Set();
            const media = {
                matches: false,
                addEventListener(name, handler) {
                    if (name === 'change') listeners.add(handler);
                },
                removeEventListener(name, handler) {
                    if (name === 'change') listeners.delete(handler);
                },
                listener_count() {
                    return listeners.size;
                },
                trigger(matches) {
                    for (const handler of [...listeners]) {
                        handler({ matches });
                    }
                },
            };
            media_lists.push(media);
            return media;
        };
        let hydrated = 0;

        hydrate_when_media('counter', () => {
            hydrated += 1;
        }, '(pointer: coarse)');

        assert(dispose_hydrated('counter', roots[0]) === 1, 'media pending cleanup should report one root');
        assert(media_lists[0].listener_count() === 0, 'media pending cleanup should remove listener');
        media_lists[0].trigger(true);
        assert(hydrated === 0, 'media pending cleanup should prevent later hydration');
    }

    {
        const root = { name: 'media-detached', isConnected: true };
        install_document({ roots: [root] });
        const mutation_observers = [];
        globalThis.MutationObserver = class {
            constructor(callback) {
                this.callback = callback;
                mutation_observers.push(this);
            }
            observe() {}
            disconnect() {}
            trigger() {
                this.callback([]);
            }
        };
        const media_lists = [];
        globalThis.matchMedia = () => {
            const listeners = new Set();
            const media = {
                matches: false,
                addEventListener(name, handler) {
                    if (name === 'change') listeners.add(handler);
                },
                removeEventListener(name, handler) {
                    if (name === 'change') listeners.delete(handler);
                },
                listener_count() {
                    return listeners.size;
                },
                trigger(matches) {
                    for (const handler of [...listeners]) {
                        handler({ matches });
                    }
                },
            };
            media_lists.push(media);
            return media;
        };
        let hydrated = 0;

        hydrate_when_media('counter', () => {
            hydrated += 1;
        }, '(hover: hover)');

        root.isConnected = false;
        mutation_observers[0].trigger();
        assert(media_lists[0].listener_count() === 0, 'detached pending media root should remove listener');
        media_lists[0].trigger(true);
        assert(hydrated === 0, 'detached pending media root should not hydrate later');
        assert(dispose_hydrated('counter', root) === 0, 'detached pending media cleanup should remove pending record');
    }
} finally {
    if (previous_document === undefined) {
        delete globalThis.document;
    } else {
        globalThis.document = previous_document;
    }
    if (previous_intersection_observer === undefined) {
        delete globalThis.IntersectionObserver;
    } else {
        globalThis.IntersectionObserver = previous_intersection_observer;
    }
    if (previous_mutation_observer === undefined) {
        delete globalThis.MutationObserver;
    } else {
        globalThis.MutationObserver = previous_mutation_observer;
    }
    if (previous_request_idle_callback === undefined) {
        delete globalThis.requestIdleCallback;
    } else {
        globalThis.requestIdleCallback = previous_request_idle_callback;
    }
    if (previous_cancel_idle_callback === undefined) {
        delete globalThis.cancelIdleCallback;
    } else {
        globalThis.cancelIdleCallback = previous_cancel_idle_callback;
    }
    if (previous_match_media === undefined) {
        delete globalThis.matchMedia;
    } else {
        globalThis.matchMedia = previous_match_media;
    }
}

if (failed > 0) {
    process.exit(1);
}

console.log('All hydration lifecycle tests passed.');
