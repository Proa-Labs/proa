import { signal, bind_text, bind_conditional, bind_attr, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";
import { rsjs, dispose_query, register_query } from "/rsjs-runtime-query.js";

hydrate("serverinitialquerysnapshot_35db9d2b", (root, __rsjs_disposers = []) => {
    const __rsjs_query_0 = register_query({
        island: "serverinitialquerysnapshot_35db9d2b",
        query_id: 0,
        root,
        key: [null],
        fetcher: "{\"credentials\":\"same-origin\",\"method\":\"GET\",\"response\":\"json\",\"url\":\"/api/stocks/AAPL\"}",
        options: { interval_ms: null, refetch_on_focus: false, download_progress: false, stream_text: false, retry_count: 0, stale_time_ms: 2000 },
    });
    __rsjs_disposers.push(() => dispose_query(__rsjs_query_0));

    function __handler_0(event) {
        console.log("refresh");
        __rsjs_query_0.refetch();
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => rsjs.__project(__rsjs_query_0.data(), ["value"]), [], [__rsjs_query_0]));

    __rsjs_disposers.push(bind_attr(__node_1, "data-currency", "value", () => rsjs.__project(__rsjs_query_0.data(), ["currency"]), [], [__rsjs_query_0]));

    __rsjs_disposers.push(bind_attr(__node_1, "data-status", "value", () => __rsjs_query_0.status(), [], [__rsjs_query_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["2", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
