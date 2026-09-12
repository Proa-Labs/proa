// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::markdown::{escape_attr, escape_text};

/// Per-page state and renderer for `<Accordions>` groups.
#[derive(Debug, Default)]
pub(super) struct Accordions {
    depth: usize,
    groups_seen: usize,
    active_group: Option<String>,
}

impl Accordions {
    pub(super) fn open(&mut self, exclusive: bool, html: &mut String) {
        let group = self.groups_seen;
        self.groups_seen += 1;
        self.depth += 1;
        // `type="single"` maps to native disclosure grouping, so opening one
        // panel closes its sibling with no client-side script.
        self.active_group = exclusive.then(|| format!("proa-docs-accordion-group-{group}"));
        html.push_str("<div class=\"proa-docs-accordions\">");
    }

    pub(super) fn close(&mut self, html: &mut String) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        self.active_group = None;
        html.push_str("</div>");
        true
    }

    pub(super) fn item<'a>(&'a self, title: &'a str, id: Option<&'a str>) -> Accordion<'a> {
        Accordion {
            group: self.active_group.as_deref(),
            title,
            id,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.depth > 0
    }
}

/// Build-time renderer for one `<Accordion>` item.
pub(super) struct Accordion<'a> {
    group: Option<&'a str>,
    title: &'a str,
    id: Option<&'a str>,
}

impl Accordion<'_> {
    pub(super) fn open(self, html: &mut String) {
        html.push_str("<details class=\"proa-docs-accordion\"");
        if let Some(group) = self.group {
            html.push_str(" name=\"");
            escape_attr(group, html);
            html.push('"');
        }
        if let Some(id) = self.id {
            html.push_str(" id=\"");
            escape_attr(id, html);
            html.push('"');
        }
        html.push_str("><summary class=\"proa-docs-accordion-title\">");
        escape_text(self.title, html);
        html.push_str("</summary><div class=\"proa-docs-accordion-body\">");
    }

    pub(super) fn close(html: &mut String) {
        html.push_str("</div></details>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_groups_share_a_native_name() {
        let mut accordions = Accordions::default();
        let mut html = String::new();
        accordions.open(true, &mut html);
        accordions.item("One", None).open(&mut html);
        Accordion::close(&mut html);
        accordions.item("Two", Some("two")).open(&mut html);

        assert_eq!(
            html.matches("name=\"proa-docs-accordion-group-0\"").count(),
            2
        );
        assert!(html.contains("id=\"two\""));
    }

    #[test]
    fn multiple_groups_and_standalone_items_have_no_name() {
        let mut accordions = Accordions::default();
        let mut html = String::new();
        accordions.open(false, &mut html);
        accordions.item("One", None).open(&mut html);
        assert!(!html.contains(" name="));

        let mut html = String::new();
        Accordions::default()
            .item("Standalone", None)
            .open(&mut html);
        assert!(!html.contains(" name="));
    }

    #[test]
    fn titles_are_escaped() {
        let mut html = String::new();
        Accordions::default().item("<script>", None).open(&mut html);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
