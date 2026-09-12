import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_input, bind_delegated_events, dispose_all } from "/rsjs-runtime.js";

export function mount_repeatedbuttonssnapshot_c5e5a417(root, options = {}) {
    const __rsjs_disposers = [];
    try {
    const __rsjs_signal_initials = read_signal_initials(options.seedRoot ?? root);
    const __rsjs_signal_overrides = options.signals ?? {};
    const __rsjs_signal_seeds = options.signalSeeds ?? {};
    const __sig_0 = __rsjs_signal_overrides[0] ?? (Object.prototype.hasOwnProperty.call(__rsjs_signal_seeds, 0) ? signal(__rsjs_signal_seeds[0]) : signal(signal_initial(__rsjs_signal_initials, 0, "Today")));

    function __handler_0(event) {
        let __local_0 = JSON.parse(((event.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload-p1')) ?? (event.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload-p1')) ?? (event.target?.closest?.('[data-rsjs-payload-p1]')?.getAttribute('data-rsjs-payload-p1'))) ?? "null");
        console.log(__local_0);
        if (root.__rsjs_callbacks?.__callback_0) root.__rsjs_callbacks.__callback_0(__local_0);
    }

    for (const __node_3 of root.querySelectorAll('[data-rsjs-input-value="3"]')) {
        __rsjs_disposers.push(bind_input(__node_3, "value", __sig_0, undefined, options.preserveDirtyControls !== false));
    }

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["8", __handler_0]])));

    const __cond_nodes_2_then_0 = Array.from(root.querySelectorAll('[data-rsjs-cond-then="2:0"]'));
    const __cond_nodes_2_else_0 = Array.from(root.querySelectorAll('[data-rsjs-cond-else="2:0"]'));
    const __cond_count_2_0_0 = Math.max(0, __cond_nodes_2_then_0.length, __cond_nodes_2_else_0.length);
    for (let __cond_i_2_0_0 = 0; __cond_i_2_0_0 < __cond_count_2_0_0; __cond_i_2_0_0++) {
        const __cond_ctx_2_0_0 = (__cond_nodes_2_then_0[__cond_i_2_0_0] ?? __cond_nodes_2_else_0[__cond_i_2_0_0])?.parentElement ?? null;
        __rsjs_disposers.push(bind_conditional(
            () => (__sig_0.get() === JSON.parse((__cond_ctx_2_0_0?.getAttribute('data-rsjs-payload-p0')) ?? "null")),
            [__sig_0],
            [__cond_nodes_2_then_0[__cond_i_2_0_0]],
            [__cond_nodes_2_else_0[__cond_i_2_0_0]],
        ));
    }
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
    } catch (__rsjs_init_error) {
        dispose_all(__rsjs_disposers, "rsjs fragment init rollback");
        throw __rsjs_init_error;
    }
}
