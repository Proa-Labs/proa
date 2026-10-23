// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use proa_core::NoLoader;
use std::sync::atomic::{AtomicUsize, Ordering};
use CascadeValue::{Set, Tombstone};

fn resolve(
    providers: &[MetadataRef],
    request: &RequestContext,
) -> Result<ResolvedMetadata, MetadataError> {
    ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), providers)
        .resolve_sync(request, &NoLoader)
}

struct Template;
impl Metadata for Template {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        Some(MetadataPatch {
            title: Set("%s | Site".into()),
            description: Set("site description".into()),
            open_graph: OpenGraphPatch {
                title: Set("%s OG".into()),
                description: Set("%s OG".into()),
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

struct RequestTitle;
impl Metadata for RequestTitle {
    fn metadata(&self, cx: &dyn MetadataContext, parent: &ResolvedMetadata) -> MetadataPatch {
        assert_eq!(parent.description.as_deref(), Some("site description"));
        MetadataPatch {
            title: Set(cx.param("slug").unwrap().to_owned().into()),
            description: Set(cx.request().user_agent().unwrap().to_owned().into()),
            ..Default::default()
        }
    }
}

#[test]
fn request_providers_are_not_called_at_startup_and_receive_fresh_inputs() {
    let providers: &[MetadataRef] = &[&Template, &RequestTitle];
    let plan = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), providers);
    assert!(!plan.is_static());
    assert!(plan.title.is_none());
    for slug in ["first", "second"] {
        let mut request =
            RequestContext::new().with_route_params(proa_core::RouteParams::from_vec(vec![
                proa_core::RouteParam::new("slug", slug),
            ]));
        request
            .headers_mut()
            .insert("user-agent", http::HeaderValue::from_static("crawler"));
        let resolved = plan.resolve_sync(&request, &NoLoader).unwrap();
        assert_eq!(
            resolved.title.as_deref(),
            Some(format!("{slug} | Site").as_str())
        );
        assert_eq!(
            resolved.open_graph.title.as_deref(),
            Some(format!("{slug} OG").as_str())
        );
        assert_eq!(
            resolved.open_graph.description.as_deref(),
            Some("crawler OG")
        );
    }
}

struct CountStatic(AtomicUsize);
impl Metadata for CountStatic {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(MetadataPatch {
            title: Set("static".into()),
            ..Default::default()
        })
    }
}
static COUNT_STATIC: CountStatic = CountStatic(AtomicUsize::new(0));

#[test]
fn static_prefix_runs_once_and_borrows_literal_values() {
    let before = COUNT_STATIC.0.load(Ordering::SeqCst);
    let plan = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), &[&COUNT_STATIC]);
    assert!(plan.is_static());
    for _ in 0..2 {
        let result = plan
            .resolve_sync(&RequestContext::new(), &NoLoader)
            .unwrap();
        assert!(matches!(result.title, Some(Cow::Borrowed("static"))));
    }
    assert_eq!(COUNT_STATIC.0.load(Ordering::SeqCst), before + 1);
}

