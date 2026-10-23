// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::ctx::Ctx;
use proa_core::text::{text, UnsafeRaw};
use proa_core::WebRenderSync;

use crate::manifest::IslandManifest;

pub type MetadataRef = &'static dyn Metadata;

/// Represents how a metadata field should cascade from parent to child.
#[derive(Clone, Debug, Default)]
pub enum CascadeValue<T> {
    /// Override with this value (wins over parent).
    Set(T),
    /// Explicitly clear this field (force None, don't inherit from parent).
    Tombstone,
    /// Not specified (inherit from parent if available).
    #[default]
    Unset,
}

impl<T> CascadeValue<T> {
    pub fn resolve(self, parent_value: Option<T>) -> Option<T>
    where
        T: Clone,
    {
        match self {
            CascadeValue::Set(v) => Some(v),
            CascadeValue::Tombstone => None,
            CascadeValue::Unset => parent_value,
        }
    }
}

/// Simple metadata struct that only provides a title.
pub struct TitleOnly {
    pub title: &'static str,
}

impl TitleOnly {
    pub const fn new(title: &'static str) -> Self {
        Self { title }
    }
}

impl Metadata for TitleOnly {
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Set(self.title)
    }
}

/// Trait for defining page metadata (title, description, Open Graph, etc.).
pub trait Metadata: Send + Sync + 'static {
    /// Returns the page title cascade value. Can be a template like "%s • Site Name" or a complete title.
    fn title(&self) -> CascadeValue<&'static str> {
        CascadeValue::Unset
    }

    /// Returns true if this title should be treated as an absolute title that ignores templates.
    fn absolute_title(&self) -> bool {
        false
    }

    fn description(&self) -> CascadeValue<&'static str> {
        CascadeValue::Unset
    }

    fn open_graph(&self) -> CascadeOpenGraph {
        CascadeOpenGraph::default()
    }

    /// Returns Twitter Card cascade values.
    ///
    /// Unset fields inherit from the parent provider. At render time, title,
    /// description, image, and image alt still fall back to Open Graph and then
    /// to the page title and description when those Twitter fields stay empty.
    fn twitter(&self) -> CascadeTwitter {
        CascadeTwitter::default()
    }
}

/// Precomputed metadata for a specific route (flattened at build time).
#[derive(Clone, Default)]
pub struct ComputedRouteMeta {
    pub title: Option<String>,
    pub description: Option<&'static str>,
    pub open_graph: ComputedOpenGraph,
    pub twitter: ResolvedTwitter,
    pub(crate) title_template: Option<String>,
    pub(crate) og_title_template: Option<String>,
    pub(crate) og_description_template: Option<String>,
}

