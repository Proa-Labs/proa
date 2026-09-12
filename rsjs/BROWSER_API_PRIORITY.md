# rsjs browser API priority map

This is the browser-API counterpart to [`STATUS.md`](./STATUS.md). The goal is
to avoid discovering missing browser capabilities one-by-one as RSJS-analyzed
components get more serious.

The list below is ranked by API family, not by every individual interface. That
is the useful product/planning granularity: MDN currently lists 147 Web API
families, while the machine-readable interface inventories are already in the
thousands.

## Sources

| Source | Used for |
|---|---|
| [MDN Web APIs](https://developer.mozilla.org/en-US/docs/Web/API) | Human-readable denominator for Web API families and experimental/deprecated/nonstandard flags. |
| [MDN browser-compat-data](https://github.com/mdn/browser-compat-data) | Machine-readable browser compatibility data for Web APIs, JavaScript features, CSS, and more. |
| [`@webref/idl`](https://www.npmjs.com/package/@webref/idl) | Machine-readable Web IDL scraped from current web platform specs. |
| [W3C WebAuthn Level 3](https://www.w3.org/TR/webauthn-3/) | Normative shape for the WebAuthn/passkey API. |
| [WHATWG standards index](https://spec.whatwg.org/) | Normative source for core platform standards like DOM, Fetch, URL, Web IDL, WebSockets, and XMLHttpRequest. |
| [web-platform-tests](https://github.com/web-platform-tests/wpt) | Behavioral test corpus to mine when we add conformance fixtures. |
| [`web-sys` feature list](https://docs.rs/crate/web-sys/latest/features) | Rust-adjacent API/interface naming cross-check. |

Observed source-inventory sizes:

| Inventory | Count |
|---|---:|
| MDN Web API families | 147 |
| MDN BCD top-level `api.*` entries | 1,088 |
| MDN BCD `web-features:*` API groups | 353 |
| `@webref/idl` spec shortnames | 325 |
| TypeScript `lib.dom.d.ts` interfaces/dictionaries | 1,520 |
| `web-sys` feature flags | 1,706 |

## Priority tiers

| Tier | Meaning |
|---|---|
| P0 | First-class rsjs IR/runtime surface. These directly affect normal event handling, hydration, query/mutation plumbing, and controlled components. |
| P1 | Typed rsjs helper soon. These are common enough or important enough that users should not need arbitrary JS for them. |
| P2 | Framework/platform helper after core. Useful for real apps, but generally not required to make rsjs authoring feel complete. |
| P3 | Narrow typed browser helper only. Usually better as a small explicit capability than as core IR. |
| P4 | Specialized browser capability. Support when a concrete product needs it. |
| P5 | Defer or avoid direct wrapping. Deprecated, nonstandard, ad-tech/privacy-sandbox-specific, or too broad for rsjs core. |

Rule of thumb: P0 and P1 can justify new typed IR. P2 and below should usually
start as an explicit typed browser capability or application-owned runtime
adapter, then graduate only if several real components need the same shape.
Origin-free `#[rsjs::client]` helpers are for portable computation, not browser
effects.

## Near-term interpretation

The immediate gap is not "support all browser APIs." The immediate gap is:

1. Finish the P0 surface consistently: storage, fetch-backed query/mutation
   authoring, event/value coverage, DOM refs, URL/history, clipboard,
   streams/encoding.
2. Add P1 typed helpers for auth/security and serious app ergonomics:
   WebAuthn, Credential Management, Web Crypto, Permissions, File/drag-drop,
   observers, Page Visibility, Navigation, Performance, workers/channels, and
   WebSocket/SSE.
3. Keep P2+ behind an escape hatch until actual components demand them.

WebAuthn belongs in P1 because it is product-critical for modern auth and
security-sensitive enough that users should not hand-roll the browser ceremony
inside random helper JS. It depends on Credential Management, Web Crypto-ish
binary data handling, Permissions Policy constraints, secure contexts, and
server challenge/response plumbing. The first rsjs surface now covers the
client ceremony through server JSON options and credential JSON signals; the
remaining work is server-side ceremony ergonomics, policy diagnostics, and
older-browser compatibility if the Level 3 JSON helpers are unavailable.

## P0 audit

The P0 tier is the browser surface that should feel native inside an
origin-capable RSJS component.
This does not mean rsjs should expose every raw method. It means the common
authoring use cases should have typed, documented rsjs shapes and clear errors
for the rest.

| API family | Current rsjs status | What works today | Main gaps | Next action |
|---|---|---|---|---|
| Clipboard API | Mostly done | `rsjs::clipboard::write_text(...)`; paste text via `event.clipboard_text()`; async `readText()` through `read_text_into(&signal)` / `read_text_into_or(&signal, fallback)`. | Rich `ClipboardItem` reads/writes and explicit permission/error state. | Keep rich clipboard items out of core until a real component needs non-text payloads. |
| Console API | Mostly done | `console::log`, `info`, `debug`, `warn`, `error`, `table`, `time`, `time_end`, `group`, and `group_end` lower to JS. | True Rust-side variadic function stubs are impossible; the macro lowers multi-arg calls, but rust-analyzer may prefer single-arg/no-arg facade shapes. | Keep as diagnostics/debug helper surface, not production logging abstraction. |
| CSSOM view API | Partial | `window.scroll_to`, `window.scroll_by`; `window.inner_width`, `inner_height`, `scroll_x`, `scroll_y`; node-ref `scroll_into_view`, `scroll_to`, `scroll_by`, scroll/size accessors, and `getBoundingClientRect` scalar accessors; event coordinates; `on_scroll`; `on_window_resize`; `on_window_scroll`. | Visual viewport, resize/scroll observers, and richer geometry snapshots. | Add read-only helpers only where they feed signals or handler logic; avoid arbitrary layout thrash APIs by default. |
| Document Object Model (DOM) | Partial | Stable node refs, document events, custom event dispatch, title writes, text/class/style/attribute operations. | Raw `querySelector`, dynamic DOM creation/removal, ref-in-loop, broader event targets. | Keep raw traversal in application-owned controllers; expand typed node refs and diagnostics instead. |
| Encoding API | Partial | `rsjs::encoding::{utf8_bytes,utf8_string,base64_encode,base64_decode,base64url_encode,base64url_decode}` lower through typed IR. | Streaming decoders, fatal/ignore-BOM options, richer ArrayBuffer/DataView helpers, and typed binary values returned from WebAuthn/WebCrypto. | Extend the same `Bytes`/`Uint8Array` shape when WebAuthn/WebCrypto land. |
| Fetch API | Partial | Query/mutation IR, runtime, asset plumbing, async macro authoring, static `rsjs::FetchRequest` descriptors, request method/headers/body/credentials/timeout, JSON/text/bytes/empty response decoding, opt-in query download progress, opt-in streaming text for text responses, and per-instance query dehydration. Direct raw `fetch` is intentionally rejected. | Dynamic request construction, generic streaming body resources, XHR-backed upload progress if product pressure requires it, and richer ergonomic error policies. | Keep extending query/mutation descriptors before exposing any raw fetch helper. |
| History API | Mostly done | `push_state`, `replace_state`, `back`, `forward`, `go`, location reads/navigation/hash writes, query-string push/replace mutations, `on_window_popstate`, and `on_window_hashchange`. | State payloads beyond `null`, typed route helpers, and Navigation API integration. | Add richer route helpers only when routing demos outgrow current location/history primitives. |
| HTML DOM API | Partial | Input/textarea/select value reads; input checked/selection reads/writes; native validity reads/reporting/custom messages; input file count/name/type/size reads, including indexed file metadata; first selected/dropped file text reads into signals; form submit/reset; dialog methods; generic focus/blur/click. | Full `File` object handling, dataset, select options, media elements, details/summary, richer form controls. | Grow through concrete component migrations; node-ref methods are the right model. |
| Pointer events | Mostly done | Pointer event attrs for down/up/move/over/out/enter/leave, got/lost pointer capture events, document pointerdown, mouse-like coords/buttons/modifiers, pointer id/type/pressure/tilt/twist/primary/size reads, and pointer capture methods on node refs. | Coalesced/predicted event lists. | Add event-list helpers only when drawing/gesture components need them. |
| Popover API | Mostly done | `show_popover`, `hide_popover`, `toggle_popover` on `HtmlElement`/`HtmlDialogElement`; `toggle`/`beforetoggle` events with old/new state reads. | Anchor/target ergonomics and support diagnostics. | Add broader popover helpers only when catalog overlay components need them. |
| Streams API | Missing as public API | Query runtime consumes fetch responses internally. | `ReadableStream`, `WritableStream`, streaming request/response bodies, incremental decoding. | Defer until fetch/query authoring is stable; model streams as typed resources, not arbitrary JS objects. |
| Touch events | Partial | `on_touchstart`, `on_touchmove`, `on_touchend`, `on_touchcancel`; first changed-touch identifier/coordinates; changed/target/all touch counts and indexed scalar reads; normal `prevent_default()` / `stop_propagation()`. | Ergonomic list traversal over `touches`/`targetTouches`/`changedTouches`. | Keep loop traversal out of core until Client IR v3 can lower collection loops cleanly. |
| UI Events | Partial | Click/input/change/paste/focus/blur/key/mouse/pointer/submit; wheel, scroll, window resize, beforeinput, composition/IME; drag/drop with text, first-file metadata, indexed file metadata, first-file text reads into signals, and indexed `DataTransferItem` kind/type reads; toggle/beforetoggle with state reads; animation/transition event attrs and timing/name accessors; prevent-default and stop-propagation. | Richer keyboard/input details and ergonomic `DataTransferItem` traversal. | Add by component pressure; keep loop traversal for Client IR v3. |
| URL API | Partial | Location reads, assignment/replacement/reload/hash set; history URL writes; current query-string reads; query-string set/delete with either `history.replaceState` or `history.pushState`. | General URL parse/build and richer path mutation helpers. | Add broader URL helpers only when filter/search demos outgrow current-location query-string editing. |
| Web Storage API | Mostly done | `local_storage`/`session_storage` string reads, boolean/canonical full-domain `i64` decimal/finite-f64 parsing with explicit fallback semantics, writes, removals, and `on_window_storage` event reads, wrapped in `try`/`catch` where browsers can throw. Client `i64` results are BigInt-backed. | Rich JSON/object parsing and quota/security error policy. | Add richer parsing only after a concrete component needs it. |

Practical P0 order:

Recently completed: Web Storage reads, URL/location reads, basic touch events,
basic async query/mutation authoring over the query runtime, typed
encoding/base64 helpers, static rich fetch descriptors for queries and
mutations, URLSearchParams-style current query-string helpers,
toggle/beforetoggle event helpers, and window-level
popstate/hashchange/storage/resize handlers, plus async clipboard
`readText()` into signals, and CSS animation/transition event helpers.
Also completed node-ref `getBoundingClientRect` scalar geometry accessors.
Also completed form/control native constraint-validation helpers and pointer
enter/leave plus got/lost pointer-capture events. Also completed async
first selected/dropped file text reads into signals and typed storage parsing
for booleans, canonical full-domain `i64` decimals, and finite f64 values. Also
completed push-state
query-string mutation variants and indexed event collection reads for
touch/file/data-transfer payloads, query download progress plus streaming text
responses, and WebAuthn credential JSON helpers.

1. Treat the scoped pre-WebAuthn streams/collections work as complete except
   for explicitly deferred upload progress.
2. Treat upload progress as deferred unless a concrete upload component needs
   byte-accurate progress; hide any future XHR behind mutation descriptors.
3. Add an end-to-end WebAuthn server ceremony demo when an auth crate is
   chosen.

## Ranked API families

Rank is tier-first and alphabetical within tier. This is good enough for
planning; exact implementation order should still follow concrete demo/app
pressure.

| Rank | Tier | API family | MDN flags | rsjs stance |
|---:|---|---|---|---|
| 1 | P0 | [Clipboard API](https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API) | - | first-class rsjs IR/runtime |
| 2 | P0 | [Console API](https://developer.mozilla.org/en-US/docs/Web/API/Console_API) | - | first-class rsjs IR/runtime |
| 3 | P0 | [CSSOM view API](https://developer.mozilla.org/en-US/docs/Web/API/CSSOM_view_API) | - | first-class rsjs IR/runtime |
| 4 | P0 | [Document Object Model (DOM)](https://developer.mozilla.org/en-US/docs/Web/API/Document_Object_Model) | - | first-class rsjs IR/runtime |
| 5 | P0 | [Encoding API](https://developer.mozilla.org/en-US/docs/Web/API/Encoding_API) | - | first-class rsjs IR/runtime |
| 6 | P0 | [Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API) | - | first-class rsjs IR/runtime |
| 7 | P0 | [History API](https://developer.mozilla.org/en-US/docs/Web/API/History_API) | - | first-class rsjs IR/runtime |
| 8 | P0 | [HTML DOM API](https://developer.mozilla.org/en-US/docs/Web/API/HTML_DOM_API) | - | first-class rsjs IR/runtime |
| 9 | P0 | [Pointer events](https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events) | - | first-class rsjs IR/runtime |
| 10 | P0 | [Popover API](https://developer.mozilla.org/en-US/docs/Web/API/Popover_API) | - | first-class rsjs IR/runtime |
| 11 | P0 | [Streams API](https://developer.mozilla.org/en-US/docs/Web/API/Streams_API) | - | first-class rsjs IR/runtime |
| 12 | P0 | [Touch events](https://developer.mozilla.org/en-US/docs/Web/API/Touch_events) | - | first-class rsjs IR/runtime |
| 13 | P0 | [UI Events](https://developer.mozilla.org/en-US/docs/Web/API/UI_Events) | - | first-class rsjs IR/runtime |
| 14 | P0 | [URL API](https://developer.mozilla.org/en-US/docs/Web/API/URL_API) | - | first-class rsjs IR/runtime |
| 15 | P0 | [Web Storage API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Storage_API) | - | first-class rsjs IR/runtime |
| 16 | P1 | [Broadcast Channel API](https://developer.mozilla.org/en-US/docs/Web/API/Broadcast_Channel_API) | - | typed helper or typed IR soon |
| 17 | P1 | [Channel Messaging API](https://developer.mozilla.org/en-US/docs/Web/API/Channel_Messaging_API) | - | typed helper or typed IR soon |
| 18 | P1 | [Compression Streams API](https://developer.mozilla.org/en-US/docs/Web/API/Compression_Streams_API) | - | typed helper or typed IR soon |
| 19 | P1 | [Cookie Store API](https://developer.mozilla.org/en-US/docs/Web/API/Cookie_Store_API) | - | typed helper or typed IR soon |
| 20 | P1 | [Credential Management API](https://developer.mozilla.org/en-US/docs/Web/API/Credential_Management_API) | - | partial: WebAuthn JSON create/get helpers |
| 21 | P1 | [File API](https://developer.mozilla.org/en-US/docs/Web/API/File_API) | - | typed helper or typed IR soon |
| 22 | P1 | [Fullscreen API](https://developer.mozilla.org/en-US/docs/Web/API/Fullscreen_API) | - | typed helper or typed IR soon |
| 23 | P1 | [HTML Drag and Drop API](https://developer.mozilla.org/en-US/docs/Web/API/HTML_Drag_and_Drop_API) | - | typed helper or typed IR soon |
| 24 | P1 | [HTML Sanitizer API](https://developer.mozilla.org/en-US/docs/Web/API/HTML_Sanitizer_API) | - | typed helper or typed IR soon |
| 25 | P1 | [Intersection Observer API](https://developer.mozilla.org/en-US/docs/Web/API/Intersection_Observer_API) | - | typed helper or typed IR soon |
| 26 | P1 | [Keyboard API](https://developer.mozilla.org/en-US/docs/Web/API/Keyboard_API) | experimental | typed helper or typed IR soon |
| 27 | P1 | [Navigation API](https://developer.mozilla.org/en-US/docs/Web/API/Navigation_API) | - | typed helper or typed IR soon |
| 28 | P1 | [Page Visibility API](https://developer.mozilla.org/en-US/docs/Web/API/Page_Visibility_API) | - | typed helper or typed IR soon |
| 29 | P1 | [Performance APIs](https://developer.mozilla.org/en-US/docs/Web/API/Performance_API) | - | typed helper or typed IR soon |
| 30 | P1 | [Permissions API](https://developer.mozilla.org/en-US/docs/Web/API/Permissions_API) | - | typed helper or typed IR soon |
| 31 | P1 | [Resize Observer API](https://developer.mozilla.org/en-US/docs/Web/API/Resize_Observer_API) | - | typed helper or typed IR soon |
| 32 | P1 | [Selection API](https://developer.mozilla.org/en-US/docs/Web/API/Selection_API) | - | typed helper or typed IR soon |
| 33 | P1 | [Server-sent events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events) | - | typed helper or typed IR soon |
| 34 | P1 | [Storage Access API](https://developer.mozilla.org/en-US/docs/Web/API/Storage_Access_API) | - | typed helper or typed IR soon |
| 35 | P1 | [Storage API](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API) | - | typed helper or typed IR soon |
| 36 | P1 | [Trusted Types API](https://developer.mozilla.org/en-US/docs/Web/API/Trusted_Types_API) | - | typed helper or typed IR soon |
| 37 | P1 | [URL Pattern API](https://developer.mozilla.org/en-US/docs/Web/API/URL_Pattern_API) | - | typed helper or typed IR soon |
| 38 | P1 | [View Transition API](https://developer.mozilla.org/en-US/docs/Web/API/View_Transition_API) | - | typed helper or typed IR soon |
| 39 | P1 | [VirtualKeyboard API](https://developer.mozilla.org/en-US/docs/Web/API/VirtualKeyboard_API) | experimental | typed helper or typed IR soon |
| 40 | P1 | [Web Authentication API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Authentication_API) | - | typed helper or typed IR soon |
| 41 | P1 | [Web Crypto API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Crypto_API) | - | typed helper or typed IR soon |
| 42 | P1 | [Web Workers API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API) | - | typed helper or typed IR soon |
| 43 | P1 | [WebOTP API](https://developer.mozilla.org/en-US/docs/Web/API/WebOTP_API) | experimental | typed helper or typed IR soon |
| 44 | P1 | [WebSocket API (WebSockets)](https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API) | - | typed helper or typed IR soon |
| 45 | P2 | [Background Synchronization API](https://developer.mozilla.org/en-US/docs/Web/API/Background_Synchronization_API) | - | framework/platform helper after core |
| 46 | P2 | [Background Tasks API](https://developer.mozilla.org/en-US/docs/Web/API/Background_Tasks_API) | - | framework/platform helper after core |
| 47 | P2 | [Badging API](https://developer.mozilla.org/en-US/docs/Web/API/Badging_API) | - | framework/platform helper after core |
| 48 | P2 | [Beacon API](https://developer.mozilla.org/en-US/docs/Web/API/Beacon_API) | - | framework/platform helper after core |
| 49 | P2 | [Federated Credential Management (FedCM) API](https://developer.mozilla.org/en-US/docs/Web/API/FedCM_API) | experimental | framework/platform helper after core |
| 50 | P2 | [File and Directory Entries API](https://developer.mozilla.org/en-US/docs/Web/API/File_and_Directory_Entries_API) | - | framework/platform helper after core |
| 51 | P2 | [File System API](https://developer.mozilla.org/en-US/docs/Web/API/File_System_API) | - | framework/platform helper after core |
| 52 | P2 | [Geolocation API](https://developer.mozilla.org/en-US/docs/Web/API/Geolocation_API) | - | framework/platform helper after core |
| 53 | P2 | [IndexedDB API](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API) | - | framework/platform helper after core |
| 54 | P2 | [Media Capabilities API](https://developer.mozilla.org/en-US/docs/Web/API/Media_Capabilities_API) | - | framework/platform helper after core |
| 55 | P2 | [Media Capture and Streams API (Media Stream)](https://developer.mozilla.org/en-US/docs/Web/API/Media_Capture_and_Streams_API) | - | framework/platform helper after core |
| 56 | P2 | [Media Session API](https://developer.mozilla.org/en-US/docs/Web/API/Media_Session_API) | - | framework/platform helper after core |
| 57 | P2 | [MediaStream Image Capture API](https://developer.mozilla.org/en-US/docs/Web/API/MediaStream_Image_Capture_API) | - | framework/platform helper after core |
| 58 | P2 | [MediaStream Recording API](https://developer.mozilla.org/en-US/docs/Web/API/MediaStream_Recording_API) | - | framework/platform helper after core |
| 59 | P2 | [Network Information API](https://developer.mozilla.org/en-US/docs/Web/API/Network_Information_API) | - | framework/platform helper after core |
| 60 | P2 | [Notifications API](https://developer.mozilla.org/en-US/docs/Web/API/Notifications_API) | - | framework/platform helper after core |
| 61 | P2 | [Payment Request API](https://developer.mozilla.org/en-US/docs/Web/API/Payment_Request_API) | - | framework/platform helper after core |
| 62 | P2 | [Picture-in-Picture API](https://developer.mozilla.org/en-US/docs/Web/API/Picture-in-Picture_API) | - | framework/platform helper after core |
| 63 | P2 | [Prioritized Task Scheduling API](https://developer.mozilla.org/en-US/docs/Web/API/Prioritized_Task_Scheduling_API) | - | framework/platform helper after core |
| 64 | P2 | [Push API](https://developer.mozilla.org/en-US/docs/Web/API/Push_API) | - | framework/platform helper after core |
| 65 | P2 | [Screen Capture API](https://developer.mozilla.org/en-US/docs/Web/API/Screen_Capture_API) | - | framework/platform helper after core |
| 66 | P2 | [Screen Orientation API](https://developer.mozilla.org/en-US/docs/Web/API/Screen_Orientation_API) | - | framework/platform helper after core |
| 67 | P2 | [Screen Wake Lock API](https://developer.mozilla.org/en-US/docs/Web/API/Screen_Wake_Lock_API) | - | framework/platform helper after core |
| 68 | P2 | [Service Worker API](https://developer.mozilla.org/en-US/docs/Web/API/Service_Worker_API) | - | framework/platform helper after core |
| 69 | P2 | [Speculation Rules API](https://developer.mozilla.org/en-US/docs/Web/API/Speculation_Rules_API) | experimental | framework/platform helper after core |
| 70 | P2 | [Web Locks API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Locks_API) | - | framework/platform helper after core |
| 71 | P2 | [Web Share API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Share_API) | - | framework/platform helper after core |
| 72 | P2 | [Web-based Payment Handler API](https://developer.mozilla.org/en-US/docs/Web/API/Web-Based_Payment_Handler_API) | experimental | framework/platform helper after core |
| 73 | P2 | [WebRTC API](https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API) | - | framework/platform helper after core |
| 74 | P2 | [WebTransport API](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport_API) | - | framework/platform helper after core |
| 75 | P2 | [XMLHttpRequest API](https://developer.mozilla.org/en-US/docs/Web/API/XMLHttpRequest_API) | - | framework/platform helper after core |
| 76 | P3 | [Canvas API](https://developer.mozilla.org/en-US/docs/Web/API/Canvas_API) | - | narrow helper; usually not IR core |
| 77 | P3 | [CSS Custom Highlight API](https://developer.mozilla.org/en-US/docs/Web/API/CSS_Custom_Highlight_API) | - | narrow helper; usually not IR core |
| 78 | P3 | [CSS Font Loading API](https://developer.mozilla.org/en-US/docs/Web/API/CSS_Font_Loading_API) | - | narrow helper; usually not IR core |
| 79 | P3 | [CSS Object Model (CSSOM)](https://developer.mozilla.org/en-US/docs/Web/API/CSS_Object_Model) | - | narrow helper; usually not IR core |
| 80 | P3 | [CSS Properties and Values API](https://developer.mozilla.org/en-US/docs/Web/API/CSS_Properties_and_Values_API) | - | narrow helper; usually not IR core |
| 81 | P3 | [CSS Typed Object Model API](https://developer.mozilla.org/en-US/docs/Web/API/CSS_Typed_OM_API) | - | narrow helper; usually not IR core |
| 82 | P3 | [Device Memory API](https://developer.mozilla.org/en-US/docs/Web/API/Device_Memory_API) | - | narrow helper; usually not IR core |
| 83 | P3 | [Document Picture-in-Picture API](https://developer.mozilla.org/en-US/docs/Web/API/Document_Picture-in-Picture_API) | - | narrow helper; usually not IR core |
| 84 | P3 | [EditContext API](https://developer.mozilla.org/en-US/docs/Web/API/EditContext_API) | experimental | narrow helper; usually not IR core |
| 85 | P3 | [Geometry interfaces](https://developer.mozilla.org/en-US/docs/Web/API/Geometry_interfaces) | - | narrow helper; usually not IR core |
| 86 | P3 | [InputDeviceCapabilities API](https://developer.mozilla.org/en-US/docs/Web/API/InputDeviceCapabilities_API) | experimental | narrow helper; usually not IR core |
| 87 | P3 | [Invoker Commands API](https://developer.mozilla.org/en-US/docs/Web/API/Invoker_Commands_API) | - | narrow helper; usually not IR core |
| 88 | P3 | [SVG API](https://developer.mozilla.org/en-US/docs/Web/API/SVG_API) | - | narrow helper; usually not IR core |
| 89 | P3 | [URL Fragment Text Directives](https://developer.mozilla.org/en-US/docs/Web/API/URL_Fragment_Text_Directives) | - | narrow helper; usually not IR core |
| 90 | P3 | [User Preferences API](https://developer.mozilla.org/en-US/docs/Web/API/User_Preferences_API) | experimental | narrow helper; usually not IR core |
| 91 | P3 | [User-Agent Client Hints API](https://developer.mozilla.org/en-US/docs/Web/API/User-Agent_Client_Hints_API) | experimental | narrow helper; usually not IR core |
| 92 | P3 | [Vibration API](https://developer.mozilla.org/en-US/docs/Web/API/Vibration_API) | - | narrow helper; usually not IR core |
| 93 | P3 | [Web Animations API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Animations_API) | - | narrow helper; usually not IR core |
| 94 | P3 | [Web Components](https://developer.mozilla.org/en-US/docs/Web/API/Web_components) | - | narrow helper; usually not IR core |
| 95 | P3 | [WebVTT API](https://developer.mozilla.org/en-US/docs/Web/API/WebVTT_API) | - | narrow helper; usually not IR core |
| 96 | P4 | [Audio Output Devices API](https://developer.mozilla.org/en-US/docs/Web/API/Audio_Output_Devices_API) | experimental | specialized browser capability |
| 97 | P4 | [Audio Session API](https://developer.mozilla.org/en-US/docs/Web/API/Audio_Session_API) | experimental | specialized browser capability |
| 98 | P4 | [Background Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Background_Fetch_API) | experimental | specialized browser capability |
| 99 | P4 | [Barcode Detection API](https://developer.mozilla.org/en-US/docs/Web/API/Barcode_Detection_API) | experimental | specialized browser capability |
| 100 | P4 | [Battery Status API](https://developer.mozilla.org/en-US/docs/Web/API/Battery_Status_API) | - | specialized browser capability |
| 101 | P4 | [Compute Pressure API](https://developer.mozilla.org/en-US/docs/Web/API/Compute_Pressure_API) | experimental | specialized browser capability |
| 102 | P4 | [Contact Picker API](https://developer.mozilla.org/en-US/docs/Web/API/Contact_Picker_API) | experimental | specialized browser capability |
| 103 | P4 | [Content Index API](https://developer.mozilla.org/en-US/docs/Web/API/Content_Index_API) | experimental | specialized browser capability |
| 104 | P4 | [Device orientation events](https://developer.mozilla.org/en-US/docs/Web/API/Device_orientation_events) | - | specialized browser capability |
| 105 | P4 | [Device Posture API](https://developer.mozilla.org/en-US/docs/Web/API/Device_Posture_API) | experimental | specialized browser capability |
| 106 | P4 | [Encrypted Media Extensions API](https://developer.mozilla.org/en-US/docs/Web/API/Encrypted_Media_Extensions_API) | - | specialized browser capability |
| 107 | P4 | [EyeDropper API](https://developer.mozilla.org/en-US/docs/Web/API/EyeDropper_API) | experimental | specialized browser capability |
| 108 | P4 | [Gamepad API](https://developer.mozilla.org/en-US/docs/Web/API/Gamepad_API) | - | specialized browser capability |
| 109 | P4 | [Idle Detection API](https://developer.mozilla.org/en-US/docs/Web/API/Idle_Detection_API) | experimental | specialized browser capability |
| 110 | P4 | [Ink API](https://developer.mozilla.org/en-US/docs/Web/API/Ink_API) | experimental | specialized browser capability |
| 111 | P4 | [Insertable Streams for MediaStreamTrack API](https://developer.mozilla.org/en-US/docs/Web/API/Insertable_Streams_for_MediaStreamTrack_API) | experimental | specialized browser capability |
| 112 | P4 | [JS Self-Profiling API](https://developer.mozilla.org/en-US/docs/Web/API/JS_Self-Profiling_API) | experimental | specialized browser capability |
| 113 | P4 | [Launch Handler API](https://developer.mozilla.org/en-US/docs/Web/API/Launch_Handler_API) | experimental | specialized browser capability |
| 114 | P4 | [Local Font Access API](https://developer.mozilla.org/en-US/docs/Web/API/Local_Font_Access_API) | experimental | specialized browser capability |
| 115 | P4 | [Media Source API](https://developer.mozilla.org/en-US/docs/Web/API/Media_Source_Extensions_API) | - | specialized browser capability |
| 116 | P4 | [Pointer Lock API](https://developer.mozilla.org/en-US/docs/Web/API/Pointer_Lock_API) | - | specialized browser capability |
| 117 | P4 | [Presentation API](https://developer.mozilla.org/en-US/docs/Web/API/Presentation_API) | experimental | specialized browser capability |
| 118 | P4 | [Remote Playback API](https://developer.mozilla.org/en-US/docs/Web/API/Remote_Playback_API) | - | specialized browser capability |
| 119 | P4 | [Reporting API](https://developer.mozilla.org/en-US/docs/Web/API/Reporting_API) | - | specialized browser capability |
| 120 | P4 | [Sensor APIs](https://developer.mozilla.org/en-US/docs/Web/API/Sensor_APIs) | - | specialized browser capability |
| 121 | P4 | [Summarizer API](https://developer.mozilla.org/en-US/docs/Web/API/Summarizer_API) | experimental | specialized browser capability |
| 122 | P4 | [Translator and Language Detector APIs](https://developer.mozilla.org/en-US/docs/Web/API/Translator_and_Language_Detector_APIs) | experimental | specialized browser capability |
| 123 | P4 | [Viewport Segments API](https://developer.mozilla.org/en-US/docs/Web/API/Viewport_segments_API) | experimental | specialized browser capability |
| 124 | P4 | [Web Audio API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Audio_API) | - | specialized browser capability |
| 125 | P4 | [Web Bluetooth API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Bluetooth_API) | experimental | specialized browser capability |
| 126 | P4 | [Web MIDI API](https://developer.mozilla.org/en-US/docs/Web/API/Web_MIDI_API) | - | specialized browser capability |
| 127 | P4 | [Web NFC API](https://developer.mozilla.org/en-US/docs/Web/API/Web_NFC_API) | experimental | specialized browser capability |
| 128 | P4 | [Web Periodic Background Synchronization API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Periodic_Background_Synchronization_API) | experimental | specialized browser capability |
| 129 | P4 | [Web Serial API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Serial_API) | - | specialized browser capability |
| 130 | P4 | [Web Speech API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Speech_API) | - | specialized browser capability |
| 131 | P4 | [WebCodecs API](https://developer.mozilla.org/en-US/docs/Web/API/WebCodecs_API) | - | specialized browser capability |
| 132 | P4 | [WebGL: 2D and 3D graphics for the web](https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API) | - | specialized browser capability |
| 133 | P4 | [WebGPU API](https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API) | - | specialized browser capability |
| 134 | P4 | [WebHID API](https://developer.mozilla.org/en-US/docs/Web/API/WebHID_API) | experimental | specialized browser capability |
| 135 | P4 | [WebUSB API](https://developer.mozilla.org/en-US/docs/Web/API/WebUSB_API) | experimental | specialized browser capability |
| 136 | P4 | [WebXR Device API](https://developer.mozilla.org/en-US/docs/Web/API/WebXR_Device_API) | experimental | specialized browser capability |
| 137 | P4 | [Window Controls Overlay API](https://developer.mozilla.org/en-US/docs/Web/API/Window_Controls_Overlay_API) | experimental | specialized browser capability |
| 138 | P4 | [Window Management API](https://developer.mozilla.org/en-US/docs/Web/API/Window_Management_API) | experimental | specialized browser capability |
| 139 | P5 | [Attribution Reporting API](https://developer.mozilla.org/en-US/docs/Web/API/Attribution_Reporting_API) | nonstandard, deprecated | defer or do not wrap directly |
| 140 | P5 | [CSS Painting API](https://developer.mozilla.org/en-US/docs/Web/API/CSS_Painting_API) | experimental | defer or do not wrap directly |
| 141 | P5 | [Fenced Frame API](https://developer.mozilla.org/en-US/docs/Web/API/Fenced_frame_API) | experimental | defer or do not wrap directly |
| 142 | P5 | [Force Touch events](https://developer.mozilla.org/en-US/docs/Web/API/Force_Touch_events) | nonstandard | defer or do not wrap directly |
| 143 | P5 | [Houdini APIs](https://developer.mozilla.org/en-US/docs/Web/API/Houdini_APIs) | - | defer or do not wrap directly |
| 144 | P5 | [Private State Token API](https://developer.mozilla.org/en-US/docs/Web/API/Private_State_Token_API) | experimental | defer or do not wrap directly |
| 145 | P5 | [Shared Storage API](https://developer.mozilla.org/en-US/docs/Web/API/Shared_Storage_API) | nonstandard, deprecated | defer or do not wrap directly |
| 146 | P5 | [Topics API](https://developer.mozilla.org/en-US/docs/Web/API/Topics_API) | nonstandard, deprecated | defer or do not wrap directly |
| 147 | P5 | [WebVR API](https://developer.mozilla.org/en-US/docs/Web/API/WebVR_API) | nonstandard, deprecated | defer or do not wrap directly |

## First APIs to model next

After the current node-ref/browser-effect work, the highest-signal missing
pieces are:

1. Streams/fetch progress and incremental body helpers, once a concrete
   component needs them. Scope them through typed rsjs descriptors rather than
   exposing arbitrary browser stream objects.
2. Ergonomic loops over indexed browser collections once Client IR v3 loop
   lowering exists.
3. WebAuthn server challenge/response sample and compatibility fallback policy.
4. Web Crypto binary helpers needed by signed/encrypted client
   flows.
5. Permissions and Permissions Policy diagnostics for APIs that require user
   mediation or secure contexts.
6. Observer helpers: intersection, resize, page visibility, performance.
7. Worker/channel helpers for isolated long-running client work.

## Maintenance rules

When an API family moves tiers or gains support:

1. Add or update rsjs conformance fixtures for the authoring shape.
2. Update [`STATUS.md`](./STATUS.md) if it changes the public DSL surface.
3. Add a short note here explaining why the tier changed.
4. For security-sensitive APIs, link the normative spec and WPT coverage before
   adding broad sugar.
