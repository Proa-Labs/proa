import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("navigatorlanguagesnapshot_cc1665b3", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, ""));

    function __handler_0(event) {
        __sig_0.set(((typeof navigator !== "undefined") ? (navigator.language ?? "") : ""));
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => ((typeof navigator !== "undefined") ? (navigator.language ?? "") : ""), []));

    const __node_4 = walk_path(root, [0, 2], "data-rsjs-bind", "4");
    __rsjs_disposers.push(bind_text(__node_4, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["2", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
