import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, memo, bind_delegated_events, walk_path, hydrate, dispose_all, checked_int_add } from "/rsjs-runtime.js";

hydrate("numerici64precisionsnapshot_6a5cac95", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 9007199254740993n, "i64"), "i64");
    const __sig_1 = memo(() => checked_int_add(__sig_0.get(), 1n, "i64"), [__sig_0]);
    __rsjs_disposers.push(__sig_1._dispose);

    function __handler_0(event) {
        __sig_0.set(checked_int_add(__sig_0.get(), 1n, "i64"));
    }

    function __handler_1(event) {
        __sig_0.update((__c0) => checked_int_add(__sig_0.get(), 1n, "i64"));
    }

    function __handler_2(event) {
        let __local_0 = __sig_0.get();
        __sig_0.set(checked_int_add(__local_0, 1n, "i64"));
    }

    function __handler_3(event) {
        __sig_0.set(checked_int_add(__sig_1.get(), 1n, "i64"));
    }

    const __node_9 = walk_path(root, [0, 4], "data-rsjs-bind", "9");
    __rsjs_disposers.push(bind_text(__node_9, () => __sig_0.get(), [__sig_0]));

    const __node_10 = walk_path(root, [0, 5], "data-rsjs-bind", "10");
    __rsjs_disposers.push(bind_text(__node_10, () => __sig_1.get(), [__sig_1]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([
        ["1", __handler_0],
        ["3", __handler_1],
        ["5", __handler_2],
        ["7", __handler_3],
    ])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
