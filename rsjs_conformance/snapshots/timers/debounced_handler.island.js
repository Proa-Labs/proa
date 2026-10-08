import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, integer_signal, signal_initial_integer, debounce, bind_delegated_events, walk_path, hydrate, dispose_all, checked_int_add } from "/rsjs-runtime.js";

hydrate("debouncedhandlersnapshot_7247a66a", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, ""));
    const __sig_1 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 1, 0n, "i64"), "i64");

    const __handler_0 = debounce((event) => {
        __sig_0.set((event.target?.value ?? ""));
        __sig_1.update((__c0) => checked_int_add(__sig_1.get(), 1n, "i64"));
    }, 30);
    __rsjs_disposers.push(__handler_0._dispose);

    const __node_2 = walk_path(root, [0, 1], "data-rsjs-bind", "2");
    __rsjs_disposers.push(bind_text(__node_2, () => __sig_0.get(), [__sig_0]));

    const __node_3 = walk_path(root, [0, 2], "data-rsjs-bind", "3");
    __rsjs_disposers.push(bind_text(__node_3, () => __sig_1.get(), [__sig_1]));

    __rsjs_disposers.push(bind_delegated_events(root, "input", new Map([["1", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
