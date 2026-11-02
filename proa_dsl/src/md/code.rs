// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! CommonMark code boundaries around opaque Rust slots.

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