impl ComputedRouteMeta {
    /// Creates computed metadata by cascading parent metadata with local metadata providers.
    pub fn cascade(parent: &ComputedRouteMeta, local_providers: &[MetadataRef]) -> Self {
        let mut computed = parent.clone();
        let mut raw_page_title: Option<&'static str> = None;

        for provider in local_providers {
            match provider.title() {
                CascadeValue::Set(title) => {
                    if provider.absolute_title() {
                        computed.title = Some(title.to_string());
                        computed.title_template = None;
                        raw_page_title = None;
                    } else if title.contains("%s") {
                        computed.title_template = Some(title.to_string());
                    } else {
                        raw_page_title = Some(title);
                        if let Some(template) = &computed.title_template {
                            computed.title = Some(template.replace("%s", title));
                        } else {
                            computed.title = Some(title.to_string());
                        }
                    }
                }
                CascadeValue::Tombstone => {
                    computed.title = None;
                    computed.title_template = None;
                    raw_page_title = None;
                }
                CascadeValue::Unset => {}
            }

            computed.description = provider.description().resolve(computed.description);

            let og = provider.open_graph();

            match og.title {
                CascadeValue::Set(og_title) => {
                    if og_title.contains("%s") {
                        computed.og_title_template = Some(og_title.to_string());
                    } else if let Some(template) = &computed.og_title_template {
                        computed.open_graph.title = Some(template.replace("%s", og_title));
                    } else {
                        computed.open_graph.title = Some(og_title.to_string());
                    }
                }
                CascadeValue::Tombstone => {
                    computed.open_graph.title = None;
                    computed.og_title_template = None;
                }
                CascadeValue::Unset => {}
            }

            match og.description {
                CascadeValue::Set(og_desc) => {
                    if og_desc.contains("%s") {
                        computed.og_description_template = Some(og_desc.to_string());
                    } else if let Some(template) = &computed.og_description_template {
                        computed.open_graph.description = Some(template.replace("%s", og_desc));
                    } else {
                        computed.open_graph.description = Some(og_desc.to_string());
                    }
                }
                CascadeValue::Tombstone => {
                    computed.open_graph.description = None;
                    computed.og_description_template = None;
                }
                CascadeValue::Unset => {}
            }

            match og.site_name {
                CascadeValue::Set(v) => computed.open_graph.site_name = Some(v.to_string()),
                CascadeValue::Tombstone => computed.open_graph.site_name = None,
                CascadeValue::Unset => {}
            }
            computed.open_graph.determiner = og.determiner.resolve(computed.open_graph.determiner);
            computed.open_graph.r#type = og.r#type.resolve(computed.open_graph.r#type);
            computed.open_graph.url = og.url.resolve(computed.open_graph.url);
            computed.open_graph.locale = og.locale.resolve(computed.open_graph.locale);

            match og.locale_alternates {
                CascadeValue::Set(alts) => {
                    computed.open_graph.locale_alternates = alts.to_vec();
                }
                CascadeValue::Tombstone => {
                    computed.open_graph.locale_alternates.clear();
                }
                CascadeValue::Unset => {}
            }

            match og.images {
                CascadeValue::Set(images) => {
                    for image in images.iter() {
                        if let Some(resolved_image) = ResolvedOpenGraphImage::from_image(image) {
                            computed.open_graph.images.push(resolved_image);
                        }
                    }
                }
                CascadeValue::Tombstone => {
                    computed.open_graph.images.clear();
                }
                CascadeValue::Unset => {}
            }

            match og.audio {
                CascadeValue::Set(audio) => {
                    for media in audio.iter() {
                        if let Some(resolved_media) = ResolvedOpenGraphMedia::from_media(media) {
                            computed.open_graph.audio.push(resolved_media);
                        }
                    }
                }
                CascadeValue::Tombstone => {
                    computed.open_graph.audio.clear();
                }
                CascadeValue::Unset => {}
            }

            match og.video {
                CascadeValue::Set(video) => {
                    for media in video.iter() {
                        if let Some(resolved_media) = ResolvedOpenGraphMedia::from_media(media) {
                            computed.open_graph.video.push(resolved_media);
                        }
                    }
                }
                CascadeValue::Tombstone => {
                    computed.open_graph.video.clear();
                }
                CascadeValue::Unset => {}
            }

            apply_twitter_cascade(&mut computed.twitter, provider.twitter());
        }

        if computed.open_graph.title.is_none() {
            if let (Some(template), Some(page_title)) =
                (&computed.og_title_template, raw_page_title)
            {
                computed.open_graph.title = Some(template.replace("%s", page_title));
            }
        }

        if computed.open_graph.description.is_none() {
            if let (Some(template), Some(page_desc)) =
                (&computed.og_description_template, computed.description)
            {
                computed.open_graph.description = Some(template.replace("%s", page_desc));
            }
        }

        computed
    }
}

/// Resolved metadata from a stack of metadata providers.
#[derive(Clone, Default)]
pub struct ResolvedMetadata {
    pub title: Option<String>,
    pub description: Option<&'static str>,
    pub open_graph: ResolvedOpenGraph,
    pub twitter: ResolvedTwitter,
}

