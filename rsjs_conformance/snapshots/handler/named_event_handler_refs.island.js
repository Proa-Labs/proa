import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("namedeventhandlersnapshot_35f3607b", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, ""));
    const __sig_1 = signal(signal_initial(__rsjs_signal_initials, 1, ""));

    function __handler_0(event) {
        console.log("today");
        __sig_1.set("today");
        __sig_0.set((event.target?.closest?.("[data-date]")?.getAttribute("data-date") ?? ""));
    }

    const __node_3 = walk_path(root, [0, 1], "data-rsjs-bind", "3");
    __rsjs_disposers.push(bind_text(__node_3, () => __sig_0.get(), [__sig_0]));

    const __node_4 = walk_path(root, [0, 2], "data-rsjs-bind", "4");
    __rsjs_disposers.push(bind_text(__node_4, () => __sig_1.get(), [__sig_1]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["1", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
