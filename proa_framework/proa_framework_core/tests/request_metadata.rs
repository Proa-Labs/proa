// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use futures::StreamExt;
use proa_core::text::{text, StaticRaw};
use proa_core::{
    DataLoader, DynLoaderHandle, LoadError, LoadKey, LoadRequest, LoadValue, Metadata, RenderMode,
    RenderOutcome, RequestContext, RequestLocation, RouteParam, RouteParams, SendWebContext,
    WebContext, WebRender, WebRenderSend, WebRenderSendSync, WebRenderSync, WriteError,
};
use proa_framework_core::{
    FrameworkBody, FrameworkRequestContext, IslandManifest, Layout, RouteResponse, RouteSetupPlan,
};
use proa_stream::{DataResolver, Suspense};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

struct Site;
fn site() -> Metadata {
    Metadata::new()
        .title("Site")
        .title_template("%s | Site")
        .description("inherited")
        .open_graph(|og| {
            og.site_name("Site");
            og.image("https://example.com/default.png");
        })
}
impl Layout for Site {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        assert_eq!(cx.param("slug"), Some("one"));
        assert_eq!(
            cx.response().streaming_started(),
            matches!(
                cx.render_mode(),
                RenderMode::ChunkedInOrder | RenderMode::ChunkedOutOfOrder
            )
        );
        cx.metadata(site());
        Ok(())
    }
    fn setup_send(&self, cx: &mut SendWebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        assert_eq!(cx.param("slug"), Some("one"));
        assert_eq!(
            cx.response().streaming_started(),
            matches!(
                cx.render_mode(),
                RenderMode::ChunkedInOrder | RenderMode::ChunkedOutOfOrder
            )
        );
        cx.metadata(site());
        Ok(())
    }
}
struct Section;
impl Layout for Section {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title_template("%s | Section"));
        Ok(())
    }
    fn setup_send(&self, cx: &mut SendWebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title_template("%s | Section"));
        Ok(())
    }
}
struct Post {
    title: String,
}
struct PostLoader(Arc<AtomicUsize>);
impl DataLoader for PostLoader {
    async fn load(&self, request: &LoadRequest) -> Result<LoadValue, LoadError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        Ok(LoadValue::typed(Post {
            title: request.key().to_string(),
        }))
    }
}
fn page_metadata(request: &RequestContext, post: &Post) -> Metadata {
    let lang = request.headers()["accept-language"].to_str().unwrap();
    let query = request.location().unwrap().query().unwrap();
    Metadata::new()
        .title(format!("{} {lang}", post.title))
        .description(query.to_owned())
        .canonical("https://staging.example/blog/one")
        .open_graph(|og| {
            og.image("https://staging.example/post.png");
        })
}
struct PostPage;
impl<L: DataLoader> WebRender<L> for PostPage {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let key = LoadKey::named("post", cx.param("slug").unwrap());
            let post = cx.load_typed::<Post>(key).await?;
            cx.metadata(page_metadata(cx.request(), &post));
            WebRenderSync::render(StaticRaw("<html><head><!--before-->"), cx)?;
            WebRenderSync::render(cx.metadata.head(), cx)?;
            WebRenderSync::render(StaticRaw("<!--after--></head><body>"), cx)?;
            cx.metadata(Metadata::new().clear_title().description("late"));
            assert_eq!(cx.metadata.rejections(), 1);
            WebRenderSync::render(text(post.title.as_str()), cx)?;
            WebRenderSync::render(StaticRaw("</body></html>"), cx)
        })
    }
}
impl<L: DataLoader> WebRenderSend<L> for PostPage {
    fn render_send(
        self,
        cx: &mut SendWebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            let key = LoadKey::named("post", cx.param("slug").unwrap());
            let post = cx.load_typed::<Post>(key).await?;
            cx.metadata(page_metadata(cx.request(), &post));
            raw_send(cx, "<html><head><!--before-->")?;
            WebRenderSendSync::render_send_sync(cx.metadata.head(), cx)?;
            raw_send(cx, "<!--after--></head><body>")?;
            cx.metadata(Metadata::new().clear_title().description("late"));
            assert_eq!(cx.metadata.rejections(), 1);
            post.title.as_str().render_send_sync(cx)?;
            raw_send(cx, "</body></html>")
        })
    }
}
fn request() -> FrameworkRequestContext {
    let mut request = RequestContext::new()
        .with_location(RequestLocation::new(
            "/blog/one?tag=a&tag=b".parse().unwrap(),
        ))
        .with_route_params(RouteParams::from_vec(vec![RouteParam::new("slug", "one")]));
    request
        .headers_mut()
        .insert("accept-language", http::HeaderValue::from_static("en"));
    request.extensions_mut().insert(RouteSetupPlan::new(
        &[&Site, &Section],
        Some("compatibility title"),
    ));
    FrameworkRequestContext {
        request,
        ..Default::default()
    }
}
fn manifest() -> IslandManifest {
    IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    }
}
async fn body(response: proa_framework_core::FrameworkResponse) -> String {
    let bytes = match response.body {
        FrameworkBody::Full(bytes) => bytes.to_vec(),
        FrameworkBody::BufferedPages(buffer) => buffer.into_vec(),
        FrameworkBody::Stream(proa_framework_core::FrameworkStreamFactory::Local(make)) => make()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .flat_map(Result::unwrap)
            .collect(),
        FrameworkBody::Stream(proa_framework_core::FrameworkStreamFactory::Send(make)) => make()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .flat_map(Result::unwrap)
            .collect(),
    };
    String::from_utf8(bytes).unwrap()
}
#[tokio::test]
async fn data_is_loaded_once_head_is_immediate_and_late_writes_fail_in_all_modes() {
    for send in [false, true] {
        for mode in [
            RenderMode::Static,
            RenderMode::Agent,
            RenderMode::ChunkedInOrder,
            RenderMode::ChunkedOutOfOrder,
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let loader = DataResolver::request_scope(Arc::new(PostLoader(calls.clone())));
            let response = if send {
                RouteResponse::ssr_async_send_with_loader(PostPage, loader).with_render_mode(mode)
            } else {
                RouteResponse::ssr_async_with_loader(PostPage, loader).with_render_mode(mode)
            };
            let html = body(
                response
                    .into_framework_response(&manifest(), &request())
                    .await
                    .unwrap(),
            )
            .await;
            assert!(
                html.contains("<!--before--><title>post:one en | Section</title>"),
                "{mode:?}, send={send}: {html}"
            );
            assert!(html.contains("content=\"tag=a&amp;tag=b\""));
            assert!(html.contains("href=\"https://staging.example/blog/one\""));
            assert_eq!(html.matches("property=\"og:image\"").count(), 1);
            assert!(!html.contains("default.png"));
            assert!(html.contains("og:site_name\" content=\"Site\""));
            assert!(!html.contains("og:title"));
            assert!(!html.contains("late"));
            assert!(html.find("</title>").unwrap() < html.find("<!--after-->").unwrap());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
}

struct Intruder(Duration);
impl<L: DataLoader> WebRender<L> for Intruder {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::time::sleep(self.0).await;
            let previous_rejections = cx.metadata.rejections();
            cx.metadata(Metadata::new().title("intruder"));
            WebRenderSync::render(cx.metadata.head(), cx)?;
            assert_eq!(cx.metadata.rejections(), previous_rejections + 2);
            WebRenderSync::render(StaticRaw("<p>branch</p>"), cx)
        })
    }
}
impl<L: DataLoader> WebRenderSend<L> for Intruder {
    fn render_send(
        self,
        cx: &mut SendWebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            tokio::time::sleep(self.0).await;
            let previous_rejections = cx.metadata.rejections();
            cx.metadata(Metadata::new().title("intruder"));
            cx.metadata.head().render_send_sync(cx)?;
            assert_eq!(cx.metadata.rejections(), previous_rejections + 2);
            raw_send(cx, "<p>branch</p>")
        })
    }
}
struct Fallback;
impl<L: DataLoader> WebRenderSync<L> for Fallback {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("fallback"));
        WebRenderSync::render(cx.metadata.head(), cx)?;
        WebRenderSync::render(StaticRaw("<p>fallback</p>"), cx)
    }
}
impl<L: DataLoader> WebRenderSend<L> for Fallback {
    fn render_send(
        self,
        cx: &mut SendWebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        cx.metadata(Metadata::new().title("fallback"));
        let result = cx
            .metadata
            .head()
            .render_send_sync(cx)
            .and_then(|()| raw_send(cx, "<p>fallback</p>"));
        RenderOutcome::<std::future::Ready<_>>::Done(result)
    }
}
struct BranchPage(bool);
impl<L: DataLoader + 'static> WebRender<L> for BranchPage {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.metadata(Metadata::new().title("owner"));
            for delay in if self.0 { [0, 3] } else { [3, 0] } {
                proa_core::render_child(
                    cx,
                    Suspense::new(Intruder(Duration::from_millis(delay)), Fallback),
                )
                .await?;
            }
            WebRenderSync::render(cx.metadata.head(), cx)
        })
    }
}
impl<L: DataLoader + 'static> WebRenderSend<L> for BranchPage {
    fn render_send(
        self,
        cx: &mut SendWebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            cx.metadata(Metadata::new().title("owner"));
            for delay in if self.0 { [0, 3] } else { [3, 0] } {
                proa_core::render_send_child(
                    cx,
                    Suspense::new(Intruder(Duration::from_millis(delay)), Fallback),
                )
                .await?;
            }
            cx.metadata.head().render_send_sync(cx)
        })
    }
}
#[tokio::test]
async fn suspense_children_and_fallbacks_cannot_declare_or_emit_even_before_owner_head() {
    for mode in [
        RenderMode::Static,
        RenderMode::ChunkedInOrder,
        RenderMode::ChunkedOutOfOrder,
    ] {
        for send in [false, true] {
            for reverse in [false, true] {
                let response = if send {
                    RouteResponse::ssr_async_send(BranchPage(reverse)).with_render_mode(mode)
                } else {
                    RouteResponse::ssr_async(BranchPage(reverse)).with_render_mode(mode)
                };
                let html = body(
                    response
                        .into_framework_response(&manifest(), &FrameworkRequestContext::default())
                        .await
                        .unwrap(),
                )
                .await;
                assert_eq!(
                    html.matches("<title>").count(),
                    1,
                    "{mode:?}, send={send}: {html}"
                );
                assert!(html.contains("<title>owner</title>"));
                assert!(!html.contains("intruder"));
            }
        }
    }
}

