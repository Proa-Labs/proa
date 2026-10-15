// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MacroDialect {
    Html,
    Xml,
    /// `md!` / `md_sync!`. The body is a single string literal rather than a
    /// markup tree, so it never reaches the HTML template parser.
    Md,
}

#[derive(Debug, Clone)]
pub struct MacroInvocation {
    pub name: String,
    pub dialect: MacroDialect,
    pub name_start: usize,
    pub open_brace: usize,
    pub body_start: usize,
    pub body_end: usize,
    pub close_brace: usize,
    pub name_line: usize,
    pub name_column: usize,
    /// Indentation of the source line containing the macro invocation.
    pub indentation_column: usize,
    pub body_start_line: usize,
    pub body_start_column: usize,
}

impl MacroInvocation {
    pub fn body<'a>(&self, source: &'a str) -> &'a str {
        &source[self.body_start..self.body_end]
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct LineColumn {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug)]
pub struct SourceMap<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self {
            source,
            line_starts,
        }
    }

    pub fn line_column(&self, byte: usize) -> LineColumn {
        let line_idx = match self.line_starts.binary_search(&byte) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        let column = self
            .source
            .get(line_start..byte)
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or_else(|| byte.saturating_sub(line_start));
        LineColumn {
            line: line_idx + 1,
            column,
        }
    }

    pub fn byte_for_line_column(&self, line: usize, column: usize) -> Option<usize> {
        if line == 0 {
            return None;
        }
        let line_start = *self.line_starts.get(line - 1)?;
        let line_end = self
            .line_starts
            .get(line)
            .map(|next_start| next_start.saturating_sub(1))
            .unwrap_or(self.source.len());
        let line_source = self.source.get(line_start..line_end)?;
        if column == 0 {
            return Some(line_start);
        }
        let char_count = line_source.chars().count();
        if column == char_count {
            return Some(line_end);
        }
        if column > char_count {
            return None;
        }
        line_source
            .char_indices()
            .nth(column)
            .map(|(offset, _)| line_start + offset)
    }

    pub fn span_snippet(&self, span: proc_macro2::Span) -> Option<&'a str> {
        let start = span.start();
        let end = span.end();
        if start.line == 0 || end.line == 0 {
            return None;
        }
        let start = self.byte_for_line_column(start.line, start.column)?;
        let end = self.byte_for_line_column(end.line, end.column)?;
        if start <= end && end <= self.source.len() {
            Some(&self.source[start..end])
        } else {
            None
        }
    }

    pub fn source_from_span_start(&self, span: proc_macro2::Span) -> Option<&'a str> {
        let start = span.start();
        if start.line == 0 {
            return None;
        }
        let start = self.byte_for_line_column(start.line, start.column)?;
        self.source.get(start..)
    }
}

pub fn file_line_column_for_body_span(
    inv: &MacroInvocation,
    span: proc_macro2::Span,
) -> Option<LineColumn> {
    let start = span.start();
    if start.line == 0 {
        return None;
    }

    let line = inv.body_start_line + start.line - 1;
    let column = if start.line == 1 {
        inv.body_start_column + start.column
    } else {
        start.column
    };
    Some(LineColumn { line, column })
}

pub fn find_rust_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let roots = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };

    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            if root.extension().is_some_and(|ext| ext == "rs") {
                files.push(root);
            }
            continue;
        }

        for entry in WalkDir::new(&root)
            .into_iter()
            .filter_entry(is_visible_entry)
        {
            let entry = entry.with_context(|| format!("failed to walk {}", root.display()))?;
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "rs")
                && should_scan_path(entry.path())
            {
                files.push(entry.path().to_path_buf());
            }
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

pub fn find_changed_rust_files(base: Option<&str>, staged: bool) -> Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir().context("resolve current directory")?;
    find_changed_rust_files_in(&cwd, base, staged)
}

