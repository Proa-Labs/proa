// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Build-time renderers and lowerers for authored docs components.
//!
//! Container tags survive MDX lowering so their arbitrary Markdown children
//! continue through the normal Markdown renderer. Line-oriented components are
//! lowered to Markdown earlier, while fenced code and deferred TypeTable
//! directives are rendered later. Each component owns its state and output in
//! a focused submodule; this module only exposes dispatch and shared page state.

mod accordions;
mod agent_terminal;
mod attributes;
mod callout;
mod cards;
mod code_block;
mod code_explorer;
mod demo;
mod files;
mod language_icon;
mod steps;
mod tabs;
mod tag;
mod type_table;

use accordions::{Accordion, Accordions};
use callout::Callout;
use code_explorer::CodeExplorer;
use demo::Demo;
use language_icon::LanguageIcon;
use steps::{Step, Steps};
use tabs::Tabs;
use tag::Tag;

pub(crate) use tag::normalize_tabs_items;
pub(crate) use {
    agent_terminal::AgentTerminal,
    attributes::value as attr_value,
    cards::Cards,
    code_block::CodeBlock,
    files::Files,
    type_table::{parse_type_table, render_type_table_placeholder},
};

/// Highest tab count addressed by the generated `nth-of-type` rules.
///
/// Extra panels are still emitted, but the stylesheet cannot switch between
/// them, so the build reports the authoring mistake.
pub(super) const MAX_STYLED_TABS: usize = 8;

/// Per-page state for MDX container components.
#[derive(Debug, Default)]
pub(crate) struct ComponentState {
    tab_groups_seen: usize,
    open_tabs: Option<Tabs>,
    open_explorer: Option<CodeExplorer>,
    steps: Steps,
    accordions: Accordions,
    overflowing_tabs: Vec<usize>,
    overflowing_explorers: Vec<usize>,
    invalid_demos: Vec<Demo>,
}

impl ComponentState {
    /// Tab blocks on this page that declared more tabs than the stylesheet can
    /// switch between.
    pub(crate) fn overflowing_tabs(&self) -> &[usize] {
        &self.overflowing_tabs
    }

    /// Explorers on this page that listed more files than the stylesheet can
    /// switch between.
    pub(crate) fn overflowing_explorers(&self) -> &[usize] {
        &self.overflowing_explorers
    }

    /// Invalid `<Demo>` tags this page dropped, as `(src, reason)` pairs.
    pub(crate) fn invalid_demos(&self) -> impl Iterator<Item = (&str, &'static str)> {
        self.invalid_demos.iter().map(|demo| {
            (
                demo.src(),
                demo.invalid_reason().expect("stored demos are invalid"),
            )
        })
    }

    /// Returns the name of a container that was opened but never closed.
    pub(crate) fn unclosed(&self) -> Option<&'static str> {
        if self.open_tabs.is_some() {
            return Some("Tabs");
        }
        if self.open_explorer.is_some() {
            return Some("CodeExplorer");
        }
        if self.steps.is_open() {
            return Some("Steps");
        }
        if self.accordions.is_open() {
            return Some("Accordions");
        }
        None
    }

    fn next_tab_group(&mut self) -> usize {
        let group = self.tab_groups_seen;
        self.tab_groups_seen += 1;
        group
    }
}

/// Whether a line is a container-component tag that survives MDX lowering and
/// is intercepted as a raw-HTML event by the Markdown renderer.
///
/// The lowering pass uses this to keep each tag in its own block, so the
/// children between an open and close tag are parsed as Markdown instead of
/// being swallowed by the tag's HTML block.
pub(crate) fn is_container_tag(trimmed: &str) -> bool {
    Tag::parse(trimmed).is_some()
}

