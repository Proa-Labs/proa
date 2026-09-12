// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for `bind_delegated_events`. Run with:
//
//     node rsjs/runtime/rsjs-runtime.delegation.test.mjs
//
// The runtime doesn't depend on the DOM at module-load time, so the
// test imports it directly and constructs a small hand-rolled DOM
// shim. The shim implements the surface area `bind_delegated_events`
// touches:
//
//   - addEventListener(name, handler, options)   on the island root
//   - dispatchEvent({ target, type })            to drive a click
//   - target.closest(selector)                   walks up the parent
//                                                chain looking for a
//                                                matching ancestor
//   - root.contains(node)                        ancestor check
//   - element.getAttribute(name)                 marker attr read
//   - element.parentElement / .nodeType          walk fallback
//
// We keep the shim deliberately minimal so the test reads as a clear
// statement of the contract `bind_delegated_events` relies on.

import {
    bind_delegated_events,
    bind_document_event,
    bind_window_event,
} from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

// -------------------------------------------------------------------
// Minimal DOM shim.
// -------------------------------------------------------------------

function make_element(tag, attrs = {}) {
    const node = {
        nodeType: 1, // ELEMENT_NODE
        tagName: tag.toUpperCase(),
        attrs: { ...attrs },
        children: [],
        parentElement: null,
        _listeners: new Map(),
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
            if (!this._listeners.has(name)) this._listeners.set(name, []);
            const capture = options === true || !!options?.capture;
            this._listeners.get(name).push({ handler, capture });
        },
        removeEventListener(name, handler, options) {
            const listeners = this._listeners.get(name);
            if (listeners == null) return;
            const capture = options === true || !!options?.capture;
            this._listeners.set(
                name,
                listeners.filter(
                    (listener) =>
                        listener.handler !== handler ||
                        listener.capture !== capture,
                ),
            );
        },
        dispatchEvent(event) {
            // Climb the parent chain. Capturing listeners fire from
            // root to target, then bubbling listeners fire target to
            // root. This is enough to cover delegated focus/blur,
            // which the runtime registers in capture mode because
            // those events do not bubble.
            const path = [];
            let walker = this;
            while (walker != null) {
                path.push(walker);
                walker = walker.parentElement;
            }
            for (const node of [...path].reverse()) {
                const listeners = node._listeners.get(event.type) ?? [];
                for (const { handler, capture } of listeners) {
                    if (capture) handler(event);
                }
            }
            for (const node of path) {
                const listeners = node._listeners.get(event.type) ?? [];
                for (const { handler, capture } of listeners) {
                    if (!capture) handler(event);
                }
            }
        },
        // closest() returns the nearest ancestor (inclusive) that
        // matches the selector. We only need `[attr-name]` selectors
        // for this test.
        closest(selector) {
            const m = /^\[([a-zA-Z0-9-]+)\]$/.exec(selector);
            if (!m) throw new Error(`unsupported selector: ${selector}`);
            const attr = m[1];
            let walker = this;
            while (walker != null) {
                if (walker.hasAttribute && walker.hasAttribute(attr)) return walker;
                walker = walker.parentElement;
            }
            return null;
        },
        contains(other) {
            let walker = other;
            while (walker != null) {
                if (walker === this) return true;
                walker = walker.parentElement;
            }
            return false;
        },
    };
    return node;
}

