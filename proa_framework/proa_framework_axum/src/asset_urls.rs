// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! URL-aware transformations for managed application assets. Parsing and caching
//! occur at the asset boundary, never while rendering or buffering page HTML.

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, Response, StatusCode},
    middleware::{self, Next},
    Router,
};
use base16ct::HexDisplay;
use bytes::Bytes;
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, SourceType, Span};
use proa_core::BasePath;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;
const CACHE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct AssetCache {
    entries: VecDeque<([u8; 32], Bytes)>,
    bytes: usize,
}

/// Compose root-relative CSS URLs and native JavaScript URL operations in a
/// static router. FrameworkBuilder applies this to `with_static_service` when
/// a base path is configured. Custom routers/scaffolds can install it directly.
/// Page HTML bodies remain untouched and streaming. Parsed asset results are
/// cached by content with a bounded memory budget.
pub fn base_path_assets(router: Router, base_path: BasePath) -> Router {
    if base_path.as_str().is_empty() {
        return router;
    }
    let cache = Arc::new(Mutex::new(AssetCache::default()));
    router.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let base_path = base_path.clone();
            let cache = cache.clone();
            async move {
                let extension = request.uri().path().rsplit('.').next().unwrap_or("");
                let javascript = matches!(extension, "js" | "mjs");
                let head = request.method() == axum::http::Method::HEAD;
                if !(head || request.method() == axum::http::Method::GET)
                    || !(javascript || extension == "css")
                {
                    return next.run(request).await;
                }
                if head {
                    *request.method_mut() = axum::http::Method::GET;
                }
                let conditional = request.headers().get(header::IF_NONE_MATCH).cloned();
                for name in [
                    header::ACCEPT_ENCODING,
                    header::RANGE,
                    header::IF_RANGE,
                    header::IF_NONE_MATCH,
                    header::IF_MODIFIED_SINCE,
                ] {
                    request.headers_mut().remove(name);
                }
                let response = next.run(request).await;
                if response.status() != StatusCode::OK
                    || response.headers().contains_key(header::CONTENT_ENCODING)
                {
                    return if head {
                        response.map(|_| Body::empty())
                    } else {
                        response
                    };
                }
                let mime = response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("")
                    .split(';')
                    .next()
                    .unwrap_or("");
                let transformable = if javascript {
                    matches!(mime.trim(), "text/javascript" | "application/javascript")
                } else {
                    mime.trim() == "text/css"
                };
                if !transformable {
                    return if head {
                        response.map(|_| Body::empty())
                    } else {
                        response
                    };
                }
                let (mut parts, body) = response.into_parts();
                let original = match to_bytes(body, MAX_ASSET_BYTES).await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::error!(%error, "could not read managed URL asset");
                        return if head {
                            failure().map(|_| Body::empty())
                        } else {
                            failure()
                        };
                    }
                };
                let mut hash = Sha256::new();
                hash.update([javascript as u8]);
                hash.update(&original);
                let key: [u8; 32] = hash.finalize().into();
                let cached = cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .entries
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, bytes)| bytes.clone());
                let bytes = match cached {
                    Some(bytes) => bytes,
                    None => {
                        let source = match std::str::from_utf8(&original) {
                            Ok(source) => source,
                            Err(_) => {
                                return if head {
                                    failure().map(|_| Body::empty())
                                } else {
                                    failure()
                                }
                            }
                        };
                        let result = if javascript {
                            rewrite_js(source, &base_path)
                        } else {
                            rewrite_css(source, &base_path)
                        };
                        let bytes = match result {
                            Ok(source) => Bytes::from(source),
                            Err(error) => {
                                tracing::error!(%error, "could not compose managed asset URLs");
                                return if head {
                                    failure().map(|_| Body::empty())
                                } else {
                                    failure()
                                };
                            }
                        };
                        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
                        if bytes.len() <= CACHE_BYTES {
                            while cache.bytes + bytes.len() > CACHE_BYTES
                                || cache.entries.len() >= 32
                            {
                                if let Some((_, previous)) = cache.entries.pop_front() {
                                    cache.bytes -= previous.len();
                                } else {
                                    break;
                                }
                            }
                            cache.bytes += bytes.len();
                            cache.entries.push_back((key, bytes.clone()));
                        }
                        bytes
                    }
                };
                let etag = format!("\"{:x}\"", HexDisplay(&Sha256::digest(&bytes)));
                parts.headers.remove(header::CONTENT_RANGE);
                parts.headers.remove(header::ACCEPT_RANGES);
                parts.headers.remove("content-md5");
                parts.headers.remove("digest");
                parts.headers.remove("content-digest");
                parts.headers.remove("repr-digest");
                parts
                    .headers
                    .insert(header::ETAG, etag.parse().expect("valid asset ETag"));
                if conditional
                    .as_ref()
                    .and_then(|h| h.to_str().ok())
                    .is_some_and(|value| {
                        value
                            .split(',')
                            .any(|v| v.trim().trim_start_matches("W/") == etag || v.trim() == "*")
                    })
                {
                    parts.status = StatusCode::NOT_MODIFIED;
                    parts.headers.remove(header::CONTENT_LENGTH);
                    return Response::from_parts(parts, Body::empty());
                }
                parts
                    .headers
                    .insert(header::CONTENT_LENGTH, bytes.len().into());
                Response::from_parts(
                    parts,
                    if head {
                        Body::empty()
                    } else {
                        Body::from(bytes)
                    },
                )
            }
        },
    ))
}