/// Twitter Card type written to `twitter:card`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwitterCard {
    /// Small image beside title and description.
    Summary,
    /// Large image above title and description.
    SummaryLargeImage,
}

impl TwitterCard {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::SummaryLargeImage => "summary_large_image",
        }
    }
}

/// Cascade specification for Twitter Card metadata.
#[derive(Clone, Default)]
pub struct CascadeTwitter {
    pub card: CascadeValue<TwitterCard>,
    pub site: CascadeValue<&'static str>,
    pub creator: CascadeValue<&'static str>,
    pub title: CascadeValue<&'static str>,
    pub description: CascadeValue<&'static str>,
    pub image: CascadeValue<&'static str>,
    pub image_alt: CascadeValue<&'static str>,
}

/// Resolved Twitter Card metadata ready for rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedTwitter {
    pub card: Option<TwitterCard>,
    pub site: Option<&'static str>,
    pub creator: Option<&'static str>,
    pub title: Option<&'static str>,
    pub description: Option<&'static str>,
    pub image: Option<&'static str>,
    pub image_alt: Option<&'static str>,
}

impl ResolvedTwitter {
    /// True when no Twitter Card field was set on the cascade.
    pub fn is_empty(&self) -> bool {
        self.card.is_none()
            && self.site.is_none()
            && self.creator.is_none()
            && self.title.is_none()
            && self.description.is_none()
            && self.image.is_none()
            && self.image_alt.is_none()
    }
}

fn apply_twitter_cascade(dst: &mut ResolvedTwitter, spec: CascadeTwitter) {
    dst.card = spec.card.resolve(dst.card);
    dst.site = spec.site.resolve(dst.site);
    dst.creator = spec.creator.resolve(dst.creator);
    dst.title = spec.title.resolve(dst.title);
    dst.description = spec.description.resolve(dst.description);
    dst.image = spec.image.resolve(dst.image);
    dst.image_alt = spec.image_alt.resolve(dst.image_alt);
}

/// Cascade specification for Open Graph metadata.
#[derive(Clone, Default)]
pub struct CascadeOpenGraph {
    pub title: CascadeValue<&'static str>,
    pub description: CascadeValue<&'static str>,
    pub site_name: CascadeValue<&'static str>,
    pub determiner: CascadeValue<&'static str>,
    pub r#type: CascadeValue<&'static str>,
    pub url: CascadeValue<&'static str>,
    pub locale: CascadeValue<&'static str>,
    pub locale_alternates: CascadeValue<&'static [&'static str]>,
    pub images: CascadeValue<&'static [OpenGraphImage]>,
    pub audio: CascadeValue<&'static [OpenGraphMedia]>,
    pub video: CascadeValue<&'static [OpenGraphMedia]>,
}

/// Static Open Graph descriptor returned by metadata providers.
#[derive(Clone, Default)]
pub struct OpenGraph {
    pub title: Option<&'static str>,
    pub description: Option<&'static str>,
    pub site_name: Option<&'static str>,
    pub determiner: Option<&'static str>,
    pub r#type: Option<&'static str>,
    pub url: Option<&'static str>,
    pub locale: Option<&'static str>,
    pub locale_alternates: &'static [&'static str],
    pub images: &'static [OpenGraphImage],
    pub audio: &'static [OpenGraphMedia],
    pub video: &'static [OpenGraphMedia],
}

/// Static Open Graph image descriptor.
#[derive(Clone)]
pub struct OpenGraphImage {
    pub url: Option<&'static str>,
    pub secure_url: Option<&'static str>,
    pub path: Option<&'static str>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<&'static str>,
    pub mime_type: Option<&'static str>,
}

impl OpenGraphImage {
    pub const fn new(url: &'static str) -> Self {
        Self {
            url: Some(url),
            secure_url: None,
            path: None,
            width: None,
            height: None,
            alt: None,
            mime_type: None,
        }
    }

