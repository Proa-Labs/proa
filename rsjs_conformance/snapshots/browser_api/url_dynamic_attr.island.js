import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_attr, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("dynamicurlattr_f4434747", (root) => {
    const __rsjs_disposers = [];
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, "javascript:alert(1)"));

    function __handler_0(event) {
        __sig_0.set("https://example.com/safe");
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-attr", "1");
    __rsjs_disposers.push(bind_attr(__node_1, "href", "url", () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["3", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
