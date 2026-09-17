import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, interval, walk_path, hydrate, dispose_all, checked_int_add } from "/rsjs-runtime.js";

hydrate("timerintervalsnapshot_1cb532fe", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 0n, "i64"), "i64");

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(interval(20, () => {
        __sig_0.update((__c0) => checked_int_add(__sig_0.get(), 1n, "i64"));
    }));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