    pub const fn with_secure_url(mut self, secure_url: &'static str) -> Self {
        self.secure_url = Some(secure_url);
        self
    }

    pub const fn with_path(mut self, path: &'static str) -> Self {
        self.path = Some(path);
        self
    }

    pub const fn with_width(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    pub const fn with_height(mut self, height: u32) -> Self {
        self.height = Some(height);
        self
    }

    pub const fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub const fn with_alt(mut self, alt: &'static str) -> Self {
        self.alt = Some(alt);
        self
    }

    pub const fn with_mime(mut self, mime_type: &'static str) -> Self {
        self.mime_type = Some(mime_type);
        self
    }
}

/// Static Open Graph media descriptor (audio/video).
#[derive(Clone)]
pub struct OpenGraphMedia {
    pub url: Option<&'static str>,
    pub secure_url: Option<&'static str>,
    pub mime_type: Option<&'static str>,
}

impl OpenGraphMedia {
    pub const fn new(url: &'static str) -> Self {
        Self {
            url: Some(url),
            secure_url: None,
            mime_type: None,
        }
    }

    pub const fn with_secure_url(mut self, secure_url: &'static str) -> Self {
        self.secure_url = Some(secure_url);
        self
    }

    pub const fn with_mime(mut self, mime_type: &'static str) -> Self {
        self.mime_type = Some(mime_type);
        self
    }
}

/// Precomputed Open Graph metadata for a specific route.
#[derive(Clone, Default)]
pub struct ComputedOpenGraph {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site_name: Option<String>,
    pub determiner: Option<&'static str>,
    pub r#type: Option<&'static str>,
    pub url: Option<&'static str>,
    pub locale: Option<&'static str>,
    pub locale_alternates: Vec<&'static str>,
    pub images: Vec<ResolvedOpenGraphImage>,
    pub audio: Vec<ResolvedOpenGraphMedia>,
    pub video: Vec<ResolvedOpenGraphMedia>,
}

/// Resolved Open Graph metadata ready for rendering.
#[derive(Clone, Default)]
pub struct ResolvedOpenGraph {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site_name: Option<String>,
    pub determiner: Option<&'static str>,
    pub r#type: Option<&'static str>,
    pub url: Option<&'static str>,
    pub locale: Option<&'static str>,
    pub locale_alternates: Vec<&'static str>,
    pub images: Vec<ResolvedOpenGraphImage>,
    pub audio: Vec<ResolvedOpenGraphMedia>,
    pub video: Vec<ResolvedOpenGraphMedia>,
}

#[derive(Clone)]
pub struct ResolvedOpenGraphImage {
    pub url: String,
    pub secure_url: Option<String>,
    pub mime_type: Option<&'static str>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<&'static str>,
}

impl ResolvedOpenGraphImage {
    pub(crate) fn from_image(image: &OpenGraphImage) -> Option<Self> {
        let url = image.url.or(image.path)?;
        Some(Self {
            url: url.to_string(),
            secure_url: image.secure_url.map(|value| value.to_string()),
            mime_type: image.mime_type,
            width: image.width,
            height: image.height,
            alt: image.alt,
        })
    }
}

#[derive(Clone)]
pub struct ResolvedOpenGraphMedia {
    pub url: String,
    pub secure_url: Option<String>,
    pub mime_type: Option<&'static str>,
}

impl ResolvedOpenGraphMedia {
    pub(crate) fn from_media(media: &OpenGraphMedia) -> Option<Self> {
        let url = media.url?;
        Some(Self {
            url: url.to_string(),
            secure_url: media.secure_url.map(|value| value.to_string()),
            mime_type: media.mime_type,
        })
    }
}

pub fn resolved_metadata_from_computed(computed: &ComputedRouteMeta) -> ResolvedMetadata {
    ResolvedMetadata {
        title: computed.title.clone(),
        description: computed.description,
        open_graph: ResolvedOpenGraph {
            title: computed.open_graph.title.clone(),
            description: computed.open_graph.description.clone(),
            site_name: computed.open_graph.site_name.clone(),
            determiner: computed.open_graph.determiner,
            r#type: computed.open_graph.r#type,
            url: computed.open_graph.url,
            locale: computed.open_graph.locale,
            locale_alternates: computed.open_graph.locale_alternates.clone(),
            images: computed.open_graph.images.clone(),
            audio: computed.open_graph.audio.clone(),
            video: computed.open_graph.video.clone(),
        },
        twitter: computed.twitter.clone(),
    }
}

pub fn resolve_metadata_stack(metadata: &[MetadataRef]) -> Option<ResolvedMetadata> {
    if metadata.is_empty() {
        return None;
    }

    let mut resolved = ResolvedMetadata::default();
    let mut title_template: Option<&'static str> = None;
    let mut og_title_template: Option<&'static str> = None;
    let mut og_description_template: Option<&'static str> = None;
    let mut raw_page_title: Option<&'static str> = None;

    for metadata in metadata {
        match metadata.title() {
            CascadeValue::Set(title) => {
                if metadata.absolute_title() {
                    resolved.title = Some(title.to_string());
                    title_template = None;
                    raw_page_title = None;
                } else if title.contains("%s") {
                    title_template = Some(title);
                } else {
                    raw_page_title = Some(title);
                    if let Some(template) = title_template {
                        resolved.title = Some(template.replace("%s", title));
                    } else {
                        resolved.title = Some(title.to_string());
                    }
                }
            }
            CascadeValue::Tombstone => {
                resolved.title = None;
                title_template = None;
            }
            CascadeValue::Unset => {}
        }

        resolved.description = metadata.description().resolve(resolved.description);

        let og = metadata.open_graph();
        match og.title {
            CascadeValue::Set(og_title) => {
                if og_title.contains("%s") {
                    og_title_template = Some(og_title);
                } else if let Some(template) = og_title_template {
                    resolved.open_graph.title = Some(template.replace("%s", og_title));
                } else {
                    resolved.open_graph.title = Some(og_title.to_string());
                }
            }
            CascadeValue::Tombstone => {
                resolved.open_graph.title = None;
            }
            CascadeValue::Unset => {}
        }

        match og.description {
            CascadeValue::Set(og_desc) => {
                if og_desc.contains("%s") {
                    og_description_template = Some(og_desc);
                } else if let Some(template) = og_description_template {
                    resolved.open_graph.description = Some(template.replace("%s", og_desc));
                } else {
                    resolved.open_graph.description = Some(og_desc.to_string());
                }
            }
            CascadeValue::Tombstone => {
                resolved.open_graph.description = None;
            }
            CascadeValue::Unset => {}
        }

        apply_twitter_cascade(&mut resolved.twitter, metadata.twitter());
    }

    if resolved.open_graph.title.is_none() {
        if let (Some(template), Some(page_title)) = (og_title_template, raw_page_title) {
            resolved.open_graph.title = Some(template.replace("%s", page_title));
        }
    }

    if resolved.open_graph.description.is_none() {
        if let (Some(template), Some(page_desc)) = (og_description_template, resolved.description) {
            resolved.open_graph.description = Some(template.replace("%s", page_desc));
        }
    }

    Some(resolved)
}

pub fn css_asset_path(manifest: &IslandManifest) -> Option<&str> {
    manifest.assets.css.as_ref().map(|css| css.path.as_str())
}

/// Write a string as an HTML attribute value, escaping `& < > " '` for the
/// active dialect.
///
/// The metadata head builds `<meta>` / `<link>` tags by hand with `UnsafeRaw`
/// scaffolding, so any value placed inside `"..."` must use attribute escaping,
/// which neutralizes the closing quote. Plain text escaping (`text()`) does NOT
/// escape `"` and would let a value carrying a quote break out of the attribute
/// and inject further attributes (for example an `http-equiv="refresh"`
/// redirect). Metadata fields are `&'static str` today, so this is defense in
/// depth, but it keeps the head safe if the API later accepts dynamic values.
fn write_attr_value(cx: &mut Ctx, value: &str) {
    let dialect = cx.__proa_current_escape_dialect();
    proa_core::shared::escape::escape_attr_value_for_dialect(&mut cx.out, value, dialect).unwrap();
}

pub fn render_css_link(cx: &mut Ctx, css_path: &str) {
    UnsafeRaw("<link rel=\"stylesheet\" href=\"")
        .render(cx)
        .unwrap();
    write_attr_value(cx, css_path);
    UnsafeRaw("\"/>").render(cx).unwrap();
}

pub fn render_metadata_head(cx: &mut Ctx, metadata: &ResolvedMetadata) {
    if let Some(title) = &metadata.title {
        UnsafeRaw("<title>").render(cx).unwrap();
        text(title.as_str()).render(cx).unwrap();
        UnsafeRaw("</title>").render(cx).unwrap();
    }

    if let Some(description) = metadata.description {
        render_named_meta(cx, "description", description);
    }

    let og = &metadata.open_graph;

    if let Some(title) = &og.title {
        render_property_meta(cx, "og:title", title.as_str());
    }

    if let Some(description) = &og.description {
        render_property_meta(cx, "og:description", description.as_str());
    }

    if let Some(site_name) = &og.site_name {
        render_property_meta(cx, "og:site_name", site_name.as_str());
    }

    if let Some(determiner) = og.determiner {
        render_property_meta(cx, "og:determiner", determiner);
    }

    if let Some(kind) = og.r#type {
        render_property_meta(cx, "og:type", kind);
    }

