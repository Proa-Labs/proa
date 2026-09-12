// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Linked component conditional-lifecycle regressions. Run with:
//
//     node rsjs/runtime/rsjs-runtime.component-lifecycle.test.mjs

import {
    bind_component_lifecycle,
    bind_conditional,
    bind_input,
    dispose_all,
    signal,
} from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function assert_throws(run, includes, message) {
    try {
        run();
        assert(false, `${message}: expected a throw`);
    } catch (error) {
        assert(
            String(error?.message ?? error).includes(includes),
            `${message}: ${String(error?.message ?? error)}`,
        );
    }
}

function tick() {
    return Promise.resolve();
}

function lifecycle_control(cell, overrides = {}) {
    return {
        get: () => !!cell.get(),
        deps: [cell],
        handles: [],
        ...overrides,
    };
}

function form_control(value, defaultValue) {
    const listeners = new Map();
    return {
        value,
        defaultValue,
        addEventListener(name, listener) {
            if (!listeners.has(name)) listeners.set(name, new Set());
            listeners.get(name).add(listener);
        },
        removeEventListener(name, listener) {
            listeners.get(name)?.delete(listener);
        },
    };
}

// Malformed producer/runtime protocol fails synchronously before owner
// initialization can commit any lifecycle subscription.
{
    const predicate = signal(true);
    const valid = lifecycle_control(predicate);
    const mount = () => () => {};
    assert_throws(
        () => bind_component_lifecycle(null, 'then', mount, 'bad control'),
        'control must be an object',
        'null lifecycle control fails closed',
    );
    assert_throws(
        () =>
            bind_component_lifecycle(
                { ...valid, get: true },
                'then',
                mount,
                'bad getter',
            ),
        'control.get must be a function',
        'non-callable lifecycle getter fails closed',
    );
    assert_throws(
        () =>
            bind_component_lifecycle(
                { ...valid, deps: [null] },
                'then',
                mount,
                'bad dependency',
            ),
        'dependency must be a signal',
        'invalid lifecycle dependency fails closed',
    );
    assert_throws(
        () =>
            bind_component_lifecycle(
                { ...valid, handles: [{}] },
                'then',
                mount,
                'bad handle',
            ),
        'extra handle must be subscribable',
        'invalid lifecycle handle fails closed',
    );
    assert_throws(
        () => bind_component_lifecycle(valid, 'maybe', mount, 'bad branch'),
        'branch must be',
        'unknown lifecycle branch fails closed',
    );
}

// Groups mount in response to the selected branch, dispose on deactivation,
// and remount against the exact retained SSR nodes. Teardown unsubscribes
// before disposing the active group and is idempotent.
{
    const predicate = signal(true);
    const order = [];
    const dep = {
        _subscribe(effect) {
            const unsubscribe = predicate._subscribe(effect);
            return () => {
                order.push('unsubscribe');
                unsubscribe();
            };
        },
    };
    const ssr_nodes = [{ id: 'child' }, { id: 'grandchild' }];
    const original_nodes = [...ssr_nodes];
    let mounts = 0;
    const dispose = bind_component_lifecycle(
        {
            get: () => predicate.get(),
            deps: [dep],
            handles: [],
        },
        'then',
        () => {
            mounts += 1;
            const current = mounts;
            order.push(`mount-${current}`);
            return () => order.push(`dispose-${current}`);
        },
        'remountable group',
    );

    assert(mounts === 1, 'an initially active lifecycle mounts during owner init');
    predicate.set(false);
    await tick();
    assert(
        order.includes('dispose-1'),
        'deactivation disposes the first mounted group',
    );
    predicate.set(true);
    await tick();
    assert(mounts === 2, 'reactivation creates a fresh group mount');
    assert(
        ssr_nodes.length === 2 &&
            ssr_nodes[0] === original_nodes[0] &&
            ssr_nodes[1] === original_nodes[1],
        'lifecycle transitions retain the original SSR DOM',
    );

    dispose();
    dispose();
    assert(
        order.slice(-2).join(',') === 'unsubscribe,dispose-2',
        `teardown unsubscribes before active-group disposal: ${order.join(',')}`,
    );
}

