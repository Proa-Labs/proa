import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_attr, bind_delegated_events, walk_path, dispose_all } from "/rsjs-runtime.js";

export function mount_callbacktextinputsnapshot_b6bb0121(root, options = {}) {
    const __rsjs_disposers = [];
    try {
    const __rsjs_signal_initials = read_signal_initials(options.seedRoot ?? root);
    const __rsjs_signal_overrides = options.signals ?? {};
    const __rsjs_signal_seeds = options.signalSeeds ?? {};
    const __sig_0 = __rsjs_signal_overrides[0] ?? (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 0) ? signal(__rsjs_signal_seeds[0]) : signal(signal_initial(__rsjs_signal_initials, 0, null)));

    function __handler_0(event) {
        let __local_0 = (event.target?.value ?? "");
        console.log(__local_0);
        if (root.__rsjs_callbacks?.__callback_0) root.__rsjs_callbacks.__callback_0(__local_0);
    }

    const __node_1 = walk_path(root, [0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __sig_0.get(), [__sig_0]));

    const __node_2 = walk_path(root, [1], "data-rsjs-attr", "2");
    __rsjs_disposers.push(bind_attr(__node_2, "value", "value", () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "input", new Map([["2", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
    } catch (__rsjs_init_error) {
        dispose_all(__rsjs_disposers, "rsjs fragment init rollback");
        throw __rsjs_init_error;
    }
}