struct ClearTemplates;
impl Metadata for ClearTemplates {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        Some(MetadataPatch {
            title: Tombstone,
            open_graph: OpenGraphPatch {
                title: Tombstone,
                description: Tombstone,
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

#[test]
fn tombstones_remove_templates_and_prevent_fallback_resurrection() {
    let value = resolve(
        &[&Template, &ClearTemplates, &CHILD],
        &RequestContext::new(),
    )
    .unwrap();
    assert_eq!(value.title.as_deref(), Some("child"));
    assert!(value.open_graph.title.is_none());
    assert!(value.open_graph.description.is_none());
}

struct Absolute;
impl Metadata for Absolute {
    fn metadata(&self, _: &dyn MetadataContext, _: &ResolvedMetadata) -> MetadataPatch {
        MetadataPatch {
            title: Set("100% specific %s".into()),
            absolute_title: true,
            ..Default::default()
        }
    }
}

#[test]
fn absolute_titles_ignore_templates_even_when_containing_placeholder() {
    let value = resolve(&[&Template, &Absolute], &RequestContext::new()).unwrap();
    assert_eq!(value.title.as_deref(), Some("100% specific %s"));
    let value = resolve(&[&Template, &Absolute, &LEAF], &RequestContext::new()).unwrap();
    assert_eq!(value.title.as_deref(), Some("leaf"));
}

struct Lists;
impl Metadata for Lists {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        Some(MetadataPatch {
            open_graph: OpenGraphPatch {
                images: Set(Cow::Borrowed(&MEDIA_0)),
                audio: Set(Cow::Borrowed(&MEDIA_1)),
                locale_alternates: Set(Cow::Borrowed(&[Cow::Borrowed("en_US")])),
                ..Default::default()
            },
            ..Default::default()
        })
    }
}
struct ReplaceLists;
impl Metadata for ReplaceLists {
    fn metadata(&self, _: &dyn MetadataContext, _: &ResolvedMetadata) -> MetadataPatch {
        MetadataPatch {
            open_graph: OpenGraphPatch {
                images: Set(Cow::Borrowed(&MEDIA_2)),
                audio: Set(Cow::Borrowed(&[])),
                locale_alternates: Tombstone,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

#[test]
fn lists_replace_and_empty_sets_and_tombstones_clear() {
    let value = resolve(&[&Lists, &ReplaceLists], &RequestContext::new()).unwrap();
    assert_eq!(value.open_graph.images.len(), 1);
    assert_eq!(value.open_graph.images[0].url, "https://example.com/new");
    assert!(value.open_graph.audio.is_empty());
    assert!(value.open_graph.locale_alternates.is_empty());
    assert!(matches!(value.open_graph.images, Cow::Borrowed(_)));
}

struct RelativeUrls;
impl Metadata for RelativeUrls {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        Some(MetadataPatch {
            canonical: Set("/blog/post?q=one&lang=en".into()),
            open_graph: OpenGraphPatch {
                url: Set("blog/post".into()),
                images: Set(Cow::Borrowed(&MEDIA_3)),
                audio: Set(Cow::Borrowed(&MEDIA_4)),
                video: Set(Cow::Borrowed(&MEDIA_5)),
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

#[test]
fn url_resolution_uses_deployment_base_or_request_origin_and_mount_path() {
    let plan = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), &[&RelativeUrls]);
    for host in ["staging.example", "production.example"] {
        let request = RequestContext::new()
            .with_location(proa_core::RequestLocation::new(
                format!("https://{host}/blog/post").parse().unwrap(),
            ))
            .with_base_path(proa_core::BasePath::new("/app").unwrap());
        let value = plan.resolve_sync(&request, &NoLoader).unwrap();
        assert_eq!(
            value.canonical.as_deref(),
            Some(format!("https://{host}/app/blog/post?q=one&lang=en").as_str())
        );
        assert_eq!(
            value.open_graph.images[0].url,
            format!("https://{host}/app/og.png")
        );
        assert_eq!(
            value.open_graph.audio[0].url,
            format!("https://{host}/app/audio.mp3")
        );
        assert_eq!(
            value.open_graph.video[0].url,
            "https://cdn.example/video.mp4"
        );
    }
    let mut request = RequestContext::new();
    request
        .extensions_mut()
        .insert(MetadataBase::new("https://preview.example/prefix").unwrap());
    let value = plan.resolve_sync(&request, &NoLoader).unwrap();
    assert_eq!(
        value.open_graph.url.as_deref(),
        Some("https://preview.example/prefix/blog/post")
    );
    assert!(matches!(
        plan.resolve_sync(&RequestContext::new(), &NoLoader),
        Err(MetadataError::MissingBase)
    ));
}

#[test]
fn invalid_bases_and_unsafe_urls_fail() {
    for base in [
        "/relative",
        "javascript:alert(1)",
        "https://user:pass@example.com",
        "https://example.com?q=1",
        "https://example.com/#hash",
    ] {
        assert!(MetadataBase::new(base).is_err(), "{base}");
    }
    let mut state = MetadataState::default();
    state.apply(MetadataPatch {
        canonical: Set("javascript:alert(1)".into()),
        ..Default::default()
    });
    assert!(matches!(
        finish_urls(state.snapshot(), &RequestContext::new()),
        Err(MetadataError::InvalidUrl)
    ));
}

#[test]
fn generated_values_are_escaped_as_text_and_attributes() {
    let metadata = ResolvedMetadata {
        title: Some("</title><script>alert(1)</script>".to_owned().into()),
        description: Some("\" onload=\"x<&'".to_owned().into()),
        canonical: Some("https://example.com/?q=\"<&".to_owned().into()),
        ..Default::default()
    };
    let mut cx = WebContext::<NoLoader>::new();
    render_metadata_head(&mut cx, &metadata);
    let html = cx.into_output().into_string();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;/title&gt;"));
    assert!(
        html.contains("content=\"&quot; onload=&quot;x&lt;&amp;&#39;\""),
        "{html}"
    );
    assert!(html.contains("href=\"https://example.com/?q=&quot;&lt;&amp;\""));
}

static MEDIA_0: [ResolvedOpenGraphImage; 1] =
    [ResolvedOpenGraphImage::new("https://example.com/old")];
static MEDIA_1: [ResolvedOpenGraphMedia; 1] =
    [ResolvedOpenGraphMedia::new("https://example.com/audio")];
static MEDIA_2: [ResolvedOpenGraphImage; 1] =
    [ResolvedOpenGraphImage::new("https://example.com/new")];
static MEDIA_3: [ResolvedOpenGraphImage; 1] = [ResolvedOpenGraphImage::new("/og.png")];
static MEDIA_4: [ResolvedOpenGraphMedia; 1] = [ResolvedOpenGraphMedia::new("/audio.mp3")];
static MEDIA_5: [ResolvedOpenGraphMedia; 1] =
    [ResolvedOpenGraphMedia::new("https://cdn.example/video.mp4")];

static CHILD: TitleOnly = TitleOnly::new("child");
static LEAF: TitleOnly = TitleOnly::new("leaf");

static TAIL_CALLS: AtomicUsize = AtomicUsize::new(0);
struct StaticTail;
impl Metadata for StaticTail {
    fn static_metadata(&self) -> Option<MetadataPatch> {
        TAIL_CALLS.fetch_add(1, Ordering::SeqCst);
        Some(MetadataPatch {
            description: Set("tail".into()),
            ..Default::default()
        })
    }
}

#[test]
fn precomputation_stops_at_the_first_request_provider() {
    let plan = ComputedRouteMeta::cascade(
        &ComputedRouteMeta::default(),
        &[&Template, &RequestTitle, &StaticTail],
    );
    assert_eq!(TAIL_CALLS.load(Ordering::SeqCst), 0);
    let mut request =
        RequestContext::new().with_route_params(proa_core::RouteParams::from_vec(vec![
            proa_core::RouteParam::new("slug", "post"),
        ]));
    request
        .headers_mut()
        .insert("user-agent", http::HeaderValue::from_static("crawler"));
    let result = plan.resolve_sync(&request, &NoLoader).unwrap();
    assert_eq!(TAIL_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(result.title.as_deref(), Some("post | Site"));
    assert_eq!(result.description.as_deref(), Some("tail"));
}

#[test]
fn legacy_stack_resolution_never_invokes_request_providers_with_missing_inputs() {
    let metadata = resolve_metadata_stack(&[&RequestTitle]).unwrap();
    assert!(metadata.title.is_none());
}

struct LocalGenerator;
impl LocalMetadata for LocalGenerator {
    async fn generate_metadata(
        &self,
        _: &dyn MetadataContext,
        _: &ResolvedMetadata,
    ) -> Result<MetadataPatch, MetadataError> {
        let title = std::rc::Rc::new("local title".to_owned());
        tokio::task::yield_now().await;
        Ok(MetadataPatch {
            title: Set(title.as_str().to_owned().into()),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn local_metadata_futures_preserve_render_capability_requirements() {
    let plan = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), &[&LOCAL_GENERATOR]);
    let request = RequestContext::new();
    assert_eq!(
        plan.resolve(&request, &NoLoader)
            .await
            .unwrap()
            .title
            .as_deref(),
        Some("local title")
    );
    assert!(matches!(
        plan.resolve_sync(&request, &NoLoader),
        Err(MetadataError::AsyncInSyncResponse)
    ));
    assert!(matches!(
        plan.resolve_send(&request, &NoLoader).await,
        Err(MetadataError::LocalInSendResponse)
    ));
}

static LOCAL_GENERATOR: LocalMetadataProvider<LocalGenerator> = local_metadata(LocalGenerator);

struct ImmediateGenerator;
impl Metadata for ImmediateGenerator {
    async fn generate_metadata(
        &self,
        cx: &dyn MetadataContext,
        parent: &ResolvedMetadata,
    ) -> Result<MetadataPatch, MetadataError> {
        let title = cx
            .request()
            .user_agent()
            .ok_or(MetadataError::InvalidBase)?;
        assert_eq!(parent.description.as_deref(), Some("site description"));
        Ok(MetadataPatch {
            title: Set(title.to_owned().into()),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn native_async_generators_can_complete_immediately_and_propagate_errors() {
    let plan = ComputedRouteMeta::cascade(
        &ComputedRouteMeta::default(),
        &[&Template, &ImmediateGenerator],
    );
    let mut request = RequestContext::new();
    assert!(matches!(
        plan.resolve_sync(&request, &NoLoader),
        Err(MetadataError::InvalidBase)
    ));
    request
        .headers_mut()
        .insert("user-agent", http::HeaderValue::from_static("crawler"));
    let synchronous = plan.resolve_sync(&request, &NoLoader).unwrap();
    let local = plan.resolve(&request, &NoLoader).await.unwrap();
    let send = plan.resolve_send(&request, &NoLoader).await.unwrap();
    assert_eq!(synchronous.title.as_deref(), Some("crawler | Site"));
    assert_eq!(local.title, synchronous.title);
    assert_eq!(send.title, synchronous.title);
}
