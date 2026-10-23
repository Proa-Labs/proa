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
    /// See the [X Cards markup reference] for the `twitter:*` tags and the
    /// [Open Graph protocol] for the `og:*` values used as fallbacks. X
    /// removed the Cards pages from its current docs, so the links point to
    /// archived copies.
    ///
    /// [X Cards markup reference]: https://web.archive.org/web/20260201012312/https://developer.x.com/en/docs/x-for-websites/cards/overview/markup
    /// [Open Graph protocol]: https://ogp.me/
    ///
    /// Unset fields inherit from the parent provider. `title` and
    /// `description` accept `%s` templates, formatted with the page title and
    /// description the same way as Open Graph. At render time, title,
    /// description, image, and image alt fall back to Open Graph and then to
    /// the page title and description when those Twitter fields stay empty.
    ///
    /// Tombstoning a field clears that field and keeps the rest of the card,
    /// including Open Graph fallbacks. Tombstoning `card` instead turns
    /// Twitter tags off and clears every inherited Twitter field; a field set
    /// afterward turns them back on.
    ///
    /// Setting or tombstoning `image` without restating `image_alt` drops the
    /// inherited alt, because alt text describes one image. An Open Graph alt
    /// is used only alongside the Open Graph image.
    /// [`TwitterCard::SummaryLargeImage`] is written as
    /// [`TwitterCard::Summary`] when the resolved page has no Twitter or Open
    /// Graph image.
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
    /// `open_graph.title` was filled from `og_title_template` rather than set
    /// explicitly, so each route refills it from its own page title.
    pub(crate) og_title_filled: bool,
    /// `open_graph.description` was filled from `og_description_template`
    /// rather than set explicitly, so each route refills it.
    pub(crate) og_description_filled: bool,
    pub(crate) twitter_templates: TwitterTemplates,
    /// Plain (non-template) page title in effect for this route, kept across
    /// the cascade so a descendant's Open Graph and Twitter templates are
    /// filled with the descendant's title rather than reusing a value filled
    /// for an ancestor.
    pub(crate) page_title: Option<&'static str>,
}

