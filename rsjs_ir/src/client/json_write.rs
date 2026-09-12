// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! JSON emission for Client IR v3 sidecars.
//!
//! Descriptor and client-helper sidecars should serialize through this module
//! so JSON snapshots exercise the same Client IR v3 contract that parsers,
//! emitters, and optimization passes consume.

use super::expr::{
    AsType, BrowserStorageArea, EncodingOp, EventAttributeSource, EventCollection,
    EventCollectionField, EventValue, JsonPointer, LocationField, MutationAccessor,
    NodeRefAccessor, QueryAccessor, StorageValueType, StringPredicateOp, StringTransformOp,
    WindowField,
};
use crate::abi::CLIENT_IR_SCHEMA_VERSION;
use crate::reactive::{ConsoleLevel, EventFileSource, StorageArea, UrlHistoryMode};
use crate::value::Value;

use super::expr::{
    CheckedIntegerOp, ClientBinaryOp, ClientBrowserRead, ClientExpr, ClientFunctionRef,
    ClientUnaryOp,
};
use super::module::{
    ClientCapture, ClientConstant, ClientExport, ClientFunction, ClientImport, ClientModule,
    ClientModuleKind, ClientParam, ClientVisibility,
};
use super::stmt::{
    ClientBlock, ClientBrowserEffect, ClientNodeRefEffect, ClientPlace, ClientSimpleClosure,
    ClientStmt,
};
use super::types::ClientType;

pub fn client_block_json(block: &ClientBlock) -> String {
    let mut out = String::new();
    write_client_block_json(&mut out, block);
    out
}

pub fn write_client_block_json(out: &mut String, block: &ClientBlock) {
    out.push_str("{\"kind\":\"block\",\"stmts\":[");
    for (i, stmt) in block.stmts.iter().enumerate() {
        comma(out, i);
        push_client_stmt(out, stmt);
    }
    out.push_str("]}");
}

pub fn client_expr_json(expr: &ClientExpr) -> String {
    let mut out = String::new();
    write_client_expr_json(&mut out, expr);
    out
}

pub fn write_client_expr_json(out: &mut String, expr: &ClientExpr) {
    push_client_expr(out, expr);
}

/// Serialize a [`ClientType`] in the `{"kind": ...}` shape
/// `parse_client_type_json` reads back. Used by the
/// component-descriptor writer for [`crate::reactive::SignalDecl::ty`].
pub fn write_client_type_json(out: &mut String, ty: &ClientType) {
    push_client_type(out, ty);
}

pub fn client_module_json(module: &ClientModule) -> String {
    let mut out = String::new();
    write_client_module_json(&mut out, module);
    out
}

pub fn write_client_module_json(out: &mut String, module: &ClientModule) {
    out.push_str("{\"format\":\"rsjs.client_module_ir\",\"client_ir_version\":");
    out.push_str(&CLIENT_IR_SCHEMA_VERSION.to_string());
    out.push_str(",\"name\":");
    push_json_string(out, &module.name);
    out.push_str(",\"module_kind\":");
    push_json_string(out, client_module_kind(module.kind));

    out.push_str(",\"imports\":[");
    for (i, import) in module.imports.iter().enumerate() {
        comma(out, i);
        push_client_import(out, import);
    }
    out.push(']');

    out.push_str(",\"constants\":[");
    for (i, constant) in module.constants.iter().enumerate() {
        comma(out, i);
        push_client_constant(out, constant);
    }
    out.push(']');

    out.push_str(",\"functions\":[");
    for (i, function) in module.functions.iter().enumerate() {
        comma(out, i);
        push_client_function(out, function);
    }
    out.push(']');

    out.push_str(",\"exports\":[");
    for (i, export) in module.exports.iter().enumerate() {
        comma(out, i);
        push_client_export(out, export);
    }
    out.push(']');

    out.push_str(",\"metadata\":{");
    for (i, (key, value)) in module.metadata.iter().enumerate() {
        comma(out, i);
        push_json_string(out, key);
        out.push(':');
        push_json_string(out, value);
    }
    out.push_str("}}");
}

fn push_client_import(out: &mut String, import: &ClientImport) {
    out.push_str("{\"module\":");
    push_json_string(out, &import.module);
    out.push_str(",\"name\":");
    push_json_string(out, &import.name);
    out.push_str(",\"alias\":");
    match &import.alias {
        Some(alias) => push_json_string(out, alias),
        None => out.push_str("null"),
    }
    out.push('}');
}

fn push_client_constant(out: &mut String, constant: &ClientConstant) {
    out.push_str("{\"name\":");
    push_json_string(out, &constant.name);
    out.push_str(",\"ty\":");
    push_client_type(out, &constant.ty);
    out.push_str(",\"value\":");
    push_value(out, &constant.value);
    out.push('}');
}

fn push_client_function(out: &mut String, function: &ClientFunction) {
    out.push_str("{\"id\":");
    out.push_str(&function.id.0.to_string());
    out.push_str(",\"name\":");
    push_json_string(out, &function.name);
    out.push_str(",\"visibility\":");
    push_json_string(out, client_visibility(function.visibility));

    out.push_str(",\"params\":[");
    for (i, param) in function.params.iter().enumerate() {
        comma(out, i);
        push_client_param(out, param);
    }
    out.push(']');

    out.push_str(",\"return_ty\":");
    push_client_type(out, &function.return_ty);

    out.push_str(",\"captures\":[");
    for (i, capture) in function.captures.iter().enumerate() {
        comma(out, i);
        push_client_capture(out, capture);
    }
    out.push(']');

    out.push_str(",\"body\":");
    write_client_block_json(out, &function.body);
    out.push('}');
}

fn push_client_param(out: &mut String, param: &ClientParam) {
    out.push_str("{\"name\":");
    push_json_string(out, &param.name);
    out.push_str(",\"ty\":");
    push_client_type(out, &param.ty);
    out.push('}');
}

fn push_client_capture(out: &mut String, capture: &ClientCapture) {
    out.push_str("{\"name\":");
    push_json_string(out, &capture.name);
    out.push_str(",\"ty\":");
    push_client_type(out, &capture.ty);
    out.push('}');
}

fn push_client_export(out: &mut String, export: &ClientExport) {
    out.push_str("{\"name\":");
    push_json_string(out, &export.name);
    out.push_str(",\"function\":");
    out.push_str(&export.function.0.to_string());
    out.push('}');
}