    if let Some(url) = og.url {
        render_property_meta(cx, "og:url", url);
    }

    if let Some(locale) = og.locale {
        render_property_meta(cx, "og:locale", locale);
    }

    for alternate in &og.locale_alternates {
        render_property_meta(cx, "og:locale:alternate", alternate);
    }

    for image in &og.images {
        render_property_meta(cx, "og:image", image.url.as_str());
        if let Some(secure_url) = image.secure_url.as_ref() {
            render_property_meta(cx, "og:image:secure_url", secure_url.as_str());
        }
        if let Some(width) = image.width {
            let width_str = width.to_string();
            render_property_meta(cx, "og:image:width", width_str.as_str());
        }
        if let Some(height) = image.height {
            let height_str = height.to_string();
            render_property_meta(cx, "og:image:height", height_str.as_str());
        }
        if let Some(alt) = image.alt {
            render_property_meta(cx, "og:image:alt", alt);
        }
        if let Some(mime) = image.mime_type {
            render_property_meta(cx, "og:image:type", mime);
        }
    }

    for media in &og.audio {
        render_property_meta(cx, "og:audio", media.url.as_str());
        if let Some(secure_url) = media.secure_url.as_ref() {
            render_property_meta(cx, "og:audio:secure_url", secure_url.as_str());
        }
        if let Some(mime) = media.mime_type {
            render_property_meta(cx, "og:audio:type", mime);
        }
    }