impl ComputedRouteMeta {
    /// Creates computed metadata by cascading parent metadata with local metadata providers.
    pub fn cascade(parent: &ComputedRouteMeta, local_providers: &[MetadataRef]) -> Self {
        let mut computed = parent.clone();

        for provider in local_providers {
            match provider.title() {
                CascadeValue::Set(title) => {
                    if provider.absolute_title() {
                        computed.title = Some(title.to_string());
                        computed.title_template = None;
                        computed.page_title = None;
                    } else if title.contains("%s") {
                        computed.title_template = Some(title.to_string());
                    } else {
                        computed.page_title = Some(title);
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
                    computed.page_title = None;
                }
                CascadeValue::Unset => {}
            }

            computed.description = provider.description().resolve(computed.description);

            let og = provider.open_graph();

            apply_templated(
                og.title,
                &mut computed.open_graph.title,
                &mut computed.og_title_template,
                &mut computed.og_title_filled,
            );
            apply_templated(
                og.description,
                &mut computed.open_graph.description,
                &mut computed.og_description_template,
                &mut computed.og_description_filled,
            );

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

            apply_twitter_cascade(
                &mut computed.twitter,
                &mut computed.twitter_templates,
                provider.twitter(),
            );
        }

        // Template-filled values are refilled on every route, so a child page
        // uses its own title and description instead of its ancestor's.
        refill_from_template(
            &mut computed.open_graph.title,
            &mut computed.og_title_filled,
            computed.og_title_template.as_deref(),
            computed.page_title,
        );
        refill_from_template(
            &mut computed.open_graph.description,
            &mut computed.og_description_filled,
            computed.og_description_template.as_deref(),
            computed.description,
        );

        finish_twitter_templates(
            &mut computed.twitter,
            &mut computed.twitter_templates,
            computed.page_title,
            computed.description,
        );

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
    ///
    /// Rendered as [`TwitterCard::Summary`] when the resolved page has no
    /// Twitter or Open Graph image.
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
///
/// Each field maps to one tag in the [X Cards markup reference], which also
/// lists the Open Graph tag X reads when a `twitter:*` tag is absent. See the
/// [Open Graph protocol] for those tags. X removed the Cards pages from its
/// current docs, so the X link points to an archived copy.
///
/// [X Cards markup reference]: https://web.archive.org/web/20260201012312/https://developer.x.com/en/docs/x-for-websites/cards/overview/markup
/// [Open Graph protocol]: https://ogp.me/
#[derive(Clone, Default)]
pub struct CascadeTwitter {
    /// `twitter:card`: the card type.
    pub card: CascadeValue<TwitterCard>,
    /// `twitter:site`: `@username` of the website.
    pub site: CascadeValue<&'static str>,
    /// `twitter:creator`: `@username` of the content creator.
    pub creator: CascadeValue<&'static str>,
    /// `twitter:title`: falls back to `og:title`. Accepts `%s` templates.
    pub title: CascadeValue<&'static str>,
    /// `twitter:description`: falls back to `og:description`. Accepts `%s`
    /// templates.
    pub description: CascadeValue<&'static str>,
    /// `twitter:image`: absolute image URL. Falls back to the first `og:image`.
    pub image: CascadeValue<&'static str>,
    /// `twitter:image:alt`: alt text for `image`.
    pub image_alt: CascadeValue<&'static str>,
}

/// Resolved Twitter Card metadata ready for rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedTwitter {
    pub card: Option<TwitterCard>,
    pub site: Option<&'static str>,
    pub creator: Option<&'static str>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<&'static str>,
    pub image_alt: Option<&'static str>,
    /// Writes Twitter tags even when every field above is empty, filling them
    /// from Open Graph and the page title and description. The cascade sets
    /// this when any provider sets a Twitter field, so tombstoning individual
    /// fields keeps the card. Tombstoning `card` clears it.
    pub enabled: bool,
}

impl ResolvedTwitter {
    /// True when no Twitter tags will be written: not enabled and no field set.
    pub fn is_empty(&self) -> bool {
        !self.enabled
            && self.card.is_none()
            && self.site.is_none()
            && self.creator.is_none()
            && self.title.is_none()
            && self.description.is_none()
            && self.image.is_none()
            && self.image_alt.is_none()
    }
}

fn twitter_spec_sets_any(spec: &CascadeTwitter) -> bool {
    matches!(spec.card, CascadeValue::Set(_))
        || matches!(spec.site, CascadeValue::Set(_))
        || matches!(spec.creator, CascadeValue::Set(_))
        || matches!(spec.title, CascadeValue::Set(_))
        || matches!(spec.description, CascadeValue::Set(_))
        || matches!(spec.image, CascadeValue::Set(_))
        || matches!(spec.image_alt, CascadeValue::Set(_))
}

/// `%s` templates for Twitter title and description, carried down the cascade
/// the same way as the Open Graph templates.
#[derive(Clone, Default)]
pub(crate) struct TwitterTemplates {
    title: Option<String>,
    description: Option<String>,
    /// The resolved title was filled from `title` rather than set explicitly,
    /// so each route refills it from its own page title.
    title_filled: bool,
    /// The resolved description was filled from `description` rather than set
    /// explicitly, so each route refills it from its own page description.
    description_filled: bool,
}

fn apply_twitter_cascade(
    dst: &mut ResolvedTwitter,
    templates: &mut TwitterTemplates,
    spec: CascadeTwitter,
) {
    // Tombstoning the card turns Twitter tags off: every inherited field and
    // template is cleared. Fields set by the same or a later provider turn
    // the card back on.
    if matches!(spec.card, CascadeValue::Tombstone) {
        *dst = ResolvedTwitter::default();
        *templates = TwitterTemplates::default();
    }
    if twitter_spec_sets_any(&spec) {
        dst.enabled = true;
    }
    dst.card = spec.card.resolve(dst.card);
    dst.site = spec.site.resolve(dst.site);
    dst.creator = spec.creator.resolve(dst.creator);
    apply_templated(
        spec.title,
        &mut dst.title,
        &mut templates.title,
        &mut templates.title_filled,
    );
    apply_templated(
        spec.description,
        &mut dst.description,
        &mut templates.description,
        &mut templates.description_filled,
    );
    // Alt text describes one image. A provider that replaces or clears the
    // image without restating the alt drops the inherited alt instead of
    // attaching it to a different image.
    let image_changed = !matches!(spec.image, CascadeValue::Unset);
    dst.image = spec.image.resolve(dst.image);
    dst.image_alt = match spec.image_alt {
        CascadeValue::Unset if image_changed => None,
        image_alt => image_alt.resolve(dst.image_alt),
    };
}

/// Applies a templatable cascade value: a value containing `%s` becomes the
/// template for descendants, and a plain value is formatted through the
/// current template. `filled` tracks whether `dst` holds a template-filled
/// value rather than an explicit one.
fn apply_templated(
    value: CascadeValue<&'static str>,
    dst: &mut Option<String>,
    template: &mut Option<String>,
    filled: &mut bool,
) {
    match value {
        CascadeValue::Set(value) if value.contains("%s") => {
            *template = Some(value.to_string());
        }
        CascadeValue::Set(value) => {
            *dst = Some(match template {
                Some(template) => template.replace("%s", value),
                None => value.to_string(),
            });
            *filled = false;
        }
        CascadeValue::Tombstone => {
            *dst = None;
            *template = None;
            *filled = false;
        }
        CascadeValue::Unset => {}
    }
}

/// Fills the Twitter title and description from their templates and the
/// page title and description, the same way as Open Graph.
///
/// A value filled for an ancestor route is refilled for each descendant, so a
/// child page's card uses the child's title. Explicitly set values are kept.
fn finish_twitter_templates(
    dst: &mut ResolvedTwitter,
    templates: &mut TwitterTemplates,
    page_title: Option<&str>,
    page_description: Option<&str>,
) {
    refill_from_template(
        &mut dst.title,
        &mut templates.title_filled,
        templates.title.as_deref(),
        page_title,
    );
    refill_from_template(
        &mut dst.description,
        &mut templates.description_filled,
        templates.description.as_deref(),
        page_description,
    );
}

fn refill_from_template(
    dst: &mut Option<String>,
    filled: &mut bool,
    template: Option<&str>,
    source: Option<&str>,
) {
    if dst.is_some() && !*filled {
        return;
    }
    match (template, source) {
        (Some(template), Some(source)) => {
            *dst = Some(template.replace("%s", source));
            *filled = true;
        }
        _ if *filled => {
            *dst = None;
            *filled = false;
        }
        _ => {}
    }
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

/// Resolves a full provider stack (outermost layout first) in one pass.
///
/// This is the fallback for requests whose path is not in the precomputed
/// metadata map, such as dynamic `:param` routes. It runs the same cascade as
/// [`ComputedRouteMeta::cascade`], so a route renders the same head whichever
/// path resolves it.
pub fn resolve_metadata_stack(metadata: &[MetadataRef]) -> Option<ResolvedMetadata> {
    if metadata.is_empty() {
        return None;
    }
    let computed = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), metadata);
    Some(resolved_metadata_from_computed(&computed))
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
    // An Open Graph alt only describes the Open Graph image, so it is used
    // only when that image is the one written to `twitter:image`.
    let (image, image_alt) = match (twitter.image, og.images.first()) {
        (Some(image), _) => (Some(image), twitter.image_alt),
        (None, Some(og_image)) => (
            Some(og_image.url.as_str()),
            twitter.image_alt.or(og_image.alt),
        ),
        (None, None) => (None, None),
    };
    let card = if image.is_some() {
        twitter.card.unwrap_or(TwitterCard::SummaryLargeImage)
    } else {
        TwitterCard::Summary
    };
    render_named_meta(cx, "twitter:card", card.as_str());

    if let Some(site) = twitter.site {
        render_named_meta(cx, "twitter:site", site);
    }
    if let Some(creator) = twitter.creator {
        render_named_meta(cx, "twitter:creator", creator);
    }

    if let Some(title) = twitter
        .title
        .as_deref()
        .or(og.title.as_deref())
        .or(metadata.title.as_deref())
    {
        render_named_meta(cx, "twitter:title", title);
    }

    if let Some(description) = twitter
        .description
        .as_deref()
        .or(og.description.as_deref())
        .or(metadata.description)
    {
        render_named_meta(cx, "twitter:description", description);
    }

    if let Some(image) = image {
        render_named_meta(cx, "twitter:image", image);
        if let Some(alt) = image_alt {
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
        assert!(
            !html.contains("name=\"twitter:image:alt\""),
            "the Open Graph alt describes a different image: {html}"
        );
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
                title: Some("Docs & <examples> \"quoted\"".to_string()),
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

    #[test]
    fn summary_large_image_falls_back_to_summary_when_resolved_page_has_no_image() {
        let metadata = ResolvedMetadata {
            title: Some("Hello".to_string()),
            twitter: ResolvedTwitter {
                card: Some(TwitterCard::SummaryLargeImage),
                site: Some("@site"),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &metadata);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:card\" content=\"summary\""));
        assert!(!html.contains("summary_large_image"), "{html}");
        assert!(html.contains("name=\"twitter:site\" content=\"@site\""));
        assert!(html.contains("name=\"twitter:title\" content=\"Hello\""));
        assert!(!html.contains("twitter:image"));
    }

    struct LayoutTwitterImageOnly;

    impl Metadata for LayoutTwitterImageOnly {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Page")
        }

        fn description(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("page-desc")
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
                image: CascadeValue::Set("https://example.com/twitter.png"),
                ..Default::default()
            }
        }
    }

