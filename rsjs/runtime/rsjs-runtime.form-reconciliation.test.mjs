// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Dirty form-control reconciliation before deferred owner activation.
//
// Run with:
//
//     node rsjs/runtime/rsjs-runtime.form-reconciliation.test.mjs

let failed = 0;

function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function control(initial, defaults = {}) {
    const listeners = new Map();
    let value = initial;
    let writes = 0;
    const node = {
        defaultValue: defaults.defaultValue ?? initial,
        checked: defaults.checked ?? false,
        defaultChecked: defaults.defaultChecked ?? defaults.checked ?? false,
        selected: defaults.selected ?? false,
        defaultSelected: defaults.defaultSelected ?? defaults.selected ?? false,
        textContent: defaults.textContent ?? '',
        options: defaults.options,
        addEventListener(name, listener) {
            let set = listeners.get(name);
            if (set === undefined) {
                set = new Set();
                listeners.set(name, set);
            }
            set.add(listener);
        },
        removeEventListener(name, listener) {
            listeners.get(name)?.delete(listener);
        },
        getAttribute(name) {
            return name === 'data-rsjs-ssr-text'
                ? (defaults.ssrText ?? null)
                : null;
        },
        listenerCount() {
            let count = 0;
            for (const set of listeners.values()) count += set.size;
            return count;
        },
        writes() {
            return writes;
        },
    };
    Object.defineProperty(node, 'value', {
        get() {
            return value;
        },
        set(next) {
            writes += 1;
            value = String(next);
        },
        enumerable: true,
    });
    return node;
}

const root = {};
globalThis.document = {
    readyState: 'complete',
    body: root,
    documentElement: root,
    addEventListener() {},
    removeEventListener() {},
    querySelectorAll(selector) {
        return selector.includes('FormOwner') ? [root] : [];
    },
};

const {
    bind_input,
    dispose_hydrated,
    hydrate,
    signal,
} = await import('./rsjs-runtime.js');

{
    const node = control('Andy', { defaultValue: 'Andrew' });
    const value = signal('Andrew');
    const dispose = bind_input(node, 'value', value);
    assert(value.peek() === 'Andy', 'dirty input value is adopted into the signal');
    assert(node.value === 'Andy', 'dirty input value is not restored to the SSR seed');
    assert(node.writes() === 0, 'dirty matching input does not receive a no-op write');
    dispose();
}

{
    const node = control('Andrew', { defaultValue: 'Andrew' });
    const value = signal('Amy');
    const dispose = bind_input(node, 'value', value);
    assert(value.peek() === 'Amy', 'untouched input keeps the current cell value');
    assert(node.value === 'Amy', 'current cell value wins for an untouched input');
    dispose();
}

{
    const node = control('', {
        checked: true,
        defaultChecked: false,
    });
    const value = signal(false);
    const dispose = bind_input(node, 'checked', value);
    assert(value.peek() === true, 'dirty checked state is adopted');
    assert(node.checked === true, 'dirty checkbox is not reset');
    dispose();
}

{
    const options = [
        { selected: false, defaultSelected: true },
        { selected: true, defaultSelected: false },
    ];
    const node = control('b', { defaultValue: 'a', options });
    const value = signal('a');
    const dispose = bind_input(node, 'value', value);
    assert(value.peek() === 'b', 'dirty select option state is adopted');
    dispose();
}

{
    const node = control('', {
        textContent: 'edited',
        ssrText: 'server',
    });
    const value = signal('server');
    const dispose = bind_input(node, 'textContent', value);
    assert(value.peek() === 'edited', 'dirty contenteditable text is adopted');
    assert(node.textContent === 'edited', 'dirty contenteditable text is preserved');
    dispose();
}

{
    const edited = control('typed', { defaultValue: 'server' });
    const untouched = control('server', { defaultValue: 'server' });
    const shared = signal('server');
    let first_binding_deferred = false;

    const unregister = hydrate('FormOwner', (_root, resources) => {
        resources.push(bind_input(edited, 'value', shared));
        first_binding_deferred = untouched.value === 'server';
        resources.push(bind_input(untouched, 'value', shared));
    });

    assert(first_binding_deferred, 'owner defers first DOM commits until every control reconciles');
    assert(shared.peek() === 'typed', 'owner reconciliation adopts dirty state synchronously');
    assert(edited.value === 'typed', 'owner preserves the edited control');
    assert(edited.writes() === 0, 'owner performs no no-op write to the edited control');
    assert(untouched.value === 'typed', 'owner updates untouched controls from final state');
    assert(edited.listenerCount() === 2, 'owner installed edit listeners');

    unregister();
    assert(edited.listenerCount() === 0, 'owner disposal removes edited-control listeners');
    assert(untouched.listenerCount() === 0, 'owner disposal removes untouched-control listeners');
}

dispose_hydrated();

if (failed > 0) {
    process.exit(1);
}

console.log('All form reconciliation tests passed.');
