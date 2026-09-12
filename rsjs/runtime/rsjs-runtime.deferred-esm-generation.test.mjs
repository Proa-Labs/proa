// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Generation fencing and activation atomicity for managed island ESM.
// Run with:
//
//     node rsjs/runtime/rsjs-runtime.deferred-esm-generation.test.mjs

import {
    RsjsActivationError,
    defer_island_module,
    dispose_hydrated,
    hydrate,
    hydrate_from_module,
    hydrate_on_interaction,
    hydrate_when_idle,
    hydrate_when_media,
    hydrate_when_visible,
    load_island_module,
} from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((resolve_promise, reject_promise) => {
        resolve = resolve_promise;
        reject = reject_promise;
    });
    return { promise, resolve, reject };
}

async function settle() {
    await Promise.resolve();
    await Promise.resolve();
}

function make_root(asset_id, owner_id = `${asset_id}:owner`) {
    const listeners = new Map();
    return {
        asset_id,
        owner_id,
        isConnected: true,
        parentElement: null,
        getAttribute(name) {
            if (name === 'data-rsjs-island-root') return asset_id;
            if (name === 'data-rsjs-owner-id') return owner_id;
            return null;
        },
        hasAttribute(name) {
            return (
                name === 'data-rsjs-island-root' ||
                name === 'data-rsjs-owner-id'
            );
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
                        entry.handler !== handler ||
                        entry.capture !== capture,
                ),
            );
        },
        listener_count(name, capture) {
            const entries = listeners.get(name) ?? [];
            return capture === undefined
                ? entries.length
                : entries.filter((entry) => entry.capture === capture).length;
        },
        dispatch(name) {
            const event = {
                type: name,
                target: this,
                defaultPrevented: false,
                preventDefault() {
                    this.defaultPrevented = true;
                },
            };
            for (const entry of [...(listeners.get(name) ?? [])]) {
                if (entry.capture) entry.handler(event);
            }
            for (const entry of [...(listeners.get(name) ?? [])]) {
                if (!entry.capture) entry.handler(event);
            }
            return event;
        },
    };
}

const roots = new Map();
const document_listeners = new Map();
const previous_document = globalThis.document;
const previous_console_error = console.error;
const protocol_errors = [];

globalThis.document = {
    readyState: 'complete',
    body: make_root('body', 'document-body'),
    documentElement: {},
    querySelectorAll(selector) {
        const current = /data-rsjs-island-root="([^"]+)"/.exec(selector);
        if (current != null) return roots.get(current[1]) ?? [];
        const legacy = /rsjs-island\[id="([^"]+)"\]/.exec(selector);
        if (legacy != null) return [];
        return [];
    },
    addEventListener(name, handler) {
        if (!document_listeners.has(name)) document_listeners.set(name, []);
        document_listeners.get(name).push(handler);
    },
    removeEventListener(name, handler) {
        document_listeners.set(
            name,
            (document_listeners.get(name) ?? []).filter(
                (candidate) => candidate !== handler,
            ),
        );
    },
};
console.error = (...args) => {
    if (
        args[1] instanceof RsjsActivationError ||
        String(args[0]).startsWith('rsjs: owner ')
    ) {
        protocol_errors.push(args);
        return;
    }
    previous_console_error(...args);
};

