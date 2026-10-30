// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Typed fields, atomic validation, trusted URLs and context-correct serialization.
use crate::mode::ContentMode;
use crate::shared::escape::{escape_attr_value, escape_text};
use crate::{CspNonce, WriteBuf, WriteError};
use std::{any::Any, borrow::Cow, sync::Arc};
use url::Url;
const VALUE_LIMIT: usize = 16 * 1024;
const SLOT_LIMIT: usize = 128;
pub const HEAD_LIMIT: usize = 64 * 1024;
fn invalid() -> WriteError {
    WriteError::render("invalid managed head value")
}
pub fn text(value: &str) -> Result<(), WriteError> {
    if value.len() > VALUE_LIMIT {
        Err(invalid())
    } else {
        Ok(())
    }
}
/// Trusted application configuration, never inferred from request headers.
#[derive(Clone, Debug)]
pub struct HeadUrlPolicy {
    base: Url,
    allowed_resources: Vec<url::Origin>,
}
impl HeadUrlPolicy {
    pub fn new(origin: &str, mount: &str) -> Result<Self, WriteError> {
        let mut base = Url::parse(origin).map_err(|_| invalid())?;
        validate_url(&base)?;
        if base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
            || !mount.starts_with('/')
            || mount.starts_with("//")
            || mount.contains(['\\', '?', '#', '%'])
            || mount.chars().any(char::is_control)
            || mount.split('/').any(|p| p == "." || p == "..")
        {
            return Err(invalid());
        }
        base.set_path(&format!("{}/", mount.trim_end_matches('/')));
        Ok(Self {
            allowed_resources: vec![base.origin()],
            base,
        })
    }
    pub fn allow_resource_origin(mut self, origin: &str) -> Result<Self, WriteError> {
        let url = Url::parse(origin).map_err(|_| invalid())?;
        validate_url(&url)?;
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(invalid());
        }
        self.allowed_resources.push(url.origin());
        Ok(self)
    }
    fn resolve(&self, value: &str, resource: bool) -> Result<String, WriteError> {
        text(value)?;
        if value.trim() != value
            || value.contains('\\')
            || value.starts_with("//")
            || value.chars().any(char::is_control)
            || proa_html_policy::has_dangerous_url_scheme(value)
        {
            return Err(invalid());
        }
        let absolute = Url::parse(value).is_ok();
        let url = if absolute {
            Url::parse(value).map_err(|_| invalid())?
        } else {
            self.base
                .join(value.trim_start_matches('/'))
                .map_err(|_| invalid())?
        };
        validate_url(&url)?;
        if !absolute
            && (url.origin() != self.base.origin() || !url.path().starts_with(self.base.path()))
        {
            return Err(invalid());
        }
        if resource && !self.allowed_resources.contains(&url.origin()) {
            return Err(invalid());
        }
        let value: String = url.into();
        text(&value)?;
        Ok(value)
    }
}
fn validate_url(url: &Url) -> Result<(), WriteError> {
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        Err(invalid())
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResourceFamily {
    Module,
    Stylesheet,
}
#[derive(Clone)]
struct Resource {
    family: ResourceFamily,
    key: Arc<dyn Any + Send + Sync>,
    href: Option<Arc<str>>,
}
#[derive(Default, Clone)]
pub struct HeadFields {
    pub(crate) title: Option<HeadText>,
    template: Option<HeadText>,
    ancestor_template: Option<HeadText>,
    title_template: Option<HeadText>,
    graph: OpenGraph,
    canonical: Option<Arc<str>>,
    meta: Vec<(Arc<str>, Option<HeadText>)>,
    resources: Vec<Resource>,
}
impl HeadFields {
    pub fn meta(&mut self, name: &str, value: Option<Cow<'static, str>>) -> Result<(), WriteError> {
        text(name)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b))
        {
            return Err(invalid());
        }
        if let Some(v) = &value {
            text(v)?;
        }
        if let Some(slot) = self
            .meta
            .iter_mut()
            .find(|s| s.0.eq_ignore_ascii_case(name))
        {
            slot.1 = value.map(HeadText::from);
        } else {
            if self.meta.len() + self.resources.len() >= SLOT_LIMIT {
                return Err(invalid());
            }
            self.meta
                .push((name.to_ascii_lowercase().into(), value.map(HeadText::from)));
        }
        Ok(())
    }
    pub fn canonical(&mut self, policy: &HeadUrlPolicy, value: &str) -> Result<(), WriteError> {
        let url = policy.resolve(value, false)?;
        self.canonical = Some(url.into());
        Ok(())
    }
    pub fn resource<K: Eq + Send + Sync + 'static>(
        &mut self,
        policy: &HeadUrlPolicy,
        family: ResourceFamily,
        key: K,
        value: Option<&str>,
    ) -> Result<(), WriteError> {
        // Validate before touching the old value or reserving a new ordering slot.
        let href = value
            .map(|v| policy.resolve(v, true).map(Arc::from))
            .transpose()?;
        if let Some(slot) = self
            .resources
            .iter_mut()
            .find(|s| s.family == family && s.key.downcast_ref::<K>() == Some(&key))
        {
            slot.href = href;
        } else {
            if self.meta.len() + self.resources.len() >= SLOT_LIMIT {
                return Err(invalid());
            }
            self.resources.push(Resource {
                family,
                key: Arc::new(key),
                href,
            });
        }
        Ok(())
    }
    pub fn write<B: WriteBuf>(
        &self,
        out: &mut B,
        nonce: Option<&CspNonce>,
    ) -> Result<(), WriteError> {
        if let Some(title) = &self.title {
            out.extend_static(b"<title>")?;
            self.write_title(out, title, false)?;
            out.extend_static(b"</title>")?;
        }
        for (name, value) in &self.meta {
            if let Some(value) = value {
                out.extend_static(b"<meta name=\"")?;
                escape_attr_value(out, name)?;
                out.extend_static(b"\" content=\"")?;
                escape_attr_value(out, value)?;
                out.extend_static(b"\">")?;
            }
        }
        if let Some(url) = &self.canonical {
            out.extend_static(b"<link rel=\"canonical\" href=\"")?;
            escape_attr_value(out, url)?;
            out.extend_static(b"\">")?;
        }
        self.write_graph(out)?;
        for entry in &self.resources {
            if let Some(href) = &entry.href {
                match entry.family {
                    ResourceFamily::Module => {
                        out.extend_static(b"<script type=\"module\" src=\"")?
                    }
                    ResourceFamily::Stylesheet => {
                        out.extend_static(b"<link rel=\"stylesheet\" href=\"")?
                    }
                }
                escape_attr_value(out, href)?;
                out.extend_static(b"\"")?;
                if let Some(nonce) = nonce {
                    nonce.write_html_attr(out)?;
                }
                match entry.family {
                    ResourceFamily::Module => out.extend_static(b"></script>")?,
                    ResourceFamily::Stylesheet => out.extend_static(b">")?,
                }
            }
        }
        Ok(())
    }
}