fn push_client_stmt(out: &mut String, stmt: &ClientStmt) {
    match stmt {
        ClientStmt::Expr(expr) => {
            out.push_str("{\"kind\":\"expr\",\"expr\":");
            push_client_expr(out, expr);
            out.push('}');
        }
        ClientStmt::Let {
            local,
            name,
            ty,
            value,
        } => {
            out.push_str("{\"kind\":\"let\",\"local\":");
            out.push_str(&local.0.to_string());
            out.push_str(",\"name\":");
            push_json_string(out, name);
            out.push_str(",\"ty\":");
            push_client_type(out, ty);
            out.push_str(",\"value\":");
            match value {
                Some(value) => push_client_expr(out, value),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        ClientStmt::Assign { local, value } => {
            out.push_str("{\"kind\":\"assign\",\"local\":");
            out.push_str(&local.0.to_string());
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
        ClientStmt::AssignPlace { place, value } => {
            out.push_str("{\"kind\":\"assign_place\",\"place\":");
            push_client_place(out, place);
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
        ClientStmt::SignalSet { id, value } => {
            out.push_str("{\"kind\":\"signal_set\",\"signal\":");
            out.push_str(&id.0.to_string());
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
        ClientStmt::SignalUpdate { id, closure } => {
            out.push_str("{\"kind\":\"signal_update\",\"signal\":");
            out.push_str(&id.0.to_string());
            out.push_str(",\"closure\":");
            push_client_closure(out, closure);
            out.push('}');
        }
        ClientStmt::EventPreventDefault => {
            out.push_str("{\"kind\":\"event_prevent_default\"}");
        }
        ClientStmt::EventStopPropagation => {
            out.push_str("{\"kind\":\"event_stop_propagation\"}");
        }
        ClientStmt::Console { level, args } => {
            out.push_str("{\"kind\":\"console\",\"level\":");
            push_json_string(out, console_level(*level));
            out.push_str(",\"args\":[");
            for (i, arg) in args.iter().enumerate() {
                comma(out, i);
                push_client_expr(out, arg);
            }
            out.push_str("]}");
        }
        ClientStmt::CallbackInvoke { callback, payload } => {
            out.push_str("{\"kind\":\"callback_invoke\",\"callback\":");
            out.push_str(&callback.0.to_string());
            out.push_str(",\"payload\":");
            push_client_expr(out, payload);
            out.push('}');
        }
        ClientStmt::RunMutation { mutation, args } => {
            out.push_str("{\"kind\":\"run_mutation\",\"mutation\":");
            out.push_str(&mutation.0.to_string());
            out.push_str(",\"args\":[");
            for (i, arg) in args.iter().enumerate() {
                comma(out, i);
                push_client_expr(out, arg);
            }
            out.push_str("]}");
        }
        ClientStmt::QueryRefetch { query } => {
            out.push_str("{\"kind\":\"query_refetch\",\"query\":");
            out.push_str(&query.0.to_string());
            out.push('}');
        }
        ClientStmt::QueryInvalidate { query } => {
            out.push_str("{\"kind\":\"query_invalidate\",\"query\":");
            out.push_str(&query.0.to_string());
            out.push('}');
        }
        ClientStmt::BrowserEffect(effect) => {
            out.push_str("{\"kind\":\"browser_effect\",\"effect\":");
            push_client_browser_effect(out, effect);
            out.push('}');
        }
        ClientStmt::NodeRefEffect { id, effect } => {
            out.push_str("{\"kind\":\"node_ref_effect\",\"ref\":");
            out.push_str(&id.0.to_string());
            out.push_str(",\"effect\":");
            push_client_node_ref_effect(out, effect);
            out.push('}');
        }
        ClientStmt::Return(value) => {
            out.push_str("{\"kind\":\"return\",\"value\":");
            match value {
                Some(value) => push_client_expr(out, value),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        ClientStmt::Block(block) => {
            out.push_str("{\"kind\":\"nested_block\",\"block\":");
            write_client_block_json(out, block);
            out.push('}');
        }
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            out.push_str("{\"kind\":\"if\",\"condition\":");
            push_client_expr(out, condition);
            out.push_str(",\"then\":");
            write_client_block_json(out, then_block);
            out.push_str(",\"else\":");
            match else_block {
                Some(block) => write_client_block_json(out, block),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        ClientStmt::ForRange {
            local,
            name,
            integer_domain,
            start,
            end,
            inclusive,
            body,
        } => {
            out.push_str("{\"kind\":\"for_range\",\"local\":");
            out.push_str(&local.0.to_string());
            out.push_str(",\"name\":");
            push_json_string(out, name);
            out.push_str(",\"integer_domain\":");
            push_json_string(out, integer_domain.as_str());
            out.push_str(",\"start\":");
            push_client_expr(out, start);
            out.push_str(",\"end\":");
            push_client_expr(out, end);
            out.push_str(",\"inclusive\":");
            out.push_str(if *inclusive { "true" } else { "false" });
            out.push_str(",\"body\":");
            write_client_block_json(out, body);
            out.push('}');
        }
    }
}

fn push_client_place(out: &mut String, place: &ClientPlace) {
    match place {
        ClientPlace::Local(local) => {
            out.push_str("{\"kind\":\"local\",\"local\":");
            out.push_str(&local.0.to_string());
            out.push('}');
        }
        ClientPlace::Field { base, name } => {
            out.push_str("{\"kind\":\"field\",\"base\":");
            push_client_place(out, base);
            out.push_str(",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
    }
}

fn push_client_expr(out: &mut String, expr: &ClientExpr) {
    match expr {
        ClientExpr::Unit => {
            out.push_str("{\"kind\":\"unit\"}");
        }
        ClientExpr::Literal(value) => {
            out.push_str("{\"kind\":\"literal\",\"value\":");
            push_value(out, value);
            out.push('}');
        }
        ClientExpr::ArrayLit(elems) => {
            out.push_str("{\"kind\":\"array_lit\",\"elems\":[");
            for (i, elem) in elems.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_value(out, elem);
            }
            out.push_str("]}");
        }
        ClientExpr::QueryKey(parts) => {
            out.push_str("{\"kind\":\"query_key\",\"parts\":[");
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_client_expr(out, part);
            }
            out.push_str("]}");
        }
        ClientExpr::Local(id) => {
            out.push_str("{\"kind\":\"local\",\"local\":");
            out.push_str(&id.0.to_string());
            out.push('}');
        }
        ClientExpr::SignalRead(id) => {
            out.push_str("{\"kind\":\"signal_read\",\"signal\":");
            out.push_str(&id.0.to_string());
            out.push('}');
        }
        ClientExpr::QueryRead { id, accessor } => {
            out.push_str("{\"kind\":\"query_read\",\"query\":");
            out.push_str(&id.0.to_string());
            out.push_str(",\"accessor\":");
            push_json_string(out, query_accessor(*accessor));
            out.push('}');
        }
        ClientExpr::MutationRead { id, accessor } => {
            out.push_str("{\"kind\":\"mutation_read\",\"mutation\":");
            out.push_str(&id.0.to_string());
            out.push_str(",\"accessor\":");
            push_json_string(out, mutation_accessor(*accessor));
            out.push('}');
        }
        ClientExpr::EventRead(value) => {
            out.push_str("{\"kind\":\"event_read\",\"value\":");
            push_json_string(out, event_value(*value));
            out.push('}');
        }
        ClientExpr::EventCurrentTarget => {
            out.push_str("{\"kind\":\"event_current_target\"}");
        }
        ClientExpr::DomRead { target, accessor } => {
            out.push_str("{\"kind\":\"dom_read\",\"target\":");
            push_client_expr(out, target);
            push_node_ref_accessor(out, accessor);
            out.push('}');
        }
        ClientExpr::EventCollectionLen(collection) => {
            out.push_str("{\"kind\":\"event_collection_len\",\"collection\":");
            push_json_string(out, event_collection(*collection));
            out.push('}');
        }
        ClientExpr::EventCollectionRead {
            collection,
            index,
            field,
        } => {
            out.push_str("{\"kind\":\"event_collection_read\",\"collection\":");
            push_json_string(out, event_collection(*collection));
            out.push_str(",\"index\":");
            push_client_expr(out, index);
            out.push_str(",\"field\":");
            push_json_string(out, event_collection_field(*field));
            out.push('}');
        }
        ClientExpr::NodeRefRead { id, accessor } => {
            out.push_str("{\"kind\":\"node_ref_read\",\"ref\":");
            out.push_str(&id.0.to_string());
            push_node_ref_accessor(out, accessor);
            out.push('}');
        }
        ClientExpr::EventTargetWithinNodeRef { id, inside } => {
            out.push_str("{\"kind\":\"event_target_within_node_ref\",\"ref\":");
            out.push_str(&id.0.to_string());
            out.push_str(",\"inside\":");
            out.push_str(if *inside { "true" } else { "false" });
            out.push('}');
        }
        ClientExpr::EventAttribute { source, attr } => {
            out.push_str("{\"kind\":\"event_attribute\",\"source\":");
            push_json_string(out, event_attribute_source(*source));
            out.push_str(",\"attr\":");
            push_json_string(out, attr);
            out.push('}');
        }
        ClientExpr::EventPayload { attr } => {
            out.push_str("{\"kind\":\"event_payload\",\"attr\":");
            push_json_string(out, attr);
            out.push('}');
        }
        ClientExpr::CallbackPayload => {
            out.push_str("{\"kind\":\"callback_payload\"}");
        }
        ClientExpr::NodePayload { attr, tokens } => {
            out.push_str("{\"kind\":\"node_payload\",\"attr\":");
            push_json_string(out, attr);
            out.push_str(",\"tokens\":");
            push_json_string(out, tokens);
            out.push('}');
        }
        ClientExpr::BrowserRead(read) => {
            push_client_browser_read(out, read);
        }
        ClientExpr::Encoding { op, value } => {
            out.push_str("{\"kind\":\"encoding\",\"op\":");
            push_json_string(out, encoding_op(*op));
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            out.push_str("{\"kind\":\"conditional\",\"condition\":");
            push_client_expr(out, condition);
            out.push_str(",\"then\":");
            push_client_expr(out, then_expr);
            out.push_str(",\"else\":");
            push_client_expr(out, else_expr);
            out.push('}');
        }
        ClientExpr::IsNonEmpty { target } => {
            out.push_str("{\"kind\":\"is_non_empty\",\"target\":");
            push_client_expr(out, target);
            out.push('}');
        }
        ClientExpr::StringToLowercase { target } => {
            out.push_str("{\"kind\":\"string_to_lowercase\",\"target\":");
            push_client_expr(out, target);
            out.push('}');
        }
        ClientExpr::StringContains { haystack, needle } => {
            out.push_str("{\"kind\":\"string_contains\",\"haystack\":");
            push_client_expr(out, haystack);
            out.push_str(",\"needle\":");
            push_client_expr(out, needle);
            out.push('}');
        }
        ClientExpr::StringTransform { op, target } => {
            out.push_str("{\"kind\":\"string_transform\",\"op\":");
            push_json_string(out, string_transform_op(*op));
            out.push_str(",\"target\":");
            push_client_expr(out, target);
            out.push('}');
        }
        ClientExpr::StringPredicate {
            op,
            haystack,
            needle,
        } => {
            out.push_str("{\"kind\":\"string_predicate\",\"op\":");
            push_json_string(out, string_predicate_op(*op));
            out.push_str(",\"haystack\":");
            push_client_expr(out, haystack);
            out.push_str(",\"needle\":");
            push_client_expr(out, needle);
            out.push('}');
        }
        ClientExpr::Project { target, path } => {
            out.push_str("{\"kind\":\"project\",\"target\":");
            push_client_expr(out, target);
            out.push_str(",\"path\":");
            push_json_string(out, json_pointer(path));
            out.push('}');
        }
        ClientExpr::AsType { target, ty } => {
            out.push_str("{\"kind\":\"as_type\",\"target\":");
            push_client_expr(out, target);
            out.push_str(",\"ty\":");
            push_json_string(out, as_type(*ty));
            out.push('}');
        }
        ClientExpr::IsPresent { target } => {
            out.push_str("{\"kind\":\"is_present\",\"target\":");
            push_client_expr(out, target);
            out.push('}');
        }
        ClientExpr::Eq { target, value } => {
            out.push_str("{\"kind\":\"eq\",\"target\":");
            push_client_expr(out, target);
            out.push_str(",\"value\":");
            push_value(out, value);
            out.push('}');
        }
        ClientExpr::Neq { target, value } => {
            out.push_str("{\"kind\":\"neq\",\"target\":");
            push_client_expr(out, target);
            out.push_str(",\"value\":");
            push_value(out, value);
            out.push('}');
        }
        ClientExpr::Coalesce { target, fallback } => {
            out.push_str("{\"kind\":\"coalesce\",\"target\":");
            push_client_expr(out, target);
            out.push_str(",\"fallback\":");
            push_value(out, fallback);
            out.push('}');
        }
        ClientExpr::Unary { op, expr } => {
            out.push_str("{\"kind\":\"unary\",\"op\":");
            push_json_string(out, client_unary_op(*op));
            out.push_str(",\"expr\":");
            push_client_expr(out, expr);
            out.push('}');
        }
        ClientExpr::Binary { op, left, right } => {
            out.push_str("{\"kind\":\"binary\",\"op\":");
            push_json_string(out, client_binary_op(*op));
            out.push_str(",\"left\":");
            push_client_expr(out, left);
            out.push_str(",\"right\":");
            push_client_expr(out, right);
            out.push('}');
        }
        ClientExpr::StructInit { type_name, fields } => {
            out.push_str("{\"kind\":\"struct_init\",\"type_name\":");
            push_json_string(out, type_name);
            out.push_str(",\"fields\":[");
            for (index, field) in fields.iter().enumerate() {
                comma(out, index);
                out.push_str("{\"name\":");
                push_json_string(out, &field.name);
                out.push_str(",\"value\":");
                push_client_expr(out, &field.value);
                out.push('}');
            }
            out.push_str("]}");
        }
        ClientExpr::Field { base, name } => {
            out.push_str("{\"kind\":\"field\",\"base\":");
            push_client_expr(out, base);
            out.push_str(",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
        ClientExpr::Index { base, index } => {
            out.push_str("{\"kind\":\"index\",\"base\":");
            push_client_expr(out, base);
            out.push_str(",\"index\":");
            push_client_expr(out, index);
            out.push('}');
        }
        ClientExpr::Call { function, args } => {
            out.push_str("{\"kind\":\"call\",\"function\":");
            push_client_function_ref(out, function);
            out.push_str(",\"args\":[");
            for (i, arg) in args.iter().enumerate() {
                comma(out, i);
                push_client_expr(out, arg);
            }
            out.push_str("]}");
        }
    }
}

fn push_client_function_ref(out: &mut String, function: &ClientFunctionRef) {
    match function {
        ClientFunctionRef::Local(id) => {
            out.push_str("{\"kind\":\"local\",\"function\":");
            out.push_str(&id.0.to_string());
            out.push('}');
        }
        ClientFunctionRef::Import { module, name } => {
            out.push_str("{\"kind\":\"import\",\"module\":");
            push_json_string(out, module);
            out.push_str(",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
        ClientFunctionRef::Helper {
            edge,
            authored_path,
            name,
        } => {
            out.push_str("{\"kind\":\"helper\",\"edge\":");
            out.push_str(&edge.index().to_string());
            out.push_str(",\"authored_path\":");
            push_json_string(out, authored_path);
            out.push_str(",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
        ClientFunctionRef::Runtime(name) => {
            out.push_str("{\"kind\":\"runtime\",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
        ClientFunctionRef::Unresolved(path) => {
            out.push_str("{\"kind\":\"unresolved\",\"path\":");
            push_json_string(out, path);
            out.push('}');
        }
    }
}

fn push_client_type(out: &mut String, ty: &ClientType) {
    match ty {
        ClientType::Unit => out.push_str("{\"kind\":\"unit\"}"),
        ClientType::Bool => out.push_str("{\"kind\":\"bool\"}"),
        ClientType::I32 => out.push_str("{\"kind\":\"i32\"}"),
        ClientType::U32 => out.push_str("{\"kind\":\"u32\"}"),
        ClientType::I64 => out.push_str("{\"kind\":\"i64\"}"),
        ClientType::U64 => out.push_str("{\"kind\":\"u64\"}"),
        ClientType::F32 => out.push_str("{\"kind\":\"f32\"}"),
        ClientType::F64 => out.push_str("{\"kind\":\"f64\"}"),
        ClientType::String => out.push_str("{\"kind\":\"string\"}"),
        ClientType::Option(inner) => {
            out.push_str("{\"kind\":\"option\",\"inner\":");
            push_client_type(out, inner);
            out.push('}');
        }
        ClientType::Vec(inner) => {
            out.push_str("{\"kind\":\"vec\",\"inner\":");
            push_client_type(out, inner);
            out.push('}');
        }
        ClientType::Object(fields) => {
            out.push_str("{\"kind\":\"object\",\"fields\":[");
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str("{\"name\":");
                push_json_string(out, &field.name);
                out.push_str(",\"ty\":");
                push_client_type(out, &field.ty);
                out.push('}');
            }
            out.push_str("]}");
        }
        ClientType::ValueObject { type_key, fields } => {
            out.push_str("{\"kind\":\"value_object\",\"type_key\":");
            push_json_string(out, type_key);
            out.push_str(",\"fields\":[");
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str("{\"name\":");
                push_json_string(out, &field.name);
                out.push_str(",\"ty\":");
                push_client_type(out, &field.ty);
                out.push('}');
            }
            out.push_str("]}");
        }
        ClientType::Struct(name) => {
            out.push_str("{\"kind\":\"struct\",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
        ClientType::Enum(name) => {
            out.push_str("{\"kind\":\"enum\",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
        ClientType::Event => out.push_str("{\"kind\":\"event\"}"),
        ClientType::DomElement => out.push_str("{\"kind\":\"dom_element\"}"),
        ClientType::Signal(inner) => {
            out.push_str("{\"kind\":\"signal\",\"inner\":");
            push_client_type(out, inner);
            out.push('}');
        }
        ClientType::Callback(inner) => {
            out.push_str("{\"kind\":\"callback\",\"payload\":");
            push_client_type(out, inner);
            out.push('}');
        }
        ClientType::Dyn => out.push_str("{\"kind\":\"dyn\"}"),
        ClientType::Unknown => out.push_str("{\"kind\":\"unknown\"}"),
    }
}

fn client_module_kind(kind: ClientModuleKind) -> &'static str {
    match kind {
        ClientModuleKind::Island => "island",
        ClientModuleKind::ComponentFragment => "component_fragment",
        ClientModuleKind::Route => "route",
        ClientModuleKind::ClientModule => "client_module",
        ClientModuleKind::Runtime => "runtime",
    }
}

fn client_visibility(visibility: ClientVisibility) -> &'static str {
    match visibility {
        ClientVisibility::Private => "private",
        ClientVisibility::Exported => "exported",
    }
}

fn push_client_closure(out: &mut String, closure: &ClientSimpleClosure) {
    out.push('{');
    out.push_str("\"param\":");
    push_json_string(out, &closure.param);
    out.push_str(",\"body\":");
    push_client_expr(out, &closure.body);
    out.push('}');
}

fn push_client_browser_read(out: &mut String, read: &ClientBrowserRead) {
    match read {
        ClientBrowserRead::Window(field) => {
            out.push_str("{\"kind\":\"browser_read\",\"read\":\"window\",\"field\":");
            push_json_string(out, window_field(*field));
            out.push('}');
        }
        ClientBrowserRead::Location(field) => {
            out.push_str("{\"kind\":\"browser_read\",\"read\":\"location\",\"field\":");
            push_json_string(out, location_field(*field));
            out.push('}');
        }
        ClientBrowserRead::Storage {
            area,
            value_type,
            key,
            fallback,
        } => {
            out.push_str("{\"kind\":\"browser_read\",\"read\":\"storage\",\"area\":");
            push_json_string(out, browser_storage_area(*area));
            if *value_type != StorageValueType::String {
                out.push_str(",\"value_type\":");
                push_json_string(out, storage_value_type(*value_type));
            }
            out.push_str(",\"key\":");
            push_client_expr(out, key);
            if let Some(fallback) = fallback {
                out.push_str(",\"fallback\":");
                push_client_expr(out, fallback);
            }
            out.push('}');
        }
        ClientBrowserRead::UrlSearchParam { key, fallback } => {
            out.push_str("{\"kind\":\"browser_read\",\"read\":\"url_search_param\",\"key\":");
            push_client_expr(out, key);
            if let Some(fallback) = fallback {
                out.push_str(",\"fallback\":");
                push_client_expr(out, fallback);
            }
            out.push('}');
        }
        ClientBrowserRead::Declared { helper, args } => {
            out.push_str("{\"kind\":\"browser_read\",\"read\":\"declared\",\"helper\":");
            push_json_string(out, helper);
            out.push_str(",\"args\":[");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_client_expr(out, arg);
            }
            out.push_str("]}");
        }
    }
}

fn push_client_browser_effect(out: &mut String, effect: &ClientBrowserEffect) {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y } => {
            out.push_str("{\"kind\":\"window_scroll_to\",\"x\":");
            push_client_expr(out, x);
            out.push_str(",\"y\":");
            push_client_expr(out, y);
            out.push('}');
        }
        ClientBrowserEffect::WindowScrollBy { x, y } => {
            out.push_str("{\"kind\":\"window_scroll_by\",\"x\":");
            push_client_expr(out, x);
            out.push_str(",\"y\":");
            push_client_expr(out, y);
            out.push('}');
        }
        ClientBrowserEffect::WindowPrint => {
            out.push_str("{\"kind\":\"window_print\"}");
        }
        ClientBrowserEffect::LocationSetHash { value } => {
            out.push_str("{\"kind\":\"location_set_hash\",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
        ClientBrowserEffect::LocationAssign { url } => {
            out.push_str("{\"kind\":\"location_assign\",\"url\":");
            push_client_expr(out, url);
            out.push('}');
        }
        ClientBrowserEffect::LocationReplace { url } => {
            out.push_str("{\"kind\":\"location_replace\",\"url\":");
            push_client_expr(out, url);
            out.push('}');
        }
        ClientBrowserEffect::LocationReload => {
            out.push_str("{\"kind\":\"location_reload\"}");
        }
        ClientBrowserEffect::HistoryPushState { url } => {
            out.push_str("{\"kind\":\"history_push_state\",\"url\":");
            push_client_expr(out, url);
            out.push('}');
        }
        ClientBrowserEffect::HistoryReplaceState { url } => {
            out.push_str("{\"kind\":\"history_replace_state\",\"url\":");
            push_client_expr(out, url);
            out.push('}');
        }
        ClientBrowserEffect::HistoryBack => {
            out.push_str("{\"kind\":\"history_back\"}");
        }
        ClientBrowserEffect::HistoryForward => {
            out.push_str("{\"kind\":\"history_forward\"}");
        }
        ClientBrowserEffect::HistoryGo { delta } => {
            out.push_str("{\"kind\":\"history_go\",\"delta\":");
            push_client_expr(out, delta);
            out.push('}');
        }
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            out.push_str("{\"kind\":\"document_dispatch_event\",\"name\":");
            push_client_expr(out, name);
            if let Some(detail) = detail {
                out.push_str(",\"detail\":");
                push_client_expr(out, detail);
            }
            out.push('}');
        }
        ClientBrowserEffect::DocumentSetTitle { title } => {
            out.push_str("{\"kind\":\"document_set_title\",\"title\":");
            push_client_expr(out, title);
            out.push('}');
        }
        ClientBrowserEffect::ClipboardWriteText { text } => {
            out.push_str("{\"kind\":\"clipboard_write_text\",\"text\":");
            push_client_expr(out, text);
            out.push('}');
        }
        ClientBrowserEffect::ClipboardReadTextInto { target, fallback } => {
            out.push_str("{\"kind\":\"clipboard_read_text_into\",\"target\":");
            out.push_str(&target.0.to_string());
            if let Some(fallback) = fallback {
                out.push_str(",\"fallback\":");
                push_client_expr(out, fallback);
            }
            out.push('}');
        }
        ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            target,
            fallback,
        } => {
            out.push_str("{\"kind\":\"webauthn_create_json_into\",\"options_json\":");
            push_client_expr(out, options_json);
            out.push_str(",\"target\":");
            out.push_str(&target.0.to_string());
            if let Some(fallback) = fallback {
                out.push_str(",\"fallback\":");
                push_client_expr(out, fallback);
            }
            out.push('}');
        }
        ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            target,
            fallback,
        } => {
            out.push_str("{\"kind\":\"webauthn_get_json_into\",\"options_json\":");
            push_client_expr(out, options_json);
            out.push_str(",\"target\":");
            out.push_str(&target.0.to_string());
            if let Some(fallback) = fallback {
                out.push_str(",\"fallback\":");
                push_client_expr(out, fallback);
            }
            out.push('}');
        }
        ClientBrowserEffect::EventFileTextInto {
            source,
            target,
            fallback,
        } => {
            out.push_str("{\"kind\":\"event_file_text_into\",\"source\":");
            push_json_string(out, event_file_source(*source));
            out.push_str(",\"target\":");
            out.push_str(&target.0.to_string());
            if let Some(fallback) = fallback {
                out.push_str(",\"fallback\":");
                push_client_expr(out, fallback);
            }
            out.push('}');
        }
        ClientBrowserEffect::StorageSetItem { area, key, value } => {
            out.push_str("{\"kind\":\"storage_set_item\",\"area\":");
            push_json_string(out, storage_area(*area));
            out.push_str(",\"key\":");
            push_client_expr(out, key);
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
        ClientBrowserEffect::StorageRemoveItem { area, key } => {
            out.push_str("{\"kind\":\"storage_remove_item\",\"area\":");
            push_json_string(out, storage_area(*area));
            out.push_str(",\"key\":");
            push_client_expr(out, key);
            out.push('}');
        }
        ClientBrowserEffect::UrlSetSearchParam { mode, key, value } => {
            out.push_str("{\"kind\":\"url_set_search_param\",\"key\":");
            push_client_expr(out, key);
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            if *mode != UrlHistoryMode::Replace {
                out.push_str(",\"history\":");
                push_json_string(out, url_history_mode(*mode));
            }
            out.push('}');
        }
        ClientBrowserEffect::UrlDeleteSearchParam { mode, key } => {
            out.push_str("{\"kind\":\"url_delete_search_param\",\"key\":");
            push_client_expr(out, key);
            if *mode != UrlHistoryMode::Replace {
                out.push_str(",\"history\":");
                push_json_string(out, url_history_mode(*mode));
            }
            out.push('}');
        }
        ClientBrowserEffect::Declared { helper, args } => {
            out.push_str("{\"kind\":\"declared\",\"helper\":");
            push_json_string(out, helper);
            out.push_str(",\"args\":[");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_client_expr(out, arg);
            }
            out.push_str("]}");
        }
    }
}

fn push_client_node_ref_effect(out: &mut String, effect: &ClientNodeRefEffect) {
    match effect {
        ClientNodeRefEffect::Focus => out.push_str("{\"kind\":\"focus\"}"),
        ClientNodeRefEffect::Blur => out.push_str("{\"kind\":\"blur\"}"),
        ClientNodeRefEffect::ScrollIntoView => out.push_str("{\"kind\":\"scroll_into_view\"}"),
        ClientNodeRefEffect::Click => out.push_str("{\"kind\":\"click\"}"),
        ClientNodeRefEffect::RequestSubmit => out.push_str("{\"kind\":\"request_submit\"}"),
        ClientNodeRefEffect::Reset => out.push_str("{\"kind\":\"reset\"}"),
        ClientNodeRefEffect::ReportValidity => out.push_str("{\"kind\":\"report_validity\"}"),
        ClientNodeRefEffect::ShowModal => out.push_str("{\"kind\":\"show_modal\"}"),
        ClientNodeRefEffect::Close => out.push_str("{\"kind\":\"close\"}"),
        ClientNodeRefEffect::ShowPopover => out.push_str("{\"kind\":\"show_popover\"}"),
        ClientNodeRefEffect::HidePopover => out.push_str("{\"kind\":\"hide_popover\"}"),
        ClientNodeRefEffect::TogglePopover => out.push_str("{\"kind\":\"toggle_popover\"}"),
        ClientNodeRefEffect::ScrollTo { x, y } => {
            out.push_str("{\"kind\":\"scroll_to\",\"x\":");
            push_client_expr(out, x);
            out.push_str(",\"y\":");
            push_client_expr(out, y);
            out.push('}');
        }
        ClientNodeRefEffect::ScrollBy { x, y } => {
            out.push_str("{\"kind\":\"scroll_by\",\"x\":");
            push_client_expr(out, x);
            out.push_str(",\"y\":");
            push_client_expr(out, y);
            out.push('}');
        }
        ClientNodeRefEffect::SetPointerCapture(expr) => {
            push_client_node_ref_effect_with_value(out, "set_pointer_capture", expr)
        }
        ClientNodeRefEffect::ReleasePointerCapture(expr) => {
            push_client_node_ref_effect_with_value(out, "release_pointer_capture", expr)
        }
        ClientNodeRefEffect::Select => out.push_str("{\"kind\":\"select\"}"),
        ClientNodeRefEffect::SetValue(expr) => {
            push_client_node_ref_effect_with_value(out, "set_value", expr)
        }
        ClientNodeRefEffect::SetChecked(expr) => {
            push_client_node_ref_effect_with_value(out, "set_checked", expr)
        }
        ClientNodeRefEffect::SetCustomValidity(expr) => {
            push_client_node_ref_effect_with_value(out, "set_custom_validity", expr)
        }
        ClientNodeRefEffect::SetSelectionRange(start, end) => {
            out.push_str("{\"kind\":\"set_selection_range\",\"start\":");
            push_client_expr(out, start);
            out.push_str(",\"end\":");
            push_client_expr(out, end);
            out.push('}');
        }
        ClientNodeRefEffect::SetTextContent(expr) => {
            push_client_node_ref_effect_with_value(out, "set_text_content", expr)
        }
        ClientNodeRefEffect::AddClass(expr) => {
            push_client_node_ref_effect_with_value(out, "add_class", expr)
        }
        ClientNodeRefEffect::RemoveClass(expr) => {
            push_client_node_ref_effect_with_value(out, "remove_class", expr)
        }
        ClientNodeRefEffect::ToggleClass(expr) => {
            push_client_node_ref_effect_with_value(out, "toggle_class", expr)
        }
        ClientNodeRefEffect::SetAttribute { name, value } => {
            out.push_str("{\"kind\":\"set_attribute\",\"name\":");
            push_json_string(out, name);
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
        ClientNodeRefEffect::RemoveAttribute { name } => {
            out.push_str("{\"kind\":\"remove_attribute\",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
        ClientNodeRefEffect::SetStyleProperty { name, value } => {
            out.push_str("{\"kind\":\"set_style_property\",\"name\":");
            push_json_string(out, name);
            out.push_str(",\"value\":");
            push_client_expr(out, value);
            out.push('}');
        }
    }
}

fn push_client_node_ref_effect_with_value(out: &mut String, kind: &str, expr: &ClientExpr) {
    out.push_str("{\"kind\":");
    push_json_string(out, kind);
    out.push_str(",\"value\":");
    push_client_expr(out, expr);
    out.push('}');
}

fn window_field(field: WindowField) -> &'static str {
    match field {
        WindowField::InnerWidth => "inner_width",
        WindowField::InnerHeight => "inner_height",
        WindowField::ScrollX => "scroll_x",
        WindowField::ScrollY => "scroll_y",
    }
}

fn location_field(field: LocationField) -> &'static str {
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

fn browser_storage_area(area: BrowserStorageArea) -> &'static str {
    match area {
        BrowserStorageArea::Local => "local",
        BrowserStorageArea::Session => "session",
    }
}

fn storage_value_type(value_type: StorageValueType) -> &'static str {
    match value_type {
        StorageValueType::String => "string",
        StorageValueType::Bool => "bool",
        StorageValueType::I64 => "i64",
        StorageValueType::F64 => "f64",
    }
}

fn encoding_op(op: EncodingOp) -> &'static str {
    match op {
        EncodingOp::Utf8Bytes => "utf8_bytes",
        EncodingOp::Utf8String => "utf8_string",
        EncodingOp::Base64Encode => "base64_encode",
        EncodingOp::Base64Decode => "base64_decode",
        EncodingOp::Base64UrlEncode => "base64url_encode",
        EncodingOp::Base64UrlDecode => "base64url_decode",
    }
}

fn string_transform_op(op: StringTransformOp) -> &'static str {
    match op {
        StringTransformOp::ToUppercase => "to_uppercase",
        StringTransformOp::Trim => "trim",
    }
}

fn string_predicate_op(op: StringPredicateOp) -> &'static str {
    match op {
        StringPredicateOp::StartsWith => "starts_with",
        StringPredicateOp::EndsWith => "ends_with",
    }
}

fn push_value(out: &mut String, value: &Value) {
    match value {
        Value::Bool(v) => {
            out.push_str("{\"kind\":\"bool\",\"value\":");
            out.push_str(if *v { "true" } else { "false" });
            out.push('}');
        }
        Value::Int(v) => {
            out.push_str("{\"kind\":\"int\",\"value\":");
            out.push_str(&v.to_string());
            out.push('}');
        }
        Value::BigInt(v) => {
            out.push_str("{\"kind\":\"bigint\",\"value\":");
            push_json_string(out, &v.to_string());
            out.push('}');
        }
        Value::Float(v) => {
            out.push_str("{\"kind\":\"float\",\"value\":");
            if v.is_finite() {
                out.push_str(&v.to_string());
            } else {
                push_json_string(out, &v.to_string());
            }
            out.push('}');
        }
        Value::String(v) => {
            out.push_str("{\"kind\":\"string\",\"value\":");
            push_json_string(out, v);
            out.push('}');
        }
        Value::Null => {
            out.push_str("{\"kind\":\"null\",\"value\":null}");
        }
    }
}

fn console_level(level: ConsoleLevel) -> &'static str {
    match level {
        ConsoleLevel::Log => "log",
        ConsoleLevel::Info => "info",
        ConsoleLevel::Debug => "debug",
        ConsoleLevel::Warn => "warn",
        ConsoleLevel::Error => "error",
        ConsoleLevel::Table => "table",
        ConsoleLevel::Time => "time",
        ConsoleLevel::TimeEnd => "time_end",
        ConsoleLevel::Group => "group",
        ConsoleLevel::GroupEnd => "group_end",
    }
}

fn event_value(value: EventValue) -> &'static str {
    match value {
        EventValue::Type => "type",
        EventValue::TargetValue => "target_value",
        EventValue::TargetChecked => "target_checked",
        EventValue::TargetDate => "target_date",
        EventValue::TargetId => "target_id",
        EventValue::CurrentTargetId => "current_target_id",
        EventValue::CurrentTargetValue => "current_target_value",
        EventValue::CurrentTargetChecked => "current_target_checked",
        EventValue::Key => "key",
        EventValue::Code => "code",
        EventValue::Repeat => "repeat",
        EventValue::ShiftKey => "shift_key",
        EventValue::MetaKey => "meta_key",
        EventValue::CtrlKey => "ctrl_key",
        EventValue::AltKey => "alt_key",
        EventValue::ClientX => "client_x",
        EventValue::ClientY => "client_y",
        EventValue::PageX => "page_x",
        EventValue::PageY => "page_y",
        EventValue::ScreenX => "screen_x",
        EventValue::ScreenY => "screen_y",
        EventValue::OffsetX => "offset_x",
        EventValue::OffsetY => "offset_y",
        EventValue::MovementX => "movement_x",
        EventValue::MovementY => "movement_y",
        EventValue::Button => "button",
        EventValue::Buttons => "buttons",
        EventValue::Detail => "detail",
        EventValue::DetailU32 => "detail_u32",
        EventValue::PointerId => "pointer_id",
        EventValue::PointerType => "pointer_type",
        EventValue::IsPrimary => "is_primary",
        EventValue::Pressure => "pressure",
        EventValue::TangentialPressure => "tangential_pressure",
        EventValue::TiltX => "tilt_x",
        EventValue::TiltY => "tilt_y",
        EventValue::Twist => "twist",
        EventValue::PointerWidth => "pointer_width",
        EventValue::PointerHeight => "pointer_height",
        EventValue::DeltaX => "delta_x",
        EventValue::DeltaY => "delta_y",
        EventValue::DeltaZ => "delta_z",
        EventValue::DeltaMode => "delta_mode",
        EventValue::CurrentTargetScrollTop => "current_target_scroll_top",
        EventValue::CurrentTargetScrollLeft => "current_target_scroll_left",
        EventValue::InputType => "input_type",
        EventValue::Data => "data",
        EventValue::OldState => "old_state",
        EventValue::NewState => "new_state",
        EventValue::AnimationName => "animation_name",
        EventValue::PropertyName => "property_name",
        EventValue::ElapsedTime => "elapsed_time",
        EventValue::PseudoElement => "pseudo_element",
        EventValue::StorageKey => "storage_key",
        EventValue::StorageOldValue => "storage_old_value",
        EventValue::StorageNewValue => "storage_new_value",
        EventValue::StorageUrl => "storage_url",
        EventValue::StorageArea => "storage_area",
        EventValue::OldUrl => "old_url",
        EventValue::NewUrl => "new_url",
        EventValue::ClipboardText => "clipboard_text",
        EventValue::DataTransferText => "data_transfer_text",
        EventValue::DataTransferFileCount => "data_transfer_file_count",
        EventValue::DataTransferFirstFileName => "data_transfer_first_file_name",
        EventValue::DataTransferFirstFileType => "data_transfer_first_file_type",
        EventValue::DataTransferFirstFileSize => "data_transfer_first_file_size",
        EventValue::TargetFileCount => "target_file_count",
        EventValue::TargetFirstFileName => "target_first_file_name",
        EventValue::TargetFirstFileType => "target_first_file_type",
        EventValue::TargetFirstFileSize => "target_first_file_size",
        EventValue::ChangedTouchIdentifier => "changed_touch_identifier",
        EventValue::ChangedTouchClientX => "changed_touch_client_x",
        EventValue::ChangedTouchClientY => "changed_touch_client_y",
        EventValue::ChangedTouchPageX => "changed_touch_page_x",
        EventValue::ChangedTouchPageY => "changed_touch_page_y",
        EventValue::ChangedTouchScreenX => "changed_touch_screen_x",
        EventValue::ChangedTouchScreenY => "changed_touch_screen_y",
    }
}

fn event_collection(collection: EventCollection) -> &'static str {
    match collection {
        EventCollection::ChangedTouches => "changed_touches",
        EventCollection::TargetTouches => "target_touches",
        EventCollection::Touches => "touches",
        EventCollection::TargetFiles => "target_files",
        EventCollection::DataTransferFiles => "data_transfer_files",
        EventCollection::DataTransferItems => "data_transfer_items",
    }
}

fn event_collection_field(field: EventCollectionField) -> &'static str {
    match field {
        EventCollectionField::TouchIdentifier => "touch_identifier",
        EventCollectionField::TouchClientX => "touch_client_x",
        EventCollectionField::TouchClientY => "touch_client_y",
        EventCollectionField::TouchPageX => "touch_page_x",
        EventCollectionField::TouchPageY => "touch_page_y",
        EventCollectionField::TouchScreenX => "touch_screen_x",
        EventCollectionField::TouchScreenY => "touch_screen_y",
        EventCollectionField::FileName => "file_name",
        EventCollectionField::FileType => "file_type",
        EventCollectionField::FileSize => "file_size",
        EventCollectionField::DataTransferItemKind => "data_transfer_item_kind",
        EventCollectionField::DataTransferItemType => "data_transfer_item_type",
    }
}

fn event_attribute_source(source: EventAttributeSource) -> &'static str {
    match source {
        EventAttributeSource::Target => "target",
        EventAttributeSource::Closest => "closest",
        EventAttributeSource::CurrentTarget => "current_target",
    }
}

fn push_node_ref_accessor(out: &mut String, accessor: &NodeRefAccessor) {
    out.push_str(",\"accessor\":");
    match accessor {
        NodeRefAccessor::Id => push_json_string(out, "id"),
        NodeRefAccessor::TextContent => push_json_string(out, "text_content"),
        NodeRefAccessor::Value => push_json_string(out, "value"),
        NodeRefAccessor::Checked => push_json_string(out, "checked"),
        NodeRefAccessor::CheckValidity => push_json_string(out, "check_validity"),
        NodeRefAccessor::ValidationMessage => push_json_string(out, "validation_message"),
        NodeRefAccessor::ValidityValid => push_json_string(out, "validity_valid"),
        NodeRefAccessor::WillValidate => push_json_string(out, "will_validate"),
        NodeRefAccessor::SelectionStart => push_json_string(out, "selection_start"),
        NodeRefAccessor::SelectionEnd => push_json_string(out, "selection_end"),
        NodeRefAccessor::ScrollTop => push_json_string(out, "scroll_top"),
        NodeRefAccessor::ScrollLeft => push_json_string(out, "scroll_left"),
        NodeRefAccessor::ClientWidth => push_json_string(out, "client_width"),
        NodeRefAccessor::ClientHeight => push_json_string(out, "client_height"),
        NodeRefAccessor::ScrollWidth => push_json_string(out, "scroll_width"),
        NodeRefAccessor::ScrollHeight => push_json_string(out, "scroll_height"),
        NodeRefAccessor::OffsetWidth => push_json_string(out, "offset_width"),
        NodeRefAccessor::OffsetHeight => push_json_string(out, "offset_height"),
        NodeRefAccessor::RectX => push_json_string(out, "rect_x"),
        NodeRefAccessor::RectY => push_json_string(out, "rect_y"),
        NodeRefAccessor::RectTop => push_json_string(out, "rect_top"),
        NodeRefAccessor::RectRight => push_json_string(out, "rect_right"),
        NodeRefAccessor::RectBottom => push_json_string(out, "rect_bottom"),
        NodeRefAccessor::RectLeft => push_json_string(out, "rect_left"),
        NodeRefAccessor::RectWidth => push_json_string(out, "rect_width"),
        NodeRefAccessor::RectHeight => push_json_string(out, "rect_height"),
        NodeRefAccessor::FileCount => push_json_string(out, "file_count"),
        NodeRefAccessor::FirstFileName => push_json_string(out, "first_file_name"),
        NodeRefAccessor::FirstFileType => push_json_string(out, "first_file_type"),
        NodeRefAccessor::FirstFileSize => push_json_string(out, "first_file_size"),
        NodeRefAccessor::Attribute { name } => {
            push_json_string(out, "attribute");
            out.push_str(",\"name\":");
            push_json_string(out, name);
        }
        NodeRefAccessor::HasAttribute { name } => {
            push_json_string(out, "has_attribute");
            out.push_str(",\"name\":");
            push_json_string(out, name);
        }
    }
}

fn client_binary_op(op: ClientBinaryOp) -> &'static str {
    match op {
        ClientBinaryOp::Add => "add",
        ClientBinaryOp::Sub => "sub",
        ClientBinaryOp::Mul => "mul",
        ClientBinaryOp::Div => "div",
        ClientBinaryOp::DivInt => "div_int",
        ClientBinaryOp::Rem => "rem",
        ClientBinaryOp::Eq => "eq",
        ClientBinaryOp::Ne => "ne",
        ClientBinaryOp::Lt => "lt",
        ClientBinaryOp::Le => "le",
        ClientBinaryOp::Gt => "gt",
        ClientBinaryOp::Ge => "ge",
        ClientBinaryOp::And => "and",
        ClientBinaryOp::Or => "or",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Add,
            domain: super::types::IntegerDomain::I32,
        } => "checked_i32_add",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Sub,
            domain: super::types::IntegerDomain::I32,
        } => "checked_i32_sub",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Mul,
            domain: super::types::IntegerDomain::I32,
        } => "checked_i32_mul",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Div,
            domain: super::types::IntegerDomain::I32,
        } => "checked_i32_div",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Rem,
            domain: super::types::IntegerDomain::I32,
        } => "checked_i32_rem",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Add,
            domain: super::types::IntegerDomain::U32,
        } => "checked_u32_add",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Sub,
            domain: super::types::IntegerDomain::U32,
        } => "checked_u32_sub",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Mul,
            domain: super::types::IntegerDomain::U32,
        } => "checked_u32_mul",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Div,
            domain: super::types::IntegerDomain::U32,
        } => "checked_u32_div",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Rem,
            domain: super::types::IntegerDomain::U32,
        } => "checked_u32_rem",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Add,
            domain: super::types::IntegerDomain::I64,
        } => "checked_i64_add",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Sub,
            domain: super::types::IntegerDomain::I64,
        } => "checked_i64_sub",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Mul,
            domain: super::types::IntegerDomain::I64,
        } => "checked_i64_mul",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Div,
            domain: super::types::IntegerDomain::I64,
        } => "checked_i64_div",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Rem,
            domain: super::types::IntegerDomain::I64,
        } => "checked_i64_rem",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Add,
            domain: super::types::IntegerDomain::U64,
        } => "checked_u64_add",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Sub,
            domain: super::types::IntegerDomain::U64,
        } => "checked_u64_sub",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Mul,
            domain: super::types::IntegerDomain::U64,
        } => "checked_u64_mul",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Div,
            domain: super::types::IntegerDomain::U64,
        } => "checked_u64_div",
        ClientBinaryOp::Checked {
            op: CheckedIntegerOp::Rem,
            domain: super::types::IntegerDomain::U64,
        } => "checked_u64_rem",
        ClientBinaryOp::DynAdd => "dyn_add",
        ClientBinaryOp::DynSub => "dyn_sub",
        ClientBinaryOp::DynMul => "dyn_mul",
        ClientBinaryOp::DynDiv => "dyn_div",
        ClientBinaryOp::DynDivInt => "dyn_div_int",
        ClientBinaryOp::DynRem => "dyn_rem",
        ClientBinaryOp::DynEq => "dyn_eq",
        ClientBinaryOp::DynNe => "dyn_ne",
    }
}