// -------------------------------------------------------------------
// Test 1: each handler fires for its own button, exactly once.
// -------------------------------------------------------------------
{
    const root = make_element('rsjs-island');
    const b0 = make_element('button', { 'data-rsjs-handler': '0' });
    const b1 = make_element('button', { 'data-rsjs-handler': '1' });
    const b2 = make_element('button', { 'data-rsjs-handler': '2' });
    root.appendChild(b0);
    root.appendChild(b1);
    root.appendChild(b2);

    let h0_calls = 0;
    let h1_calls = 0;
    let h2_calls = 0;
    let h0_current_target = null;
    bind_delegated_events(
        root,
        'click',
        new Map([
            ['0', (event) => { h0_calls += 1; h0_current_target = event.__rsjsCurrentTarget; }],
            ['1', () => { h1_calls += 1; }],
            ['2', () => { h2_calls += 1; }],
        ]),
    );

    b0.dispatchEvent({ type: 'click', target: b0 });
    assert(h0_calls === 1, `test 1: expected h0 to fire once, got ${h0_calls}`);
    assert(h0_current_target === b0, 'test 1: delegated event should expose the matched handler element as __rsjsCurrentTarget');
    assert(h1_calls === 0, `test 1: h1 must not fire on b0 click, got ${h1_calls}`);
    assert(h2_calls === 0, `test 1: h2 must not fire on b0 click, got ${h2_calls}`);

    b1.dispatchEvent({ type: 'click', target: b1 });
    assert(h0_calls === 1, `test 1: h0 must not refire on b1 click, got ${h0_calls}`);
    assert(h1_calls === 1, `test 1: expected h1 to fire once, got ${h1_calls}`);
    assert(h2_calls === 0, `test 1: h2 must not fire on b1 click, got ${h2_calls}`);

    b2.dispatchEvent({ type: 'click', target: b2 });
    assert(h2_calls === 1, `test 1: expected h2 to fire once, got ${h2_calls}`);

    // Sanity: only one listener was registered on the root despite
    // three handler entries. This is the entire point of the
    // optimisation.
    const click_listeners = root._listeners.get('click') ?? [];
    assert(
        click_listeners.length === 1,
        `test 1: expected exactly 1 click listener on root, got ${click_listeners.length}`,
    );
    assert(
        click_listeners[0].capture === false,
        'test 1: bubbling click listener should not use capture',
    );
}

// -------------------------------------------------------------------
// Test 2: clicking an element that has no `data-rsjs-handler` ancestor
// inside the island root must not fire any handler.
// -------------------------------------------------------------------
{
    const root = make_element('rsjs-island');
    const button = make_element('button', { 'data-rsjs-handler': '0' });
    const decoration = make_element('span'); // sibling, no marker
    root.appendChild(button);
    root.appendChild(decoration);

    let calls = 0;
    bind_delegated_events(
        root,
        'click',
        new Map([['0', () => { calls += 1; }]]),
    );

    decoration.dispatchEvent({ type: 'click', target: decoration });
    assert(calls === 0, `test 2: handler must not fire on a non-handler-bearing element, got ${calls}`);
}

// -------------------------------------------------------------------
// Test 3: a click outside the island root must not fire a handler,
// even if the click target carries a `data-rsjs-handler` attribute
// (e.g. a sibling island instance with the same NodeId numbering).
// -------------------------------------------------------------------
{
    const root_a = make_element('rsjs-island'); // the island we wire
    const root_b = make_element('rsjs-island'); // a sibling island
    const button_in_b = make_element('button', { 'data-rsjs-handler': '0' });
    root_b.appendChild(button_in_b);

    let calls = 0;
    bind_delegated_events(
        root_a,
        'click',
        new Map([['0', () => { calls += 1; }]]),
    );

    // Dispatch on the foreign island's button. `dispatchEvent` only
    // bubbles within `root_b`, which has no listener; even if the
    // listener somehow saw it, `root_a.contains(button_in_b)` is
    // false so the handler is rejected.
    button_in_b.dispatchEvent({ type: 'click', target: button_in_b });
    assert(calls === 0, `test 3: handler must not fire for clicks outside its island root, got ${calls}`);
}

// -------------------------------------------------------------------
// Test 4: bubbling — a click on a nested element inside a
// handler-bearing element must resolve to the parent's handler via
// `closest()`. This is the typical case for an SVG icon inside a
// `<button on_click=...>`.
// -------------------------------------------------------------------
{
    const root = make_element('rsjs-island');
    const button = make_element('button', { 'data-rsjs-handler': '0' });
    const icon = make_element('svg'); // no marker; nested inside button
    root.appendChild(button);
    button.appendChild(icon);

    let calls = 0;
    bind_delegated_events(
        root,
        'click',
        new Map([['0', () => { calls += 1; }]]),
    );

    // event.target is the inner svg, which has no marker; closest()
    // walks up to find the button.
    icon.dispatchEvent({ type: 'click', target: icon });
    assert(calls === 1, `test 4: nested-element click should bubble to parent handler, got ${calls}`);
}

