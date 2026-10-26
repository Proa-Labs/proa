// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Sitemap and robots.txt documents that honor the public origin and runtime
//! base path.
//!
//! This module generates documents; crawler matching and fetch behavior belong
//! to clients. References: [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html),
//! [Google's interpretation](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec),
//! and the [Sitemaps protocol](https://www.sitemaps.org/protocol.html).

use std::time::Duration;

use jiff::civil::Date;
use proa_core::{
    BasePath, DataLoader, MdRenderSync, OutputMeter, TrustedMarkdown, WebContext, WebRenderSync,
    WriteBuf, WriteError,
};
use proa_macros::{md_sync, text, xml_sync};
use thiserror::Error;

// Conservative UTF-8 byte bound for the protocol's character limit:
// https://www.sitemaps.org/protocol.html#locdef
const MAX_LOC_BYTES: usize = 2048;
// The schemas impose this lower bound in addition to the protocol's upper bound:
// https://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd
// https://www.sitemaps.org/schemas/sitemap/0.9/siteindex.xsd
const MIN_LOC_CHARACTERS: usize = 12;
/// Maximum number of URLs in a sitemap, or sitemaps in an index.
/// See [Sitemaps: index limits](https://www.sitemaps.org/protocol.html#index).
pub const MAX_SITEMAP_ENTRIES: usize = 50_000;
/// Maximum uncompressed UTF-8 document size permitted by the sitemap protocol.
/// See [Sitemaps: index limits](https://www.sitemaps.org/protocol.html#index).
pub const MAX_SITEMAP_BYTES: usize = 50 * 1024 * 1024;
/// Google's robots.txt processing limit, not an RFC maximum document size.
/// See [Google: file format](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec#file-format)
/// and [RFC 9309 §2.5](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.5).
pub const MAX_ROBOTS_BYTES: usize = 500 * 1024;
const PRIORITY_TEXT: [&str; 11] = [
    "0.0", "0.1", "0.2", "0.3", "0.4", "0.5", "0.6", "0.7", "0.8", "0.9", "1.0",
];

/// Failure while building a sitemap or robots.txt document.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiscoveryError {
    #[error(
        "origin must be an http or https host (optional port) with no path, query, or fragment"
    )]
    InvalidOrigin,
    #[error("a public origin is required when sitemap routes are enabled")]
    MissingPublicOrigin,
    #[error(
        "sitemap ID must contain only ASCII letters, digits, `-`, or `_`, and cannot be empty"
    )]
    InvalidSitemapId,
    #[error("sitemap IDs must be unique")]
    DuplicateSitemapId,
    #[error("a sitemap index must contain at least one sitemap")]
    EmptySitemapIndex,
    #[error("a sitemap must contain at least one page URL")]
    EmptySitemap,
    #[error("a sitemap or index cannot contain more than 50,000 entries")]
    TooManySitemapEntries,
    #[error("a sitemap or index cannot exceed 50 MiB of uncompressed XML")]
    SitemapTooLarge,
    #[error("path must be a root-relative application path or an http(s) URL without fragments or invalid URL characters")]
    InvalidPath,
    #[error("lastmod must be a YYYY-MM-DD calendar date")]
    InvalidLastmod,
    #[error("absolute sitemap URL must use the sitemap origin")]
    CrossOrigin,
    #[error("sitemap URL must be inside the base path directory that serves the document")]
    OutsideBasePath,
    #[error("sitemap loc must be shorter than 2048 bytes")]
    LocTooLong,
    #[error("sitemap loc must contain at least 12 characters, as required by the XML schemas")]
    LocTooShort,
    #[error("robots field cannot contain whitespace, `#`, or control characters")]
    InvalidRobotsField,
    #[error("robots user agent must be `*` or contain only ASCII letters, `_`, and `-`")]
    InvalidRobotsUserAgent,
    #[error("robots.txt exceeds Google's 500 KiB processing limit")]
    RobotsTooLarge,
    #[error("robots host must be a hostname with optional port, or an HTTP(S) origin")]
    InvalidRobotsHost,
    #[error(
        "custom robots directive must have a non-reserved ASCII field name and a value without comments or control characters"
    )]
    InvalidRobotsDirective,
    #[error("robots rule path must be empty or start with `/` or `*`")]
    InvalidRobotsPath,
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// How often a sitemap URL is expected to change.
/// Values are crawler hints: [Sitemaps: changefreq](https://www.sitemaps.org/protocol.html#changefreqdef).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeFreq {
    Always,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Never,
}

impl ChangeFreq {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Yearly => "yearly",
            Self::Never => "never",
        }
    }
}

/// Sitemap priority in tenths from 0.0 through 1.0.
/// Tenths are Proa's subset of the [priority range](https://www.sitemaps.org/protocol.html#prioritydef).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SitemapPriority(u8);

impl SitemapPriority {
    pub const ZERO: Self = Self(0);
    pub const HALF: Self = Self(5);
    pub const ONE: Self = Self(10);

    pub const fn tenths(tenths: u8) -> Option<Self> {
        if tenths <= 10 {
            Some(Self(tenths))
        } else {
            None
        }
    }

    pub const fn as_str(self) -> &'static str {
        PRIORITY_TEXT[self.0 as usize]
    }
}

/// One URL in a sitemap.
/// See [Sitemaps: loc](https://www.sitemaps.org/protocol.html#locdef).
/// Supply URL-encoded paths; XML escaping is applied separately when rendering.
/// Fragments and raw characters excluded from URI/IRI syntax are rejected.
/// Local paths must not traverse above the application root. Absolute URLs
/// retain their spelling; their parsed path determines directory scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapUrl {
    path: SitemapLocation,
    lastmod: Option<Date>,
    changefreq: Option<ChangeFreq>,
    priority: Option<SitemapPriority>,
}

impl SitemapUrl {
    pub fn new(path: impl Into<String>) -> Result<Self, DiscoveryError> {
        let path = SitemapLocation::new(path.into())?;
        Ok(Self {
            path,
            lastmod: None,
            changefreq: None,
            priority: None,
        })
    }

    /// Page modification date, using the protocol's permitted date-only form.
    /// See [Sitemaps: lastmod](https://www.sitemaps.org/protocol.html#lastmoddef).
    /// This API supports `YYYY-MM-DD`, not the specification's optional timestamps.
    /// Jiff validates and stores the date without retaining an owned string.
    pub fn lastmod(self, lastmod: impl AsRef<str>) -> Result<Self, DiscoveryError> {
        self.lastmod_date(parse_lastmod(lastmod.as_ref())?)
    }

    /// Page modification date supplied as a Jiff calendar date.
    /// Proa supports years `0001` through `9999`; the
    /// [sitemap schema](https://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd)
    /// uses XML Schema 1.0 dates, which exclude year zero.
    ///
    /// ```
    /// use proa_framework_core::discovery::SitemapUrl;
    ///
    /// let page = SitemapUrl::new("/pricing")?
    ///     .lastmod_date(jiff::civil::date(2026, 10, 25))?;
    /// # Ok::<(), proa_framework_core::discovery::DiscoveryError>(())
    /// ```
    pub fn lastmod_date(mut self, lastmod: Date) -> Result<Self, DiscoveryError> {
        self.lastmod = Some(validate_lastmod_date(lastmod)?);
        Ok(self)
    }

    pub fn changefreq(mut self, changefreq: ChangeFreq) -> Self {
        self.changefreq = Some(changefreq);
        self
    }

    pub fn priority(mut self, priority: SitemapPriority) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn path(&self) -> &str {
        &self.path.spelling
    }
}

/// A sitemap.xml document.
/// Element order follows the [official schema](https://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd).
#[derive(Debug, Clone)]
pub struct Sitemap {
    origin: String,
    canonical_origin: url::Origin,
    base_path: BasePath,
    urls: Vec<SitemapUrl>,
}

impl Sitemap {
    pub fn new(origin: &str) -> Result<Self, DiscoveryError> {
        let (origin, canonical_origin) = parse_origin(origin)?;
        Ok(Self {
            origin: origin.to_owned(),
            canonical_origin,
            base_path: BasePath::default(),
            urls: Vec::new(),
        })
    }

    pub fn with_base_path(mut self, base_path: BasePath) -> Self {
        self.base_path = base_path;
        self
    }

    pub fn with_url(self, path: impl Into<String>) -> Result<Self, DiscoveryError> {
        self.with_entry(SitemapUrl::new(path)?)
    }

    /// Add an entry. Absolute entries must use the sitemap origin, because the
    /// default [sitemap scope](https://www.sitemaps.org/protocol.html#location)
    /// is same-origin. Ownership-verified cross-submission is outside this API.
    /// Rendering checks the normalized URL path against the final base path.
    /// Custom handlers must set the base path to the document's directory.
    pub fn with_entry(mut self, url: SitemapUrl) -> Result<Self, DiscoveryError> {
        if self.urls.len() >= MAX_SITEMAP_ENTRIES {
            return Err(DiscoveryError::TooManySitemapEntries);
        }
        if url
            .path
            .absolute
            .as_ref()
            .is_some_and(|url| url.origin() != self.canonical_origin)
        {
            return Err(DiscoveryError::CrossOrigin);
        }
        self.urls.push(url);
        Ok(self)
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn urls(&self) -> &[SitemapUrl] {
        &self.urls
    }

    /// Render the sitemap protocol XML.
    ///
    /// Allocates an owned XML body outside HTML page rendering. The framework
    /// builder calls this once at startup; custom handlers control their own refresh.
    /// Validated URL components are escaped directly into the output without
    /// allocating temporary URL strings or reparsing URLs for each entry.
    pub fn render_xml(&self) -> Result<String, DiscoveryError> {
        let mut cx = WebContext::new();
        let meter = OutputMeter::new(MAX_SITEMAP_BYTES as u64);
        cx.with_metered_output(meter.borrow(), |cx| self.write_xml(cx))
            .map_err(document_write_error)?;
        checked_document(cx)
    }

    fn write_xml<L: DataLoader, B: WriteBuf>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), DiscoveryError> {
        if self.urls.is_empty() {
            return Err(DiscoveryError::EmptySitemap);
        }
        validate_mount(&self.base_path)?;
        // Preserve discovery errors before entering the WriteError-only renderer.
        for url in &self.urls {
            url.path.validate(&self.origin, &self.base_path)?;
        }
        xml_sync! {
            <?xml version="1.0" encoding="UTF-8"?>
            "\n"
            <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                {for url in &self.urls {
                    xml_sync! {
                        <url>
                            <loc>{url.path.xml(&self.origin, &self.base_path)}</loc>
                            {if let Some(lastmod) = url.lastmod {
                                xml_sync! { <lastmod>{text!("{:}", lastmod)}</lastmod> }
                            }}
                            {if let Some(changefreq) = url.changefreq {
                                xml_sync! { <changefreq>{changefreq.as_str()}</changefreq> }
                            }}
                            {if let Some(priority) = url.priority {
                                xml_sync! { <priority>{priority.as_str()}</priority> }
                            }}
                        </url>
                    }
                }}
            </urlset>
            "\n"
        }
        .render(cx)?;
        Ok(())
    }
}

