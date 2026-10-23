// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{
    borrow::Cow,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use futures::StreamExt;
use proa_core::text::{text, StaticRaw};
use proa_core::{
    DataLoader, LoadError, LoadKey, LoadRequest, LoadValue, RenderMode, RenderOutcome,
    RequestContext, RequestLocation, RouteParam, RouteParams, SendWebContext, WebContext,
    WebRenderSend, WebRenderSync, WriteError,
};
use proa_framework_core::{
    metadata_from_context, render_metadata_head, render_metadata_head_send, CascadeValue::Set,
    ComputedRouteMeta, FrameworkBody, FrameworkRequestContext, IslandManifest, Metadata,
    MetadataBase, MetadataContext, MetadataOutcome, MetadataPatch, OpenGraphPatch,
    ResolvedMetadata, ResolvedOpenGraphImage, RouteResponse,
};
use proa_stream::DataResolver;

struct Site;
impl Metadata for Site {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        Some(MetadataPatch {
            title: Set("%s | Site".into()),
            description: Set("inherited".into()),
            open_graph: OpenGraphPatch {
                images: Set(Cow::Borrowed(&MEDIA_0)),
                ..Default::default()
            },
            ..Default::default()
        })
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

struct PostMetadata;
impl Metadata for PostMetadata {
    fn generate_metadata<'a>(
        &'a self,
        cx: MetadataContext<'a>,
        parent: &'a ResolvedMetadata,
    ) -> MetadataOutcome<'a> {
        MetadataOutcome::pending(async move {
            let slug = cx.param("slug").unwrap();
            let post = cx.load_typed::<Post>(LoadKey::named("post", slug)).await?;
            let lang = cx.request.headers()["accept-language"].to_str().unwrap();
            let query: Vec<_> = cx
                .search_params()
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            assert_eq!(
                query,
                [("tag".into(), "a b".into()), ("tag".into(), "c&d".into())]
            );
            assert_eq!(parent.description.as_deref(), Some("inherited"));
            let mut images = parent.open_graph.images.to_vec();
            images.push(ResolvedOpenGraphImage::new("/post.png"));
            Ok(MetadataPatch {
                title: Set(format!("{} {lang}", post.title).into()),
                description: Set(format!("{slug}\"<>&").into()),
                canonical: Set(format!("/blog/{slug}").into()),
                open_graph: OpenGraphPatch {
                    images: Set(Cow::Owned(images)),
                    ..Default::default()
                },
                ..Default::default()
            })
        })
    }
}

struct PostPage;
impl<L: DataLoader> proa_core::WebRender<L> for PostPage {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            // The data comes from the same request resolver used by metadata.
            let slug = cx.request().param("slug").unwrap().to_owned();
            let post = cx
                .load_typed::<Post>(LoadKey::named("post", slug))
                .await
                .unwrap();
            assert!(metadata_from_context(cx).is_some());
            WebRenderSync::render(StaticRaw("<html><head>"), cx)?;
            render_metadata_head(cx, metadata_from_context(cx).cloned().as_ref().unwrap());
            WebRenderSync::render(StaticRaw("</head><body>"), cx)?;
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
            let slug = cx.request().param("slug").unwrap().to_owned();
            let post = cx
                .load_typed::<Post>(LoadKey::named("post", slug))
                .await
                .unwrap();
            let metadata = cx
                .request()
                .extensions()
                .get::<Arc<ResolvedMetadata>>()
                .unwrap()
                .clone();
            cx.__proa_with_stateless_context(|local| StaticRaw("<html><head>").render(local))?;
            render_metadata_head_send(cx, &metadata);
            cx.__proa_with_stateless_context(|local| {
                StaticRaw("</head><body>").render(local)?;
                text(post.title.as_str()).render(local)?;
                StaticRaw("</body></html>").render(local)
            })
        })
    }
}

