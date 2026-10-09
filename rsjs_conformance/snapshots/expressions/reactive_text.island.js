import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, integer_signal, signal_initial_integer, bind_attr, bind_delegated_events, walk_path, hydrate, dispose_all, checked_int_add, checked_int_div, checked_int_mul, checked_int_rem, format_text, format_text_attr } from "/rsjs-runtime.js";

hydrate("reactivetextsnapshot_437d105e", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, null, "u32"), "u32");
    const __sig_1 = signal(signal_initial(__rsjs_signal_initials, 1, "<tag & value>"));
    const __sig_2 = signal(signal_initial(__rsjs_signal_initials, 2, null));
    const __sig_3 = signal(signal_initial(__rsjs_signal_initials, 3, "fallback"));
    const __sig_4 = signal(signal_initial(__rsjs_signal_initials, 4, 0.0));
    const __sig_5 = signal(signal_initial(__rsjs_signal_initials, 5, 1.0));
    const __sig_6 = signal(signal_initial(__rsjs_signal_initials, 6, 0.0));
    const __sig_7 = signal(signal_initial(__rsjs_signal_initials, 7, null));
    const __sig_8 = signal(signal_initial(__rsjs_signal_initials, 8, null));

    function __handler_0(event) {
        __sig_0.update((__c0) => checked_int_add(__sig_0.get(), 1, "u32"));
        __sig_1.set("<new & value>");
        __sig_2.update((__c0) => (__sig_2.get() === false));
        __sig_8.set(__sig_2.get());
        __sig_6.set((__sig_2.get() ? ((__sig_5.get() / __sig_4.get()) * -1.0) : (__sig_5.get() / __sig_4.get())));
    }

    const __node_3 = walk_path(root, [0, 1], "data-rsjs-bind", "3");
    __rsjs_disposers.push(bind_text(__node_3, () => format_text("Subtotal: ${}.{:02}", "[{\"text\":\"Subtotal: $\"},{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":false},{\"text\":\".\"},{\"align\":0,\"alternate\":false,\"arg\":1,\"domain\":\"u32\",\"fill\":\" \",\"kind\":0,\"plus\":false,\"precision\":null,\"string\":false,\"width\":2,\"zero\":true}]", checked_int_div(checked_int_mul(__sig_0.get(), 105, "u32"), 100, "u32"), checked_int_rem(checked_int_mul(__sig_0.get(), 105, "u32"), 100, "u32")), [__sig_0]));

    const __node_6 = walk_path(root, [0, 2, 0], "data-rsjs-bind", "6");
    __rsjs_disposers.push(bind_text(__node_6, () => format_text("{{{}}} {} {}", "[{\"text\":\"{\"},{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":true},{\"text\":\"} \"},{\"arg\":1,\"attr_bool\":true,\"float\":false,\"string\":false},{\"text\":\" \"},{\"arg\":2,\"attr_bool\":true,\"float\":false,\"string\":true}]", __sig_1.get(), __sig_0.get(), __sig_1.get()), [__sig_1, __sig_0]));

    const __node_8 = walk_path(root, [0, 3], "data-rsjs-bind", "8");
    __rsjs_disposers.push(bind_text(__node_8, () => format_text("{} {}", "[{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":true},{\"text\":\" \"},{\"arg\":1,\"attr_bool\":true,\"float\":false,\"string\":true}]", (__sig_2.get() ? __sig_1.get() : __sig_3.get()), __sig_1.get()), [__sig_2, __sig_1, __sig_3]));

    const __node_9 = walk_path(root, [0, 4], "data-rsjs-bind", "9");
    __rsjs_disposers.push(bind_text(__node_9, () => format_text("{} {:02} {:x} {:#X}", "[{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":false},{\"text\":\" \"},{\"align\":0,\"alternate\":false,\"arg\":1,\"domain\":\"u32\",\"fill\":\" \",\"kind\":0,\"plus\":false,\"precision\":null,\"string\":false,\"width\":2,\"zero\":true},{\"text\":\" \"},{\"align\":0,\"alternate\":false,\"arg\":2,\"domain\":\"i32\",\"fill\":\" \",\"kind\":3,\"plus\":false,\"precision\":null,\"string\":false,\"width\":0,\"zero\":false},{\"text\":\" \"},{\"align\":0,\"alternate\":true,\"arg\":3,\"domain\":\"i64\",\"fill\":\" \",\"kind\":4,\"plus\":false,\"precision\":null,\"string\":false,\"width\":0,\"zero\":false}]", __sig_0.get(), checked_int_add(5, 0, "u32"), checked_int_add(-1, 0, "i32"), checked_int_add(-1n, 0n, "i64")), [__sig_0]));

    const __node_10 = walk_path(root, [0, 5], "data-rsjs-bind", "10");
    __rsjs_disposers.push(bind_text(__node_10, () => format_text("Ratio {}", "[{\"text\":\"Ratio \"},{\"arg\":0,\"attr_bool\":true,\"float\":true,\"string\":false}]", __sig_6.get()), [__sig_6]));

    const __node_11 = walk_path(root, [0, 6], "data-rsjs-bind", "11");
    __rsjs_disposers.push(bind_text(__node_11, () => format_text("{} {}", "[{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":true},{\"text\":\" \"},{\"arg\":1,\"attr_bool\":true,\"float\":false,\"string\":true}]", (__sig_2.get() ? __sig_1.get() : (String((__sig_3.get()) ?? "").toUpperCase())), __sig_1.get()), [__sig_2, __sig_1, __sig_3]));

    const __node_12 = walk_path(root, [0, 7], "data-rsjs-bind", "12");
    __rsjs_disposers.push(bind_text(__node_12, () => format_text("{} {}", "[{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":false},{\"text\":\" \"},{\"arg\":1,\"attr_bool\":true,\"float\":false,\"string\":false}]", (__sig_2.get() ? __sig_7.get() : __sig_1.get()), __sig_7.get()), [__sig_2, __sig_7, __sig_1]));

    const __node_13 = walk_path(root, [0, 8], "data-rsjs-bind", "13");
    __rsjs_disposers.push(bind_text(__node_13, () => format_text("{}", "[{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":true}]", (__sig_2.get() ? "true" : "false")), [__sig_2]));

    const __node_14 = walk_path(root, [0, 9], "data-rsjs-bind", "14");
    __rsjs_disposers.push(bind_text(__node_14, () => format_text("Active: {}", "[{\"text\":\"Active: \"},{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":true}]", (__sig_8.get() ? "true" : "false")), [__sig_8]));

    const __node_4 = walk_path(root, [0, 2], "data-rsjs-attr", "4");
    __rsjs_disposers.push(bind_attr(__node_4, "title", "value", () => format_text_attr("Quantity {}", "[{\"text\":\"Quantity \"},{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":false}]", __sig_0.get()), [__sig_0]));

    __rsjs_disposers.push(bind_attr(__node_11, "title", "value", () => format_text_attr("{:>16.8}", "[{\"align\":2,\"alternate\":false,\"arg\":0,\"domain\":null,\"fill\":\" \",\"kind\":0,\"plus\":false,\"precision\":8,\"string\":true,\"width\":16,\"zero\":false}]", (__sig_2.get() ? __sig_1.get() : (String((__sig_3.get()) ?? "").toUpperCase()))), [__sig_2, __sig_1, __sig_3]));

    __rsjs_disposers.push(bind_attr(__node_13, "title", "value", () => format_text_attr("Active: {}", "[{\"text\":\"Active: \"},{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":false}]", __sig_2.get()), [__sig_2]));

    __rsjs_disposers.push(bind_attr(__node_14, "data-state", "value", () => format_text_attr("Active: {}", "[{\"text\":\"Active: \"},{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":false}]", __sig_8.get()), [__sig_8]));

    __rsjs_disposers.push(bind_attr(__node_14, "title", "value", () => format_text_attr("Active: {}", "[{\"text\":\"Active: \"},{\"arg\":0,\"attr_bool\":true,\"float\":false,\"string\":false}]", __sig_8.get()), [__sig_8]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["1", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