/// One named document served at `/sitemap-{id}.xml` by `with_sitemaps`.
///
/// IDs are literal path segments, not route patterns. Origin and mount prefix
/// are supplied by the framework builder after application generation finishes.
#[derive(Debug, Clone)]
pub struct SitemapFile {
    id: String,
    urls: Vec<SitemapUrl>,
}

impl SitemapFile {
    pub fn new(
        id: impl Into<String>,
        urls: impl IntoIterator<Item = SitemapUrl>,
    ) -> Result<Self, DiscoveryError> {
        let id = id.into();
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(DiscoveryError::InvalidSitemapId);
        }
        let mut entries = Vec::new();
        for url in urls {
            if entries.len() >= MAX_SITEMAP_ENTRIES {
                return Err(DiscoveryError::TooManySitemapEntries);
            }
            entries.push(url);
        }
        if entries.is_empty() {
            return Err(DiscoveryError::EmptySitemap);
        }
        Ok(Self { id, urls: entries })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn urls(&self) -> &[SitemapUrl] {
        &self.urls
    }

    pub fn into_urls(self) -> Vec<SitemapUrl> {
        self.urls
    }
}

/// A reference to a sitemap document, rather than a page URL.
/// See [Sitemaps: index loc](https://www.sitemaps.org/protocol.html#sitemapIndex_loc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapIndexEntry {
    path: SitemapLocation,
    lastmod: Option<Date>,
}

impl SitemapIndexEntry {
    pub fn new(path: impl Into<String>) -> Result<Self, DiscoveryError> {
        let path = SitemapLocation::new(path.into())?;
        Ok(Self {
            path,
            lastmod: None,
        })
    }

    /// When this sitemap document last changed, not the index generation date.
    /// See [Sitemaps: index lastmod](https://www.sitemaps.org/protocol.html#sitemapIndex_lastmod).
    /// Accepts only `YYYY-MM-DD`; Jiff validates and stores the calendar date.
    pub fn lastmod(self, lastmod: impl AsRef<str>) -> Result<Self, DiscoveryError> {
        self.lastmod_date(parse_lastmod(lastmod.as_ref())?)
    }

    /// When this sitemap document last changed, supplied as a Jiff calendar date.
    /// Uses the same year range as [`SitemapUrl::lastmod_date`].
    /// See [Sitemaps: index lastmod](https://www.sitemaps.org/protocol.html#sitemapIndex_lastmod).
    pub fn lastmod_date(mut self, lastmod: Date) -> Result<Self, DiscoveryError> {
        self.lastmod = Some(validate_lastmod_date(lastmod)?);
        Ok(self)
    }
}

/// An XML sitemap index linking to multiple same-origin sitemap documents.
///
/// See <https://www.sitemaps.org/protocol.html#index>.
#[derive(Debug, Clone)]
pub struct SitemapIndex {
    origin: String,
    canonical_origin: url::Origin,
    base_path: BasePath,
    entries: Vec<SitemapIndexEntry>,
}

impl SitemapIndex {
    pub fn new(origin: &str) -> Result<Self, DiscoveryError> {
        let (origin, canonical_origin) = parse_origin(origin)?;
        Ok(Self {
            origin: origin.to_owned(),
            canonical_origin,
            base_path: BasePath::default(),
            entries: Vec::new(),
        })
    }

    pub fn with_base_path(mut self, base_path: BasePath) -> Self {
        self.base_path = base_path;
        self
    }

    /// Add a same-origin document. Rendering checks its normalized path against
    /// the final base path, following [Google's index-directory requirements](https://developers.google.com/search/docs/crawling-indexing/sitemaps/large-sitemaps).
    pub fn with_entry(mut self, entry: SitemapIndexEntry) -> Result<Self, DiscoveryError> {
        if self.entries.len() >= MAX_SITEMAP_ENTRIES {
            return Err(DiscoveryError::TooManySitemapEntries);
        }
        if entry
            .path
            .absolute
            .as_ref()
            .is_some_and(|url| url.origin() != self.canonical_origin)
        {
            return Err(DiscoveryError::CrossOrigin);
        }
        self.entries.push(entry);
        Ok(self)
    }

    /// Render and validate the index once, outside the HTML render hot path.
    /// Validated URL components are written directly without temporary strings.
    pub fn render_xml(&self) -> Result<String, DiscoveryError> {
        let mut cx = WebContext::new();
        let meter = OutputMeter::new(MAX_SITEMAP_BYTES as u64);
        cx.with_metered_output(meter.borrow(), |cx| self.write_xml(cx))
            .map_err(document_write_error)?;
        checked_document(cx)
    }

    fn write_xml<L: DataLoader, B: WriteBuf>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), DiscoveryError> {
        if self.entries.is_empty() {
            return Err(DiscoveryError::EmptySitemapIndex);
        }
        validate_mount(&self.base_path)?;
        // Preserve discovery errors before entering the WriteError-only renderer.
        for entry in &self.entries {
            entry.path.validate(&self.origin, &self.base_path)?;
        }
        xml_sync! {
            <?xml version="1.0" encoding="UTF-8"?>
            "\n"
            <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                {for entry in &self.entries {
                    xml_sync! {
                        <sitemap>
                            <loc>{entry.path.xml(&self.origin, &self.base_path)}</loc>
                            {if let Some(lastmod) = entry.lastmod {
                                xml_sync! { <lastmod>{text!("{:}", lastmod)}</lastmod> }
                            }}
                        </sitemap>
                    }
                }}
            </sitemapindex>
            "\n"
        }
        .render(cx)?;
        Ok(())
    }
}

/// Preserve caller spelling while retaining the parsed absolute URL for origin
/// and directory checks. Parsing happens at construction, never during rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SitemapLocation {
    spelling: String,
    absolute: Option<url::Url>,
}

impl SitemapLocation {
    fn new(spelling: String) -> Result<Self, DiscoveryError> {
        let absolute = if spelling.starts_with('/') {
            validate_local_path(&spelling)?;
            None
        } else {
            Some(parse_http_url(&spelling)?)
        };
        Ok(Self { spelling, absolute })
    }

    fn validate(&self, origin: &str, base_path: &BasePath) -> Result<(), DiscoveryError> {
        if self
            .absolute
            .as_ref()
            .is_some_and(|url| !is_within_base_path(url.path(), base_path.as_str()))
        {
            return Err(DiscoveryError::OutsideBasePath);
        }
        loc_parts(origin, base_path, &self.spelling)?;
        Ok(())
    }

    /// Borrow components after `validate` has checked this origin and base path.
    /// Rendering does not repeat scope/length checks or allocate a URL string.
    fn xml<'a>(&'a self, origin: &'a str, base_path: &'a BasePath) -> SitemapLoc<'a> {
        let (origin, mount) = if self.absolute.is_some() {
            ("", "")
        } else {
            (origin, base_path.as_str())
        };
        SitemapLoc([origin, mount, &self.spelling])
    }
}

/// Escape quotes too, as required by [Sitemaps: entity escaping](https://www.sitemaps.org/protocol.html#escaping),
/// even though ordinary XML text permits them. Borrow all parts to avoid temporary strings.
struct SitemapLoc<'a>([&'a str; 3]);

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SitemapLoc<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for part in self.0 {
            proa_core::shared::escape::escape_attr_value_xml(cx.output_mut(), part)?;
        }
        Ok(())
    }
}

fn document_write_error(error: DiscoveryError) -> DiscoveryError {
    match error {
        DiscoveryError::Write(WriteError::OutputLimitExceeded { .. }) => {
            DiscoveryError::SitemapTooLarge
        }
        other => other,
    }
}

fn checked_document(cx: WebContext) -> Result<String, DiscoveryError> {
    let output = cx.into_output();
    check_document_size(output.len())?;
    Ok(output.into_string())
}

fn check_document_size(bytes: usize) -> Result<(), DiscoveryError> {
    if bytes > MAX_SITEMAP_BYTES {
        return Err(DiscoveryError::SitemapTooLarge);
    }
    Ok(())
}

/// One `User-agent` group in robots.txt.
/// See [RFC 9309 §2.1](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsGroup {
    user_agents: Vec<String>,
    rules: Vec<RobotsRule>,
    crawl_delay: Option<Duration>,
    directives: Vec<(String, String)>,
}

impl RobotsGroup {
    /// The first user agent in this group. See `user_agents` for shared groups.
    pub fn user_agent(&self) -> &str {
        &self.user_agents[0]
    }

    pub fn user_agents(&self) -> &[String] {
        &self.user_agents
    }

    pub fn rules(&self) -> &[RobotsRule] {
        &self.rules
    }

    pub fn crawl_delay(&self) -> Option<Duration> {
        self.crawl_delay
    }

    /// Custom directive name/value pairs, in insertion order.
    pub fn directives(&self) -> &[(String, String)] {
        &self.directives
    }
}

/// An `Allow` or `Disallow` rule.
/// Crawlers resolve precedence: [RFC 9309 §2.2.2](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RobotsRule {
    Allow(String),
    Disallow(String),
}

/// A robots.txt document.
///
/// Crawlers fetch `/robots.txt` at the origin root. Incoming Proa routes stay
/// unprefixed, so the handler path is still `/robots.txt`; a prefix-stripping
/// proxy must forward that origin-root URL to the handler. Root-relative rules
/// are application paths that render under `with_base_path`, and
/// `sitemap_path` joins sitemap locs with the public mount prefix.
/// See [RFC 9309 §2.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.3).
/// Rules are not authorization: [RFC 9309 §3](https://www.rfc-editor.org/rfc/rfc9309.html#section-3).
#[derive(Debug, Clone, Default)]
pub struct RobotsTxt {
    base_path: BasePath,
    groups: Vec<RobotsGroup>,
    sitemaps: Vec<String>,
    host: Option<String>,
}