fn request(slug: &str, lang: &'static str) -> FrameworkRequestContext {
    let mut request = RequestContext::new()
        .with_location(RequestLocation::new(
            "/blog/post?tag=a+b&tag=c%26d".parse().unwrap(),
        ))
        .with_route_params(RouteParams::from_vec(vec![RouteParam::new(
            "slug",
            slug.to_owned().into_boxed_str(),
        )]));
    request
        .headers_mut()
        .insert("accept-language", http::HeaderValue::from_static(lang));
    request
        .extensions_mut()
        .insert(MetadataBase::new("https://staging.example/app").unwrap());
    request.extensions_mut().insert(ComputedRouteMeta::cascade(
        &ComputedRouteMeta::default(),
        &[&Site, &PostMetadata],
    ));
    FrameworkRequestContext {
        request,
        ..Default::default()
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
async fn metadata_and_page_share_data_in_every_async_delivery_mode() {
    for send in [false, true] {
        for mode in [
            RenderMode::Static,
            RenderMode::Agent,
            RenderMode::ChunkedInOrder,
            RenderMode::ChunkedOutOfOrder,
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let loader = Arc::new(DataResolver::new_typed(Arc::new(PostLoader(calls.clone()))));
            let response = if send {
                RouteResponse::ssr_async_send_with_loader(PostPage, loader).with_render_mode(mode)
            } else {
                RouteResponse::ssr_async_with_loader(PostPage, loader).with_render_mode(mode)
            };
            let html = body(
                response
                    .into_framework_response(&manifest(), &request("one", "en"))
                    .await
                    .unwrap(),
            )
            .await;
            assert!(
                html.contains("<title>post:one en | Site</title>"),
                "{mode:?}, send={send}: {html}"
            );
            assert!(
                html.contains("content=\"one&quot;&lt;&gt;&amp;\""),
                "{html}"
            );
            assert!(html.contains("href=\"https://staging.example/app/blog/one\""));
            assert_eq!(html.matches("property=\"og:image\"").count(), 2);
            assert!(html.contains("https://staging.example/app/default.png"));
            assert!(html.contains("https://staging.example/app/post.png"));
            assert_eq!(calls.load(Ordering::SeqCst), 1, "{mode:?}, send={send}");
        }
    }
}

#[tokio::test]
async fn client_payload_uses_generated_title_and_shares_loader() {
    let calls = Arc::new(AtomicUsize::new(0));
    let loader = Arc::new(DataResolver::new_typed(Arc::new(PostLoader(calls.clone()))));
    let response = RouteResponse::ssr_async_with_loader(PostPage, loader)
        .into_client_payload_framework_response(
            Some("old title".into()),
            &manifest(),
            &request("two", "fr"),
        )
        .await;
    let payload: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(payload["title"], "post:two fr | Site");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct FailingMetadata;
impl Metadata for FailingMetadata {
    fn generate_metadata<'a>(
        &'a self,
        _cx: MetadataContext<'a>,
        _parent: &'a ResolvedMetadata,
    ) -> MetadataOutcome<'a> {
        MetadataOutcome::pending(async {
            tokio::task::yield_now().await;
            Err(proa_framework_core::MetadataError::InvalidBase)
        })
    }
}

#[tokio::test]
async fn streaming_metadata_failure_precedes_every_body_chunk() {
    for send in [false, true] {
        for mode in [RenderMode::ChunkedInOrder, RenderMode::ChunkedOutOfOrder] {
            let mut request = FrameworkRequestContext::default();
            request
                .request
                .extensions_mut()
                .insert(ComputedRouteMeta::cascade(
                    &ComputedRouteMeta::default(),
                    &[&FailingMetadata],
                ));
            let response = if send {
                RouteResponse::ssr_stream_send(())
            } else {
                RouteResponse::ssr_stream(())
            }
            .with_render_mode(mode);
            let response = response
                .into_stream_framework_response(&manifest(), &request)
                .unwrap();
            let FrameworkBody::Stream(factory) = response.body else {
                panic!("expected stream")
            };
            let chunks = match factory {
                proa_framework_core::FrameworkStreamFactory::Local(make) => {
                    make().collect::<Vec<_>>().await
                }
                proa_framework_core::FrameworkStreamFactory::Send(make) => {
                    make().collect::<Vec<_>>().await
                }
            };
            assert_eq!(chunks.len(), 1);
            assert!(chunks[0].is_err());
        }
    }
}

static MEDIA_0: [ResolvedOpenGraphImage; 1] = [ResolvedOpenGraphImage::new("/default.png")];

fn manifest() -> IslandManifest {
    IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    }
}

struct ClearTitle;
impl Metadata for ClearTitle {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        Some(MetadataPatch {
            title: proa_framework_core::CascadeValue::Tombstone,
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn client_payload_does_not_restore_a_cleared_title_from_fallback() {
    let mut request = FrameworkRequestContext::default();
    request
        .request
        .extensions_mut()
        .insert(ComputedRouteMeta::cascade(
            &ComputedRouteMeta::default(),
            &[&ClearTitle],
        ));
    let response = RouteResponse::ssr(())
        .into_client_payload_framework_response(Some("stale title".into()), &manifest(), &request)
        .await;
    let payload: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert!(payload.get("title").is_none());
}

struct MetadataBoundary {
    expected: Arc<ResolvedMetadata>,
    seen: Arc<AtomicUsize>,
}
impl<L: DataLoader> proa_core::WebRender<L> for MetadataBoundary {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            tokio::task::yield_now().await;
            let current = cx
                .request()
                .extensions()
                .get::<Arc<ResolvedMetadata>>()
                .unwrap();
            assert!(Arc::ptr_eq(current, &self.expected));
            self.seen.fetch_add(1, Ordering::SeqCst);
            WebRenderSync::render(text(self.expected.title.as_deref().unwrap()), cx)
        })
    }
}

struct BoundaryPage {
    seen: Arc<AtomicUsize>,
}
impl<L: DataLoader + 'static> proa_core::WebRender<L> for BoundaryPage {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let expected = cx
            .request()
            .extensions()
            .get::<Arc<ResolvedMetadata>>()
            .unwrap()
            .clone();
        proa_core::WebRender::render(
            proa_stream::Suspense::new(
                MetadataBoundary {
                    expected,
                    seen: self.seen,
                },
                (),
            ),
            cx,
        )
    }
}

#[tokio::test]
async fn deferred_boundary_keeps_the_same_resolved_metadata_snapshot() {
    let calls = Arc::new(AtomicUsize::new(0));
    let loader = Arc::new(DataResolver::new_typed(Arc::new(PostLoader(calls.clone()))));
    let seen = Arc::new(AtomicUsize::new(0));
    let response =
        RouteResponse::ssr_stream_with_loader(BoundaryPage { seen: seen.clone() }, loader)
            .with_render_mode(RenderMode::ChunkedOutOfOrder)
            .into_stream_framework_response(&manifest(), &request("late", "en"))
            .unwrap();
    let html = body(response).await;
    assert!(html.contains("data-proa-encoding=\"base64-v1\""));
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
