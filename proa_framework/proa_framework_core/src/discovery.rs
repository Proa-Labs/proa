// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Sitemap and robots.txt documents that honor the public origin and runtime
//! base path.

use proa_core::text::UnsafeRaw;
use proa_core::{BasePath, DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::xml_sync;
use thiserror::Error;

const MAX_LOC_BYTES: usize = 2048;
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
    #[error("path must be a root-relative application path or an http(s) URL")]
    InvalidPath,
    #[error("lastmod must be a YYYY-MM-DD calendar date")]
    InvalidLastmod,
    #[error("sitemap loc exceeds 2048 bytes")]
    LocTooLong,
    #[error("robots field cannot be empty or contain CR, LF, or other control characters")]
    InvalidRobotsField,
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// How often a sitemap URL is expected to change.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapUrl {
    path: String,
    lastmod: Option<String>,
    changefreq: Option<ChangeFreq>,
    priority: Option<SitemapPriority>,
}

impl SitemapUrl {
    pub fn new(path: impl Into<String>) -> Result<Self, DiscoveryError> {
        let path = path.into();
        validate_entry_path(&path)?;
        Ok(Self {
            path,
            lastmod: None,
            changefreq: None,
            priority: None,
        })
    }

    pub fn lastmod(mut self, lastmod: impl Into<String>) -> Result<Self, DiscoveryError> {
        let lastmod = lastmod.into();
        if !is_calendar_date(&lastmod) {
            return Err(DiscoveryError::InvalidLastmod);
        }
        self.lastmod = Some(lastmod);
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
        &self.path
    }
}

/// A sitemap.xml document.
#[derive(Debug, Clone)]
pub struct Sitemap {
    origin: String,
    base_path: BasePath,
    urls: Vec<SitemapUrl>,
}

impl Sitemap {
    pub fn new(origin: &str) -> Result<Self, DiscoveryError> {
        Ok(Self {
            origin: normalize_origin(origin)?.to_string(),
            base_path: BasePath::default(),
            urls: Vec::new(),
        })
    }

    pub fn with_base_path(mut self, base_path: BasePath) -> Self {
        self.base_path = base_path;
        self
    }

    pub fn with_url(self, path: impl Into<String>) -> Result<Self, DiscoveryError> {
        Ok(self.with_entry(SitemapUrl::new(path)?))
    }

    pub fn with_entry(mut self, url: SitemapUrl) -> Self {
        self.urls.push(url);
        self
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn urls(&self) -> &[SitemapUrl] {
        &self.urls
    }

    /// Render the sitemap protocol XML.
    ///
    /// Allocates the response body once per request, outside HTML page rendering.
    // proa-perf-ok: sitemap XML is emitted once per request outside Proa HTML rendering
    pub fn render_xml(&self) -> Result<String, DiscoveryError> {
        let mut cx = WebContext::new();
        self.write_xml(&mut cx)?;
        Ok(cx.into_output().into_string())
    }

    fn write_xml<L: DataLoader>(&self, cx: &mut WebContext<L>) -> Result<(), DiscoveryError> {
        WebRenderSync::render(
            UnsafeRaw(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">",
            ),
            cx,
        )?;
        for url in &self.urls {
            let loc = absolute_http_url(&self.origin, &self.base_path, &url.path)?;
            let lastmod = url.lastmod.as_deref().unwrap_or("");
            let changefreq = url.changefreq.map(ChangeFreq::as_str).unwrap_or("");
            let priority = url.priority.map(SitemapPriority::as_str).unwrap_or("");
            xml_sync! {
                <url>
                    <loc>{loc.as_str()}</loc>
                    {if !lastmod.is_empty() {
                        xml_sync! { <lastmod>{lastmod}</lastmod> }
                    }}
                    {if !changefreq.is_empty() {
                        xml_sync! { <changefreq>{changefreq}</changefreq> }
                    }}
                    {if !priority.is_empty() {
                        xml_sync! { <priority>{priority}</priority> }
                    }}
                </url>
            }
            .render(cx)?;
        }
        WebRenderSync::render(UnsafeRaw("</urlset>\n"), cx)?;
        Ok(())
    }
}

/// One `User-agent` group in robots.txt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsGroup {
    user_agent: String,
    rules: Vec<RobotsRule>,
}