impl RobotsTxt {
    pub fn new() -> Self {
        Self::default()
    }

    /// Render root-relative rules under this mount prefix.
    ///
    /// With `/apps/demo`, `disallow("/admin")` renders
    /// `Disallow: /apps/demo/admin`. Application-wide rules (`/`, `/*`, etc.)
    /// also cover the bare mount and its query variants. The exact-root `/$`
    /// covers the bare and slash root without queries. Literal `*` and `$` in
    /// the mount prefix are escaped, while rule operators remain unchanged.
    /// Empty and `*`-leading rules render unchanged. The prefix replaces any
    /// earlier one; it is never applied twice.
    /// Mount composition is Proa policy; literal operator escaping follows
    /// [RFC 9309 §2.2.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.3).
    pub fn with_base_path(mut self, base_path: BasePath) -> Self {
        self.base_path = base_path;
        self
    }

    pub fn base_path(&self) -> &BasePath {
        &self.base_path
    }

    /// Start an independent group; repeated matching tokens are combined by crawlers.
    /// See [RFC 9309 §2.2.1](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.1).
    pub fn group(self, user_agent: &str) -> Result<Self, DiscoveryError> {
        self.group_agents([user_agent])
    }

    /// Start a group whose rules apply to all supplied user agents.
    /// Tokens follow [RFC 9309 §2.2.1](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.1):
    /// `*`, or ASCII letters, `_`, and `-`. At least one token is required.
    pub fn group_agents<I, S>(mut self, user_agents: I) -> Result<Self, DiscoveryError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut agents = Vec::new();
        for agent in user_agents {
            let agent = agent.into();
            validate_robots_user_agent(&agent)?;
            agents.push(agent);
        }
        if agents.is_empty() {
            return Err(DiscoveryError::InvalidRobotsUserAgent);
        }
        self.groups.push(RobotsGroup {
            user_agents: agents,
            rules: Vec::new(),
            crawl_delay: None,
            directives: Vec::new(),
        });
        Ok(self)
    }

    /// Set a crawler-specific delay for the current group, in seconds.
    /// `Duration` supports fractional seconds without negative or nonfinite values.
    /// This extension is not supported by every crawler;
    /// [Google ignores it](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec#syntax).
    /// Creates a wildcard group when called before `group` or `group_agents`.
    pub fn crawl_delay(mut self, delay: Duration) -> Self {
        self.current_group_mut().crawl_delay = Some(delay);
        self
    }

    /// Add a nonstandard per-group directive, such as `Request-Rate`.
    ///
    /// Names retain their casing and repeated calls emit repeated lines.
    /// Values may contain spaces, but not `#` or control characters. Values
    /// render verbatim: the base path is applied only to allow/disallow rules.
    /// Reserved fields must use their typed methods instead.
    /// Extension handling: [RFC 9309 §2.2.4](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.4).
    pub fn directive(mut self, name: &str, value: &str) -> Result<Self, DiscoveryError> {
        validate_robots_directive(name, value)?;
        self.current_group_mut()
            .directives
            .push((name.to_owned(), value.to_owned()));
        Ok(self)
    }

    /// Set the optional, nonstandard `Host` directive.
    /// Accepts a hostname with an optional port, or an HTTP(S) origin. It is
    /// explicit document configuration, not inferred from request headers.
    /// Google does not support it: [Google: syntax](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec#syntax).
    pub fn host(mut self, host: &str) -> Result<Self, DiscoveryError> {
        let host = if host.contains("://") && !host.starts_with(char::is_whitespace) {
            parse_origin(host)
                .map_err(|_| DiscoveryError::InvalidRobotsHost)?
                .0
        } else {
            // Host is configuration-time data; use the same origin parser.
            // The borrowed spelling below must not trim or accept path suffixes.
            let candidate = format!("https://{host}");
            let (origin, _) =
                parse_origin(&candidate).map_err(|_| DiscoveryError::InvalidRobotsHost)?;
            if origin != candidate || host.is_empty() {
                return Err(DiscoveryError::InvalidRobotsHost);
            }
            host
        };
        self.host = Some(host.to_owned());
        Ok(self)
    }

    /// Add an `Allow` rule. The path must be empty, start with `*`, or be a
    /// root-relative application path (not protocol-relative `//...`).
    /// See [RFC 9309 §2.2.2](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.2)
    /// and its [wildcard-leading example (§5.1)](https://www.rfc-editor.org/rfc/rfc9309.html#section-5.1).
    pub fn allow(self, path: &str) -> Result<Self, DiscoveryError> {
        self.push_rule(RobotsRule::Allow(checked_robots_path(path)?))
    }

    /// Add a `Disallow` rule. The path must be empty, start with `*`, or be a
    /// root-relative application path (not protocol-relative `//...`).
    /// See [`Self::allow`] for the applicable RFC sections.
    pub fn disallow(self, path: &str) -> Result<Self, DiscoveryError> {
        self.push_rule(RobotsRule::Disallow(checked_robots_path(path)?))
    }

    /// Advertise an absolute sitemap URL, optionally on another host.
    /// This document-level extension is independent of user-agent groups:
    /// [Google: sitemap](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec#sitemap).
    pub fn sitemap(mut self, loc: &str) -> Result<Self, DiscoveryError> {
        parse_http_url(loc)?;
        if loc.len() >= MAX_LOC_BYTES {
            return Err(DiscoveryError::LocTooLong);
        }
        self.sitemaps.push(loc.to_string());
        Ok(self)
    }

    pub fn sitemap_path(
        mut self,
        origin: &str,
        base_path: &BasePath,
        path: &str,
    ) -> Result<Self, DiscoveryError> {
        self.sitemaps
            .push(absolute_http_url(origin, base_path, path)?);
        Ok(self)
    }

    /// Groups with rules as supplied, before the base path is applied.
    pub fn groups(&self) -> &[RobotsGroup] {
        &self.groups
    }

    pub fn sitemaps(&self) -> &[String] {
        &self.sitemaps
    }

    /// Render UTF-8 text with LF separators ([RFC 9309 §2.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.3)).
    /// This low-level serializer has no size cap; use [`Self::try_render_txt`]
    /// for Google's limit, as the builder and Axum response helper do.
    pub fn render_txt(&self) -> String {
        let mut cx = WebContext::new();
        // Fields have already passed robots validation. TrustedMarkdown preserves
        // protocol punctuation (including `*`, `$`, and URL query separators).
        // An empty Disallow preserves empty group boundaries; blank lines do not:
        // https://www.rfc-editor.org/rfc/rfc9309.html#section-2.1
        md_sync! { r###"
            {for (index, group) in self.groups.iter().enumerate() {
            {if index > 0 {

            }}{for agent in &group.user_agents {
            User-agent: {TrustedMarkdown(agent)}
            }}{if group.rules.is_empty() {
            Disallow:
            }}{for (field, base, path) in self.rule_lines(group) {
            {TrustedMarkdown(field)}{RobotsMount(base)}{TrustedMarkdown(path)}
            }}{if let Some(delay) = group.crawl_delay {
            Crawl-delay: {RobotsCrawlDelay(delay)}
            }}{for (name, value) in &group.directives {
            {TrustedMarkdown(name)}: {TrustedMarkdown(value)}
            }}}}{if !self.groups.is_empty() && (self.host.is_some() || !self.sitemaps.is_empty()) {

            }}{if let Some(host) = &self.host {
            Host: {TrustedMarkdown(host)}
            }}{for loc in &self.sitemaps {
            Sitemap: {TrustedMarkdown(loc)}
            }}"###
        }
        .render_md(&mut cx)
        .expect("validated robots.txt renders into an owned buffer");
        cx.into_output().into_string()
    }

    /// Borrow the final rules, including the bare mount and its query variants.
    fn rule_lines<'a>(
        &'a self,
        group: &'a RobotsGroup,
    ) -> impl Iterator<Item = (&'static str, &'a str, &'a str)> {
        group.rules.iter().flat_map(move |rule| {
            let (field, path) = match rule {
                RobotsRule::Allow(path) => ("Allow: ", path.as_str()),
                RobotsRule::Disallow(path) => ("Disallow: ", path.as_str()),
            };
            let base = if path.starts_with('/') {
                self.base_path.as_str()
            } else {
                ""
            };
            let app_wide = is_application_wide_rule(path);
            [
                (!base.is_empty() && (app_wide || path == "/$")).then_some("$"),
                (!base.is_empty() && app_wide).then_some("?"),
                Some(path),
            ]
            .into_iter()
            .flatten()
            .map(move |path| (field, base, path))
        })
    }

    /// Render and reject documents whose trailing rules Google would ignore.
    /// See [Google: file format](https://developers.google.com/crawling/docs/robots-txt/robots-txt-spec#file-format).
    /// The check includes final mount expansion and all extension records.
    pub fn try_render_txt(&self) -> Result<String, DiscoveryError> {
        let text = self.render_txt();
        if text.len() > MAX_ROBOTS_BYTES {
            return Err(DiscoveryError::RobotsTooLarge);
        }
        Ok(text)
    }

    fn push_rule(mut self, rule: RobotsRule) -> Result<Self, DiscoveryError> {
        self.current_group_mut().rules.push(rule);
        Ok(self)
    }

    fn current_group_mut(&mut self) -> &mut RobotsGroup {
        if self.groups.is_empty() {
            self.groups.push(RobotsGroup {
                user_agents: vec!["*".to_string()],
                rules: Vec::new(),
                crawl_delay: None,
                directives: Vec::new(),
            });
        }
        self.groups.last_mut().expect("group inserted above")
    }
}

/// A borrowed mount with literal robots operators escaped, without a temporary string.
struct RobotsMount<'a>(&'a str);

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for RobotsMount<'_> {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for part in self.0.split_inclusive(['*', '$']) {
            let (prefix, escaped) = if let Some(prefix) = part.strip_suffix('*') {
                (prefix, "%2A")
            } else if let Some(prefix) = part.strip_suffix('$') {
                (prefix, "%24")
            } else {
                (part, "")
            };
            TrustedMarkdown(prefix).render_md(cx)?;
            TrustedMarkdown(escaped).render_md(cx)?;
        }
        Ok(())
    }
}

