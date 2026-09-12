// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Isolated JavaScript emitter for Client IR v3.
//!
//! Island and client-helper emission now route supported handler/helper bodies
//! through this backend. The isolated API remains useful for direct Client IR
//! v3 tests and conformance snapshots.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

use super::expr::{
    AsType, BrowserStorageArea, EncodingOp, EventAttributeSource, EventCollection,
    EventCollectionField, EventValue, LocationField, MutationAccessor, NodeRefAccessor,
    QueryAccessor, StorageValueType, StringPredicateOp, StringTransformOp, WindowField,
};
use super::linked_helpers::{LinkedHelperCallTarget, UNLINKED_HELPER_IDENTIFIER_PREFIX};
use crate::reactive::{ConsoleLevel, EventFileSource, StorageArea, UrlHistoryMode};
use crate::types::{CallbackSlotId, MutationId, NodeRefId, SignalId};
use crate::value::Value;

use super::expr::{
    ClientBinaryOp, ClientBrowserRead, ClientExpr, ClientFunctionId, ClientFunctionRef,
    ClientLocalId, ClientUnaryOp,
};
use super::module::{ClientFunction, ClientImport, ClientModule};
use super::stmt::{
    ClientBlock, ClientBrowserEffect, ClientNodeRefEffect, ClientPlace, ClientSimpleClosure,
    ClientStmt,
};
use super::types::ClientType;

/// Error returned when an IR node has no v3 JS lowering yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEmitError {
    UnsupportedExpr(&'static str),
    UnsupportedStmt(&'static str),
    UnsupportedEventValue(EventValue),
    UnresolvedFunctionRef(String),
    ReservedUnlinkedFunctionRef(String),
    UnboundHelperCall(crate::abi::HelperCallEdgeId),
    LinkedHelperCallArityMismatch {
        edge: crate::abi::HelperCallEdgeId,
        expected: usize,
        actual: usize,
    },
    ContextualIntegerLiteralOutOfRange {
        value: i128,
        expected: ClientType,
    },
    UnknownFunctionExport {
        export: String,
        function: ClientFunctionId,
    },
}

impl fmt::Display for ClientEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedExpr(kind) => write!(f, "unsupported Client IR v3 expression: {kind}"),
            Self::UnsupportedStmt(kind) => write!(f, "unsupported Client IR v3 statement: {kind}"),
            Self::UnsupportedEventValue(value) => {
                write!(f, "unsupported Client IR v3 event value: {value:?}")
            }
            Self::UnresolvedFunctionRef(path) => {
                write!(
                    f,
                    "unresolved Client IR v3 function reference `{path}` reached JS emission"
                )
            }
            Self::ReservedUnlinkedFunctionRef(reference) => write!(
                f,
                "deployable Client IR function reference `{reference}` uses the compiler-reserved unresolved namespace `{UNLINKED_HELPER_IDENTIFIER_PREFIX}`"
            ),
            Self::UnboundHelperCall(edge) => write!(
                f,
                "linked Client IR helper edge {} has no deployable function binding",
                edge.index()
            ),
            Self::LinkedHelperCallArityMismatch {
                edge,
                expected,
                actual,
            } => write!(
                f,
                "linked Client IR helper edge {} has {actual} arguments; its authenticated target signature expects {expected}",
                edge.index()
            ),
            Self::ContextualIntegerLiteralOutOfRange { value, expected } => write!(
                f,
                "contextual integer literal {value} is outside linked helper parameter type {expected:?}"
            ),
            Self::UnknownFunctionExport { export, function } => {
                write!(
                    f,
                    "Client IR v3 module exports `{export}` from unknown function id {}",
                    function.0
                )
            }
        }
    }
}

impl std::error::Error for ClientEmitError {}

/// Naming hooks used by the isolated v3 emitter.
#[derive(Debug, Clone)]
pub struct ClientEmitContext {
    pub event_name: String,
    pub root_name: String,
    pub local_prefix: String,
    pub local_names: BTreeMap<ClientLocalId, String>,
    pub signal_prefix: String,
    pub query_prefix: String,
    pub mutation_prefix: String,
    pub callback_prefix: String,
    pub node_ref_prefix: String,
    pub node_name: Option<String>,
    pub(crate) linked_helper_calls:
        Option<BTreeMap<crate::abi::HelperCallEdgeId, LinkedHelperCallTarget>>,
}

impl Default for ClientEmitContext {
    fn default() -> Self {
        Self {
            event_name: "event".to_string(),
            root_name: "root".to_string(),
            local_prefix: "__local_".to_string(),
            local_names: BTreeMap::new(),
            signal_prefix: "__sig_".to_string(),
            query_prefix: "__rsjs_query_".to_string(),
            mutation_prefix: "__rsjs_mutation_".to_string(),
            callback_prefix: "__callback_".to_string(),
            node_ref_prefix: "__ref_".to_string(),
            node_name: None,
            linked_helper_calls: None,
        }
    }
}

impl ClientEmitContext {
    pub fn local_name(&self, id: ClientLocalId) -> String {
        self.local_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("{}{}", self.local_prefix, id.0))
    }

    pub fn with_local_name(mut self, id: ClientLocalId, name: impl Into<String>) -> Self {
        self.local_names.insert(id, name.into());
        self
    }

    pub(crate) fn with_linked_helper_calls(
        mut self,
        calls: BTreeMap<crate::abi::HelperCallEdgeId, LinkedHelperCallTarget>,
    ) -> Self {
        self.linked_helper_calls = Some(calls);
        self
    }

    pub fn signal_name(&self, id: SignalId) -> String {
        format!("{}{}", self.signal_prefix, id.0)
    }

    pub fn query_name(&self, id: crate::types::QueryId) -> String {
        format!("{}{}", self.query_prefix, id.0)
    }

    pub fn mutation_name(&self, id: MutationId) -> String {
        format!("{}{}", self.mutation_prefix, id.0)
    }

    pub fn callback_name(&self, id: CallbackSlotId) -> String {
        format!("{}{}", self.callback_prefix, id.0)
    }

    pub fn node_ref_name(&self, id: NodeRefId) -> String {
        format!("{}{}", self.node_ref_prefix, id.0)
    }
}

/// Emit a Client IR v3 expression with default names.
pub fn emit_expr(expr: &ClientExpr) -> Result<String, ClientEmitError> {
    let mut out = String::new();
    emit_expr_with_context(&mut out, expr, &ClientEmitContext::default())?;
    Ok(out)
}

/// Emit a Client IR v3 statement with default names.
pub fn emit_stmt(stmt: &ClientStmt) -> Result<String, ClientEmitError> {
    let mut out = String::new();
    emit_stmt_with_context(&mut out, stmt, &ClientEmitContext::default())?;
    Ok(out)
}

/// Emit a Client IR v3 block with default names.
pub fn emit_block(block: &ClientBlock) -> Result<String, ClientEmitError> {
    let mut out = String::new();
    emit_block_with_context(&mut out, block, &ClientEmitContext::default())?;
    Ok(out)
}

/// Emit a complete Client IR v3 module as an ES module.
pub fn emit_module(module: &ClientModule) -> Result<String, ClientEmitError> {
    let mut out = String::new();
    emit_module_with_context(&mut out, module, &ClientEmitContext::default())?;
    Ok(out)
}

pub fn emit_module_with_context(
    out: &mut String,
    module: &ClientModule,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    let mut numeric_helpers = module_numeric_runtime_helpers(module);
    numeric_helpers.retain(|helper| {
        !module.imports.iter().any(|import| {
            import.module == "/rsjs-runtime.js"
                && import.name == *helper
                && import.alias.as_deref().is_none_or(|alias| alias == *helper)
        })
    });
    if !numeric_helpers.is_empty() {
        out.push_str("import { ");
        for (index, helper) in numeric_helpers.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(helper);
        }
        out.push_str(" } from \"/rsjs-runtime.js\";\n");
    }
    for import in &module.imports {
        emit_import(out, import);
    }
    if !numeric_helpers.is_empty() || !module.imports.is_empty() {
        out.push('\n');
    }

    for constant in &module.constants {
        out.push_str("const ");
        out.push_str(&constant.name);
        out.push_str(" = ");
        emit_value(out, &constant.value);
        out.push_str(";\n");
    }
    if !module.constants.is_empty() {
        out.push('\n');
    }

    for function in &module.functions {
        emit_function(out, function, ctx)?;
        out.push('\n');
    }

    let mut exports: Vec<(String, ClientFunctionId)> = module
        .exports
        .iter()
        .map(|export| (export.name.clone(), export.function))
        .collect();
    for function in &module.functions {
        if function.visibility == super::module::ClientVisibility::Exported
            && !exports
                .iter()
                .any(|(_, exported_id)| *exported_id == function.id)
        {
            exports.push((function.name.clone(), function.id));
        }
    }

    if !exports.is_empty() {
        out.push_str("export { ");
        for (i, (name, function_id)) in exports.iter().enumerate() {
            if !module
                .functions
                .iter()
                .any(|function| function.id == *function_id)
            {
                return Err(ClientEmitError::UnknownFunctionExport {
                    export: name.clone(),
                    function: *function_id,
                });
            }
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&local_function_name(*function_id));
            if name != &local_function_name(*function_id) {
                out.push_str(" as ");
                out.push_str(name);
            }
        }
        out.push_str(" };\n");
    }

    Ok(())
}