// The first active mount is part of owner initialization. A failure rolls
// back the partial group, unsubscribes, and rethrows instead of registering a
// partial owner.
{
    const predicate = signal(true);
    let unsubscribed = 0;
    let rolled_back = 0;
    const dep = {
        _subscribe(effect) {
            const unsubscribe = predicate._subscribe(effect);
            return () => {
                unsubscribed += 1;
                unsubscribe();
            };
        },
    };
    assert_throws(
        () =>
            bind_component_lifecycle(
                {
                    get: () => predicate.get(),
                    deps: [dep],
                    handles: [],
                },
                'then',
                () => {
                    const resources = [
                        () => {
                            rolled_back += 1;
                        },
                    ];
                    try {
                        throw new Error('first mount failed');
                    } catch (error) {
                        dispose_all(resources, 'test first mount rollback');
                        throw error;
                    }
                },
                'initial failure',
            ),
        'first mount failed',
        'initial lifecycle mount failure aborts owner init',
    );
    assert(
        unsubscribed === 1 && rolled_back === 1,
        'initial mount failure rolls back both subscriptions and partial group resources',
    );
}

// A later mount failure is contained. The generated mount callback owns its
// partial-resource rollback; the binder reports the error, stays inactive,
// and retries after a later false -> true transition.
{
    const predicate = signal(false);
    let should_fail = true;
    let attempts = 0;
    let rollbacks = 0;
    let active_disposals = 0;
    const errors = [];
    const original_error = console.error;
    console.error = (...args) => errors.push(args);
    const dispose = bind_component_lifecycle(
        lifecycle_control(predicate),
        'then',
        () => {
            attempts += 1;
            const resources = [
                () => {
                    rollbacks += 1;
                },
            ];
            try {
                if (should_fail) throw new Error('transient mount failure');
                return () => {
                    active_disposals += 1;
                    dispose_all(resources, 'test active group');
                };
            } catch (error) {
                dispose_all(resources, 'test retry rollback');
                throw error;
            }
        },
        'retryable group',
    );

    predicate.set(true);
    await tick();
    assert(
        attempts === 1 && rollbacks === 1,
        'later failed mount rolls back its partial group once',
    );
    assert(
        errors.some(
            ([message, error]) =>
                message === 'rsjs: retryable group transition failed' &&
                error?.message === 'transient mount failure',
        ),
        'later mount failure is reported under the lifecycle scope',
    );

    should_fail = false;
    predicate.set(false);
    await tick();
    predicate.set(true);
    await tick();
    assert(attempts === 2, 'an inactive failed group retries on a later activation');
    dispose();
    console.error = original_error;
    assert(
        active_disposals === 1 && rollbacks === 2,
        'the successful retry is disposed exactly once',
    );
}

// A later predicate failure deactivates the current group, reports without
// escaping the scheduler, and remains retryable.
{
    const trigger = signal(0);
    let predicate_value = true;
    let predicate_fails = false;
    let mounts = 0;
    let disposals = 0;
    const errors = [];
    const original_error = console.error;
    console.error = (...args) => errors.push(args);
    const dispose = bind_component_lifecycle(
        {
            get() {
                trigger.get();
                if (predicate_fails) throw new Error('predicate failed');
                return predicate_value;
            },
            deps: [trigger],
            handles: [],
        },
        'then',
        () => {
            mounts += 1;
            return () => {
                disposals += 1;
            };
        },
        'predicate group',
    );

    predicate_fails = true;
    trigger.set(1);
    await tick();
    assert(
        disposals === 1 &&
            errors.some(
                ([message, error]) =>
                    message === 'rsjs: predicate group transition failed' &&
                    error?.message === 'predicate failed',
            ),
        'later predicate failure reports and leaves the group inactive',
    );
    predicate_fails = false;
    trigger.set(2);
    await tick();
    assert(mounts === 2, 'a later valid predicate notification remounts the group');
    dispose();
    console.error = original_error;
}