fn failure() -> Response<Body> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from("Could not prepare application asset URLs"))
        .expect("valid asset response")
}

fn application_url(base: &BasePath, url: &str) -> Option<String> {
    let prefix = base.as_str();
    if prefix.is_empty() || !url.starts_with('/') || url.starts_with("//") {
        return None;
    }
    if url
        .strip_prefix(prefix)
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(['/', '?', '#']))
    {
        return None;
    }
    Some(base.url(url).to_string())
}

#[derive(Debug)]
struct Edit {
    span: Span,
    text: String,
}

fn apply_edits(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|edit| (edit.span.start, edit.span.end));
    let mut out = String::with_capacity(source.len());
    let mut written = 0;
    for edit in edits {
        let start = edit.span.start as usize;
        let end = edit.span.end as usize;
        debug_assert!(start >= written, "overlapping URL edits");
        out.push_str(&source[written..start]);
        out.push_str(&edit.text);
        written = end;
    }
    out.push_str(&source[written..]);
    out
}

pub(crate) fn rewrite_css(source: &str, base: &BasePath) -> Result<String, String> {
    use cssparser::{Parser, ParserInput, Token};
    fn visit<'i>(
        parser: &mut Parser<'i, '_>,
        base: &BasePath,
        edits: &mut Vec<Edit>,
        depth: usize,
        image_set: bool,
    ) -> Result<(), cssparser::ParseError<'i, ()>> {
        if depth > 128 {
            return Err(parser.new_custom_error(()));
        }
        let mut import = false;
        loop {
            let start = parser.position().byte_index();
            let token = match parser.next_including_whitespace_and_comments() {
                Ok(token) => token.clone(),
                Err(_) => break,
            };
            let mut replacement = None;
            match token {
                Token::WhiteSpace(_) | Token::Comment(_) => continue,
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => {
                    import = true;
                    continue;
                }
                Token::UnquotedUrl(url) => {
                    replacement = application_url(base, &url).map(|url| (url, true));
                }
                Token::QuotedString(url) if import || image_set => {
                    replacement = application_url(base, &url).map(|url| (url, false));
                }
                Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                    let url =
                        parser.parse_nested_block(|nested| Ok(nested.expect_string_cloned()?))?;
                    replacement = application_url(base, &url).map(|url| (url, true));
                }
                Token::Function(name)
                    if name.eq_ignore_ascii_case("image-set")
                        || name.eq_ignore_ascii_case("-webkit-image-set") =>
                {
                    parser
                        .parse_nested_block(|nested| visit(nested, base, edits, depth + 1, true))?;
                }
                Token::Function(_)
                | Token::CurlyBracketBlock
                | Token::SquareBracketBlock
                | Token::ParenthesisBlock => {
                    parser.parse_nested_block(|nested| {
                        visit(nested, base, edits, depth + 1, false)
                    })?;
                }
                _ => {}
            }
            if let Some((url, function)) = replacement {
                let mut value = String::new();
                if function {
                    value.push_str("url(");
                }
                cssparser::serialize_string(&url, &mut value).expect("write CSS string");
                if function {
                    value.push(')');
                }
                edits.push(Edit {
                    span: Span::new(start as u32, parser.position().byte_index() as u32),
                    text: value,
                });
            }
            import = false;
        }
        Ok(())
    }
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut edits = Vec::new();
    visit(&mut parser, base, &mut edits, 0, false)
        .map_err(|_| "invalid or excessively nested CSS URL syntax".to_string())?;
    Ok(apply_edits(source, edits))
}