fn find_changed_rust_files_in(
    dir: &Path,
    base: Option<&str>,
    staged: bool,
) -> Result<Vec<PathBuf>> {
    // git prints paths relative to the repository root, not the current
    // directory, so resolve the root first or every path check below would
    // silently miss when the command runs from a subdirectory.
    let toplevel = git_output(dir, &["rev-parse", "--show-toplevel"])?
        .into_iter()
        .next()
        .map(PathBuf::from)
        .context("git did not report a repository root")?;

    let mut names = if staged {
        git_output(
            dir,
            &[
                "diff",
                "--cached",
                "--name-only",
                "--diff-filter=ACMRTUXB",
                "--",
                "*.rs",
            ],
        )?
    } else {
        let mut names = git_output(
            dir,
            &[
                "diff",
                "--name-only",
                "--diff-filter=ACMRTUXB",
                base.unwrap_or("HEAD"),
                "--",
                "*.rs",
            ],
        )?;
        names.extend(git_output(
            dir,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--full-name",
                "--",
                "*.rs",
            ],
        )?);
        names
    };

    names.sort();
    names.dedup();

    let mut files = Vec::new();
    for name in names {
        let rel = PathBuf::from(name);
        let path = toplevel.join(&rel);
        if path.exists() && should_scan_path(&rel) {
            files.push(path);
        }
    }
    Ok(files)
}

fn git_output(dir: &Path, args: &[&str]) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| "failed to run git")?;

    if !output.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn is_visible_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if !entry.file_type().is_dir() {
        return true;
    }
    entry.file_name().to_str().is_some_and(|name| {
        !name.starts_with('.') && !matches!(name, "target" | "node_modules" | "dist")
    })
}

fn should_scan_path(path: &Path) -> bool {
    if path.extension().is_none_or(|ext| ext != "rs") {
        return false;
    }
    if path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(
                ".git"
                    | ".claude"
                    | ".codex"
                    | ".worktrees"
                    | ".next"
                    | "target"
                    | "node_modules"
                    | "dist"
            )
        )
    }) {
        return false;
    }
    !is_generator_owned_golden(path) && !is_compile_fail_fixture(path)
}

fn is_generator_owned_golden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".golden.rs"))
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("golden")
        && path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("rsjs_macros")
}

fn is_compile_fail_fixture(path: &Path) -> bool {
    let mut under_ui_tests = false;
    let mut saw_tests = false;
    for component in path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
    {
        if component == "tests" {
            saw_tests = true;
            continue;
        }
        if saw_tests && component == "ui" {
            under_ui_tests = true;
            continue;
        }
        if under_ui_tests && component == "fail" {
            return true;
        }
    }
    false
}

pub fn find_macros(source: &str) -> Vec<MacroInvocation> {
    let map = SourceMap::new(source);
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let bare_html_is_foreign = bare_html_is_imported_from_yew(source);

    while i < bytes.len() {
        if let Some(next) = skip_ignored(source, i) {
            i = next;
            continue;
        }

        if is_ident_start(bytes[i]) {
            let name_start = i;
            let name_end = parse_ident_end(bytes, i);
            let name = &source[name_start..name_end];

            if name == "macro_rules" {
                if let Some(close_delimiter) = macro_rules_close_delimiter(source, name_end) {
                    i = close_delimiter + 1;
                    continue;
                }
            }

            if let Some((open_delimiter, close_delimiter)) = macro_open_delimiter(source, name_end)
            {
                if let Some(close_delimiter_at) =
                    find_matching_delimiter(source, open_delimiter, close_delimiter)
                {
                    if let Some(dialect) = macro_dialect(name) {
                        let foreign = qualified_macro_owner(source, name_start)
                            .is_some_and(|owner| owner != "proa_macros")
                            || (name == "html"
                                && qualified_macro_owner(source, name_start).is_none()
                                && bare_html_is_foreign);
                        if foreign || close_delimiter != b'}' {
                            i = close_delimiter_at + 1;
                            continue;
                        }

                        let name_pos = map.line_column(name_start);
                        let indentation_column = line_indentation_column(source, name_start);
                        let body_pos = map.line_column(open_delimiter + 1);
                        out.push(MacroInvocation {
                            name: name.to_owned(),
                            dialect,
                            name_start,
                            open_brace: open_delimiter,
                            body_start: open_delimiter + 1,
                            body_end: close_delimiter_at,
                            close_brace: close_delimiter_at,
                            name_line: name_pos.line,
                            name_column: name_pos.column,
                            indentation_column,
                            body_start_line: body_pos.line,
                            body_start_column: body_pos.column,
                        });
                        i = close_delimiter_at + 1;
                        continue;
                    }

                    // Macro inputs are arbitrary token languages. Do not treat
                    // an html!-looking identifier nested inside an unrelated
                    // macro as Proa source before that macro expands it.
                    i = close_delimiter_at + 1;
                    continue;
                }
            }
            i = name_end;
            continue;
        }

        i += 1;
    }

    out
}

