// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Query/mutation/fetch request parsing: options, headers, bodies, projections, and invalidation strategies.
use super::*;

pub(super) fn request_config_expr(
    expr: &Expr,
    default_method: &str,
    message: &str,
) -> syn::Result<FrontendExpr> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    }) = expr
    {
        return Ok(FrontendExpr::Literal(Value::String(value.value())));
    }
    if let Expr::Struct(s) = expr {
        return fetch_request_struct_expr(s, default_method);
    }
    Err(syn::Error::new(expr.span(), message))
}

pub(super) fn request_config_response_is_text(expr: &FrontendExpr) -> bool {
    let FrontendExpr::Literal(Value::String(raw)) = expr else {
        return false;
    };
    let trimmed = raw.trim_start();
    if !trimmed.starts_with('{') {
        return false;
    }
    serde_json::from_str::<JsonValue>(trimmed)
        .ok()
        .and_then(|value| {
            value
                .get("response")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .is_some_and(|response| response.eq_ignore_ascii_case("text"))
}

#[derive(Default)]
pub(super) struct FetchRequestDescriptor {
    pub(super) url: Option<String>,
    pub(super) method: Option<String>,
    pub(super) credentials: Option<String>,
    pub(super) response: Option<String>,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Option<JsonValue>,
    pub(super) timeout_ms: Option<u32>,
}

pub(super) fn fetch_request_struct_expr(
    s: &ExprStruct,
    default_method: &str,
) -> syn::Result<FrontendExpr> {
    if !path_last_ident_is(&s.path, "FetchRequest") {
        return Err(syn::Error::new(
            s.path.span(),
            "`#[rsjs]` request config struct must be `rsjs::FetchRequest { ... }`",
        ));
    }
    if let Some(rest) = &s.rest {
        if !is_default_call(rest) {
            return Err(syn::Error::new(
                rest.span(),
                "`rsjs::FetchRequest` struct update must be `..Default::default()`",
            ));
        }
    }

    let mut seen = HashSet::new();
    let mut descriptor = FetchRequestDescriptor {
        method: Some(default_method.to_string()),
        credentials: Some("same-origin".to_string()),
        response: Some("json".to_string()),
        ..Default::default()
    };

    for field in &s.fields {
        let syn::Member::Named(name) = &field.member else {
            return Err(syn::Error::new(
                field.member.span(),
                "`rsjs::FetchRequest` fields must be named",
            ));
        };
        let field_name = name.to_string();
        if !seen.insert(field_name.clone()) {
            return Err(syn::Error::new(
                name.span(),
                format!("duplicate `rsjs::FetchRequest` field `{field_name}`"),
            ));
        }
        match field_name.as_str() {
            "url" => {
                descriptor.url = Some(parse_static_string_field(
                    &field.expr,
                    "`rsjs::FetchRequest.url` must be a string literal",
                )?);
            }
            "method" => {
                descriptor.method = parse_optional_enum_variant(
                    &field.expr,
                    "`rsjs::FetchRequest.method` must be `None` or \
                     `Some(rsjs::HttpMethod::{Get|Post|Put|Patch|Delete})`",
                    |variant| match variant {
                        "Get" => Some("GET"),
                        "Post" => Some("POST"),
                        "Put" => Some("PUT"),
                        "Patch" => Some("PATCH"),
                        "Delete" => Some("DELETE"),
                        _ => None,
                    },
                )?;
            }
            "credentials" => {
                descriptor.credentials = parse_optional_enum_variant(
                    &field.expr,
                    "`rsjs::FetchRequest.credentials` must be `None` or \
                     `Some(rsjs::FetchCredentials::{SameOrigin|Include|Omit})`",
                    |variant| match variant {
                        "SameOrigin" => Some("same-origin"),
                        "Include" => Some("include"),
                        "Omit" => Some("omit"),
                        _ => None,
                    },
                )?;
            }
            "response" => {
                descriptor.response = parse_optional_enum_variant(
                    &field.expr,
                    "`rsjs::FetchRequest.response` must be `None` or \
                     `Some(rsjs::ResponseKind::{Json|Text|Bytes|Empty})`",
                    |variant| match variant {
                        "Json" => Some("json"),
                        "Text" => Some("text"),
                        "Bytes" => Some("bytes"),
                        "Empty" => Some("empty"),
                        _ => None,
                    },
                )?;
            }
            "headers" => descriptor.headers = parse_headers_field(&field.expr)?,
            "body" => descriptor.body = parse_request_body_field(&field.expr)?,
            "timeout_ms" => {
                descriptor.timeout_ms = parse_named_option_u32(
                    &field.expr,
                    "`rsjs::FetchRequest.timeout_ms` must be `Some(<milliseconds>)` or `None`",
                )?
            }
            other => {
                return Err(syn::Error::new(
                    name.span(),
                    format!("`rsjs::FetchRequest` does not have a supported field named `{other}`"),
                ));
            }
        }
    }

    let Some(url) = descriptor.url else {
        return Err(syn::Error::new(
            s.path.span(),
            "`rsjs::FetchRequest` requires `url: \"...\"`",
        ));
    };

    let mut object = JsonMap::new();
    object.insert("url".to_string(), JsonValue::String(url));
    if let Some(method) = descriptor.method {
        object.insert("method".to_string(), JsonValue::String(method));
    }
    if let Some(credentials) = descriptor.credentials {
        object.insert("credentials".to_string(), JsonValue::String(credentials));
    }
    if let Some(response) = descriptor.response {
        object.insert("response".to_string(), JsonValue::String(response));
    }
    if !descriptor.headers.is_empty() {
        object.insert(
            "headers".to_string(),
            JsonValue::Array(
                descriptor
                    .headers
                    .into_iter()
                    .map(|(name, value)| json!([name, value]))
                    .collect(),
            ),
        );
    }
    if let Some(body) = descriptor.body {
        object.insert("body".to_string(), body);
    }
    if let Some(timeout_ms) = descriptor.timeout_ms {
        object.insert("timeout_ms".to_string(), json!(timeout_ms));
    }

    let encoded =
        rsjs_transport::canonical_json_bytes(&JsonValue::Object(object)).map_err(|err| {
            syn::Error::new(
                s.path.span(),
                format!("failed to encode `rsjs::FetchRequest` descriptor: {err}"),
            )
        })?;
    let encoded = String::from_utf8(encoded)
        .expect("canonical JSON serialization always produces valid UTF-8");
    Ok(FrontendExpr::Literal(Value::String(encoded)))
}

pub(super) fn parse_static_string_field(expr: &Expr, message: &str) -> syn::Result<String> {
    let Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    }) = expr
    else {
        return Err(syn::Error::new(expr.span(), message));
    };
    Ok(value.value())
}

