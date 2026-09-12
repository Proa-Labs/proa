// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::markdown::{escape_attr, escape_text};

/// The four callout kinds, each with its own colour treatment and icon.
///
/// The vocabulary matches the one docs authors already know from Nextra, so a
/// page ported from another site keeps its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum CalloutKind {
    #[default]
    Default,
    Info,
    Warning,
    Error,
}

impl CalloutKind {
    /// Parse the `type` attribute. An unknown value falls back to the neutral
    /// treatment rather than dropping the callout.
    pub(super) fn parse(value: Option<&str>) -> Self {
        match value {
            Some("info") => Self::Info,
            Some("warning") => Self::Warning,
            Some("error") => Self::Error,
            _ => Self::Default,
        }
    }

    /// Value written to `data-type`, which the stylesheet keys its colours on.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Built-in icon, used when the author supplies no `emoji`.
    ///
    /// These are inline SVG rather than emoji so they inherit `currentColor`
    /// and match the callout's text colour in both themes.
    const fn icon(self) -> &'static str {
        match self {
            // Lightbulb: a neutral aside.
            Self::Default => "<svg viewBox=\"0 0 16 16\" fill=\"currentColor\" width=\"1em\" height=\"1em\" aria-hidden=\"true\"><path d=\"M8 1.5a4.5 4.5 0 0 0-2.6 8.17c.35.25.6.62.6 1.04V11h4v-.29c0-.42.25-.79.6-1.04A4.5 4.5 0 0 0 8 1.5ZM4.53 10.9a6 6 0 1 1 6.94 0 .48.48 0 0 0-.22.35V12a1.5 1.5 0 0 1-1.5 1.5h-.06a1.75 1.75 0 0 1-3.38 0H6.25A1.5 1.5 0 0 1 4.75 12v-.75a.48.48 0 0 0-.22-.35Z\"/></svg>",
            // Info circle, the shape Nextra uses for `type=\"info\"`.
            Self::Info => "<svg viewBox=\"0 0 16 16\" fill=\"currentColor\" width=\"1em\" height=\"1em\" aria-hidden=\"true\"><path d=\"M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z\"/></svg>",
            // Warning triangle.
            Self::Warning => "<svg viewBox=\"0 0 16 16\" fill=\"currentColor\" width=\"1em\" height=\"1em\" aria-hidden=\"true\"><path d=\"M6.46 1.14a1.75 1.75 0 0 1 3.08 0l5.25 9.5A1.75 1.75 0 0 1 13.25 13.5H2.75A1.75 1.75 0 0 1 1.21 10.64ZM8.23 1.87a.25.25 0 0 0-.46 0l-5.25 9.5a.25.25 0 0 0 .23.38h10.5a.25.25 0 0 0 .23-.38ZM8 5a.75.75 0 0 1 .75.75v2.5a.75.75 0 0 1-1.5 0v-2.5A.75.75 0 0 1 8 5Zm0 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z\"/></svg>",
            // Filled circle with a cross: something is broken.
            Self::Error => "<svg viewBox=\"0 0 16 16\" fill=\"currentColor\" width=\"1em\" height=\"1em\" aria-hidden=\"true\"><path d=\"M8 0a8 8 0 1 1 0 16A8 8 0 0 1 8 0Zm2.78 4.16a.75.75 0 0 0-1.06 0L8 5.88 6.28 4.16a.75.75 0 1 0-1.06 1.06L6.94 6.94 5.22 8.66a.75.75 0 1 0 1.06 1.06L8 8l1.72 1.72a.75.75 0 1 0 1.06-1.06L9.06 6.94l1.72-1.72a.75.75 0 0 0 0-1.06Z\"/></svg>",
        }
    }
}

/// Build-time renderer for a `<Callout>` container.
///
/// The markup is two columns: a decorative icon and the author's Markdown.
/// The icon is `aria-hidden` because it repeats what the prose already says,
/// while the container keeps `role="note"` so assistive technology announces
/// the block as an aside rather than as body copy.
pub(super) struct Callout<'a> {
    kind: CalloutKind,
    emoji: Option<&'a str>,
}

impl<'a> Callout<'a> {
    pub(super) fn new(kind: CalloutKind, emoji: Option<&'a str>) -> Self {
        Self { kind, emoji }
    }

    pub(super) fn open(self, html: &mut String) {
        html.push_str("<div class=\"proa-docs-callout\" data-type=\"");
        escape_attr(self.kind.as_str(), html);
        html.push_str(
            "\" role=\"note\"><div class=\"proa-docs-callout-icon\" aria-hidden=\"true\">",
        );
        match self.emoji {
            Some(emoji) => escape_text(emoji, html),
            None => html.push_str(self.kind.icon()),
        }
        html.push_str("</div><div class=\"proa-docs-callout-body\">");
    }

    pub(super) fn close(html: &mut String) {
        html.push_str("</div></div>");
    }
}