// The parent visibility binding subscribes before the linked lifecycle
// binding in generated modules. A later predicate failure in that first
// subscriber must be contained so the lifecycle subscriber can still
// deactivate the active child group.
{
    const trigger = signal(0);
    const branch = { style: { display: '' } };
    let predicate_fails = false;
    let mounts = 0;
    let disposals = 0;
    const errors = [];
    const original_error = console.error;
    console.error = (...args) => errors.push(args);
    const predicate = () => {
        trigger.get();
        if (predicate_fails) throw new Error('shared predicate failed');
        return true;
    };
    const dispose_visibility = bind_conditional(
        predicate,
        [trigger],
        [branch],
        [],
    );
    const dispose_lifecycle = bind_component_lifecycle(
        {
            get: predicate,
            deps: [trigger],
            handles: [],
        },
        'then',
        () => {
            mounts += 1;
            return () => {
                disposals += 1;
            };
        },
        'shared predicate group',
    );

    predicate_fails = true;
    trigger.set(1);
    await tick();
    assert(
        disposals === 1 &&
            errors.some(
                ([message, error]) =>
                    message ===
                        'rsjs: conditional binding transition failed' &&
                    error?.message === 'shared predicate failed',
            ) &&
            errors.some(
                ([message, error]) =>
                    message ===
                        'rsjs: shared predicate group transition failed' &&
                    error?.message === 'shared predicate failed',
            ),
        'a visibility predicate failure cannot starve lifecycle deactivation',
    );

    predicate_fails = false;
    trigger.set(2);
    await tick();
    assert(
        mounts === 2,
        'the shared visibility/lifecycle predicate remains retryable',
    );
    dispose_lifecycle();
    dispose_visibility();
    console.error = original_error;
}

// Explicit-dependency conditional initialization is transactional: a getter
// failure after subscription cannot leave an effect attached.
{
    let unsubscribe_count = 0;
    const dep = {
        _subscribe(effect) {
            effect.deps.add(dep);
            return () => {
                unsubscribe_count += 1;
                effect.deps.delete(dep);
            };
        },
    };
    const node = { style: { display: '' } };
    assert_throws(
        () =>
            bind_conditional(
                () => {
                    throw new Error('conditional getter failed');
                },
                [dep],
                [node],
                [],
            ),
        'conditional getter failed',
        'conditional first apply rethrows',
    );
    assert(
        unsubscribe_count === 1,
        'conditional first-apply failure rolls back its dependency subscription',
    );
}

// Dirty pre-hydration controls win only on their first owner mount. A
// lifecycle remount receives a fresh SeedOnce value and must not adopt a stale
// retained DOM edit merely because it differs from the immutable SSR default.
{
    const first_node = form_control('typed-before-hydration', 'server');
    const first_cell = signal('server');
    const dispose_first = bind_input(
        first_node,
        'value',
        first_cell,
        'server',
        true,
    );
    assert(
        first_cell.get() === 'typed-before-hydration',
        'first mount preserves a dirty pre-hydration control',
    );
    dispose_first();

    const retained_node = form_control('stale-child-edit', 'server');
    const remounted_cell = signal('fresh-parent-seed');
    const dispose_remount = bind_input(
        retained_node,
        'value',
        remounted_cell,
        'server',
        false,
    );
    assert(
        remounted_cell.get() === 'fresh-parent-seed' &&
            retained_node.value === 'fresh-parent-seed',
        'lifecycle remount ignores stale retained DOM and keeps the fresh SeedOnce value',
    );
    dispose_remount();
}

if (failed > 0) {
    console.error(`${failed} assertion(s) failed`);
    process.exit(1);
}
console.log(
    'rsjs-runtime.component-lifecycle.test.mjs: all assertions passed',
);