fn raw_send<L: DataLoader>(
    cx: &mut SendWebContext<L>,
    value: &'static str,
) -> Result<(), WriteError> {
    cx.__proa_with_stateless_context(|local| WebRenderSync::render(StaticRaw(value), local))
}

struct FailingPage;
impl WebRenderSync for FailingPage {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("failed document"));
        WebRenderSync::render(cx.metadata.head(), cx)?;
        Err(WriteError::OutOfCapacity)
    }
}
struct ReplacementPage;
impl WebRenderSync for ReplacementPage {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("replacement"));
        WebRenderSync::render(cx.metadata.head(), cx)
    }
}
#[tokio::test]
async fn failed_buffered_documents_discard_metadata_and_replacement_uses_fresh_state() {
    let failed = RouteResponse::ssr(FailingPage)
        .into_framework_response(&manifest(), &FrameworkRequestContext::default())
        .await;
    assert!(failed.is_err());
    let replacement = RouteResponse::ssr(ReplacementPage)
        .into_framework_response(&manifest(), &FrameworkRequestContext::default())
        .await
        .unwrap();
    assert_eq!(body(replacement).await, "<title>replacement</title>");
}

struct Fragment;
impl WebRenderSync for Fragment {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("no ambient head"));
        WebRenderSync::render(cx.metadata.head(), cx)?;
        assert_eq!(cx.metadata.rejections(), 2);
        WebRenderSync::render(StaticRaw("<p>fragment</p>"), cx)
    }
}
#[tokio::test]
async fn fragments_do_not_run_document_layouts_or_generate_head_patches() {
    let mut request = FrameworkRequestContext::default();
    // Site would fail its request assertions if fragment finalization ran setup.
    request
        .request
        .extensions_mut()
        .insert(RouteSetupPlan::new(&[&Site], None));
    let response = RouteResponse::ssr(Fragment)
        .into_client_payload_framework_response(None, &manifest(), &request)
        .await;
    let json: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(json["html"], "<p>fragment</p>");
    assert!(json.get("title").is_none());
}

