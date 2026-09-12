// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::markdown::{escape_attr, escape_text};

const DEFAULT_HEIGHT: u32 = 320;
const MIN_HEIGHT: u32 = 80;
const MAX_HEIGHT: u32 = 1600;

/// A same-origin live demo embedded in an `<iframe>`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Demo {
    src: String,
    title: String,
    height: u32,
}

impl Demo {
    pub(super) fn new(src: String, title: String, height: Option<&str>) -> Self {
        let height = height
            .and_then(|raw| raw.parse::<u32>().ok())
            .unwrap_or(DEFAULT_HEIGHT)
            .clamp(MIN_HEIGHT, MAX_HEIGHT);
        Self { src, title, height }
    }

    pub(super) fn src(&self) -> &str {
        &self.src
    }

    pub(super) fn invalid_reason(&self) -> Option<&'static str> {
        if !self.src.starts_with('/') {
            Some("src must be a same-origin path starting with `/`")
        } else if self.title.is_empty() {
            Some("title is required so the frame is labelled for assistive technology")
        } else {
            None
        }
    }

    // proa-perf-ok(scope): docs demos are lowered once at build time into trusted static HTML; authored URLs and labels are escaped before insertion
    pub(super) fn emit_html(&self, html: &mut String) {
        html.push_str("<div class=\"proa-docs-demo\" data-proa-docs-demo><iframe class=\"proa-docs-demo-frame\" src=\"");
        escape_attr(&self.src, html);
        html.push_str("\" title=\"");
        escape_attr(&self.title, html);
        let _ = write!(
            html,
            "\" height=\"{height}\" loading=\"lazy\"></iframe>",
            height = self.height
        );
        html.push_str("<a class=\"proa-docs-demo-link\" href=\"");
        escape_attr(&self.src, html);
        html.push_str("\" target=\"_blank\" rel=\"noopener\">Open ");
        escape_text(&self.title, html);
        html.push_str(" in a new tab</a></div>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_titled_frame_and_plain_link() {
        let demo = Demo::new(
            "/demos/quantity-stepper".into(),
            "Quantity stepper".into(),
            Some("260"),
        );
        let mut html = String::new();
        demo.emit_html(&mut html);

        assert!(html.contains("src=\"/demos/quantity-stepper\""));
        assert!(html.contains("title=\"Quantity stepper\""));
        assert!(html.contains("height=\"260\""));
        assert!(html.contains("loading=\"lazy\""));
        assert!(html.contains("<a class=\"proa-docs-demo-link\""));
    }

    #[test]
    fn height_falls_back_and_clamps() {
        assert_eq!(
            Demo::new("/d".into(), "T".into(), None).height,
            DEFAULT_HEIGHT
        );
        assert_eq!(
            Demo::new("/d".into(), "T".into(), Some("99999")).height,
            MAX_HEIGHT
        );
        assert_eq!(
            Demo::new("/d".into(), "T".into(), Some("1")).height,
            MIN_HEIGHT
        );
    }

    #[test]
    fn validates_origin_and_title() {
        let external = Demo::new("https://example.com".into(), "External".into(), None);
        assert!(external.invalid_reason().unwrap().contains("same-origin"));
        let untitled = Demo::new("/demo".into(), String::new(), None);
        assert!(untitled.invalid_reason().unwrap().contains("title"));
    }
}
