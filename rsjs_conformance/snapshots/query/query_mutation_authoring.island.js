import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_attr, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";
import { rsjs, dispose_query, register_query, register_mutation } from "/rsjs-runtime-query.js";

hydrate("querymutationsnapshot_61bcf5b7", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, ""));
    const __rsjs_query_0 = register_query({
        island: "querymutationsnapshot_61bcf5b7",
        query_id: 0,
        root,
        key: ["stock", "AAPL"],
        fetcher: "{\"body\":{\"kind\":\"json\",\"value\":\"{\\\"symbol\\\":\\\"AAPL\\\"}\"},\"credentials\":\"include\",\"headers\":[[\"Accept\",\"application/json\"],[\"X-RSJS\",\"1\"]],\"method\":\"POST\",\"response\":\"json\",\"timeout_ms\":1500,\"url\":\"/api/stocks/AAPL\"}",
        options: { interval_ms: null, refetch_on_focus: true, download_progress: true, stream_text: false, retry_count: 2, stale_time_ms: 2000 },
    });
    __rsjs_disposers.push(() => dispose_query(__rsjs_query_0));
    const __rsjs_query_1 = register_query({
        island: "querymutationsnapshot_61bcf5b7",
        query_id: 1,
        root,
        key: ["feed"],
        fetcher: "{\"headers\":[[\"Accept\",\"text/plain\"]],\"response\":\"text\",\"url\":\"/api/feed\"}",
        options: { interval_ms: null, refetch_on_focus: false, download_progress: true, stream_text: true, retry_count: 0, stale_time_ms: 0 },
    });
    __rsjs_disposers.push(() => dispose_query(__rsjs_query_1));
    const __rsjs_mutation_0 = register_mutation({
        island: "querymutationsnapshot_61bcf5b7",
        mutation_id: 0,
        root,
        key: ["update_cart", "AAPL"],
        runner: "{\"credentials\":\"omit\",\"headers\":[[\"X-Mutate\",\"1\"]],\"method\":\"PATCH\",\"response\":\"empty\",\"timeout_ms\":2500,\"url\":\"/api/cart\"}",
        invalidates: [["stock", "AAPL"], ["feed"]],
        options: { retry_count: 1, on_success_invalidate_strategy: "refetch" },
    });

    function __handler_0(event) {
        __sig_0.set((event.target?.value ?? ""));
    }

    function __handler_1(event) {
        __rsjs_mutation_0.run(__sig_0.get());
    }

    function __handler_2(event) {
        __rsjs_query_0.refetch();
    }

    function __handler_3(event) {
        __rsjs_query_0.invalidate();
    }

    const __node_2 = walk_path(root, [0, 1], "data-rsjs-bind", "2");
    __rsjs_disposers.push(bind_text(__node_2, () => rsjs.__project(__rsjs_query_0.data(), ["value"]), [], [__rsjs_query_0]));

    const __node_3 = walk_path(root, [0, 2], "data-rsjs-bind", "3");
    __rsjs_disposers.push(bind_text(__node_3, () => __rsjs_query_0.status(), [], [__rsjs_query_0]));

    const __node_4 = walk_path(root, [0, 3], "data-rsjs-bind", "4");
    __rsjs_disposers.push(bind_text(__node_4, () => __rsjs_query_0.download_loaded(), [], [__rsjs_query_0]));

    const __node_5 = walk_path(root, [0, 4], "data-rsjs-bind", "5");
    __rsjs_disposers.push(bind_text(__node_5, () => __rsjs_query_0.download_total(), [], [__rsjs_query_0]));

    const __node_6 = walk_path(root, [0, 5], "data-rsjs-bind", "6");
    __rsjs_disposers.push(bind_text(__node_6, () => __rsjs_query_0.download_percent(), [], [__rsjs_query_0], "rust-float"));

    const __node_7 = walk_path(root, [0, 6], "data-rsjs-bind", "7");
    __rsjs_disposers.push(bind_text(__node_7, () => __rsjs_query_0.fetch_count(), [], [__rsjs_query_0]));

    const __node_8 = walk_path(root, [0, 7], "data-rsjs-bind", "8");
    __rsjs_disposers.push(bind_text(__node_8, () => rsjs.__as_str(__rsjs_query_1.data()), [], [__rsjs_query_1]));

    const __node_9 = walk_path(root, [0, 8], "data-rsjs-bind", "9");
    __rsjs_disposers.push(bind_text(__node_9, () => __rsjs_mutation_0.status(), [], [__rsjs_mutation_0]));

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-attr", "1");
    __rsjs_disposers.push(bind_attr(__node_1, "value", "value", () => __sig_0.get(), [__sig_0]));

    const __node_10 = walk_path(root, [0, 9], "data-rsjs-attr", "10");
    __rsjs_disposers.push(bind_attr(__node_10, "hidden", "boolean", () => (__rsjs_mutation_0.is_pending() === false), [], [__rsjs_mutation_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([
        ["11", __handler_1],
        ["12", __handler_2],
        ["13", __handler_3],
    ])));

    __rsjs_disposers.push(bind_delegated_events(root, "input", new Map([["1", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