pub(super) fn parse_optional_enum_variant(
    expr: &Expr,
    message: &str,
    map: impl Fn(&str) -> Option<&'static str>,
) -> syn::Result<Option<String>> {
    if is_none_expr(expr) {
        return Ok(None);
    }

    let expr = if let Expr::Call(call) = expr {
        if is_some_call(&call.func) {
            if call.args.len() != 1 {
                return Err(syn::Error::new(call.span(), message));
            }
            &call.args[0]
        } else {
            expr
        }
    } else {
        expr
    };

    let Some(variant) = enum_variant_ident(expr) else {
        return Err(syn::Error::new(expr.span(), message));
    };
    let Some(value) = map(variant.as_str()) else {
        return Err(syn::Error::new(expr.span(), message));
    };
    Ok(Some(value.to_string()))
}

pub(super) fn enum_variant_ident(expr: &Expr) -> Option<String> {
    let Expr::Path(ExprPath { path, .. }) = expr else {
        return None;
    };
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
}

pub(super) fn parse_headers_field(expr: &Expr) -> syn::Result<Vec<(String, String)>> {
    let expr = if let Expr::Reference(reference) = expr {
        reference.expr.as_ref()
    } else {
        expr
    };
    let Expr::Array(array) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "`rsjs::FetchRequest.headers` must be a static slice such as \
             `&[(\"Accept\", \"application/json\")]`",
        ));
    };
    let mut headers = Vec::new();
    for element in &array.elems {
        let Expr::Tuple(tuple) = element else {
            return Err(syn::Error::new(
                element.span(),
                "`rsjs::FetchRequest.headers` entries must be string-literal pairs",
            ));
        };
        if tuple.elems.len() != 2 {
            return Err(syn::Error::new(
                tuple.span(),
                "`rsjs::FetchRequest.headers` entries must have exactly two values",
            ));
        }
        headers.push((
            parse_static_string_field(
                &tuple.elems[0],
                "`rsjs::FetchRequest.headers` names must be string literals",
            )?,
            parse_static_string_field(
                &tuple.elems[1],
                "`rsjs::FetchRequest.headers` values must be string literals",
            )?,
        ));
    }
    Ok(headers)
}

