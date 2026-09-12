// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::{config::MarkdownPipeline, markdown::escape_text, typescript::TypeTableRequest};

use super::attr_value;

/// Parse the configured TypeTable component into a deferred docgen request.
pub(crate) fn parse_type_table(
    raw: &str,
    source_path: &Path,
    pipeline: &MarkdownPipeline,
) -> Option<TypeTableRequest> {
    let trimmed = raw.trim();
    let component = &pipeline.typescript.component_name;
    if !trimmed.starts_with('<') || !trimmed[1..].starts_with(component) {
        return None;
    }

    Some(TypeTableRequest {
        source_markdown_path: source_path.to_path_buf(),
        path: attr_value(trimmed, "path"),
        name: attr_value(trimmed, "name"),
        inline_type: attr_value(trimmed, "type"),
    })
}

/// Emit an escaped placeholder replaced after TypeScript docgen completes.
// proa-perf-ok(scope): temporary docs build placeholder escapes the raw directive before insertion
pub(crate) fn render_type_table_placeholder(html: &mut String, raw: &str) {
    html.push_str("<div data-proa-docs-type-table-pending>");
    escape_text(raw, html);
    html.push_str("</div>");
}