pub(crate) fn rewrite_js(source: &str, base: &BasePath) -> Result<String, String> {
    let allocator = Allocator::default();
    let mut parsed = oxc_parser::Parser::new(&allocator, source, SourceType::unambiguous()).parse();
    // Oxc's unambiguous mode parses with script context before deciding the
    // module kind, so valid top-level await needs an explicit module retry.
    if !parsed.errors.is_empty() {
        parsed = oxc_parser::Parser::new(&allocator, source, SourceType::mjs()).parse();
    }
    if !parsed.errors.is_empty() {
        return Err("invalid JavaScript asset".to_string());
    }
    let semantic = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
    if !semantic.errors.is_empty() {
        return Err("invalid JavaScript bindings".to_string());
    }
    let mut helper = format!(
        "__proa_url_{:x}",
        HexDisplay(&Sha256::digest(source.as_bytes()))
    );
    while source.contains(&helper) {
        helper.push('_');
    }
    let mut visitor = JsUrls {
        base,
        scoping: semantic.semantic.scoping(),
        edits: Vec::new(),
        helper,
        dynamic: false,
    };
    visitor.visit_program(&parsed.program);
    let mut out = apply_edits(source, visitor.edits);
    if visitor.dynamic {
        let prefix = serde_json::to_string(base.as_str()).expect("serialize asset mount");
        out.push_str(&format!("\nfunction {}(p){{const b={prefix};return typeof p==='string'&&p.startsWith('/')&&!p.startsWith('//')&&p!==b&&!p.startsWith(b+'/')&&!p.startsWith(b+'?')&&!p.startsWith(b+'#')?b+p:p;}}\n", visitor.helper));
    }
    Ok(out)
}

struct JsUrls<'s> {
    base: &'s BasePath,
    scoping: &'s oxc_semantic::Scoping,
    edits: Vec<Edit>,
    helper: String,
    dynamic: bool,
}

impl JsUrls<'_> {
    fn global(&self, expression: &Expression<'_>, name: &str) -> bool {
        match expression.get_inner_expression() {
            Expression::Identifier(id) if id.name == name => id
                .reference_id
                .get()
                .is_some_and(|id| self.scoping.get_reference(id).symbol_id().is_none()),
            _ => false,
        }
    }
    fn browser_global(&self, expression: &Expression<'_>) -> bool {
        self.global(expression, "window")
            || self.global(expression, "globalThis")
            || self.global(expression, "self")
    }
    fn native(&self, expression: &Expression<'_>, name: &str) -> bool {
        self.global(expression, name)
            || matches!(expression.get_inner_expression(), Expression::StaticMemberExpression(member) if member.property.name == name && self.browser_global(&member.object))
    }
    fn local_url_base(&self, expression: &Expression<'_>) -> bool {
        if self.native(expression, "location") {
            return true;
        }
        match expression.get_inner_expression() {
            Expression::StaticMemberExpression(member) if member.property.name == "url" => {
                matches!(member.object.get_inner_expression(), Expression::MetaProperty(meta)
                    if meta.meta.name == "import" && meta.property.name == "meta")
            }
            Expression::StaticMemberExpression(member)
                if matches!(member.property.name.as_str(), "href" | "origin") =>
            {
                self.native(&member.object, "location")
            }
            _ => false,
        }
    }
    fn literal(&mut self, literal: &StringLiteral<'_>) {
        if let Some(url) = application_url(self.base, &literal.value) {
            self.edits.push(Edit {
                span: literal.span,
                text: serde_json::to_string(&url).expect("serialize URL literal"),
            });
        }
    }
    fn url(&mut self, expression: &Expression<'_>) {
        if let Expression::StringLiteral(literal) = expression.get_inner_expression() {
            self.literal(literal);
            return;
        }
        let span = expression.span();
        self.edits.push(Edit {
            span: Span::new(span.start, span.start),
            text: format!("{}(", self.helper),
        });
        self.edits.push(Edit {
            span: Span::new(span.end, span.end),
            text: ")".into(),
        });
        self.dynamic = true;
    }
}

