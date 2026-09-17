import { signal, bind_text, bind_conditional, bind_document_event, bind_window_event, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("repeatedglobaleventssnapshot_6ffd6fb3", (root, __rsjs_disposers = []) => {

    function __handler_0(event) {
        console.log(JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-p0')) ?? (event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-p0')) ?? (event.target?.closest?.('[data-rsjs-payload-p0]')?.getAttribute('data-rsjs-payload-p0'))) ?? "null"), (event.key ?? ""));
    }

    function __handler_1(event) {
        console.log(JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-p1')) ?? (event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-p1')) ?? (event.target?.closest?.('[data-rsjs-payload-p1]')?.getAttribute('data-rsjs-payload-p1'))) ?? "null"));
    }

    function __handler_2(event) {
        console.log(JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-p2')) ?? (event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-p2')) ?? (event.target?.closest?.('[data-rsjs-payload-p2]')?.getAttribute('data-rsjs-payload-p2'))) ?? "null"), ((typeof window !== "undefined") ? (window.scrollY ?? 0) : 0));
    }

    for (const __node_2 of root.querySelectorAll('[data-rsjs-handler="2"]')) {
        __rsjs_disposers.push(bind_document_event(__node_2, "keydown", __handler_0));
    }

    for (const __node_2 of root.querySelectorAll('[data-rsjs-handler="2"]')) {
        __rsjs_disposers.push(bind_window_event(__node_2, "resize", __handler_1));
    }

    for (const __node_2 of root.querySelectorAll('[data-rsjs-handler="2"]')) {
        __rsjs_disposers.push(bind_window_event(__node_2, "scroll", __handler_2));
    }
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
