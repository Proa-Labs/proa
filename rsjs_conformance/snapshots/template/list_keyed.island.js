import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_list, bind_delegated_events, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("keyedlistsnapshot_054e57ab", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, null));

    function __handler_0(event) {
        __sig_0.set(["Gamma", "Beta", "Alpha"]);
    }

    function __handler_1(event) {
        __sig_0.set(["Gamma", "Alpha"]);
    }

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([
        ["1", __handler_0],
        ["3", __handler_1],
    ])));

    const __node_5 = (root.matches('[data-rsjs-list="5"]') ? root : root.querySelector('[data-rsjs-list="5"]'));
    const __list_create_0 = (item) => {
        const __li_0_0 = document.createElement("li");
        __li_0_0.setAttribute("class", "todo");
        const update = (item) => {
            __li_0_0.textContent = String(item);
        };
        update(item);
        return { node: __li_0_0, update };
    };
    const __list_adopt_0 = (node, item) => {
        const __li_0_0 = node;
        const update = (item) => {
            __li_0_0.textContent = String(item);
        };
        return { node, update };
    };
    __rsjs_disposers.push(bind_list(__node_5, () => __sig_0.get(), {
        key: (item) => String(item),
        create: __list_create_0,
        adopt: __list_adopt_0,
        deps: [__sig_0],
    }));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