// -------------------------------------------------------------------
// Test 5: a handler that's registered but whose marker doesn't appear
// in the Map is a no-op. (Defensive: keeps a stale `data-rsjs-handler`
// from crashing the runtime.)
// -------------------------------------------------------------------
{
    const root = make_element('rsjs-island');
    const button = make_element('button', { 'data-rsjs-handler': '99' });
    root.appendChild(button);

    let calls = 0;
    bind_delegated_events(
        root,
        'click',
        new Map([['0', () => { calls += 1; }]]),
    );

    // Should not throw, should not fire.
    button.dispatchEvent({ type: 'click', target: button });
    assert(calls === 0, `test 5: unknown marker must be a no-op, got ${calls}`);
}

// -------------------------------------------------------------------
// Test 6: nested component-fragment roots have their own local NodeId
// numbering. A parent island listener must ignore handler markers
// inside `data-rsjs-component` subtrees so colliding marker values do
// not call the wrong parent handler.
// -------------------------------------------------------------------
{
    const parent = make_element('rsjs-island');
    const parent_button = make_element('button', { 'data-rsjs-handler': '0' });
    const component = make_element('div', { 'data-rsjs-component': 'child_1234' });
    const child_button = make_element('button', { 'data-rsjs-handler': '0' });
    parent.appendChild(parent_button);
    parent.appendChild(component);
    component.appendChild(child_button);

    let parent_calls = 0;
    let child_calls = 0;
    bind_delegated_events(
        parent,
        'click',
        new Map([['0', () => { parent_calls += 1; }]]),
    );
    bind_delegated_events(
        component,
        'click',
        new Map([['0', () => { child_calls += 1; }]]),
    );

    child_button.dispatchEvent({ type: 'click', target: child_button });
    assert(child_calls === 1, `test 6: child component handler should fire once, got ${child_calls}`);
    assert(parent_calls === 0, `test 6: parent handler must ignore nested component marker collision, got ${parent_calls}`);

    parent_button.dispatchEvent({ type: 'click', target: parent_button });
    assert(parent_calls === 1, `test 6: parent handler should still fire for parent-owned marker, got ${parent_calls}`);
}

// -------------------------------------------------------------------
// Test 7: an explicit nested island is an independent owner even when
// parent and child reuse the same local handler id. Parent-owned
// markers outside that island still work, and both registrations clean
// up independently.
// -------------------------------------------------------------------
{
    const parent = make_element('rsjs-island');
    const parent_button = make_element('button', { 'data-rsjs-handler': '0' });
    const child = make_element('rsjs-island');
    const child_button = make_element('button', { 'data-rsjs-handler': '0' });
    parent.appendChild(parent_button);
    parent.appendChild(child);
    child.appendChild(child_button);

    let parent_calls = 0;
    let child_calls = 0;
    const dispose_parent = bind_delegated_events(
        parent,
        'click',
        new Map([['0', () => { parent_calls += 1; }]]),
    );
    const dispose_child = bind_delegated_events(
        child,
        'click',
        new Map([['0', () => { child_calls += 1; }]]),
    );

    child_button.dispatchEvent({ type: 'click', target: child_button });
    assert(child_calls === 1, `test 7: child island handler should fire once, got ${child_calls}`);
    assert(parent_calls === 0, `test 7: parent handler must ignore nested island marker collision, got ${parent_calls}`);

    parent_button.dispatchEvent({ type: 'click', target: parent_button });
    assert(parent_calls === 1, `test 7: parent-owned handler should still fire outside nested island, got ${parent_calls}`);

    dispose_child();
    dispose_child();
    child_button.dispatchEvent({ type: 'click', target: child_button });
    assert(child_calls === 1, `test 7: disposed child handler must not fire again, got ${child_calls}`);
    assert(parent_calls === 1, `test 7: disposed child click must not leak to the colliding parent handler, got ${parent_calls}`);

    dispose_parent();
    dispose_parent();
    parent_button.dispatchEvent({ type: 'click', target: parent_button });
    assert(parent_calls === 1, `test 7: disposed parent handler must not fire again, got ${parent_calls}`);
}

