// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Mirrors Proa representation negotiation for routes whose alternate URL is dynamic.
use axum::http::{header::ACCEPT, HeaderMap};

pub fn prefers_markdown(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(ACCEPT).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let ranges: Vec<MediaRange<'_>> = value.split(',').filter_map(MediaRange::parse).collect();
    let explicit = ranges.iter().any(|range| {
        range.ty.eq_ignore_ascii_case("text")
            && range.subtype.eq_ignore_ascii_case("markdown")
            && range.quality > 0
    });
    if !explicit {
        return false;
    }
    let markdown = quality_for(&ranges, "text", "markdown");
    let html = quality_for(&ranges, "text", "html");
    markdown > 0 && markdown > html
}

#[derive(Debug, Clone, Copy)]
struct MediaRange<'a> {
    ty: &'a str,
    subtype: &'a str,
    quality: u16,
}

impl<'a> MediaRange<'a> {
    fn parse(raw: &'a str) -> Option<Self> {
        let mut parts = raw.trim().split(';');
        let media = parts.next()?.trim();
        let (ty, subtype) = media.split_once('/')?;
        if ty.trim().is_empty() || subtype.trim().is_empty() {
            return None;
        }
        let mut quality = 1_000;
        for parameter in parts {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("q") {
                quality = parse_quality(value.trim())?;
            }
        }
        Some(Self {
            ty: ty.trim(),
            subtype: subtype.trim(),
            quality,
        })
    }

    fn specificity(self, ty: &str, subtype: &str) -> Option<u8> {
        if self.ty == "*" && self.subtype == "*" {
            Some(0)
        } else if self.ty.eq_ignore_ascii_case(ty) && self.subtype == "*" {
            Some(1)
        } else if self.ty.eq_ignore_ascii_case(ty) && self.subtype.eq_ignore_ascii_case(subtype) {
            Some(2)
        } else {
            None
        }
    }
}

fn quality_for(ranges: &[MediaRange<'_>], ty: &str, subtype: &str) -> u16 {
    ranges
        .iter()
        .filter_map(|range| {
            range
                .specificity(ty, subtype)
                .map(|specificity| (specificity, range.quality))
        })
        .max_by_key(|(specificity, quality)| (*specificity, *quality))
        .map(|(_, quality)| quality)
        .unwrap_or(0)
}

fn parse_quality(raw: &str) -> Option<u16> {
    if raw == "0" {
        return Some(0);
    }
    if raw == "1" {
        return Some(1_000);
    }
    let (whole, fraction) = raw.split_once('.')?;
    if !matches!(whole, "0" | "1") || fraction.is_empty() || fraction.len() > 3 {
        return None;
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut scaled = fraction.parse::<u16>().ok()?;
    for _ in fraction.len()..3 {
        scaled *= 10;
    }
    if whole == "1" {
        (scaled == 0).then_some(1_000)
    } else {
        Some(scaled)
    }
}
