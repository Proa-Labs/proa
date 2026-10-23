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
    #[error("absolute sitemap URL must use the sitemap origin")]
    CrossOrigin,
    #[error("sitemap loc must be shorter than 2048 bytes")]
    LocTooLong,
    #[error("robots field cannot contain whitespace, `#`, or control characters")]
    InvalidRobotsField,
    #[error("robots rule path must be empty or start with `/` or `*`")]
    InvalidRobotsPath,
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
        self.with_entry(SitemapUrl::new(path)?)
    }

    /// Add an entry. Absolute entries must use the sitemap origin, because the
    /// sitemap protocol only covers URLs on the host that serves the sitemap.
    pub fn with_entry(mut self, url: SitemapUrl) -> Result<Self, DiscoveryError> {
        if is_absolute_http_url(&url.path) && !has_origin(&url.path, &self.origin) {
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
///
/// Crawlers fetch `/robots.txt` at the origin root. Incoming Proa routes stay
/// unprefixed, so the handler path is still `/robots.txt`; a prefix-stripping
/// proxy must forward that origin-root URL to the handler. Use `allow_path`,
/// `disallow_path`, and `sitemap_path` so rules and sitemap locs use the
/// public mount prefix (`Disallow: /admin` does not block `/apps/demo/admin`).
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

    /// Add an `Allow` rule. The path must be empty or start with `/` or `*`.
    pub fn allow(self, path: &str) -> Result<Self, DiscoveryError> {
        self.push_rule(RobotsRule::Allow(checked_robots_path(path)?))
    }

    /// Add a `Disallow` rule. The path must be empty or start with `/` or `*`.
    pub fn disallow(self, path: &str) -> Result<Self, DiscoveryError> {
        self.push_rule(RobotsRule::Disallow(checked_robots_path(path)?))
    }

    /// Allow a public path, joining `base_path` the same way sitemap locs are.
    pub fn allow_path(self, base_path: &BasePath, path: &str) -> Result<Self, DiscoveryError> {
        validate_local_path(path)?;
        // proa-perf-ok: robots.txt is built once per request outside Proa HTML rendering
        self.allow(&base_path.url(path).to_string())
    }

    /// Disallow a public path, joining `base_path` the same way sitemap locs are.
    pub fn disallow_path(self, base_path: &BasePath, path: &str) -> Result<Self, DiscoveryError> {
        validate_local_path(path)?;
        // proa-perf-ok: robots.txt is built once per request outside Proa HTML rendering
        self.disallow(&base_path.url(path).to_string())
    }

    pub fn sitemap(mut self, loc: &str) -> Result<Self, DiscoveryError> {
        if !is_absolute_http_url(loc) {
            return Err(DiscoveryError::InvalidPath);
        }
        validate_absolute_http_url(loc)?;
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
            if group.rules.is_empty() {
                // An empty `Disallow` allows everything. Without a rule line, this
                // `User-agent` merges into the next group (RFC 9309 section 2.1).
                out.push_str("Disallow:\n");
            }
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
        if path.len() >= MAX_LOC_BYTES {
            return Err(DiscoveryError::LocTooLong);
        }
        return Ok(path.to_string());
    }

    let origin = normalize_origin(origin)?;
    validate_entry_path(path)?;
    let composed = format!("{}{}", origin, base_path.url(path));
    if composed.len() >= MAX_LOC_BYTES {
        return Err(DiscoveryError::LocTooLong);
    }
    Ok(composed)
}

fn normalize_origin(origin: &str) -> Result<&str, DiscoveryError> {
    let origin = origin.trim();
    let origin = origin.strip_suffix('/').unwrap_or(origin);
    let authority = strip_http_scheme(origin).ok_or(DiscoveryError::InvalidOrigin)?;
    if contains_invalid_url_char(authority) || !is_valid_authority(authority) {
        return Err(DiscoveryError::InvalidOrigin);
    }
    Ok(origin)
}

fn strip_http_scheme(url: &str) -> Option<&str> {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
}

fn is_absolute_http_url(path: &str) -> bool {
    strip_http_scheme(path).is_some()
}

/// `host[:port]` with no userinfo, path, query, or fragment. Bracketed hosts
/// must be non-empty IPv6 literals; ports must be 1 to 5 digits up to 65535.
fn is_valid_authority(authority: &str) -> bool {
    let (host_ok, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else {
            return false;
        };
        let host_ok = !host.is_empty()
            && host
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b':' || byte == b'.');
        let port = match after {
            "" => None,
            _ => match after.strip_prefix(':') {
                Some(port) => Some(port),
                None => return false,
            },
        };
        (host_ok, port)
    } else {
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        (
            !host.is_empty() && !host.contains(['@', '[', ']', '/', '?', '#']),
            port,
        )
    };
    host_ok
        && port.is_none_or(|port| {
            !port.is_empty()
                && port.len() <= 5
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && port.parse::<u16>().is_ok()
        })
}

/// True when `url` is `origin` itself or a URL on it (ASCII case-insensitive
/// scheme and host). Rejects look-alikes such as `https://example.com.evil.test`.
fn has_origin(url: &str, origin: &str) -> bool {
    url.len() >= origin.len()
        && url.is_char_boundary(origin.len())
        && url[..origin.len()].eq_ignore_ascii_case(origin)
        && matches!(
            url.as_bytes().get(origin.len()),
            None | Some(b'/' | b'?' | b'#')
        )
}