/// Try to render `raw` as a supported MDX container-component tag.
///
/// Returns `false` when the tag is unknown or invalid in the current context,
/// allowing the Markdown renderer to apply its normal raw-HTML behavior.
// proa-perf-ok(scope): docs component tags are lowered once at build time into trusted static HTML; authored labels and attributes are escaped before insertion
pub(crate) fn render_tag(raw: &str, state: &mut ComponentState, html: &mut String) -> bool {
    let Some(tag) = Tag::parse(raw.trim()) else {
        return false;
    };

    match tag {
        Tag::TabsOpen(labels) => {
            let group = state.next_tab_group();
            if labels.len() > MAX_STYLED_TABS {
                state.overflowing_tabs.push(labels.len());
            }
            state.open_tabs = Some(Tabs::open(group, labels, html));
        }
        Tag::TabsClose => {
            if let Some(tabs) = state.open_tabs.take() {
                tabs.finish(html);
            }
        }
        Tag::TabOpen(value) => {
            let Some(tabs) = state.open_tabs.as_mut() else {
                return false;
            };
            tabs.open_panel(value.as_deref(), html);
        }
        Tag::TabClose => {
            let Some(tabs) = state.open_tabs.as_ref() else {
                return false;
            };
            tabs.close_panel(html);
        }
        Tag::ExplorerOpen => {
            let group = state.next_tab_group();
            state.open_explorer = Some(CodeExplorer::open(group, html));
        }
        Tag::ExplorerClose => {
            let Some(explorer) = state.open_explorer.take() else {
                return false;
            };
            let file_count = explorer.finish(html);
            if file_count > MAX_STYLED_TABS {
                state.overflowing_explorers.push(file_count);
            }
        }
        Tag::CodeFileOpen(name) => {
            let Some(explorer) = state.open_explorer.as_mut() else {
                return false;
            };
            explorer.open_file(name, html);
        }
        Tag::CodeFileClose => {
            let Some(explorer) = state.open_explorer.as_ref() else {
                return false;
            };
            explorer.close_file(html);
        }
        Tag::AccordionsOpen { exclusive } => state.accordions.open(exclusive, html),
        Tag::AccordionsClose => {
            if !state.accordions.close(html) {
                return false;
            }
        }
        Tag::AccordionOpen { title, id } => {
            state.accordions.item(&title, id.as_deref()).open(html);
        }
        Tag::AccordionClose => Accordion::close(html),
        Tag::StepsOpen => state.steps.open(html),
        Tag::StepsClose => {
            if !state.steps.close(html) {
                return false;
            }
        }
        Tag::StepOpen => Step::open(html),
        Tag::StepClose => Step::close(html),
        Tag::CalloutOpen { kind, emoji } => Callout::new(kind, emoji.as_deref()).open(html),
        Tag::CalloutClose => Callout::close(html),
        Tag::Demo(demo) => {
            if demo.invalid_reason().is_some() {
                state.invalid_demos.push(demo);
                return false;
            }
            demo.emit_html(html);
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(raw: &str, state: &mut ComponentState) -> String {
        let mut html = String::new();
        assert!(render_tag(raw, state, &mut html), "{raw} should be a tag");
        html
    }

    #[test]
    fn callout_defaults_to_the_neutral_type() {
        let mut state = ComponentState::default();
        let html = render("<Callout>", &mut state);
        assert!(html.contains("class=\"proa-docs-callout\""), "{html}");
        assert!(html.contains("data-type=\"default\""), "{html}");
        assert!(html.contains("role=\"note\""), "{html}");
        // The icon repeats what the prose says, so it is decorative.
        assert!(html.contains("aria-hidden=\"true\""), "{html}");
        assert!(html.contains("<svg"), "{html}");
    }

    #[test]
    fn callout_types_reach_the_data_attribute() {
        for kind in ["info", "warning", "error"] {
            let mut state = ComponentState::default();
            let html = render(&format!("<Callout type=\"{kind}\">"), &mut state);
            assert!(html.contains(&format!("data-type=\"{kind}\"")), "{html}");
        }
    }

    #[test]
    fn callout_falls_back_to_the_neutral_type_for_an_unknown_value() {
        let mut state = ComponentState::default();
        let html = render("<Callout type=\"nope\">", &mut state);
        assert!(html.contains("data-type=\"default\""), "{html}");
    }

    #[test]
    fn callout_emoji_replaces_the_built_in_icon() {
        let mut state = ComponentState::default();
        let html = render("<Callout type=\"warning\" emoji=\"🔥\">", &mut state);
        assert!(html.contains('🔥'), "{html}");
        assert!(
            !html.contains("<svg"),
            "built-in icon should be replaced: {html}"
        );
    }

    #[test]
    fn callout_emoji_is_escaped() {
        let mut state = ComponentState::default();
        let html = render("<Callout emoji=\"<script>\">", &mut state);
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn callout_close_balances_both_columns() {
        let mut state = ComponentState::default();
        let mut html = render("<Callout>", &mut state);
        html.push_str(&render("</Callout>", &mut state));
        assert_eq!(
            html.matches("<div").count(),
            html.matches("</div>").count(),
            "unbalanced markup: {html}"
        );
    }

    #[test]
    fn tabs_and_explorers_share_unique_radio_groups() {
        let mut state = ComponentState::default();
        let mut html = String::new();

        render_tag("<Tabs data-items=\"a\">", &mut state, &mut html);
        render_tag("</Tabs>", &mut state, &mut html);
        let explorer_start = html.len();
        render_tag("<CodeExplorer>", &mut state, &mut html);
        render_tag("<CodeFile name=\"a.rs\">", &mut state, &mut html);
        render_tag("</CodeFile>", &mut state, &mut html);
        render_tag("</CodeExplorer>", &mut state, &mut html);

        assert!(html[..explorer_start].contains("name=\"proa-docs-tabs-0\""));
        assert!(html[explorer_start..].contains("name=\"proa-docs-tabs-1\""));
    }

    #[test]
    fn context_dependent_tags_are_not_claimed_outside_their_container() {
        let mut state = ComponentState::default();
        let mut html = String::new();

        assert!(!render_tag("<Tab>", &mut state, &mut html));
        assert!(!render_tag("</Tab>", &mut state, &mut html));
        assert!(!render_tag(
            "<CodeFile name=\"a.rs\">",
            &mut state,
            &mut html
        ));
        assert!(!render_tag("</CodeFile>", &mut state, &mut html));
        assert!(!render_tag("</CodeExplorer>", &mut state, &mut html));
        assert!(html.is_empty());
    }

    #[test]
    fn unknown_and_lookalike_tags_are_not_claimed() {
        let mut state = ComponentState::default();
        let mut html = String::new();
        for raw in ["<Tabsy>", "<div>", "<TabPanel>", "not a tag", "<Tabsy foo>"] {
            assert!(!render_tag(raw, &mut state, &mut html), "{raw}");
        }
        assert!(html.is_empty());
    }

    #[test]
    fn unclosed_containers_are_reported() {
        let mut state = ComponentState::default();
        render("<Tabs data-items=\"a\">", &mut state);
        assert_eq!(state.unclosed(), Some("Tabs"));

        let mut state = ComponentState::default();
        render("<CodeExplorer>", &mut state);
        assert_eq!(state.unclosed(), Some("CodeExplorer"));

        let mut state = ComponentState::default();
        render("<Steps>", &mut state);
        assert_eq!(state.unclosed(), Some("Steps"));

        let mut state = ComponentState::default();
        render("<Accordions>", &mut state);
        assert_eq!(state.unclosed(), Some("Accordions"));
    }

    #[test]
    fn overflowing_component_counts_are_reported() {
        let mut state = ComponentState::default();
        render("<Tabs data-items=\"1|2|3|4|5|6|7|8|9\">", &mut state);
        assert_eq!(state.overflowing_tabs(), &[MAX_STYLED_TABS + 1]);

        let mut html = String::new();
        render_tag("<CodeExplorer>", &mut state, &mut html);
        for index in 0..=MAX_STYLED_TABS {
            // proa-perf-ok: test fixture constructs authored component syntax, not response HTML
            let file = format!("<CodeFile name=\"f{index}.rs\">");
            render_tag(&file, &mut state, &mut html);
            render_tag("</CodeFile>", &mut state, &mut html);
        }
        render_tag("</CodeExplorer>", &mut state, &mut html);
        assert_eq!(state.overflowing_explorers(), &[MAX_STYLED_TABS + 1]);
    }

    #[test]
    fn invalid_demos_are_dropped_and_reported() {
        let mut state = ComponentState::default();
        let mut html = String::new();
        assert!(!render_tag(
            "<Demo src=\"https://example.com/x\" title=\"External\" />",
            &mut state,
            &mut html
        ));
        assert!(!render_tag(
            "<Demo src=\"/demos/x\" />",
            &mut state,
            &mut html
        ));

        let reported: Vec<_> = state.invalid_demos().collect();
        assert_eq!(reported.len(), 2);
        assert!(reported[0].1.contains("same-origin"));
        assert!(reported[1].1.contains("title"));
        assert!(html.is_empty());
    }
}
