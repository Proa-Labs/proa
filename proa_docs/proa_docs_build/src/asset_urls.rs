// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Rewrite only parsed Markdown destinations, retaining authored labels, titles,
//! reference syntax, and code examples. This runs solely during docs builds.
use pulldown_cmark::{Event, LinkType, Parser, Tag};
use std::{collections::BTreeMap, ops::Range};

pub(crate) fn rewrite(markdown: &str, urls: &BTreeMap<String, String>) -> String {
    if urls.is_empty() {
        return markdown.to_owned();
    }
    let parser = Parser::new(markdown);
    let mut edits = Vec::new();
    for (_, definition) in parser.reference_definitions().iter() {
        if let Some(url) = versioned(&definition.dest, urls) {
            if let Some(range) =
                destination(markdown, definition.span.clone(), &definition.dest, b':')
            {
                edits.push((range, url));
            }
        }
    }
    for (event, span) in parser.into_offset_iter() {
        if let Event::Start(
            Tag::Image {
                link_type: LinkType::Inline,
                dest_url,
                ..
            }
            | Tag::Link {
                link_type: LinkType::Inline,
                dest_url,
                ..
            },
        ) = event
        {
            if let Some(url) = versioned(&dest_url, urls) {
                if let Some(range) = destination(markdown, span, &dest_url, b'(') {
                    edits.push((range, url));
                }
            }
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    edits.dedup_by(|a, b| a.0 == b.0);
    let mut output = markdown.to_owned();
    for (range, url) in edits.into_iter().rev() {
        output.replace_range(range, &url);
    }
    output
}

fn versioned(url: &str, urls: &BTreeMap<String, String>) -> Option<String> {
    let (url, fragment) = url
        .split_once('#')
        .map_or((url, None), |(u, f)| (u, Some(f)));
    let path = url.split_once('?').map_or(url, |(p, _)| p);
    let mut replacement = urls.get(path)?.clone();
    if let Some(fragment) = fragment {
        replacement.push('#');
        replacement.push_str(fragment);
    }
    Some(replacement)
}

// Locate the destination after the label, never an identical URL in alt text
// or a title. The parser has already established that this is a real link.
fn destination(
    markdown: &str,
    span: Range<usize>,
    url: &str,
    delimiter: u8,
) -> Option<Range<usize>> {
    let bytes = markdown.as_bytes();
    let mut i = span.start;
    if bytes.get(i) == Some(&b'!') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'[') {
        return None;
    }
    let mut depth = 1;
    i += 1;
    let mut ticks = 0;
    while i < span.end && depth != 0 {
        match bytes[i] {
            b'\\' if ticks == 0 => {
                i += 2;
                continue;
            }
            b'`' => {
                let start = i;
                while i < span.end && bytes[i] == b'`' {
                    i += 1;
                }
                let count = i - start;
                if ticks == 0 {
                    ticks = count;
                } else if ticks == count {
                    ticks = 0;
                }
                continue;
            }
            b'[' if ticks == 0 => depth += 1,
            b']' if ticks == 0 => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    if bytes.get(i) != Some(&delimiter) {
        return None;
    }
    i += 1;
    while i < span.end && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if bytes.get(i) == Some(&b'<') {
        i += 1;
    }
    let end = i.checked_add(url.len())?;
    (end <= span.end && markdown.get(i..end) == Some(url)).then_some(i..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_destinations_preserving_labels_titles_references_and_code() {
        let urls = [("/static/a.svg".into(), "/static/a.svg?v=new".into())].into();
        let source = "![label /static/a.svg](/static/a.svg \"/static/a.svg\")\n\n![old](/static/a.svg?v=2#part)\n\n![nested [label]](</static/a.svg>)\n\n![ref][diagram]\n\n[diagram]: /static/a.svg \"title\"\n\n`![code](/static/a.svg)`\n\n```md\n![code](/static/a.svg)\n```\n\n![remote](https://example.com/a.svg)\n";
        let output = rewrite(source, &urls);
        assert!(output.contains("![label /static/a.svg](/static/a.svg?v=new \"/static/a.svg\")"));
        assert!(output.contains("![old](/static/a.svg?v=new#part)"));
        assert!(output.contains("![nested [label]](</static/a.svg?v=new>)"));
        assert!(output.contains("[diagram]: /static/a.svg?v=new \"title\""));
        assert!(output.contains("`![code](/static/a.svg)`"));
        assert!(output.contains("```md\n![code](/static/a.svg)\n```"));
        assert!(output.contains("![remote](https://example.com/a.svg)"));
        assert_eq!(rewrite(&output, &urls), output);
    }
}
