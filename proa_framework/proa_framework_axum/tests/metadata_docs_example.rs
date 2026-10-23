// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The complete example from `proa_marketing/src/docs/framework/metadata.mdx`.
//! Keep the code between the markers identical to the docs sample, and the
//! asserted tags identical to the rendered head shown there.

// DOCS SAMPLE START
use axum::routing::get;
use proa_core::{WebContext, WebRenderSync, WriteError};
use proa_framework_axum::manifest::IslandManifest;
use proa_framework_axum::{
    render_metadata_head, router_from_spec, CascadeOpenGraph, CascadeTwitter, CascadeValue,
    FrameworkBuilder, Layout, Metadata, OpenGraphImage, Route, RouteResponse, TwitterCard,
};
use proa_macros::html_sync;

// 1. Site-wide defaults live on the layout.
pub struct SiteLayout;

impl Layout for SiteLayout {}

impl Metadata for SiteLayout {
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set("%s | Acme") // `%s` makes this a template for child titles
    }

    fn description(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set("Acme builds rockets.")
    }

    fn open_graph(&self) -> CascadeOpenGraph {
        static IMAGES: [OpenGraphImage; 1] =
            [OpenGraphImage::new("https://acme.example/og/default.png")
                .with_dimensions(1200, 630)
                .with_alt("The Acme logo")];
        CascadeOpenGraph {
            site_name: CascadeValue::Set("Acme"),
            r#type: CascadeValue::Set("website"),
            images: CascadeValue::Set(&IMAGES),
            ..Default::default()
        }
    }

    fn twitter(&self) -> CascadeTwitter {
        CascadeTwitter {
            card: CascadeValue::Set(TwitterCard::SummaryLargeImage),
            site: CascadeValue::Set("@acme"),
            ..Default::default()
        }
    }
}

// 2. A page overrides only what differs.
pub struct PricingMeta;

impl Metadata for PricingMeta {
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set("Pricing") // becomes "Pricing | Acme"
    }

    fn description(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set("Plans for every launch.")
    }

    fn open_graph(&self) -> CascadeOpenGraph {
        static IMAGES: [OpenGraphImage; 1] =
            [OpenGraphImage::new("https://acme.example/og/pricing.png").with_alt("Acme pricing")];
        CascadeOpenGraph {
            images: CascadeValue::Set(&IMAGES), // appended after the layout's image
            ..Default::default()
        }
    }

    fn twitter(&self) -> CascadeTwitter {
        CascadeTwitter {
            image: CascadeValue::Set("https://acme.example/og/pricing.png"),
            image_alt: CascadeValue::Set("Acme pricing"),
            ..Default::default()
        }
    }
}

// 3. One component renders the resolved tags into <head>.
pub struct MetadataHead;

impl WebRenderSync for MetadataHead {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        render_metadata_head(cx);
        Ok(())
    }
}

pub struct Document<C> {
    pub content: C,
}

impl<C: WebRenderSync> WebRenderSync for Document<C> {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    {MetadataHead}
                </head>
                <body>{self.content}</body>
            </html>
        }
        .render(cx)
    }
}

async fn pricing() -> RouteResponse {
    RouteResponse::ssr(Document {
        content: html_sync! { <h1>"Pricing"</h1> },
    })
}

async fn blog_post() -> RouteResponse {
    RouteResponse::ssr(Document {
        content: html_sync! { <h1>"A post"</h1> },
    })
}

static PRICING_META: PricingMeta = PricingMeta;

// 4. The route tree says which metadata applies where.
pub fn app(manifest: IslandManifest) -> axum::Router {
    let (router, metadata) = router_from_spec(vec![Route::layout(
        "/",
        &SiteLayout,
        vec![
            Route::page_with_metadata("pricing", get(pricing), &PRICING_META),
            Route::page("blog/:slug", "Blog", get(blog_post)),
        ],
    )]);

    FrameworkBuilder::new(manifest)
        .with_dynamic_routes_and_metadata(router, metadata)
        .build()
}
// DOCS SAMPLE END

fn manifest() -> IslandManifest {
    IslandManifest {
        islands: Default::default(),
        assets: Default::default(),
        import_map: Default::default(),
        ssr_bundle: None,
    }
}

async fn head(path: &str) -> String {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let response = app(manifest())
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    let start = html.find("<title>").expect("head has a title");
    let end = html.find("</head>").expect("head is closed");
    html[start..end].to_string()
}

#[tokio::test]
async fn pricing_head_matches_the_docs() {
    let expected = [
        r#"<title>Pricing | Acme</title>"#,
        r#"<meta name="description" content="Plans for every launch."/>"#,
        r#"<meta property="og:site_name" content="Acme"/>"#,
        r#"<meta property="og:type" content="website"/>"#,
        r#"<meta property="og:image" content="https://acme.example/og/default.png"/>"#,
        r#"<meta property="og:image:width" content="1200"/>"#,
        r#"<meta property="og:image:height" content="630"/>"#,
        r#"<meta property="og:image:alt" content="The Acme logo"/>"#,
        r#"<meta property="og:image" content="https://acme.example/og/pricing.png"/>"#,
        r#"<meta property="og:image:alt" content="Acme pricing"/>"#,
        r#"<meta name="twitter:card" content="summary_large_image"/>"#,
        r#"<meta name="twitter:site" content="@acme"/>"#,
        r#"<meta name="twitter:title" content="Pricing | Acme"/>"#,
        r#"<meta name="twitter:description" content="Plans for every launch."/>"#,
        r#"<meta name="twitter:image" content="https://acme.example/og/pricing.png"/>"#,
        r#"<meta name="twitter:image:alt" content="Acme pricing"/>"#,
    ]
    .concat();
    assert_eq!(head("/pricing").await, expected);
}

#[tokio::test]
async fn dynamic_route_inherits_layout_metadata_as_the_docs_describe() {
    let head = head("/blog/hello").await;
    assert!(head.starts_with("<title>Blog | Acme</title>"), "{head}");
    assert!(head.contains(r#"<meta name="description" content="Acme builds rockets."/>"#));
    assert!(head
        .contains(r#"<meta property="og:image" content="https://acme.example/og/default.png"/>"#));
    assert!(head
        .contains(r#"<meta name="twitter:image" content="https://acme.example/og/default.png"/>"#));
}
