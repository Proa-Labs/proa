// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Same-source complete-request timing and allocation probe.
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::get,
    Router,
};
use proa_core::{
    ComponentRole, HeadConfig, HeadUrlPolicy, SyncHeadOperation, WebContext, WebRenderSync,
    WriteError,
};
use proa_framework_axum::{IslandManifest, Route, RouteContextLayer, RouteResponse};
use proa_macros::html_sync;
use sha2::{Digest, Sha256};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};
use tower::ServiceExt;
struct Counter;
static TRACK: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(l.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(n, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Counter = Counter;
struct DynamicBody;
impl WebRenderSync for DynamicBody {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let params = cx.route_params().cloned().unwrap_or_default();
        let id = params.get("id").unwrap();
        html_sync! {
            <h1>{id}</h1>
        }.render(cx)
    }
}
struct Doc;
impl WebRenderSync for Doc {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
        #[cfg(feature = "bulk")]
        {
            cx.head_mut().metadata(proa_core::HeadMetadata {
                title_template: Some("%s | Shop".into()),
                description: Some("Layout description".into()),
                ..Default::default()
            })?;
        }
        #[cfg(not(feature = "bulk"))]
        {
            cx.head_mut().title_template("%s | Shop")?;
            cx.head_mut().description("Layout description")?;
        }
        Ok(())
    }
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let head = cx.head();
        html_sync! {
            <html>
                <head>{head}</head>
                <body>{Page}</body>
            </html>
        }.render(cx)
    }
}
struct Page;
impl WebRenderSync for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
        #[cfg(feature = "bulk")]
        {
            cx.head_mut().metadata(proa_core::HeadMetadata {
                title: Some("Trail & Travel".into()),
                description: Some("Built for the outdoors.".into()),
                canonical: Some("/products/42".into()),
                ..Default::default()
            })?;
        }
        #[cfg(not(feature = "bulk"))]
        {
            cx.head_mut().title("Trail & Travel")?;
            cx.head_mut().description("Built for the outdoors.")?;
            cx.head_mut().canonical("/products/42")?;
        }
        Ok(())
    }
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <main>
                <h1>"Trail & Travel"</h1>
            </main>
        }.render(cx)
    }
}
fn app(case: &str) -> Router {
    let routes = match case {
        "empty_raw" => Router::new().route("/route", get(|| async { "ok" })),
        "empty_spec" => Route::endpoint("/route", get(|| async { "ok" })).into_router(),
        "dynamic_spec" => Route::endpoint(
            "/products/:id",
            get(|| async { RouteResponse::ssr(DynamicBody) }),
        )
        .into_router(),
        "managed" => Router::new().route("/route", get(|| async { RouteResponse::ssr(Doc) })),
        _ => panic!("unknown case"),
    };
    let manifest = Arc::new(IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: Default::default(),
    });
    let layer = RouteContextLayer::new(manifest, Arc::new(Default::default()));
    let layer = if case == "managed" {
        layer.with_head(
            HeadConfig::default()
                .with_urls(HeadUrlPolicy::new("https://shop.example", "/").unwrap()),
        )
    } else {
        layer
    };
    routes.layer(layer)
}
async fn request(app: &Router, path: &str) -> axum::body::Bytes {
    let response = black_box(app.clone())
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    to_bytes(response.into_body(), usize::MAX).await.unwrap()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let case = args.get(1).map(String::as_str).unwrap_or("managed");
    let n: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(100_000);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async{
  let app=app(case);let path=if case=="dynamic_spec"{"/products/42"}else{"/route"};
  for _ in 0..250{black_box(request(&app,path).await);}
  CALLS.store(0,Ordering::Relaxed);BYTES.store(0,Ordering::Relaxed);TRACK.store(true,Ordering::Relaxed);
  let html=request(&app,path).await;TRACK.store(false,Ordering::Relaxed);
  let calls=CALLS.load(Ordering::Relaxed);let bytes=BYTES.load(Ordering::Relaxed);
  let hash=format!("{:x}",Sha256::digest(&html));let output=String::from_utf8(html.to_vec()).unwrap();let len=html.len();drop(html);
  let start=Instant::now();for _ in 0..n{black_box(request(black_box(&app),black_box(path)).await);}let ns=start.elapsed().as_nanos()as f64/n as f64;
  println!("{}",serde_json::json!({"case":case,"iterations":n,"ns_per_request":ns,"allocations":calls,"allocated_bytes":bytes,"output_bytes":len,"sha256":hash,"output":output,"bulk":cfg!(feature="bulk")}));
 });
}
