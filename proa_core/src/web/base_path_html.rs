// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::shared::attr_value::AttrValue;
use crate::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};

/// Trusted compiled HTML whose application-local URL attributes follow the request mount.
///
/// Like `StaticRaw`, this requires trusted static markup. It does not sanitize
/// HTML. Comments, text, and script/style contents are preserved byte for byte.
#[derive(Clone, Copy)]
pub struct BasePathHtml(pub &'static str);

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BasePathHtml {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if cx.request().base_path().as_str().is_empty() {
            return cx.output_mut().extend_static(self.0.as_bytes());
        }
        // proa-perf-ok: share the immutable mount while writing trusted HTML segments.
        let base_path = cx.request().base_path().clone();
        let mut written = 0;
        for offset in HtmlUrlOffsets::new(self.0) {
            cx.output_mut()
                .extend_static(self.0[written..offset].as_bytes())?;
            base_path.as_str().render_attr_value(cx)?;
            written = offset;
        }
        cx.output_mut().extend_static(self.0[written..].as_bytes())
    }
}

/// Locate root-relative attribute values without allocating or changing markup.
/// This tokenizer only inserts at attribute boundaries, never in text or raw text.
struct HtmlUrlOffsets<'a> {
    source: &'a str,
    position: usize,
    in_tag: bool,
    object_tag: bool,
    raw_tag: Option<&'a str>,
    srcset: Option<(usize, usize)>,
}

impl<'a> HtmlUrlOffsets<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
            in_tag: false,
            object_tag: false,
            raw_tag: None,
            srcset: None,
        }
    }

    fn skip_space(&mut self) {
        while self
            .source
            .as_bytes()
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }

    fn finish_tag(&mut self) {
        self.in_tag = false;
        if let Some(tag) = self.raw_tag.take() {
            if tag.eq_ignore_ascii_case("plaintext") {
                self.position = self.source.len();
                return;
            }
            // HTML raw-text/RCDATA ends only at the corresponding end tag.
            while let Some(index) = self.source[self.position..].find('<') {
                self.position += index;
                let rest = &self.source.as_bytes()[self.position..];
                if rest.starts_with(b"</")
                    && rest
                        .get(2..2 + tag.len())
                        .is_some_and(|name| name.eq_ignore_ascii_case(tag.as_bytes()))
                    && rest.get(2 + tag.len()).is_some_and(|byte| {
                        byte.is_ascii_whitespace() || matches!(*byte, b'>' | b'/')
                    })
                {
                    return;
                }
                self.position += 1;
            }
            self.position = self.source.len();
        }
    }
}

impl Iterator for HtmlUrlOffsets<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let bytes = self.source.as_bytes();
        loop {
            if let Some((mut start, end)) = self.srcset.take() {
                while start < end {
                    while start < end
                        && (bytes[start].is_ascii_whitespace() || bytes[start] == b',')
                    {
                        start += 1;
                    }
                    let value = start;
                    while start < end && !bytes[start].is_ascii_whitespace() {
                        start += 1;
                    }
                    let trailing_comma = start > value && bytes[start - 1] == b',';
                    if !trailing_comma {
                        let mut nesting = 0;
                        while start < end {
                            match bytes[start] {
                                b'(' => nesting += 1,
                                b')' => nesting -= 1,
                                b',' if nesting == 0 => {
                                    start += 1;
                                    break;
                                }
                                _ => {}
                            }
                            start += 1;
                        }
                    }
                    if let Some(offset) = root_url_start(&self.source[value..start]) {
                        self.srcset = Some((start, end));
                        return Some(value + offset);
                    }
                }
            }
            if self.position >= bytes.len() {
                return None;
            }
            if !self.in_tag {
                self.position += self.source[self.position..].find('<')?;
                if self.source[self.position..].starts_with("<!--") {
                    self.position += self.source[self.position..].find("-->")? + 3;
                    continue;
                }
                self.position += 1;
                if !bytes
                    .get(self.position)
                    .is_some_and(u8::is_ascii_alphabetic)
                {
                    self.position += self.source[self.position..].find('>')? + 1;
                    continue;
                }
                let start = self.position;
                while bytes
                    .get(self.position)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b':'))
                {
                    self.position += 1;
                }
                let tag = &self.source[start..self.position];
                self.object_tag = tag.eq_ignore_ascii_case("object");
                self.raw_tag = [
                    "script",
                    "style",
                    "textarea",
                    "title",
                    "xmp",
                    "iframe",
                    "noembed",
                    "noframes",
                    "plaintext",
                ]
                .into_iter()
                .any(|raw| tag.eq_ignore_ascii_case(raw))
                .then_some(tag);
                self.in_tag = true;
            }
            self.skip_space();
            match bytes.get(self.position) {
                None => return None,
                Some(b'>') => {
                    self.position += 1;
                    self.finish_tag();
                    continue;
                }
                Some(b'/') => {
                    self.position += 1;
                    continue;
                }
                _ => {}
            }
            let start = self.position;
            while bytes.get(self.position).is_some_and(|b| {
                !b.is_ascii_whitespace() && !matches!(b, b'=' | b'>' | b'/' | b'<' | b'\'' | b'"')
            }) {
                self.position += 1;
            }
            if start == self.position {
                self.position += 1;
                continue;
            }
            let attribute = &self.source[start..self.position];
            self.skip_space();
            if bytes.get(self.position) != Some(&b'=') {
                continue;
            }
            self.position += 1;
            self.skip_space();
            let quote = bytes
                .get(self.position)
                .copied()
                .filter(|b| matches!(b, b'\'' | b'"'));
            if quote.is_some() {
                self.position += 1;
            }
            let value = self.position;
            while let Some(byte) = bytes.get(self.position) {
                if quote.map_or(byte.is_ascii_whitespace() || *byte == b'>', |q| *byte == q) {
                    break;
                }
                self.position += 1;
            }
            let end = self.position;
            if quote.is_some() && self.position < bytes.len() {
                self.position += 1;
            }
            let is_url = [
                "href",
                "src",
                "action",
                "formaction",
                "poster",
                "cite",
                "xlink:href",
                "data-docs-markdown-src",
            ]
            .into_iter()
            .any(|name| attribute.eq_ignore_ascii_case(name))
                || (self.object_tag && attribute.eq_ignore_ascii_case("data"));
            if attribute.eq_ignore_ascii_case("srcset")
                || attribute.eq_ignore_ascii_case("imagesrcset")
            {
                self.srcset = Some((value, end));
            }
            if is_url {
                if let Some(offset) = root_url_start(&self.source[value..end]) {
                    return Some(value + offset);
                }
            }
        }
    }
}

