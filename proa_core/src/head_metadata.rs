// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Typed, inheritable document metadata. Values are prepared before publication.
use std::borrow::Cow;

use crate::{HeadMedia, OpenGraph};

/// A metadata patch: omission inherits, `Set` replaces, and `Clear` removes.
#[derive(Clone, Debug, Default)]
pub enum HeadValue<T> {
    #[default]
    Inherit,
    Set(T),
    Clear,
}
impl<T> From<T> for HeadValue<T> {
    fn from(value: T) -> Self {
        Self::Set(value)
    }
}
impl From<&'static str> for HeadValue<Cow<'static, str>> {
    fn from(value: &'static str) -> Self {
        Self::Set(value.into())
    }
}
impl From<String> for HeadValue<Cow<'static, str>> {
    fn from(value: String) -> Self {
        Self::Set(value.into())
    }
}
impl<T> HeadValue<T> {
    pub(crate) fn changed(&self) -> bool {
        !matches!(self, Self::Inherit)
    }
    pub(crate) fn apply(self, target: &mut Option<T>) {
        match self {
            Self::Inherit => {}
            Self::Set(value) => *target = Some(value),
            Self::Clear => *target = None,
        }
    }
    pub(crate) fn try_map<U>(
        self,
        f: impl FnOnce(T) -> Result<U, crate::WriteError>,
    ) -> Result<HeadValue<U>, crate::WriteError> {
        Ok(match self {
            Self::Inherit => HeadValue::Inherit,
            Self::Set(value) => HeadValue::Set(f(value)?),
            Self::Clear => HeadValue::Clear,
        })
    }
}

/// A normal title uses the ancestor template; an absolute title bypasses it.
#[derive(Clone, Debug)]
pub enum HeadTitle {
    Text(Cow<'static, str>),
    Absolute(Cow<'static, str>),
}
impl HeadTitle {
    pub fn absolute(value: impl Into<Cow<'static, str>>) -> Self {
        Self::Absolute(value.into())
    }
}
impl From<&'static str> for HeadValue<HeadTitle> {
    fn from(value: &'static str) -> Self {
        Self::Set(HeadTitle::Text(value.into()))
    }
}
impl From<String> for HeadValue<HeadTitle> {
    fn from(value: String) -> Self {
        Self::Set(HeadTitle::Text(value.into()))
    }
}
impl From<Cow<'static, str>> for HeadValue<HeadTitle> {
    fn from(value: Cow<'static, str>) -> Self {
        Self::Set(HeadTitle::Text(value))
    }
}

/// An atomic metadata patch for a layout or Page's head hook.
///
/// Default fields inherit. `HeadValue::Clear` removes an inherited field.
/// Lists replace whole lists. Open Graph and Twitter builders patch individual
/// fields; clearing either outer field removes the complete inherited object.
#[derive(Clone, Debug, Default)]
pub struct HeadMetadata {
    pub title: HeadValue<HeadTitle>,
    pub description: HeadValue<Cow<'static, str>>,
    pub title_template: HeadValue<Cow<'static, str>>,
    pub canonical: HeadValue<Cow<'static, str>>,
    pub open_graph: HeadValue<OpenGraph>,
    pub twitter: HeadValue<Twitter>,
    pub robots: HeadValue<Robots>,
    pub googlebot: HeadValue<Robots>,
    pub icons: HeadValue<Vec<HeadIcon>>,
    pub alternates: HeadValue<Vec<HeadAlternate>>,
    pub manifest: HeadValue<Cow<'static, str>>,
    pub authors: HeadValue<Vec<HeadAuthor>>,
    pub application_name: HeadValue<Cow<'static, str>>,
    pub creator: HeadValue<Cow<'static, str>>,
    pub publisher: HeadValue<Cow<'static, str>>,
    pub keywords: HeadValue<Vec<Cow<'static, str>>>,
    pub referrer: HeadValue<ReferrerPolicy>,
    pub verification: HeadValue<Vec<HeadVerification>>,
}