fn macro_rules_close_delimiter(source: &str, after_macro_rules: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = skip_ws(bytes, after_macro_rules);
    if bytes.get(cursor) != Some(&b'!') {
        return None;
    }
    cursor = skip_ws(bytes, cursor + 1);
    if !bytes.get(cursor).is_some_and(|byte| is_ident_start(*byte)) {
        return None;
    }
    cursor = parse_ident_end(bytes, cursor);
    cursor = skip_ws(bytes, cursor);
    let close = match bytes.get(cursor).copied()? {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        _ => return None,
    };
    find_matching_delimiter(source, cursor, close)
}

fn qualified_macro_owner(source: &str, name_start: usize) -> Option<&str> {
    let prefix = source.get(..name_start)?.trim_end();
    let before_separator = prefix.strip_suffix("::")?.trim_end();
    let start = before_separator
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!ch.is_alphanumeric() && ch != '_').then_some(idx + ch.len_utf8()))
        .unwrap_or(0);
    before_separator
        .get(start..)
        .filter(|segment| !segment.is_empty())
}

fn bare_html_is_imported_from_yew(source: &str) -> bool {
    let compact = compact_code_for_imports(source);
    let mut yew_html = false;
    let mut proa_html = false;
    for statement in compact.split(';') {
        yew_html |= use_statement_imports(statement, "yew", "html");
        proa_html |= use_statement_imports(statement, "proa_macros", "html");
    }
    yew_html && !proa_html
}

fn compact_code_for_imports(source: &str) -> String {
    let mut compact = String::with_capacity(source.len());
    let mut cursor = 0;
    while cursor < source.len() {
        if let Some(next) = skip_ignored(source, cursor) {
            cursor = next;
            continue;
        }
        let ch = source[cursor..].chars().next().unwrap();
        if !ch.is_whitespace() {
            compact.push(ch);
        }
        cursor += ch.len_utf8();
    }
    compact
}

fn use_statement_imports(statement: &str, root: &str, target: &str) -> bool {
    let marker = format!("use{root}::");
    let Some(imports) = statement
        .find(&marker)
        .and_then(|start| statement.get(start + marker.len()..))
    else {
        return false;
    };
    if imports.contains('*') {
        return true;
    }

    let bytes = imports.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if !is_ident_start(bytes[cursor]) {
            cursor += 1;
            continue;
        }
        let end = parse_ident_end(bytes, cursor);
        if imports.get(cursor..end) == Some(target) {
            return true;
        }
        cursor = end;
    }
    false
}

fn line_indentation_column(source: &str, byte: usize) -> usize {
    let line_start = source
        .get(..byte)
        .and_then(|prefix| prefix.rfind('\n').map(|idx| idx + 1))
        .unwrap_or(0);
    source[line_start..byte]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .count()
}

fn macro_dialect(name: &str) -> Option<MacroDialect> {
    match name {
        "html" | "html_sync" => Some(MacroDialect::Html),
        "xml" | "xml_sync" => Some(MacroDialect::Xml),
        "md" | "md_sync" => Some(MacroDialect::Md),
        _ => None,
    }
}

fn macro_open_delimiter(source: &str, after_name: usize) -> Option<(usize, u8)> {
    let bytes = source.as_bytes();
    let mut i = skip_ws(bytes, after_name);
    if bytes.get(i) != Some(&b'!') {
        return None;
    }
    i = skip_ws(bytes, i + 1);
    let close_delimiter = match bytes.get(i).copied()? {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        _ => return None,
    };
    Some((i, close_delimiter))
}

