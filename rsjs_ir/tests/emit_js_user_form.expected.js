import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_input, bind_submit, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("UserForm", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, ""));
    const __sig_1 = signal(signal_initial(__rsjs_signal_initials, 1, ""));
    const __sig_2 = signal(signal_initial(__rsjs_signal_initials, 2, false));

    function __handler_0(event) {
        __sig_2.set(true);
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-input-value", "1");
    __rsjs_disposers.push(bind_input(__node_1, "value", __sig_0));

    const __node_4 = walk_path(root, [0, 2], "data-rsjs-input-value", "4");
    __rsjs_disposers.push(bind_input(__node_4, "value", __sig_1));

    const __node_0 = walk_path(root, [0], "data-rsjs-handler", "0");
    __rsjs_disposers.push(bind_submit(__node_0, __handler_0));

    const __cond_node_2_then_0 = walk_path(root, [0, 1, 0], "data-rsjs-cond-then", "2:0");
    __rsjs_disposers.push(bind_conditional(
        () => ((((__v) => (__v != null && (typeof __v === 'string' ? __v.length > 0 : Array.isArray(__v) ? __v.length > 0 : typeof __v === 'object' ? Object.keys(__v).length > 0 : false)))(__sig_0.get())) === false),
        [__sig_0],
        [__cond_node_2_then_0],
        [],
    ));

    const __cond_node_5_then_0 = walk_path(root, [0, 3, 0], "data-rsjs-cond-then", "5:0");
    __rsjs_disposers.push(bind_conditional(
        () => (__sig_1.get() === ""),
        [__sig_1],
        [__cond_node_5_then_0],
        [],
    ));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