pub fn emit_expr_with_context(
    out: &mut String,
    expr: &ClientExpr,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    match expr {
        ClientExpr::Unit => out.push_str("undefined"),
        ClientExpr::Literal(value) => emit_value(out, value),
        ClientExpr::ArrayLit(elems) => {
            out.push('[');
            for (i, elem) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_value(out, elem);
            }
            out.push(']');
        }
        ClientExpr::QueryKey(parts) => {
            out.push('[');
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr_with_context(out, part, ctx)?;
            }
            out.push(']');
        }
        ClientExpr::Local(id) => out.push_str(&ctx.local_name(*id)),
        ClientExpr::SignalRead(id) => {
            out.push_str(&ctx.signal_name(*id));
            out.push_str(".get()");
        }
        ClientExpr::EventRead(value) => emit_event_value(out, *value, ctx)?,
        ClientExpr::EventCurrentTarget => {
            out.push_str(&ctx.event_name);
            out.push_str(".__rsjsCurrentTarget");
        }
        ClientExpr::DomRead { target, accessor } => {
            out.push_str("((");
            emit_expr_with_context(out, target, ctx)?;
            out.push(')');
            emit_dom_read_suffix(out, accessor, "");
        }
        ClientExpr::EventCollectionLen(collection) => {
            emit_event_collection_len(out, *collection, ctx);
        }
        ClientExpr::EventCollectionRead {
            collection,
            index,
            field,
        } => emit_event_collection_read(out, *collection, index, *field, ctx)?,
        ClientExpr::NodeRefRead { id, accessor } => {
            emit_node_ref_read(out, *id, accessor, ctx);
        }
        ClientExpr::EventTargetWithinNodeRef { id, inside } => {
            emit_event_target_within_node_ref(out, *id, *inside, ctx);
        }
        ClientExpr::EventAttribute { source, attr } => {
            emit_event_attribute(out, *source, attr, ctx);
        }
        ClientExpr::EventPayload { attr } => emit_event_payload(out, attr, ctx),
        ClientExpr::CallbackPayload => out.push_str("__payload"),
        ClientExpr::NodePayload { attr, .. } => emit_node_payload(out, attr, ctx),
        ClientExpr::BrowserRead(read) => emit_browser_read(out, read, ctx)?,
        ClientExpr::Encoding { op, value } => emit_encoding(out, *op, value, ctx)?,
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            out.push('(');
            emit_expr_with_context(out, condition, ctx)?;
            out.push_str(" ? ");
            emit_expr_with_context(out, then_expr, ctx)?;
            out.push_str(" : ");
            emit_expr_with_context(out, else_expr, ctx)?;
            out.push(')');
        }
        ClientExpr::IsNonEmpty { target } => {
            out.push_str("(((__v) => (__v != null && (typeof __v === 'string' ? __v.length > 0 : Array.isArray(__v) ? __v.length > 0 : typeof __v === 'object' ? Object.keys(__v).length > 0 : false)))(");
            emit_expr_with_context(out, target, ctx)?;
            out.push_str("))");
        }
        ClientExpr::StringToLowercase { target } => {
            out.push_str("(String((");
            emit_expr_with_context(out, target, ctx)?;
            out.push_str(") ?? \"\").toLowerCase())");
        }
        ClientExpr::StringContains { haystack, needle } => {
            out.push_str("(String((");
            emit_expr_with_context(out, haystack, ctx)?;
            out.push_str(") ?? \"\").includes(String((");
            emit_expr_with_context(out, needle, ctx)?;
            out.push_str(") ?? \"\")))");
        }
        // Rust to_uppercase/to_lowercase apply the full Unicode case
        // mappings while JS toUpperCase/toLowerCase use the
        // locale-independent default mappings, which differ for a
        // handful of characters. `trim` is emitted separately below:
        // native JS trim has a different whitespace set from Rust.
        ClientExpr::StringTransform { op, target } => {
            match op {
                StringTransformOp::ToUppercase => {
                    out.push_str("(String((");
                    emit_expr_with_context(out, target, ctx)?;
                    out.push_str(") ?? \"\").toUpperCase())");
                }
                StringTransformOp::Trim => {
                    // Rust `str::trim` uses `char::is_whitespace`:
                    // U+0009..U+000D, U+0020, U+0085, U+00A0,
                    // U+1680, U+2000..U+200A, U+2028, U+2029,
                    // U+202F, U+205F, and U+3000. In particular it
                    // includes U+0085 and excludes U+FEFF, unlike JS
                    // `String.prototype.trim`.
                    out.push_str("(String((");
                    emit_expr_with_context(out, target, ctx)?;
                    out.push_str(") ?? \"\").replace(/^[\\u0009-\\u000D\\u0020\\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]+|[\\u0009-\\u000D\\u0020\\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]+$/g, \"\"))");
                }
            }
        }
        ClientExpr::StringPredicate {
            op,
            haystack,
            needle,
        } => {
            out.push_str("(String((");
            emit_expr_with_context(out, haystack, ctx)?;
            out.push_str(") ?? \"\").");
            out.push_str(string_predicate_method_js(*op));
            out.push_str("(String((");
            emit_expr_with_context(out, needle, ctx)?;
            out.push_str(") ?? \"\")))");
        }
        ClientExpr::Unary { op, expr } => {
            if let ClientUnaryOp::CheckedNeg(domain) = op {
                out.push_str("checked_int_neg(");
                emit_expr_with_context(out, expr, ctx)?;
                out.push_str(", \"");
                out.push_str(domain.as_str());
                out.push_str("\")");
                return Ok(());
            }
            out.push('(');
            out.push_str(unary_op_js(*op));
            emit_expr_with_context(out, expr, ctx)?;
            out.push(')');
        }
        ClientExpr::Binary { op, left, right } => {
            if let ClientBinaryOp::Checked { domain, .. } = op {
                out.push_str(
                    op.checked_helper_js()
                        .expect("checked integer op has a runtime helper"),
                );
                out.push('(');
                emit_checked_integer_operand(out, left, *domain, ctx)?;
                out.push_str(", ");
                emit_checked_integer_operand(out, right, *domain, ctx)?;
                out.push_str(", \"");
                out.push_str(domain.as_str());
                out.push_str("\")");
                return Ok(());
            }
            // A `Dyn*` operator has an operand whose JS representation the page
            // seed pins, so it lowers to a helper call rather than an infix
            // operator. `Math.trunc` must never be reached from here: it throws
            // on a BigInt argument, and `num_div_int` branches instead.
            if let Some(helper) = op.dyn_helper_js() {
                out.push_str(helper);
                out.push('(');
                emit_expr_with_context(out, left, ctx)?;
                out.push_str(", ");
                emit_expr_with_context(out, right, ctx)?;
                out.push(')');
                return Ok(());
            }
            // Integer division truncates toward zero in Rust; bare JS `/`
            // yields a fractional Number, so DivInt wraps the quotient.
            let trunc = matches!(op, ClientBinaryOp::DivInt);
            if trunc {
                out.push_str("Math.trunc");
            }
            out.push('(');
            emit_expr_with_context(out, left, ctx)?;
            out.push(' ');
            out.push_str(binary_op_js(*op));
            out.push(' ');
            emit_expr_with_context(out, right, ctx)?;
            out.push(')');
        }
        ClientExpr::QueryRead { id, accessor } => {
            out.push_str(&ctx.query_name(*id));
            out.push('.');
            out.push_str(query_accessor_js(*accessor));
            out.push_str("()");
        }
        ClientExpr::MutationRead { id, accessor } => {
            out.push_str(&ctx.mutation_name(*id));
            out.push('.');
            out.push_str(mutation_accessor_js(*accessor));
            out.push_str("()");
        }
        ClientExpr::Project { target, path } => {
            out.push_str("rsjs.__project(");
            emit_expr_with_context(out, target, ctx)?;
            out.push_str(", [");
            for (i, seg) in path.segments().iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push('"');
                out.push_str(&js_string_inner(seg));
                out.push('"');
            }
            out.push_str("])");
        }
        ClientExpr::AsType { target, ty } => {
            if matches!(ty, AsType::F64FromInt) {
                out.push_str("Number(");
                emit_expr_with_context(out, target, ctx)?;
                out.push(')');
                return Ok(());
            }
            out.push_str("rsjs.");
            out.push_str(as_type_js_helper(*ty));
            out.push('(');
            emit_expr_with_context(out, target, ctx)?;
            out.push(')');
        }
        ClientExpr::IsPresent { target } => {
            out.push_str("rsjs.__is_present(");
            emit_expr_with_context(out, target, ctx)?;
            out.push(')');
        }
        ClientExpr::Eq { target, value } => {
            out.push_str("rsjs.__eq(");
            emit_expr_with_context(out, target, ctx)?;
            out.push_str(", ");
            emit_value(out, value);
            out.push(')');
        }
        ClientExpr::Neq { target, value } => {
            out.push_str("rsjs.__neq(");
            emit_expr_with_context(out, target, ctx)?;
            out.push_str(", ");
            emit_value(out, value);
            out.push(')');
        }
        ClientExpr::Coalesce { target, fallback } => {
            out.push_str("rsjs.__coalesce(");
            emit_expr_with_context(out, target, ctx)?;
            out.push_str(", ");
            emit_value(out, fallback);
            out.push(')');
        }
        ClientExpr::StructInit {
            type_name: _,
            fields,
        } => {
            out.push_str("({");
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    out.push_str(", ");
                }
                out.push('"');
                out.push_str(&js_string_inner(&field.name));
                out.push_str("\": ");
                emit_expr_with_context(out, &field.value, ctx)?;
            }
            out.push_str("})");
        }
        ClientExpr::Field { base, name } => {
            out.push('(');
            emit_expr_with_context(out, base, ctx)?;
            out.push_str(")[\"");
            out.push_str(&js_string_inner(name));
            out.push_str("\"]");
        }
        ClientExpr::Index { .. } => return Err(ClientEmitError::UnsupportedExpr("index access")),
        ClientExpr::Call { function, args } => emit_call(out, function, args, ctx)?,
    }
    Ok(())
}

/// Decode the JSON-string wire representation used by `ClientEncode` for
/// i64/u64 payload attributes before a checked wide-integer operation sees
/// it. Ordinary helper and signal operands remain strict BigInts; conversion
/// is deliberately limited to DOM payload crossings whose server encoding is
/// known to be a quoted canonical decimal.
fn emit_checked_integer_operand(
    out: &mut String,
    expr: &ClientExpr,
    domain: crate::IntegerDomain,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    let wide_wire_payload = domain.is_wide()
        && matches!(
            expr,
            ClientExpr::EventPayload { .. } | ClientExpr::NodePayload { .. }
        );
    if !wide_wire_payload {
        return emit_expr_with_context(out, expr, ctx);
    }

    out.push_str(
        "((__rsjs_wire) => { if (typeof __rsjs_wire !== \"string\" || \
         !/^(?:0|-?[1-9][0-9]*)$/.test(__rsjs_wire)) { throw new TypeError(\"rsjs checked wide \
         integer payload must be a canonical decimal string\"); } return BigInt(__rsjs_wire); })(",
    );
    emit_expr_with_context(out, expr, ctx)?;
    out.push(')');
    Ok(())
}

pub fn emit_stmt_with_context(
    out: &mut String,
    stmt: &ClientStmt,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    emit_stmt_inner(out, stmt, ctx, 0)
}

pub fn emit_block_with_context(
    out: &mut String,
    block: &ClientBlock,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    emit_block_with_context_and_indent(out, block, ctx, 0)
}

pub fn emit_block_with_context_and_indent(
    out: &mut String,
    block: &ClientBlock,
    ctx: &ClientEmitContext,
    indent_level: usize,
) -> Result<(), ClientEmitError> {
    for stmt in &block.stmts {
        emit_stmt_inner(out, stmt, ctx, indent_level)?;
    }
    Ok(())
}