impl<'a> Visit<'a> for JsUrls<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.native(&call.callee, "fetch") {
            if let Some(argument) = call.arguments.first().and_then(Argument::as_expression) {
                self.url(argument);
            }
        }
        walk::walk_call_expression(self, call);
    }
    fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
        let local_url = self.native(&expression.callee, "URL")
            && expression
                .arguments
                .get(1)
                .and_then(Argument::as_expression)
                .is_some_and(|base| self.local_url_base(base));
        if local_url
            || [
                "Request",
                "Worker",
                "SharedWorker",
                "WebSocket",
                "EventSource",
            ]
            .into_iter()
            .any(|name| self.native(&expression.callee, name))
        {
            if let Some(argument) = expression
                .arguments
                .first()
                .and_then(Argument::as_expression)
            {
                self.url(argument);
            }
        }
        walk::walk_new_expression(self, expression);
    }
    fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
        self.url(&expression.source);
        walk::walk_import_expression(self, expression);
    }
    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
        self.literal(&declaration.source);
        walk::walk_import_declaration(self, declaration);
    }
    fn visit_export_named_declaration(&mut self, declaration: &ExportNamedDeclaration<'a>) {
        if let Some(source) = &declaration.source {
            self.literal(source);
        }
        walk::walk_export_named_declaration(self, declaration);
    }
    fn visit_export_all_declaration(&mut self, declaration: &ExportAllDeclaration<'a>) {
        self.literal(&declaration.source);
        walk::walk_export_all_declaration(self, declaration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    fn base() -> BasePath {
        BasePath::new("/apps/demo").unwrap()
    }

    #[test]
    fn css_changes_url_tokens_without_changing_comments_or_content() {
        let source = r###"/* url(/comment) */ @import "/theme.css" screen;
@media screen { .a { background: url('/images/a b.svg'); mask: url(/mask.svg#icon); background-image: image-set("/small.png" 1x, url(/large.png) 2x); content: "url(/text)"; } }
.b { a: url(//cdn.test/a); b: url(https://cdn.test/a); c: url(data:image/png;base64,abc); d: url(#icon); e: url(/apps/demo/already); }"###;
        let result = rewrite_css(source, &base()).unwrap();
        assert!(
            result.contains("@import \"/apps/demo/theme.css\" screen"),
            "{result}"
        );
        assert!(
            result.contains("url(\"/apps/demo/images/a b.svg\")"),
            "{result}"
        );
        assert!(
            result.contains("url(\"/apps/demo/mask.svg#icon\")"),
            "{result}"
        );
        assert!(
            result.contains(
                "image-set(\"/apps/demo/small.png\" 1x, url(\"/apps/demo/large.png\") 2x)"
            ),
            "{result}"
        );
        for literal in [
            "/* url(/comment) */",
            "content: \"url(/text)\"",
            "url(//cdn.test/a)",
            "url(https://cdn.test/a)",
            "url(data:image/png;base64,abc)",
            "url(#icon)",
            "url(/apps/demo/already)",
        ] {
            assert!(result.contains(literal), "{literal}: {result}");
        }
    }

    #[test]
    fn js_changes_native_url_operations_and_module_imports_only() {
        let source = r###"'use strict';
import x from '/library.js'; export { x } from '/exports.js';
const label = '/ordinary'; const regex = /fetch\('/;
fetch('/api/items'); window.fetch('/api/window');
const request = new Request('/api/save');
new Worker('/worker.js'); new EventSource('/events');
new URL('/image.svg', import.meta.url); new URL('/icon.svg', window.location.href);
new URL('/remote.svg', 'https://cdn.test/');
import('/lazy.js'); fetch('//cdn.test/data'); fetch('https://api.test/data');
function local(fetch, window, Request) { fetch('/local'); window.fetch('/local'); new Request('/local'); }
"###;
        let result = rewrite_js(source, &base()).unwrap();
        for path in [
            "/library.js",
            "/exports.js",
            "/api/items",
            "/api/window",
            "/api/save",
            "/worker.js",
            "/events",
            "/lazy.js",
            "/image.svg",
            "/icon.svg",
        ] {
            assert!(
                result.contains(&format!("\"/apps/demo{path}\"")),
                "{path}: {result}"
            );
        }
        for literal in [
            "const label = '/ordinary'",
            "new URL('/remote.svg', 'https://cdn.test/')",
            "const regex = /fetch\\('/",
            "fetch('/local')",
            "window.fetch('/local')",
            "new Request('/local')",
            "fetch('//cdn.test/data')",
            "fetch('https://api.test/data')",
        ] {
            assert!(result.contains(literal), "{literal}: {result}");
        }
        assert!(!result.contains("function __proa_url_"));
    }

    #[test]
    fn dynamic_url_wrappers_preserve_directives_and_valid_nested_expressions() {
        let source =
            "'use strict'; fetch(new Request(path)); import(module_path); fetch(`/api/${id}`);";
        let result = rewrite_js(source, &base()).unwrap();
        assert!(result.starts_with("'use strict';"));
        assert!(result.contains("function __proa_url_"));
        let alloc = Allocator::default();
        let parsed = oxc_parser::Parser::new(&alloc, &result, SourceType::mjs()).parse();
        assert!(parsed.errors.is_empty(), "{:?}: {result}", parsed.errors);
        assert!(rewrite_js("const =", &base()).is_err());
    }

    #[test]
    fn rewritten_dynamic_browser_calls_execute_with_the_correct_paths() {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let source = r###"
import assert from 'node:assert/strict';
const seen = [];
globalThis.Request = class { constructor(url) { this.url = url; } };
globalThis.fetch = async value => { seen.push(typeof value === 'string' ? value : value.url); };
const path = '/api/dynamic';
await fetch(path);
await fetch(new Request('/api/nested'));
await fetch('/apps/demo/already');
await fetch('https://api.example/data');
await fetch('//cdn.example/data');
function custom(fetch) { fetch('/ordinary'); }
custom(value => seen.push(value));
assert.deepEqual(seen, ['/apps/demo/api/dynamic', '/apps/demo/api/nested', '/apps/demo/already', 'https://api.example/data', '//cdn.example/data', '/ordinary']);
"###;
        let source = rewrite_js(source, &base()).unwrap();
        let child = Command::new("node")
            .arg("--input-type=module")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("node unavailable; JavaScript execution check skipped");
                return;
            }
            Err(error) => panic!("start node: {error}"),
        };
        child
            .stdin
            .take()
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn managed_assets_use_transformed_validators_head_and_full_ranges() {
        let source = "a { background:url(/image.svg) }";
        let router = base_path_assets(
            Router::new().route(
                "/app.css",
                axum::routing::get(move || async move {
                    (
                        [
                            (header::CONTENT_TYPE, "text/css"),
                            (header::ETAG, "\"original\""),
                        ],
                        source,
                    )
                }),
            ),
            base(),
        );
        let send = |method: &str, etag: Option<&str>| {
            let mut request = Request::builder()
                .method(method)
                .uri("/app.css")
                .header(header::RANGE, "bytes=0-1");
            if let Some(etag) = etag {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            router.clone().oneshot(request.body(Body::empty()).unwrap())
        };
        let response = send("GET", None).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()[header::ETAG]
            .to_str()
            .unwrap()
            .to_owned();
        let length = response.headers()[header::CONTENT_LENGTH].clone();
        assert_ne!(etag, "\"original\"");
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert!(std::str::from_utf8(&body)
            .unwrap()
            .contains("/apps/demo/image.svg"));
        let head = send("HEAD", None).await.unwrap();
        assert_eq!(head.headers()[header::CONTENT_LENGTH], length);
        assert_eq!(head.headers()[header::ETAG], etag);
        assert!(to_bytes(head.into_body(), 1024).await.unwrap().is_empty());
        let conditional = send("GET", Some(&etag)).await.unwrap();
        assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);
        assert!(to_bytes(conditional.into_body(), 1024)
            .await
            .unwrap()
            .is_empty());
    }
}