/// Static literals stay borrowed; owned values and shared defaults clone by Arc.
#[derive(Clone, Debug)]
pub(crate) enum HeadText {
    Static(&'static str),
    Shared(Arc<str>),
}
impl From<Cow<'static, str>> for HeadText {
    fn from(value: Cow<'static, str>) -> Self {
        match value {
            Cow::Borrowed(s) => Self::Static(s),
            Cow::<str>::Owned(s) => Self::Shared(s.into()),
        }
    }
}
impl std::ops::Deref for HeadText {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Self::Static(s) => s,
            Self::Shared(s) => s,
        }
    }
}

/// A structured media replacement. Related properties always travel together.
#[derive(Clone, Debug)]
pub struct HeadMedia {
    url: HeadText,
    secure_url: Option<HeadText>,
    mime: Option<HeadText>,
    alt: Option<HeadText>,
    width: Option<u32>,
    height: Option<u32>,
}
impl HeadMedia {
    pub fn new(url: impl Into<Cow<'static, str>>) -> Self {
        Self {
            url: url.into().into(),
            secure_url: None,
            mime: None,
            alt: None,
            width: None,
            height: None,
        }
    }
    pub fn secure_url(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        self.secure_url = Some(value.into().into());
        self
    }
    pub fn mime_type(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        self.mime = Some(value.into().into());
        self
    }
    pub fn alt(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        self.alt = Some(value.into().into());
        self
    }
    pub fn dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }
    fn resolve(&mut self, policy: &HeadUrlPolicy) -> Result<(), WriteError> {
        self.url = Cow::<str>::Owned(policy.resolve(&self.url, false)?).into();
        if let Some(url) = &mut self.secure_url {
            let resolved = policy.resolve(url, false)?;
            if !resolved.starts_with("https://") {
                return Err(invalid());
            }
            *url = Cow::<str>::Owned(resolved).into();
        }
        for field in [&self.mime, &self.alt].into_iter().flatten() {
            text(field)?;
        }
        if self.width == Some(0) || self.height == Some(0) {
            return Err(invalid());
        }
        Ok(())
    }
}