// proa-perf-ok(scope): trusted low-level JavaScript source emitter, not SSR HTML rendering
fn emit_stmt_inner(
    out: &mut String,
    stmt: &ClientStmt,
    ctx: &ClientEmitContext,
    indent_level: usize,
) -> Result<(), ClientEmitError> {
    push_indent(out, indent_level);
    match stmt {
        ClientStmt::Expr(expr) => {
            emit_expr_with_context(out, expr, ctx)?;
            out.push_str(";\n");
        }
        ClientStmt::Let {
            local,
            value,
            name: _,
            ty: _,
        } => {
            out.push_str("let ");
            out.push_str(&ctx.local_name(*local));
            if let Some(value) = value {
                out.push_str(" = ");
                emit_expr_with_context(out, value, ctx)?;
            }
            out.push_str(";\n");
        }
        ClientStmt::Assign { local, value } => {
            out.push_str(&ctx.local_name(*local));
            out.push_str(" = ");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(";\n");
        }
        ClientStmt::AssignPlace { place, value } => {
            emit_place(out, place, ctx);
            out.push_str(" = ");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(";\n");
        }
        ClientStmt::SignalSet { id, value } => {
            out.push_str(&ctx.signal_name(*id));
            out.push_str(".set(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(");\n");
        }
        ClientStmt::SignalUpdate { id, closure } => {
            out.push_str(&ctx.signal_name(*id));
            out.push_str(".update(");
            emit_closure(out, closure, ctx)?;
            out.push_str(");\n");
        }
        ClientStmt::EventPreventDefault => {
            out.push_str(&ctx.event_name);
            out.push_str(".preventDefault();\n");
        }
        ClientStmt::EventStopPropagation => {
            out.push_str(&ctx.event_name);
            out.push_str(".stopPropagation();\n");
        }
        ClientStmt::Console { level, args } => {
            out.push_str("console.");
            out.push_str(console_level_js(*level));
            out.push('(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr_with_context(out, arg, ctx)?;
            }
            out.push_str(");\n");
        }
        ClientStmt::CallbackInvoke { callback, payload } => {
            out.push_str("if (");
            out.push_str(&ctx.root_name);
            out.push_str(".__rsjs_callbacks?.");
            out.push_str(&ctx.callback_name(*callback));
            out.push_str(") ");
            out.push_str(&ctx.root_name);
            out.push_str(".__rsjs_callbacks.");
            out.push_str(&ctx.callback_name(*callback));
            out.push('(');
            emit_expr_with_context(out, payload, ctx)?;
            out.push_str(");\n");
        }
        ClientStmt::RunMutation { mutation, args } => {
            out.push_str(&ctx.mutation_name(*mutation));
            out.push_str(".run(");
            if args.len() == 1 {
                emit_expr_with_context(out, &args[0], ctx)?;
            } else {
                out.push('[');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    emit_expr_with_context(out, arg, ctx)?;
                }
                out.push(']');
            }
            out.push_str(");\n");
        }
        ClientStmt::QueryRefetch { query } => {
            out.push_str(&ctx.query_name(*query));
            out.push_str(".refetch();\n");
        }
        ClientStmt::QueryInvalidate { query } => {
            out.push_str(&ctx.query_name(*query));
            out.push_str(".invalidate();\n");
        }
        ClientStmt::BrowserEffect(effect) => emit_browser_effect(out, effect, ctx)?,
        ClientStmt::NodeRefEffect { id, effect } => {
            emit_node_ref_effect(out, *id, effect, ctx)?;
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            out.push_str("if (");
            emit_expr_with_context(out, condition, ctx)?;
            out.push_str(") {\n");
            emit_block_with_context_and_indent(out, then_block, ctx, indent_level + 1)?;
            push_indent(out, indent_level);
            if let Some(else_block) = else_block {
                out.push_str("} else {\n");
                emit_block_with_context_and_indent(out, else_block, ctx, indent_level + 1)?;
                push_indent(out, indent_level);
            }
            out.push_str("}\n");
        }
        ClientStmt::ForRange {
            local,
            start,
            end,
            inclusive,
            body,
            ..
        } => {
            let local_name = ctx.local_name(*local);
            let start_name = format!("__rsjs_range_start_{}", local.0);
            let end_name = format!("__rsjs_range_end_{}", local.0);
            let start_raw_name = format!("__rsjs_range_start_raw_{}", local.0);
            let end_raw_name = format!("__rsjs_range_end_raw_{}", local.0);
            out.push_str("{\n");
            push_indent(out, indent_level + 1);
            out.push_str("const ");
            out.push_str(&start_raw_name);
            out.push_str(" = ");
            emit_expr_with_context(out, start, ctx)?;
            out.push_str(";\n");
            push_indent(out, indent_level + 1);
            out.push_str("const ");
            out.push_str(&end_raw_name);
            out.push_str(" = ");
            emit_expr_with_context(out, end, ctx)?;
            out.push_str(";\n");
            push_indent(out, indent_level + 1);
            out.push_str("const ");
            out.push_str(&start_name);
            out.push_str(" = typeof ");
            out.push_str(&start_raw_name);
            out.push_str(" === \"bigint\" ? ");
            out.push_str(&start_raw_name);
            out.push_str(" : Math.trunc(Number(");
            out.push_str(&start_raw_name);
            out.push_str("));\n");
            push_indent(out, indent_level + 1);
            out.push_str("const ");
            out.push_str(&end_name);
            out.push_str(" = typeof ");
            out.push_str(&end_raw_name);
            out.push_str(" === \"bigint\" ? ");
            out.push_str(&end_raw_name);
            out.push_str(" : Math.trunc(Number(");
            out.push_str(&end_raw_name);
            out.push_str("));\n");
            push_indent(out, indent_level + 1);
            out.push_str("for (let ");
            out.push_str(&local_name);
            out.push_str(" = ");
            out.push_str(&start_name);
            out.push_str("; ");
            out.push_str(&local_name);
            if *inclusive {
                out.push_str(" <= ");
            } else {
                out.push_str(" < ");
            }
            out.push_str(&end_name);
            out.push_str("; ");
            out.push_str(&local_name);
            out.push_str(" += typeof ");
            out.push_str(&local_name);
            out.push_str(" === \"bigint\" ? 1n : 1) {\n");
            emit_block_with_context_and_indent(out, body, ctx, indent_level + 2)?;
            push_indent(out, indent_level + 1);
            out.push_str("}\n");
            push_indent(out, indent_level);
            out.push_str("}\n");
        }
        ClientStmt::Return(value) => {
            out.push_str("return");
            if let Some(value) = value {
                out.push(' ');
                emit_expr_with_context(out, value, ctx)?;
            }
            out.push_str(";\n");
        }
        ClientStmt::Block(block) => {
            out.push_str("{\n");
            emit_block_with_context_and_indent(out, block, ctx, indent_level + 1)?;
            push_indent(out, indent_level);
            out.push_str("}\n");
        }
    }
    Ok(())
}

fn emit_place(out: &mut String, place: &ClientPlace, ctx: &ClientEmitContext) {
    match place {
        ClientPlace::Local(local) => out.push_str(&ctx.local_name(*local)),
        ClientPlace::Field { base, name } => {
            emit_place(out, base, ctx);
            out.push_str("[\"");
            out.push_str(&js_string_inner(name));
            out.push_str("\"]");
        }
    }
}

fn push_indent(out: &mut String, indent_level: usize) {
    for _ in 0..indent_level {
        out.push_str("    ");
    }
}

fn module_numeric_runtime_helpers(module: &ClientModule) -> BTreeSet<&'static str> {
    let mut helpers = BTreeSet::new();
    for function in &module.functions {
        crate::analysis::dep_collector::walk_client_block(&function.body, &mut |expr| match expr {
            ClientExpr::Binary { op, .. } => {
                if let Some(helper) = op.dyn_helper_js() {
                    helpers.insert(helper);
                }
                if let Some(helper) = op.checked_helper_js() {
                    helpers.insert(helper);
                }
            }
            ClientExpr::Unary {
                op: ClientUnaryOp::CheckedNeg(_),
                ..
            } => {
                helpers.insert("checked_int_neg");
            }
            _ => {}
        });
    }
    helpers
}

fn emit_import(out: &mut String, import: &ClientImport) {
    if import.name == "*" {
        out.push_str("import * as ");
        out.push_str(import.alias.as_deref().unwrap_or("__rsjs_module"));
        out.push_str(" from \"");
        out.push_str(&js_string_inner(&import.module));
        out.push_str("\";\n");
        return;
    }

    out.push_str("import { ");
    out.push_str(&import.name);
    if let Some(alias) = &import.alias {
        out.push_str(" as ");
        out.push_str(alias);
    }
    out.push_str(" } from \"");
    out.push_str(&js_string_inner(&import.module));
    out.push_str("\";\n");
}

fn emit_function(
    out: &mut String,
    function: &ClientFunction,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    out.push_str("function ");
    out.push_str(&local_function_name(function.id));
    out.push('(');
    for (i, param) in function.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&param.name);
    }
    out.push_str(") {\n");

    let mut function_ctx = ctx.clone();
    for (idx, param) in function.params.iter().enumerate() {
        function_ctx = function_ctx.with_local_name(ClientLocalId(idx as u32), param.name.clone());
    }
    emit_block_with_context_and_indent(out, &function.body, &function_ctx, 1)?;
    out.push_str("}\n");
    Ok(())
}

fn emit_closure(
    out: &mut String,
    closure: &ClientSimpleClosure,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    out.push_str("(__c0) => ");
    emit_expr_with_context(out, &closure.body, ctx)
}