pub(super) fn parse_request_body_field(expr: &Expr) -> syn::Result<Option<JsonValue>> {
    if is_none_expr(expr) {
        return Ok(None);
    }
    let expr = if let Expr::Call(call) = expr {
        if is_some_call(&call.func) {
            if call.args.len() != 1 {
                return Err(syn::Error::new(
                    call.span(),
                    "`rsjs::FetchRequest.body` must be `None` or \
                     `Some(rsjs::RequestBody::{Json|Text|BytesBase64|BytesBase64Url}(\"...\"))`",
                ));
            }
            &call.args[0]
        } else {
            expr
        }
    } else {
        expr
    };

    let Expr::Call(call) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "`rsjs::FetchRequest.body` must be `None` or \
             `Some(rsjs::RequestBody::{Json|Text|BytesBase64|BytesBase64Url}(\"...\"))`",
        ));
    };
    if call.args.len() != 1 {
        return Err(syn::Error::new(
            call.span(),
            "`rsjs::RequestBody` variants take exactly one string literal",
        ));
    }
    let Some(variant) = enum_variant_ident(&call.func) else {
        return Err(syn::Error::new(
            call.func.span(),
            "`rsjs::FetchRequest.body` must use a `rsjs::RequestBody` variant",
        ));
    };
    let kind = match variant.as_str() {
        "Json" => "json",
        "Text" => "text",
        "BytesBase64" => "bytes_base64",
        "BytesBase64Url" => "bytes_base64url",
        _ => {
            return Err(syn::Error::new(
                call.func.span(),
                "`rsjs::FetchRequest.body` must use `Json`, `Text`, `BytesBase64`, \
                 or `BytesBase64Url`",
            ));
        }
    };
    let value = parse_static_string_field(
        &call.args[0],
        "`rsjs::RequestBody` payload must be a string literal",
    )?;
    Ok(Some(json!({ "kind": kind, "value": value })))
}

pub(super) fn path_last_ident_is(path: &syn::Path, name: &str) -> bool {
    path.segments
        .last()
        .map(|segment| segment.ident == name)
        .unwrap_or(false)
}

pub(super) fn projection_literal_arg(expr: &Expr) -> syn::Result<Value> {
    literal_to_value(expr).map_err(|_| {
        syn::Error::new(
            expr.span(),
            "`#[rsjs]` projection predicates currently accept literal arguments only",
        )
    })
}

pub(super) fn expect_static_json_pointer_arg(expr: &Expr) -> syn::Result<JsonPointer> {
    let Expr::Lit(ExprLit {
        lit: Lit::Str(s), ..
    }) = expr
    else {
        return Err(syn::Error::new(
            expr.span(),
            "`value.at(...)` expects a static JSON Pointer string literal, e.g. \
             `value.at(\"/user/name\")`",
        ));
    };
    let value = s.value();
    if !value.is_empty() && !value.starts_with('/') {
        return Err(syn::Error::new(
            s.span(),
            "`value.at(...)` expects a JSON Pointer string starting with `/` \
             (or the empty string for the root value)",
        ));
    }
    Ok(JsonPointer::new(value))
}

pub(super) fn projection_field_segment(member: &syn::Member) -> syn::Result<String> {
    match member {
        syn::Member::Named(ident) => {
            let raw = ident.to_string();
            Ok(raw.strip_prefix("r#").unwrap_or(&raw).to_string())
        }
        syn::Member::Unnamed(index) => Ok(index.index.to_string()),
    }
}

pub(super) fn projection_index_segment(expr: &Expr) -> syn::Result<String> {
    let Expr::Lit(ExprLit {
        lit: Lit::Int(index),
        ..
    }) = expr
    else {
        return Err(syn::Error::new(
            expr.span(),
            "`#[rsjs]` typed JSON projection indexes must be non-negative integer \
             literals, e.g. `query.data().items[0].title`",
        ));
    };
    let value = index.base10_parse::<usize>().map_err(|_| {
        syn::Error::new(
            index.span(),
            "`#[rsjs]` typed JSON projection indexes must fit in `usize`",
        )
    })?;
    Ok(value.to_string())
}

pub(super) fn project_field_segment(target: FrontendExpr, segment: String) -> FrontendExpr {
    let escaped = json_pointer_escape_segment(&segment);
    match target {
        FrontendExpr::Project { target, mut path } => {
            path.0.push('/');
            path.0.push_str(&escaped);
            FrontendExpr::Project { target, path }
        }
        other => FrontendExpr::Project {
            target: Box::new(other),
            path: JsonPointer::new(format!("/{escaped}")),
        },
    }
}

