import { signal, bind_text, bind_conditional, read_signal_initials, signal_initial, bind_attr, bind_delegated_events, walk_path, hydrate, dispose_all } from "/rsjs-runtime.js";

hydrate("browsereffectssnapshot_62b4041a", (root, __rsjs_disposers = []) => {
    const __rsjs_signal_initials = read_signal_initials(root);
    const __sig_0 = signal(signal_initial(__rsjs_signal_initials, 0, "filters=open"));
    const __sig_1 = signal(signal_initial(__rsjs_signal_initials, 1, "{\"challenge\":\"passkey\"}"));
    const __sig_2 = signal(signal_initial(__rsjs_signal_initials, 2, ""));

    function __handler_0(event) {
        __sig_0.set((() => { try { const __v = window.localStorage?.getItem?.(String("")); return __v == null ? String("") : __v; } catch (_) { return String(""); } })());
        console.log((() => { try { const __v = window.sessionStorage?.getItem?.(String("")); return __v == null ? String("") : __v; } catch (_) { return String(""); } })());
        console.log((() => { try { const __v = window.localStorage?.getItem?.(String("enabled")); if (__v == null) return false; const __parsed = JSON.parse(__v); return typeof __parsed === "boolean" ? __parsed : false; } catch (_) { return false; } })(), (() => { try { const __v = window.localStorage?.getItem?.(String("count")); if (__v == null) return 0n; if (!/^(?:0|-?[1-9][0-9]*)$/.test(__v)) return 0n; const __parsed = BigInt(__v); return __parsed >= -9223372036854775808n && __parsed <= 9223372036854775807n ? __parsed : 0n; } catch (_) { return 0n; } })(), (() => { try { const __v = window.sessionStorage?.getItem?.(String("ratio")); if (__v == null) return 0.0; const __parsed = JSON.parse(__v); return typeof __parsed === "number" && Number.isFinite(__parsed) ? __parsed : 0.0; } catch (_) { return 0.0; } })());
        window.scrollTo?.(Number(0), Number(100));
        window.scrollBy?.(Number(0), Number(20));
        window.print?.();
        window.location.hash = String(__sig_0.get());
        window.location.assign(String("/catalog/filters"));
        window.location.replace(String(__sig_0.get()));
        window.location.reload();
        window.history?.pushState?.(null, "", String("/catalog#filters"));
        window.history?.replaceState?.(null, "", String(((typeof window !== "undefined" && window.location) ? (window.location.pathname ?? "") : "")));
        window.history?.back?.();
        window.history?.forward?.();
        window.history?.go?.(Number(1));
        document.dispatchEvent(new CustomEvent(String("filters:changed")));
        document.dispatchEvent(new CustomEvent(String("filters:changed"), { detail: __sig_0.get() }));
        document.title = String(__sig_0.get());
        navigator.clipboard?.writeText?.(String(__sig_0.get()));
        { let __rsjs_webauthn = null; try { const __rsjs_webauthn_options = globalThis.PublicKeyCredential?.parseCreationOptionsFromJSON?.(JSON.parse(String(__sig_1.get()))); if (__rsjs_webauthn_options) { __rsjs_webauthn = globalThis.navigator?.credentials?.create?.({ publicKey: __rsjs_webauthn_options }); } } catch (_) {} if (__rsjs_webauthn) { __rsjs_webauthn.then((__cred) => { try { const __json = __cred?.toJSON?.(); if (__json == null) { __sig_2.set(""); } else { __sig_2.set(JSON.stringify(__json)); } } catch (_) { __sig_2.set(""); } }).catch(() => __sig_2.set("")); } else { __sig_2.set(""); } }
        { let __rsjs_webauthn = null; try { const __rsjs_webauthn_options = globalThis.PublicKeyCredential?.parseRequestOptionsFromJSON?.(JSON.parse(String(__sig_1.get()))); if (__rsjs_webauthn_options) { __rsjs_webauthn = globalThis.navigator?.credentials?.get?.({ publicKey: __rsjs_webauthn_options }); } } catch (_) {} if (__rsjs_webauthn) { __rsjs_webauthn.then((__cred) => { try { const __json = __cred?.toJSON?.(); if (__json == null) { __sig_2.set("unavailable"); } else { __sig_2.set(JSON.stringify(__json)); } } catch (_) { __sig_2.set("unavailable"); } }).catch(() => __sig_2.set("unavailable")); } else { __sig_2.set("unavailable"); } }
        try { window.localStorage?.setItem?.(String("filters"), String(__sig_0.get())); } catch (_) {}
        try { window.localStorage?.removeItem?.(String("filters:stale")); } catch (_) {}
        try { window.sessionStorage?.setItem?.(String("filters"), String(__sig_0.get())); } catch (_) {}
        try { window.sessionStorage?.removeItem?.(String("filters:stale")); } catch (_) {}
        try { const __url = new URL(window.location.href); __url.searchParams.set(String("filters"), String(__sig_0.get())); window.history?.replaceState?.(null, "", __url.href); } catch (_) {}
        try { const __url = new URL(window.location.href); __url.searchParams.delete(String("stale")); window.history?.replaceState?.(null, "", __url.href); } catch (_) {}
        try { const __url = new URL(window.location.href); __url.searchParams.set(String("page"), String("2")); window.history?.pushState?.(null, "", __url.href); } catch (_) {}
        try { const __url = new URL(window.location.href); __url.searchParams.delete(String("modal")); window.history?.pushState?.(null, "", __url.href); } catch (_) {}
        console.log(((__v) => { const __b = __v instanceof ArrayBuffer ? new Uint8Array(__v) : new Uint8Array(__v ?? []); let __s = ""; for (let __i = 0; __i < __b.length; __i += 32768) { __s += String.fromCharCode(...__b.subarray(__i, __i + 32768)); } const __r = btoa(__s); return __r.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, ""); })((new TextEncoder().encode(String((__sig_0.get()) ?? "")))));
    }

    const __node_1 = walk_path(root, [0, 0], "data-rsjs-bind", "1");
    __rsjs_disposers.push(bind_text(__node_1, () => ((typeof window !== "undefined" && window.location) ? (window.location.search ?? "") : ""), []));

    const __node_0 = walk_path(root, [0], "data-rsjs-attr", "0");
    __rsjs_disposers.push(bind_attr(__node_0, "data-url", "value", () => ((typeof window !== "undefined" && window.location) ? (window.location.href ?? "") : ""), []));

    __rsjs_disposers.push(bind_delegated_events(root, "click", new Map([["2", __handler_0]])));
    return () => dispose_all(__rsjs_disposers, "rsjs bindings");
});
