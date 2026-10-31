// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Optional metadata families. Ordinary documents keep only a null Arc pointer.
use super::*;
use crate::{HeadAlternate, HeadAuthor, HeadIcon, HeadVerification, Robots, TwitterCard};

const TWITTER_NAMES: [&str; 6] = [
    "twitter:title",
    "twitter:description",
    "twitter:site",
    "twitter:site:id",
    "twitter:creator",
    "twitter:creator:id",
];
#[derive(Clone, Default)]
pub(super) struct ExtendedHead {
    twitter: Option<Arc<ResolvedTwitter>>,
    icons: Option<Arc<[HeadIcon]>>,
    alternates: Option<Arc<[HeadAlternate]>>,
    manifest: Option<HeadText>,
    authors: Option<Arc<[HeadAuthor]>>,
    verification: Option<Arc<[HeadVerification]>>,
}
// Owned social text is shared like Open Graph text. A request that changes a
// single card property need not duplicate inherited dynamic strings.
#[derive(Clone, Default)]
struct ResolvedTwitter {
    card: Option<TwitterCard>,
    scalars: [HeadValue<HeadText>; 6],
    image: HeadValue<HeadMedia>,
}
#[derive(Default)]
pub(super) struct PreparedExtended {
    replacement: Option<Arc<ExtendedHead>>,
    named: Vec<(Cow<'static, str>, Option<HeadText>)>,
    remove_overrides: Vec<Cow<'static, str>>,
}
fn resolve(
    value: &mut Cow<'static, str>,
    policy: Option<&HeadUrlPolicy>,
) -> Result<(), WriteError> {
    *value = policy
        .ok_or_else(|| WriteError::render("missing trusted URL policy"))?
        .resolve(value, false)?
        .into();
    Ok(())
}
fn optional_text(value: &Option<Cow<'static, str>>) -> Result<(), WriteError> {
    if let Some(value) = value {
        text(value)?;
    }
    Ok(())
}
fn list_limit(size: usize) -> Result<(), WriteError> {
    if size > SLOT_LIMIT {
        Err(invalid())
    } else {
        Ok(())
    }
}
fn prepare_named<T>(
    output: &mut PreparedExtended,
    name: &'static str,
    value: HeadValue<T>,
    prepare: impl FnOnce(T) -> Result<Cow<'static, str>, WriteError>,
) -> Result<(), WriteError> {
    match value {
        HeadValue::Inherit => {}
        HeadValue::Clear => output.named.push((name.into(), None)),
        HeadValue::Set(value) => {
            let value = prepare(value)?;
            text(&value)?;
            output.named.push((name.into(), Some(value.into())));
        }
    }
    Ok(())
}
fn prepare_text(value: Cow<'static, str>) -> Result<Cow<'static, str>, WriteError> {
    text(&value)?;
    Ok(value)
}
fn prepare_robots(robots: Robots) -> Result<Cow<'static, str>, WriteError> {
    use std::fmt::Write;
    if robots.max_snippet.is_some_and(|n| n < -1)
        || robots.max_video_preview.is_some_and(|n| n < -1)
    {
        return Err(invalid());
    }
    // proa-perf-ok: typed directives are assembled during managed head preparation.
    let mut result = String::new();
    let mut directive = |value: &str| {
        if !result.is_empty() {
            result.push_str(", ");
        }
        result.push_str(value);
    };
    if let Some(index) = robots.index {
        directive(if index { "index" } else { "noindex" });
    }
    if let Some(follow) = robots.follow {
        directive(if follow { "follow" } else { "nofollow" });
    }
    for (enabled, name) in [
        (robots.no_archive, "noarchive"),
        (robots.no_snippet, "nosnippet"),
        (robots.no_image_index, "noimageindex"),
        (robots.no_translate, "notranslate"),
        (robots.index_if_embedded, "indexifembedded"),
    ] {
        if enabled {
            directive(name);
        }
    }
    for (name, value) in [
        ("max-snippet", robots.max_snippet),
        ("max-video-preview", robots.max_video_preview),
    ] {
        if let Some(value) = value {
            if !result.is_empty() {
                result.push_str(", ");
            }
            write!(&mut result, "{name}:{value}").expect("String writes are infallible");
        }
    }
    if let Some(value) = robots.max_image_preview {
        if !result.is_empty() {
            result.push_str(", ");
        }
        result.push_str("max-image-preview:");
        result.push_str(value.as_str());
    }
    Ok(result.into())
}
impl ExtendedHead {
    fn slots(&self, overridden: impl Fn(&str) -> bool) -> usize {
        self.twitter.as_ref().map_or(0, |_| {
            TWITTER_NAMES
                .into_iter()
                .chain(["twitter:card", "twitter:image", "twitter:image:alt"])
                .filter(|name| !overridden(name))
                .count()
        }) + self.icons.as_ref().map_or(0, |v| v.len())
            + self.alternates.as_ref().map_or(0, |v| v.len())
            + usize::from(self.manifest.is_some())
            + self
                .authors
                .as_ref()
                .filter(|_| !overridden("author"))
                .map_or(0, |v| {
                    v.iter().map(|a| 1 + usize::from(a.url.is_some())).sum()
                })
            + self
                .verification
                .as_ref()
                .map_or(0, |v| v.iter().filter(|v| !overridden(&v.name)).count())
    }
}
impl HeadFields {
    pub(super) fn extended_slots(&self) -> usize {
        self.extended
            .as_ref()
            .map_or(0, |e| e.slots(|name| self.overridden(name)))
    }
    pub(super) fn extended_slots_after_named(&self, name: &str) -> usize {
        self.extended.as_ref().map_or(0, |e| {
            e.slots(|n| n.eq_ignore_ascii_case(name) || self.overridden(n))
        })
    }
    #[inline(always)]
    pub(super) fn prepare_extended(
        &self,
        metadata: &mut HeadMetadata,
        policy: Option<&HeadUrlPolicy>,
    ) -> Result<PreparedExtended, WriteError> {
        let mut output = PreparedExtended::default();
        prepare_named(
            &mut output,
            "robots",
            std::mem::take(&mut metadata.robots),
            prepare_robots,
        )?;
        prepare_named(
            &mut output,
            "googlebot",
            std::mem::take(&mut metadata.googlebot),
            prepare_robots,
        )?;
        prepare_named(
            &mut output,
            "application-name",
            std::mem::take(&mut metadata.application_name),
            prepare_text,
        )?;
        prepare_named(
            &mut output,
            "creator",
            std::mem::take(&mut metadata.creator),
            prepare_text,
        )?;
        prepare_named(
            &mut output,
            "publisher",
            std::mem::take(&mut metadata.publisher),
            prepare_text,
        )?;
        prepare_named(
            &mut output,
            "referrer",
            std::mem::take(&mut metadata.referrer),
            |v| Ok(v.as_str().into()),
        )?;
        prepare_named(
            &mut output,
            "keywords",
            std::mem::take(&mut metadata.keywords),
            |values| {
                list_limit(values.len())?;
                let mut length = values.len().saturating_sub(1) * 2;
                for value in &values {
                    text(value)?;
                    length += value.len();
                }
                if length > VALUE_LIMIT {
                    return Err(invalid());
                }
                // proa-perf-ok: keyword composition happens once in head preparation.
                Ok(values.join(", ").into())
            },
        )?;
        if !metadata.twitter.changed()
            && !metadata.icons.changed()
            && !metadata.alternates.changed()
            && !metadata.manifest.changed()
            && !metadata.authors.changed()
            && !metadata.verification.changed()
        {
            return Ok(output);
        }
        self.prepare_extended_families(metadata, policy, output)
    }
    #[cold]
    fn prepare_extended_families(
        &self,
        metadata: &mut HeadMetadata,
        policy: Option<&HeadUrlPolicy>,
        mut output: PreparedExtended,
    ) -> Result<PreparedExtended, WriteError> {
        // Clone only Arc handles; existing URL strings and lists stay shared.
        let mut extra = self.extended.as_deref().cloned().unwrap_or_default();
        match std::mem::take(&mut metadata.twitter) {
            HeadValue::Inherit => {}
            HeadValue::Clear => {
                extra.twitter = None;
                output
                    .remove_overrides
                    .extend(TWITTER_NAMES.into_iter().map(Cow::Borrowed));
                output.remove_overrides.extend([
                    "twitter:card".into(),
                    "twitter:image".into(),
                    "twitter:image:alt".into(),
                ]);
            }
            HeadValue::Set(mut patch) => {
                for value in &patch.scalars {
                    if let HeadValue::Set(value) = value {
                        text(value)?;
                    }
                }
                if let HeadValue::Set(image) = &mut patch.image {
                    image.resolve(policy.ok_or_else(invalid)?)?;
                }
                let twitter = extra
                    .twitter
                    .get_or_insert_with(|| Arc::new(ResolvedTwitter::default()));
                let twitter = Arc::make_mut(twitter);
                if patch.card.is_some() {
                    twitter.card = patch.card;
                    output.remove_overrides.push("twitter:card".into());
                }
                for ((name, old), new) in TWITTER_NAMES
                    .into_iter()
                    .zip(&mut twitter.scalars)
                    .zip(patch.scalars)
                {
                    if new.changed() {
                        *old = new.try_map(|value| Ok(HeadText::from(value)))?;
                        output.remove_overrides.push(name.into());
                    }
                }
                if patch.image.changed() {
                    twitter.image = patch.image;
                    output
                        .remove_overrides
                        .extend(["twitter:image".into(), "twitter:image:alt".into()]);
                }
            }
        }
        match std::mem::take(&mut metadata.icons) {
            HeadValue::Inherit => {}
            HeadValue::Clear => extra.icons = None,
            HeadValue::Set(mut icons) => {
                list_limit(icons.len())?;
                for icon in &mut icons {
                    resolve(&mut icon.url, policy)?;
                    for value in [&icon.sizes, &icon.mime, &icon.media, &icon.color] {
                        optional_text(value)?;
                    }
                }
                extra.icons = Some(icons.into());
            }
        }
        match std::mem::take(&mut metadata.alternates) {
            HeadValue::Inherit => {}
            HeadValue::Clear => extra.alternates = None,
            HeadValue::Set(mut links) => {
                list_limit(links.len())?;
                for link in &mut links {
                    resolve(&mut link.url, policy)?;
                    for value in [&link.language, &link.mime, &link.media, &link.title] {
                        optional_text(value)?;
                    }
                }
                extra.alternates = Some(links.into());
            }
        }
        std::mem::take(&mut metadata.manifest)
            .try_map(|mut value| {
                resolve(&mut value, policy)?;
                Ok(value.into())
            })?
            .apply(&mut extra.manifest);
        match std::mem::take(&mut metadata.authors) {
            HeadValue::Inherit => {}
            HeadValue::Clear => {
                extra.authors = None;
                output.remove_overrides.push("author".into());
            }
            HeadValue::Set(mut authors) => {
                list_limit(authors.len())?;
                for author in &mut authors {
                    text(&author.name)?;
                    if let Some(url) = &mut author.url {
                        resolve(url, policy)?;
                    }
                }
                extra.authors = Some(authors.into());
                output.remove_overrides.push("author".into());
            }
        }
        match std::mem::take(&mut metadata.verification) {
            HeadValue::Inherit => {}
            value => {
                if let Some(old) = &extra.verification {
                    output
                        .remove_overrides
                        .extend(old.iter().map(|v| v.name.clone()));
                }
                match value {
                    HeadValue::Set(mut values) => {
                        list_limit(values.len())?;
                        for value in &mut values {
                            text(&value.name)?;
                            text(&value.token)?;
                            if value.name.is_empty()
                                || !value
                                    .name
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
                                || [
                                    "author",
                                    "description",
                                    "robots",
                                    "googlebot",
                                    "application-name",
                                    "creator",
                                    "publisher",
                                    "referrer",
                                    "keywords",
                                ]
                                .iter()
                                .any(|n| value.name.eq_ignore_ascii_case(n))
                                || value.name.to_ascii_lowercase().starts_with("twitter:")
                            {
                                return Err(invalid());
                            }
                            if value.name.bytes().any(|b| b.is_ascii_uppercase()) {
                                value.name = value.name.to_ascii_lowercase().into();
                            }
                            output.remove_overrides.push(value.name.clone());
                        }
                        extra.verification = Some(values.into());
                    }
                    HeadValue::Clear => extra.verification = None,
                    HeadValue::Inherit => unreachable!(),
                }
            }
        }
        output.replacement = Some(Arc::new(extra));
        Ok(output)
    }
    #[inline(always)]
    pub(super) fn check_extended_slots(
        &self,
        patch: &PreparedExtended,
        description: usize,
    ) -> Result<(), WriteError> {
        if patch.replacement.is_none()
            && patch.named.is_empty()
            && patch.remove_overrides.is_empty()
        {
            return if description == 0 {
                Ok(())
            } else {
                list_limit(
                    self.meta.len() + description + self.resources.len() + self.extended_slots(),
                )
            };
        }
        self.check_changed_extended_slots(patch, description)
    }
    #[cold]
    fn check_changed_extended_slots(
        &self,
        patch: &PreparedExtended,
        description: usize,
    ) -> Result<(), WriteError> {
        let retained = self
            .meta
            .iter()
            .filter(|(name, _)| {
                !patch
                    .remove_overrides
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(name))
            })
            .count();
        let new = patch
            .named
            .iter()
            .filter(|(name, value)| {
                value.is_some()
                    && !self
                        .meta
                        .iter()
                        .any(|(old, _)| name.eq_ignore_ascii_case(old))
            })
            .count();
        let extra = patch
            .replacement
            .as_ref()
            .or(self.extended.as_ref())
            .map_or(0, |e| {
                e.slots(|name| {
                    patch
                        .named
                        .iter()
                        .any(|(n, _)| n.eq_ignore_ascii_case(name))
                        || self.meta.iter().any(|(n, _)| {
                            n.eq_ignore_ascii_case(name)
                                && !patch
                                    .remove_overrides
                                    .iter()
                                    .any(|removed| removed.eq_ignore_ascii_case(n))
                        })
                })
            });
        list_limit(retained + new + description + self.resources.len() + extra)
    }
    #[inline(always)]
    pub(super) fn apply_extended(&mut self, patch: PreparedExtended) {
        if !patch.remove_overrides.is_empty() {
            self.meta.retain(|(name, _)| {
                !patch
                    .remove_overrides
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(name))
            });
        }
        for (name, value) in patch.named {
            if value.is_some()
                || self
                    .meta
                    .iter()
                    .any(|(old, _)| name.eq_ignore_ascii_case(old))
            {
                self.apply_named(name, value);
            }
        }
        if let Some(extra) = patch.replacement {
            self.extended = Some(extra);
        }
    }
    fn overridden(&self, name: &str) -> bool {
        self.meta
            .iter()
            .any(|(old, _)| old.eq_ignore_ascii_case(name))
    }
    pub(super) fn write_extended<B: WriteBuf>(&self, out: &mut B) -> Result<(), WriteError> {
        let Some(extra) = &self.extended else {
            return Ok(());
        };
        if let Some(twitter) = &extra.twitter {
            self.write_twitter(out, twitter)?;
        }
        if let Some(icons) = &extra.icons {
            for icon in icons.iter() {
                link(
                    out,
                    icon.kind.as_str(),
                    &icon.url,
                    &[
                        ("sizes", icon.sizes.as_deref()),
                        ("type", icon.mime.as_deref()),
                        ("media", icon.media.as_deref()),
                        ("color", icon.color.as_deref()),
                    ],
                )?;
            }
        }
        if let Some(links) = &extra.alternates {
            for value in links.iter() {
                link(
                    out,
                    "alternate",
                    &value.url,
                    &[
                        ("hreflang", value.language.as_deref()),
                        ("type", value.mime.as_deref()),
                        ("media", value.media.as_deref()),
                        ("title", value.title.as_deref()),
                    ],
                )?;
            }
        }
        if let Some(manifest) = &extra.manifest {
            link(out, "manifest", manifest, &[])?;
        }
        if let Some(authors) = &extra.authors {
            if !self.overridden("author") {
                for author in authors.iter() {
                    named(out, "author", &author.name)?;
                    if let Some(url) = &author.url {
                        link(out, "author", url, &[])?;
                    }
                }
            }
        }
        if let Some(values) = &extra.verification {
            for value in values.iter() {
                if !self.overridden(&value.name) {
                    named(out, &value.name, &value.token)?;
                }
            }
        }
        Ok(())
    }
    fn write_twitter<B: WriteBuf>(
        &self,
        out: &mut B,
        twitter: &ResolvedTwitter,
    ) -> Result<(), WriteError> {
        if !self.overridden("twitter:card") {
            named(
                out,
                "twitter:card",
                twitter.card.unwrap_or(TwitterCard::Summary).as_str(),
            )?;
        }
        for (index, (name, value)) in TWITTER_NAMES.into_iter().zip(&twitter.scalars).enumerate() {
            if self.overridden(name) {
                continue;
            }
            match value {
                HeadValue::Set(value) => named(out, name, value)?,
                HeadValue::Clear => {}
                HeadValue::Inherit if index < 2 => {
                    let graph_value = if self.graph.enabled == Some(true) {
                        self.graph.scalars[index]
                            .as_ref()
                            .and_then(|v| v.as_deref())
                    } else {
                        None
                    };
                    if let Some(value) = graph_value {
                        named(out, name, value)?;
                    } else if index == 0 {
                        if let Some(value) = &self.title {
                            out.extend_static(b"<meta name=\"twitter:title\" content=\"")?;
                            self.write_title(out, value, true)?;
                            out.extend_static(b"\">")?;
                        }
                    } else if let Some((_, Some(value))) = self
                        .meta
                        .iter()
                        .find(|(name, _)| name.as_ref() == "description")
                    {
                        named(out, name, value)?;
                    }
                }
                HeadValue::Inherit => {}
            }
        }
        let image = match &twitter.image {
            HeadValue::Set(image) => Some(image),
            HeadValue::Clear => None,
            HeadValue::Inherit if self.graph.enabled == Some(true) => {
                self.graph.images.as_ref().and_then(|images| images.first())
            }
            HeadValue::Inherit => None,
        };
        if let Some(image) = image {
            if !self.overridden("twitter:image") {
                named(out, "twitter:image", &image.url)?;
            }
            if !self.overridden("twitter:image") && !self.overridden("twitter:image:alt") {
                if let Some(alt) = &image.alt {
                    named(out, "twitter:image:alt", alt)?;
                }
            }
        }
        Ok(())
    }
    pub(super) fn prepare_article(
        &self,
        graph: &mut OpenGraph,
        policy: Option<&HeadUrlPolicy>,
    ) -> Result<(), WriteError> {
        if let Some(Some(article)) = &mut graph.article {
            let article = Arc::make_mut(article);
            list_limit(
                article.authors.len()
                    + article.tags.len()
                    + [
                        &article.published_time,
                        &article.modified_time,
                        &article.expiration_time,
                        &article.section,
                    ]
                    .into_iter()
                    .filter(|v| v.is_some())
                    .count(),
            )?;
            for value in [
                &article.published_time,
                &article.modified_time,
                &article.expiration_time,
                &article.section,
            ] {
                optional_text(value)?;
            }
            for author in &mut article.authors {
                resolve(author, policy)?;
            }
            for tag in &article.tags {
                text(tag)?;
            }
        }
        Ok(())
    }
    pub(super) fn write_article<B: WriteBuf>(&self, out: &mut B) -> Result<(), WriteError> {
        if self.graph.scalars[5].as_ref().and_then(|v| v.as_deref()) != Some("article") {
            return Ok(());
        }
        let Some(Some(article)) = &self.graph.article else {
            return Ok(());
        };
        for (name, value) in [
            ("article:published_time", &article.published_time),
            ("article:modified_time", &article.modified_time),
            ("article:expiration_time", &article.expiration_time),
            ("article:section", &article.section),
        ] {
            if let Some(value) = value {
                property(out, name, value)?;
            }
        }
        for author in &article.authors {
            property(out, "article:author", author)?;
        }
        for tag in &article.tags {
            property(out, "article:tag", tag)?;
        }
        Ok(())
    }
}
fn named<B: WriteBuf>(out: &mut B, name: &str, value: &str) -> Result<(), WriteError> {
    out.extend_static(b"<meta name=\"")?;
    escape_attr_value(out, name)?;
    out.extend_static(b"\" content=\"")?;
    escape_attr_value(out, value)?;
    out.extend_static(b"\">")
}
fn link<B: WriteBuf>(
    out: &mut B,
    rel: &str,
    url: &str,
    attributes: &[(&str, Option<&str>)],
) -> Result<(), WriteError> {
    out.extend_static(b"<link rel=\"")?;
    escape_attr_value(out, rel)?;
    out.extend_static(b"\" href=\"")?;
    escape_attr_value(out, url)?;
    out.extend_static(b"\"")?;
    for (name, value) in attributes {
        if let Some(value) = value {
            out.extend_static(b" ")?;
            escape_attr_value(out, name)?;
            out.extend_static(b"=\"")?;
            escape_attr_value(out, value)?;
            out.extend_static(b"\"")?;
        }
    }
    out.extend_static(b">")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Twitter;
    use crate::{
        ArticleMetadata, HeadMetadata, HeadTitle, HeadValue, IconKind, ImagePreview, ReferrerPolicy,
    };

    fn policy() -> HeadUrlPolicy {
        HeadUrlPolicy::new("https://example.com", "/shop").unwrap()
    }
    fn apply(fields: &mut HeadFields, mut metadata: HeadMetadata) -> Result<(), WriteError> {
        let prepared = fields.prepare_metadata(&mut metadata, Some(&policy()))?;
        fields.apply_metadata(prepared);
        Ok(())
    }
    fn html(fields: &HeadFields) -> String {
        let mut buffer = crate::RenderBuffer::new();
        fields.write(&mut buffer, None).unwrap();
        buffer.into_string()
    }
    #[test]
    fn typed_families_resolve_urls_escape_attributes_and_preserve_repeated_values() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                title: "A & B".into(),
                description: "A < B".into(),
                canonical: "/products/42".into(),
                robots: Robots {
                    index: Some(false),
                    follow: Some(true),
                    max_image_preview: Some(ImagePreview::Large),
                    max_snippet: Some(-1),
                    ..Default::default()
                }
                .into(),
                googlebot: Robots {
                    no_snippet: true,
                    ..Default::default()
                }
                .into(),
                twitter: Twitter::new()
                    .card(TwitterCard::SummaryLargeImage)
                    .site("@acme")
                    .image(HeadMedia::new("/preview.png").alt("\"Green\" & blue"))
                    .into(),
                icons: vec![
                    HeadIcon::new("/favicon.svg")
                        .mime_type("image/svg+xml")
                        .sizes("any"),
                    HeadIcon::new("/apple.png")
                        .kind(IconKind::AppleTouch)
                        .sizes("180x180"),
                ]
                .into(),
                alternates: vec![
                    HeadAlternate::new("/fr/products/42").language("fr"),
                    HeadAlternate::new("/feed.xml")
                        .mime_type("application/rss+xml")
                        .title("A & B"),
                ]
                .into(),
                manifest: "/app.webmanifest".into(),
                authors: vec![
                    HeadAuthor::new("A & B").url("/authors/a"),
                    HeadAuthor::new("C"),
                ]
                .into(),
                verification: vec![
                    HeadVerification::google("one"),
                    HeadVerification::google("two\"&"),
                    HeadVerification::new("msvalidate.01", "bing"),
                ]
                .into(),
                application_name: "Store".into(),
                creator: "A".into(),
                publisher: "B".into(),
                referrer: ReferrerPolicy::StrictOriginWhenCrossOrigin.into(),
                keywords: vec!["outdoors".into(), "travel".into()].into(),
                ..Default::default()
            },
        )
        .unwrap();
        let output = html(&fields);
        for expected in [
            "<title>A &amp; B</title>",
            "<meta name=\"robots\" content=\"noindex, follow, max-snippet:-1, max-image-preview:large\">",
            "<meta name=\"googlebot\" content=\"nosnippet\">",
            "<meta name=\"twitter:title\" content=\"A &amp; B\">",
            "<meta name=\"twitter:description\" content=\"A &lt; B\">",
            "<meta name=\"twitter:image\" content=\"https://example.com/shop/preview.png\">",
            "<meta name=\"twitter:image:alt\" content=\"&quot;Green&quot; &amp; blue\">",
            "<link rel=\"icon\" href=\"https://example.com/shop/favicon.svg\" sizes=\"any\" type=\"image/svg+xml\">",
            "<link rel=\"apple-touch-icon\" href=\"https://example.com/shop/apple.png\" sizes=\"180x180\">",
            "<link rel=\"alternate\" href=\"https://example.com/shop/fr/products/42\" hreflang=\"fr\">",
            "<link rel=\"alternate\" href=\"https://example.com/shop/feed.xml\" type=\"application/rss+xml\" title=\"A &amp; B\">",
            "<link rel=\"manifest\" href=\"https://example.com/shop/app.webmanifest\">",
            "<meta name=\"author\" content=\"A &amp; B\"><link rel=\"author\" href=\"https://example.com/shop/authors/a\">",
            "<meta name=\"referrer\" content=\"strict-origin-when-cross-origin\">",
            "<meta name=\"keywords\" content=\"outdoors, travel\">",
        ] { assert!(output.contains(expected), "missing {expected}: {output}"); }
        assert_eq!(output.matches("name=\"author\"").count(), 2);
        assert_eq!(
            output.matches("name=\"google-site-verification\"").count(),
            2
        );
        assert!(output.contains("content=\"two&quot;&amp;\""));
    }
    #[test]
    fn patches_inherit_clear_replace_lists_and_bypass_title_templates() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                title: "Root".into(),
                title_template: "%s | Site".into(),
                description: "Inherited".into(),
                canonical: "/root".into(),
                twitter: Twitter::new().site("@root").into(),
                icons: vec![HeadIcon::new("/root.svg")].into(),
                manifest: "/app.json".into(),
                open_graph: OpenGraph::new()
                    .images([HeadMedia::new("/root.png")])
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        fields.begin_hook();
        apply(
            &mut fields,
            HeadMetadata {
                title: "Child".into(),
                twitter: Twitter::new().creator("@child").into(),
                ..Default::default()
            },
        )
        .unwrap();
        let output = html(&fields);
        assert!(output.contains("<title>Child | Site</title>"));
        assert!(output.contains("name=\"twitter:title\" content=\"Child | Site\""));
        assert!(output.contains("name=\"twitter:site\" content=\"@root\""));
        assert!(output.contains("name=\"twitter:creator\" content=\"@child\""));
        assert!(
            output.contains("name=\"twitter:image\" content=\"https://example.com/shop/root.png\"")
        );
        apply(
            &mut fields,
            HeadMetadata {
                title: HeadTitle::absolute("Checkout").into(),
                description: HeadValue::Clear,
                canonical: HeadValue::Clear,
                icons: vec![].into(),
                manifest: HeadValue::Clear,
                twitter: Twitter::new()
                    .clear_title()
                    .clear_description()
                    .clear_image()
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        let output = html(&fields);
        assert!(output.contains("<title>Checkout</title>"));
        assert!(!output.contains("twitter:title"));
        assert!(!output.contains("twitter:description"));
        assert!(!output.contains("twitter:image"));
        assert!(!output.contains("rel=\"canonical\""));
        assert!(!output.contains("rel=\"manifest\""));
        assert!(!output.contains("rel=\"icon\""));
        assert!(!output.contains("name=\"description\""));
        apply(
            &mut fields,
            HeadMetadata {
                twitter: HeadValue::Clear,
                open_graph: HeadValue::Clear,
                title: HeadValue::Clear,
                title_template: HeadValue::Clear,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(html(&fields), "");
        fields.begin_hook();
        apply(
            &mut fields,
            HeadMetadata {
                title: "Next".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(html(&fields), "<title>Next</title>");
    }
    #[test]
    fn invalid_patches_leave_all_inherited_state_intact() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                title: "Original".into(),
                robots: Robots {
                    index: Some(false),
                    ..Default::default()
                }
                .into(),
                icons: vec![HeadIcon::new("/valid.svg")].into(),
                ..Default::default()
            },
        )
        .unwrap();
        let original = html(&fields);
        let bad = "javascript:alert(1)";
        for mut patch in [
            HeadMetadata {
                manifest: bad.into(),
                ..Default::default()
            },
            HeadMetadata {
                icons: vec![HeadIcon::new(bad)].into(),
                ..Default::default()
            },
            HeadMetadata {
                alternates: vec![HeadAlternate::new(bad)].into(),
                ..Default::default()
            },
            HeadMetadata {
                authors: vec![HeadAuthor::new("Name").url(bad)].into(),
                ..Default::default()
            },
            HeadMetadata {
                twitter: Twitter::new().image(HeadMedia::new(bad)).into(),
                ..Default::default()
            },
            HeadMetadata {
                twitter: Twitter::new()
                    .image(HeadMedia::new("/ok").dimensions(0, 1))
                    .into(),
                ..Default::default()
            },
            HeadMetadata {
                robots: Robots {
                    max_snippet: Some(-2),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            HeadMetadata {
                verification: vec![HeadVerification::new("bad\"name", "x")].into(),
                ..Default::default()
            },
            HeadMetadata {
                verification: vec![HeadVerification::new("robots", "index")].into(),
                ..Default::default()
            },
            HeadMetadata {
                icons: (0..129)
                    .map(|_| HeadIcon::new("/ok"))
                    .collect::<Vec<_>>()
                    .into(),
                ..Default::default()
            },
            HeadMetadata {
                application_name: "a".repeat(VALUE_LIMIT + 1).into(),
                ..Default::default()
            },
            HeadMetadata {
                open_graph: OpenGraph::new()
                    .article(ArticleMetadata {
                        authors: vec![bad.into()],
                        ..Default::default()
                    })
                    .into(),
                ..Default::default()
            },
        ] {
            patch.title = "Must not apply".into();
            patch.description = "Must not apply".into();
            assert!(apply(&mut fields, patch).is_err());
            assert_eq!(html(&fields), original);
        }
    }
    #[test]
    fn defaults_are_shared_without_cross_request_mutation() {
        let mut defaults = HeadFields::default();
        apply(
            &mut defaults,
            HeadMetadata {
                twitter: Twitter::new().site(String::from("@site")).into(),
                authors: vec![HeadAuthor::new(String::from("Root"))].into(),
                verification: vec![HeadVerification::google("root")].into(),
                ..Default::default()
            },
        )
        .unwrap();
        let expected = html(&defaults);
        let mut request = defaults.clone();
        apply(
            &mut request,
            HeadMetadata {
                twitter: Twitter::new().site("@request").into(),
                authors: HeadValue::Clear,
                verification: HeadValue::Clear,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(html(&defaults), expected);
        assert!(!html(&request).contains("Root"));
        assert!(!html(&request).contains("google-site-verification"));
    }
    #[test]
    fn named_and_typed_setters_obey_last_write_and_do_not_duplicate() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                title: "Document".into(),
                twitter: Twitter::new().title("Typed").into(),
                authors: vec![HeadAuthor::new("Typed")].into(),
                verification: vec![HeadVerification::google("typed")].into(),
                ..Default::default()
            },
        )
        .unwrap();
        fields.meta("TWITTER:TITLE", Some("Named".into())).unwrap();
        fields.meta("author", None).unwrap();
        fields
            .meta("google-site-verification", Some("named".into()))
            .unwrap();
        let output = html(&fields);
        assert_eq!(output.matches("name=\"twitter:title\"").count(), 1);
        assert!(output.contains("name=\"twitter:title\" content=\"Named\""));
        assert!(!output.contains("name=\"author\""));
        assert_eq!(
            output.matches("name=\"google-site-verification\"").count(),
            1
        );
        apply(
            &mut fields,
            HeadMetadata {
                twitter: Twitter::new().title("Typed again").into(),
                authors: vec![HeadAuthor::new("Typed again")].into(),
                verification: vec![HeadVerification::google("typed-again")].into(),
                ..Default::default()
            },
        )
        .unwrap();
        let output = html(&fields);
        assert_eq!(output.matches("name=\"twitter:title\"").count(), 1);
        assert!(output.contains("name=\"twitter:title\" content=\"Typed again\""));
        assert!(output.contains("name=\"author\" content=\"Typed again\""));
        assert_eq!(
            output.matches("name=\"google-site-verification\"").count(),
            1
        );
        fields
            .meta("twitter:card", Some("summary_large_image".into()))
            .unwrap();
        apply(
            &mut fields,
            HeadMetadata {
                twitter: HeadValue::Clear,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!html(&fields).contains("twitter:"));
    }
    #[test]
    fn extended_and_named_slots_share_a_bounded_budget() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                icons: (0..64)
                    .map(|_| HeadIcon::new("/icon"))
                    .collect::<Vec<_>>()
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..64 {
            fields
                .meta(&format!("custom-{i}"), Some("x".into()))
                .unwrap();
        }
        let original = html(&fields);
        assert!(apply(
            &mut fields,
            HeadMetadata {
                title: "Wrong".into(),
                description: "Too many".into(),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(html(&fields), original);
        assert!(fields.meta("one-more", Some("x".into())).is_err());
        apply(
            &mut fields,
            HeadMetadata {
                icons: HeadValue::Clear,
                description: "Now fits".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(html(&fields).contains("Now fits"));
    }
    #[test]
    fn article_properties_replace_and_are_suppressed_for_non_articles() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                open_graph: OpenGraph::new()
                    .article(ArticleMetadata {
                        published_time: Some("2026-10-30T12:00:00Z".into()),
                        authors: vec!["/authors/a".into(), "https://other.example/author".into()],
                        tags: vec!["Rust & Web".into()],
                        ..Default::default()
                    })
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        let output = html(&fields);
        assert_eq!(output.matches("property=\"article:author\"").count(), 2);
        assert!(output.contains("content=\"https://example.com/shop/authors/a\""));
        assert!(output.contains("content=\"Rust &amp; Web\""));
        apply(
            &mut fields,
            HeadMetadata {
                open_graph: OpenGraph::new().kind("website").into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!html(&fields).contains("article:"));
        apply(
            &mut fields,
            HeadMetadata {
                open_graph: OpenGraph::new()
                    .article(ArticleMetadata {
                        section: Some("News".into()),
                        ..Default::default()
                    })
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(html(&fields).contains("article:section"));
        assert!(!html(&fields).contains("article:author"));
        apply(
            &mut fields,
            HeadMetadata {
                open_graph: OpenGraph::new().clear_article().into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!html(&fields).contains("article:"));
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;
    use crate::Twitter;
    use crate::{ArticleMetadata, HeadMetadata};
    fn apply(fields: &mut HeadFields, mut patch: HeadMetadata) -> Result<(), WriteError> {
        let urls = HeadUrlPolicy::new("https://example.com", "/").unwrap();
        let prepared = fields.prepare_metadata(&mut patch, Some(&urls))?;
        fields.apply_metadata(prepared);
        Ok(())
    }
    fn html(fields: &HeadFields) -> String {
        let mut buffer = crate::RenderBuffer::new();
        fields.write(&mut buffer, None).unwrap();
        buffer.into_string()
    }
    #[test]
    fn named_image_override_cannot_keep_unrelated_typed_alt_text() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                twitter: Twitter::new()
                    .image(HeadMedia::new("/old.png").alt("Old image"))
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        fields
            .meta("twitter:image", Some("https://example.com/new.png".into()))
            .unwrap();
        let output = html(&fields);
        assert!(!output.contains("old.png"));
        assert!(!output.contains("Old image"));
        assert!(output.contains("new.png"));
        apply(
            &mut fields,
            HeadMetadata {
                twitter: Twitter::new()
                    .image(HeadMedia::new("/last.png").alt("Last image"))
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        let output = html(&fields);
        assert_eq!(output.matches("name=\"twitter:image\"").count(), 1);
        assert!(output.contains("Last image"));
        assert!(!output.contains("new.png"));
    }
    #[test]
    fn full_typed_list_can_be_replaced_by_a_named_override() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                authors: (0..128)
                    .map(|_| HeadAuthor::new("Inherited"))
                    .collect::<Vec<_>>()
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(html(&fields).matches("name=\"author\"").count(), 128);
        fields.meta("author", Some("Replacement".into())).unwrap();
        assert_eq!(
            html(&fields),
            "<meta name=\"author\" content=\"Replacement\">"
        );
        // Removing the override makes the typed list count again: validate
        // the final state before changing the title or list.
        for i in 0..127 {
            fields
                .meta(&format!("extra-{i}"), Some("value".into()))
                .unwrap();
        }
        let before = html(&fields);
        assert!(apply(
            &mut fields,
            HeadMetadata {
                title: "Wrong".into(),
                authors: vec![HeadAuthor::new("A"), HeadAuthor::new("B")].into(),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(html(&fields), before);
    }
    #[test]
    fn article_lists_and_media_share_the_graph_budget() {
        let mut fields = HeadFields::default();
        apply(
            &mut fields,
            HeadMetadata {
                open_graph: OpenGraph::new()
                    .article(ArticleMetadata {
                        tags: (0..128).map(|_| "tag".into()).collect(),
                        ..Default::default()
                    })
                    .into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(html(&fields).matches("article:tag").count(), 128);
        let before = html(&fields);
        assert!(apply(
            &mut fields,
            HeadMetadata {
                open_graph: OpenGraph::new()
                    .images([HeadMedia::new("/one-too-many.png")])
                    .into(),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(html(&fields), before);
    }
}
