import { signal, bind_text, bind_conditional, bind_attr, bind_delegated_events, walk_path, dispose_all } from "/rsjs-runtime.js";
import { rsjs, dispose_query, register_query, register_mutation } from "/rsjs-runtime-query.js";

export function mount_asyncchildcompositiondatachildsnapshot_ca62ef17(root, options = {}) {
    const __rsjs_disposers = [];
    try {
    const __rsjs_query_0 = register_query({
        island: "asyncchildcompositiondatachildsnapshot_ca62ef17",
        query_id: 0,
        root: options.seedRoot ?? root,
        key: [null],
        fetcher: "{\"credentials\":\"same-origin\",\"method\":\"GET\",\"response\":\"json\",\"url\":\"/api/child-stocks/AAPL\"}",
        options: { interval_ms: null, refetch_on_focus: false, download_progress: false, stream_text: false, retry_count: 0, stale_time_ms: 0 },
    });
    __rsjs_disposers.push(() => dispose_query(__rsjs_query_0));
    const __rsjs_mutation_0 = register_mutation({
        island: "asyncchildcompositiondatachildsnapshot_ca62ef17",
        mutation_id: 0,
        root: options.seedRoot ?? root,
        key: [null],
        runner: "{\"credentials\":\"same-origin\",\"headers\":[[\"X-Mutate\",\"1\"]],\"method\":\"PATCH\",\"response\":\"empty\",\"url\":\"/api/child-cart\"}",
        invalidates: [],
        options: { retry_count: 1, on_success_invalidate_strategy: "refetch" },
    });

    function __handler_0(event) {
        __rsjs_mutation_0.run("AAPL");
        __rsjs_query_0.refetch();
    }

    const __node_1 = walk_path(root, [0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => rsjs.__project(__rsjs_query_0.data(), ["value"]), [], [__rsjs_query_0]));

    const __node_2 = walk_path(root, [1], "data-rsjs-bind", "2");
    __rsjs_disposers.push(bind_text(__node_2, () => __rsjs_mutation_0.status(), [], [__rsjs_mutation_0]));

    __rsjs_disposers.push(bind_attr(__node_1, "data-currency", "value", () => rsjs.__project(__rsjs_query_0.data(), ["currency"]), [], [__rsjs_query_0]));

    const __node_3 = walk_path(root, [2], "data-rsjs-attr", "3");
    __rsjs_disposers.push(bind_attr(__node_3, "hidden", "boolean", () => (__rsjs_mutation_0.is_pending() === false), [], [__rsjs_mutation_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["4", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
    } catch (__rsjs_init_error) {
        dispose_all(__rsjs_disposers, "rsjs fragment init rollback");
        throw __rsjs_init_error;
    }
}
