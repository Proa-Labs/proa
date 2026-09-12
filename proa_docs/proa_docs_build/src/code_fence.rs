// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineHighlights {
    ranges: Vec<(usize, usize)>,
}

impl LineHighlights {
    fn parse(token: &str) -> Option<Self> {
        let inner = token.strip_prefix('{')?.strip_suffix('}')?;
        if inner.is_empty() {
            return None;
        }

        let mut ranges = Vec::new();
        for part in inner.split(',') {
            let (start, end) = match part.split_once('-') {
                Some((start, end)) if !end.contains('-') => {
                    (parse_line_number(start)?, parse_line_number(end)?)
                }
                Some(_) => return None,
                None => {
                    let line = parse_line_number(part)?;
                    (line, line)
                }
            };
            if start > end {
                return None;
            }
            ranges.push((start, end));
        }

        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            if let Some((_, current_end)) = merged.last_mut() {
                if start <= current_end.saturating_add(1) {
                    *current_end = (*current_end).max(end);
                    continue;
                }
            }
            merged.push((start, end));
        }

        Some(Self { ranges: merged })
    }

    pub(crate) fn contains(&self, line: usize) -> bool {
        self.ranges
            .iter()
            .any(|(start, end)| (*start..=*end).contains(&line))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeFenceInfo<'a> {
    pub(crate) language: &'a str,
    pub(crate) filename: Option<String>,
    pub(crate) line_highlights: Option<LineHighlights>,
    line_highlight_token: Option<&'a str>,
}

impl<'a> CodeFenceInfo<'a> {
    pub(crate) fn parse(info: &'a str) -> Self {
        let tokens = metadata_tokens(info);
        let first = tokens.first().copied().unwrap_or_default();
        let first_highlights = LineHighlights::parse(first);
        let first_is_braced_metadata = first.starts_with('{') && first.ends_with('}');
        let first_is_filename = first.starts_with("filename=");
        let language =
            if first_highlights.is_some() || first_is_braced_metadata || first_is_filename {
                ""
            } else {
                first
            };

        let mut line_highlights = first_highlights;
        let mut line_highlight_token = line_highlights.as_ref().map(|_| first);
        let mut filename = None;
        for token in tokens {
            if filename.is_none() {
                filename = parse_filename(token);
            }
            if line_highlights.is_none() {
                if let Some(parsed) = LineHighlights::parse(token) {
                    line_highlights = Some(parsed);
                    line_highlight_token = Some(token);
                }
            }
        }

        Self {
            language,
            filename,
            line_highlights,
            line_highlight_token,
        }
    }

    pub(crate) fn normalized(&self) -> String {
        let mut normalized = self.language.to_string();
        if let Some(filename) = self.filename.as_deref() {
            push_metadata_separator(&mut normalized);
            normalized.push_str("filename=\"");
            push_escaped_filename(&mut normalized, filename);
            normalized.push('"');
        }
        if let Some(token) = self.line_highlight_token {
            push_metadata_separator(&mut normalized);
            normalized.push_str(token);
        }
        normalized
    }
}

fn metadata_tokens(info: &str) -> Vec<&str> {
    let bytes = info.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }

        let start = index;
        let mut quote = None;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte == b'\\' && quote.is_some() && index + 1 < bytes.len() {
                index += 2;
                continue;
            }
            if matches!(byte, b'\'' | b'"') {
                if quote == Some(byte) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(byte);
                }
                index += 1;
                continue;
            }
            if quote.is_none() && byte.is_ascii_whitespace() {
                break;
            }
            index += 1;
        }
        tokens.push(&info[start..index]);
    }

    tokens
}

fn parse_filename(token: &str) -> Option<String> {
    let raw = token.strip_prefix("filename=")?;
    if raw.is_empty() {
        return None;
    }

    let value = match raw.as_bytes().first().copied() {
        Some(quote @ (b'\'' | b'"')) if raw.as_bytes().last() == Some(&quote) => {
            unescape_quoted_filename(&raw[1..raw.len() - 1], quote)
        }
        Some(b'\'' | b'"') => return None,
        Some(_) => raw.to_string(),
        None => return None,
    };

    (!value.trim().is_empty()).then_some(value)
}

