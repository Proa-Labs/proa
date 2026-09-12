// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use super::attr_value;

/// Build-time lowerer for an authored `<Cards>` collection.
#[derive(Debug, Default)]
pub(crate) struct Cards {
    items: Vec<Card>,
}

impl Cards {
    pub(crate) fn push(&mut self, raw: &str) -> Result<(), String> {
        self.items.push(Card::parse(raw)?);
        Ok(())
    }

    pub(crate) fn emit_markdown(self, out: &mut String) {
        out.push('\n');
        for card in self.items {
            card.emit_markdown(out);
        }
        out.push('\n');
    }
}

/// One authored `<Card />` item.
#[derive(Debug)]
struct Card {
    title: String,
    description: Option<String>,
    href: String,
}

impl Card {
    // proa-perf-ok(scope): build-time MDX lowering reports unsupported authored Card markup; this is diagnostic text, not rendered HTML
    fn parse(raw: &str) -> Result<Self, String> {
        if !raw.starts_with("<Card ") || !raw.ends_with("/>") {
            return Err(format!("unsupported <Cards> child: {raw}"));
        }
        Ok(Self {
            title: attr_value(raw, "title").ok_or_else(|| format!("missing Card title: {raw}"))?,
            description: attr_value(raw, "description"),
            href: attr_value(raw, "href").ok_or_else(|| format!("missing Card href: {raw}"))?,
        })
    }

    fn emit_markdown(self, out: &mut String) {
        let _ = write!(
            out,
            "- [{title}]({href})",
            title = self.title,
            href = self.href
        );
        if let Some(description) = self.description {
            let _ = write!(out, ": {description}");
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowers_cards_to_linked_markdown_items() {
        let mut cards = Cards::default();
        cards
            .push(r#"<Card title="Runtime" description="Render it." href="/runtime" />"#)
            .unwrap();
        let mut out = String::new();
        cards.emit_markdown(&mut out);
        assert_eq!(out, "\n- [Runtime](/runtime): Render it.\n\n");
    }

    #[test]
    fn rejects_unsupported_children() {
        let mut cards = Cards::default();
        assert!(cards.push("<Button />").is_err());
        assert!(cards.push(r#"<Card href="/missing-title" />"#).is_err());
    }
}
