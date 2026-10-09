import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, memo, bind_delegated_events, walk_path, hydrate, dispose_all, checked_int_div, checked_int_mul, checked_int_rem } from "/rsjs-runtime.js";

hydrate("numerici64divisionmodulosnapshot_b8d85dc3", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, -7n, "i64"), "i64");
    const __sig_1 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 1, 3n, "i64"), "i64");
    const __sig_2 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 2, 7n, "i64"), "i64");
    const __sig_3 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 3, -3n, "i64"), "i64");
    const __sig_4 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 4, 0n, "i64"), "i64");
    const __sig_5 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 5, 0n, "i64"), "i64");
    const __sig_6 = memo(() => checked_int_mul(__sig_0.get(), 2n, "i64"), [__sig_0]);
    __rsjs_disposers.push(__sig_6._dispose);

    function __handler_0(event) {
        __sig_4.set(checked_int_div(__sig_0.get(), __sig_1.get(), "i64"));
        __sig_5.set(checked_int_rem(__sig_0.get(), __sig_1.get(), "i64"));
    }

    function __handler_1(event) {
        __sig_4.set(checked_int_div(__sig_2.get(), __sig_3.get(), "i64"));
        __sig_5.set(checked_int_rem(__sig_2.get(), __sig_3.get(), "i64"));
    }

    function __handler_2(event) {
        let __local_0 = __sig_0.get();
        let __local_1 = checked_int_div(__local_0, __sig_1.get(), "i64");
        __local_1 = checked_int_div(__local_1, 2n, "i64");
        __sig_4.set(__local_1);
        __sig_5.set(checked_int_div(__sig_6.get(), 4n, "i64"));
    }

    const __node_7 = walk_path(root, [0, 3], "data-rsjs-bind", "7");
    __rsjs_disposers.push(bind_text(__node_7, () => __sig_4.get(), [__sig_4]));

    const __node_8 = walk_path(root, [0, 4], "data-rsjs-bind", "8");
    __rsjs_disposers.push(bind_text(__node_8, () => __sig_5.get(), [__sig_5]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([
        ["1", __handler_0],
        ["3", __handler_1],
        ["5", __handler_2],
    ])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