fn validate_absolute_http_url(url: &str) -> Result<(), DiscoveryError> {
    if contains_invalid_url_char(url) {
        return Err(DiscoveryError::InvalidPath);
    }
    let rest = strip_http_scheme(url).ok_or(DiscoveryError::InvalidPath)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if !is_valid_authority(authority) {
        return Err(DiscoveryError::InvalidPath);
    }
    Ok(())
}

/// A root-relative application path (`/...`, not protocol-relative `//...`).
fn validate_local_path(path: &str) -> Result<(), DiscoveryError> {
    if !path.starts_with('/') || path.starts_with("//") || contains_invalid_url_char(path) {
        return Err(DiscoveryError::InvalidPath);
    }
    Ok(())
}

fn validate_entry_path(path: &str) -> Result<(), DiscoveryError> {
    if is_absolute_http_url(path) {
        return validate_absolute_http_url(path);
    }
    validate_local_path(path)
}

/// Space, ASCII controls, backslash, and XML 1.0 non-Char values (including
/// U+FFFE and U+FFFF). XmlPolicy only rejects NUL, so these must be caught
/// before a `<loc>` is written.
fn contains_invalid_url_char(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ch <= ' ' || ch == '\\' || !is_xml10_char(ch))
}

fn is_xml10_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
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
    let year = parse_four_digits(&bytes[0..4]);
    let month = parse_two_digits(&bytes[5..7]);
    let day = parse_two_digits(&bytes[8..10]);
    day >= 1 && day <= days_in_month(year, month)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn parse_four_digits(bytes: &[u8]) -> u16 {
    u16::from(bytes[0] - b'0') * 1000
        + u16::from(bytes[1] - b'0') * 100
        + u16::from(bytes[2] - b'0') * 10
        + u16::from(bytes[3] - b'0')
}

fn parse_two_digits(bytes: &[u8]) -> u8 {
    (bytes[0] - b'0') * 10 + (bytes[1] - b'0')
}

/// RFC 9309 values exclude controls (including DEL), space, and `#`, which
/// starts a comment and would silently truncate the value.
fn has_invalid_robots_byte(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte <= b' ' || byte == b'#' || byte == 0x7F)
}

fn validate_robots_field(value: &str) -> Result<(), DiscoveryError> {
    if value.is_empty() || has_invalid_robots_byte(value) {
        return Err(DiscoveryError::InvalidRobotsField);
    }
    Ok(())
}

/// Crawlers ignore rules whose path does not start with `/` or `*`, so such a
/// rule would leave the intended path crawlable. Empty means "match nothing".
fn checked_robots_path(value: &str) -> Result<String, DiscoveryError> {
    if has_invalid_robots_byte(value) {
        return Err(DiscoveryError::InvalidRobotsField);
    }
    if !value.is_empty() && !value.starts_with(['/', '*']) {
        return Err(DiscoveryError::InvalidRobotsPath);
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
    fn sitemap_keeps_same_origin_absolute_entries() {
        let xml = Sitemap::new("https://example.com")
            .unwrap()
            .with_base_path(BasePath::new("/apps/demo").unwrap())
            .with_url("https://example.com/docs")
            .unwrap()
            .with_url("https://EXAMPLE.com")
            .unwrap()
            .render_xml()
            .unwrap();
        assert!(xml.contains("<loc>https://example.com/docs</loc>"));
        assert!(xml.contains("<loc>https://EXAMPLE.com</loc>"));
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
    fn lastmod_rejects_nonexistent_calendar_dates() {
        let url = || SitemapUrl::new("/ok").unwrap();
        assert_eq!(
            url().lastmod("2026-02-30").unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
        assert_eq!(
            url().lastmod("2026-04-31").unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
        assert_eq!(
            url().lastmod("2026-02-29").unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
        assert_eq!(
            url().lastmod("1900-02-29").unwrap_err(),
            DiscoveryError::InvalidLastmod
        );
        assert!(url().lastmod("2026-01-31").is_ok());
        assert!(url().lastmod("2026-04-30").is_ok());
        assert!(url().lastmod("2024-02-29").is_ok());
        assert!(url().lastmod("2000-02-29").is_ok());
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
    fn robots_txt_prefixes_public_rule_paths() {
        let base = BasePath::new("/apps/demo").unwrap();
        let body = RobotsTxt::new()
            .group("*")
            .unwrap()
            .allow_path(&base, "/")
            .unwrap()
            .disallow_path(&base, "/admin")
            .unwrap()
            .sitemap_path("https://example.com", &base, "/sitemap.xml")
            .unwrap()
            .render_txt();

        assert_eq!(
            body,
            "User-agent: *\nAllow: /apps/demo/\nDisallow: /apps/demo/admin\n\nSitemap: https://example.com/apps/demo/sitemap.xml\n"
        );
    }

    #[test]
    fn robots_txt_keeps_empty_groups_separate() {
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
        let base = BasePath::new("/apps/demo").unwrap();
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
        for path in ["https://x.test/admin", "//x.test/admin", "admin"] {
            assert_eq!(
                RobotsTxt::new().disallow_path(&base, path).unwrap_err(),
                DiscoveryError::InvalidPath,
                "{path}"
            );
            assert_eq!(
                RobotsTxt::new().allow_path(&base, path).unwrap_err(),
                DiscoveryError::InvalidPath,
                "{path}"
            );
        }
    }

    #[test]
    fn robots_txt_rejects_control_characters() {
        assert_eq!(
            RobotsTxt::new().group("Bot\u{7F}").unwrap_err(),
            DiscoveryError::InvalidRobotsField
        );
        assert_eq!(
            RobotsTxt::new().disallow("/a\u{7F}").unwrap_err(),
            DiscoveryError::InvalidRobotsField
        );
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
