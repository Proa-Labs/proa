// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::markdown::{escape_attr, escape_text};

/// Build-time state and renderer for one `<Tabs>` container.
#[derive(Debug)]
pub(super) struct Tabs {
    labels: Vec<String>,
    panel_index: usize,
}

impl Tabs {
    /// Render the tab controls and return state for the panels that follow.
    // proa-perf-ok(scope): docs tabs are lowered once at build time into trusted static HTML; authored labels are escaped before insertion
    pub(super) fn open(group: usize, labels: Vec<String>, html: &mut String) -> Self {
        html.push_str("<div class=\"proa-docs-tabs\" data-proa-docs-tabs>");
        for (index, _) in labels.iter().enumerate() {
            let _ = write!(
                html,
                "<input class=\"proa-docs-tabs-radio\" type=\"radio\" name=\"proa-docs-tabs-{group}\" id=\"proa-docs-tabs-{group}-{index}\""
            );
            if index == 0 {
                html.push_str(" checked");
            }
            html.push('>');
        }
        html.push_str("<div class=\"proa-docs-tabs-list\" role=\"tablist\">");
        for (index, label) in labels.iter().enumerate() {
            let _ = write!(
                html,
                "<label class=\"proa-docs-tabs-tab\" for=\"proa-docs-tabs-{group}-{index}\">"
            );
            escape_text(label, html);
            html.push_str("</label>");
        }
        html.push_str("</div>");

        Self {
            labels,
            panel_index: 0,
        }
    }

    pub(super) fn open_panel(&mut self, value: Option<&str>, html: &mut String) {
        if self.panel_index == 0 {
            html.push_str("<div class=\"proa-docs-tabs-panels\">");
        }
        let index = self.panel_index;
        self.panel_index += 1;
        let label = value
            .or_else(|| self.labels.get(index).map(String::as_str))
            .unwrap_or_default();
        Tab { index, label }.open(html);
    }

    pub(super) fn close_panel(&self, html: &mut String) {
        Tab::close(html);
    }

    pub(super) fn finish(self, html: &mut String) {
        if self.panel_index > 0 {
            html.push_str("</div>");
        }
        html.push_str("</div>");
    }
}

/// Build-time renderer for one `<Tab>` panel.
struct Tab<'a> {
    index: usize,
    label: &'a str,
}

impl Tab<'_> {
    fn open(self, html: &mut String) {
        let _ = write!(
            html,
            "<div class=\"proa-docs-tabs-panel\" role=\"tabpanel\" data-index=\"{index}\" aria-label=\"",
            index = self.index
        );
        escape_attr(self.label, html);
        html.push_str("\">");
    }

    fn close(html: &mut String) {
        html.push_str("</div>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_controls_and_panels() {
        let mut html = String::new();
        let mut tabs = Tabs::open(0, vec!["One".into(), "Two".into()], &mut html);
        assert!(html.contains("id=\"proa-docs-tabs-0-0\" checked"));
        assert!(html.contains("for=\"proa-docs-tabs-0-1\">Two</label>"));

        tabs.open_panel(None, &mut html);
        assert!(html.contains("aria-label=\"One\""));
        tabs.close_panel(&mut html);
        tabs.open_panel(Some("Second"), &mut html);
        tabs.close_panel(&mut html);
        tabs.finish(&mut html);

        assert!(html.contains("data-index=\"1\" aria-label=\"Second\""));
        assert_eq!(html.matches("<div").count(), html.matches("</div>").count());
    }
}
