import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, bind_delegated_events, walk_path, hydrate, dispose_all, checked_int_add } from "/rsjs-runtime.js";

hydrate("numerici32overflowsnapshot_9b3b3fc2", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 2147483647, "i32"), "i32");

    function __handler_0(event) {
        __sig_0.set(checked_int_add(__sig_0.get(), 1, "i32"));
    }

    const __node_3 = walk_path(root, [0, 1], "data-rsjs-bind", "3");
    __rsjs_disposers.push(bind_text(__node_3, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["1", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
