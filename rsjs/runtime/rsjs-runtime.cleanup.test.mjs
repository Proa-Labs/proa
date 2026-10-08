// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Tests for runtime binding cleanup. Run with:
//
//     node rsjs/runtime/rsjs-runtime.cleanup.test.mjs

import {
    bind_attr,
    bind_conditional,
    bind_delegated_events,
    bind_document_event,
    bind_event,
    bind_input,
    bind_submit,
    bind_text,
    bind_window_event,
    memo,
    signal,
} from './rsjs-runtime.js';

let failed = 0;
function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        failed += 1;
    }
}

function tick() {
    return Promise.resolve();
}

function make_event_target() {
    return {
        _listeners: new Map(),
        addEventListener(name, handler, options) {
            if (!this._listeners.has(name)) this._listeners.set(name, []);
            this._listeners.get(name).push({ handler, options });
        },
        removeEventListener(name, handler) {
            const current = this._listeners.get(name) ?? [];
            this._listeners.set(
                name,
                current.filter((entry) => entry.handler !== handler),
            );
        },
        dispatchEvent(event) {
            event.target ??= this;
            const listeners = [...(this._listeners.get(event.type) ?? [])];
            for (const { handler } of listeners) {
                handler(event);
            }
        },
        listener_count(name) {
            return (this._listeners.get(name) ?? []).length;
        },
    };
}