    #[test]
    fn tombstoning_sole_twitter_image_keeps_other_twitter_tags() {
        let providers: &[MetadataRef] = &[&LayoutTwitterImageOnly, &TombstoneTwitterImage];
        let mut computed = ComputedRouteMeta::default();
        computed = ComputedRouteMeta::cascade(&computed, providers);
        assert!(computed.twitter.image.is_none());
        assert!(computed.twitter.image_alt.is_none());
        assert!(computed.twitter.card.is_none());
        assert!(computed.twitter.enabled);
        assert!(!computed.twitter.is_empty());

        let resolved = resolved_metadata_from_computed(&computed);
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:card\" content=\"summary_large_image\""));
        assert!(html.contains("name=\"twitter:title\" content=\"Page\""));
        assert!(html.contains("name=\"twitter:description\" content=\"page-desc\""));
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/og.png\""));
        assert!(html.contains("name=\"twitter:image:alt\" content=\"OG preview\""));
        assert!(!html.contains("https://example.com/twitter.png"));
    }

    struct LayoutTwitterWithoutOpenGraph;

    impl Metadata for LayoutTwitterWithoutOpenGraph {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Page")
        }

        fn description(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("page-desc")
        }

        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                card: CascadeValue::Set(TwitterCard::SummaryLargeImage),
                site: CascadeValue::Set("@site"),
                image: CascadeValue::Set("https://example.com/twitter.png"),
                image_alt: CascadeValue::Set("Twitter preview"),
                ..Default::default()
            }
        }
    }

    #[test]
    fn tombstoning_twitter_image_without_open_graph_drops_only_image_tags() {
        let providers: &[MetadataRef] = &[&LayoutTwitterWithoutOpenGraph, &TombstoneTwitterImage];
        let mut computed = ComputedRouteMeta::default();
        computed = ComputedRouteMeta::cascade(&computed, providers);
        assert!(computed.twitter.image.is_none());
        assert!(computed.twitter.image_alt.is_none());
        assert_eq!(computed.twitter.card, Some(TwitterCard::SummaryLargeImage));
        assert_eq!(computed.twitter.site, Some("@site"));

        let resolved = resolved_metadata_from_computed(&computed);
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:card\" content=\"summary\""));
        assert!(!html.contains("summary_large_image"), "{html}");
        assert!(html.contains("name=\"twitter:site\" content=\"@site\""));
        assert!(html.contains("name=\"twitter:title\" content=\"Page\""));
        assert!(html.contains("name=\"twitter:description\" content=\"page-desc\""));
        assert!(!html.contains("twitter:image"));
        assert!(!html.contains("https://example.com/twitter.png"));
    }

    fn render_providers(providers: &[MetadataRef]) -> String {
        let computed = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), providers);
        let resolved = resolved_metadata_from_computed(&computed);
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved);
        cx.into_output().into_string()
    }

    struct PageTwitterImage;

    impl Metadata for PageTwitterImage {
        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                image: CascadeValue::Set("https://example.com/page.png"),
                ..Default::default()
            }
        }
    }

    #[test]
    fn replacing_twitter_image_drops_inherited_alt() {
        let html = render_providers(&[&LayoutTwitterWithoutOpenGraph, &PageTwitterImage]);
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/page.png\""));
        assert!(
            !html.contains("Twitter preview"),
            "the layout alt describes the layout image: {html}"
        );
    }

    #[test]
    fn replacing_twitter_image_keeps_alt_set_by_the_same_provider() {
        struct PageImageWithAlt;

        impl Metadata for PageImageWithAlt {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    image: CascadeValue::Set("https://example.com/page.png"),
                    image_alt: CascadeValue::Set("Page preview"),
                    ..Default::default()
                }
            }
        }

        let html = render_providers(&[&LayoutTwitterWithoutOpenGraph, &PageImageWithAlt]);
        assert!(html.contains("name=\"twitter:image:alt\" content=\"Page preview\""));
    }

    #[test]
    fn twitter_alt_without_twitter_image_describes_open_graph_image() {
        struct AltOnly;

        impl Metadata for AltOnly {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    image_alt: CascadeValue::Set("Twitter copy of the OG alt"),
                    ..Default::default()
                }
            }
        }

        let html = render_providers(&[&LayoutTwitterImageOnly, &TombstoneTwitterImage, &AltOnly]);
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/og.png\""));
        assert!(html.contains("name=\"twitter:image:alt\" content=\"Twitter copy of the OG alt\""));
    }

    struct LayoutTwitterTemplates;

    impl Metadata for LayoutTwitterTemplates {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("%s | Site")
        }

        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                site: CascadeValue::Set("@site"),
                title: CascadeValue::Set("%s on Site"),
                description: CascadeValue::Set("Read about %s"),
                ..Default::default()
            }
        }
    }

    struct PricingPage;

    impl Metadata for PricingPage {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Pricing")
        }

        fn description(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("plans")
        }
    }

    #[test]
    fn twitter_templates_format_page_title_and_description() {
        let html = render_providers(&[&LayoutTwitterTemplates, &PricingPage]);
        assert!(html.contains("name=\"twitter:title\" content=\"Pricing on Site\""));
        assert!(html.contains("name=\"twitter:description\" content=\"Read about plans\""));
        assert!(!html.contains("%s"), "{html}");

        let resolved = resolve_metadata_stack(&[&LayoutTwitterTemplates, &PricingPage])
            .expect("metadata should resolve");
        assert_eq!(resolved.twitter.title.as_deref(), Some("Pricing on Site"));
        assert_eq!(
            resolved.twitter.description.as_deref(),
            Some("Read about plans")
        );
    }

    #[test]
    fn twitter_title_value_is_formatted_through_inherited_template() {
        struct PageTwitterTitle;

        impl Metadata for PageTwitterTitle {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    title: CascadeValue::Set("Plans"),
                    ..Default::default()
                }
            }
        }

        let providers: &[MetadataRef] = &[&LayoutTwitterTemplates, &PricingPage, &PageTwitterTitle];
        let html = render_providers(providers);
        assert!(html.contains("name=\"twitter:title\" content=\"Plans on Site\""));
        let resolved = resolve_metadata_stack(providers).expect("metadata should resolve");
        assert_eq!(resolved.twitter.title.as_deref(), Some("Plans on Site"));
    }

    #[test]
    fn tombstoned_twitter_title_template_falls_back_to_open_graph_and_page_title() {
        struct ClearTwitterTitle;

        impl Metadata for ClearTwitterTitle {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    title: CascadeValue::Tombstone,
                    ..Default::default()
                }
            }
        }

        let html = render_providers(&[&LayoutTwitterTemplates, &ClearTwitterTitle, &PricingPage]);
        assert!(html.contains("name=\"twitter:title\" content=\"Pricing | Site\""));
    }

    struct NoTwitter;

    impl Metadata for NoTwitter {
        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                card: CascadeValue::Tombstone,
                ..Default::default()
            }
        }
    }

    #[test]
    fn tombstoning_twitter_card_turns_twitter_tags_off() {
        let providers: &[MetadataRef] = &[&LayoutTwitter, &PageTwitter, &NoTwitter];
        let html = render_providers(providers);
        assert!(!html.contains("twitter:"), "{html}");
        assert!(
            html.contains("property=\"og:image\""),
            "Open Graph is unaffected: {html}"
        );

        let resolved = resolve_metadata_stack(providers).expect("metadata should resolve");
        assert!(resolved.twitter.is_empty());
    }

    #[test]
    fn twitter_fields_after_card_tombstone_turn_tags_back_on() {
        let html = render_providers(&[&LayoutTwitter, &NoTwitter, &PageTwitter]);
        assert!(html.contains("name=\"twitter:creator\" content=\"@author\""));
        assert!(
            !html.contains("@site"),
            "layout fields stay cleared: {html}"
        );
        assert!(!html.contains("https://example.com/twitter.png"), "{html}");
        assert!(html.contains("name=\"twitter:card\" content=\"summary_large_image\""));
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/og.png\""));
    }

    #[test]
    fn enabled_twitter_without_fields_uses_open_graph_and_page_copy() {
        let metadata = ResolvedMetadata {
            title: Some("Hello".to_string()),
            twitter: ResolvedTwitter {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &metadata);
        let html = cx.into_output().into_string();
        assert!(html.contains("name=\"twitter:card\" content=\"summary\""));
        assert!(html.contains("name=\"twitter:title\" content=\"Hello\""));
    }

    struct HomeLayout;

    impl Metadata for HomeLayout {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Home")
        }

        fn description(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("home-desc")
        }

        fn twitter(&self) -> CascadeTwitter {
            CascadeTwitter {
                title: CascadeValue::Set("%s on Site"),
                description: CascadeValue::Set("About %s"),
                ..Default::default()
            }
        }
    }

    fn nested(parent: &[MetadataRef], child: &[MetadataRef]) -> ResolvedTwitter {
        let parent = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), parent);
        assert_eq!(parent.twitter.title.as_deref(), Some("Home on Site"));
        let child = ComputedRouteMeta::cascade(&parent, child);
        resolved_metadata_from_computed(&child).twitter
    }

    #[test]
    fn nested_route_refills_twitter_templates_with_its_own_copy() {
        let twitter = nested(&[&HomeLayout], &[&PricingPage]);
        assert_eq!(twitter.title.as_deref(), Some("Pricing on Site"));
        assert_eq!(twitter.description.as_deref(), Some("About plans"));
    }

    #[test]
    fn nested_route_without_its_own_title_keeps_the_inherited_fill() {
        struct Untitled;
        impl Metadata for Untitled {}

        let twitter = nested(&[&HomeLayout], &[&Untitled]);
        assert_eq!(twitter.title.as_deref(), Some("Home on Site"));
        assert_eq!(twitter.description.as_deref(), Some("About home-desc"));
    }

    #[test]
    fn nested_route_refills_with_a_new_template_and_the_inherited_title() {
        struct NewTemplate;

        impl Metadata for NewTemplate {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    title: CascadeValue::Set("%s, now on Site"),
                    ..Default::default()
                }
            }
        }

        let twitter = nested(&[&HomeLayout], &[&NewTemplate]);
        assert_eq!(twitter.title.as_deref(), Some("Home, now on Site"));
    }

    #[test]
    fn explicit_twitter_title_is_not_refilled_by_descendants() {
        struct ExplicitTitle;

        impl Metadata for ExplicitTitle {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    title: CascadeValue::Set("Plans"),
                    ..Default::default()
                }
            }
        }

        let parent = ComputedRouteMeta::cascade(
            &ComputedRouteMeta::default(),
            &[&HomeLayout, &ExplicitTitle],
        );
        assert_eq!(parent.twitter.title.as_deref(), Some("Plans on Site"));
        let child = ComputedRouteMeta::cascade(&parent, &[&PricingPage]);
        assert_eq!(child.twitter.title.as_deref(), Some("Plans on Site"));
    }

    #[test]
    fn nested_absolute_title_drops_the_inherited_fill() {
        struct AbsolutePage;

        impl Metadata for AbsolutePage {
            fn title(&self) -> CascadeValue<&'static str> {
                CascadeValue::Set("Standalone")
            }

            fn absolute_title(&self) -> bool {
                true
            }
        }

        let twitter = nested(&[&HomeLayout], &[&AbsolutePage]);
        assert!(twitter.title.is_none(), "{:?}", twitter.title);

        let parent = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), &[&HomeLayout]);
        let child = ComputedRouteMeta::cascade(&parent, &[&AbsolutePage]);
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved_metadata_from_computed(&child));
        let html = cx.into_output().into_string();
        assert!(
            html.contains("name=\"twitter:title\" content=\"Standalone\""),
            "{html}"
        );
    }

    struct OgHomeLayout;

    impl Metadata for OgHomeLayout {
        fn title(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("Home")
        }

        fn description(&self) -> CascadeValue<&'static str> {
            CascadeValue::Set("home-desc")
        }

        fn open_graph(&self) -> CascadeOpenGraph {
            CascadeOpenGraph {
                title: CascadeValue::Set("%s on OG"),
                description: CascadeValue::Set("About %s"),
                ..Default::default()
            }
        }
    }

    fn nested_og(child: &[MetadataRef]) -> ComputedOpenGraph {
        let parent = ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), &[&OgHomeLayout]);
        assert_eq!(parent.open_graph.title.as_deref(), Some("Home on OG"));
        assert_eq!(
            parent.open_graph.description.as_deref(),
            Some("About home-desc")
        );
        ComputedRouteMeta::cascade(&parent, child).open_graph
    }

    #[test]
    fn nested_route_refills_open_graph_templates_with_its_own_copy() {
        let og = nested_og(&[&PricingPage]);
        assert_eq!(og.title.as_deref(), Some("Pricing on OG"));
        assert_eq!(og.description.as_deref(), Some("About plans"));
    }

    #[test]
    fn nested_route_without_its_own_title_keeps_the_inherited_open_graph_fill() {
        struct Untitled;
        impl Metadata for Untitled {}

        let og = nested_og(&[&Untitled]);
        assert_eq!(og.title.as_deref(), Some("Home on OG"));
        assert_eq!(og.description.as_deref(), Some("About home-desc"));
    }

    #[test]
    fn nested_route_refills_open_graph_with_a_new_template() {
        struct NewTemplate;

        impl Metadata for NewTemplate {
            fn open_graph(&self) -> CascadeOpenGraph {
                CascadeOpenGraph {
                    title: CascadeValue::Set("%s, now on OG"),
                    ..Default::default()
                }
            }
        }

        let og = nested_og(&[&NewTemplate]);
        assert_eq!(og.title.as_deref(), Some("Home, now on OG"));
    }

    #[test]
    fn explicit_open_graph_title_is_not_refilled_by_descendants() {
        struct ExplicitTitle;

        impl Metadata for ExplicitTitle {
            fn open_graph(&self) -> CascadeOpenGraph {
                CascadeOpenGraph {
                    title: CascadeValue::Set("Plans"),
                    ..Default::default()
                }
            }
        }

        let parent = ComputedRouteMeta::cascade(
            &ComputedRouteMeta::default(),
            &[&OgHomeLayout, &ExplicitTitle],
        );
        assert_eq!(parent.open_graph.title.as_deref(), Some("Plans on OG"));
        let child = ComputedRouteMeta::cascade(&parent, &[&PricingPage]);
        assert_eq!(child.open_graph.title.as_deref(), Some("Plans on OG"));
    }

    #[test]
    fn nested_route_without_description_drops_the_inherited_open_graph_fill() {
        struct NoDescription;

        impl Metadata for NoDescription {
            fn description(&self) -> CascadeValue<&'static str> {
                CascadeValue::Tombstone
            }
        }

        let og = nested_og(&[&NoDescription]);
        assert!(og.description.is_none(), "{:?}", og.description);
    }

    #[test]
    fn metadata_stack_keeps_open_graph_media_and_twitter_image() {
        let resolved = resolve_metadata_stack(&[&LayoutTwitterImageOnly, &TombstoneTwitterImage])
            .expect("metadata should resolve");
        let mut cx = Ctx::new();
        render_metadata_head(&mut cx, &resolved);
        let html = cx.into_output().into_string();
        assert!(html.contains("property=\"og:image\" content=\"https://example.com/og.png\""));
        assert!(html.contains("name=\"twitter:card\" content=\"summary_large_image\""));
        assert!(html.contains("name=\"twitter:image\" content=\"https://example.com/og.png\""));
    }

    mod split_invariance {
        use super::super::*;

        macro_rules! provider {
            ($name:ident { $($body:tt)* }) => {
                struct $name;
                impl Metadata for $name {
                    $($body)*
                }
            };
        }

        provider!(TitleTemplate {
            fn title(&self) -> CascadeValue<&'static str> {
                CascadeValue::Set("%s | Site")
            }
        });
        provider!(Home {
            fn title(&self) -> CascadeValue<&'static str> {
                CascadeValue::Set("Home")
            }
            fn description(&self) -> CascadeValue<&'static str> {
                CascadeValue::Set("home-desc")
            }
        });
        provider!(Pricing {
            fn title(&self) -> CascadeValue<&'static str> {
                CascadeValue::Set("Pricing")
            }
            fn description(&self) -> CascadeValue<&'static str> {
                CascadeValue::Set("plans")
            }
        });
        provider!(Absolute {
            fn title(&self) -> CascadeValue<&'static str> {
                CascadeValue::Set("Standalone")
            }
            fn absolute_title(&self) -> bool {
                true
            }
        });
        provider!(ClearTitle {
            fn title(&self) -> CascadeValue<&'static str> {
                CascadeValue::Tombstone
            }
        });
        provider!(ClearDescription {
            fn description(&self) -> CascadeValue<&'static str> {
                CascadeValue::Tombstone
            }
        });
        provider!(OgTemplates {
            fn open_graph(&self) -> CascadeOpenGraph {
                CascadeOpenGraph {
                    title: CascadeValue::Set("%s on OG"),
                    description: CascadeValue::Set("About %s"),
                    ..Default::default()
                }
            }
        });
        provider!(OgPlain {
            fn open_graph(&self) -> CascadeOpenGraph {
                static IMAGES: [OpenGraphImage; 1] =
                    [OpenGraphImage::new("https://example.com/og.png").with_alt("OG alt")];
                CascadeOpenGraph {
                    title: CascadeValue::Set("Plans"),
                    site_name: CascadeValue::Set("Site"),
                    images: CascadeValue::Set(&IMAGES),
                    ..Default::default()
                }
            }
        });
        provider!(OgClear {
            fn open_graph(&self) -> CascadeOpenGraph {
                CascadeOpenGraph {
                    title: CascadeValue::Tombstone,
                    description: CascadeValue::Tombstone,
                    images: CascadeValue::Tombstone,
                    ..Default::default()
                }
            }
        });
        provider!(TwitterTemplates {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    site: CascadeValue::Set("@site"),
                    title: CascadeValue::Set("%s on X"),
                    description: CascadeValue::Set("X %s"),
                    ..Default::default()
                }
            }
        });
        provider!(TwitterImage {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    title: CascadeValue::Set("Tw"),
                    image: CascadeValue::Set("https://example.com/x.png"),
                    image_alt: CascadeValue::Set("X alt"),
                    ..Default::default()
                }
            }
        });
        provider!(TwitterClear {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    title: CascadeValue::Tombstone,
                    image: CascadeValue::Tombstone,
                    ..Default::default()
                }
            }
        });
        provider!(TwitterOff {
            fn twitter(&self) -> CascadeTwitter {
                CascadeTwitter {
                    card: CascadeValue::Tombstone,
                    ..Default::default()
                }
            }
        });

        fn head(metadata: &ResolvedMetadata) -> String {
            let mut cx = Ctx::new();
            render_metadata_head(&mut cx, metadata);
            cx.into_output().into_string()
        }

        /// The precomputed map cascades one route level at a time, while the
        /// request-time fallback resolves the whole stack at once. Splitting a
        /// stack across route levels must not change the rendered head.
        #[test]
        fn splitting_a_stack_across_route_levels_keeps_the_same_head() {
            let providers: [MetadataRef; 13] = [
                &TitleTemplate,
                &Home,
                &Pricing,
                &Absolute,
                &ClearTitle,
                &ClearDescription,
                &OgTemplates,
                &OgPlain,
                &OgClear,
                &TwitterTemplates,
                &TwitterImage,
                &TwitterClear,
                &TwitterOff,
            ];
            let mut stacks: Vec<Vec<MetadataRef>> = Vec::new();
            for &a in &providers {
                for &b in &providers {
                    stacks.push(vec![a, b]);
                    for &c in &providers {
                        stacks.push(vec![a, b, c]);
                    }
                }
            }

            for stack in &stacks {
                let flat = head(&resolve_metadata_stack(stack).expect("non-empty stack"));
                for split in 1..stack.len() {
                    let parent =
                        ComputedRouteMeta::cascade(&ComputedRouteMeta::default(), &stack[..split]);
                    let child = ComputedRouteMeta::cascade(&parent, &stack[split..]);
                    let nested = head(&resolved_metadata_from_computed(&child));
                    assert_eq!(
                        nested,
                        flat,
                        "stack of {} providers split after {split}",
                        stack.len()
                    );
                }
            }
        }
    }
}
