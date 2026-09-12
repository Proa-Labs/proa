import { signal, bind_text, bind_conditional, bind_delegated_events, dispose_all } from "/rsjs-runtime.js";

export function mount_serverdecoratedbuttonsnapshot_d7f28fae(root, options = {}) {
    const __rsjs_disposers = [];
    try {

    function __handler_0(event) {
        if (root.__rsjs_callbacks?.__callback_0) root.__rsjs_callbacks.__callback_0(null);
    }

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["0", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
    } catch (__rsjs_init_error) {
        dispose_all(__rsjs_disposers, "rsjs fragment init rollback");
        throw __rsjs_init_error;
    }
}