/// Directives for one HTML page, separate from the site's `RobotsTxt`.
/// An omitted directive emits nothing. A Page's `Robots` replaces its ancestor's
/// whole policy; Google-specific directives use `HeadMetadata::googlebot`.
#[derive(Clone, Debug, Default)]
pub struct Robots {
    pub index: Option<bool>,
    pub follow: Option<bool>,
    pub no_archive: bool,
    pub no_snippet: bool,
    pub no_image_index: bool,
    pub no_translate: bool,
    pub index_if_embedded: bool,
    /// `-1` means unlimited; smaller values are invalid.
    pub max_snippet: Option<i32>,
    /// `-1` means unlimited; smaller values are invalid.
    pub max_video_preview: Option<i32>,
    pub max_image_preview: Option<ImagePreview>,
}
#[derive(Clone, Copy, Debug)]
pub enum ImagePreview {
    None,
    Standard,
    Large,
}
impl ImagePreview {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Standard => "standard",
            Self::Large => "large",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum ReferrerPolicy {
    NoReferrer,
    NoReferrerWhenDowngrade,
    Origin,
    OriginWhenCrossOrigin,
    SameOrigin,
    StrictOrigin,
    StrictOriginWhenCrossOrigin,
    UnsafeUrl,
}
impl ReferrerPolicy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoReferrer => "no-referrer",
            Self::NoReferrerWhenDowngrade => "no-referrer-when-downgrade",
            Self::Origin => "origin",
            Self::OriginWhenCrossOrigin => "origin-when-cross-origin",
            Self::SameOrigin => "same-origin",
            Self::StrictOrigin => "strict-origin",
            Self::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
            Self::UnsafeUrl => "unsafe-url",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum TwitterCard {
    #[default]
    Summary,
    SummaryLargeImage,
}
impl TwitterCard {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::SummaryLargeImage => "summary_large_image",
        }
    }
}
/// A Twitter/X card patch. Unset title/description/images fall back to the final
/// Open Graph/document values. Explicit clears suppress their fallback.
#[derive(Clone, Debug, Default)]
pub struct Twitter {
    pub(crate) card: Option<TwitterCard>,
    pub(crate) scalars: [HeadValue<Cow<'static, str>>; 6],
    pub(crate) image: HeadValue<HeadMedia>,
}
macro_rules! twitter_scalar {
    ($set:ident, $clear:ident, $index:literal) => {
        pub fn $set(mut self, value: impl Into<Cow<'static, str>>) -> Self {
            self.scalars[$index] = HeadValue::Set(value.into());
            self
        }
        pub fn $clear(mut self) -> Self {
            self.scalars[$index] = HeadValue::Clear;
            self
        }
    };
}
impl Twitter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn card(mut self, card: TwitterCard) -> Self {
        self.card = Some(card);
        self
    }
    twitter_scalar!(title, clear_title, 0);
    twitter_scalar!(description, clear_description, 1);
    twitter_scalar!(site, clear_site, 2);
    twitter_scalar!(site_id, clear_site_id, 3);
    twitter_scalar!(creator, clear_creator, 4);
    twitter_scalar!(creator_id, clear_creator_id, 5);
    pub fn image(mut self, image: HeadMedia) -> Self {
        self.image = HeadValue::Set(image);
        self
    }
    pub fn clear_image(mut self) -> Self {
        self.image = HeadValue::Clear;
        self
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum IconKind {
    #[default]
    Icon,
    AppleTouch,
    Mask,
}
impl IconKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Icon => "icon",
            Self::AppleTouch => "apple-touch-icon",
            Self::Mask => "mask-icon",
        }
    }
}
#[derive(Clone, Debug)]
pub struct HeadIcon {
    pub(crate) url: Cow<'static, str>,
    pub(crate) kind: IconKind,
    pub(crate) sizes: Option<Cow<'static, str>>,
    pub(crate) mime: Option<Cow<'static, str>>,
    pub(crate) media: Option<Cow<'static, str>>,
    pub(crate) color: Option<Cow<'static, str>>,
}
macro_rules! text_builder {
    ($method:ident, $field:ident) => {
        pub fn $method(mut self, value: impl Into<Cow<'static, str>>) -> Self {
            self.$field = Some(value.into());
            self
        }
    };
}
impl HeadIcon {
    pub fn new(url: impl Into<Cow<'static, str>>) -> Self {
        Self {
            url: url.into(),
            kind: IconKind::Icon,
            sizes: None,
            mime: None,
            media: None,
            color: None,
        }
    }
    pub fn kind(mut self, kind: IconKind) -> Self {
        self.kind = kind;
        self
    }
    text_builder!(sizes, sizes);
    text_builder!(mime_type, mime);
    text_builder!(media, media);
    text_builder!(color, color);
}

/// An alternate language, feed, or media-specific representation of a page.
#[derive(Clone, Debug)]
pub struct HeadAlternate {
    pub(crate) url: Cow<'static, str>,
    pub(crate) language: Option<Cow<'static, str>>,
    pub(crate) mime: Option<Cow<'static, str>>,
    pub(crate) media: Option<Cow<'static, str>>,
    pub(crate) title: Option<Cow<'static, str>>,
}
impl HeadAlternate {
    pub fn new(url: impl Into<Cow<'static, str>>) -> Self {
        Self {
            url: url.into(),
            language: None,
            mime: None,
            media: None,
            title: None,
        }
    }
    text_builder!(language, language);
    text_builder!(mime_type, mime);
    text_builder!(media, media);
    text_builder!(title, title);
}

#[derive(Clone, Debug)]
pub struct HeadAuthor {
    pub(crate) name: Cow<'static, str>,
    pub(crate) url: Option<Cow<'static, str>>,
}
impl HeadAuthor {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            url: None,
        }
    }
    text_builder!(url, url);
}

/// A named site-verification tag. Repeated names may carry multiple tokens.
#[derive(Clone, Debug)]
pub struct HeadVerification {
    pub(crate) name: Cow<'static, str>,
    pub(crate) token: Cow<'static, str>,
}
impl HeadVerification {
    pub fn new(name: impl Into<Cow<'static, str>>, token: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            token: token.into(),
        }
    }
    pub fn google(token: impl Into<Cow<'static, str>>) -> Self {
        Self::new("google-site-verification", token)
    }
}

/// Article-specific Open Graph properties. Author values are profile URLs.
/// Setting this object replaces its inherited article fields as one unit.
#[derive(Clone, Debug, Default)]
pub struct ArticleMetadata {
    pub published_time: Option<Cow<'static, str>>,
    pub modified_time: Option<Cow<'static, str>>,
    pub expiration_time: Option<Cow<'static, str>>,
    pub authors: Vec<Cow<'static, str>>,
    pub section: Option<Cow<'static, str>>,
    pub tags: Vec<Cow<'static, str>>,
}