/// An atomic Open Graph field patch. Unset fields inherit; explicit clears do not
/// fall back. Lists replace whole inherited lists, including an empty list.
#[derive(Clone, Debug, Default)]
pub struct OpenGraph {
    enabled: Option<bool>,
    scalars: [Option<Option<HeadText>>; 7],
    locales: Option<Vec<HeadText>>,
    images: Option<Vec<HeadMedia>>,
    audio: Option<Vec<HeadMedia>>,
    video: Option<Vec<HeadMedia>>,
}
const GRAPH_NAMES: [&str; 7] = [
    "og:title",
    "og:description",
    "og:url",
    "og:site_name",
    "og:determiner",
    "og:type",
    "og:locale",
];
macro_rules! graph_scalar {
    ($set:ident, $clear:ident, $n:literal) => {
        pub fn $set(mut self, value: impl Into<Cow<'static, str>>) -> Self {
            self.scalars[$n] = Some(Some(value.into().into()));
            self
        }
        pub fn $clear(mut self) -> Self {
            self.scalars[$n] = Some(None);
            self
        }
    };
}
impl OpenGraph {
    pub fn new() -> Self {
        Self {
            enabled: Some(true),
            ..Self::default()
        }
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }
    graph_scalar!(title, clear_title, 0);
    graph_scalar!(description, clear_description, 1);
    graph_scalar!(url, clear_url, 2);
    graph_scalar!(site_name, clear_site_name, 3);
    graph_scalar!(determiner, clear_determiner, 4);
    graph_scalar!(kind, clear_kind, 5);
    graph_scalar!(locale, clear_locale, 6);
    pub fn images(mut self, values: impl IntoIterator<Item = HeadMedia>) -> Self {
        self.images = Some(values.into_iter().take(SLOT_LIMIT + 1).collect());
        self
    }
    pub fn audio(mut self, values: impl IntoIterator<Item = HeadMedia>) -> Self {
        self.audio = Some(values.into_iter().take(SLOT_LIMIT + 1).collect());
        self
    }
    pub fn video(mut self, values: impl IntoIterator<Item = HeadMedia>) -> Self {
        self.video = Some(values.into_iter().take(SLOT_LIMIT + 1).collect());
        self
    }
    pub fn alternate_locales<T: Into<Cow<'static, str>>>(
        mut self,
        values: impl IntoIterator<Item = T>,
    ) -> Self {
        self.locales = Some(
            values
                .into_iter()
                .take(SLOT_LIMIT + 1)
                .map(|v| v.into().into())
                .collect(),
        );
        self
    }
}
impl HeadFields {
    pub(crate) fn begin_hook(&mut self) {
        self.ancestor_template = self.template.clone();
    }
    pub(crate) fn set_title(
        &mut self,
        value: Option<Cow<'static, str>>,
        absolute: bool,
    ) -> Result<(), WriteError> {
        if let Some(value) = &value {
            text(value)?;
        }
        self.title = value.map(HeadText::from);
        self.title_template = if absolute {
            None
        } else {
            self.ancestor_template.clone()
        };
        Ok(())
    }
    pub(crate) fn set_template(
        &mut self,
        value: Option<Cow<'static, str>>,
    ) -> Result<(), WriteError> {
        if let Some(value) = &value {
            text(value)?;
            if value.matches("%s").count() != 1 {
                return Err(invalid());
            }
        }
        self.template = value.map(HeadText::from);
        Ok(())
    }
    pub(crate) fn clear_canonical(&mut self) {
        self.canonical = None;
    }
    pub(crate) fn graph(
        &mut self,
        mut patch: OpenGraph,
        policy: Option<&HeadUrlPolicy>,
    ) -> Result<(), WriteError> {
        for value in patch.scalars.iter().flatten().flatten() {
            text(value)?;
        }
        if let Some(Some(url)) = &mut patch.scalars[2] {
            *url = Cow::<str>::Owned(policy.ok_or_else(invalid)?.resolve(url, false)?).into();
        }
        let mut count = patch.locales.as_ref().map_or(0, Vec::len);
        for list in [&mut patch.images, &mut patch.audio, &mut patch.video]
            .into_iter()
            .flatten()
        {
            count += list.len();
            if count > SLOT_LIMIT {
                return Err(invalid());
            }
            for media in list {
                media.resolve(policy.ok_or_else(invalid)?)?;
            }
        }
        if count > SLOT_LIMIT {
            return Err(invalid());
        }
        if let Some(locales) = &patch.locales {
            for value in locales {
                text(value)?;
            }
        }
        let retained = patch
            .locales
            .as_ref()
            .or(self.graph.locales.as_ref())
            .map_or(0, Vec::len)
            + patch
                .images
                .as_ref()
                .or(self.graph.images.as_ref())
                .map_or(0, Vec::len)
            + patch
                .audio
                .as_ref()
                .or(self.graph.audio.as_ref())
                .map_or(0, Vec::len)
            + patch
                .video
                .as_ref()
                .or(self.graph.video.as_ref())
                .map_or(0, Vec::len);
        if retained > SLOT_LIMIT {
            return Err(invalid());
        }
        // Apply only after validation of every supplied field/record.
        if let Some(value) = patch.enabled {
            self.graph.enabled = Some(value);
        }
        for (old, new) in self.graph.scalars.iter_mut().zip(patch.scalars) {
            if new.is_some() {
                *old = new;
            }
        }
        if patch.locales.is_some() {
            self.graph.locales = patch.locales;
        }
        if patch.images.is_some() {
            self.graph.images = patch.images;
        }
        if patch.audio.is_some() {
            self.graph.audio = patch.audio;
        }
        if patch.video.is_some() {
            self.graph.video = patch.video;
        }
        Ok(())
    }
    pub(crate) fn resolved_title(&self) -> Option<String> {
        self.title.as_ref().map(|title| match &self.title_template {
            Some(template) => template.replacen("%s", title, 1),
            None => title.to_string(),
        })
    }
    fn write_title<B: WriteBuf>(
        &self,
        out: &mut B,
        value: &str,
        attribute: bool,
    ) -> Result<(), WriteError> {
        let mut escape = |value: &str| {
            if attribute {
                escape_attr_value(out, value)
            } else {
                escape_text(out, value, ContentMode::Html)
            }
        };
        if let Some(template) = &self.title_template {
            let (before, after) = template.split_once("%s").expect("validated template");
            escape(before)?;
            escape(value)?;
            escape(after)
        } else {
            escape(value)
        }
    }
    fn write_graph<B: WriteBuf>(&self, out: &mut B) -> Result<(), WriteError> {
        if self.graph.enabled != Some(true) {
            return Ok(());
        }
        for (i, (name, value)) in GRAPH_NAMES.iter().zip(&self.graph.scalars).enumerate() {
            let fallback = match i {
                0 => self.title.as_deref(),
                1 => self
                    .meta
                    .iter()
                    .find(|(n, _)| n.as_ref() == "description")
                    .and_then(|(_, v)| v.as_deref()),
                2 => self.canonical.as_deref(),
                _ => None,
            };
            let selected = match value {
                Some(value) => value.as_deref(),
                None => fallback,
            };
            if let Some(selected) = selected {
                if i == 0 && value.is_none() {
                    out.extend_static(b"<meta property=\"og:title\" content=\"")?;
                    self.write_title(out, selected, true)?;
                    out.extend_static(b"\">")?;
                } else {
                    property(out, name, selected)?;
                }
            }
        }
        if let Some(locales) = &self.graph.locales {
            for value in locales {
                property(out, "og:locale:alternate", value)?;
            }
        }
        for (family, list) in [
            ("og:image", &self.graph.images),
            ("og:audio", &self.graph.audio),
            ("og:video", &self.graph.video),
        ] {
            if let Some(list) = list {
                for media in list {
                    property(out, family, &media.url)?;
                    for (suffix, value) in [
                        (":secure_url", &media.secure_url),
                        (":type", &media.mime),
                        (":alt", &media.alt),
                    ] {
                        if let Some(value) = value {
                            property_parts(out, family, suffix, value)?;
                        }
                    }
                    for (suffix, value) in [(":width", media.width), (":height", media.height)] {
                        if let Some(value) = value {
                            let mut number = itoa_lut::Buffer::new();
                            property_parts(out, family, suffix, number.format(value))?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
fn property<B: WriteBuf>(out: &mut B, name: &str, value: &str) -> Result<(), WriteError> {
    property_parts(out, name, "", value)
}
fn property_parts<B: WriteBuf>(
    out: &mut B,
    name: &str,
    suffix: &str,
    value: &str,
) -> Result<(), WriteError> {
    out.extend_static(b"<meta property=\"")?;
    escape_attr_value(out, name)?;
    escape_attr_value(out, suffix)?;
    out.extend_static(b"\" content=\"")?;
    escape_attr_value(out, value)?;
    out.extend_static(b"\">")
}