impl RobotsGroup {
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub fn rules(&self) -> &[RobotsRule] {
        &self.rules
    }
}

/// An `Allow` or `Disallow` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RobotsRule {
    Allow(String),
    Disallow(String),
}

/// A robots.txt document.
#[derive(Debug, Clone, Default)]
pub struct RobotsTxt {
    groups: Vec<RobotsGroup>,
    sitemaps: Vec<String>,
}

impl RobotsTxt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn group(mut self, user_agent: &str) -> Result<Self, DiscoveryError> {
        validate_robots_field(user_agent)?;
        self.groups.push(RobotsGroup {
            user_agent: user_agent.to_string(),
            rules: Vec::new(),
        });
        Ok(self)
    }

    pub fn allow(self, path: &str) -> Result<Self, DiscoveryError> {
        self.push_rule(RobotsRule::Allow(checked_robots_field(path)?))
    }

    pub fn disallow(self, path: &str) -> Result<Self, DiscoveryError> {
        self.push_rule(RobotsRule::Disallow(checked_robots_field(path)?))
    }

    pub fn sitemap(mut self, loc: &str) -> Result<Self, DiscoveryError> {
        if !is_absolute_http_url(loc) {
            return Err(DiscoveryError::InvalidPath);
        }
        validate_absolute_http_url(loc)?;
        if loc.len() > MAX_LOC_BYTES {
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

    pub fn groups(&self) -> &[RobotsGroup] {
        &self.groups
    }

    pub fn sitemaps(&self) -> &[String] {
        &self.sitemaps
    }

    /// Render robots.txt as UTF-8 text.
    // proa-perf-ok: robots.txt is emitted once per request outside Proa HTML rendering
    pub fn render_txt(&self) -> String {
        let mut out = String::new();
        for (index, group) in self.groups.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str("User-agent: ");
            out.push_str(&group.user_agent);
            out.push('\n');
            for rule in &group.rules {
                match rule {
                    RobotsRule::Allow(path) => {
                        out.push_str("Allow: ");
                        out.push_str(path);
                        out.push('\n');
                    }
                    RobotsRule::Disallow(path) => {
                        out.push_str("Disallow: ");
                        out.push_str(path);
                        out.push('\n');
                    }
                }
            }
        }
        if !self.sitemaps.is_empty() && !out.is_empty() {
            out.push('\n');
        }
        for loc in &self.sitemaps {
            out.push_str("Sitemap: ");
            out.push_str(loc);
            out.push('\n');
        }
        out
    }

    fn push_rule(mut self, rule: RobotsRule) -> Result<Self, DiscoveryError> {
        if self.groups.is_empty() {
            self.groups.push(RobotsGroup {
                user_agent: "*".to_string(),
                rules: Vec::new(),
            });
        }
        self.groups
            .last_mut()
            .expect("group inserted above")
            .rules
            .push(rule);
        Ok(self)
    }
}

/// Join a public origin, runtime base path, and application path into an
/// absolute `http` or `https` URL.
pub fn absolute_http_url(
    origin: &str,
    base_path: &BasePath,
    path: &str,
) -> Result<String, DiscoveryError> {
    if is_absolute_http_url(path) {
        validate_absolute_http_url(path)?;
        if path.len() > MAX_LOC_BYTES {
            return Err(DiscoveryError::LocTooLong);
        }
        return Ok(path.to_string());
    }

    let origin = normalize_origin(origin)?;
    validate_entry_path(path)?;
    let composed = format!("{}{}", origin, base_path.url(path));
    if composed.len() > MAX_LOC_BYTES {
        return Err(DiscoveryError::LocTooLong);
    }
    Ok(composed)
}

