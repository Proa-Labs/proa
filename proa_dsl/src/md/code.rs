// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! CommonMark/GFM code boundaries around opaque Rust slots.

use std::{borrow::Cow, ops::Range};

use pulldown_cmark::{Event, Parser, Tag};

pub(super) struct CodeContexts<'a> {
    source: Cow<'a, str>,
    ranges: Vec<Range<usize>>,
    next: usize,
}

impl<'a> CodeContexts<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            source: Cow::Borrowed(source),
            ranges: code_ranges(source),
            next: 0,
        }
    }

    pub(super) fn end_at(&mut self, position: usize) -> Option<usize> {
        while self
            .ranges
            .get(self.next)
            .is_some_and(|range| range.end <= position)
        {
            self.next += 1;
        }
        self.ranges
            .get(self.next)
            .filter(|range| range.start <= position)
            .map(|range| range.end)
    }

    pub(super) fn exclude_slot(&mut self, range: Range<usize>) {
        // Scalar references cannot open Markdown syntax. Keep the common case
        // linear even for documents with thousands of placeholders.
        if self.source[range.clone()]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"{}_ .:&".contains(&byte))
        {
            return;
        }
        // A slot is one opaque inline value. Its Rust strings, comments and
        // layout must not open Markdown blocks or delimiters outside the slot.
        // Preserve both UTF-8 and UTF-16 widths, so the compiler and editor
        // feed equivalent text to their Markdown parsers without moving spans.
        let end = range.end;
        let mask: String = self.source[range.clone()]
            .chars()
            .map(|ch| match ch.len_utf8() {
                1 => 'x',
                2 => 'é',
                3 => '中',
                _ => '🦀',
            })
            .collect();
        self.source.to_mut().replace_range(range, &mask);
        self.ranges = code_ranges(&self.source);
        // Parsing proceeds left to right: a newly discovered span may not
        // reclassify an already consumed Rust slot as Markdown code.
        self.next = self.ranges.partition_point(|range| range.start < end);
    }
}

fn code_ranges(source: &str) -> Vec<Range<usize>> {
    // A one-column table can omit pipes if its delimiter has an alignment colon.
    if !source.contains(['|', ':']) {
        return tab_code_ranges(source);
    }
    // Comrak follows cmark's table/block grammar, including failed header
    // attempts and indented continuations. Its inline source columns drift
    // after escaped table pipes and link definitions, so use pulldown-cmark
    // for byte-accurate inline ranges, independently within each table cell.
    let mut options = comrak::Options::default();
    options.extension.table = true;
    let arena = comrak::Arena::new();
    let tree = comrak::parse_document(&arena, source, &options);
    let mut lines = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' || (byte == b'\r' && source.as_bytes().get(index + 1) != Some(&b'\n')) {
            lines.push(index + 1);
        }
    }
    let mut breaks = Vec::new();
    let mut tables = Vec::new();
    let mut cells = Vec::new();
    for node in tree.descendants() {
        let node = node.data.borrow();
        match node.value {
            comrak::nodes::NodeValue::Table(_) => {
                tables.push(
                    (lines[node.sourcepos.start.line - 1] + node.sourcepos.start.column - 1)
                        ..(lines[node.sourcepos.end.line - 1] + node.sourcepos.end.column),
                );
                breaks.push(lines[node.sourcepos.start.line - 1]);
                if let Some(end) = lines.get(node.sourcepos.end.line) {
                    breaks.push(*end);
                }
            }
            comrak::nodes::NodeValue::TableRow(_) => {
                let start = lines[node.sourcepos.start.line - 1] + node.sourcepos.start.column - 1;
                let end = lines[node.sourcepos.end.line - 1] + node.sourcepos.end.column;
                // Read authored cells, including extra cells a renderer may
                // discard. Autocompleted cells have no source, and Comrak's
                // inline columns shift after escaped pipes. GFM only splits
                // pipes not immediately preceded by a backslash.
                let mut cell = start;
                for offset in start..end {
                    if source.as_bytes()[offset] == b'|'
                        && (offset == start || source.as_bytes()[offset - 1] != b'\\')
                    {
                        if cell < offset {
                            cells.push(cell..offset);
                        }
                        cell = offset + 1;
                    }
                }
                if cell < end {
                    cells.push(cell..end);
                }
            }
            _ => {}
        }
    }
    if tables.is_empty() {
        return tab_code_ranges(source);
    }
    // Explicit block boundaries prevent the CommonMark-only offset parser
    // from continuing a paragraph or code span across an extracted table.
    breaks.sort_unstable();
    breaks.dedup();
    breaks.retain(|position| *position > 0 && *position < source.len());
    let mut view = String::with_capacity(source.len() + breaks.len());
    let mut previous = 0;
    for &position in &breaks {
        view.push_str(&source[previous..position]);
        view.push('\n');
        previous = position;
    }
    view.push_str(&source[previous..]);
    let inserted: Vec<_> = breaks
        .iter()
        .enumerate()
        .map(|(index, position)| index + position)
        .collect();
    let original = |offset| offset - inserted.partition_point(|position| *position < offset);
    let mut ranges: Vec<_> = tab_code_ranges(&view)
        .into_iter()
        .map(|range| original(range.start)..original(range.end))
        .filter(|range| {
            let next = tables.partition_point(|table| table.end <= range.start);
            !tables
                .get(next)
                .is_some_and(|table| table.start < range.end)
        })
        .collect();
    for cell in cells {
        // Cells contain inline Markdown, even if their text starts with a
        // heading, fence, or list marker. A plain prefix prevents block parsing.
        let text = format!("x {}", &source[cell.clone()]);
        ranges.extend(
            raw_code_ranges(&text)
                .into_iter()
                .map(|range| (cell.start + range.start - 2)..(cell.start + range.end - 2)),
        );
    }
    ranges.sort_unstable_by_key(|range| range.start);
    ranges
}

fn tab_code_ranges(source: &str) -> Vec<Range<usize>> {
    if !source.contains('\t') {
        return raw_code_ranges(source);
    }
    // pulldown-cmark 0.13 mishandles leading tabs after some quote boundaries
    // and tabs after closing fences. Expand indentation to CommonMark's four
    // column tab stops, and treat trailing tabs like spaces, in this view only.
    // Keep a boundary map because one tab can become several parser bytes.
    let mut normalized = String::with_capacity(source.len());
    let mut boundaries = vec![0];
    let mut line_start = 0;
    for line in source.split_inclusive(['\n', '\r']) {
        let leading = line
            .bytes()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
        let trailing = line.trim_end_matches([' ', '\t', '\r', '\n']).len();
        let mut column = 0;
        for (offset, ch) in line.char_indices() {
            let original = line_start + offset;
            if ch == '\t' && offset < leading {
                let width = 4 - column % 4;
                for n in 0..width {
                    normalized.push(' ');
                    boundaries.push(if n + 1 == width {
                        original + 1
                    } else {
                        original
                    });
                }
                column += width;
            } else {
                normalized.push(if ch == '\t' && offset >= trailing {
                    ' '
                } else {
                    ch
                });
                boundaries.extend(original + 1..=original + ch.len_utf8());
                column += 1;
            }
        }
        line_start += line.len();
    }
    raw_code_ranges(&normalized)
        .into_iter()
        .map(|range| boundaries[range.start]..boundaries[range.end])
        .collect()
}

fn raw_code_ranges(source: &str) -> Vec<Range<usize>> {
    Parser::new(source)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))).then_some(range)
        })
        .collect()
}
