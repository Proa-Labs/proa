import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, integer_signal, signal_initial_integer, bind_delegated_events, walk_path, hydrate, dispose_all, checked_int_add } from "/rsjs-runtime.js";

hydrate("conditionaltruthinesssnapshot_11f0ac8e", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 0n, "i64"), "i64");
    const __sig_1 = signal(signal_initial(__rsjs_signal_initials, 1, "unset"));
    const __sig_2 = signal(signal_initial(__rsjs_signal_initials, 2, "unset"));

    function __handler_0(event) {
        if ((__sig_0.get() === 0n)) {
            __sig_1.set("zero");
        } else {
            __sig_1.set("nonzero");
        }
        __sig_0.set(checked_int_add(__sig_0.get(), 1n, "i64"));
    }

    function __handler_1(event) {
        let __local_0 = (event.target?.value ?? "");
        if ((__local_0 === "")) {
            __sig_2.set("empty");
        } else {
            __sig_2.set("filled");
        }
    }

    const __node_4 = walk_path(root, [0, 2], "data-rsjs-bind", "4");
    __rsjs_disposers.push(bind_text(__node_4, () => __sig_1.get(), [__sig_1]));

    const __node_5 = walk_path(root, [0, 3], "data-rsjs-bind", "5");
    __rsjs_disposers.push(bind_text(__node_5, () => __sig_2.get(), [__sig_2]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["1", __handler_0]])));

    __rsjs_disposers.push(bind_delegated_events(root, "input", new Map([["3", __handler_1]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