fn unescape_quoted_filename(raw: &str, quote: u8) -> String {
    let mut value = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            value.push(ch);
            continue;
        }

        match chars.next() {
            Some(next) if next == char::from(quote) || next == '\\' => value.push(next),
            Some(next) => {
                value.push('\\');
                value.push(next);
            }
            None => value.push('\\'),
        }
    }
    value
}

fn push_metadata_separator(metadata: &mut String) {
    if !metadata.is_empty() {
        metadata.push(' ');
    }
}

fn push_escaped_filename(metadata: &mut String, filename: &str) {
    for ch in filename.chars() {
        if matches!(ch, '\\' | '"') {
            metadata.push('\\');
        }
        metadata.push(ch);
    }
}

fn parse_line_number(raw: &str) -> Option<usize> {
    let number = raw.parse::<usize>().ok()?;
    (number > 0).then_some(number)
}

pub(crate) fn split_line_ending(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, "\r\n")
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, "\n")
    } else {
        (line, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_merges_line_ranges_without_expanding_them() {
        let info = CodeFenceInfo::parse("rust {1,4-5,2-4,999999999}");

        assert_eq!(info.language, "rust");
        assert_eq!(info.filename, None);
        assert_eq!(info.line_highlight_token, Some("{1,4-5,2-4,999999999}"));
        assert_eq!(
            info.line_highlights.unwrap().ranges,
            vec![(1, 5), (999_999_999, 999_999_999)]
        );
    }

    #[test]
    fn supports_a_selector_without_a_language() {
        let info = CodeFenceInfo::parse("{2-3}");

        assert_eq!(info.language, "");
        assert!(info.line_highlights.as_ref().unwrap().contains(2));
        assert_eq!(info.normalized(), "{2-3}");
    }

    #[test]
    fn ignores_invalid_line_selectors() {
        for selector in ["{}", "{0}", "{4-2}", "{1,,2}", "{1-2-3}", "{wat}"] {
            let raw = format!("rust {selector}");
            let info = CodeFenceInfo::parse(&raw);
            assert_eq!(info.language, "rust");
            assert!(info.line_highlights.is_none(), "accepted {selector}");
            assert_eq!(info.normalized(), "rust");
        }

        let info = CodeFenceInfo::parse("{0}");
        assert_eq!(info.language, "");
        assert!(info.line_highlights.is_none());
        assert_eq!(info.normalized(), "");
    }

    #[test]
    fn preserves_quoted_filenames_with_spaces_and_line_ranges() {
        let info = CodeFenceInfo::parse("rust filename=\"src/components/hero card.rs\" {2,4-5}");

        assert_eq!(info.language, "rust");
        assert_eq!(
            info.filename.as_deref(),
            Some("src/components/hero card.rs")
        );
        assert!(info.line_highlights.as_ref().unwrap().contains(4));
        assert_eq!(
            info.normalized(),
            "rust filename=\"src/components/hero card.rs\" {2,4-5}"
        );
    }

    #[test]
    fn supports_filename_metadata_without_a_language() {
        let info = CodeFenceInfo::parse("filename=\"Terminal\"");

        assert_eq!(info.language, "");
        assert_eq!(info.filename.as_deref(), Some("Terminal"));
        assert_eq!(info.normalized(), "filename=\"Terminal\"");
    }

    #[test]
    fn normalizes_unquoted_and_escaped_filenames() {
        let unquoted = CodeFenceInfo::parse("rust filename=build.rs");
        assert_eq!(unquoted.filename.as_deref(), Some("build.rs"));
        assert_eq!(unquoted.normalized(), "rust filename=\"build.rs\"");

        let quoted = CodeFenceInfo::parse(r#"rust filename="examples/\"quoted\".rs""#);
        assert_eq!(quoted.filename.as_deref(), Some("examples/\"quoted\".rs"));
        assert_eq!(
            quoted.normalized(),
            r#"rust filename="examples/\"quoted\".rs""#
        );
    }
}
