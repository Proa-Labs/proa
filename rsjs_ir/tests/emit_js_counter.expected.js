import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("Counter", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, 0));

    function __handler_0(event) {
        __sig_0.update((__c0) => (__sig_0.get() - 1));
    }

    function __handler_1(event) {
        __sig_0.update((__c0) => (__sig_0.get() + 1));
    }

    const __node_2 = walk_path(root, [0, 1], "data-rsjs-bind", "2");
    __rsjs_disposers.push(bind_text(__node_2, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([
        ["1", __handler_0],
        ["3", __handler_1],
    ])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
