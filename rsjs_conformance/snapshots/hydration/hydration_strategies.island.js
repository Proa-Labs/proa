// StrategyVisibleSnapshot
import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, bind_delegated_events, walk_path, hydrate_from_module, hydrate_when_visible, dispose_all } from "/rsjs-runtime.js";

hydrate_from_module(import.meta.url, hydrate_when_visible)("strategyvisiblesnapshot_8e896aba", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 0n, "i64"), "i64");

    function __handler_0(event) {
        __sig_0.set(1n);
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["0", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});

// StrategyIdleSnapshot
import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, bind_delegated_events, walk_path, hydrate_from_module, hydrate_when_idle, dispose_all } from "/rsjs-runtime.js";

hydrate_from_module(import.meta.url, hydrate_when_idle)("strategyidlesnapshot_169255a8", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 0n, "i64"), "i64");

    function __handler_0(event) {
        __sig_0.set(1n);
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["0", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});

// StrategyInteractionSnapshot
import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, bind_delegated_events, walk_path, hydrate_from_module, hydrate_on_interaction, dispose_all } from "/rsjs-runtime.js";

hydrate_from_module(import.meta.url, hydrate_on_interaction)("strategyinteractionsnapshot_d386af7f", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 0n, "i64"), "i64");

    function __handler_0(event) {
        __sig_0.set(1n);
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["0", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
}, ["click"]);

// StrategyMediaSnapshot
import { signal, bind_text, bind_conditional, read_signal_initials, integer_signal, signal_initial_integer, bind_delegated_events, walk_path, hydrate_from_module, hydrate_when_media, dispose_all } from "/rsjs-runtime.js";

hydrate_from_module(import.meta.url, hydrate_when_media)("strategymediasnapshot_91369263", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = integer_signal(signal_initial_integer(__rsjs_signal_initials, 0, 0n, "i64"), "i64");

    function __handler_0(event) {
        __sig_0.set(1n);
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["0", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
}, "(min-width: 768px)");