fn emit_call(
    out: &mut String,
    function: &ClientFunctionRef,
    args: &[ClientExpr],
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    if ctx.linked_helper_calls.is_some() {
        let raw_reference = match function {
            ClientFunctionRef::Runtime(reference) | ClientFunctionRef::Unresolved(reference) => {
                Some(reference)
            }
            _ => None,
        };
        if let Some(reference) = raw_reference {
            if reference.contains(UNLINKED_HELPER_IDENTIFIER_PREFIX) {
                return Err(ClientEmitError::ReservedUnlinkedFunctionRef(
                    reference.clone(),
                ));
            }
        }
    }

    let linked_target = match function {
        ClientFunctionRef::Helper { edge, .. } => ctx
            .linked_helper_calls
            .as_ref()
            .map(|calls| {
                calls
                    .get(edge)
                    .ok_or(ClientEmitError::UnboundHelperCall(*edge))
            })
            .transpose()?,
        _ => None,
    };
    if let ClientFunctionRef::Helper { edge, .. } = function {
        if let Some(target) = linked_target {
            if args.len() != target.parameter_types().len() {
                return Err(ClientEmitError::LinkedHelperCallArityMismatch {
                    edge: *edge,
                    expected: target.parameter_types().len(),
                    actual: args.len(),
                });
            }
        }
    }

    match function {
        ClientFunctionRef::Local(id) => {
            out.push_str(&local_function_name(*id));
        }
        ClientFunctionRef::Import { module, name } => {
            out.push_str(&client_module_namespace(module));
            out.push('.');
            out.push_str(&bundle_export_name(module, name));
        }
        ClientFunctionRef::Helper { edge, .. } => {
            if let Some(target) = linked_target {
                out.push_str(&target.function().js_name());
            } else {
                // Descriptor/debug emission deliberately remains
                // non-deployable. Final linked emission consumes the typed
                // edge mapping and re-emits this call as a private helper
                // reference; no phase mutates this placeholder text.
                out.push_str(UNLINKED_HELPER_IDENTIFIER_PREFIX);
                out.push_str("helper_edge_");
                out.push_str(&edge.index().to_string());
            }
        }
        ClientFunctionRef::Runtime(name) => out.push_str(name),
        ClientFunctionRef::Unresolved(path) => {
            return Err(ClientEmitError::UnresolvedFunctionRef(path.clone()));
        }
    }
    out.push('(');
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        if let Some(expected) = linked_target.and_then(|target| target.parameter_types().get(i)) {
            emit_expr_with_expected_type(out, arg, expected, ctx)?;
        } else {
            emit_expr_with_context(out, arg, ctx)?;
        }
    }
    out.push(')');
    Ok(())
}

/// Emit an expression in the exact Rust type context supplied by a linked
/// helper parameter.
///
/// Proc-macro analysis deliberately cannot resolve an imported helper's
/// signature. Consequently an unsuffixed literal reaches Client IR as the
/// generic representation-level `Value::Int`, even when ordinary Rust
/// inference selected `i64` or `u64`. Final linking is the first phase that
/// owns both rustc-resolved relocation evidence and the authenticated helper
/// signature, so this is the unique sound place to restore the BigInt
/// representation. Context flows only through source constructs where Rust
/// contextual typing flows too: `Option`'s erased `Some`, array elements, and
/// conditional result arms. Arbitrary Number-producing expressions are never
/// coerced.
fn emit_expr_with_expected_type(
    out: &mut String,
    expression: &ClientExpr,
    expected: &ClientType,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    let expected = option_inner_type(expected);
    match (expression, expected) {
        (ClientExpr::Literal(value), expected) => {
            emit_value_with_expected_type(out, value, expected)
        }
        (ClientExpr::ArrayLit(elements), ClientType::Vec(element_type)) => {
            out.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                emit_value_with_expected_type(out, element, element_type)?;
            }
            out.push(']');
            Ok(())
        }
        (
            ClientExpr::Conditional {
                condition,
                then_expr,
                else_expr,
            },
            expected,
        ) => {
            out.push('(');
            emit_expr_with_context(out, condition, ctx)?;
            out.push_str(" ? ");
            emit_expr_with_expected_type(out, then_expr, expected, ctx)?;
            out.push_str(" : ");
            emit_expr_with_expected_type(out, else_expr, expected, ctx)?;
            out.push(')');
            Ok(())
        }
        _ => emit_expr_with_context(out, expression, ctx),
    }
}

fn emit_value_with_expected_type(
    out: &mut String,
    value: &Value,
    expected: &ClientType,
) -> Result<(), ClientEmitError> {
    let expected = option_inner_type(expected);
    if let Value::Int(value) = value {
        match expected {
            ClientType::I64 => {
                let _ = write!(out, "{value}n");
                return Ok(());
            }
            ClientType::U64 if *value >= 0 => {
                let _ = write!(out, "{value}n");
                return Ok(());
            }
            ClientType::U64 => {
                return Err(ClientEmitError::ContextualIntegerLiteralOutOfRange {
                    value: i128::from(*value),
                    expected: expected.clone(),
                });
            }
            _ => {}
        }
    }
    emit_value(out, value);
    Ok(())
}

fn option_inner_type(mut ty: &ClientType) -> &ClientType {
    while let ClientType::Option(inner) = ty {
        ty = inner;
    }
    ty
}

fn local_function_name(id: ClientFunctionId) -> String {
    format!("__fn_{}", id.0)
}