fn find_matching_delimiter(source: &str, open: usize, close_delimiter: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    let open_delimiter = *bytes.get(open)?;
    let mut depth = 1usize;
    let mut i = open + 1;

    while i < bytes.len() {
        if let Some(next) = skip_ignored(source, i) {
            i = next;
            continue;
        }

        match bytes[i] {
            byte if byte == open_delimiter => depth += 1,
            byte if byte == close_delimiter => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

fn skip_ignored(source: &str, i: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    match bytes.get(i).copied()? {
        b'/' if bytes.get(i + 1) == Some(&b'/') => Some(skip_line_comment(bytes, i + 2)),
        b'/' if bytes.get(i + 1) == Some(&b'*') => Some(skip_block_comment(bytes, i + 2)),
        b'"' => Some(skip_string(bytes, i + 1)),
        b'\'' if looks_like_char_literal(bytes, i) => Some(skip_char(bytes, i + 1)),
        b'b' if bytes.get(i + 1) == Some(&b'"') => Some(skip_string(bytes, i + 2)),
        b'r' => skip_raw_string(source, i, 0),
        b'b' if bytes.get(i + 1) == Some(&b'r') => skip_raw_string(source, i, 1),
        _ => None,
    }
}

fn skip_line_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], mut i: usize) -> usize {
    let mut depth = 1usize;
    while i + 1 < bytes.len() {
        match (bytes[i], bytes[i + 1]) {
            (b'/', b'*') => {
                depth += 1;
                i += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_raw_string(source: &str, i: usize, prefix_len: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = i + prefix_len + 1;
    let mut hashes = 0usize;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;

    while cursor < bytes.len() {
        if bytes[cursor] == b'"' {
            let end = cursor + 1 + hashes;
            if end <= bytes.len() && bytes[cursor + 1..end].iter().all(|b| *b == b'#') {
                return Some(end);
            }
        }
        cursor += 1;
    }

    Some(bytes.len())
}

pub(crate) fn looks_like_char_literal(bytes: &[u8], i: usize) -> bool {
    let Some(next) = bytes.get(i + 1).copied() else {
        return false;
    };
    if is_ident_start(next) {
        let ident_end = parse_ident_end(bytes, i + 1);
        return bytes.get(ident_end) == Some(&b'\'');
    }
    true
}

pub(crate) fn skip_char(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b'\'' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn parse_ident_end(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    i
}

fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

pub fn display_path(path: &Path) -> PathBuf {
    let Ok(cwd) = std::env::current_dir() else {
        return path.to_path_buf();
    };
    pathdiff::diff_paths(path, cwd).unwrap_or_else(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use syn::spanned::Spanned;

    #[test]
    fn finds_supported_macros_and_ignores_strings_and_comments() {
        let source = r#"
            let ignored = "html_sync! { <div></div> }";
            // html! { <div></div> }
            proa_macros::html_sync! { <div>{name}</div> }
            xml! { <svg:path /> }
            let generated = quote! {
                ::proa_macros::html_sync! {
                    < #tag #attr=#value >
                        #(#children)*
                    </ #tag >
                }
            };
        "#;

        let macros = find_macros(source);
        assert_eq!(macros.len(), 2);
        assert_eq!(macros[0].name, "html_sync");
        assert_eq!(macros[0].dialect, MacroDialect::Html);
        assert_eq!(macros[0].name_column, 25);
        assert_eq!(macros[0].indentation_column, 12);
        assert_eq!(macros[1].name, "xml");
        assert_eq!(macros[1].dialect, MacroDialect::Xml);
    }

    #[test]
    fn ignores_foreign_and_macro_generated_html_invocations() {
        let source = r#"
            use yew::prelude::*;

            macro_rules! generated {
                ($child:ident) => {
                    html_sync! { <div>{$child}</div> }
                };
            }

            seq_macro::seq!(N in 0..3 {
                html_sync! {
                    #(<span>{N}</span>)*
                }
            });

            fn yew_view() -> Html {
                html! {
                    <main>{ for items }</main>
                }
            }

            fn proa_view() {
                proa_macros::html_sync! { <main>"Proa"</main> }
            }
        "#;

        let macros = find_macros(source);
        assert_eq!(macros.len(), 1, "{macros:#?}");
        assert_eq!(macros[0].name, "html_sync");
        assert!(macros[0].body(source).contains("\"Proa\""));
    }

    #[test]
    fn explicit_proa_html_import_wins_over_yew_glob() {
        let source = r#"
            use proa_macros::html;
            use yew::prelude::*;

            fn view() {
                html! { <main>"Proa"</main> }
            }
        "#;

        let macros = find_macros(source);
        assert_eq!(macros.len(), 1, "{macros:#?}");
        assert_eq!(macros[0].name, "html");
    }

    #[test]
    fn ignores_qualified_foreign_html_macro() {
        let source = r#"
            fn view() {
                yew::html! { <main>{ for items }</main> }
                ::proa_macros::html_sync! { <main>"Proa"</main> }
            }
        "#;

        let macros = find_macros(source);
        assert_eq!(macros.len(), 1, "{macros:#?}");
        assert_eq!(macros[0].name, "html_sync");
    }

    #[test]
    fn source_map_columns_are_utf8_safe() {
        let source = "let label = \"Rust→Zig\";\nhtml_sync! { <div>{self.title}</div> }\n";
        let map = SourceMap::new(source);
        let byte = source.find("Zig").unwrap();
        let pos = map.line_column(byte);
        assert_eq!(map.byte_for_line_column(pos.line, pos.column), Some(byte));

        let body = r#"<div title="Rust→Zig" data-label={self.title}></div>"#;
        let tokens = proc_macro2::TokenStream::from_str(body).unwrap();
        let root = syn::parse2::<proa_dsl::parse::HtmlRoot>(tokens).unwrap();
        let proa_dsl::parse::Node::Element(element) = &root.nodes[0] else {
            panic!("expected element");
        };
        let proa_dsl::parse::Attribute::Dynamic { value, .. } = &element.attrs[1] else {
            panic!("expected dynamic attr");
        };
        assert_eq!(
            SourceMap::new(body).span_snippet(value.span()),
            Some("self.title")
        );
    }

    #[test]
    fn finds_rs_files_under_visible_directories() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "").unwrap();
        std::fs::create_dir(temp.path().join("target")).unwrap();
        std::fs::write(temp.path().join("target").join("ignored.rs"), "").unwrap();
        let hidden_worktree = temp.path().join(".claude/worktrees/agent/src");
        std::fs::create_dir_all(&hidden_worktree).unwrap();
        std::fs::write(hidden_worktree.join("ignored.rs"), "").unwrap();

        let files = find_rust_files(&[temp.path().to_path_buf()]).unwrap();
        assert_eq!(files, vec![src.join("lib.rs")]);

        let hidden = hidden_worktree.join("ignored.rs");
        let files = find_rust_files(std::slice::from_ref(&hidden)).unwrap();
        assert_eq!(files, vec![hidden]);
    }

    #[test]
    fn directory_discovery_excludes_compile_fail_fixtures_but_explicit_files_are_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let fail = temp
            .path()
            .join("crate/tests/ui/conformance/fail/broken.rs");
        let pass = temp.path().join("crate/tests/ui/conformance/pass/valid.rs");
        std::fs::create_dir_all(fail.parent().unwrap()).unwrap();
        std::fs::create_dir_all(pass.parent().unwrap()).unwrap();
        std::fs::write(&fail, "compile_error!(\"expected\");").unwrap();
        std::fs::write(&pass, "fn valid() {}").unwrap();

        let files = find_rust_files(&[temp.path().to_path_buf()]).unwrap();
        assert_eq!(files, vec![pass]);

        let files = find_rust_files(std::slice::from_ref(&fail)).unwrap();
        assert_eq!(files, vec![fail]);
    }

    #[test]
    fn directory_discovery_excludes_generator_owned_goldens_but_explicit_files_are_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let golden_dir = temp.path().join("rsjs_macros/golden");
        let other_golden_dir = temp.path().join("other/golden");
        std::fs::create_dir_all(&golden_dir).unwrap();
        std::fs::create_dir_all(&other_golden_dir).unwrap();

        let generated = golden_dir.join("counter.golden.rs");
        let handwritten = golden_dir.join("helpers.rs");
        let unrelated = other_golden_dir.join("counter.golden.rs");
        std::fs::write(&generated, "").unwrap();
        std::fs::write(&handwritten, "").unwrap();
        std::fs::write(&unrelated, "").unwrap();

        let files = find_rust_files(&[temp.path().to_path_buf()]).unwrap();
        assert_eq!(files, vec![unrelated, handwritten]);

        let files = find_rust_files(std::slice::from_ref(&generated)).unwrap();
        assert_eq!(files, vec![generated]);
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed in {}", dir.display());
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
    }

    #[test]
    fn changed_files_resolve_against_repo_root_from_subdirectory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        init_repo(&root);
        std::fs::create_dir_all(root.join("crate/src")).unwrap();
        std::fs::write(root.join("crate/src/lib.rs"), "pub fn a() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "init"]);
        std::fs::write(root.join("crate/src/lib.rs"), "pub fn b() {}\n").unwrap();

        // Run the selection from a subdirectory: paths must still resolve.
        let files = find_changed_rust_files_in(&root.join("crate"), None, false).unwrap();
        assert_eq!(files, vec![root.join("crate/src/lib.rs")]);
        assert!(files[0].exists());
    }

    #[test]
    fn changed_files_include_untracked_from_subdirectory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        init_repo(&root);
        std::fs::create_dir_all(root.join("crate/src")).unwrap();
        std::fs::write(root.join("crate/src/lib.rs"), "pub fn a() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "init"]);
        std::fs::write(root.join("crate/src/new.rs"), "pub fn c() {}\n").unwrap();

        let files = find_changed_rust_files_in(&root.join("crate/src"), None, false).unwrap();
        assert_eq!(files, vec![root.join("crate/src/new.rs")]);
    }

    #[test]
    fn changed_files_exclude_generator_owned_goldens() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        init_repo(&root);
        std::fs::create_dir_all(root.join("rsjs_macros/golden")).unwrap();
        std::fs::create_dir_all(root.join("rsjs_macros/src")).unwrap();
        std::fs::write(
            root.join("rsjs_macros/golden/counter.golden.rs"),
            "generated v1\n",
        )
        .unwrap();
        std::fs::write(root.join("rsjs_macros/src/lib.rs"), "pub fn before() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "init"]);

        std::fs::write(
            root.join("rsjs_macros/golden/counter.golden.rs"),
            "generated v2\n",
        )
        .unwrap();
        std::fs::write(root.join("rsjs_macros/golden/new.golden.rs"), "generated\n").unwrap();
        std::fs::write(root.join("rsjs_macros/src/lib.rs"), "pub fn after() {}\n").unwrap();
        std::fs::write(
            root.join("rsjs_macros/src/new.rs"),
            "pub fn untracked() {}\n",
        )
        .unwrap();

        let files = find_changed_rust_files_in(&root, None, false).unwrap();
        assert_eq!(
            files,
            vec![
                root.join("rsjs_macros/src/lib.rs"),
                root.join("rsjs_macros/src/new.rs"),
            ]
        );
    }

    #[test]
    fn changed_files_staged_selection() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        init_repo(&root);
        std::fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "init"]);
        std::fs::write(root.join("a.rs"), "pub fn b() {}\n").unwrap();
        std::fs::write(root.join("b.rs"), "pub fn c() {}\n").unwrap();
        git(&root, &["add", "a.rs"]);

        let files = find_changed_rust_files_in(&root, None, true).unwrap();
        assert_eq!(files, vec![root.join("a.rs")]);
    }

    #[test]
    fn changed_files_bad_base_ref_errors_instead_of_reporting_clean() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        init_repo(&root);
        std::fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "init"]);

        let error = find_changed_rust_files_in(&root, Some("no-such-ref"), false).unwrap_err();
        assert!(error.to_string().contains("git diff"), "{error}");
    }

    #[test]
    fn changed_files_outside_repo_errors_instead_of_reporting_clean() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let error = find_changed_rust_files_in(&root, None, false).unwrap_err();
        assert!(error.to_string().contains("git"), "{error}");
    }
}