pub(super) fn json_pointer_escape_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

pub(super) fn is_projection_field_base(expr: &FrontendExpr) -> bool {
    matches!(
        expr,
        FrontendExpr::QueryRead {
            accessor: QueryAccessor::Data | QueryAccessor::Error,
            ..
        } | FrontendExpr::MutationRead {
            accessor: MutationAccessor::Data | MutationAccessor::Error,
            ..
        } | FrontendExpr::SignalRead(_)
            | FrontendExpr::Local(_)
            | FrontendExpr::Project { .. }
            | FrontendExpr::Coalesce { .. }
    )
}

pub(super) fn query_accessor_method(method: &str) -> Option<QueryAccessor> {
    match method {
        "data" => Some(QueryAccessor::Data),
        "status" => Some(QueryAccessor::Status),
        "is_loading" => Some(QueryAccessor::IsLoading),
        "is_success" => Some(QueryAccessor::IsSuccess),
        "is_error" => Some(QueryAccessor::IsError),
        "is_refetching" => Some(QueryAccessor::IsRefetching),
        "is_stale" => Some(QueryAccessor::IsStale),
        "error" => Some(QueryAccessor::Error),
        "data_updated_at" => Some(QueryAccessor::DataUpdatedAt),
        "error_updated_at" => Some(QueryAccessor::ErrorUpdatedAt),
        "fetch_count" => Some(QueryAccessor::FetchCount),
        "download_loaded" => Some(QueryAccessor::DownloadLoaded),
        "download_total" => Some(QueryAccessor::DownloadTotal),
        "download_percent" => Some(QueryAccessor::DownloadPercent),
        _ => None,
    }
}

pub(super) fn mutation_accessor_method(method: &str) -> Option<MutationAccessor> {
    match method {
        "status" => Some(MutationAccessor::Status),
        "is_idle" => Some(MutationAccessor::IsIdle),
        "is_pending" => Some(MutationAccessor::IsPending),
        "is_success" => Some(MutationAccessor::IsSuccess),
        "is_error" => Some(MutationAccessor::IsError),
        "data" => Some(MutationAccessor::Data),
        "error" => Some(MutationAccessor::Error),
        _ => None,
    }
}

pub(super) fn as_type_method(method: &str) -> Option<AsType> {
    match method {
        "as_str" => Some(AsType::Str),
        "as_f64" => Some(AsType::F64),
        "as_i64" => Some(AsType::I64),
        "as_bool" => Some(AsType::Bool),
        "as_array" => Some(AsType::Array),
        "as_object" => Some(AsType::Object),
        _ => None,
    }
}

pub(super) fn unknown_query_method_error(span: proc_macro2::Span, method: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "`#[rsjs]` does not recognise query method `.{method}()`. Supported query methods are \
             `data()`, `status()`, `is_loading()`, `is_success()`, `is_error()`, \
             `is_refetching()`, `is_stale()`, `error()`, `data_updated_at()`, \
             `error_updated_at()`, `fetch_count()`, `download_loaded()`, \
             `download_total()`, and `download_percent()`. `refetch()` and \
             `invalidate()` are supported only as standalone event-handler statements. \
             Projection methods such as `.at(\"/path\").as_str()` are called on \
             `data()` or `error()` results."
        ),
    )
}

pub(super) fn unknown_mutation_method_error(span: proc_macro2::Span, method: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "`#[rsjs]` does not recognise mutation method `.{method}()`. Supported mutation \
             methods are `run(args)` as a handler statement, plus `status()`, `is_idle()`, \
             `is_pending()`, `is_success()`, `is_error()`, `data()`, and `error()` as \
             reactive reads."
        ),
    )
}

