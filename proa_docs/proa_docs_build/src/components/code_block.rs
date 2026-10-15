// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{fmt::Write as _, path::Path};

use crate::{
    code_fence::{CodeFenceInfo, LineHighlights},
    config::{MarkdownPipeline, MermaidFallback},
    highlight_cache::HighlightCache,
    markdown::{escape_attr, escape_text},
    syntax_highlight::HighlightedLine,
};

use super::{Files, LanguageIcon};

/// Build-time renderer for a fenced Markdown code block.
pub(crate) struct CodeBlock<'a> {
    code: &'a str,
    info: &'a str,
    source_path: &'a Path,
    pipeline: &'a MarkdownPipeline,
    highlight_cache: Option<&'a HighlightCache>,
}

impl<'a> CodeBlock<'a> {
    pub(crate) fn new(
        code: &'a str,
        info: &'a str,
        source_path: &'a Path,
        pipeline: &'a MarkdownPipeline,
    ) -> Self {
        Self {
            code,
            info,
            source_path,
            pipeline,
            highlight_cache: None,
        }
    }

    pub(crate) fn with_highlight_cache(mut self, cache: Option<&'a HighlightCache>) -> Self {
        self.highlight_cache = cache;
        self
    }

    /// Emit the complete highlighted block and its optional filename chrome.
    // proa-perf-ok(scope): docs code fences are compiled into trusted static HTML at build time; source code and attributes are escaped before insertion
    pub(crate) fn emit_html(self, html: &mut String) {
        let fence = CodeFenceInfo::parse(self.info);
        let language = fence.language;

        if self.pipeline.mermaid.enabled
            && self.pipeline.mermaid.code_fences
            && language == "mermaid"
        {
            self.emit_mermaid(html);
            return;
        }

        if language == "proa-filetree" {
            Files::emit_html(self.code, html);
            return;
        }

        html.push_str("<div data-code-block");
        if self.pipeline.code_highlight.enabled {
            html.push_str(" data-code-highlight=\"syntect\"");
        }
        if fence.line_highlights.is_some() {
            html.push_str(" data-code-lines");
        }
        if !language.is_empty() {
            html.push_str(" data-lang=\"");
            escape_attr(language, html);
            html.push('"');
        }
        if let Some(filename) = fence.filename.as_deref() {
            html.push_str(" data-filename=\"");
            escape_attr(filename, html);
            html.push('"');
        }
        html.push_str(" class=\"proa-docs-code-block\">");
        if let Some(filename) = fence.filename.as_deref() {
            CodeHeader::new(filename, language).emit_html(html);
        } else {
            CopyButton::emit_html(html);
        }
        html.push_str("<pre><code");
        if is_safe_language_class(language) {
            let _ = write!(html, " class=\"language-{language}");
            html.push('"');
        }
        html.push('>');
        let highlighted = self.highlighted_lines(language);
        if let Some(line_highlights) = fence.line_highlights.as_ref() {
            emit_code_lines(html, self.code, highlighted, line_highlights);
        } else if let Some(lines) = highlighted {
            for line in lines {
                html.push_str(&line.html);
                html.push_str(&line.ending);
            }
        } else {
            escape_text(self.code, html);
        }
        html.push_str("</code></pre></div>");
    }

    fn emit_mermaid(&self, html: &mut String) {
        html.push_str("<figure data-proa-docs-mermaid><pre data-proa-docs-mermaid-source hidden>");
        escape_text(self.code, html);
        html.push_str(
            "</pre><div data-proa-docs-mermaid-output aria-label=\"Mermaid diagram\"></div>",
        );
        if matches!(self.pipeline.mermaid.fallback, MermaidFallback::CodeBlock) {
            html.push_str("<noscript><pre><code class=\"language-mermaid\">");
            escape_text(self.code, html);
            html.push_str("</code></pre></noscript>");
        }
        html.push_str("</figure>");
    }

    fn highlighted_lines(&self, language: &str) -> Option<Vec<HighlightedLine>> {
        if !self.pipeline.code_highlight.enabled {
            return None;
        }

        let compute = || crate::syntax_highlight::highlight_lines(self.code, language);
        let result = match self.highlight_cache {
            Some(cache) => cache.get_or_compute(self.code, language, compute),
            None => compute(),
        };
        match result {
            Ok(highlighted) => highlighted,
            Err(error) => {
                println!(
                    "cargo:warning=docs code highlight: {}: `{language}` failed to parse ({error}); emitting escaped source text",
                    self.source_path.display()
                );
                None
            }
        }
    }
}

/// Filename and language icon displayed above a code block.
struct CodeHeader<'a> {
    filename: &'a str,
    language: &'a str,
}

impl<'a> CodeHeader<'a> {
    fn new(filename: &'a str, language: &'a str) -> Self {
        Self { filename, language }
    }

    // proa-perf-ok(scope): docs code-block chrome is compiled once; filenames are escaped before insertion
    fn emit_html(self, html: &mut String) {
        html.push_str("<div class=\"proa-docs-code-header\" data-code-header>");
        LanguageIcon::new(self.language).emit_html(html);
        html.push_str("<span class=\"proa-docs-code-filename\" data-code-filename>");
        escape_text(self.filename, html);
        html.push_str("</span>");
        CopyButton::emit_html(html);
        html.push_str("</div>");
    }
}

/// Copy control shared by titled and untitled code blocks.
struct CopyButton;

impl CopyButton {
    // proa-perf-ok(scope): docs copy-button chrome is trusted static HTML emitted only during the docs build
    fn emit_html(html: &mut String) {
        html.push_str(
            "<button type=\"button\" data-code-copy aria-label=\"Copy code\" title=\"Copy code\">Copy</button>",
        );
    }
}

// proa-perf-ok(scope): build-time syntax highlighting emits trusted highlighter spans; fallback code text is escaped before insertion
fn emit_code_lines(
    html: &mut String,
    code: &str,
    highlighted: Option<Vec<HighlightedLine>>,
    line_highlights: &LineHighlights,
) {
    if let Some(lines) = highlighted {
        for (index, line) in lines.into_iter().enumerate() {
            emit_code_line_start(html, index + 1, line_highlights);
            html.push_str(&line.html);
            html.push_str("</span>");
            html.push_str(&line.ending);
        }
        return;
    }

    for (index, line) in code.split_inclusive('\n').enumerate() {
        let (body, ending) = crate::code_fence::split_line_ending(line);
        emit_code_line_start(html, index + 1, line_highlights);
        escape_text(body, html);
        html.push_str("</span>");
        html.push_str(ending);
    }
}

// proa-perf-ok(scope): build-time code-line framing emits fixed trusted markup and numeric line metadata
fn emit_code_line_start(html: &mut String, line: usize, line_highlights: &LineHighlights) {
    let _ = write!(html, "<span data-code-line=\"{line}\"");
    if line_highlights.contains(line) {
        html.push_str(" data-highlighted-line");
    }
    html.push('>');
}

fn is_safe_language_class(language: &str) -> bool {
    !language.is_empty()
        && language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