    for media in &og.video {
        render_property_meta(cx, "og:video", media.url.as_str());
        if let Some(secure_url) = media.secure_url.as_ref() {
            render_property_meta(cx, "og:video:secure_url", secure_url.as_str());
        }
        if let Some(mime) = media.mime_type {
            render_property_meta(cx, "og:video:type", mime);
        }
    }

    render_twitter_tags(cx, metadata);
}

fn render_twitter_tags(cx: &mut Ctx, metadata: &ResolvedMetadata) {
    let twitter = &metadata.twitter;
    if twitter.is_empty() {
        return;
    }

    let og = &metadata.open_graph;
    let image = twitter
        .image
        .or_else(|| og.images.first().map(|image| image.url.as_str()));
    let card = twitter.card.unwrap_or(if image.is_some() {
        TwitterCard::SummaryLargeImage
    } else {
        TwitterCard::Summary
    });
    render_named_meta(cx, "twitter:card", card.as_str());

    if let Some(site) = twitter.site {
        render_named_meta(cx, "twitter:site", site);
    }
    if let Some(creator) = twitter.creator {
        render_named_meta(cx, "twitter:creator", creator);
    }

    if let Some(title) = twitter
        .title
        .or(og.title.as_deref())
        .or(metadata.title.as_deref())
    {
        render_named_meta(cx, "twitter:title", title);
    }

    if let Some(description) = twitter
        .description
        .or(og.description.as_deref())
        .or(metadata.description)
    {
        render_named_meta(cx, "twitter:description", description);
    }

    if let Some(image) = image {
        render_named_meta(cx, "twitter:image", image);
        if let Some(alt) = twitter
            .image_alt
            .or_else(|| og.images.first().and_then(|image| image.alt))
        {
            render_named_meta(cx, "twitter:image:alt", alt);
        }
    }
}