fn normalize_origin(origin: &str) -> Result<&str, DiscoveryError> {
    let origin = origin.trim();
    let origin = origin.strip_suffix('/').unwrap_or(origin);
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .ok_or(DiscoveryError::InvalidOrigin)?;
    if rest.is_empty() {
        return Err(DiscoveryError::InvalidOrigin);
    }
    if rest.starts_with('[') {
        let end = rest.find(']').ok_or(DiscoveryError::InvalidOrigin)?;
        let after = &rest[end + 1..];
        if !after.is_empty() {
            let port = after
                .strip_prefix(':')
                .ok_or(DiscoveryError::InvalidOrigin)?;
            if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(DiscoveryError::InvalidOrigin);
            }
        }
    } else if let Some((host, port)) = rest.rsplit_once(':') {
        if host.is_empty() || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(DiscoveryError::InvalidOrigin);
        }
    }
    if rest.contains(['/', '?', '#', '\\', ' ']) {
        return Err(DiscoveryError::InvalidOrigin);
    }
    Ok(origin)
}

fn is_absolute_http_url(path: &str) -> bool {
    path.starts_with("https://") || path.starts_with("http://")
}

fn validate_absolute_http_url(url: &str) -> Result<(), DiscoveryError> {
    if url.contains([' ', '\t', '\n', '\r', '\\']) {
        return Err(DiscoveryError::InvalidPath);
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or(DiscoveryError::InvalidPath)?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return Err(DiscoveryError::InvalidPath);
    }
    Ok(())
}

fn validate_entry_path(path: &str) -> Result<(), DiscoveryError> {
    if is_absolute_http_url(path) {
        return validate_absolute_http_url(path);
    }
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(DiscoveryError::InvalidPath);
    }
    if path.contains([' ', '\t', '\n', '\r', '\\']) {
        return Err(DiscoveryError::InvalidPath);
    }
    Ok(())
}

fn is_calendar_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let month = parse_two_digits(&bytes[5..7]);
    let day = parse_two_digits(&bytes[8..10]);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn parse_two_digits(bytes: &[u8]) -> u8 {
    (bytes[0] - b'0') * 10 + (bytes[1] - b'0')
}

fn validate_robots_field(value: &str) -> Result<(), DiscoveryError> {
    if value.is_empty() || value.bytes().any(|byte| byte < b' ') {
        return Err(DiscoveryError::InvalidRobotsField);
    }
    Ok(())
}

fn checked_robots_field(value: &str) -> Result<String, DiscoveryError> {
    if value.bytes().any(|byte| byte < b' ') {
        return Err(DiscoveryError::InvalidRobotsField);
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            );

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
    fn sitemap_escapes_query_ampersands_in_loc() {
        let xml = Sitemap::new("https://example.com")
            .unwrap()
            .with_url("/search?q=a&b=c")
            .unwrap()
            .render_xml()
            .unwrap();
        assert!(xml.contains("<loc>https://example.com/search?q=a&amp;b=c</loc>"));
        assert!(!xml.contains("<loc>https://example.com/search?q=a&b=c</loc>"));
    }

    #[test]
    fn sitemap_keeps_absolute_http_entries() {
        let xml = Sitemap::new("https://example.com")
            .unwrap()
            .with_url("https://cdn.example.com/docs")
            .unwrap()
            .render_xml()
            .unwrap();
        assert!(xml.contains("<loc>https://cdn.example.com/docs</loc>"));
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
    fn robots_txt_rejects_control_characters() {
        assert_eq!(
            RobotsTxt::new().group("*\nGooglebot").unwrap_err(),
            DiscoveryError::InvalidRobotsField
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
        let long_path = format!("/{}", "a".repeat(MAX_LOC_BYTES));
        assert_eq!(
            absolute_http_url("https://example.com", &BasePath::default(), &long_path).unwrap_err(),
            DiscoveryError::LocTooLong
        );
    }
}