/// Stream exact decimal seconds through Proa's numeric and formatting adapters.
struct RobotsCrawlDelay(Duration);

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for RobotsCrawlDelay {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        self.0.as_secs().render_md(cx)?;
        let mut fraction = self.0.subsec_nanos();
        if fraction != 0 {
            let mut width = 9;
            while fraction.is_multiple_of(10) {
                fraction /= 10;
                width -= 1;
            }
            proa_core::md::FmtBuf::new(cx.output_mut())
                .write_fmt(format_args!(".{fraction:0width$}"))?;
        }
        Ok(())
    }
}

fn is_application_wide_rule(path: &str) -> bool {
    let Some(tail) = path.strip_prefix('/') else {
        return false;
    };
    if tail.is_empty() {
        return true;
    }
    let stars = tail.strip_suffix('$').unwrap_or(tail);
    !stars.is_empty() && stars.bytes().all(|byte| byte == b'*')
}

/// Join a public origin, runtime base path, and application path into an
/// absolute `http` or `https` URL.
/// The result must fit the sitemap location bounds; see [`SitemapUrl`].
/// Absolute input passes through, including other origins. Sitemap documents
/// separately enforce origin and directory scope; robots sitemap links may be external.
pub fn absolute_http_url(
    origin: &str,
    base_path: &BasePath,
    path: &str,
) -> Result<String, DiscoveryError> {
    let origin = if path.starts_with('/') {
        let (origin, _) = parse_origin(origin)?;
        validate_mount(base_path)?;
        validate_local_path(path)?;
        origin
    } else {
        parse_http_url(path)?;
        ""
    };

    let (origin, mount, path) = loc_parts(origin, base_path, path)?;
    let mut composed = String::with_capacity(origin.len() + mount.len() + path.len());
    composed.push_str(origin);
    composed.push_str(mount);
    composed.push_str(path);
    Ok(composed)
}

/// Borrow the parts of a location without constructing or reparsing a URL.
/// The caller must already have validated the origin and entry path, as the
/// document constructors do. Only the composed length depends on the final mount.
/// The lower bound comes from the [schema](https://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd).
fn loc_parts<'a>(
    origin: &'a str,
    base_path: &'a BasePath,
    path: &'a str,
) -> Result<(&'a str, &'a str, &'a str), DiscoveryError> {
    let (origin, mount) = if path.starts_with('/') {
        (origin, base_path.as_str())
    } else {
        ("", "")
    };
    if origin.len() + mount.len() + path.len() >= MAX_LOC_BYTES {
        return Err(DiscoveryError::LocTooLong);
    }
    if [origin, mount, path]
        .into_iter()
        .flat_map(str::chars)
        .take(MIN_LOC_CHARACTERS)
        .count()
        < MIN_LOC_CHARACTERS
    {
        return Err(DiscoveryError::LocTooShort);
    }
    Ok((origin, mount, path))
}

/// Origin-only configuration is Proa policy; scheme/host/port matching serves
/// [Sitemaps: location](https://www.sitemaps.org/protocol.html#location).
fn parse_origin(origin: &str) -> Result<(&str, url::Origin), DiscoveryError> {
    let origin = origin.trim();
    let origin = origin.strip_suffix('/').unwrap_or(origin);
    let parsed = parse_http_url(origin).map_err(|_| DiscoveryError::InvalidOrigin)?;
    // Inspect the supplied path before URL normalization can erase `/a/..`.
    let (_, authority) = origin
        .split_once("://")
        .ok_or(DiscoveryError::InvalidOrigin)?;
    if authority.contains(['/', '?']) {
        return Err(DiscoveryError::InvalidOrigin);
    }
    Ok((origin, parsed.origin()))
}

/// Delegate URL grammar, IDNA, ports, and normalization to `url`.
/// Keep the spelling checks shared with local paths: `Url::parse` repairs some
/// malformed inputs, but discovery documents retain the supplied spelling.
fn parse_http_url(value: &str) -> Result<url::Url, DiscoveryError> {
    if contains_invalid_url_char(value) || has_invalid_percent_escape(value) {
        return Err(DiscoveryError::InvalidPath);
    }
    let (_, rest) = value.split_once("://").ok_or(DiscoveryError::InvalidPath)?;
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    if authority.is_empty()
        || authority.contains('@')
        || authority.ends_with(':')
        || rest[authority.len()..].contains(['[', ']'])
    {
        return Err(DiscoveryError::InvalidPath);
    }
    let parsed = url::Url::parse(value).map_err(|_| DiscoveryError::InvalidPath)?;
    if !matches!(parsed.scheme(), "http" | "https") || !parsed.has_host() {
        return Err(DiscoveryError::InvalidPath);
    }
    Ok(parsed)
}

fn has_invalid_percent_escape(value: &str) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%'
            && (!bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
                || !bytes.next().is_some_and(|b| b.is_ascii_hexdigit()))
        {
            return true;
        }
    }
    false
}

/// A root-relative application path (`/...`, not protocol-relative `//...`).
/// Proa's application-local paths must not traverse above the mount. Dot segments
/// are interpreted as in [URL parsing](https://url.spec.whatwg.org/#path-state),
/// including percent-encoded dots; query text is not a path.
fn validate_local_path(path: &str) -> Result<(), DiscoveryError> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || contains_invalid_url_char(path)
        || path.contains(['[', ']'])
        || has_invalid_percent_escape(path)
    {
        return Err(DiscoveryError::InvalidPath);
    }
    let mut depth = 0usize;
    for segment in path.split('?').next().unwrap_or(path).split('/').skip(1) {
        if segment == "." || segment.eq_ignore_ascii_case("%2e") {
            continue;
        }
        if ["..", ".%2e", "%2e.", "%2e%2e"]
            .iter()
            .any(|dot| segment.eq_ignore_ascii_case(dot))
        {
            depth = depth.checked_sub(1).ok_or(DiscoveryError::InvalidPath)?;
        } else {
            depth += 1;
        }
    }
    Ok(())
}

fn validate_mount(base_path: &BasePath) -> Result<(), DiscoveryError> {
    if !base_path.as_str().is_empty() {
        validate_local_path(base_path.as_str())?;
    }
    Ok(())
}

/// Check the normalized absolute path against the document's directory, per
/// [Sitemaps: location](https://www.sitemaps.org/protocol.html#location).
/// The URL parser has already removed literal and encoded dot segments. Compare
/// whole segments so sibling names and encoded slashes cannot cross the boundary.
fn is_within_base_path(path: &str, base: &str) -> bool {
    if base.is_empty() {
        return true;
    }
    let mut segments = path[1..].split('/');
    for segment in base[1..].split('/') {
        let Some(candidate) = segments.next() else {
            return false;
        };
        if !normalized_segment_bytes(candidate).eq(normalized_segment_bytes(segment)) {
            return false;
        }
    }
    // The bare mount is outside its directory; a trailing slash is inside.
    segments.next().is_some()
}

/// Percent-triplet case and unreserved escapes are equivalent under
/// [RFC 3986 §6.2.2](https://www.rfc-editor.org/rfc/rfc3986.html#section-6.2.2).
/// UTF-8 bytes also match their percent-encoded IRI form under
/// [RFC 3987 §3.1](https://www.rfc-editor.org/rfc/rfc3987.html#section-3.1).
/// Keep escaped ASCII reserved bytes distinct from their literal forms. Both
/// inputs have already passed percent-escape validation.
fn normalized_segment_bytes(segment: &str) -> impl Iterator<Item = (u8, bool)> + '_ {
    let mut bytes = segment.bytes();
    std::iter::from_fn(move || {
        let byte = bytes.next()?;
        if byte != b'%' {
            return Some((byte, false));
        }
        let high = char::from(bytes.next()?).to_digit(16)? as u8;
        let low = char::from(bytes.next()?).to_digit(16)? as u8;
        let byte = high * 16 + low;
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        Some((byte, byte.is_ascii() && !unreserved))
    })
}

/// Reject raw characters excluded from URI/IRI syntax, controls, and XML
/// non-Char values: [Sitemaps: escaping](https://www.sitemaps.org/protocol.html#escaping)
/// and [RFC 3986 §2](https://www.rfc-editor.org/rfc/rfc3986.html#section-2).
/// Fragment-free discovery URLs are Proa policy; `#` also starts a robots comment
/// ([RFC 9309 §2.2.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.3)).
/// Other non-ASCII text remains accepted and is written as supplied.
fn contains_invalid_url_char(value: &str) -> bool {
    value.chars().any(|ch| {
        ch <= ' '
            || ch.is_control()
            || matches!(
                ch,
                '"' | '<' | '>' | '\\' | '^' | '`' | '{' | '|' | '}' | '#'
            )
            || !is_xml10_char(ch)
    })
}

fn is_xml10_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

/// Keep Proa's date-only input grammar: Jiff also accepts datetimes and discards
/// their time when parsing a `Date`. Calendar validation belongs to Jiff.
/// See [Sitemaps: lastmod](https://www.sitemaps.org/protocol.html#lastmoddef).
fn parse_lastmod(value: &str) -> Result<Date, DiscoveryError> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(DiscoveryError::InvalidLastmod);
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err(DiscoveryError::InvalidLastmod);
    }
    value.parse().map_err(|_| DiscoveryError::InvalidLastmod)
}

/// Proa's four-digit, positive-year subset of `xsd:date` in the
/// [sitemap schema](https://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd).
fn validate_lastmod_date(date: Date) -> Result<Date, DiscoveryError> {
    if !(1..=9999).contains(&date.year()) {
        return Err(DiscoveryError::InvalidLastmod);
    }
    Ok(date)
}

/// Proa rejects controls/whitespace instead of repairing fields. `#` is syntax:
/// [RFC 9309 §2.2.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2.3).
fn has_invalid_robots_byte(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte <= b' ' || byte == b'#' || byte == 0x7F)
}

fn validate_robots_user_agent(value: &str) -> Result<(), DiscoveryError> {
    if value != "*"
        && (value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'-')))
    {
        return Err(DiscoveryError::InvalidRobotsUserAgent);
    }
    Ok(())
}

