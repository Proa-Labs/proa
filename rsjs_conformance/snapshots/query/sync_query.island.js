import { signal, bind_text, bind_conditional, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";
import { rsjs, dispose_query, register_query } from "/rsjs-runtime-query.js";

hydrate("syncquerysnapshot_659d3b12", (root, __rsjs_disposers = []) => {
    const __rsjs_query_0 = register_query({
        island: "syncquerysnapshot_659d3b12",
        query_id: 0,
        root,
        key: ["stock", "AAPL"],
        fetcher: "/api/stocks/AAPL",
        options: { interval_ms: null, refetch_on_focus: false, download_progress: false, stream_text: false, retry_count: 0, stale_time_ms: 0 },
    });
    __rsjs_disposers.push(() => dispose_query(__rsjs_query_0));

    function __handler_0(event) {
        console.log("refresh");
        __rsjs_query_0.refetch();
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => __rsjs_query_0.data(), [], [__rsjs_query_0]));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["2", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
