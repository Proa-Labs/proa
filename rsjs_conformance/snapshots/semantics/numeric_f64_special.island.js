import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("numericf64specialsnapshot_6b559b06", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, 0.0));
    const __sig_1 = signal(signal_initial(__rsjs_signal_initials, 1, 1.0));
    const __sig_2 = signal(signal_initial(__rsjs_signal_initials, 2, 0.0));
    const __sig_3 = signal(signal_initial(__rsjs_signal_initials, 3, 0.0));

    function __handler_0(event) {
        __sig_2.set((__sig_0.get() / __sig_0.get()));
    }

    function __handler_1(event) {
        __sig_2.set((__sig_1.get() / __sig_0.get()));
    }

    function __handler_2(event) {
        __sig_3.set((0.1 + 0.2));
    }

    const __node_7 = walk_path(root, [0, 3], "data-rsjs-bind", "7");
    __rsjs_disposers.push(bind_text(__node_7, () => __sig_2.get(), [__sig_2], [], "rust-float"));

    const __node_8 = walk_path(root, [0, 4], "data-rsjs-bind", "8");
    __rsjs_disposers.push(bind_text(__node_8, () => __sig_3.get(), [__sig_3], [], "rust-float"));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([
        ["1", __handler_0],
        ["3", __handler_1],
        ["5", __handler_2],
    ])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