fn validate_robots_directive(name: &str, value: &str) -> Result<(), DiscoveryError> {
    const RESERVED: [&str; 6] = [
        "user-agent",
        "allow",
        "disallow",
        "sitemap",
        "host",
        "crawl-delay",
    ];
    if !name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || RESERVED
            .iter()
            .any(|field| name.eq_ignore_ascii_case(field))
        || value.chars().any(|ch| ch == '#' || ch.is_control())
    {
        return Err(DiscoveryError::InvalidRobotsDirective);
    }
    Ok(())
}

/// [RFC 9309 §2.2](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2)
/// specifies `/`-leading paths, but [§5.1](https://www.rfc-editor.org/rfc/rfc9309.html#section-5.1)
/// explicitly allows `*` to match that slash. Prefer `/`-leading patterns for
/// portability. Rejecting `//...` is Proa's application-path policy.
fn checked_robots_path(value: &str) -> Result<String, DiscoveryError> {
    if has_invalid_robots_byte(value) {
        return Err(DiscoveryError::InvalidRobotsField);
    }
    if !value.is_empty() && !value.starts_with(['/', '*']) {
        return Err(DiscoveryError::InvalidRobotsPath);
    }
    if value.starts_with("//") {
        return Err(DiscoveryError::InvalidPath);
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composed_sitemap_locations_obey_the_schema_minimum_length() {
        for path in ["/", "http://a.io", "http://é.xy"] {
            assert_eq!(
                absolute_http_url("http://a.io", &BasePath::default(), path),
                if path == "/" {
                    Ok("http://a.io/".to_owned())
                } else {
                    Err(DiscoveryError::LocTooShort)
                }
            );
        }
        let sitemap = Sitemap::new("http://a").unwrap().with_url("/").unwrap();
        assert_eq!(sitemap.render_xml(), Err(DiscoveryError::LocTooShort));
        let index = SitemapIndex::new("http://a")
            .unwrap()
            .with_entry(SitemapIndexEntry::new("/").unwrap())
            .unwrap();
        assert_eq!(index.render_xml(), Err(DiscoveryError::LocTooShort));
        assert!(sitemap
            .with_base_path(BasePath::new("/app").unwrap())
            .render_xml()
            .is_ok());
        assert!(index
            .with_base_path(BasePath::new("/app").unwrap())
            .render_xml()
            .is_ok());
    }

    #[test]
    fn robots_user_agents_follow_rfc_product_token_grammar() {
        for agent in ["*", "Googlebot", "Bot_Name-test", "A"] {
            assert_eq!(
                RobotsTxt::new().group(agent).unwrap().groups()[0].user_agent(),
                agent
            );
        }
        for agent in ["", "Bot1", "Googlebot/2.1", "Googlebot*", "é", "a:b"] {
            assert_eq!(
                RobotsTxt::new().group(agent).unwrap_err(),
                DiscoveryError::InvalidRobotsUserAgent,
                "{agent}"
            );
            assert_eq!(
                RobotsTxt::new()
                    .group_agents(["ValidBot", agent])
                    .unwrap_err(),
                DiscoveryError::InvalidRobotsUserAgent
            );
        }
    }

    #[test]
    fn checked_robots_size_counts_utf8_bytes_and_final_mount() {
        let overhead = RobotsTxt::new().disallow("/").unwrap().render_txt().len();
        let payload_bytes = MAX_ROBOTS_BYTES - overhead;
        let mut path = format!("/{}", "é".repeat(payload_bytes / 2));
        if !payload_bytes.is_multiple_of(2) {
            path.push('x');
        }
        let robots = RobotsTxt::new().disallow(&path).unwrap();
        assert_eq!(robots.try_render_txt().unwrap().len(), MAX_ROBOTS_BYTES);
        assert_eq!(
            robots
                .with_base_path(BasePath::new("/app").unwrap())
                .try_render_txt(),
            Err(DiscoveryError::RobotsTooLarge)
        );
        path.push('x');
        let oversized = RobotsTxt::new().disallow(&path).unwrap();
        assert_eq!(
            oversized.try_render_txt(),
            Err(DiscoveryError::RobotsTooLarge)
        );
        // The low-level serializer may still target crawlers with larger limits.
        assert_eq!(oversized.render_txt().len(), MAX_ROBOTS_BYTES + 1);
    }

    #[test]
    fn document_meter_includes_escaping_and_closing_tags_at_the_limit() {
        let sitemap = Sitemap::new("https://example.com")
            .unwrap()
            .with_entry(
                SitemapUrl::new("/search?a=1&b=2")
                    .unwrap()
                    .lastmod("2026-10-25")
                    .unwrap(),
            )
            .unwrap();
        let index = SitemapIndex::new("https://example.com")
            .unwrap()
            .with_entry(SitemapIndexEntry::new("/sitemap-pages.xml?a=1&b=2").unwrap())
            .unwrap();
        for is_index in [false, true] {
            let expected = if is_index {
                index.render_xml()
            } else {
                sitemap.render_xml()
            }
            .unwrap();
            for limit in [expected.len(), expected.len() - 1] {
                let mut cx: WebContext = WebContext::new();
                let meter = OutputMeter::new(limit as u64);
                let result = cx
                    .with_metered_output(meter.borrow(), |cx| {
                        if is_index {
                            index.write_xml(cx)
                        } else {
                            sitemap.write_xml(cx)
                        }
                    })
                    .map_err(document_write_error);
                if limit == expected.len() {
                    result.unwrap();
                    assert_eq!(cx.into_output().into_string(), expected);
                } else {
                    assert_eq!(result.unwrap_err(), DiscoveryError::SitemapTooLarge);
                    assert!(cx.output().len() <= limit);
                }
            }
        }
    }

    #[test]
    fn urls_reject_malformed_hosts_and_percent_escapes() {
        for origin in [
            "https://bad^host.example",
            "https://bad%zz.example",
            "https://bad<host.example",
            "https:///example.com",
        ] {
            assert_eq!(
                Sitemap::new(origin).unwrap_err(),
                DiscoveryError::InvalidOrigin,
                "{origin}"
            );
            assert_eq!(
                SitemapUrl::new(format!("{origin}/page")).unwrap_err(),
                DiscoveryError::InvalidPath
            );
        }
        for path in [
            "/bad%",
            "/bad%2",
            "/bad%xx",
            "https://example.com/page?x=%qq",
        ] {
            assert_eq!(
                SitemapUrl::new(path).unwrap_err(),
                DiscoveryError::InvalidPath,
                "{path}"
            );
        }
        assert_eq!(
            RobotsTxt::new().host("bad^host.example").unwrap_err(),
            DiscoveryError::InvalidRobotsHost
        );
        assert!(SitemapUrl::new("/valid%25?x=%2f").is_ok());
    }

    #[test]
    fn sitemap_origin_comparison_uses_canonical_url_hosts() {
        for (origin, page) in [
            (
                "https://bücher.example",
                "https://xn--bcher-kva.example/page",
            ),
            (
                "https://xn--bcher-kva.example",
                "https://bücher.example/page",
            ),
            ("https://example.com", "https://%65xample.com/page"),
            ("https://127.0.0.1", "https://127.1/page"),
        ] {
            let xml = Sitemap::new(origin)
                .unwrap()
                .with_url(page)
                .unwrap()
                .render_xml()
                .unwrap();
            assert!(xml.contains(&format!("<loc>{page}</loc>")));
        }
    }

    #[test]
    fn schema_requires_a_page_and_disallows_year_zero() {
        assert_eq!(
            Sitemap::new("https://example.com")
                .unwrap()
                .render_xml()
                .unwrap_err(),
            DiscoveryError::EmptySitemap
        );
        assert_eq!(
            SitemapFile::new("empty", []).unwrap_err(),
            DiscoveryError::EmptySitemap
        );
        assert_eq!(
            SitemapUrl::new("/")
                .unwrap()
                .lastmod("0000-01-01")
                .unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
        assert_eq!(
            SitemapIndexEntry::new("/sitemap-0.xml")
                .unwrap()
                .lastmod("0000-01-01")
                .unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
        assert!(SitemapUrl::new("/").unwrap().lastmod("0001-01-01").is_ok());
    }

    #[test]
    fn robots_mount_prefix_escapes_literal_pattern_operators() {
        for (mount, escaped) in [
            ("/app*", "/app%2A"),
            ("/app$", "/app%24"),
            ("/app%2A", "/app%2A"),
        ] {
            let body = RobotsTxt::new()
                .allow("/public/*")
                .unwrap()
                .disallow("/private$")
                .unwrap()
                .with_base_path(BasePath::new(mount).unwrap())
                .render_txt();
            assert_eq!(
                body,
                format!("User-agent: *\nAllow: {escaped}/public/*\nDisallow: {escaped}/private$\n")
            );
        }
    }

    #[test]
    fn robots_application_wide_rules_cover_bare_mount_queries() {
        for pattern in ["/", "/*", "/*$", "/**", "/**$"] {
            let body = RobotsTxt::new()
                .disallow(pattern)
                .unwrap()
                .with_base_path(BasePath::new("/app").unwrap())
                .render_txt();
            assert_eq!(
                body,
                format!(
                    "User-agent: *\nDisallow: /app$\nDisallow: /app?\nDisallow: /app{pattern}\n"
                )
            );
        }
        let exact = RobotsTxt::new()
            .disallow("/$")
            .unwrap()
            .with_base_path(BasePath::new("/app").unwrap())
            .render_txt();
        assert_eq!(exact, "User-agent: *\nDisallow: /app$\nDisallow: /app/$\n");
    }

    #[test]
    fn sitemap_index_resolves_document_urls_and_escapes_metadata() {
        let xml = SitemapIndex::new("https://example.com")
            .unwrap()
            .with_base_path(BasePath::new("/app").unwrap())
            .with_entry(
                SitemapIndexEntry::new("/sitemap/products.xml?x='1'&y=%22%3C2%3E%22")
                    .unwrap()
                    .lastmod("2024-02-29")
                    .unwrap(),
            )
            .unwrap()
            .with_entry(
                SitemapIndexEntry::new("https://example.com:443/app/sitemap/pages.xml").unwrap(),
            )
            .unwrap()
            .render_xml()
            .unwrap();
        assert_eq!(
            xml,
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">",
                "<sitemap><loc>https://example.com/app/sitemap/products.xml?x=&#39;1&#39;&amp;y=%22%3C2%3E%22</loc>",
                "<lastmod>2024-02-29</lastmod></sitemap>",
                "<sitemap><loc>https://example.com:443/app/sitemap/pages.xml</loc></sitemap>",
                "</sitemapindex>\n"
            )
        );
    }

    #[test]
    fn sitemap_index_validates_origin_dates_and_composed_locations() {
        assert_eq!(
            SitemapIndex::new("https://example.com")
                .unwrap()
                .render_xml()
                .unwrap_err(),
            DiscoveryError::EmptySitemapIndex
        );
        assert_eq!(
            SitemapIndexEntry::new("/sitemap/0.xml")
                .unwrap()
                .lastmod("2026-02-29")
                .unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
        assert_eq!(
            SitemapIndexEntry::new("javascript:alert(1)").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            SitemapIndex::new("https://example.com")
                .unwrap()
                .with_entry(SitemapIndexEntry::new("https://other.example/sitemap.xml").unwrap())
                .unwrap_err(),
            DiscoveryError::CrossOrigin
        );
        assert_eq!(
            SitemapIndex::new("https://example.com")
                .unwrap()
                .with_base_path(BasePath::new(&format!("/{}", "x".repeat(2030))).unwrap())
                .with_entry(SitemapIndexEntry::new("/sitemap/0.xml").unwrap())
                .unwrap()
                .render_xml()
                .unwrap_err(),
            DiscoveryError::LocTooLong
        );
    }

    #[test]
    fn sitemap_ids_cannot_inject_routes_or_url_components() {
        for id in [
            "", "../pages", "a/b", ":id", "{id}", "*rest", "a?x=1", "a#b", "a%2fb", "a b", "é",
        ] {
            assert_eq!(
                SitemapFile::new(id, []).unwrap_err(),
                DiscoveryError::InvalidSitemapId,
                "{id}"
            );
        }
        let file =
            SitemapFile::new("products_0-ABC", [SitemapUrl::new("/products/1").unwrap()]).unwrap();
        assert_eq!(file.id(), "products_0-ABC");
        assert_eq!(file.urls()[0].path(), "/products/1");
    }

    #[test]
    fn sitemap_and_index_enforce_entry_limits_at_the_boundary() {
        let url = SitemapUrl::new("/page").unwrap();
        let file =
            SitemapFile::new("0", std::iter::repeat_n(url.clone(), MAX_SITEMAP_ENTRIES)).unwrap();
        assert_eq!(file.urls().len(), MAX_SITEMAP_ENTRIES);
        assert_eq!(
            SitemapFile::new(
                "0",
                std::iter::repeat_n(url.clone(), MAX_SITEMAP_ENTRIES + 1)
            )
            .unwrap_err(),
            DiscoveryError::TooManySitemapEntries
        );
        let mut sitemap = Sitemap::new("https://example.com").unwrap();
        sitemap.urls = file.into_urls();
        assert!(sitemap.render_xml().is_ok());
        assert_eq!(
            sitemap.with_entry(url).unwrap_err(),
            DiscoveryError::TooManySitemapEntries
        );

        let entry = SitemapIndexEntry::new("/sitemap/0.xml").unwrap();
        let mut index = SitemapIndex::new("https://example.com").unwrap();
        index.entries = vec![entry.clone(); MAX_SITEMAP_ENTRIES];
        assert!(index.render_xml().is_ok());
        assert_eq!(
            index.with_entry(entry).unwrap_err(),
            DiscoveryError::TooManySitemapEntries
        );
    }

    #[test]
    fn sitemap_size_limit_counts_escaped_xml_bytes() {
        // Each raw URL fits the loc limit, but XML escaping takes the document
        // beyond 50 MiB even though its unescaped URL strings fit comfortably.
        let path = format!("/search?q={}", "&".repeat(210));
        let mut sitemap = Sitemap::new("https://example.com").unwrap();
        sitemap.urls = vec![SitemapUrl::new(path.clone()).unwrap(); MAX_SITEMAP_ENTRIES];
        assert_eq!(
            sitemap.render_xml().unwrap_err(),
            DiscoveryError::SitemapTooLarge
        );
        let mut cx = WebContext::new();
        let meter = OutputMeter::new(MAX_SITEMAP_BYTES as u64);
        assert_eq!(
            document_write_error(
                cx.with_metered_output(meter.borrow(), |cx| sitemap.write_xml(cx))
                    .unwrap_err()
            ),
            DiscoveryError::SitemapTooLarge
        );
        assert!(cx.output().len() <= MAX_SITEMAP_BYTES);
        assert_eq!(meter.consumed(), cx.output().len() as u64);
        let mut index = SitemapIndex::new("https://example.com").unwrap();
        index.entries = vec![SitemapIndexEntry::new(path).unwrap(); MAX_SITEMAP_ENTRIES];
        assert_eq!(
            index.render_xml().unwrap_err(),
            DiscoveryError::SitemapTooLarge
        );
    }

    #[test]
    fn sitemap_joins_origin_base_path_and_local_routes() {
        let sitemap = Sitemap::new("https://example.com/")
            .unwrap()
            .with_base_path(BasePath::new("/apps/demo").unwrap())
            .with_url("/")
            .unwrap()
            .with_entry(
                SitemapUrl::new("/about")
                    .unwrap()
                    .lastmod("2026-10-23")
                    .unwrap()
                    .changefreq(ChangeFreq::Weekly)
                    .priority(SitemapPriority::tenths(8).unwrap()),
            )
            .unwrap();

        let xml = sitemap.render_xml().unwrap();
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\""));
        assert!(xml.contains("<loc>https://example.com/apps/demo/</loc>"));
        assert!(xml.contains("<loc>https://example.com/apps/demo/about</loc>"));
        assert!(xml.contains("<lastmod>2026-10-23</lastmod>"));
        assert!(xml.contains("<changefreq>weekly</changefreq>"));
        assert!(xml.contains("<priority>0.8</priority>"));
        assert!(xml.contains("</urlset>"));
    }

    #[test]
    fn sitemap_escapes_xml_without_reencoding_urls() {
        let xml = Sitemap::new("https://example.com")
            .unwrap()
            .with_base_path(BasePath::new("/apps/a&b").unwrap())
            .with_url("/search?q='a'&b=%22%3Cx%3E%22")
            .unwrap()
            .render_xml()
            .unwrap();
        assert!(xml.contains(
            "<loc>https://example.com/apps/a&amp;b/search?q=&#39;a&#39;&amp;b=%22%3Cx%3E%22</loc>"
        ));
    }

    #[test]
    fn sitemap_and_index_check_normalized_scope_without_rewriting_entries() {
        for (base, path, allowed) in [
            ("", "", true),
            ("", "?q=1", true),
            ("/apps/demo", "/apps/demo/", true),
            ("/apps/demo", "/apps/demo/docs?q=/../../secret", true),
            ("/apps/demo", "/apps/demo/a/../docs", true),
            ("/apps/demo", "/apps/demo/a/%2e%2E/docs", true),
            ("/apps/demo", "/apps/%64emo/docs", true),
            ("/apps/%64emo", "/apps/demo/docs", true),
            ("/apps/%E2%9C%93", "/apps/%e2%9c%93/docs", true),
            ("/apps/%E2%9C%93", "/apps/✓/docs", true),
            ("/café", "/café/docs", true),
            ("/café", "/caf%C3%A9/docs", true),
            ("/apps/✓", "/apps/%e2%9c%93/docs", true),
            ("/🙂", "/%F0%9F%99%82/docs", true),
            ("/apps/a%3Ab", "/apps/a%3ab/docs", true),
            ("/apps/demo", "/other/page", false),
            ("/apps/demo", "/apps/demo", false),
            ("/apps/demo", "/apps/demo?q=1", false),
            ("/apps/demo", "/apps/demo2/page", false),
            ("/apps/demo", "/apps/demo/../../secret", false),
            ("/apps/demo", "/apps/demo/%2e%2E/%2E./secret", false),
            ("/apps/demo", "/apps%2Fdemo/page", false),
            ("/apps/demo", "/apps/demo%2Fpage", false),
            ("/apps/a:b", "/apps/a%3Ab/page", false),
            ("/café", "/cafe/docs", false),
            ("/café", "/caf%C3%A9x/docs", false),
        ] {
            let url = format!("https://EXAMPLE.com:443{path}");
            let base = BasePath::new(base).unwrap();
            let sitemap = Sitemap::new("https://example.com")
                .unwrap()
                .with_url(url.clone())
                .unwrap()
                .with_base_path(base.clone());
            let index = SitemapIndex::new("https://example.com")
                .unwrap()
                .with_base_path(base.clone())
                .with_entry(SitemapIndexEntry::new(url.clone()).unwrap())
                .unwrap();
            for result in [sitemap.render_xml(), index.render_xml()] {
                if allowed {
                    assert!(result.unwrap().contains(&format!("<loc>{url}</loc>")));
                } else {
                    assert_eq!(
                        result,
                        Err(DiscoveryError::OutsideBasePath),
                        "{base:?}: {url}"
                    );
                }
            }
        }

        // Local paths compose under the mount; safe internal dot segments stay valid.
        let xml = Sitemap::new("https://example.com")
            .unwrap()
            .with_base_path(BasePath::new("/apps/demo").unwrap())
            .with_url("/a/../docs")
            .unwrap()
            .with_url("/")
            .unwrap()
            .render_xml()
            .unwrap();
        assert!(xml.contains("<loc>https://example.com/apps/demo/a/../docs</loc>"));
        assert!(xml.contains("<loc>https://example.com/apps/demo/</loc>"));
    }

    #[test]
    fn sitemap_compares_scheme_host_and_effective_port() {
        for (origin, url) in [
            ("https://example.com", "https://example.com:443/docs"),
            ("https://example.com:443", "https://example.com/docs"),
            ("http://example.com", "http://example.com:80/docs"),
            ("http://example.com:80", "http://example.com/docs"),
            ("HTTPS://EXAMPLE.COM:00443", "https://example.com/docs?q=1"),
            ("http://example.com", "HTTP://EXAMPLE.COM:00080/docs"),
            ("https://example.com:8443", "https://EXAMPLE.COM:08443/docs"),
            ("https://[::1]", "https://[0:0:0:0:0:0:0:1]:443/docs"),
        ] {
            let xml = Sitemap::new(origin)
                .unwrap()
                .with_url(url)
                .unwrap_or_else(|error| panic!("{origin} / {url}: {error}"))
                .render_xml()
                .unwrap();
            assert!(xml.contains(&format!("<loc>{url}</loc>")), "{xml}");
        }
        for (origin, url) in [
            ("https://example.com", "https://example.com:80/docs"),
            ("http://example.com", "http://example.com:443/docs"),
            ("https://example.com:8443", "https://example.com/docs"),
            ("https://[::1]", "https://[::2]/docs"),
        ] {
            assert_eq!(
                Sitemap::new(origin).unwrap().with_url(url).unwrap_err(),
                DiscoveryError::CrossOrigin,
                "{origin} / {url}"
            );
        }
    }

    #[test]
    fn sitemap_rejects_cross_origin_absolute_entries() {
        let sitemap = || Sitemap::new("https://example.com").unwrap();
        for url in [
            "https://cdn.example.com/docs",
            "http://example.com/docs",
            "https://example.com:8443/docs",
            "https://example.com.evil.test/docs",
        ] {
            assert_eq!(
                sitemap().with_url(url).unwrap_err(),
                DiscoveryError::CrossOrigin,
                "{url}"
            );
        }
        assert_eq!(
            sitemap()
                .with_entry(SitemapUrl::new("https://cdn.example.com/").unwrap())
                .unwrap_err(),
            DiscoveryError::CrossOrigin
        );
    }

    #[test]
    fn origins_reject_userinfo_extra_ports_and_empty_ipv6() {
        for origin in [
            "https://example.com:80:80",
            "https://user:pw@example.com",
            "https://user@example.com",
            "https://[]",
            "https://[::1",
            "https://[::1]x",
            "https://[abcd]",
            "https://[:::]",
            "https://[1:2:3]",
            "https://[1:2:3:4:5:6:7:8:9]",
            "https://example.com:",
            "https://example.com:65536",
            "https://:443",
        ] {
            assert_eq!(
                Sitemap::new(origin).unwrap_err(),
                DiscoveryError::InvalidOrigin,
                "{origin}"
            );
        }
        for origin in [
            "https://example.com:8443",
            "http://[::1]:3000",
            "http://[::1]",
            "http://127.0.0.1:3000",
        ] {
            assert!(Sitemap::new(origin).is_ok(), "{origin}");
        }
        assert_eq!(
            SitemapUrl::new("https://user@example.com/").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            SitemapUrl::new("https://[abcd]/docs").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            RobotsTxt::new()
                .sitemap("https://example.com:1:2/sitemap.xml")
                .unwrap_err(),
            DiscoveryError::InvalidPath
        );
    }

    #[test]
    fn sitemap_rejects_javascript_and_origin_paths() {
        assert_eq!(
            Sitemap::new("https://example.com/docs").unwrap_err(),
            DiscoveryError::InvalidOrigin
        );
        assert_eq!(
            SitemapUrl::new("javascript:alert(1)").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            SitemapUrl::new("./relative").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            SitemapUrl::new("/ok")
                .unwrap()
                .lastmod("2026-13-01")
                .unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
    }

    #[test]
    fn lastmod_rejects_invalid_dates_and_non_date_inputs() {
        for value in [
            "2026-02-30",
            "2026-04-31",
            "2026-02-29",
            "1900-02-29",
            "0000-01-01",
            "-000001-01-01",
            "10000-01-01",
            "2026-1-01",
            "2026-01-1",
            "2026/01/01",
            " 2026-01-01",
            "2026-01-01 ",
            "2026-01-01Z",
            "2026-01-01T12:00:00Z",
            "2026-01-01[America/New_York]",
            "２０２６-01-01",
            "",
        ] {
            assert_eq!(
                SitemapUrl::new("/ok").unwrap().lastmod(value).unwrap_err(),
                DiscoveryError::InvalidLastmod,
                "page lastmod: {value}"
            );
            assert_eq!(
                SitemapIndexEntry::new("/sitemap-0.xml")
                    .unwrap()
                    .lastmod(value)
                    .unwrap_err(),
                DiscoveryError::InvalidLastmod,
                "index lastmod: {value}"
            );
        }
        for year in [-9999, -1, 0] {
            let date = Date::new(year, 1, 1).unwrap();
            assert_eq!(
                SitemapUrl::new("/ok")
                    .unwrap()
                    .lastmod_date(date)
                    .unwrap_err(),
                DiscoveryError::InvalidLastmod
            );
            assert_eq!(
                SitemapIndexEntry::new("/sitemap-0.xml")
                    .unwrap()
                    .lastmod_date(date)
                    .unwrap_err(),
                DiscoveryError::InvalidLastmod
            );
        }
    }

    #[test]
    fn lastmod_string_and_jiff_dates_render_the_supported_year_range() {
        for value in [
            "0001-01-01",
            "0099-12-31",
            "0100-01-01",
            "2000-02-29",
            "2024-02-29",
            "2026-01-31",
            "2026-04-30",
            "9999-12-31",
        ] {
            let date = value.parse::<Date>().unwrap();
            let pages = [
                SitemapUrl::new("/ok").unwrap().lastmod(value).unwrap(),
                SitemapUrl::new("/ok").unwrap().lastmod_date(date).unwrap(),
            ];
            let documents = [
                SitemapIndexEntry::new("/sitemap-0.xml")
                    .unwrap()
                    .lastmod(value)
                    .unwrap(),
                SitemapIndexEntry::new("/sitemap-0.xml")
                    .unwrap()
                    .lastmod_date(date)
                    .unwrap(),
            ];
            let expected = format!("<lastmod>{value}</lastmod>");
            for page in pages {
                let xml = Sitemap::new("https://example.com")
                    .unwrap()
                    .with_entry(page)
                    .unwrap()
                    .render_xml()
                    .unwrap();
                assert_eq!(xml.matches(&expected).count(), 1, "{xml}");
            }
            for document in documents {
                let xml = SitemapIndex::new("https://example.com")
                    .unwrap()
                    .with_entry(document)
                    .unwrap()
                    .render_xml()
                    .unwrap();
                assert_eq!(xml.matches(&expected).count(), 1, "{xml}");
            }
        }
    }

    #[test]
    fn sitemap_rejects_xml_forbidden_control_characters() {
        assert_eq!(
            SitemapUrl::new("/posts/a\u{0001}b").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            SitemapUrl::new("https://example.com/a\u{0001}b").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            Sitemap::new("https://exam\u{0001}ple.com").unwrap_err(),
            DiscoveryError::InvalidOrigin
        );
        assert_eq!(
            SitemapUrl::new("/ok\u{000B}").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            SitemapUrl::new("/ok\u{FFFE}").unwrap_err(),
            DiscoveryError::InvalidPath
        );
        assert_eq!(
            absolute_http_url("https://example.com", &BasePath::default(), "/x\u{001F}")
                .unwrap_err(),
            DiscoveryError::InvalidPath
        );
    }

    #[test]
    fn discovery_urls_reject_fragments_controls_and_excluded_characters() {
        for ch in [
            '#', '"', '<', '>', '\\', '^', '`', '{', '|', '}', '[', ']', '\u{7F}', '\u{85}',
        ] {
            let local = format!("/a{ch}b");
            let absolute = format!("https://example.com{local}");
            for path in [&local, &absolute] {
                assert_eq!(
                    SitemapUrl::new(path).unwrap_err(),
                    DiscoveryError::InvalidPath
                );
                assert_eq!(
                    SitemapIndexEntry::new(path).unwrap_err(),
                    DiscoveryError::InvalidPath
                );
                assert_eq!(
                    absolute_http_url("https://example.com", &BasePath::default(), path),
                    Err(DiscoveryError::InvalidPath)
                );
                assert_eq!(
                    RobotsTxt::new()
                        .sitemap_path("https://example.com", &BasePath::default(), path)
                        .unwrap_err(),
                    DiscoveryError::InvalidPath
                );
            }
            assert_eq!(
                RobotsTxt::new().sitemap(&absolute).unwrap_err(),
                DiscoveryError::InvalidPath
            );
            assert_eq!(
                Sitemap::new(&format!("https://exam{ch}ple.com")).unwrap_err(),
                DiscoveryError::InvalidOrigin
            );
        }
        for path in [
            "/a%7F",
            "/a%23b",
            "/%22%3C%3E%5B%5D",
            "/café",
            "/a'&b",
            "/a/../b",
            "/?q=../../x",
        ] {
            assert!(SitemapUrl::new(path).is_ok(), "{path}");
        }
        for path in [
            "/../secret",
            "/%2e%2e/secret",
            "/a/../../secret",
            "/a/.%2E/%2E./secret",
        ] {
            assert_eq!(
                SitemapUrl::new(path).unwrap_err(),
                DiscoveryError::InvalidPath,
                "{path}"
            );
            assert_eq!(
                SitemapIndexEntry::new(path).unwrap_err(),
                DiscoveryError::InvalidPath,
                "{path}"
            );
        }
        assert!(RobotsTxt::new()
            .sitemap("https://cdn.example/sitemap.xml?q=%23")
            .is_ok());
    }

    #[test]
    fn robots_txt_emits_groups_and_prefixed_sitemap() {
        let body = RobotsTxt::new()
            .group("*")
            .unwrap()
            .allow("/")
            .unwrap()
            .disallow("/admin")
            .unwrap()
            .group("BadBot")
            .unwrap()
            .disallow("/")
            .unwrap()
            .sitemap_path(
                "https://example.com",
                &BasePath::new("/apps/demo").unwrap(),
                "/sitemap.xml",
            )
            .unwrap()
            .render_txt();

        assert_eq!(
            body,
            "User-agent: *\nAllow: /\nDisallow: /admin\n\nUser-agent: BadBot\nDisallow: /\n\nSitemap: https://example.com/apps/demo/sitemap.xml\n"
        );
    }

    #[test]
    fn robots_txt_shared_agents_and_extensions_keep_group_scope() {
        let robots = RobotsTxt::new()
            .group_agents(["Applebot", "Bingbot"])
            .unwrap()
            .disallow("/private/")
            .unwrap()
            .crawl_delay(Duration::from_millis(1500))
            .directive("Clean-param", "ref /articles/")
            .unwrap()
            .directive("Clean-param", "sort /products_[v2]/*?a=1&b=2$")
            .unwrap()
            .group("SeznamBot")
            .unwrap()
            .allow("/")
            .unwrap()
            .directive("Request-Rate", "10/1m")
            .unwrap()
            .host("https://example.com/")
            .unwrap()
            .sitemap("https://example.com/pages.xml")
            .unwrap()
            .sitemap("https://example.com/products.xml")
            .unwrap();
        assert_eq!(robots.groups()[0].user_agent(), "Applebot");
        assert_eq!(robots.groups()[0].user_agents(), ["Applebot", "Bingbot"]);
        assert_eq!(
            robots.groups()[0].crawl_delay(),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(robots.groups()[0].directives().len(), 2);
        assert_eq!(
            robots.render_txt(),
            concat!(
                "User-agent: Applebot\nUser-agent: Bingbot\nDisallow: /private/\n",
                "Crawl-delay: 1.5\nClean-param: ref /articles/\nClean-param: sort /products_[v2]/*?a=1&b=2$\n\n",
                "User-agent: SeznamBot\nAllow: /\nRequest-Rate: 10/1m\n\n",
                "Host: https://example.com\nSitemap: https://example.com/pages.xml\nSitemap: https://example.com/products.xml\n"
            )
        );
    }

    #[test]
    fn robots_crawl_delay_preserves_exact_seconds_and_group_boundaries() {
        for (delay, seconds) in [
            (Duration::ZERO, "0"),
            (Duration::from_nanos(1), "0.000000001"),
            (Duration::from_millis(1), "0.001"),
            (Duration::new(1, 1), "1.000000001"),
            (Duration::from_secs(10), "10"),
            (Duration::MAX, "18446744073709551615.999999999"),
        ] {
            let robots = RobotsTxt::new()
                .crawl_delay(Duration::from_secs(99))
                .crawl_delay(delay)
                .group("OtherBot")
                .unwrap()
                .disallow("/private")
                .unwrap();
            assert_eq!(
                robots.render_txt(),
                format!(
                    "User-agent: *\nDisallow:\nCrawl-delay: {seconds}\n\nUser-agent: OtherBot\nDisallow: /private\n"
                )
            );
        }
    }

    #[test]
    fn robots_extensions_reject_injection_and_reserved_fields() {
        assert_eq!(
            RobotsTxt::new()
                .group_agents(std::iter::empty::<&str>())
                .unwrap_err(),
            DiscoveryError::InvalidRobotsUserAgent
        );
        assert_eq!(
            RobotsTxt::new()
                .group_agents(["GoodBot", "Bad\nBot"])
                .unwrap_err(),
            DiscoveryError::InvalidRobotsUserAgent
        );
        for name in [
            "",
            "User-Agent",
            "ALLOW",
            "Disallow",
            "Sitemap",
            "HOST",
            "Crawl-Delay",
            "Bad:Name",
            "Bad\nName",
            "Bad Name",
            "é",
            "-Name",
        ] {
            assert_eq!(
                RobotsTxt::new().directive(name, "value").unwrap_err(),
                DiscoveryError::InvalidRobotsDirective,
                "{name}"
            );
        }
        for value in [
            "a\nDisallow: /",
            "a\rHost: other.example",
            "a#comment",
            "a\tvalue",
            "a\u{7f}",
            "a\u{85}",
        ] {
            assert_eq!(
                RobotsTxt::new()
                    .directive("Request-Rate", value)
                    .unwrap_err(),
                DiscoveryError::InvalidRobotsDirective
            );
        }
        // Spaces and repeated extension fields are valid; extensions do not
        // bypass the typed fields or interpret their values as application paths.
        let txt = RobotsTxt::new()
            .directive("Clean-param", "ref /articles/")
            .unwrap()
            .with_base_path(BasePath::new("/app").unwrap())
            .render_txt();
        assert_eq!(
            txt,
            "User-agent: *\nDisallow:\nClean-param: ref /articles/\n"
        );
    }

    #[test]
    fn robots_host_accepts_authorities_and_origins_without_paths() {
        for host in [
            "example.com",
            "example.com:8443",
            "https://example.com",
            "[::1]:3000",
            "https://[::1]:3000",
        ] {
            assert_eq!(
                RobotsTxt::new().host(host).unwrap().render_txt(),
                format!("Host: {host}\n")
            );
        }
        for host in [
            "",
            "example.com/app",
            "example.com/",
            "https://example.com/app",
            "https://example.com/a/..",
            " https://example.com",
            "example.com#comment",
            "user@example.com",
            "https://example.com?x=1",
            "example.com\nDisallow: /",
        ] {
            assert_eq!(
                RobotsTxt::new().host(host).unwrap_err(),
                DiscoveryError::InvalidRobotsHost,
                "{host}"
            );
        }
    }

    #[test]
    fn robots_txt_prefixes_application_rule_paths() {
        let base = BasePath::new("/apps/demo").unwrap();
        let body = RobotsTxt::new()
            .group("*")
            .unwrap()
            .allow("/")
            .unwrap()
            .disallow("/admin")
            .unwrap()
            .disallow("*.pdf$")
            .unwrap()
            .disallow("")
            .unwrap()
            .sitemap_path("https://example.com", &base, "/sitemap.xml")
            .unwrap()
            .with_base_path(base)
            .render_txt();

        assert_eq!(
            body,
            "User-agent: *\nAllow: /apps/demo$\nAllow: /apps/demo?\nAllow: /apps/demo/\nDisallow: /apps/demo/admin\nDisallow: *.pdf$\nDisallow: \n\nSitemap: https://example.com/apps/demo/sitemap.xml\n"
        );
    }

    #[test]
    fn robots_txt_root_rules_cover_the_bare_mount_url() {
        let body = RobotsTxt::new()
            .group("PrivateBot")
            .unwrap()
            .disallow("/")
            .unwrap()
            .group("HomeBot")
            .unwrap()
            .allow("/$")
            .unwrap()
            .with_base_path(BasePath::new("/apps/demo").unwrap())
            .render_txt();
        assert_eq!(
            body,
            "User-agent: PrivateBot\nDisallow: /apps/demo$\nDisallow: /apps/demo?\nDisallow: /apps/demo/\n\nUser-agent: HomeBot\nAllow: /apps/demo$\nAllow: /apps/demo/$\n"
        );
    }

    #[test]
    fn robots_txt_base_path_replaces_instead_of_stacking() {
        let base = BasePath::new("/apps/demo").unwrap();
        let body = RobotsTxt::new()
            .disallow("/admin")
            .unwrap()
            .with_base_path(BasePath::new("/old").unwrap())
            .with_base_path(base.clone())
            .with_base_path(base)
            .render_txt();
        assert_eq!(body, "User-agent: *\nDisallow: /apps/demo/admin\n");
    }

    #[test]
    fn robots_txt_separates_groups_without_extra_blank_lines() {
        assert_eq!(RobotsTxt::new().render_txt(), "");
        assert_eq!(
            RobotsTxt::new()
                .sitemap("https://example.com/sitemap.xml?a=1&b=2")
                .unwrap()
                .render_txt(),
            "Sitemap: https://example.com/sitemap.xml?a=1&b=2\n"
        );
        let body = RobotsTxt::new()
            .group("GoodBot")
            .unwrap()
            .group("*")
            .unwrap()
            .disallow("/")
            .unwrap()
            .render_txt();
        assert_eq!(
            body,
            "User-agent: GoodBot\nDisallow:\n\nUser-agent: *\nDisallow: /\n"
        );
    }

    #[test]
    fn robots_txt_rejects_paths_crawlers_ignore() {
        assert_eq!(
            RobotsTxt::new().disallow("admin").unwrap_err(),
            DiscoveryError::InvalidRobotsPath
        );
        assert_eq!(
            RobotsTxt::new().disallow("/admin #old").unwrap_err(),
            DiscoveryError::InvalidRobotsField
        );
        assert!(RobotsTxt::new().disallow("").is_ok());
        assert!(RobotsTxt::new().disallow("*.pdf$").is_ok());
        assert_eq!(
            RobotsTxt::new()
                .disallow("https://x.test/admin")
                .unwrap_err(),
            DiscoveryError::InvalidRobotsPath
        );
        for path in ["//x.test/admin", "//admin"] {
            assert_eq!(
                RobotsTxt::new().disallow(path).unwrap_err(),
                DiscoveryError::InvalidPath,
                "{path}"
            );
            assert_eq!(
                RobotsTxt::new().allow(path).unwrap_err(),
                DiscoveryError::InvalidPath,
                "{path}"
            );
        }
    }

    #[test]
    fn robots_txt_rejects_control_characters() {
        assert_eq!(
            RobotsTxt::new().group("Bot\u{7F}").unwrap_err(),
            DiscoveryError::InvalidRobotsUserAgent
        );
        assert_eq!(
            RobotsTxt::new().disallow("/a\u{7F}").unwrap_err(),
            DiscoveryError::InvalidRobotsField
        );
        assert_eq!(
            RobotsTxt::new().group("*\nGooglebot").unwrap_err(),
            DiscoveryError::InvalidRobotsUserAgent
        );
        assert_eq!(
            RobotsTxt::new()
                .allow("/ok\r\nSitemap: https://evil.test")
                .unwrap_err(),
            DiscoveryError::InvalidRobotsField
        );
    }

    #[test]
    fn absolute_http_url_rejects_protocol_relative_and_overlong_locs() {
        assert_eq!(
            absolute_http_url("https://example.com", &BasePath::default(), "//cdn.test/x")
                .unwrap_err(),
            DiscoveryError::InvalidPath
        );
        let origin = "https://example.com";
        let at_limit = format!("/{}", "a".repeat(MAX_LOC_BYTES - origin.len() - 1));
        assert_eq!(
            absolute_http_url(origin, &BasePath::default(), &at_limit).unwrap_err(),
            DiscoveryError::LocTooLong
        );
        let under_limit = &at_limit[..at_limit.len() - 1];
        assert_eq!(
            absolute_http_url(origin, &BasePath::default(), under_limit)
                .unwrap()
                .len(),
            MAX_LOC_BYTES - 1
        );
        let absolute_at_limit = format!("{origin}{at_limit}");
        assert_eq!(
            absolute_http_url(origin, &BasePath::default(), &absolute_at_limit).unwrap_err(),
            DiscoveryError::LocTooLong
        );
        assert_eq!(
            RobotsTxt::new().sitemap(&absolute_at_limit).unwrap_err(),
            DiscoveryError::LocTooLong
        );
    }
}
