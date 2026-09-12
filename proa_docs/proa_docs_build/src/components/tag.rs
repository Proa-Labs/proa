// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::{attr_value, callout::CalloutKind, demo::Demo};

/// Parsed command for one supported MDX component tag.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Tag {
    TabsOpen(Vec<String>),
    TabsClose,
    TabOpen(Option<String>),
    TabClose,
    ExplorerOpen,
    ExplorerClose,
    CodeFileOpen(String),
    CodeFileClose,
    AccordionsOpen {
        exclusive: bool,
    },
    AccordionsClose,
    AccordionOpen {
        title: String,
        id: Option<String>,
    },
    AccordionClose,
    StepsOpen,
    StepsClose,
    StepOpen,
    StepClose,
    CalloutOpen {
        kind: CalloutKind,
        emoji: Option<String>,
    },
    CalloutClose,
    Demo(Demo),
}

impl Tag {
    pub(super) fn parse(trimmed: &str) -> Option<Self> {
        if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
            return None;
        }

        let tag = match trimmed {
            "</Tabs>" => Self::TabsClose,
            "</Tab>" => Self::TabClose,
            "<CodeExplorer>" => Self::ExplorerOpen,
            "</CodeExplorer>" => Self::ExplorerClose,
            "</CodeFile>" => Self::CodeFileClose,
            "</Accordions>" => Self::AccordionsClose,
            "</Accordion>" => Self::AccordionClose,
            "</Steps>" => Self::StepsClose,
            "</Step>" => Self::StepClose,
            "</Callout>" => Self::CalloutClose,
            "<Accordions>" => Self::AccordionsOpen { exclusive: true },
            "<Steps>" => Self::StepsOpen,
            "<Step>" => Self::StepOpen,
            "<Callout>" => Self::CalloutOpen {
                kind: CalloutKind::Default,
                emoji: None,
            },
            _ => return Self::parse_with_attrs(trimmed),
        };
        Some(tag)
    }

    fn parse_with_attrs(trimmed: &str) -> Option<Self> {
        if let Some(rest) = open_tag_body(trimmed, "Tabs") {
            let labels = attr_value(rest, "data-items")
                .map(|raw| split_items(&raw))
                .unwrap_or_default();
            return Some(Self::TabsOpen(labels));
        }
        if let Some(rest) = open_tag_body(trimmed, "Tab") {
            return Some(Self::TabOpen(attr_value(rest, "value")));
        }
        if open_tag_body(trimmed, "CodeExplorer").is_some() {
            return Some(Self::ExplorerOpen);
        }
        if let Some(rest) = open_tag_body(trimmed, "CodeFile") {
            let name = attr_value(rest, "name")?;
            if name.is_empty() {
                return None;
            }
            return Some(Self::CodeFileOpen(name));
        }
        if let Some(rest) = open_tag_body(trimmed, "Accordions") {
            let exclusive = attr_value(rest, "type").as_deref() != Some("multiple");
            return Some(Self::AccordionsOpen { exclusive });
        }
        if let Some(rest) = open_tag_body(trimmed, "Accordion") {
            return Some(Self::AccordionOpen {
                title: attr_value(rest, "title").unwrap_or_default(),
                id: attr_value(rest, "id"),
            });
        }
        if open_tag_body(trimmed, "Steps").is_some() {
            return Some(Self::StepsOpen);
        }
        if open_tag_body(trimmed, "Step").is_some() {
            return Some(Self::StepOpen);
        }
        if let Some(rest) = open_tag_body(trimmed, "Callout") {
            return Some(Self::CalloutOpen {
                kind: CalloutKind::parse(attr_value(rest, "type").as_deref()),
                emoji: attr_value(rest, "emoji"),
            });
        }
        if let Some(rest) = open_tag_body(trimmed, "Demo") {
            let src = attr_value(rest, "src").unwrap_or_default();
            let title = attr_value(rest, "title").unwrap_or_default();
            let height = attr_value(rest, "height");
            return Some(Self::Demo(Demo::new(src, title, height.as_deref())));
        }
        None
    }
}

/// Match `<Name ...>` exactly, so `<Tabs>` never matches `<TabsFoo>`.
fn open_tag_body<'a>(trimmed: &'a str, name: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix('<')?.strip_prefix(name)?;
    let rest = rest.strip_suffix('>')?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Some(rest);
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest)
}

fn split_items(raw: &str) -> Vec<String> {
    raw.split('|')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Rewrite `items={['a', 'b']}` into a CommonMark-compatible attribute.
pub(crate) fn normalize_tabs_items(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with("<Tabs") {
        return None;
    }
    let start = trimmed.find("items=")?;
    let after = &trimmed[start + "items=".len()..];
    let after = after.strip_prefix('{')?;
    let close = after.find('}')?;
    let array = after[..close].trim();
    let array = array.strip_prefix('[')?.strip_suffix(']')?;

    let items: Vec<String> = array
        .split(',')
        .map(|item| {
            item.trim()
                .trim_matches(|c| c == '\'' || c == '"')
                .replace('|', "/")
                .trim()
                .to_string()
        })
        .filter(|item| !item.is_empty())
        .collect();

    let mut out = String::from("<Tabs data-items=\"");
    out.push_str(&items.join("|"));
    out.push('"');
    let tail = after[close + 1..].trim_end_matches('>').trim();
    if !tail.is_empty() {
        out.push(' ');
        out.push_str(tail);
    }
    out.push('>');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_authored_items_array() {
        assert_eq!(
            normalize_tabs_items("<Tabs items={['macOS and Linux', 'Windows']}>").unwrap(),
            "<Tabs data-items=\"macOS and Linux|Windows\">"
        );
    }

    #[test]
    fn normalization_preserves_trailing_attributes() {
        assert_eq!(
            normalize_tabs_items("<Tabs items={['a', 'b']} groupId=\"pkg\">").unwrap(),
            "<Tabs data-items=\"a|b\" groupId=\"pkg\">"
        );
    }

    #[test]
    fn parses_component_tags_without_claiming_lookalikes() {
        assert_eq!(Tag::parse("<CodeExplorer>"), Some(Tag::ExplorerOpen));
        assert_eq!(
            Tag::parse("<CodeFile name=\"src/main.rs\">"),
            Some(Tag::CodeFileOpen("src/main.rs".into()))
        );
        assert_eq!(Tag::parse("<Tabsy>"), None);
        assert_eq!(Tag::parse("<TabsFoo items=\"a\">"), None);
    }
}