function make_element(tag = 'div', attrs = {}) {
    return {
        ...make_event_target(),
        nodeType: 1,
        tagName: tag.toUpperCase(),
        attrs: { ...attrs },
        children: [],
        parentElement: null,
        ownerDocument: null,
        style: { display: '' },
        textContent: '',
        value: '',
        checked: false,
        setAttribute(name, value) {
            this.attrs[name] = String(value);
        },
        removeAttribute(name) {
            delete this.attrs[name];
        },
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
            child.ownerDocument ??= this.ownerDocument;
            this.children.push(child);
            return child;
        },
        closest(selector) {
            const m = /^\[([a-zA-Z0-9-]+)\]$/.exec(selector);
            if (!m) throw new Error(`unsupported selector: ${selector}`);
            const attr = m[1];
            let walker = this;
            while (walker != null) {
                if (walker.hasAttribute?.(attr)) return walker;
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
}

function make_counted_cell(initial) {
    let value = initial;
    const subscribers = new Set();
    const cell = {
        get() {
            return value;
        },
        peek() {
            return value;
        },
        set(next) {
            value = next;
        },
        _subscribe(effect) {
            subscribers.add(effect);
            effect.deps.add(cell);
            return () => cell._unsubscribe(effect);
        },
        _unsubscribe(effect) {
            subscribers.delete(effect);
            effect.deps.delete(cell);
        },
        subscriber_count() {
            return subscribers.size;
        },
    };
    return cell;
}

{
    const source = make_counted_cell('ready');
    let threw = false;
    try {
        memo(() => {
            throw new Error('memo initialization failed');
        }, [source]);
    } catch {
        threw = true;
    }

    assert(threw, 'memo should propagate an initialization failure');
    assert(
        source.subscriber_count() === 0,
        'memo should unsubscribe explicit dependencies when initialization fails',
    );
}

{
    const node = { textContent: '' };
    const label = signal('ready');
    const dispose = bind_text(node, () => label.get(), [label]);

    assert(node.textContent === 'ready', 'bind_text should apply initial value');
    label.set('live');
    await tick();
    assert(node.textContent === 'live', 'bind_text should update before dispose');

    dispose();
    dispose();
    label.set('stale');
    await tick();
    assert(node.textContent === 'live', 'bind_text disposer should unsubscribe once');
}

{
    const node = { textContent: '' };
    const value = signal(Infinity);
    const dispose = bind_text(node, () => value.get(), [value], [], 'rust-float');

    assert(node.textContent === 'inf', 'float text should use Rust positive-infinity spelling');
    value.set(-Infinity);
    await tick();
    assert(node.textContent === '-inf', 'float text should use Rust negative-infinity spelling');
    value.set(NaN);
    await tick();
    assert(node.textContent === 'NaN', 'float text should preserve the shared NaN spelling');

    dispose();
}

{
    const node = { textContent: '' };
    const dispose = bind_text(node, () => Infinity, []);

    assert(
        node.textContent === 'Infinity',
        'untyped bindings should retain generic JavaScript stringification',
    );
    dispose();
}

{
    const el = make_element('button');
    const disabled = signal(false);
    const dispose = bind_attr(el, 'disabled', 'boolean', () => disabled.get(), [disabled]);

    disabled.set(true);
    await tick();
    assert(el.hasAttribute('disabled'), 'bind_attr should update before dispose');

    dispose();
    dispose();
    disabled.set(false);
    await tick();
    assert(el.hasAttribute('disabled'), 'bind_attr disposer should unsubscribe once');
}

{
    const el = make_element('button');
    const source = make_counted_cell(false);
    let threw = false;
    try {
        bind_attr(
            el,
            'disabled',
            'boolean',
            () => {
                throw new Error('attribute initialization failed');
            },
            [source],
        );
    } catch {
        threw = true;
    }

    assert(threw, 'bind_attr should propagate an initialization failure');
    assert(
        source.subscriber_count() === 0,
        'bind_attr should unsubscribe explicit dependencies when initialization fails',
    );
}

{
    const then_node = make_element();
    const else_node = make_element();
    const open = signal(true);
    const dispose = bind_conditional(() => open.get(), [open], [then_node], [else_node]);

    assert(then_node.style.display === '', 'then branch should start visible');
    assert(else_node.style.display === 'none', 'else branch should start hidden');

    open.set(false);
    await tick();
    assert(then_node.style.display === 'none', 'then branch should hide before dispose');
    assert(else_node.style.display === '', 'else branch should show before dispose');

    dispose();
    dispose();
    open.set(true);
    await tick();
    assert(then_node.style.display === 'none', 'conditional disposer should unsubscribe once');
    assert(else_node.style.display === '', 'conditional disposer should preserve last state');
}

{
    const input = make_element('input');
    const value = signal('start');
    const dispose = bind_input(input, 'value', value);

    assert(input.value === 'start', 'bind_input should apply initial value');
    assert(input.listener_count('input') === 1, 'bind_input should add one input listener');
    assert(input.listener_count('change') === 1, 'bind_input should add one change listener');

    input.value = 'typed';
    input.dispatchEvent({ type: 'input' });
    assert(value.peek() === 'typed', 'bind_input should pull user edits before dispose');

    dispose();
    dispose();
    assert(input.listener_count('input') === 0, 'bind_input disposer should remove input listener once');
    assert(input.listener_count('change') === 0, 'bind_input disposer should remove change listener once');

    input.value = 'ignored';
    input.dispatchEvent({ type: 'input' });
    assert(value.peek() === 'typed', 'bind_input disposer should stop user edits');

    value.set('reset');
    await tick();
    assert(input.value === 'ignored', 'bind_input disposer should unsubscribe from signal updates');
}

{
    const input = make_element('input');
    const value = make_counted_cell('start');
    value.get = () => {
        throw new Error('input initialization failed');
    };
    let threw = false;
    try {
        bind_input(input, 'value', value);
    } catch {
        threw = true;
    }

    assert(threw, 'bind_input should propagate an initialization failure');
    assert(
        input.listener_count('input') === 0,
        'bind_input should remove its input listener when initialization fails',
    );
    assert(
        input.listener_count('change') === 0,
        'bind_input should remove its change listener when initialization fails',
    );
    assert(
        value.subscriber_count() === 0,
        'bind_input should unsubscribe from its cell when initialization fails',
    );
}

{
    const input = make_element('input');
    const value = make_counted_cell('start');
    const subscribe = value._subscribe;
    value._subscribe = (effect) => {
        subscribe(effect);
        throw new Error('input subscription failed');
    };
    let threw = false;
    try {
        bind_input(input, 'value', value);
    } catch {
        threw = true;
    }

    assert(threw, 'bind_input should propagate a subscription failure');
    assert(
        input.listener_count('input') === 0 &&
            input.listener_count('change') === 0,
        'bind_input should remove listeners after a partial subscription failure',
    );
    assert(
        value.subscriber_count() === 0,
        'bind_input should remove a partially registered subscription',
    );
}

{
    const button = make_element('button');
    let calls = 0;
    const dispose = bind_event(button, 'click', () => {
        calls += 1;
    });

    button.dispatchEvent({ type: 'click' });
    assert(calls === 1, 'bind_event should fire before dispose');

    dispose();
    dispose();
    button.dispatchEvent({ type: 'click' });
    assert(calls === 1, 'bind_event disposer should remove listener once');
}

{
    const form = make_element('form');
    let calls = 0;
    let prevented = false;
    const dispose = bind_submit(form, () => {
        calls += 1;
    });

    form.dispatchEvent({
        type: 'submit',
        preventDefault() {
            prevented = true;
        },
    });
    assert(calls === 1, 'bind_submit should fire before dispose');
    assert(prevented, 'bind_submit should prevent default before handler');

    dispose();
    dispose();
    form.dispatchEvent({ type: 'submit', preventDefault() {} });
    assert(calls === 1, 'bind_submit disposer should remove listener once');
}

{
    const root = make_element('rsjs-island');
    const button = make_element('button', { 'data-rsjs-handler': '0' });
    root.appendChild(button);
    let calls = 0;
    const dispose = bind_delegated_events(
        root,
        'click',
        new Map([['0', () => { calls += 1; }]]),
    );

    root.dispatchEvent({ type: 'click', target: button });
    assert(calls === 1, 'delegated handler should fire before dispose');

    dispose();
    dispose();
    root.dispatchEvent({ type: 'click', target: button });
    assert(calls === 1, 'delegated disposer should remove root listener once');
}

{
    const owner = make_element('div');
    const doc = make_event_target();
    owner.ownerDocument = doc;
    let calls = 0;
    const dispose = bind_document_event(owner, 'pointerdown', () => {
        calls += 1;
    });

    doc.dispatchEvent({ type: 'pointerdown' });
    assert(calls === 1, 'document event should fire before dispose');

    dispose();
    dispose();
    doc.dispatchEvent({ type: 'pointerdown' });
    assert(calls === 1, 'document event disposer should remove listener once');
}

{
    const owner = make_element('div');
    const win = make_event_target();
    owner.ownerDocument = { defaultView: win };
    let calls = 0;
    const dispose = bind_window_event(owner, 'resize', () => {
        calls += 1;
    });

    win.dispatchEvent({ type: 'resize' });
    assert(calls === 1, 'window event should fire before dispose');

    dispose();
    dispose();
    win.dispatchEvent({ type: 'resize' });
    assert(calls === 1, 'window event disposer should remove listener once');
}

if (failed > 0) {
    process.exit(1);
}

console.log('All cleanup lifecycle tests passed.');
