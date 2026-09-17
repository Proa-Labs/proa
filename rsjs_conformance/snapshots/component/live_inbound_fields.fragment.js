import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_attr, bind_delegated_events, walk_path, dispose_all } from "/rsjs-runtime.js";

export function mount_liveinboundfieldssnapshot_79734c02(root, options = {}) {
    const __rsjs_disposers = [];
    try {
    const __rsjs_signal_initials = read_signal_initials(options.seedRoot ?? root);
    const __rsjs_signal_overrides = options.signals ?? {};
    const __rsjs_signal_seeds = options.signalSeeds ?? {};
    const __sig_0 = __rsjs_signal_overrides[0] ?? (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 0) ? signal(__rsjs_signal_seeds[0]) : signal(signal_initial(__rsjs_signal_initials, 0, null)));
    const __sig_1 = __rsjs_signal_overrides[1] ?? (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 1) ? signal(__rsjs_signal_seeds[1]) : signal(signal_initial(__rsjs_signal_initials, 1, null)));
    const __sig_2 = __rsjs_signal_overrides[2] ?? (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 2) ? signal(__rsjs_signal_seeds[2]) : signal(signal_initial(__rsjs_signal_initials, 2, null)));

    function __handler_0(event) {
        if (root.__rsjs_callbacks?.__callback_0) root.__rsjs_callbacks.__callback_0(__sig_1.get());
    }

    const __node_1 = walk_path(root, [0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __sig_1.get(), [__sig_1]));

    const __node_0 = walk_path(root, [], "data-rsjs-attr", "0");
    __rsjs_disposers.push(bind_attr(__node_0, "class", "value", () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["2", __handler_0]])));

    const __cond_node_0_then_0 = walk_path(root, [1], "data-rsjs-cond-then", "0:0");
    __rsjs_disposers.push(bind_conditional(
        () => __sig_2.get(),
        [__sig_2],
        [__cond_node_0_then_0],
        [],
    ));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
    } catch (__rsjs_init_error) {
        dispose_all(__rsjs_disposers, "rsjs fragment init rollback");
        throw __rsjs_init_error;
    }
}