fn render_named_meta(cx: &mut Ctx, name: &str, content: &str) {
    UnsafeRaw("<meta name=\"").render(cx).unwrap();
    write_attr_value(cx, name);
    UnsafeRaw("\" content=\"").render(cx).unwrap();
    write_attr_value(cx, content);
    UnsafeRaw("\"/>").render(cx).unwrap();
}

fn render_property_meta(cx: &mut Ctx, property: &str, content: &str) {
    UnsafeRaw("<meta property=\"").render(cx).unwrap();
    write_attr_value(cx, property);
    UnsafeRaw("\" content=\"").render(cx).unwrap();
    write_attr_value(cx, content);
    UnsafeRaw("\"/>").render(cx).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BaseMeta;

    impl Metadata for BaseMeta {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("%s | Base")
        }

        fn description(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("base-desc")
        }
    }

    struct PageMeta;

    impl Metadata for PageMeta {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Page")
        }
    }

    #[test]
    fn resolve_metadata_stack_applies_templates() {
        let providers: &[MetadataRef] = &[&BaseMeta, &PageMeta];
        let resolved = resolve_metadata_stack(providers).expect("metadata should resolve");
        assert_eq!(resolved.title.as_deref(), Some("Page | Base"));
        assert_eq!(resolved.description, Some("base-desc"));
    }

    #[test]
    fn render_metadata_head_writes_tags() {
        let metadata = ResolvedMetadata {
            title: Some("Page".to_string()),
            description: Some("desc"),
            ..Default::default()
        };
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &metadata);
        let html = cx.into_output().into_string();
        assert!(html.contains("<title>Page</title>"));
        assert!(html.contains("meta name=\"description\""));
        assert!(
            !html.contains("twitter:"),
            "unset Twitter cascade must not emit twitter tags: {html}"
        );
    }

    struct LayoutTwitter;

    impl Metadata for LayoutTwitter {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("%s | Site")
        }

        fn description(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("site-desc")
        }

        fn open_graph(&self) -> CascadeOpenGraph {
            static IMAGES: [OpenGraphImage; 1] =
                [OpenGraphImage::new("https://example.com/og.png").with_alt("OG preview")];
            CascadeOpenGraph {
                images: CascadeValue::Set(&IMAGES),
                ..Default::default()
            }
        }

        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                card: CascadeValue::Set(TwitterCard::SummaryLargeImage),
                site: CascadeValue::Set("@site"),
                image: CascadeValue::Set("https://example.com/twitter.png"),
                ..Default::default()
            }
        }
    }

    struct PageTwitter;

    impl Metadata for PageTwitter {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Pricing")
        }

        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                creator: CascadeValue::Set("@author"),
                ..Default::default()
            }
        }
    }

    struct TombstoneTwitterImage;

    impl Metadata for TombstoneTwitterImage {
        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                image: CascadeValue::Tombstone,
                image_alt: CascadeValue::Tombstone,
                ..Default::default()
            }
        }
    }

    #[test]
    fn twitter_cascade_inherits_layout_and_falls_back_to_page_copy() {
        let mut computed = ComputedRouteMeta::default();
        computed = ComputedRouteMeta::cascade(&computed, &[&LayoutTwitter, &PageTwitter]);
        let resolved = resolved_metadata_from_computed(&computed);
        assert_eq!(resolved.twitter.card, Some(TwitterCard::SummaryLargeImage));
        assert_eq!(resolved.twitter.site, Some("@site"));
        assert_eq!(resolved.twitter.creator, Some("@author"));
        assert!(resolved.twitter.title.is_none());
        assert_eq!(
            resolved.twitter.image,
            Some("https://example.com/twitter.png")
        );

        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:card\" content=\"summary_large_image\""));
        assert!(html.contains("name=\"twitter:site\" content=\"@site\""));
        assert!(html.contains("name=\"twitter:creator\" content=\"@author\""));
        assert!(html.contains("name=\"twitter:title\" content=\"Pricing | Site\""));
        assert!(html.contains("name=\"twitter:description\" content=\"site-desc\""));
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/twitter.png\""));
        assert!(html.contains("name=\"twitter:image:alt\" content=\"OG preview\""));
    }

    #[test]
    fn twitter_image_tombstone_clears_override_and_uses_open_graph() {
        let providers: &[MetadataRef] = &[&LayoutTwitter, &PageTwitter, &TombstoneTwitterImage];
        let mut computed = ComputedRouteMeta::default();
        computed = ComputedRouteMeta::cascade(&computed, providers);
        assert!(computed.twitter.image.is_none());
        assert_eq!(computed.twitter.card, Some(TwitterCard::SummaryLargeImage));

        let resolved = resolved_metadata_from_computed(&computed);
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:card\" content=\"summary_large_image\""));
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/og.png\""));
        assert!(!html.contains("https://example.com/twitter.png"));
    }

    #[test]
    fn twitter_tags_escape_quotes_and_angle_brackets() {
        let metadata = ResolvedMetadata {
            twitter: ResolvedTwitter {
                card: Some(TwitterCard::Summary),
                title: Some("Docs & <examples> \"quoted\""),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &metadata);
        let html = cx.into_output().into_string();
        assert!(html.contains(
            "name=\"twitter:title\" content=\"Docs &amp; &lt;examples&gt; &quot;quoted&quot;\""
        ));
        assert!(!html.contains("content=\"Docs & <examples>"));
    }

    #[test]
    fn twitter_image_falls_back_to_first_open_graph_image() {
        struct CardOnly;

        impl Metadata for CardOnly {
            fn open_graph(&self) -> CascadeOpenGraph {
                static IMAGES: [OpenGraphImage; 1] =
                    [OpenGraphImage::new("https://example.com/og.png").with_alt("OG preview")];
                CascadeOpenGraph {
                    images: CascadeValue::Set(&IMAGES),
                    ..Default::default()
                }
            }

            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    card: CascadeValue::Set(TwitterCard::SummaryLargeImage),
                    ..Default::default()
                }
            }
        }

        let mut computed = ComputedRouteMeta::default();
        computed = ComputedRouteMeta::cascade(&computed, &[&CardOnly]);
        let resolved = resolved_metadata_from_computed(&computed);
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/og.png\""));
        assert!(html.contains("name=\"twitter:image:alt\" content=\"OG preview\""));
    }

    #[test]
    fn twitter_defaults_to_summary_when_no_image_is_available() {
        let metadata = ResolvedMetadata {
            title: Some("Hello".to_string()),
            twitter: ResolvedTwitter {
                site: Some("@site"),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &metadata);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:card\" content=\"summary\""));
        assert!(html.contains("name=\"twitter:title\" content=\"Hello\""));
        assert!(!html.contains("twitter:image"));
    }
}
