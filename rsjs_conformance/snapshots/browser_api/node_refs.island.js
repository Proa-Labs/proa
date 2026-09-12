import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("noderefssnapshot_e508b656", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, ""));
    const __ref_0 = { current: null };
    __rsjs_disposers.push(() => { __ref_0.current = null; });
    const __ref_1 = { current: null };
    __rsjs_disposers.push(() => { __ref_1.current = null; });
    const __ref_2 = { current: null };
    __rsjs_disposers.push(() => { __ref_2.current = null; });
    const __ref_3 = { current: null };
    __rsjs_disposers.push(() => { __ref_3.current = null; });

    function __handler_0(event) {
        __ref_0.current?.focus?.();
        __ref_0.current?.click?.();
        __ref_1.current?.requestSubmit?.();
        __ref_1.current?.reset?.();
        __ref_2.current?.showModal?.();
        __ref_2.current?.close?.();
        __ref_3.current?.showPopover?.();
        __ref_3.current?.hidePopover?.();
        __ref_3.current?.togglePopover?.();
        __ref_0.current?.scrollTo?.(Number(0), Number(32));
        __ref_0.current?.scrollBy?.(Number(0), Number(8));
        __ref_0.current?.select?.();
        if (__ref_0.current != null) __ref_0.current.value = "";
        __ref_0.current?.setSelectionRange?.(Number(0), Number(0));
        __ref_0.current?.setAttribute?.("aria-expanded", String("false"));
        __ref_0.current?.removeAttribute?.("hidden");
        __ref_0.current?.style?.setProperty("outline-color", String("transparent"));
        console.log((__ref_0.current?.getAttribute?.("data-kind") ?? ""), (__ref_0.current?.hasAttribute?.("hidden") ?? false));
        __sig_0.set((__ref_0.current?.value ?? ""));
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-ref", "1");
    __ref_0.current = __node_1;

    const __node_2 = walk_path(root, [0, 1], "data-rsjs-ref", "2");
    __ref_1.current = __node_2;

    const __node_3 = walk_path(root, [0, 2], "data-rsjs-ref", "3");
    __ref_2.current = __node_3;

    const __node_5 = walk_path(root, [0, 3], "data-rsjs-ref", "5");
    __ref_3.current = __node_5;

    const __node_9 = walk_path(root, [0, 5], "data-rsjs-bind", "9");
    __rsjs_disposers.push(bind_text(__node_9, () => __sig_0.get(), [__sig_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["7", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