// -------------------------------------------------------------------
// Test 8: non-bubbling focus/blur use capture so delegation still works.
// -------------------------------------------------------------------
{
    const root = make_element('rsjs-island');
    const input = make_element('input', { 'data-rsjs-handler': '0' });
    root.appendChild(input);

    let focus_calls = 0;
    bind_delegated_events(
        root,
        'focus',
        new Map([['0', () => { focus_calls += 1; }]]),
    );

    input.dispatchEvent({ type: 'focus', target: input });
    assert(focus_calls === 1, `test 8: delegated focus should fire once through capture, got ${focus_calls}`);

    const focus_listeners = root._listeners.get('focus') ?? [];
    assert(
        focus_listeners.length === 1 && focus_listeners[0].capture === true,
        'test 8: focus listener should be registered with capture=true',
    );
}

// -------------------------------------------------------------------
// Test 9: document-level listeners expose Document as the current target while
// retaining the source element as the compiler-owned payload target.
// -------------------------------------------------------------------
{
    const owner = make_element('div', { 'data-rsjs-payload': 'panel' });
    const doc = make_element('#document');
    owner.ownerDocument = doc;

    let calls = 0;
    let current_target = null;
    let owner_target = null;
    let payload = null;
    bind_document_event(owner, 'pointerdown', (event) => {
        calls += 1;
        current_target = event.__rsjsCurrentTarget;
        owner_target = event.__rsjsOwnerTarget;
        payload = event.__rsjsOwnerTarget?.getAttribute('data-rsjs-payload');
    });

    const event = { type: 'pointerdown', target: doc };
    doc.dispatchEvent(event);
    assert(calls === 1, `test 9: document listener should fire once, got ${calls}`);
    assert(current_target === doc, 'test 9: document listener should expose Document as __rsjsCurrentTarget');
    assert(owner_target === owner, 'test 9: document listener should retain owner as __rsjsOwnerTarget');
    assert(payload === 'panel', `test 9: document listener should decode owner payload, got ${payload}`);
    assert(event.__rsjsCurrentTarget === undefined, 'test 9: current target should be restored after dispatch');
    assert(event.__rsjsOwnerTarget === undefined, 'test 9: owner target should be restored after dispatch');
}

// -------------------------------------------------------------------
// Test 10: window-level listeners expose Window and retain their payload owner.
// -------------------------------------------------------------------
{
    const owner = make_element('section', { 'data-rsjs-payload': 'viewport' });
    const doc = make_element('#document');
    const win = make_element('#window');
    doc.defaultView = win;
    owner.ownerDocument = doc;

    let current_target = null;
    let owner_target = null;
    let payload = null;
    bind_window_event(owner, 'resize', (event) => {
        current_target = event.__rsjsCurrentTarget;
        owner_target = event.__rsjsOwnerTarget;
        payload = event.__rsjsOwnerTarget?.getAttribute('data-rsjs-payload');
    });

    const event = { type: 'resize', target: win };
    win.dispatchEvent(event);
    assert(current_target === win, 'test 10: window listener should expose Window as __rsjsCurrentTarget');
    assert(owner_target === owner, 'test 10: window listener should retain owner as __rsjsOwnerTarget');
    assert(payload === 'viewport', `test 10: window listener should decode owner payload, got ${payload}`);
    assert(event.__rsjsCurrentTarget === undefined, 'test 10: current target should be restored after dispatch');
    assert(event.__rsjsOwnerTarget === undefined, 'test 10: owner target should be restored after dispatch');
}

if (failed > 0) {
    console.error(`\n${failed} test(s) failed.`);
    process.exit(1);
}
console.log('All delegation tests passed.');