fn first_url_char(source: &str) -> Option<(char, usize)> {
    if let Some(entity) = source.strip_prefix('&') {
        let end = entity.find(';')?;
        let name = &entity[..end];
        let character = match name {
            "sol" => '/',
            "Tab" => '\t',
            "NewLine" => '\n',
            _ => {
                let number = name.strip_prefix('#')?;
                let code = if let Some(hex) = number
                    .strip_prefix('x')
                    .or_else(|| number.strip_prefix('X'))
                {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    number.parse().ok()?
                };
                char::from_u32(code)?
            }
        };
        Some((character, end + 2))
    } else {
        source.chars().next().map(|c| (c, c.len_utf8()))
    }
}

fn root_url_start(source: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some((character, length)) = first_url_char(&source[offset..]) {
        if character.is_ascii_whitespace() {
            offset += length;
            continue;
        }
        if character != '/' {
            return None;
        }
        return (!matches!(first_url_char(&source[offset + length..]), Some(('/', _))))
            .then_some(offset);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_only_url_attributes_and_preserves_literal_and_raw_text() {
        const SOURCE: &str = r###"<!-- <a href="/comment"> --><p>/text</p><a href="/docs?a=1&amp;b=2">Docs</a><IMG SRC='/image.svg'><form action=/save><button formaction="/other"></button></form><a href="//cdn.test/a">CDN</a><a href="#part">Part</a><script src="/entry.js">let x = '<a href="/js">';</script><style>.x{content:'href="/css"'}</style><textarea><a href="/text"></textarea>"###;
        let mut cx = WebContext::new();
        cx.request_mut()
            .set_base_path(crate::BasePath::new("/apps/a&b").unwrap());
        BasePathHtml(SOURCE).render(&mut cx).unwrap();
        let output = cx.into_output().into_string();
        for suffix in [
            "/docs?a=1&amp;b=2",
            "/image.svg",
            "/save",
            "/other",
            "/entry.js",
        ] {
            assert!(
                output.contains(&format!("/apps/a&amp;b{suffix}")),
                "{output}"
            );
        }
        for literal in [
            "href=\"/comment\"",
            "href=\"/js\"",
            "href=\"/css\"",
            "href=\"/text\"",
            "href=\"//cdn.test/a\"",
            "href=\"#part\"",
        ] {
            assert!(output.contains(literal), "{literal}: {output}");
        }
        let mut root = WebContext::new();
        BasePathHtml(SOURCE).render(&mut root).unwrap();
        assert_eq!(root.into_output().into_string(), SOURCE);
    }
}

#[cfg(test)]
mod extra_tests {
    use super::*;
    #[test]
    fn article_entities_whitespace_and_srcsets_compose_without_touching_data_urls() {
        const SOURCE: &str = r#"<a href=" &#x2f;docs">A</a><a href="&#47;&#47;cdn.test/a">External</a><img srcset="/small.png 1x, /large.png 2x, data:image/png;base64,AAAA 3x, //cdn.test/img 4x"><img src="/one.svg">"#;
        let mut cx = WebContext::new();
        cx.request_mut()
            .set_base_path(crate::BasePath::new("/mount").unwrap());
        BasePathHtml(SOURCE).render(&mut cx).unwrap();
        assert_eq!(
            cx.into_output().into_string(),
            r#"<a href=" /mount&#x2f;docs">A</a><a href="&#47;&#47;cdn.test/a">External</a><img srcset="/mount/small.png 1x, /mount/large.png 2x, data:image/png;base64,AAAA 3x, //cdn.test/img 4x"><img src="/mount/one.svg">"#
        );
    }
}
