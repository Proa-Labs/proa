// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use futures::StreamExt;
use proa_core::text::{text, StaticRaw};
use proa_core::{
    DataLoader, DynLoaderHandle, LoadError, LoadKey, LoadRequest, LoadValue, Metadata,
    MetadataError, RenderMode, RenderOutcome, RequestContext, RequestLocation, RouteParam,
    RouteParams, SendWebContext, WebContext, WebRender, WebRenderSend, WebRenderSendSync,
    WebRenderSync, WriteError,
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
        cx.metadata(site()).unwrap();
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
        cx.metadata(site()).unwrap();
        Ok(())
    }
}
struct Section;
impl Layout for Section {
    fn setup(&self, cx: &mut WebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title_template("%s | Section"))
            .unwrap();
        Ok(())
    }
    fn setup_send(&self, cx: &mut SendWebContext<DynLoaderHandle>) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title_template("%s | Section"))
            .unwrap();
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
            cx.metadata(page_metadata(cx.request(), &post)).unwrap();
            WebRenderSync::render(StaticRaw("<html><head><!--before-->"), cx)?;
            WebRenderSync::render(cx.metadata.head(), cx)?;
            WebRenderSync::render(StaticRaw("<!--after--></head><body>"), cx)?;
            assert_eq!(
                cx.metadata(Metadata::new().clear_title().description("late")),
                Err(MetadataError::Closed)
            );
            assert_eq!(cx.metadata_rejections(), 1);
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
            cx.metadata(page_metadata(cx.request(), &post)).unwrap();
            raw_send(cx, "<html><head><!--before-->")?;
            WebRenderSendSync::render_send_sync(cx.metadata.head(), cx)?;
            raw_send(cx, "<!--after--></head><body>")?;
            assert_eq!(
                cx.metadata(Metadata::new().clear_title().description("late")),
                Err(MetadataError::Closed)
            );
            assert_eq!(cx.metadata_rejections(), 1);
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
            assert!(html.contains("og:title\" content=\"post:one en | Section\""));
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
            let previous_rejections = cx.metadata_rejections();
            assert_eq!(
                cx.metadata(Metadata::new().title("intruder")),
                Err(MetadataError::Ineligible)
            );
            assert_eq!(
                WebRenderSync::render(cx.metadata.head(), cx),
                Err(WriteError::Metadata(MetadataError::Ineligible))
            );
            assert_eq!(cx.metadata_rejections(), previous_rejections + 2);
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
            let previous_rejections = cx.metadata_rejections();
            assert_eq!(
                cx.metadata(Metadata::new().title("intruder")),
                Err(MetadataError::Ineligible)
            );
            assert_eq!(
                cx.metadata.head().render_send_sync(cx),
                Err(WriteError::Metadata(MetadataError::Ineligible))
            );
            assert_eq!(cx.metadata_rejections(), previous_rejections + 2);
            raw_send(cx, "<p>branch</p>")
        })
    }
}
struct Fallback;
impl<L: DataLoader> WebRenderSync<L> for Fallback {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        assert_eq!(
            cx.metadata(Metadata::new().title("fallback")),
            Err(MetadataError::Ineligible)
        );
        assert_eq!(
            WebRenderSync::render(cx.metadata.head(), cx),
            Err(WriteError::Metadata(MetadataError::Ineligible))
        );
        WebRenderSync::render(StaticRaw("<p>fallback</p>"), cx)
    }
}
impl<L: DataLoader> WebRenderSend<L> for Fallback {
    fn render_send(
        self,
        cx: &mut SendWebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        assert_eq!(
            cx.metadata(Metadata::new().title("fallback")),
            Err(MetadataError::Ineligible)
        );
        assert_eq!(
            cx.metadata.head().render_send_sync(cx),
            Err(WriteError::Metadata(MetadataError::Ineligible))
        );
        let result = raw_send(cx, "<p>fallback</p>");
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
            cx.metadata(Metadata::new().title("owner")).unwrap();
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
            cx.metadata(Metadata::new().title("owner")).unwrap();
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
        cx.metadata(Metadata::new().title("failed document"))
            .unwrap();
        WebRenderSync::render(cx.metadata.head(), cx)?;
        Err(WriteError::OutOfCapacity)
    }
}
struct ReplacementPage;
impl WebRenderSync for ReplacementPage {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.metadata(Metadata::new().title("replacement")).unwrap();
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

struct Fragment(Option<&'static str>);
impl WebRenderSync for Fragment {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        if let Some(title) = self.0 {
            cx.metadata(Metadata::new().title(title)).unwrap();
        }
        WebRenderSync::render(cx.metadata.head(), cx)?;
        assert_eq!(cx.metadata_rejections(), 0);
        WebRenderSync::render(StaticRaw("<p>fragment</p>"), cx)
    }
}
#[tokio::test]
async fn client_payloads_report_the_resolved_title_without_head_tags() {
    for (declared, expected) in [
        (Some("Post"), "Post | Section"),
        (None, "compatibility title | Section"),
    ] {
        let response = RouteResponse::ssr(Fragment(declared))
            .into_client_payload_framework_response(None, &manifest(), &request())
            .await;
        let json: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(json["html"], "<p>fragment</p>");
        assert_eq!(json["title"], expected);
    }
}

struct SlowBody(Arc<tokio::sync::Notify>);
impl WebRender for SlowBody {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.metadata(Metadata::new().title("head before slow body"))
                .unwrap();
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
            cx.metadata(Metadata::new().title("head before slow body"))
                .unwrap();
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

struct LateDeclaration(Option<Arc<tokio::sync::Notify>>);
impl WebRender for LateDeclaration {
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.metadata(Metadata::new().title("initial"))?;
            WebRenderSync::render(cx.metadata.head(), cx)?;
            if let Some(release) = self.0 {
                cx.flush_stream_chunk()?;
                release.notified().await;
            }
            cx.metadata(Metadata::new().title("late"))?;
            Ok(())
        })
    }
}
impl WebRenderSend for LateDeclaration {
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move {
            cx.metadata(Metadata::new().title("initial"))?;
            cx.metadata.head().render_send_sync(cx)?;
            if let Some(release) = self.0 {
                cx.flush_stream_chunk()?;
                release.notified().await;
            }
            cx.metadata(Metadata::new().title("late"))?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn metadata_errors_fail_buffered_renders_and_end_committed_streams() {
    use proa_framework_core::{FrameworkError, FrameworkStreamFactory};
    for send in [false, true] {
        let page = LateDeclaration(None);
        let response = if send {
            RouteResponse::ssr_async_send(page)
        } else {
            RouteResponse::ssr_async(page)
        };
        let error = response
            .into_framework_response(&manifest(), &FrameworkRequestContext::default())
            .await
            .err()
            .expect("late metadata must fail rendering");
        assert!(matches!(
            error,
            FrameworkError::Render(WriteError::Metadata(MetadataError::Closed))
        ));
        assert!(!WriteError::Metadata(MetadataError::Closed).is_boundary_catchable());
        let release = Arc::new(tokio::sync::Notify::new());
        let page = LateDeclaration(Some(release.clone()));
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
            Box<dyn futures::Stream<Item = Result<bytes::Bytes, FrameworkError>>>,
        > = match response.body {
            FrameworkBody::Stream(FrameworkStreamFactory::Local(make)) => Box::pin(make()),
            FrameworkBody::Stream(FrameworkStreamFactory::Send(make)) => Box::pin(make()),
            _ => panic!("expected stream"),
        };
        assert_eq!(
            &stream.next().await.unwrap().unwrap()[..],
            b"<title>initial</title>"
        );
        release.notify_one();
        let error = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(
            error,
            FrameworkError::Render(WriteError::Metadata(MetadataError::Closed))
        ));
        assert!(stream.next().await.is_none());
    }
}