pub(super) fn parse_query_options(expr: &Expr) -> syn::Result<QueryOptions> {
    if is_default_call(expr) {
        return Ok(QueryOptions::default());
    }
    let Expr::Struct(s) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "`query_with_initial(...)` expects `rsjs::QueryOptions { ... }` or \
             `rsjs::QueryOptions::default()`",
        ));
    };
    let mut options = QueryOptions::default();
    for field in &s.fields {
        let syn::Member::Named(name) = &field.member else {
            return Err(syn::Error::new(
                field.member.span(),
                "`rsjs::QueryOptions` fields must be named",
            ));
        };
        match name.to_string().as_str() {
            "interval_ms" => options.interval_ms = parse_option_u32(&field.expr)?,
            "refetch_on_focus" => options.refetch_on_focus = parse_bool_literal(&field.expr)?,
            "download_progress" => options.download_progress = parse_bool_literal(&field.expr)?,
            "stream_text" => options.stream_text = parse_bool_literal(&field.expr)?,
            "retry_count" => options.retry_count = parse_u32_literal(&field.expr)?,
            "stale_time_ms" => options.stale_time_ms = parse_u32_literal(&field.expr)?,
            other => {
                return Err(syn::Error::new(
                    name.span(),
                    format!("`rsjs::QueryOptions` does not have a supported field named `{other}`"),
                ));
            }
        }
    }
    Ok(options)
}

pub(super) fn parse_mutation_options(expr: &Expr) -> syn::Result<MutationOptions> {
    if is_default_call(expr) {
        return Ok(MutationOptions::default());
    }
    let Expr::Struct(s) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "`mutation_with_options(...)` expects `rsjs::MutationOptions { ... }` or \
             `rsjs::MutationOptions::default()`",
        ));
    };
    let mut options = MutationOptions::default();
    for field in &s.fields {
        let syn::Member::Named(name) = &field.member else {
            return Err(syn::Error::new(
                field.member.span(),
                "`rsjs::MutationOptions` fields must be named",
            ));
        };
        match name.to_string().as_str() {
            "retry_count" => options.retry_count = parse_u32_literal(&field.expr)?,
            "on_success_invalidate_strategy" => {
                options.on_success_invalidate_strategy = parse_invalidation_strategy(&field.expr)?
            }
            other => {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "`rsjs::MutationOptions` does not have a supported field named `{other}`"
                    ),
                ));
            }
        }
    }
    Ok(options)
}

pub(super) fn is_default_call(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    path_ends_with(&call.func, "default") && call.args.is_empty()
}

pub(super) fn parse_u32_literal(expr: &Expr) -> syn::Result<u32> {
    let Expr::Lit(ExprLit {
        lit: Lit::Int(value),
        ..
    }) = expr
    else {
        return Err(syn::Error::new(
            expr.span(),
            "`#[rsjs]` option values must be integer literals",
        ));
    };
    value.base10_parse().map_err(|err| {
        syn::Error::new(value.span(), format!("integer literal out of range: {err}"))
    })
}

pub(super) fn parse_bool_literal(expr: &Expr) -> syn::Result<bool> {
    let Expr::Lit(ExprLit {
        lit: Lit::Bool(value),
        ..
    }) = expr
    else {
        return Err(syn::Error::new(
            expr.span(),
            "`#[rsjs]` option values must be boolean literals",
        ));
    };
    Ok(value.value)
}

pub(super) fn parse_option_u32(expr: &Expr) -> syn::Result<Option<u32>> {
    parse_named_option_u32(
        expr,
        "`interval_ms` must be `Some(<milliseconds>)` or `None`",
    )
}

pub(super) fn parse_named_option_u32(expr: &Expr, message: &str) -> syn::Result<Option<u32>> {
    if is_none_expr(expr) {
        return Ok(None);
    }
    let Expr::Call(call) = expr else {
        return Err(syn::Error::new(expr.span(), message));
    };
    if !is_some_call(&call.func) || call.args.len() != 1 {
        return Err(syn::Error::new(call.span(), message));
    }
    parse_u32_literal(&call.args[0]).map(Some)
}

pub(super) fn parse_invalidation_strategy(expr: &Expr) -> syn::Result<InvalidationStrategy> {
    let Expr::Path(ExprPath { path, .. }) = expr else {
        return Err(syn::Error::new(
            expr.span(),
            "`on_success_invalidate_strategy` must be `rsjs::InvalidationStrategy::Refetch` \
             or `rsjs::InvalidationStrategy::Reset`",
        ));
    };
    match path
        .segments
        .last()
        .map(|seg| seg.ident.to_string())
        .as_deref()
    {
        Some("Refetch") => Ok(InvalidationStrategy::Refetch),
        Some("Reset") => Ok(InvalidationStrategy::Reset),
        _ => Err(syn::Error::new(
            path.span(),
            "`on_success_invalidate_strategy` must be `rsjs::InvalidationStrategy::Refetch` \
             or `rsjs::InvalidationStrategy::Reset`",
        )),
    }
}