try {
    // Every deferred strategy reaches the same managed-import authorization
    // point. A module that registered before teardown but resolves afterward
    // must not initialize the replacement root. Interaction additionally must
    // not arm a capture listener.
    const stale_cases = [
        {
            name: 'visible-owner',
            strategy: hydrate_when_visible,
            args: [],
            triggered: true,
        },
        {
            name: 'idle-owner',
            strategy: hydrate_when_idle,
            args: [],
            triggered: true,
        },
        {
            name: 'media-owner',
            strategy: hydrate_when_media,
            args: ['(min-width: 40rem)'],
            triggered: true,
        },
        {
            name: 'interaction-owner',
            strategy: hydrate_on_interaction,
            args: [['pointerdown']],
            triggered: false,
        },
    ];

    for (const test_case of stale_cases) {
        const old_root = make_root(test_case.name, `${test_case.name}:old`);
        const new_root = make_root(test_case.name, `${test_case.name}:new`);
        roots.set(test_case.name, [old_root]);
        const imported = deferred();
        let initialized = 0;
        let requested_url = null;
        load_island_module(
            test_case.name,
            old_root,
            `./${test_case.name}.js${
                test_case.triggered ? '?rsjs-triggered=1' : ''
            }`,
            (url) => {
                requested_url = url;
                hydrate_from_module(url, test_case.strategy)(
                    test_case.name,
                    () => {
                        initialized += 1;
                    },
                    ...test_case.args,
                );
                return imported.promise;
            },
        );
        assert(
            initialized === 0,
            `${test_case.name} registration is inert until import success`,
        );

        dispose_hydrated();
        old_root.isConnected = false;
        roots.set(test_case.name, [new_root]);
        imported.resolve({ requested_url });
        await settle();

        assert(
            initialized === 0,
            `${test_case.name} late import cannot initialize the replacement generation`,
        );
        assert(
            new_root.listener_count('pointerdown', true) === 0,
            `${test_case.name} late import cannot arm replacement-page interaction listeners`,
        );
    }

    // Native import promises cannot be cancelled and are allowed to remain
    // pending forever. Page teardown must still delete their authorization
    // and release the detached owner instead of waiting for settlement.
    {
        const name = 'never-settling-import';
        const root = make_root(name);
        roots.set(name, [root]);
        let requested_url = null;
        load_island_module(name, root, './never-settles.js', (url) => {
            requested_url = url;
            return new Promise(() => {});
        });

        const registry =
            globalThis[Symbol.for('rsjs.managed-module-registry.v1')];
        const load_id = new URL(requested_url).searchParams.get('rsjs-load');
        const ticket = registry.loads_by_id.get(load_id);
        assert(
            ticket?.root === root && registry.loads_by_key.get(ticket.key) === ticket,
            'a pending managed import initially retains exactly one authorized owner ticket',
        );

        dispose_hydrated();
        assert(
            !registry.loads_by_id.has(load_id) &&
                !registry.loads_by_key.has(ticket.key),
            'teardown immediately removes authorization for a never-settling import',
        );
        assert(
            ticket.root === null && ticket.activation_start === null,
            'teardown releases the detached root and activation closure without awaiting import settlement',
        );

        let initialized = 0;
        hydrate_from_module(requested_url, hydrate)(name, () => {
            initialized += 1;
        });
        assert(
            initialized === 0,
            'late evaluation of a cancelled managed URL remains inert after its ticket is deleted',
        );
    }

    // The trigger registration itself belongs to the page generation. An old
    // visible gate that never started an import must not rearm on replacement
    // DOM and thereby authorize its old module URL in the new generation.
    {
        const name = 'untriggered-visible-owner';
        const old_root = make_root(name, `${name}:old`);
        const new_root = make_root(name, `${name}:new`);
        roots.set(name, [old_root]);
        const previous_intersection_observer = globalThis.IntersectionObserver;
        const observations = [];
        globalThis.IntersectionObserver = class {
            constructor(callback) {
                this.callback = callback;
                this.disconnected = false;
            }
            observe(root) {
                observations.push({ root, observer: this });
            }
            disconnect() {
                this.disconnected = true;
            }
        };
        try {
            defer_island_module(
                name,
                './old-visible.js?rsjs-triggered=1',
                hydrate_when_visible,
            );
            assert(
                observations.some((entry) => entry.root === old_root),
                'the old page arms its visible module gate',
            );

            dispose_hydrated();
            old_root.isConnected = false;
            roots.set(name, [new_root]);
            for (const handler of document_listeners.get('turbo:load') ?? []) {
                handler();
            }
            await settle();

            assert(
                !observations.some((entry) => entry.root === new_root),
                'an untriggered stale gate cannot rearm on replacement DOM',
            );
        } finally {
            globalThis.IntersectionObserver = previous_intersection_observer;
            dispose_hydrated();
        }
    }

    // A visible trigger is an owner-instance event. Importing the module for
    // one root must not eagerly initialize an offscreen same-asset sibling,
    // and the sibling must retain independent ticket authorization.
    {
        const name = 'repeated-visible-owner';
        const first_root = make_root(name, `${name}:first`);
        const second_root = make_root(name, `${name}:second`);
        roots.set(name, [first_root, second_root]);
        let first_initialized = 0;
        let second_initialized = 0;
        const start_for_root = (root) => {
            load_island_module(
                name,
                root,
                './repeated-visible.js?rsjs-triggered=1',
                (url) => {
                    hydrate_from_module(url, hydrate_when_visible)(
                        name,
                        (owner) => {
                            if (owner === first_root) first_initialized += 1;
                            if (owner === second_root) second_initialized += 1;
                        },
                    );
                    return Promise.resolve();
                },
            );
        };

        start_for_root(first_root);
        await settle();
        assert(
            first_initialized === 1 && second_initialized === 0,
            'one visible root initializes without activating its offscreen sibling',
        );

        start_for_root(second_root);
        await settle();
        assert(
            first_initialized === 1 && second_initialized === 1,
            'the sibling retains an independent root-scoped import ticket',
        );
        dispose_hydrated();
    }

    // Tickets authorize one exact URL. Reusing the ticket query on another
    // module path is a dependency protocol error and never initializes.
    {
        const name = 'url-mismatch';
        const root = make_root(name);
        roots.set(name, [root]);
        const imported = deferred();
        let initialized = 0;
        let requested_url;
        load_island_module(name, root, './expected-module.js', (url) => {
            requested_url = url;
            const wrong = new URL(url);
            wrong.pathname = '/wrong-module.js';
            hydrate_from_module(wrong.href, hydrate)(name, () => {
                initialized += 1;
            });
            return imported.promise;
        });
        imported.resolve();
        await settle();

        assert(initialized === 0, 'a URL-mismatched ticket leaves SSR inert');
        const mismatch = protocol_errors.at(-1)?.[1];
        assert(
            mismatch instanceof RsjsActivationError &&
                mismatch.phase === 'dependency' &&
                mismatch.asset_id === name &&
                mismatch.owner_id === root.owner_id,
            'URL mismatch reports scoped dependency context',
        );
        assert(
            new URL(requested_url).searchParams.has('rsjs-load'),
            'managed loader appends its single-use ticket to the exact module URL',
        );
        dispose_hydrated();
    }

    // Even if module evaluation reached inert registration before a dependency
    // rejection, the import failure cannot commit a partial owner.
    {
        const name = 'failed-import';
        const root = make_root(name, 'failed-import:instance-7');
        roots.set(name, [root]);
        let initialized = 0;
        let ticket;
        const dispose_load = load_island_module(name, root, './failed-import.js', (url) => {
            const registry =
                globalThis[Symbol.for('rsjs.managed-module-registry.v1')];
            ticket = registry.loads_by_id.get(
                new URL(url).searchParams.get('rsjs-load'),
            );
            hydrate_from_module(url, hydrate)(name, () => {
                initialized += 1;
                root.addEventListener('click', () => {});
            });
            return Promise.reject(new Error('dependency evaluation failed'));
        });
        await settle();

        assert(
            initialized === 0 && root.listener_count('click') === 0,
            'failed import leaves the owner inert without partial bindings',
        );
        const failure = protocol_errors.at(-1)?.[1];
        assert(
            failure instanceof RsjsActivationError &&
                failure.code === 'rsjs/owner-activation-failed' &&
                failure.phase === 'import' &&
                Number.isSafeInteger(failure.response_epoch) &&
                Number.isSafeInteger(failure.navigation_generation) &&
                failure.owner_id === 'failed-import:instance-7' &&
                failure.asset_id === name,
            'failed import reports response/generation/owner/asset/phase context',
        );
        assert(
            ticket.status === 'failed' &&
                ticket.root === null &&
                ticket.registration === null &&
                ticket.activation_start === null,
            'a failed terminal ticket keeps only retry-fence metadata, not its detached owner resources',
        );
        dispose_load();
        assert(
            ticket.root === null,
            'subtree/load disposal cannot re-retain a failed ticket owner',
        );
        dispose_hydrated();
    }

    // Initializer failures are owner-protocol failures, not successful module
    // activation and not an unstructured console-only island error.
    {
        const name = 'failed-managed-init';
        const root = make_root(name, `${name}:instance-3`);
        roots.set(name, [root]);
        const init_error = new Error('managed initializer failed');
        let rolled_back = 0;
        load_island_module(name, root, './failed-managed-init.js', (url) => {
            hydrate_from_module(url, hydrate)(
                name,
                (_owner, resources) => {
                    resources.push(() => {
                        rolled_back += 1;
                    });
                    throw init_error;
                },
            );
            return Promise.resolve();
        });
        await settle();

        assert(
            rolled_back === 1 && dispose_hydrated(name, root) === 0,
            'managed init failure rolls back partial resources and leaves no hydrated owner',
        );
        const failure = protocol_errors.at(-1)?.[1];
        assert(
            failure instanceof RsjsActivationError &&
                failure.code === 'rsjs/owner-activation-failed' &&
                failure.phase === 'init' &&
                failure.response_epoch >= 0 &&
                failure.navigation_generation >= 0 &&
                failure.owner_id === `${name}:instance-3` &&
                failure.asset_id === name &&
                failure.cause === init_error,
            'managed init failure reports complete generation/owner/asset/phase context',
        );
        dispose_hydrated();
    }

    // A successful ticket is consumed before activation. Replaying the same
    // import URL cannot replace or invoke the committed initializer.
    {
        const name = 'single-use-ticket';
        const root = make_root(name);
        roots.set(name, [root]);
        const imported = deferred();
        let requested_url;
        let initialized = 0;
        let replayed = 0;
        let ticket;
        const dispose_load = load_island_module(name, root, './single-use.js', (url) => {
            requested_url = url;
            const registry =
                globalThis[Symbol.for('rsjs.managed-module-registry.v1')];
            ticket = registry.loads_by_id.get(
                new URL(url).searchParams.get('rsjs-load'),
            );
            hydrate_from_module(url, hydrate)(name, () => {
                initialized += 1;
            });
            return imported.promise;
        });
        imported.resolve();
        await settle();
        hydrate_from_module(requested_url, hydrate)(name, () => {
            replayed += 1;
        });

        assert(
            initialized === 1 && replayed === 0,
            'a managed module ticket authorizes exactly one initializer registration',
        );
        assert(
            ticket.status === 'activated' &&
                ticket.root === null &&
                ticket.registration === null &&
                ticket.activation_start === null,
            'an activated terminal ticket keeps only retry-fence metadata, not its detached owner resources',
        );
        dispose_load();
        assert(
            ticket.root === null,
            'subtree/load disposal cannot re-retain an activated ticket owner',
        );
        dispose_hydrated();
    }

    // Resolved framework URLs and canonical generated imports can evaluate the
    // same runtime bytes as separate ESM instances. Registration crosses the
    // shared ticket singleton, but the ticket-owning framework runtime remains
    // authoritative for strategy/lifecycle state and page teardown.
    {
        const page_runtime_symbol = Symbol.for('rsjs.page-runtime');
        const framework_page_runtime = globalThis[page_runtime_symbol];
        const alias_runtime = await import(
            './rsjs-runtime.js?managed-registry-alias-test=1'
        );
        assert(
            globalThis[page_runtime_symbol] === framework_page_runtime,
            'a runtime alias cannot replace the framework lifecycle bridge',
        );

        const name = 'runtime-alias-interaction-owner';
        const root = make_root(name);
        roots.set(name, [root]);
        let initialized = 0;
        let cleaned = 0;
        load_island_module(
            name,
            null,
            './runtime-alias-interaction-owner.js',
            (url) => {
                alias_runtime.hydrate_from_module(
                    url,
                    alias_runtime.hydrate_on_interaction,
                )(
                    name,
                    () => {
                        initialized += 1;
                        return () => {
                            cleaned += 1;
                        };
                    },
                    ['pointerdown'],
                );
                return Promise.resolve();
            },
            hydrate_on_interaction,
            [['pointerdown']],
        );
        await settle();

        assert(
            root.listener_count('pointerdown', true) === 1,
            'an alias module registers through the framework-owned interaction strategy',
        );
        root.dispatch('pointerdown');
        assert(initialized === 1, 'the alias initializer activates once');
        dispose_hydrated();
        assert(
            cleaned === 1 && root.listener_count('pointerdown') === 0,
            'framework-runtime teardown owns and disposes alias-module initialization',
        );

        // Page generation is shared across runtime aliases. Teardown through
        // either URL invalidates a ticket created by the other.
        const late_name = `${name}-late`;
        const late_root = make_root(late_name);
        roots.set(late_name, [late_root]);
        const imported = deferred();
        load_island_module(
            late_name,
            null,
            './runtime-alias-late.js',
            (url) => {
                alias_runtime.hydrate_from_module(
                    url,
                    alias_runtime.hydrate_on_interaction,
                )(late_name, () => {}, ['pointerdown']);
                return imported.promise;
            },
            hydrate_on_interaction,
            [['pointerdown']],
        );
        alias_runtime.dispose_hydrated();
        imported.resolve();
        await settle();
        assert(
            late_root.listener_count('pointerdown', true) === 0,
            'page teardown through an alias invalidates framework-runtime tickets',
        );

        // The evaluated module cannot silently change the ticket-owner
        // strategy.
        const mismatch_name = `${name}-strategy-mismatch`;
        const mismatch_root = make_root(mismatch_name);
        roots.set(mismatch_name, [mismatch_root]);
        load_island_module(
            mismatch_name,
            null,
            './runtime-alias-mismatch.js',
            (url) => {
                alias_runtime.hydrate_from_module(
                    url,
                    alias_runtime.hydrate_when_visible,
                )(mismatch_name, () => {});
                return Promise.resolve();
            },
            hydrate_on_interaction,
            [['pointerdown']],
        );
        await settle();
        const mismatch = protocol_errors.at(-1)?.[1];
        assert(
            mismatch?.code === 'rsjs/owner-activation-failed' &&
                mismatch.phase === 'dependency' &&
                mismatch.asset_id === mismatch_name &&
                mismatch_root.listener_count('pointerdown') === 0,
            'an alias strategy-kind mismatch fails closed before activation',
        );
        dispose_hydrated();
    }

    // Interaction ESM is fetched first. No trigger listener exists while the
    // import is pending; after successful authorization, the first supported
    // native event initializes synchronously and reaches the installed bubble
    // handler in that same dispatch exactly once.
    {
        const name = 'managed-interaction';
        const root = make_root(name);
        roots.set(name, [root]);
        const imported = deferred();
        let initialized = 0;
        let delivered = 0;
        load_island_module(
            name,
            null,
            './managed-interaction.js',
            (url) => {
                hydrate_from_module(url, hydrate_on_interaction)(
                    name,
                    (owner) => {
                        initialized += 1;
                        const handler = () => {
                            delivered += 1;
                        };
                        owner.addEventListener('pointerdown', handler);
                        return () =>
                            owner.removeEventListener('pointerdown', handler);
                    },
                    ['pointerdown'],
                );
                return imported.promise;
            },
            hydrate_on_interaction,
            [['pointerdown']],
        );

        const early = root.dispatch('pointerdown');
        assert(
            initialized === 0 &&
                delivered === 0 &&
                early.defaultPrevented === false,
            'interaction before module readiness keeps native behavior and does not only hydrate',
        );
        assert(
            root.listener_count('pointerdown', true) === 0,
            'interaction capture listener is not armed before factory readiness',
        );

        imported.resolve();
        await settle();
        assert(
            root.listener_count('pointerdown', true) === 1,
            'successful managed import arms the interaction trigger',
        );
        const triggering = root.dispatch('pointerdown');
        assert(
            initialized === 1 &&
                delivered === 1 &&
                triggering.defaultPrevented === false,
            'first armed interaction initializes and reaches the real handler exactly once',
        );
        assert(
            root.listener_count('pointerdown', true) === 0,
            'successful first interaction removes its capture trigger',
        );
        dispose_hydrated();
    }
} finally {
    dispose_hydrated();
    console.error = previous_console_error;
    globalThis.document = previous_document;
}

if (failed > 0) {
    console.error(`\n${failed} deferred ESM generation assertion(s) failed`);
    process.exit(1);
}

console.log(
    'rsjs-runtime.deferred-esm-generation.test.mjs: all assertions passed',
);
