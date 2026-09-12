import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("branchinghandlersnapshot_b5044fed", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, "plain"));

    function __handler_0(event) {
        if (!!event.shiftKey) {
            if (!!event.ctrlKey) {
                __sig_0.set("both");
            } else {
                __sig_0.set("shift");
            }
        } else {
            __sig_0.set("plain");
        }
    }

    const __node_3 = walk_path(root, [0, 1], "data-rsjs-bind", "3");
    __rsjs_disposers.push(bind_text(__node_3, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["1", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