fn emit_browser_effect(
    out: &mut String,
    effect: &ClientBrowserEffect,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y } => {
            out.push_str("window.scrollTo?.(Number(");
            emit_expr_with_context(out, x, ctx)?;
            out.push_str("), Number(");
            emit_expr_with_context(out, y, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::WindowScrollBy { x, y } => {
            out.push_str("window.scrollBy?.(Number(");
            emit_expr_with_context(out, x, ctx)?;
            out.push_str("), Number(");
            emit_expr_with_context(out, y, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::WindowPrint => {
            out.push_str("window.print?.();\n");
        }
        ClientBrowserEffect::LocationSetHash { value } => {
            out.push_str("window.location.hash = String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(");\n");
        }
        ClientBrowserEffect::LocationAssign { url } => {
            out.push_str("window.location.assign(String(");
            emit_expr_with_context(out, url, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::LocationReplace { url } => {
            out.push_str("window.location.replace(String(");
            emit_expr_with_context(out, url, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::LocationReload => {
            out.push_str("window.location.reload();\n");
        }
        ClientBrowserEffect::HistoryPushState { url } => {
            out.push_str("window.history?.pushState?.(null, \"\", String(");
            emit_expr_with_context(out, url, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::HistoryReplaceState { url } => {
            out.push_str("window.history?.replaceState?.(null, \"\", String(");
            emit_expr_with_context(out, url, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::HistoryBack => {
            out.push_str("window.history?.back?.();\n");
        }
        ClientBrowserEffect::HistoryForward => {
            out.push_str("window.history?.forward?.();\n");
        }
        ClientBrowserEffect::HistoryGo { delta } => {
            out.push_str("window.history?.go?.(Number(");
            emit_expr_with_context(out, delta, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            out.push_str("document.dispatchEvent(new CustomEvent(String(");
            emit_expr_with_context(out, name, ctx)?;
            out.push(')');
            if let Some(detail) = detail {
                out.push_str(", { detail: ");
                emit_expr_with_context(out, detail, ctx)?;
                out.push_str(" }");
            }
            out.push_str("));\n");
        }
        ClientBrowserEffect::DocumentSetTitle { title } => {
            out.push_str("document.title = String(");
            emit_expr_with_context(out, title, ctx)?;
            out.push_str(");\n");
        }
        ClientBrowserEffect::ClipboardWriteText { text } => {
            out.push_str("navigator.clipboard?.writeText?.(String(");
            emit_expr_with_context(out, text, ctx)?;
            out.push_str("));\n");
        }
        ClientBrowserEffect::ClipboardReadTextInto { target, fallback } => {
            out.push_str("{ let __rsjs_clipboard_read = null; try { __rsjs_clipboard_read = navigator.clipboard?.readText?.(); } catch (_) {} if (__rsjs_clipboard_read) { __rsjs_clipboard_read.then((__text) => ");
            out.push_str(&ctx.signal_name(*target));
            out.push_str(".set(String(__text ?? \"\"))).catch(() => ");
            emit_signal_text_fallback(out, *target, fallback.as_ref(), ctx)?;
            out.push_str("); } else { ");
            emit_signal_text_fallback(out, *target, fallback.as_ref(), ctx)?;
            out.push_str("; } }\n");
        }
        ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            target,
            fallback,
        } => emit_webauthn_json_into(
            out,
            "parseCreationOptionsFromJSON",
            "create",
            options_json,
            *target,
            fallback.as_ref(),
            ctx,
        )?,
        ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            target,
            fallback,
        } => emit_webauthn_json_into(
            out,
            "parseRequestOptionsFromJSON",
            "get",
            options_json,
            *target,
            fallback.as_ref(),
            ctx,
        )?,
        ClientBrowserEffect::EventFileTextInto {
            source,
            target,
            fallback,
        } => {
            out.push_str("{ const __rsjs_file = ");
            out.push_str(&event_file_source_js(*source, ctx));
            out.push_str("; let __rsjs_file_text = null; try { __rsjs_file_text = __rsjs_file?.text?.(); } catch (_) {} if (__rsjs_file_text) { __rsjs_file_text.then((__text) => ");
            out.push_str(&ctx.signal_name(*target));
            out.push_str(".set(String(__text ?? \"\"))).catch(() => ");
            emit_signal_text_fallback(out, *target, fallback.as_ref(), ctx)?;
            out.push_str("); } else { ");
            emit_signal_text_fallback(out, *target, fallback.as_ref(), ctx)?;
            out.push_str("; } }\n");
        }
        ClientBrowserEffect::StorageSetItem { area, key, value } => {
            out.push_str("try { window.");
            out.push_str(storage_area_js(*area));
            out.push_str("?.setItem?.(String(");
            emit_expr_with_context(out, key, ctx)?;
            out.push_str("), String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(")); } catch (_) {}\n");
        }
        ClientBrowserEffect::StorageRemoveItem { area, key } => {
            out.push_str("try { window.");
            out.push_str(storage_area_js(*area));
            out.push_str("?.removeItem?.(String(");
            emit_expr_with_context(out, key, ctx)?;
            out.push_str(")); } catch (_) {}\n");
        }
        ClientBrowserEffect::UrlSetSearchParam { mode, key, value } => {
            out.push_str(
                "try { const __url = new URL(window.location.href); __url.searchParams.set(String(",
            );
            emit_expr_with_context(out, key, ctx)?;
            out.push_str("), String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(")); window.history?.");
            out.push_str(url_history_method(*mode));
            out.push_str("?.(null, \"\", __url.href); } catch (_) {}\n");
        }
        ClientBrowserEffect::UrlDeleteSearchParam { mode, key } => {
            out.push_str(
                "try { const __url = new URL(window.location.href); __url.searchParams.delete(String(",
            );
            emit_expr_with_context(out, key, ctx)?;
            out.push_str(")); window.history?.");
            out.push_str(url_history_method(*mode));
            out.push_str("?.(null, \"\", __url.href); } catch (_) {}\n");
        }
        ClientBrowserEffect::Declared { helper, args } => {
            let Some(spec) = crate::browser_registry::find_by_id(helper) else {
                return Err(ClientEmitError::UnsupportedExpr(
                    "declared browser effect with no matching rsjs_ir::browser_registry entry",
                ));
            };
            let crate::browser_registry::HelperLowering::DeclaredEffect { js } = spec.lowering
            else {
                return Err(ClientEmitError::UnsupportedExpr(
                    "declared browser effect whose registry entry is not DeclaredEffect",
                ));
            };
            emit_declared_js_template(out, js, args, ctx)?;
            out.push_str(";\n");
        }
    }
    Ok(())
}

fn emit_node_ref_effect(
    out: &mut String,
    id: NodeRefId,
    effect: &ClientNodeRefEffect,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    let var = ctx.node_ref_name(id);
    match effect {
        ClientNodeRefEffect::Focus => {
            out.push_str(&var);
            out.push_str(".current?.focus?.();\n");
        }
        ClientNodeRefEffect::Blur => {
            out.push_str(&var);
            out.push_str(".current?.blur?.();\n");
        }
        ClientNodeRefEffect::ScrollIntoView => {
            out.push_str(&var);
            out.push_str(".current?.scrollIntoView?.();\n");
        }
        ClientNodeRefEffect::Click => {
            out.push_str(&var);
            out.push_str(".current?.click?.();\n");
        }
        ClientNodeRefEffect::RequestSubmit => {
            out.push_str(&var);
            out.push_str(".current?.requestSubmit?.();\n");
        }
        ClientNodeRefEffect::Reset => {
            out.push_str(&var);
            out.push_str(".current?.reset?.();\n");
        }
        ClientNodeRefEffect::ReportValidity => {
            out.push_str(&var);
            out.push_str(".current?.reportValidity?.();\n");
        }
        ClientNodeRefEffect::ShowModal => {
            out.push_str(&var);
            out.push_str(".current?.showModal?.();\n");
        }
        ClientNodeRefEffect::Close => {
            out.push_str(&var);
            out.push_str(".current?.close?.();\n");
        }
        ClientNodeRefEffect::ShowPopover => {
            out.push_str(&var);
            out.push_str(".current?.showPopover?.();\n");
        }
        ClientNodeRefEffect::HidePopover => {
            out.push_str(&var);
            out.push_str(".current?.hidePopover?.();\n");
        }
        ClientNodeRefEffect::TogglePopover => {
            out.push_str(&var);
            out.push_str(".current?.togglePopover?.();\n");
        }
        ClientNodeRefEffect::ScrollTo { x, y } => {
            out.push_str(&var);
            out.push_str(".current?.scrollTo?.(Number(");
            emit_expr_with_context(out, x, ctx)?;
            out.push_str("), Number(");
            emit_expr_with_context(out, y, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::ScrollBy { x, y } => {
            out.push_str(&var);
            out.push_str(".current?.scrollBy?.(Number(");
            emit_expr_with_context(out, x, ctx)?;
            out.push_str("), Number(");
            emit_expr_with_context(out, y, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::SetPointerCapture(pointer_id) => {
            out.push_str(&var);
            out.push_str(".current?.setPointerCapture?.(Number(");
            emit_expr_with_context(out, pointer_id, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::ReleasePointerCapture(pointer_id) => {
            out.push_str(&var);
            out.push_str(".current?.releasePointerCapture?.(Number(");
            emit_expr_with_context(out, pointer_id, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::Select => {
            out.push_str(&var);
            out.push_str(".current?.select?.();\n");
        }
        ClientNodeRefEffect::SetValue(value) => {
            out.push_str("if (");
            out.push_str(&var);
            out.push_str(".current != null) ");
            out.push_str(&var);
            out.push_str(".current.value = ");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(";\n");
        }
        ClientNodeRefEffect::SetChecked(value) => {
            out.push_str("if (");
            out.push_str(&var);
            out.push_str(".current != null) ");
            out.push_str(&var);
            out.push_str(".current.checked = !!(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(");\n");
        }
        ClientNodeRefEffect::SetCustomValidity(value) => {
            out.push_str(&var);
            out.push_str(".current?.setCustomValidity?.(String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::SetSelectionRange(start, end) => {
            out.push_str(&var);
            out.push_str(".current?.setSelectionRange?.(Number(");
            emit_expr_with_context(out, start, ctx)?;
            out.push_str("), Number(");
            emit_expr_with_context(out, end, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::SetTextContent(value) => {
            out.push_str("if (");
            out.push_str(&var);
            out.push_str(".current != null) ");
            out.push_str(&var);
            out.push_str(".current.textContent = ");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(";\n");
        }
        ClientNodeRefEffect::AddClass(value) => {
            out.push_str(&var);
            out.push_str(".current?.classList?.add(String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::RemoveClass(value) => {
            out.push_str(&var);
            out.push_str(".current?.classList?.remove(String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::ToggleClass(value) => {
            out.push_str(&var);
            out.push_str(".current?.classList?.toggle(String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::SetAttribute { name, value } => {
            out.push_str(&var);
            out.push_str(".current?.setAttribute?.(");
            emit_value(out, &Value::String(name.clone()));
            out.push_str(", String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str("));\n");
        }
        ClientNodeRefEffect::RemoveAttribute { name } => {
            out.push_str(&var);
            out.push_str(".current?.removeAttribute?.(");
            emit_value(out, &Value::String(name.clone()));
            out.push_str(");\n");
        }
        ClientNodeRefEffect::SetStyleProperty { name, value } => {
            out.push_str(&var);
            out.push_str(".current?.style?.setProperty(");
            emit_value(out, &Value::String(name.clone()));
            out.push_str(", String(");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str("));\n");
        }
    }
    Ok(())
}

fn emit_encoding(
    out: &mut String,
    op: EncodingOp,
    value: &ClientExpr,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    match op {
        EncodingOp::Utf8Bytes => {
            out.push_str("(new TextEncoder().encode(String((");
            emit_expr_with_context(out, value, ctx)?;
            out.push_str(") ?? \"\")))");
        }
        EncodingOp::Utf8String => {
            out.push_str("((__v) => new TextDecoder().decode(__v instanceof ArrayBuffer ? new Uint8Array(__v) : new Uint8Array(__v ?? [])))(");
            emit_expr_with_context(out, value, ctx)?;
            out.push(')');
        }
        EncodingOp::Base64Encode => emit_base64_encode(out, value, ctx, false)?,
        EncodingOp::Base64UrlEncode => emit_base64_encode(out, value, ctx, true)?,
        EncodingOp::Base64Decode => emit_base64_decode(out, value, ctx, false)?,
        EncodingOp::Base64UrlDecode => emit_base64_decode(out, value, ctx, true)?,
    }
    Ok(())
}

// proa-perf-ok(scope): trusted low-level JavaScript source emitter, not SSR HTML rendering
fn emit_base64_encode(
    out: &mut String,
    value: &ClientExpr,
    ctx: &ClientEmitContext,
    url: bool,
) -> Result<(), ClientEmitError> {
    out.push_str("((__v) => { const __b = __v instanceof ArrayBuffer ? new Uint8Array(__v) : new Uint8Array(__v ?? []); let __s = \"\"; for (let __i = 0; __i < __b.length; __i += 32768) { __s += String.fromCharCode(...__b.subarray(__i, __i + 32768)); } const __r = btoa(__s); return ");
    if url {
        out.push_str("__r.replace(/\\+/g, \"-\").replace(/\\//g, \"_\").replace(/=+$/g, \"\")");
    } else {
        out.push_str("__r");
    }
    out.push_str("; })(");
    emit_expr_with_context(out, value, ctx)?;
    out.push(')');
    Ok(())
}

fn emit_base64_decode(
    out: &mut String,
    value: &ClientExpr,
    ctx: &ClientEmitContext,
    url: bool,
) -> Result<(), ClientEmitError> {
    if url {
        out.push_str("((__s) => { try { let __v = String(__s ?? \"\").replace(/-/g, \"+\").replace(/_/g, \"/\"); const __pad = __v.length % 4; if (__pad) __v += \"=\".repeat(4 - __pad); return Uint8Array.from(atob(__v), (__c) => __c.charCodeAt(0)); } catch (_) { return new Uint8Array(); } })(");
    } else {
        out.push_str("((__s) => { try { return Uint8Array.from(atob(String(__s ?? \"\")), (__c) => __c.charCodeAt(0)); } catch (_) { return new Uint8Array(); } })(");
    }
    emit_expr_with_context(out, value, ctx)?;
    out.push(')');
    Ok(())
}

fn emit_browser_read(
    out: &mut String,
    read: &ClientBrowserRead,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    match read {
        ClientBrowserRead::Window(field) => {
            out.push_str("((typeof window !== \"undefined\") ? (window.");
            out.push_str(window_field_js(*field));
            out.push_str(" ?? 0) : 0)");
        }
        ClientBrowserRead::Location(field) => {
            out.push_str(
                "((typeof window !== \"undefined\" && window.location) ? (window.location.",
            );
            out.push_str(location_field_js(*field));
            out.push_str(" ?? \"\") : \"\")");
        }
        ClientBrowserRead::Storage {
            area,
            value_type,
            key,
            fallback,
        } => {
            out.push_str("(() => { try { const __v = window.");
            out.push_str(browser_storage_area_js(*area));
            out.push_str("?.getItem?.(String(");
            emit_expr_with_context(out, key, ctx)?;
            out.push_str(")); ");
            emit_storage_read_return(out, *value_type, fallback.as_deref(), ctx)?;
            out.push_str(" } catch (_) { return ");
            emit_storage_fallback(out, *value_type, fallback.as_deref(), ctx)?;
            out.push_str("; } })()");
        }
        ClientBrowserRead::Declared { helper, args } => {
            let Some(spec) = crate::browser_registry::find_by_id(helper) else {
                return Err(ClientEmitError::UnsupportedExpr(
                    "declared browser read with no matching rsjs_ir::browser_registry entry",
                ));
            };
            let crate::browser_registry::HelperLowering::DeclaredRead { js } = spec.lowering else {
                return Err(ClientEmitError::UnsupportedExpr(
                    "declared browser read whose registry entry is not DeclaredRead",
                ));
            };
            emit_declared_js_template(out, js, args, ctx)?;
        }
        ClientBrowserRead::UrlSearchParam { key, fallback } => {
            out.push_str("(() => { try { const __params = new URLSearchParams((typeof window !== \"undefined\" && window.location) ? (window.location.search ?? \"\") : \"\"); const __v = __params.get(String(");
            emit_expr_with_context(out, key, ctx)?;
            out.push_str(")); return __v == null ? String(");
            if let Some(fallback) = fallback {
                emit_expr_with_context(out, fallback, ctx)?;
            } else {
                out.push_str("\"\"");
            }
            out.push_str(") : __v; } catch (_) { return String(");
            if let Some(fallback) = fallback {
                emit_expr_with_context(out, fallback, ctx)?;
            } else {
                out.push_str("\"\"");
            }
            out.push_str("); } })()");
        }
    }
    Ok(())
}

/// Splice a registry `DeclaredRead` JS template, replacing `$0`..`$9`
/// placeholders with the corresponding lowered argument expression.
fn emit_declared_js_template(
    out: &mut String,
    template: &str,
    args: &[ClientExpr],
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            if let Some(digit) = chars.peek().and_then(|d| d.to_digit(10)) {
                chars.next();
                let Some(arg) = args.get(digit as usize) else {
                    return Err(ClientEmitError::UnsupportedExpr(
                        "declared browser read template references a missing argument",
                    ));
                };
                emit_expr_with_context(out, arg, ctx)?;
                continue;
            }
        }
        out.push(c);
    }
    Ok(())
}

fn emit_event_collection_len(
    out: &mut String,
    collection: EventCollection,
    ctx: &ClientEmitContext,
) {
    out.push('(');
    out.push_str(&event_collection_js(collection, ctx));
    out.push_str("?.length ?? 0)");
}

fn emit_event_collection_read(
    out: &mut String,
    collection: EventCollection,
    index: &ClientExpr,
    field: EventCollectionField,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    out.push('(');
    out.push_str(&event_collection_js(collection, ctx));
    out.push_str("?.[");
    emit_event_collection_index(out, index, ctx)?;
    out.push_str("]?.");
    out.push_str(event_collection_field_js(field));
    out.push_str(" ?? ");
    out.push_str(event_collection_field_fallback_js(field));
    out.push(')');
    Ok(())
}

fn emit_event_collection_index(
    out: &mut String,
    index: &ClientExpr,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    out.push_str("Math.trunc(Number(");
    emit_expr_with_context(out, index, ctx)?;
    out.push_str(") || 0)");
    Ok(())
}

fn event_collection_js(collection: EventCollection, ctx: &ClientEmitContext) -> String {
    let event = &ctx.event_name;
    match collection {
        EventCollection::ChangedTouches => format!("{event}.changedTouches"),
        EventCollection::TargetTouches => format!("{event}.targetTouches"),
        EventCollection::Touches => format!("{event}.touches"),
        EventCollection::TargetFiles => format!("{event}.target?.files"),
        EventCollection::DataTransferFiles => format!("{event}.dataTransfer?.files"),
        EventCollection::DataTransferItems => format!("{event}.dataTransfer?.items"),
    }
}

fn event_collection_field_js(field: EventCollectionField) -> &'static str {
    match field {
        EventCollectionField::TouchIdentifier => "identifier",
        EventCollectionField::TouchClientX => "clientX",
        EventCollectionField::TouchClientY => "clientY",
        EventCollectionField::TouchPageX => "pageX",
        EventCollectionField::TouchPageY => "pageY",
        EventCollectionField::TouchScreenX => "screenX",
        EventCollectionField::TouchScreenY => "screenY",
        EventCollectionField::FileName => "name",
        EventCollectionField::FileType => "type",
        EventCollectionField::FileSize => "size",
        EventCollectionField::DataTransferItemKind => "kind",
        EventCollectionField::DataTransferItemType => "type",
    }
}

fn event_collection_field_fallback_js(field: EventCollectionField) -> &'static str {
    match field {
        EventCollectionField::TouchIdentifier
        | EventCollectionField::TouchClientX
        | EventCollectionField::TouchClientY
        | EventCollectionField::TouchPageX
        | EventCollectionField::TouchPageY
        | EventCollectionField::TouchScreenX
        | EventCollectionField::TouchScreenY
        | EventCollectionField::FileSize => "0",
        EventCollectionField::FileName
        | EventCollectionField::FileType
        | EventCollectionField::DataTransferItemKind
        | EventCollectionField::DataTransferItemType => "\"\"",
    }
}

fn emit_event_attribute(
    out: &mut String,
    source: EventAttributeSource,
    attr: &str,
    ctx: &ClientEmitContext,
) {
    let event = &ctx.event_name;
    match source {
        EventAttributeSource::Target => {
            let _ = write!(out, "({event}.target?.getAttribute?.(");
            emit_value(out, &Value::String(attr.to_string()));
            out.push_str(") ?? \"\")");
        }
        EventAttributeSource::Closest => {
            let _ = write!(out, "({event}.target?.closest?.(\"[");
            out.push_str(&js_string_inner(attr));
            out.push_str("]\")?.getAttribute(");
            emit_value(out, &Value::String(attr.to_string()));
            out.push_str(") ?? \"\")");
        }
        EventAttributeSource::CurrentTarget => {
            let _ = write!(out, "({event}.__rsjsCurrentTarget?.getAttribute?.(");
            emit_value(out, &Value::String(attr.to_string()));
            out.push_str(") ?? \"\")");
        }
    }
}

fn emit_node_ref_read(
    out: &mut String,
    id: NodeRefId,
    accessor: &NodeRefAccessor,
    ctx: &ClientEmitContext,
) {
    out.push('(');
    out.push_str(&ctx.node_ref_name(id));
    emit_dom_read_suffix(out, accessor, ".current");
}

fn emit_dom_read_suffix(out: &mut String, accessor: &NodeRefAccessor, prefix: &str) {
    out.push_str(prefix);
    match accessor {
        NodeRefAccessor::Id => out.push_str("?.id ?? \"\")"),
        NodeRefAccessor::TextContent => out.push_str("?.textContent ?? \"\")"),
        NodeRefAccessor::Value => out.push_str("?.value ?? \"\")"),
        NodeRefAccessor::Checked => out.push_str("?.checked ?? false)"),
        NodeRefAccessor::CheckValidity => out.push_str("?.checkValidity?.() ?? true)"),
        NodeRefAccessor::ValidationMessage => out.push_str("?.validationMessage ?? \"\")"),
        NodeRefAccessor::ValidityValid => out.push_str("?.validity?.valid ?? true)"),
        NodeRefAccessor::WillValidate => out.push_str("?.willValidate ?? false)"),
        NodeRefAccessor::SelectionStart => out.push_str("?.selectionStart ?? 0)"),
        NodeRefAccessor::SelectionEnd => out.push_str("?.selectionEnd ?? 0)"),
        NodeRefAccessor::ScrollTop => out.push_str("?.scrollTop ?? 0)"),
        NodeRefAccessor::ScrollLeft => out.push_str("?.scrollLeft ?? 0)"),
        NodeRefAccessor::ClientWidth => out.push_str("?.clientWidth ?? 0)"),
        NodeRefAccessor::ClientHeight => out.push_str("?.clientHeight ?? 0)"),
        NodeRefAccessor::ScrollWidth => out.push_str("?.scrollWidth ?? 0)"),
        NodeRefAccessor::ScrollHeight => out.push_str("?.scrollHeight ?? 0)"),
        NodeRefAccessor::OffsetWidth => out.push_str("?.offsetWidth ?? 0)"),
        NodeRefAccessor::OffsetHeight => out.push_str("?.offsetHeight ?? 0)"),
        NodeRefAccessor::RectX => emit_dom_rect_read(out, "x"),
        NodeRefAccessor::RectY => emit_dom_rect_read(out, "y"),
        NodeRefAccessor::RectTop => emit_dom_rect_read(out, "top"),
        NodeRefAccessor::RectRight => emit_dom_rect_read(out, "right"),
        NodeRefAccessor::RectBottom => emit_dom_rect_read(out, "bottom"),
        NodeRefAccessor::RectLeft => emit_dom_rect_read(out, "left"),
        NodeRefAccessor::RectWidth => emit_dom_rect_read(out, "width"),
        NodeRefAccessor::RectHeight => emit_dom_rect_read(out, "height"),
        NodeRefAccessor::FileCount => out.push_str("?.files?.length ?? 0)"),
        NodeRefAccessor::FirstFileName => out.push_str("?.files?.[0]?.name ?? \"\")"),
        NodeRefAccessor::FirstFileType => out.push_str("?.files?.[0]?.type ?? \"\")"),
        NodeRefAccessor::FirstFileSize => out.push_str("?.files?.[0]?.size ?? 0)"),
        NodeRefAccessor::Attribute { name } => {
            out.push_str("?.getAttribute?.(");
            emit_value(out, &Value::String(name.clone()));
            out.push_str(") ?? \"\")");
        }
        NodeRefAccessor::HasAttribute { name } => {
            out.push_str("?.hasAttribute?.(");
            emit_value(out, &Value::String(name.clone()));
            out.push_str(") ?? false)");
        }
    }
}

fn emit_dom_rect_read(out: &mut String, field: &str) {
    out.push_str("?.getBoundingClientRect?.()?.");
    out.push_str(field);
    out.push_str(" ?? 0)");
}

fn emit_event_target_within_node_ref(
    out: &mut String,
    id: NodeRefId,
    inside: bool,
    ctx: &ClientEmitContext,
) {
    if !inside {
        out.push('!');
    }
    out.push_str("((");
    out.push_str(&ctx.node_ref_name(id));
    out.push_str(".current?.contains?.(");
    out.push_str(&ctx.event_name);
    out.push_str(".target)) ?? false)");
}

fn emit_event_payload(out: &mut String, attr: &str, ctx: &ClientEmitContext) {
    out.push_str("JSON.parse(((");
    out.push_str(&ctx.event_name);
    out.push_str(".__rsjsOwnerTarget?.getAttribute?.('");
    out.push_str(&js_string_inner(attr));
    out.push_str("')) ?? (");
    out.push_str(&ctx.event_name);
    out.push_str(".__rsjsCurrentTarget?.getAttribute?.('");
    out.push_str(&js_string_inner(attr));
    out.push_str("')) ?? (");
    out.push_str(&ctx.event_name);
    out.push_str(".target?.closest?.('[");
    out.push_str(&js_string_inner(attr));
    out.push_str("]')?.getAttribute('");
    out.push_str(&js_string_inner(attr));
    out.push_str("'))) ?? \"null\")");
}

fn emit_node_payload(out: &mut String, attr: &str, ctx: &ClientEmitContext) {
    let Some(node) = ctx.node_name.as_deref() else {
        out.push_str("null");
        return;
    };
    out.push_str("JSON.parse((");
    out.push_str(node);
    out.push_str("?.getAttribute('");
    out.push_str(&js_string_inner(attr));
    out.push_str("')) ?? \"null\")");
}

fn emit_value(out: &mut String, value: &Value) {
    match value {
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Int(value) => {
            let _ = write!(out, "{value}");
        }
        // BigInt literal: exact integer semantics past 2^53. Mixing
        // BigInt and Number in JS arithmetic throws, which the
        // analyzer prevents by promoting sibling Int literals.
        Value::BigInt(value) => {
            let _ = write!(out, "{value}n");
        }
        Value::Float(value) => emit_float(out, *value),
        Value::String(value) => {
            out.push('"');
            out.push_str(&js_string_inner(value));
            out.push('"');
        }
        Value::Null => out.push_str("null"),
    }
}

fn emit_float(out: &mut String, value: f64) {
    if value.is_nan() {
        out.push_str("NaN");
    } else if value.is_infinite() {
        out.push_str(if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        });
    } else {
        let value = format!("{value}");
        out.push_str(&value);
        if !value.contains('.') && !value.contains('e') && !value.contains('E') {
            out.push_str(".0");
        }
    }
}

fn emit_event_value(
    out: &mut String,
    value: EventValue,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    let event = &ctx.event_name;
    match value {
        EventValue::Type => {
            let _ = write!(out, "({event}.type ?? \"\")");
        }
        EventValue::TargetValue => {
            let _ = write!(out, "({event}.target?.value ?? \"\")");
        }
        EventValue::TargetChecked => {
            let _ = write!(out, "!!{event}.target?.checked");
        }
        EventValue::TargetDate => {
            let _ = write!(
                out,
                "({event}.target?.closest?.('[data-date]')?.getAttribute('data-date') ?? \"\")"
            );
        }
        EventValue::TargetId => {
            let _ = write!(out, "({event}.target?.id ?? \"\")");
        }
        EventValue::CurrentTargetId => {
            let _ = write!(out, "({event}.__rsjsCurrentTarget?.id ?? \"\")");
        }
        EventValue::CurrentTargetValue => {
            let _ = write!(out, "({event}.__rsjsCurrentTarget?.value ?? \"\")");
        }
        EventValue::CurrentTargetChecked => {
            let _ = write!(out, "!!{event}.__rsjsCurrentTarget?.checked");
        }
        EventValue::Key => {
            let _ = write!(out, "({event}.key ?? \"\")");
        }
        EventValue::Code => {
            let _ = write!(out, "({event}.code ?? \"\")");
        }
        EventValue::Repeat => {
            let _ = write!(out, "!!{event}.repeat");
        }
        EventValue::ShiftKey => {
            let _ = write!(out, "!!{event}.shiftKey");
        }
        EventValue::MetaKey => {
            let _ = write!(out, "!!{event}.metaKey");
        }
        EventValue::CtrlKey => {
            let _ = write!(out, "!!{event}.ctrlKey");
        }
        EventValue::AltKey => {
            let _ = write!(out, "!!{event}.altKey");
        }
        EventValue::ClientX => {
            let _ = write!(out, "({event}.clientX ?? 0)");
        }
        EventValue::ClientY => {
            let _ = write!(out, "({event}.clientY ?? 0)");
        }
        EventValue::PageX => {
            let _ = write!(out, "({event}.pageX ?? 0)");
        }
        EventValue::PageY => {
            let _ = write!(out, "({event}.pageY ?? 0)");
        }
        EventValue::ScreenX => {
            let _ = write!(out, "({event}.screenX ?? 0)");
        }
        EventValue::ScreenY => {
            let _ = write!(out, "({event}.screenY ?? 0)");
        }
        EventValue::OffsetX => {
            let _ = write!(out, "({event}.offsetX ?? 0)");
        }
        EventValue::OffsetY => {
            let _ = write!(out, "({event}.offsetY ?? 0)");
        }
        EventValue::MovementX => {
            let _ = write!(out, "({event}.movementX ?? 0)");
        }
        EventValue::MovementY => {
            let _ = write!(out, "({event}.movementY ?? 0)");
        }
        EventValue::Button => {
            let _ = write!(out, "({event}.button ?? 0)");
        }
        EventValue::Buttons => {
            let _ = write!(out, "({event}.buttons ?? 0)");
        }
        EventValue::Detail => {
            let _ = write!(out, "({event}.detail ?? 0)");
        }
        EventValue::DetailU32 => {
            let _ = write!(out, "(({event}.detail ?? 0) >>> 0)");
        }
        EventValue::PointerId => {
            let _ = write!(out, "({event}.pointerId ?? 0)");
        }
        EventValue::PointerType => {
            let _ = write!(out, "({event}.pointerType ?? \"\")");
        }
        EventValue::IsPrimary => {
            let _ = write!(out, "!!{event}.isPrimary");
        }
        EventValue::Pressure => {
            let _ = write!(out, "({event}.pressure ?? 0)");
        }
        EventValue::TangentialPressure => {
            let _ = write!(out, "({event}.tangentialPressure ?? 0)");
        }
        EventValue::TiltX => {
            let _ = write!(out, "({event}.tiltX ?? 0)");
        }
        EventValue::TiltY => {
            let _ = write!(out, "({event}.tiltY ?? 0)");
        }
        EventValue::Twist => {
            let _ = write!(out, "({event}.twist ?? 0)");
        }
        EventValue::PointerWidth => {
            let _ = write!(out, "({event}.width ?? 0)");
        }
        EventValue::PointerHeight => {
            let _ = write!(out, "({event}.height ?? 0)");
        }
        EventValue::DeltaX => {
            let _ = write!(out, "({event}.deltaX ?? 0)");
        }
        EventValue::DeltaY => {
            let _ = write!(out, "({event}.deltaY ?? 0)");
        }
        EventValue::DeltaZ => {
            let _ = write!(out, "({event}.deltaZ ?? 0)");
        }
        EventValue::DeltaMode => {
            let _ = write!(out, "({event}.deltaMode ?? 0)");
        }
        EventValue::CurrentTargetScrollTop => {
            let _ = write!(out, "({event}.__rsjsCurrentTarget?.scrollTop ?? 0)");
        }
        EventValue::CurrentTargetScrollLeft => {
            let _ = write!(out, "({event}.__rsjsCurrentTarget?.scrollLeft ?? 0)");
        }
        EventValue::InputType => {
            let _ = write!(out, "({event}.inputType ?? \"\")");
        }
        EventValue::Data => {
            let _ = write!(out, "({event}.data ?? \"\")");
        }
        EventValue::OldState => {
            let _ = write!(out, "({event}.oldState ?? \"\")");
        }
        EventValue::NewState => {
            let _ = write!(out, "({event}.newState ?? \"\")");
        }
        EventValue::AnimationName => {
            let _ = write!(out, "({event}.animationName ?? \"\")");
        }
        EventValue::PropertyName => {
            let _ = write!(out, "({event}.propertyName ?? \"\")");
        }
        EventValue::ElapsedTime => {
            let _ = write!(out, "({event}.elapsedTime ?? 0)");
        }
        EventValue::PseudoElement => {
            let _ = write!(out, "({event}.pseudoElement ?? \"\")");
        }
        EventValue::StorageKey => {
            let _ = write!(out, "({event}.key ?? \"\")");
        }
        EventValue::StorageOldValue => {
            let _ = write!(out, "({event}.oldValue ?? \"\")");
        }
        EventValue::StorageNewValue => {
            let _ = write!(out, "({event}.newValue ?? \"\")");
        }
        EventValue::StorageUrl => {
            let _ = write!(out, "({event}.url ?? \"\")");
        }
        EventValue::StorageArea => {
            let _ = write!(
                out,
                "(() => {{ try {{ return {event}.storageArea === window.localStorage ? \"local\" : ({event}.storageArea === window.sessionStorage ? \"session\" : \"\"); }} catch (_) {{ return \"\"; }} }})()"
            );
        }
        EventValue::OldUrl => {
            let _ = write!(out, "({event}.oldURL ?? \"\")");
        }
        EventValue::NewUrl => {
            let _ = write!(out, "({event}.newURL ?? \"\")");
        }
        EventValue::ClipboardText => {
            let _ = write!(
                out,
                "({event}.clipboardData?.getData?.(\"text/plain\") ?? \"\")"
            );
        }
        EventValue::DataTransferText => {
            let _ = write!(
                out,
                "({event}.dataTransfer?.getData?.(\"text/plain\") ?? \"\")"
            );
        }
        EventValue::DataTransferFileCount => {
            let _ = write!(out, "({event}.dataTransfer?.files?.length ?? 0)");
        }
        EventValue::DataTransferFirstFileName => {
            let _ = write!(out, "({event}.dataTransfer?.files?.[0]?.name ?? \"\")");
        }
        EventValue::DataTransferFirstFileType => {
            let _ = write!(out, "({event}.dataTransfer?.files?.[0]?.type ?? \"\")");
        }
        EventValue::DataTransferFirstFileSize => {
            let _ = write!(out, "({event}.dataTransfer?.files?.[0]?.size ?? 0)");
        }
        EventValue::TargetFileCount => {
            let _ = write!(out, "({event}.target?.files?.length ?? 0)");
        }
        EventValue::TargetFirstFileName => {
            let _ = write!(out, "({event}.target?.files?.[0]?.name ?? \"\")");
        }
        EventValue::TargetFirstFileType => {
            let _ = write!(out, "({event}.target?.files?.[0]?.type ?? \"\")");
        }
        EventValue::TargetFirstFileSize => {
            let _ = write!(out, "({event}.target?.files?.[0]?.size ?? 0)");
        }
        EventValue::ChangedTouchIdentifier => emit_changed_touch_value(out, event, "identifier"),
        EventValue::ChangedTouchClientX => emit_changed_touch_value(out, event, "clientX"),
        EventValue::ChangedTouchClientY => emit_changed_touch_value(out, event, "clientY"),
        EventValue::ChangedTouchPageX => emit_changed_touch_value(out, event, "pageX"),
        EventValue::ChangedTouchPageY => emit_changed_touch_value(out, event, "pageY"),
        EventValue::ChangedTouchScreenX => emit_changed_touch_value(out, event, "screenX"),
        EventValue::ChangedTouchScreenY => emit_changed_touch_value(out, event, "screenY"),
    }
    Ok(())
}

fn emit_changed_touch_value(out: &mut String, event: &str, field: &str) {
    let _ = write!(out, "({event}.changedTouches?.[0]?.{field} ?? 0)");
}

fn unary_op_js(op: ClientUnaryOp) -> &'static str {
    match op {
        ClientUnaryOp::Not => "!",
        ClientUnaryOp::Neg => "-",
        // Emitted through `checked_int_neg(...)` before this table.
        ClientUnaryOp::CheckedNeg(_) => "-",
    }
}

fn binary_op_js(op: ClientBinaryOp) -> &'static str {
    match op {
        ClientBinaryOp::Add => "+",
        ClientBinaryOp::Sub => "-",
        ClientBinaryOp::Mul => "*",
        ClientBinaryOp::Div => "/",
        // The Math.trunc wrapper is added at the Binary emission site;
        // the operator itself is still a plain quotient.
        ClientBinaryOp::DivInt => "/",
        ClientBinaryOp::Rem => "%",
        ClientBinaryOp::Eq => "===",
        ClientBinaryOp::Ne => "!==",
        ClientBinaryOp::Lt => "<",
        ClientBinaryOp::Le => "<=",
        ClientBinaryOp::Gt => ">",
        ClientBinaryOp::Ge => ">=",
        ClientBinaryOp::And => "&&",
        ClientBinaryOp::Or => "||",
        // Emitted through `checked_int_*` before this table.
        ClientBinaryOp::Checked { op, .. } => match op {
            super::CheckedIntegerOp::Add => "+",
            super::CheckedIntegerOp::Sub => "-",
            super::CheckedIntegerOp::Mul => "*",
            super::CheckedIntegerOp::Div => "/",
            super::CheckedIntegerOp::Rem => "%",
        },
        // Unreachable: the `Binary` emission site branches on
        // `dyn_helper_js()` before it asks for an infix spelling. The arms
        // exist so this table stays total, and they name the operator the
        // helper implements so a future caller cannot silently get `+`.
        ClientBinaryOp::DynAdd => "+",
        ClientBinaryOp::DynSub => "-",
        ClientBinaryOp::DynMul => "*",
        ClientBinaryOp::DynDiv | ClientBinaryOp::DynDivInt => "/",
        ClientBinaryOp::DynRem => "%",
        ClientBinaryOp::DynEq => "==",
        ClientBinaryOp::DynNe => "!=",
    }
}

fn string_predicate_method_js(op: StringPredicateOp) -> &'static str {
    match op {
        StringPredicateOp::StartsWith => "startsWith",
        StringPredicateOp::EndsWith => "endsWith",
    }
}

fn console_level_js(level: ConsoleLevel) -> &'static str {
    match level {
        ConsoleLevel::Log => "log",
        ConsoleLevel::Info => "info",
        ConsoleLevel::Debug => "debug",
        ConsoleLevel::Warn => "warn",
        ConsoleLevel::Error => "error",
        ConsoleLevel::Table => "table",
        ConsoleLevel::Time => "time",
        ConsoleLevel::TimeEnd => "timeEnd",
        ConsoleLevel::Group => "group",
        ConsoleLevel::GroupEnd => "groupEnd",
    }
}

fn query_accessor_js(accessor: QueryAccessor) -> &'static str {
    match accessor {
        QueryAccessor::Data => "data",
        QueryAccessor::Status => "status",
        QueryAccessor::IsLoading => "is_loading",
        QueryAccessor::IsSuccess => "is_success",
        QueryAccessor::IsError => "is_error",
        QueryAccessor::IsRefetching => "is_refetching",
        QueryAccessor::IsStale => "is_stale",
        QueryAccessor::Error => "error",
        QueryAccessor::DataUpdatedAt => "data_updated_at",
        QueryAccessor::ErrorUpdatedAt => "error_updated_at",
        QueryAccessor::FetchCount => "fetch_count",
        QueryAccessor::DownloadLoaded => "download_loaded",
        QueryAccessor::DownloadTotal => "download_total",
        QueryAccessor::DownloadPercent => "download_percent",
    }
}

fn mutation_accessor_js(accessor: MutationAccessor) -> &'static str {
    match accessor {
        MutationAccessor::Status => "status",
        MutationAccessor::IsIdle => "is_idle",
        MutationAccessor::IsPending => "is_pending",
        MutationAccessor::IsSuccess => "is_success",
        MutationAccessor::IsError => "is_error",
        MutationAccessor::Data => "data",
        MutationAccessor::Error => "error",
    }
}

fn as_type_js_helper(ty: AsType) -> &'static str {
    match ty {
        AsType::Str => "__as_str",
        AsType::F64 => "__as_f64",
        AsType::F64FromInt => unreachable!("exact integer conversion is emitted inline"),
        AsType::I64 => "__as_i64",
        AsType::Bool => "__as_bool",
        AsType::Array => "__as_array",
        AsType::Object => "__as_object",
    }
}

fn url_history_method(mode: UrlHistoryMode) -> &'static str {
    match mode {
        UrlHistoryMode::Replace => "replaceState",
        UrlHistoryMode::Push => "pushState",
    }
}

fn storage_area_js(area: StorageArea) -> &'static str {
    match area {
        StorageArea::Local => "localStorage",
        StorageArea::Session => "sessionStorage",
    }
}

fn browser_storage_area_js(area: BrowserStorageArea) -> &'static str {
    match area {
        BrowserStorageArea::Local => "localStorage",
        BrowserStorageArea::Session => "sessionStorage",
    }
}

fn window_field_js(field: WindowField) -> &'static str {
    match field {
        WindowField::InnerWidth => "innerWidth",
        WindowField::InnerHeight => "innerHeight",
        WindowField::ScrollX => "scrollX",
        WindowField::ScrollY => "scrollY",
    }
}

fn location_field_js(field: LocationField) -> &'static str {
    match field {
        LocationField::Href => "href",
        LocationField::Origin => "origin",
        LocationField::Protocol => "protocol",
        LocationField::Host => "host",
        LocationField::Hostname => "hostname",
        LocationField::Port => "port",
        LocationField::Pathname => "pathname",
        LocationField::Search => "search",
        LocationField::Hash => "hash",
    }
}

// proa-perf-ok(scope): trusted low-level JavaScript source emitter, not SSR HTML rendering
fn emit_storage_read_return(
    out: &mut String,
    value_type: StorageValueType,
    fallback: Option<&ClientExpr>,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    match value_type {
        StorageValueType::String => {
            out.push_str("return __v == null ? ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push_str(" : __v;");
        }
        StorageValueType::Bool => {
            out.push_str("if (__v == null) return ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push_str("; const __parsed = JSON.parse(__v); return typeof __parsed === \"boolean\" ? __parsed : ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push(';');
        }
        StorageValueType::I64 => {
            out.push_str("if (__v == null) return ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push_str("; if (!/^(?:0|-?[1-9][0-9]*)$/.test(__v)) return ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push_str("; const __parsed = BigInt(__v); return __parsed >= -9223372036854775808n && __parsed <= 9223372036854775807n ? __parsed : ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push(';');
        }
        StorageValueType::F64 => {
            out.push_str("if (__v == null) return ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push_str("; const __parsed = JSON.parse(__v); return typeof __parsed === \"number\" && Number.isFinite(__parsed) ? __parsed : ");
            emit_storage_fallback(out, value_type, fallback, ctx)?;
            out.push(';');
        }
    }
    Ok(())
}

fn emit_storage_fallback(
    out: &mut String,
    value_type: StorageValueType,
    fallback: Option<&ClientExpr>,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    match value_type {
        StorageValueType::String => {
            out.push_str("String(");
            if let Some(fallback) = fallback {
                emit_expr_with_context(out, fallback, ctx)?;
            } else {
                out.push_str("\"\"");
            }
            out.push(')');
        }
        StorageValueType::Bool => {
            if let Some(fallback) = fallback {
                emit_expr_with_context(out, fallback, ctx)?;
            } else {
                out.push_str("false");
            }
        }
        StorageValueType::I64 => {
            if let Some(fallback) = fallback {
                emit_expr_with_context(out, fallback, ctx)?;
            } else {
                out.push_str("0n");
            }
        }
        StorageValueType::F64 => {
            if let Some(fallback) = fallback {
                emit_expr_with_context(out, fallback, ctx)?;
            } else {
                out.push_str("0.0");
            }
        }
    }
    Ok(())
}

fn event_file_source_js(source: EventFileSource, ctx: &ClientEmitContext) -> String {
    match source {
        EventFileSource::Target => format!("{}.target?.files?.[0]", ctx.event_name),
        EventFileSource::DataTransfer => format!("{}.dataTransfer?.files?.[0]", ctx.event_name),
    }
}

fn emit_webauthn_json_into(
    out: &mut String,
    parse_method: &str,
    credentials_method: &str,
    options_json: &ClientExpr,
    target: SignalId,
    fallback: Option<&ClientExpr>,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    out.push_str("{ let __rsjs_webauthn = null; try { const __rsjs_webauthn_options = globalThis.PublicKeyCredential?.");
    out.push_str(parse_method);
    out.push_str("?.(JSON.parse(String(");
    emit_expr_with_context(out, options_json, ctx)?;
    out.push_str(
        "))); if (__rsjs_webauthn_options) { __rsjs_webauthn = globalThis.navigator?.credentials?.",
    );
    out.push_str(credentials_method);
    out.push_str("?.({ publicKey: __rsjs_webauthn_options }); } } catch (_) {} if (__rsjs_webauthn) { __rsjs_webauthn.then((__cred) => { try { const __json = __cred?.toJSON?.(); if (__json == null) { ");
    emit_signal_text_fallback(out, target, fallback, ctx)?;
    out.push_str("; } else { ");
    out.push_str(&ctx.signal_name(target));
    out.push_str(".set(JSON.stringify(__json)); } } catch (_) { ");
    emit_signal_text_fallback(out, target, fallback, ctx)?;
    out.push_str("; } }).catch(() => ");
    emit_signal_text_fallback(out, target, fallback, ctx)?;
    out.push_str("); } else { ");
    emit_signal_text_fallback(out, target, fallback, ctx)?;
    out.push_str("; } }\n");
    Ok(())
}

fn emit_signal_text_fallback(
    out: &mut String,
    target: SignalId,
    fallback: Option<&ClientExpr>,
    ctx: &ClientEmitContext,
) -> Result<(), ClientEmitError> {
    out.push_str(&ctx.signal_name(target));
    out.push_str(".set(");
    if let Some(fallback) = fallback {
        emit_expr_with_context(out, fallback, ctx)?;
    } else {
        out.push_str("\"\"");
    }
    out.push(')');
    Ok(())
}

fn bundle_export_name(module: &str, name: &str) -> String {
    let last = module.rsplit("::").next().unwrap_or(module);
    format!("{last}__{name}")
}

fn client_module_namespace(module: &str) -> String {
    format!("__rsjs_client_module_{}", module.replace("::", "__"))
}

fn js_string_inner(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            ch if (ch as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out
}
