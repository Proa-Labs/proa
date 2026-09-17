import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_attr, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("basichandlersnapshot_091c0446", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, ""));

    function __handler_0(event) {
        let __local_0 = (event.target?.value ?? "");
        __sig_0.set(__local_0);
    }

    const __node_0 = walk_path(root, [0], "data-rsjs-attr", "0");
    __rsjs_disposers.push(bind_attr(__node_0, "value", "value", () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "input", new Map([["0", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