fn client_unary_op(op: ClientUnaryOp) -> &'static str {
    match op {
        ClientUnaryOp::Not => "not",
        ClientUnaryOp::Neg => "neg",
        ClientUnaryOp::CheckedNeg(super::types::IntegerDomain::I32) => "checked_i32_neg",
        ClientUnaryOp::CheckedNeg(super::types::IntegerDomain::U32) => "checked_u32_neg",
        ClientUnaryOp::CheckedNeg(super::types::IntegerDomain::I64) => "checked_i64_neg",
        ClientUnaryOp::CheckedNeg(super::types::IntegerDomain::U64) => "checked_u64_neg",
    }
}

fn query_accessor(accessor: QueryAccessor) -> &'static str {
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

fn mutation_accessor(accessor: MutationAccessor) -> &'static str {
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

fn as_type(ty: AsType) -> &'static str {
    match ty {
        AsType::Str => "str",
        AsType::F64 => "f64",
        AsType::F64FromInt => "f64_from_int",
        AsType::I64 => "i64",
        AsType::Bool => "bool",
        AsType::Array => "array",
        AsType::Object => "object",
    }
}

fn json_pointer(path: &JsonPointer) -> &str {
    path.as_str()
}

fn storage_area(area: StorageArea) -> &'static str {
    match area {
        StorageArea::Local => "local",
        StorageArea::Session => "session",
    }
}

