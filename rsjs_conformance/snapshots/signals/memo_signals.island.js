import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, memo, bind_delegated_events, walk_path, hydrate, dispose_all, checked_int_add, checked_int_mul } from "/rsjs-runtime.js";

hydrate("memosignalssnapshot_50112140", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 10n, "i64"), "i64");
    const __sig_1 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 1, 2n, "i64"), "i64");
    const __sig_2 = memo(() => checked_int_mul(__sig_0.get(), __sig_1.get(), "i64"), [__sig_0, __sig_1]);
    __rsjs_disposers.push(__sig_2._dispose);

    function __handler_0(event) {
        __sig_1.set(checked_int_add(__sig_1.get(), 1n, "i64"));
    }

    const __node_3 = walk_path(root, [0, 1], "data-rsjs-bind", "3");
    __rsjs_disposers.push(bind_text(__node_3, () => __sig_2.get(), [__sig_2]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["1", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
