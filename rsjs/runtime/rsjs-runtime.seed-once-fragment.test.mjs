// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Browser-runtime acceptance coverage for linked-component SeedOnce props.
// Run with:
//
//     node rsjs/runtime/rsjs-runtime.seed-once-fragment.test.mjs
//
// The two-compilation Rust fixture proves that `signal(self.initial)` is
// classified as SeedOnce and that the linked owner does not install a live
// signal alias for it. This test executes the corresponding fragment
// initialization shape: the child reads its adjacent SSR seed into a new
// child-owned signal. It intentionally covers a static direct component site;
// dynamic/generic sites remain outside the supported contract.

import {
    bind_event,
    bind_text,
    checked_int_add,
    dispose_all,
    integer_signal,
    read_signal_initials,
    signal_initial_integer,
} from './rsjs-runtime.js';

let failed = 0;
function assert(condition, message) {
    if (!condition) {
        console.error('FAIL:', message);
        failed += 1;
    }
}

function tick() {
    return Promise.resolve();
}

function seed_wrapper(name, initial) {
    const script = {
        tagName: 'SCRIPT',
        textContent: JSON.stringify({ signals: [initial] }),
        getAttribute(attribute) {
            if (attribute === 'type') return 'application/json';
            if (attribute === 'data-rsjs-island') return name;
            return null;
        },
    };
    return {
        nextElementSibling: script,
        getAttribute(attribute) {
            if (attribute === 'data-rsjs-island-root') return name;
            return null;
        },
    };
}

function button() {
    const listeners = new Map();
    return {
        textContent: '',
        addEventListener(name, handler) {
            if (!listeners.has(name)) listeners.set(name, []);
            listeners.get(name).push(handler);
        },
        removeEventListener(name, handler) {
            listeners.set(
                name,
                (listeners.get(name) ?? []).filter((entry) => entry !== handler),
            );
        },
        dispatchEvent(event) {
            event.target ??= this;
            for (const handler of [...(listeners.get(event.type) ?? [])]) {
                handler(event);
            }
        },
    };
}

// This is the exact state-ownership shape emitted for the SeededCounter
// fragment in the linked-site fixture. In particular, `options.signals` is
// reserved for LiveRead aliases. A SeedOnce edge leaves this map empty, so the
// nullish fallback creates a fresh signal from the occurrence's adjacent seed.
function mount_seeded_counter(root, options = {}) {
    const disposers = [];
    const initials = read_signal_initials(options.seedRoot ?? root);
    const signal_overrides = options.signals ?? {};
    const local = signal_overrides[0] ?? integer_signal(
        signal_initial_integer(initials, 0, 0, 'u32'),
        'u32',
    );

    disposers.push(bind_text(root, () => local.get(), [local]));
    disposers.push(bind_event(root, 'click', () => {
        local.update((value) => checked_int_add(value, 1, 'u32'));
    }));

    return () => dispose_all(disposers, 'SeedOnce fragment bindings');
}

const parent = integer_signal(7, 'u32');

const first_root = button();
const dispose_first = mount_seeded_counter(first_root, {
    seedRoot: seed_wrapper('seeded-counter-1', parent.get()),
    signals: Object.create(null),
});

assert(first_root.textContent === '7', 'the first child samples the parent value at mount');

// A control mount uses the same fragment machinery with an explicit LiveRead
// override. Its output must follow the parent, proving the harness would catch
// an accidental SeedOnce-to-LiveRead alias.
const live_control_root = button();
const dispose_live_control = mount_seeded_counter(live_control_root, {
    seedRoot: seed_wrapper('live-control', 0),
    signals: { 0: parent },
});
assert(live_control_root.textContent === '7', 'the LiveRead control observes the parent signal');

first_root.dispatchEvent({ type: 'click' });
await tick();
assert(first_root.textContent === '8', 'the child mutates its independently owned local signal');

parent.set(40);
await tick();
assert(
    first_root.textContent === '8',
    'a later parent write must not reset or alias the mounted child local signal',
);
assert(
    live_control_root.textContent === '40',
    'the LiveRead control must observe the same parent write',
);

dispose_first();

const second_root = button();
const dispose_second = mount_seeded_counter(second_root, {
    seedRoot: seed_wrapper('seeded-counter-2', parent.get()),
    signals: Object.create(null),
});
assert(
    second_root.textContent === '40',
    'a fresh occurrence samples the then-current parent value into fresh local state',
);

parent.set(41);
await tick();
assert(
    second_root.textContent === '40',
    'the fresh occurrence also stops observing the parent after initialization',
);
assert(
    live_control_root.textContent === '41',
    'the control keeps proving parent writes and scheduler flushes are observable',
);

dispose_second();
dispose_live_control();

if (failed > 0) process.exit(1);
console.log('rsjs SeedOnce fragment runtime tests passed');