fn url_history_mode(mode: UrlHistoryMode) -> &'static str {
    match mode {
        UrlHistoryMode::Replace => "replace",
        UrlHistoryMode::Push => "push",
    }
}

fn event_file_source(source: EventFileSource) -> &'static str {
    match source {
        EventFileSource::Target => "target",
        EventFileSource::DataTransfer => "data_transfer",
    }
}

fn comma(out: &mut String, index: usize) {
    if index > 0 {
        out.push(',');
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use crate::client::{
        client_block_json, client_expr_json, parse_client_block_json_str,
        parse_client_expr_json_str, ClientBinaryOp, ClientBlock, ClientExpr, ClientLocalId,
        ClientSimpleClosure, ClientStmt, ClientType, IntegerDomain,
    };
    use crate::types::{QueryId, SignalId};

    #[test]
    fn writes_block_json_that_parses_back_to_client_ir() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(0),
                "next",
                ClientType::String,
                ClientExpr::string("a\"b\nc"),
            ),
            ClientStmt::SignalUpdate {
                id: SignalId(1),
                closure: ClientSimpleClosure::new(
                    "value",
                    ClientExpr::binary(
                        ClientBinaryOp::Add,
                        ClientExpr::local(ClientLocalId(0)),
                        ClientExpr::int(1),
                    ),
                ),
            },
            ClientStmt::QueryRefetch { query: QueryId(0) },
            ClientStmt::QueryInvalidate { query: QueryId(1) },
            ClientStmt::ForRange {
                local: ClientLocalId(1),
                name: "wide_index".to_owned(),
                integer_domain: IntegerDomain::U64,
                start: ClientExpr::Literal(crate::Value::BigInt(0)),
                end: ClientExpr::Literal(crate::Value::BigInt(3)),
                inclusive: false,
                body: ClientBlock::new(),
            },
        ]);

        let json = client_block_json(&block);

        assert!(json.contains("\"integer_domain\":\"u64\""), "{json}");
        assert_eq!(parse_client_block_json_str(&json).unwrap(), block);
    }

    #[test]
    fn writes_expr_json_that_parses_back_to_client_ir() {
        let expr = ClientExpr::binary(
            ClientBinaryOp::Eq,
            ClientExpr::signal_read(SignalId(0)),
            ClientExpr::int(42),
        );

        let json = client_expr_json(&expr);

        assert_eq!(parse_client_expr_json_str(&json).unwrap(), expr);
    }

    #[test]
    fn writes_string_transform_and_predicate_json_that_parses_back() {
        use crate::client::{StringPredicateOp, StringTransformOp};

        for op in [StringTransformOp::ToUppercase, StringTransformOp::Trim] {
            let expr = ClientExpr::StringTransform {
                op,
                target: Box::new(ClientExpr::StringToLowercase {
                    target: Box::new(ClientExpr::signal_read(SignalId(0))),
                }),
            };
            let json = client_expr_json(&expr);
            assert_eq!(parse_client_expr_json_str(&json).unwrap(), expr);
        }

        for op in [StringPredicateOp::StartsWith, StringPredicateOp::EndsWith] {
            let expr = ClientExpr::StringPredicate {
                op,
                haystack: Box::new(ClientExpr::signal_read(SignalId(0))),
                needle: Box::new(ClientExpr::string("ab")),
            };
            let json = client_expr_json(&expr);
            assert_eq!(parse_client_expr_json_str(&json).unwrap(), expr);
        }

        let upper_json = client_expr_json(&ClientExpr::StringTransform {
            op: StringTransformOp::ToUppercase,
            target: Box::new(ClientExpr::string("a")),
        });
        assert!(
            upper_json.contains("\"kind\":\"string_transform\""),
            "json: {upper_json}"
        );
        assert!(
            upper_json.contains("\"op\":\"to_uppercase\""),
            "json: {upper_json}"
        );

        let starts_json = client_expr_json(&ClientExpr::StringPredicate {
            op: StringPredicateOp::StartsWith,
            haystack: Box::new(ClientExpr::string("ab")),
            needle: Box::new(ClientExpr::string("a")),
        });
        assert!(
            starts_json.contains("\"kind\":\"string_predicate\""),
            "json: {starts_json}"
        );
        assert!(
            starts_json.contains("\"op\":\"starts_with\""),
            "json: {starts_json}"
        );
    }

    #[test]
    fn writes_unresolved_function_refs_for_module_resolution() {
        let expr = ClientExpr::call(
            crate::client::ClientFunctionRef::Unresolved("pricing::subtotal".to_string()),
            vec![ClientExpr::int(1)],
        );

        let json = client_expr_json(&expr);

        assert_eq!(parse_client_expr_json_str(&json).unwrap(), expr);
    }

    #[test]
    fn writes_helper_source_edges_that_parse_back() {
        let expr = ClientExpr::call(
            crate::client::ClientFunctionRef::Helper {
                edge: crate::abi::HelperCallEdgeId::new(9),
                authored_path: "public_api::RenamedHelper".to_string(),
                name: "RenamedHelper".to_string(),
            },
            vec![ClientExpr::int(1)],
        );

        let json = client_expr_json(&expr);

        assert_eq!(parse_client_expr_json_str(&json).unwrap(), expr);
        assert!(
            json.contains("\"kind\":\"helper\"")
                && json.contains("\"edge\":9")
                && json.contains("\"authored_path\":\"public_api::RenamedHelper\""),
            "helper edge metadata should be explicit in descriptor JSON: {json}"
        );
    }
}