struct SlowBody(Arc<tokio::sync::Notify>);
impl WebRender for SlowBody {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.metadata(Metadata::new().title("head before slow body"));
            WebRenderSync::render(cx.metadata.head(), cx)?;
            cx.flush_stream_chunk()?;
            self.0.notified().await;
            WebRenderSync::render(StaticRaw("<p>slow body</p>"), cx)
        })
    }
}
impl WebRenderSend for SlowBody {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            cx.metadata(Metadata::new().title("head before slow body"));
            cx.metadata.head().render_send_sync(cx)?;
            cx.flush_stream_chunk()?;
            self.0.notified().await;
            raw_send(cx, "<p>slow body</p>")
        })
    }
}
#[tokio::test]
async fn head_can_be_delivered_while_unrelated_body_data_is_still_pending() {
    for send in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let page = SlowBody(release.clone());
        let response = if send {
            RouteResponse::ssr_stream_send(page)
        } else {
            RouteResponse::ssr_stream(page)
        }
        .with_render_mode(RenderMode::ChunkedInOrder);
        let response = response
            .into_framework_response(&manifest(), &FrameworkRequestContext::default())
            .await
            .unwrap();
        let mut stream: std::pin::Pin<
            Box<
                dyn futures::Stream<
                    Item = Result<bytes::Bytes, proa_framework_core::FrameworkError>,
                >,
            >,
        > = match response.body {
            FrameworkBody::Stream(proa_framework_core::FrameworkStreamFactory::Local(make)) => {
                Box::pin(make())
            }
            FrameworkBody::Stream(proa_framework_core::FrameworkStreamFactory::Send(make)) => {
                Box::pin(make())
            }
            _ => panic!("expected streaming response"),
        };
        let first = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(&first[..], b"<title>head before slow body</title>");
        release.notify_one();
        let rest: Vec<_> = stream.collect().await;
        assert!(rest
            .into_iter()
            .flat_map(Result::unwrap)
            .collect::<Vec<_>>()
            .windows(b"<p>slow body</p>".len())
            .any(|bytes| bytes == b"<p>slow body</p>"));
    }
}
