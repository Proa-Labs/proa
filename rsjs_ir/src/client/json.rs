// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! JSON ingestion for Client IR v3 sidecars.
//!
//! Component descriptors and client-helper manifests both persist Client IR v3
//! blocks as JSON. Keeping the parser in `rsjs_ir` makes those sidecars parse
//! through the same contract that emitters and optimization passes consume.

use std::fmt;

use serde_json::Value as JsonValue;

use super::expr::{
    AsType, BrowserStorageArea, EncodingOp, EventAttributeSource, EventCollection,
    EventCollectionField, EventValue, JsonPointer, LocationField, MutationAccessor,
    NodeRefAccessor, QueryAccessor, StorageValueType, StringPredicateOp, StringTransformOp,
    WindowField,
};
use crate::reactive::{ConsoleLevel, EventFileSource, StorageArea, UrlHistoryMode};
use crate::types::{CallbackSlotId, MutationId, NodeRefId, QueryId, SignalId};
use crate::value::Value;

use super::expr::{
    CheckedIntegerOp, ClientBinaryOp, ClientBrowserRead, ClientExpr, ClientFunctionId,
    ClientFunctionRef, ClientLocalId, ClientStructFieldValue, ClientUnaryOp,
};
use super::stmt::{
    ClientBlock, ClientBrowserEffect, ClientNodeRefEffect, ClientPlace, ClientSimpleClosure,
    ClientStmt,
};
use super::types::{ClientObjectField, ClientType, IntegerDomain};
use crate::abi::HelperCallEdgeId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientJsonError {
    message: String,
}

impl ClientJsonError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ClientJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientJsonError {}

impl From<ClientJsonError> for String {
    fn from(value: ClientJsonError) -> Self {
        value.message
    }
}

pub type ClientJsonResult<T> = Result<T, ClientJsonError>;
type UnresolvedFunctionResolver<'a> = dyn Fn(&str) -> Option<ClientFunctionRef> + 'a;
type TypeNameResolver<'a> = dyn Fn(&str) -> ClientType + 'a;

#[derive(Clone, Copy, Default)]
pub struct ClientJsonParseOptions<'a> {
    unresolved_function_ref: Option<&'a UnresolvedFunctionResolver<'a>>,
    type_name_resolver: Option<&'a TypeNameResolver<'a>>,
}

impl<'a> ClientJsonParseOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_unresolved_function_ref(
        mut self,
        resolver: &'a dyn Fn(&str) -> Option<ClientFunctionRef>,
    ) -> Self {
        self.unresolved_function_ref = Some(resolver);
        self
    }

    pub fn with_type_name_resolver(mut self, resolver: &'a dyn Fn(&str) -> ClientType) -> Self {
        self.type_name_resolver = Some(resolver);
        self
    }
}

pub fn parse_client_body_json(value: &JsonValue) -> ClientJsonResult<ClientBlock> {
    parse_client_body_json_with_options(value, &ClientJsonParseOptions::new())
}

/// Parse a [`ClientType`] from the `{"kind": ...}` shape
/// `write_client_type_json` emits. Used by the component-descriptor
/// parser for [`crate::reactive::SignalDecl::ty`].
pub fn parse_client_type_json(value: &JsonValue) -> ClientJsonResult<ClientType> {
    parse_client_type(value, &ClientJsonParseOptions::new())
}

pub fn parse_client_body_json_str(text: &str) -> ClientJsonResult<ClientBlock> {
    parse_client_body_json_str_with_options(text, &ClientJsonParseOptions::new())
}

pub fn parse_client_body_json_str_with_options(
    text: &str,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientBlock> {
    let value = parse_json_value(text)?;
    parse_client_body_json_with_options(&value, options)
}

pub fn parse_client_body_json_with_options(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientBlock> {
    if string_field(value, "kind")? == "block" {
        return parse_client_block_json_with_options(value, options);
    }
    Ok(ClientBlock::from_stmts(vec![ClientStmt::Return(Some(
        parse_client_expr_json_with_options(value, options)?,
    ))]))
}

pub fn parse_client_block_json(value: &JsonValue) -> ClientJsonResult<ClientBlock> {
    parse_client_block_json_with_options(value, &ClientJsonParseOptions::new())
}

pub fn parse_client_block_json_str(text: &str) -> ClientJsonResult<ClientBlock> {
    parse_client_block_json_str_with_options(text, &ClientJsonParseOptions::new())
}

pub fn parse_client_block_json_str_with_options(
    text: &str,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientBlock> {
    let value = parse_json_value(text)?;
    parse_client_block_json_with_options(&value, options)
}

pub fn parse_client_block_json_with_options(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientBlock> {
    if string_field(value, "kind")? != "block" {
        return Err(ClientJsonError::new(
            "Client IR v3 body must be a block JSON object",
        ));
    }
    Ok(ClientBlock::from_stmts(
        array_field(value, "stmts")?
            .iter()
            .map(|stmt| parse_client_stmt(stmt, options))
            .collect::<ClientJsonResult<Vec<_>>>()?,
    ))
}

pub fn parse_client_expr_json(value: &JsonValue) -> ClientJsonResult<ClientExpr> {
    parse_client_expr_json_with_options(value, &ClientJsonParseOptions::new())
}

pub fn parse_client_expr_json_str(text: &str) -> ClientJsonResult<ClientExpr> {
    parse_client_expr_json_str_with_options(text, &ClientJsonParseOptions::new())
}

pub fn parse_client_expr_json_str_with_options(
    text: &str,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientExpr> {
    let value = parse_json_value(text)?;
    parse_client_expr_json_with_options(&value, options)
}

pub fn parse_client_expr_json_with_options(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientExpr> {
    match string_field(value, "kind")? {
        "unit" => Ok(ClientExpr::Unit),
        "literal" => Ok(ClientExpr::Literal(parse_value(field(value, "value")?)?)),
        "array_lit" => {
            let mut elems = Vec::new();
            for elem in array_field(value, "elems")? {
                elems.push(parse_value(elem)?);
            }
            Ok(ClientExpr::ArrayLit(elems))
        }
        "query_key" => {
            let mut parts = Vec::new();
            for part in array_field(value, "parts")? {
                parts.push(parse_client_expr_json_with_options(part, options)?);
            }
            Ok(ClientExpr::QueryKey(parts))
        }
        "local" => Ok(ClientExpr::Local(ClientLocalId(id_field(value, "local")?))),
        "signal_read" => Ok(ClientExpr::SignalRead(SignalId(id_field(value, "signal")?))),
        "query_read" => Ok(ClientExpr::QueryRead {
            id: QueryId(id_field(value, "query")?),
            accessor: parse_query_accessor(string_field(value, "accessor")?)?,
        }),
        "mutation_read" => Ok(ClientExpr::MutationRead {
            id: MutationId(id_field(value, "mutation")?),
            accessor: parse_mutation_accessor(string_field(value, "accessor")?)?,
        }),
        "event_read" => Ok(ClientExpr::EventRead(parse_event_value(string_field(
            value, "value",
        )?)?)),
        "event_current_target" => Ok(ClientExpr::EventCurrentTarget),
        "dom_read" => Ok(ClientExpr::DomRead {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
            accessor: parse_node_ref_accessor(value)?,
        }),
        "event_collection_len" => Ok(ClientExpr::EventCollectionLen(parse_event_collection(
            string_field(value, "collection")?,
        )?)),
        "event_collection_read" => Ok(ClientExpr::EventCollectionRead {
            collection: parse_event_collection(string_field(value, "collection")?)?,
            index: Box::new(parse_client_expr_json_with_options(
                field(value, "index")?,
                options,
            )?),
            field: parse_event_collection_field(string_field(value, "field")?)?,
        }),
        "node_ref_read" => Ok(ClientExpr::NodeRefRead {
            id: NodeRefId(id_field(value, "ref")?),
            accessor: parse_node_ref_accessor(value)?,
        }),
        "event_target_within_node_ref" => Ok(ClientExpr::EventTargetWithinNodeRef {
            id: NodeRefId(id_field(value, "ref")?),
            inside: bool_field(value, "inside")?,
        }),
        "event_attribute" => Ok(ClientExpr::EventAttribute {
            source: parse_event_attribute_source(string_field(value, "source")?)?,
            attr: string_field(value, "attr")?.to_string(),
        }),
        "event_payload" => Ok(ClientExpr::EventPayload {
            attr: string_field(value, "attr")?.to_string(),
        }),
        "callback_payload" => Ok(ClientExpr::CallbackPayload),
        "node_payload" => Ok(ClientExpr::NodePayload {
            attr: string_field(value, "attr")?.to_string(),
            tokens: string_field(value, "tokens")?.to_string(),
        }),
        "browser_read" => Ok(ClientExpr::BrowserRead(parse_client_browser_read(
            value, options,
        )?)),
        "encoding" => Ok(ClientExpr::Encoding {
            op: parse_encoding_op(string_field(value, "op")?)?,
            value: Box::new(parse_client_expr_json_with_options(
                field(value, "value")?,
                options,
            )?),
        }),
        "conditional" => Ok(ClientExpr::Conditional {
            condition: Box::new(parse_client_expr_json_with_options(
                field(value, "condition")?,
                options,
            )?),
            then_expr: Box::new(parse_client_expr_json_with_options(
                field(value, "then")?,
                options,
            )?),
            else_expr: Box::new(parse_client_expr_json_with_options(
                field(value, "else")?,
                options,
            )?),
        }),
        "is_non_empty" => Ok(ClientExpr::IsNonEmpty {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
        }),
        "string_to_lowercase" => Ok(ClientExpr::StringToLowercase {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
        }),
        "string_contains" => Ok(ClientExpr::StringContains {
            haystack: Box::new(parse_client_expr_json_with_options(
                field(value, "haystack")?,
                options,
            )?),
            needle: Box::new(parse_client_expr_json_with_options(
                field(value, "needle")?,
                options,
            )?),
        }),
        "string_transform" => Ok(ClientExpr::StringTransform {
            op: parse_string_transform_op(string_field(value, "op")?)?,
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
        }),
        "string_predicate" => Ok(ClientExpr::StringPredicate {
            op: parse_string_predicate_op(string_field(value, "op")?)?,
            haystack: Box::new(parse_client_expr_json_with_options(
                field(value, "haystack")?,
                options,
            )?),
            needle: Box::new(parse_client_expr_json_with_options(
                field(value, "needle")?,
                options,
            )?),
        }),
        "project" => Ok(ClientExpr::Project {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
            path: JsonPointer::new(string_field(value, "path")?),
        }),
        "as_type" => Ok(ClientExpr::AsType {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
            ty: parse_as_type(string_field(value, "ty")?)?,
        }),
        "is_present" => Ok(ClientExpr::IsPresent {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
        }),
        "eq" => Ok(ClientExpr::Eq {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
            value: parse_value(field(value, "value")?)?,
        }),
        "neq" => Ok(ClientExpr::Neq {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
            value: parse_value(field(value, "value")?)?,
        }),
        "coalesce" => Ok(ClientExpr::Coalesce {
            target: Box::new(parse_client_expr_json_with_options(
                field(value, "target")?,
                options,
            )?),
            fallback: parse_value(field(value, "fallback")?)?,
        }),
        "unary" => Ok(ClientExpr::Unary {
            op: parse_client_unary_op(string_field(value, "op")?)?,
            expr: Box::new(parse_client_expr_json_with_options(
                field(value, "expr")?,
                options,
            )?),
        }),
        "binary" => Ok(ClientExpr::Binary {
            op: parse_client_binary_op(string_field(value, "op")?)?,
            left: Box::new(parse_client_expr_json_with_options(
                field(value, "left")?,
                options,
            )?),
            right: Box::new(parse_client_expr_json_with_options(
                field(value, "right")?,
                options,
            )?),
        }),
        "struct_init" => Ok(ClientExpr::StructInit {
            type_name: string_field(value, "type_name")?.to_string(),
            fields: array_field(value, "fields")?
                .iter()
                .map(|field_value| {
                    Ok(ClientStructFieldValue {
                        name: string_field(field_value, "name")?.to_string(),
                        value: parse_client_expr_json_with_options(
                            field(field_value, "value")?,
                            options,
                        )?,
                    })
                })
                .collect::<ClientJsonResult<Vec<_>>>()?,
        }),
        "field" => Ok(ClientExpr::Field {
            base: Box::new(parse_client_expr_json_with_options(
                field(value, "base")?,
                options,
            )?),
            name: string_field(value, "name")?.to_string(),
        }),
        "index" => Ok(ClientExpr::Index {
            base: Box::new(parse_client_expr_json_with_options(
                field(value, "base")?,
                options,
            )?),
            index: Box::new(parse_client_expr_json_with_options(
                field(value, "index")?,
                options,
            )?),
        }),
        "call" => Ok(ClientExpr::Call {
            function: parse_client_function_ref(field(value, "function")?, options)?,
            args: array_field(value, "args")?
                .iter()
                .map(|arg| parse_client_expr_json_with_options(arg, options))
                .collect::<ClientJsonResult<Vec<_>>>()?,
        }),
        other => Err(ClientJsonError::new(format!(
            "unsupported Client IR v3 expression kind {other}"
        ))),
    }
}

pub fn client_type_from_name(name: &str) -> ClientType {
    match name {
        "()" => ClientType::Unit,
        "bool" => ClientType::Bool,
        "i8" | "i16" | "i32" => ClientType::I32,
        "u8" | "u16" | "u32" => ClientType::U32,
        "i64" | "i128" | "isize" => ClientType::I64,
        "u64" | "u128" | "usize" => ClientType::U64,
        "f32" => ClientType::F32,
        "f64" => ClientType::F64,
        "String" | "&str" | "str" | "string" => ClientType::String,
        "unknown" => ClientType::Unknown,
        _ => ClientType::Unknown,
    }
}

fn parse_client_stmt(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientStmt> {
    match string_field(value, "kind")? {
        "expr" => Ok(ClientStmt::Expr(parse_client_expr_json_with_options(
            field(value, "expr")?,
            options,
        )?)),
        "let" => Ok(ClientStmt::Let {
            local: ClientLocalId(id_field(value, "local")?),
            name: string_field(value, "name")?.to_string(),
            ty: parse_client_type(field(value, "ty")?, options)?,
            value: optional_field(value, "value")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?,
        }),
        "assign" => Ok(ClientStmt::Assign {
            local: ClientLocalId(id_field(value, "local")?),
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        }),
        "assign_place" => Ok(ClientStmt::AssignPlace {
            place: parse_client_place(field(value, "place")?)?,
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        }),
        "signal_set" => Ok(ClientStmt::SignalSet {
            id: SignalId(id_field(value, "signal")?),
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        }),
        "signal_update" => Ok(ClientStmt::SignalUpdate {
            id: SignalId(id_field(value, "signal")?),
            closure: parse_client_closure(field(value, "closure")?, options)?,
        }),
        "event_prevent_default" => Ok(ClientStmt::EventPreventDefault),
        "event_stop_propagation" => Ok(ClientStmt::EventStopPropagation),
        "console" => Ok(ClientStmt::Console {
            level: parse_console_level(string_field(value, "level")?)?,
            args: array_field(value, "args")?
                .iter()
                .map(|arg| parse_client_expr_json_with_options(arg, options))
                .collect::<ClientJsonResult<Vec<_>>>()?,
        }),
        "callback_invoke" => Ok(ClientStmt::CallbackInvoke {
            callback: CallbackSlotId(id_field(value, "callback")?),
            payload: parse_client_expr_json_with_options(field(value, "payload")?, options)?,
        }),
        "run_mutation" => Ok(ClientStmt::RunMutation {
            mutation: MutationId(id_field(value, "mutation")?),
            args: array_field(value, "args")?
                .iter()
                .map(|arg| parse_client_expr_json_with_options(arg, options))
                .collect::<ClientJsonResult<Vec<_>>>()?,
        }),
        "query_refetch" => Ok(ClientStmt::QueryRefetch {
            query: QueryId(id_field(value, "query")?),
        }),
        "query_invalidate" => Ok(ClientStmt::QueryInvalidate {
            query: QueryId(id_field(value, "query")?),
        }),
        "browser_effect" => Ok(ClientStmt::BrowserEffect(parse_client_browser_effect(
            field(value, "effect")?,
            options,
        )?)),
        "node_ref_effect" => Ok(ClientStmt::NodeRefEffect {
            id: NodeRefId(id_field(value, "ref")?),
            effect: parse_client_node_ref_effect(field(value, "effect")?, options)?,
        }),
        "return" => Ok(ClientStmt::Return(
            optional_field(value, "value")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?,
        )),
        "nested_block" => Ok(ClientStmt::Block(parse_client_block_json_with_options(
            field(value, "block")?,
            options,
        )?)),
        "if" => Ok(ClientStmt::If {
            condition: parse_client_expr_json_with_options(field(value, "condition")?, options)?,
            then_block: parse_client_block_json_with_options(field(value, "then")?, options)?,
            else_block: optional_field(value, "else")?
                .map(|block| parse_client_block_json_with_options(block, options))
                .transpose()?,
        }),
        "for_range" => Ok(ClientStmt::ForRange {
            local: ClientLocalId(id_field(value, "local")?),
            name: string_field(value, "name")?.to_string(),
            integer_domain: parse_integer_domain_field(value, "integer_domain")?,
            start: parse_client_expr_json_with_options(field(value, "start")?, options)?,
            end: parse_client_expr_json_with_options(field(value, "end")?, options)?,
            inclusive: bool_field(value, "inclusive")?,
            body: parse_client_block_json_with_options(field(value, "body")?, options)?,
        }),
        other => Err(ClientJsonError::new(format!(
            "unsupported Client IR v3 statement kind {other}"
        ))),
    }
}

fn parse_client_place(value: &JsonValue) -> ClientJsonResult<ClientPlace> {
    match string_field(value, "kind")? {
        "local" => Ok(ClientPlace::Local(ClientLocalId(id_field(value, "local")?))),
        "field" => Ok(ClientPlace::Field {
            base: Box::new(parse_client_place(field(value, "base")?)?),
            name: string_field(value, "name")?.to_string(),
        }),
        other => Err(ClientJsonError::new(format!(
            "unsupported Client IR place kind {other}"
        ))),
    }
}

fn parse_client_function_ref(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientFunctionRef> {
    match string_field(value, "kind")? {
        "local" => Ok(ClientFunctionRef::Local(ClientFunctionId(id_field(
            value, "function",
        )?))),
        "import" => Ok(ClientFunctionRef::Import {
            module: string_field(value, "module")?.to_string(),
            name: string_field(value, "name")?.to_string(),
        }),
        "helper" => Ok(ClientFunctionRef::Helper {
            edge: HelperCallEdgeId::new(id_field(value, "edge")?),
            authored_path: string_field(value, "authored_path")?.to_string(),
            name: string_field(value, "name")?.to_string(),
        }),
        "runtime" => Ok(ClientFunctionRef::Runtime(
            string_field(value, "name")?.to_string(),
        )),
        "unresolved" => {
            let path = string_field(value, "path")?;
            if let Some(resolve) = options.unresolved_function_ref {
                if let Some(resolved) = resolve(path) {
                    return Ok(resolved);
                }
            }
            Ok(ClientFunctionRef::Unresolved(path.to_string()))
        }
        other => Err(ClientJsonError::new(format!(
            "unsupported Client IR v3 function ref {other}"
        ))),
    }
}

fn parse_client_closure(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientSimpleClosure> {
    Ok(ClientSimpleClosure::new(
        string_field(value, "param")?,
        parse_client_expr_json_with_options(field(value, "body")?, options)?,
    ))
}

fn parse_client_type(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientType> {
    if let Some(name) = value.as_str() {
        return Ok(options
            .type_name_resolver
            .map_or_else(|| client_type_from_name(name), |resolve| resolve(name)));
    }

    Ok(match string_field(value, "kind")? {
        "unit" => ClientType::Unit,
        "bool" => ClientType::Bool,
        "i32" => ClientType::I32,
        "u32" => ClientType::U32,
        "i64" => ClientType::I64,
        "u64" => ClientType::U64,
        "f32" => ClientType::F32,
        "f64" => ClientType::F64,
        "string" => ClientType::String,
        "option" => ClientType::Option(Box::new(parse_client_type(
            field(value, "inner")?,
            options,
        )?)),
        "vec" => ClientType::Vec(Box::new(parse_client_type(
            field(value, "inner")?,
            options,
        )?)),
        "object" => {
            let mut fields = Vec::new();
            for field_value in array_field(value, "fields")? {
                fields.push(ClientObjectField {
                    name: string_field(field_value, "name")?.to_string(),
                    ty: parse_client_type(field(field_value, "ty")?, options)?,
                });
            }
            ClientType::Object(fields)
        }
        "value_object" => {
            let mut fields = Vec::new();
            for field_value in array_field(value, "fields")? {
                fields.push(ClientObjectField {
                    name: string_field(field_value, "name")?.to_string(),
                    ty: parse_client_type(field(field_value, "ty")?, options)?,
                });
            }
            ClientType::ValueObject {
                type_key: string_field(value, "type_key")?.to_string(),
                fields,
            }
        }
        "struct" => ClientType::Struct(string_field(value, "name")?.to_string()),
        "enum" => ClientType::Enum(string_field(value, "name")?.to_string()),
        "event" => ClientType::Event,
        "dom_element" => ClientType::DomElement,
        "signal" => ClientType::Signal(Box::new(parse_client_type(
            field(value, "inner")?,
            options,
        )?)),
        "callback" => ClientType::Callback(Box::new(parse_client_type(
            field(value, "payload")?,
            options,
        )?)),
        "dyn" => ClientType::Dyn,
        "unknown" => ClientType::Unknown,
        other => {
            return Err(ClientJsonError::new(format!(
                "unsupported Client IR v3 type {other}"
            )));
        }
    })
}

fn parse_client_browser_read(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientBrowserRead> {
    Ok(match string_field(value, "read")? {
        "window" => ClientBrowserRead::Window(parse_window_field(string_field(value, "field")?)?),
        "location" => {
            ClientBrowserRead::Location(parse_location_field(string_field(value, "field")?)?)
        }
        "storage" => ClientBrowserRead::Storage {
            area: parse_browser_storage_area(string_field(value, "area")?)?,
            value_type: optional_string_field(value, "value_type")?
                .map(parse_storage_value_type)
                .transpose()?
                .unwrap_or(StorageValueType::String),
            key: Box::new(parse_client_expr_json_with_options(
                field(value, "key")?,
                options,
            )?),
            fallback: optional_field(value, "fallback")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?
                .map(Box::new),
        },
        "url_search_param" => ClientBrowserRead::UrlSearchParam {
            key: Box::new(parse_client_expr_json_with_options(
                field(value, "key")?,
                options,
            )?),
            fallback: optional_field(value, "fallback")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?
                .map(Box::new),
        },
        "declared" => {
            let helper = string_field(value, "helper")?;
            if crate::browser_registry::find_by_id(helper).is_none() {
                return Err(ClientJsonError::new(format!(
                    "unknown declared browser helper {helper}; \
                     not present in rsjs_ir::browser_registry"
                )));
            }
            ClientBrowserRead::Declared {
                helper: helper.to_string(),
                args: array_field(value, "args")?
                    .iter()
                    .map(|arg| parse_client_expr_json_with_options(arg, options))
                    .collect::<ClientJsonResult<Vec<_>>>()?,
            }
        }
        other => {
            return Err(ClientJsonError::new(format!(
                "unsupported Client IR v3 browser read {other}"
            )));
        }
    })
}

fn parse_client_browser_effect(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientBrowserEffect> {
    Ok(match string_field(value, "kind")? {
        "window_scroll_to" => ClientBrowserEffect::WindowScrollTo {
            x: parse_client_expr_json_with_options(field(value, "x")?, options)?,
            y: parse_client_expr_json_with_options(field(value, "y")?, options)?,
        },
        "window_scroll_by" => ClientBrowserEffect::WindowScrollBy {
            x: parse_client_expr_json_with_options(field(value, "x")?, options)?,
            y: parse_client_expr_json_with_options(field(value, "y")?, options)?,
        },
        "window_print" => ClientBrowserEffect::WindowPrint,
        "location_set_hash" => ClientBrowserEffect::LocationSetHash {
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        },
        "location_assign" => ClientBrowserEffect::LocationAssign {
            url: parse_client_expr_json_with_options(field(value, "url")?, options)?,
        },
        "location_replace" => ClientBrowserEffect::LocationReplace {
            url: parse_client_expr_json_with_options(field(value, "url")?, options)?,
        },
        "location_reload" => ClientBrowserEffect::LocationReload,
        "history_push_state" => ClientBrowserEffect::HistoryPushState {
            url: parse_client_expr_json_with_options(field(value, "url")?, options)?,
        },
        "history_replace_state" => ClientBrowserEffect::HistoryReplaceState {
            url: parse_client_expr_json_with_options(field(value, "url")?, options)?,
        },
        "history_back" => ClientBrowserEffect::HistoryBack,
        "history_forward" => ClientBrowserEffect::HistoryForward,
        "history_go" => ClientBrowserEffect::HistoryGo {
            delta: parse_client_expr_json_with_options(field(value, "delta")?, options)?,
        },
        "document_dispatch_event" => ClientBrowserEffect::DocumentDispatchEvent {
            name: parse_client_expr_json_with_options(field(value, "name")?, options)?,
            detail: optional_field(value, "detail")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?,
        },
        "document_set_title" => ClientBrowserEffect::DocumentSetTitle {
            title: parse_client_expr_json_with_options(field(value, "title")?, options)?,
        },
        "clipboard_write_text" => ClientBrowserEffect::ClipboardWriteText {
            text: parse_client_expr_json_with_options(field(value, "text")?, options)?,
        },
        "clipboard_read_text_into" => ClientBrowserEffect::ClipboardReadTextInto {
            target: SignalId(id_field(value, "target")?),
            fallback: optional_field(value, "fallback")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?,
        },
        "webauthn_create_json_into" => ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json: parse_client_expr_json_with_options(
                field(value, "options_json")?,
                options,
            )?,
            target: SignalId(id_field(value, "target")?),
            fallback: optional_field(value, "fallback")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?,
        },
        "webauthn_get_json_into" => ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json: parse_client_expr_json_with_options(
                field(value, "options_json")?,
                options,
            )?,
            target: SignalId(id_field(value, "target")?),
            fallback: optional_field(value, "fallback")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?,
        },
        "event_file_text_into" => ClientBrowserEffect::EventFileTextInto {
            source: parse_event_file_source(string_field(value, "source")?)?,
            target: SignalId(id_field(value, "target")?),
            fallback: optional_field(value, "fallback")?
                .map(|expr| parse_client_expr_json_with_options(expr, options))
                .transpose()?,
        },
        "storage_set_item" => ClientBrowserEffect::StorageSetItem {
            area: parse_storage_area(string_field(value, "area")?)?,
            key: parse_client_expr_json_with_options(field(value, "key")?, options)?,
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        },
        "storage_remove_item" => ClientBrowserEffect::StorageRemoveItem {
            area: parse_storage_area(string_field(value, "area")?)?,
            key: parse_client_expr_json_with_options(field(value, "key")?, options)?,
        },
        "url_set_search_param" => ClientBrowserEffect::UrlSetSearchParam {
            mode: parse_url_history_mode(value)?,
            key: parse_client_expr_json_with_options(field(value, "key")?, options)?,
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        },
        "url_delete_search_param" => ClientBrowserEffect::UrlDeleteSearchParam {
            mode: parse_url_history_mode(value)?,
            key: parse_client_expr_json_with_options(field(value, "key")?, options)?,
        },
        "declared" => {
            let helper = string_field(value, "helper")?;
            if crate::browser_registry::find_by_id(helper).is_none() {
                return Err(ClientJsonError::new(format!(
                    "unknown declared browser helper {helper}; \
                     not present in rsjs_ir::browser_registry"
                )));
            }
            ClientBrowserEffect::Declared {
                helper: helper.to_string(),
                args: array_field(value, "args")?
                    .iter()
                    .map(|arg| parse_client_expr_json_with_options(arg, options))
                    .collect::<ClientJsonResult<Vec<_>>>()?,
            }
        }
        other => {
            return Err(ClientJsonError::new(format!(
                "unsupported Client IR v3 browser effect {other}"
            )));
        }
    })
}

fn parse_client_node_ref_effect(
    value: &JsonValue,
    options: &ClientJsonParseOptions<'_>,
) -> ClientJsonResult<ClientNodeRefEffect> {
    Ok(match string_field(value, "kind")? {
        "focus" => ClientNodeRefEffect::Focus,
        "blur" => ClientNodeRefEffect::Blur,
        "scroll_into_view" => ClientNodeRefEffect::ScrollIntoView,
        "click" => ClientNodeRefEffect::Click,
        "request_submit" => ClientNodeRefEffect::RequestSubmit,
        "reset" => ClientNodeRefEffect::Reset,
        "report_validity" => ClientNodeRefEffect::ReportValidity,
        "show_modal" => ClientNodeRefEffect::ShowModal,
        "close" => ClientNodeRefEffect::Close,
        "show_popover" => ClientNodeRefEffect::ShowPopover,
        "hide_popover" => ClientNodeRefEffect::HidePopover,
        "toggle_popover" => ClientNodeRefEffect::TogglePopover,
        "scroll_to" => ClientNodeRefEffect::ScrollTo {
            x: parse_client_expr_json_with_options(field(value, "x")?, options)?,
            y: parse_client_expr_json_with_options(field(value, "y")?, options)?,
        },
        "scroll_by" => ClientNodeRefEffect::ScrollBy {
            x: parse_client_expr_json_with_options(field(value, "x")?, options)?,
            y: parse_client_expr_json_with_options(field(value, "y")?, options)?,
        },
        "set_pointer_capture" => ClientNodeRefEffect::SetPointerCapture(
            parse_client_expr_json_with_options(field(value, "value")?, options)?,
        ),
        "release_pointer_capture" => ClientNodeRefEffect::ReleasePointerCapture(
            parse_client_expr_json_with_options(field(value, "value")?, options)?,
        ),
        "select" => ClientNodeRefEffect::Select,
        "set_value" => ClientNodeRefEffect::SetValue(parse_client_expr_json_with_options(
            field(value, "value")?,
            options,
        )?),
        "set_checked" => ClientNodeRefEffect::SetChecked(parse_client_expr_json_with_options(
            field(value, "value")?,
            options,
        )?),
        "set_custom_validity" => ClientNodeRefEffect::SetCustomValidity(
            parse_client_expr_json_with_options(field(value, "value")?, options)?,
        ),
        "set_selection_range" => ClientNodeRefEffect::SetSelectionRange(
            parse_client_expr_json_with_options(field(value, "start")?, options)?,
            parse_client_expr_json_with_options(field(value, "end")?, options)?,
        ),
        "set_text_content" => ClientNodeRefEffect::SetTextContent(
            parse_client_expr_json_with_options(field(value, "value")?, options)?,
        ),
        "add_class" => ClientNodeRefEffect::AddClass(parse_client_expr_json_with_options(
            field(value, "value")?,
            options,
        )?),
        "remove_class" => ClientNodeRefEffect::RemoveClass(parse_client_expr_json_with_options(
            field(value, "value")?,
            options,
        )?),
        "toggle_class" => ClientNodeRefEffect::ToggleClass(parse_client_expr_json_with_options(
            field(value, "value")?,
            options,
        )?),
        "set_attribute" => ClientNodeRefEffect::SetAttribute {
            name: string_field(value, "name")?.to_string(),
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        },
        "remove_attribute" => ClientNodeRefEffect::RemoveAttribute {
            name: string_field(value, "name")?.to_string(),
        },
        "set_style_property" => ClientNodeRefEffect::SetStyleProperty {
            name: string_field(value, "name")?.to_string(),
            value: parse_client_expr_json_with_options(field(value, "value")?, options)?,
        },
        other => {
            return Err(ClientJsonError::new(format!(
                "unsupported Client IR v3 node-ref effect {other}"
            )));
        }
    })
}

fn parse_value(value: &JsonValue) -> ClientJsonResult<Value> {
    if let Ok(kind) = string_field(value, "kind") {
        return match kind {
            "bool" => Ok(Value::Bool(bool_field(value, "value")?)),
            "int" => Ok(Value::Int(i64_field(value, "value")?)),
            "bigint" => Ok(Value::BigInt(i128_field(value, "value")?)),
            "float" => match field(value, "value")? {
                JsonValue::Number(number) => number
                    .as_f64()
                    .map(Value::Float)
                    .ok_or_else(|| ClientJsonError::new("float value is not representable as f64")),
                JsonValue::String(s) => s
                    .parse::<f64>()
                    .map(Value::Float)
                    .map_err(|err| ClientJsonError::new(err.to_string())),
                _ => Err(ClientJsonError::new(
                    "float value must be a number or string",
                )),
            },
            "string" => Ok(Value::String(string_field(value, "value")?.to_string())),
            "null" => Ok(Value::Null),
            other => Err(ClientJsonError::new(format!(
                "unsupported value kind {other}"
            ))),
        };
    }

    match string_field(value, "v")? {
        "Null" => Ok(Value::Null),
        "Bool" => Ok(Value::Bool(bool_field(value, "data")?)),
        "Int" => Ok(Value::Int(i64_field(value, "data")?)),
        "Float" => match field(value, "data")? {
            JsonValue::Number(number) => number
                .as_f64()
                .map(Value::Float)
                .ok_or_else(|| ClientJsonError::new("float data is not representable as f64")),
            JsonValue::String(s) => s
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|err| ClientJsonError::new(err.to_string())),
            _ => Err(ClientJsonError::new(
                "literal float data must be a number or string",
            )),
        },
        "String" => Ok(Value::String(string_field(value, "data")?.to_string())),
        other => Err(ClientJsonError::new(format!(
            "unsupported Client IR v3 literal {other}"
        ))),
    }
}

fn parse_console_level(value: &str) -> ClientJsonResult<ConsoleLevel> {
    match value {
        "log" => Ok(ConsoleLevel::Log),
        "info" => Ok(ConsoleLevel::Info),
        "debug" => Ok(ConsoleLevel::Debug),
        "warn" => Ok(ConsoleLevel::Warn),
        "error" => Ok(ConsoleLevel::Error),
        "table" => Ok(ConsoleLevel::Table),
        "time" => Ok(ConsoleLevel::Time),
        "time_end" => Ok(ConsoleLevel::TimeEnd),
        "group" => Ok(ConsoleLevel::Group),
        "group_end" => Ok(ConsoleLevel::GroupEnd),
        other => Err(ClientJsonError::new(format!(
            "unsupported console level {other}"
        ))),
    }
}

fn parse_event_value(value: &str) -> ClientJsonResult<EventValue> {
    match value {
        "type" => Ok(EventValue::Type),
        "target_value" => Ok(EventValue::TargetValue),
        "target_checked" => Ok(EventValue::TargetChecked),
        "target_date" => Ok(EventValue::TargetDate),
        "target_id" => Ok(EventValue::TargetId),
        "current_target_id" => Ok(EventValue::CurrentTargetId),
        "current_target_value" => Ok(EventValue::CurrentTargetValue),
        "current_target_checked" => Ok(EventValue::CurrentTargetChecked),
        "key" => Ok(EventValue::Key),
        "code" => Ok(EventValue::Code),
        "repeat" => Ok(EventValue::Repeat),
        "shift_key" => Ok(EventValue::ShiftKey),
        "meta_key" => Ok(EventValue::MetaKey),
        "ctrl_key" => Ok(EventValue::CtrlKey),
        "alt_key" => Ok(EventValue::AltKey),
        "client_x" => Ok(EventValue::ClientX),
        "client_y" => Ok(EventValue::ClientY),
        "page_x" => Ok(EventValue::PageX),
        "page_y" => Ok(EventValue::PageY),
        "screen_x" => Ok(EventValue::ScreenX),
        "screen_y" => Ok(EventValue::ScreenY),
        "offset_x" => Ok(EventValue::OffsetX),
        "offset_y" => Ok(EventValue::OffsetY),
        "movement_x" => Ok(EventValue::MovementX),
        "movement_y" => Ok(EventValue::MovementY),
        "button" => Ok(EventValue::Button),
        "buttons" => Ok(EventValue::Buttons),
        "detail" => Ok(EventValue::Detail),
        "detail_u32" => Ok(EventValue::DetailU32),
        "pointer_id" => Ok(EventValue::PointerId),
        "pointer_type" => Ok(EventValue::PointerType),
        "is_primary" => Ok(EventValue::IsPrimary),
        "pressure" => Ok(EventValue::Pressure),
        "tangential_pressure" => Ok(EventValue::TangentialPressure),
        "tilt_x" => Ok(EventValue::TiltX),
        "tilt_y" => Ok(EventValue::TiltY),
        "twist" => Ok(EventValue::Twist),
        "pointer_width" => Ok(EventValue::PointerWidth),
        "pointer_height" => Ok(EventValue::PointerHeight),
        "delta_x" => Ok(EventValue::DeltaX),
        "delta_y" => Ok(EventValue::DeltaY),
        "delta_z" => Ok(EventValue::DeltaZ),
        "delta_mode" => Ok(EventValue::DeltaMode),
        "current_target_scroll_top" => Ok(EventValue::CurrentTargetScrollTop),
        "current_target_scroll_left" => Ok(EventValue::CurrentTargetScrollLeft),
        "input_type" => Ok(EventValue::InputType),
        "data" => Ok(EventValue::Data),
        "old_state" => Ok(EventValue::OldState),
        "new_state" => Ok(EventValue::NewState),
        "animation_name" => Ok(EventValue::AnimationName),
        "property_name" => Ok(EventValue::PropertyName),
        "elapsed_time" => Ok(EventValue::ElapsedTime),
        "pseudo_element" => Ok(EventValue::PseudoElement),
        "storage_key" => Ok(EventValue::StorageKey),
        "storage_old_value" => Ok(EventValue::StorageOldValue),
        "storage_new_value" => Ok(EventValue::StorageNewValue),
        "storage_url" => Ok(EventValue::StorageUrl),
        "storage_area" => Ok(EventValue::StorageArea),
        "old_url" => Ok(EventValue::OldUrl),
        "new_url" => Ok(EventValue::NewUrl),
        "clipboard_text" => Ok(EventValue::ClipboardText),
        "data_transfer_text" => Ok(EventValue::DataTransferText),
        "data_transfer_file_count" => Ok(EventValue::DataTransferFileCount),
        "data_transfer_first_file_name" => Ok(EventValue::DataTransferFirstFileName),
        "data_transfer_first_file_type" => Ok(EventValue::DataTransferFirstFileType),
        "data_transfer_first_file_size" => Ok(EventValue::DataTransferFirstFileSize),
        "target_file_count" => Ok(EventValue::TargetFileCount),
        "target_first_file_name" => Ok(EventValue::TargetFirstFileName),
        "target_first_file_type" => Ok(EventValue::TargetFirstFileType),
        "target_first_file_size" => Ok(EventValue::TargetFirstFileSize),
        "changed_touch_identifier" => Ok(EventValue::ChangedTouchIdentifier),
        "changed_touch_client_x" => Ok(EventValue::ChangedTouchClientX),
        "changed_touch_client_y" => Ok(EventValue::ChangedTouchClientY),
        "changed_touch_page_x" => Ok(EventValue::ChangedTouchPageX),
        "changed_touch_page_y" => Ok(EventValue::ChangedTouchPageY),
        "changed_touch_screen_x" => Ok(EventValue::ChangedTouchScreenX),
        "changed_touch_screen_y" => Ok(EventValue::ChangedTouchScreenY),
        other => Err(ClientJsonError::new(format!(
            "unsupported event value {other}"
        ))),
    }
}

fn parse_event_collection(value: &str) -> ClientJsonResult<EventCollection> {
    match value {
        "changed_touches" => Ok(EventCollection::ChangedTouches),
        "target_touches" => Ok(EventCollection::TargetTouches),
        "touches" => Ok(EventCollection::Touches),
        "target_files" => Ok(EventCollection::TargetFiles),
        "data_transfer_files" => Ok(EventCollection::DataTransferFiles),
        "data_transfer_items" => Ok(EventCollection::DataTransferItems),
        other => Err(ClientJsonError::new(format!(
            "unsupported event collection {other}"
        ))),
    }
}

fn parse_event_collection_field(value: &str) -> ClientJsonResult<EventCollectionField> {
    match value {
        "touch_identifier" => Ok(EventCollectionField::TouchIdentifier),
        "touch_client_x" => Ok(EventCollectionField::TouchClientX),
        "touch_client_y" => Ok(EventCollectionField::TouchClientY),
        "touch_page_x" => Ok(EventCollectionField::TouchPageX),
        "touch_page_y" => Ok(EventCollectionField::TouchPageY),
        "touch_screen_x" => Ok(EventCollectionField::TouchScreenX),
        "touch_screen_y" => Ok(EventCollectionField::TouchScreenY),
        "file_name" => Ok(EventCollectionField::FileName),
        "file_type" => Ok(EventCollectionField::FileType),
        "file_size" => Ok(EventCollectionField::FileSize),
        "data_transfer_item_kind" => Ok(EventCollectionField::DataTransferItemKind),
        "data_transfer_item_type" => Ok(EventCollectionField::DataTransferItemType),
        other => Err(ClientJsonError::new(format!(
            "unsupported event collection field {other}"
        ))),
    }
}

fn parse_event_attribute_source(value: &str) -> ClientJsonResult<EventAttributeSource> {
    match value {
        "target" => Ok(EventAttributeSource::Target),
        "closest" => Ok(EventAttributeSource::Closest),
        "current_target" => Ok(EventAttributeSource::CurrentTarget),
        other => Err(ClientJsonError::new(format!(
            "unsupported event attribute source {other}"
        ))),
    }
}

fn parse_node_ref_accessor(value: &JsonValue) -> ClientJsonResult<NodeRefAccessor> {
    match string_field(value, "accessor")? {
        "id" => Ok(NodeRefAccessor::Id),
        "text_content" => Ok(NodeRefAccessor::TextContent),
        "value" => Ok(NodeRefAccessor::Value),
        "checked" => Ok(NodeRefAccessor::Checked),
        "check_validity" => Ok(NodeRefAccessor::CheckValidity),
        "validation_message" => Ok(NodeRefAccessor::ValidationMessage),
        "validity_valid" => Ok(NodeRefAccessor::ValidityValid),
        "will_validate" => Ok(NodeRefAccessor::WillValidate),
        "selection_start" => Ok(NodeRefAccessor::SelectionStart),
        "selection_end" => Ok(NodeRefAccessor::SelectionEnd),
        "scroll_top" => Ok(NodeRefAccessor::ScrollTop),
        "scroll_left" => Ok(NodeRefAccessor::ScrollLeft),
        "client_width" => Ok(NodeRefAccessor::ClientWidth),
        "client_height" => Ok(NodeRefAccessor::ClientHeight),
        "scroll_width" => Ok(NodeRefAccessor::ScrollWidth),
        "scroll_height" => Ok(NodeRefAccessor::ScrollHeight),
        "offset_width" => Ok(NodeRefAccessor::OffsetWidth),
        "offset_height" => Ok(NodeRefAccessor::OffsetHeight),
        "rect_x" => Ok(NodeRefAccessor::RectX),
        "rect_y" => Ok(NodeRefAccessor::RectY),
        "rect_top" => Ok(NodeRefAccessor::RectTop),
        "rect_right" => Ok(NodeRefAccessor::RectRight),
        "rect_bottom" => Ok(NodeRefAccessor::RectBottom),
        "rect_left" => Ok(NodeRefAccessor::RectLeft),
        "rect_width" => Ok(NodeRefAccessor::RectWidth),
        "rect_height" => Ok(NodeRefAccessor::RectHeight),
        "file_count" => Ok(NodeRefAccessor::FileCount),
        "first_file_name" => Ok(NodeRefAccessor::FirstFileName),
        "first_file_type" => Ok(NodeRefAccessor::FirstFileType),
        "first_file_size" => Ok(NodeRefAccessor::FirstFileSize),
        "attribute" => Ok(NodeRefAccessor::Attribute {
            name: string_field(value, "name")?.to_string(),
        }),
        "has_attribute" => Ok(NodeRefAccessor::HasAttribute {
            name: string_field(value, "name")?.to_string(),
        }),
        other => Err(ClientJsonError::new(format!(
            "unsupported node-ref accessor {other}"
        ))),
    }
}

fn parse_window_field(value: &str) -> ClientJsonResult<WindowField> {
    match value {
        "inner_width" => Ok(WindowField::InnerWidth),
        "inner_height" => Ok(WindowField::InnerHeight),
        "scroll_x" => Ok(WindowField::ScrollX),
        "scroll_y" => Ok(WindowField::ScrollY),
        other => Err(ClientJsonError::new(format!(
            "unsupported window field {other}"
        ))),
    }
}

fn parse_location_field(value: &str) -> ClientJsonResult<LocationField> {
    match value {
        "href" => Ok(LocationField::Href),
        "origin" => Ok(LocationField::Origin),
        "protocol" => Ok(LocationField::Protocol),
        "host" => Ok(LocationField::Host),
        "hostname" => Ok(LocationField::Hostname),
        "port" => Ok(LocationField::Port),
        "pathname" => Ok(LocationField::Pathname),
        "search" => Ok(LocationField::Search),
        "hash" => Ok(LocationField::Hash),
        other => Err(ClientJsonError::new(format!(
            "unsupported location field {other}"
        ))),
    }
}

fn parse_browser_storage_area(value: &str) -> ClientJsonResult<BrowserStorageArea> {
    match value {
        "local" => Ok(BrowserStorageArea::Local),
        "session" => Ok(BrowserStorageArea::Session),
        other => Err(ClientJsonError::new(format!(
            "unsupported browser storage area {other}"
        ))),
    }
}

fn parse_storage_value_type(value: &str) -> ClientJsonResult<StorageValueType> {
    match value {
        "string" => Ok(StorageValueType::String),
        "bool" => Ok(StorageValueType::Bool),
        "i64" => Ok(StorageValueType::I64),
        "f64" => Ok(StorageValueType::F64),
        other => Err(ClientJsonError::new(format!(
            "unsupported storage value type {other}"
        ))),
    }
}

fn parse_encoding_op(value: &str) -> ClientJsonResult<EncodingOp> {
    match value {
        "utf8_bytes" => Ok(EncodingOp::Utf8Bytes),
        "utf8_string" => Ok(EncodingOp::Utf8String),
        "base64_encode" => Ok(EncodingOp::Base64Encode),
        "base64_decode" => Ok(EncodingOp::Base64Decode),
        "base64url_encode" => Ok(EncodingOp::Base64UrlEncode),
        "base64url_decode" => Ok(EncodingOp::Base64UrlDecode),
        other => Err(ClientJsonError::new(format!(
            "unsupported encoding op {other}"
        ))),
    }
}

fn parse_string_transform_op(value: &str) -> ClientJsonResult<StringTransformOp> {
    match value {
        "to_uppercase" => Ok(StringTransformOp::ToUppercase),
        "trim" => Ok(StringTransformOp::Trim),
        other => Err(ClientJsonError::new(format!(
            "unsupported string transform op {other}"
        ))),
    }
}

fn parse_string_predicate_op(value: &str) -> ClientJsonResult<StringPredicateOp> {
    match value {
        "starts_with" => Ok(StringPredicateOp::StartsWith),
        "ends_with" => Ok(StringPredicateOp::EndsWith),
        other => Err(ClientJsonError::new(format!(
            "unsupported string predicate op {other}"
        ))),
    }
}

fn parse_storage_area(value: &str) -> ClientJsonResult<StorageArea> {
    match value {
        "local" => Ok(StorageArea::Local),
        "session" => Ok(StorageArea::Session),
        other => Err(ClientJsonError::new(format!(
            "unsupported storage area {other}"
        ))),
    }
}

fn parse_event_file_source(value: &str) -> ClientJsonResult<EventFileSource> {
    match value {
        "target" => Ok(EventFileSource::Target),
        "data_transfer" => Ok(EventFileSource::DataTransfer),
        other => Err(ClientJsonError::new(format!(
            "unsupported event file source {other}"
        ))),
    }
}

fn parse_url_history_mode(value: &JsonValue) -> ClientJsonResult<UrlHistoryMode> {
    let Some(value) = optional_string_field(value, "history")? else {
        return Ok(UrlHistoryMode::Replace);
    };
    match value {
        "replace" => Ok(UrlHistoryMode::Replace),
        "push" => Ok(UrlHistoryMode::Push),
        other => Err(ClientJsonError::new(format!(
            "unsupported URL history mode {other}"
        ))),
    }
}

fn parse_client_binary_op(value: &str) -> ClientJsonResult<ClientBinaryOp> {
    if let Some((domain, op)) = value
        .strip_prefix("checked_")
        .and_then(|rest| rest.split_once('_'))
    {
        let domain = IntegerDomain::from_name(domain).ok_or_else(|| {
            ClientJsonError::new(format!("unsupported checked integer domain {domain}"))
        })?;
        let op = match op {
            "add" => CheckedIntegerOp::Add,
            "sub" => CheckedIntegerOp::Sub,
            "mul" => CheckedIntegerOp::Mul,
            "div" => CheckedIntegerOp::Div,
            "rem" => CheckedIntegerOp::Rem,
            other => {
                return Err(ClientJsonError::new(format!(
                    "unsupported checked integer op {other}"
                )))
            }
        };
        return Ok(ClientBinaryOp::Checked { op, domain });
    }
    match value {
        "add" => Ok(ClientBinaryOp::Add),
        "sub" => Ok(ClientBinaryOp::Sub),
        "mul" => Ok(ClientBinaryOp::Mul),
        "div" => Ok(ClientBinaryOp::Div),
        "div_int" => Ok(ClientBinaryOp::DivInt),
        "rem" => Ok(ClientBinaryOp::Rem),
        "eq" => Ok(ClientBinaryOp::Eq),
        "ne" | "neq" => Ok(ClientBinaryOp::Ne),
        "lt" => Ok(ClientBinaryOp::Lt),
        "le" => Ok(ClientBinaryOp::Le),
        "gt" => Ok(ClientBinaryOp::Gt),
        "ge" => Ok(ClientBinaryOp::Ge),
        "and" => Ok(ClientBinaryOp::And),
        "or" => Ok(ClientBinaryOp::Or),
        "dyn_add" => Ok(ClientBinaryOp::DynAdd),
        "dyn_sub" => Ok(ClientBinaryOp::DynSub),
        "dyn_mul" => Ok(ClientBinaryOp::DynMul),
        "dyn_div" => Ok(ClientBinaryOp::DynDiv),
        "dyn_div_int" => Ok(ClientBinaryOp::DynDivInt),
        "dyn_rem" => Ok(ClientBinaryOp::DynRem),
        "dyn_eq" => Ok(ClientBinaryOp::DynEq),
        "dyn_ne" => Ok(ClientBinaryOp::DynNe),
        other => Err(ClientJsonError::new(format!(
            "unsupported Client IR v3 binary op {other}"
        ))),
    }
}

fn parse_client_unary_op(value: &str) -> ClientJsonResult<ClientUnaryOp> {
    if let Some(domain) = value
        .strip_prefix("checked_")
        .and_then(|rest| rest.strip_suffix("_neg"))
    {
        let domain = IntegerDomain::from_name(domain).ok_or_else(|| {
            ClientJsonError::new(format!("unsupported checked integer domain {domain}"))
        })?;
        return Ok(ClientUnaryOp::CheckedNeg(domain));
    }
    match value {
        "not" => Ok(ClientUnaryOp::Not),
        "neg" => Ok(ClientUnaryOp::Neg),
        other => Err(ClientJsonError::new(format!(
            "unsupported Client IR v3 unary op {other}"
        ))),
    }
}

fn parse_query_accessor(value: &str) -> ClientJsonResult<QueryAccessor> {
    match value {
        "data" => Ok(QueryAccessor::Data),
        "status" => Ok(QueryAccessor::Status),
        "is_loading" => Ok(QueryAccessor::IsLoading),
        "is_success" => Ok(QueryAccessor::IsSuccess),
        "is_error" => Ok(QueryAccessor::IsError),
        "is_refetching" => Ok(QueryAccessor::IsRefetching),
        "is_stale" => Ok(QueryAccessor::IsStale),
        "error" => Ok(QueryAccessor::Error),
        "data_updated_at" => Ok(QueryAccessor::DataUpdatedAt),
        "error_updated_at" => Ok(QueryAccessor::ErrorUpdatedAt),
        "fetch_count" => Ok(QueryAccessor::FetchCount),
        "download_loaded" => Ok(QueryAccessor::DownloadLoaded),
        "download_total" => Ok(QueryAccessor::DownloadTotal),
        "download_percent" => Ok(QueryAccessor::DownloadPercent),
        other => Err(ClientJsonError::new(format!(
            "unsupported query accessor {other}"
        ))),
    }
}

fn parse_mutation_accessor(value: &str) -> ClientJsonResult<MutationAccessor> {
    match value {
        "status" => Ok(MutationAccessor::Status),
        "is_idle" => Ok(MutationAccessor::IsIdle),
        "is_pending" => Ok(MutationAccessor::IsPending),
        "is_success" => Ok(MutationAccessor::IsSuccess),
        "is_error" => Ok(MutationAccessor::IsError),
        "data" => Ok(MutationAccessor::Data),
        "error" => Ok(MutationAccessor::Error),
        other => Err(ClientJsonError::new(format!(
            "unsupported mutation accessor {other}"
        ))),
    }
}

fn parse_as_type(value: &str) -> ClientJsonResult<AsType> {
    match value {
        "str" => Ok(AsType::Str),
        "f64" => Ok(AsType::F64),
        "f64_from_int" => Ok(AsType::F64FromInt),
        "i64" => Ok(AsType::I64),
        "bool" => Ok(AsType::Bool),
        "array" => Ok(AsType::Array),
        "object" => Ok(AsType::Object),
        other => Err(ClientJsonError::new(format!("unsupported as_type {other}"))),
    }
}

fn field<'a>(value: &'a JsonValue, field: &str) -> ClientJsonResult<&'a JsonValue> {
    value
        .get(field)
        .ok_or_else(|| ClientJsonError::new(format!("missing field {field}")))
}

fn optional_field<'a>(
    value: &'a JsonValue,
    field: &str,
) -> ClientJsonResult<Option<&'a JsonValue>> {
    match value.get(field) {
        Some(JsonValue::Null) => Ok(None),
        Some(value) => Ok(Some(value)),
        None => Ok(None),
    }
}

fn array_field<'a>(value: &'a JsonValue, name: &str) -> ClientJsonResult<&'a Vec<JsonValue>> {
    field(value, name)?
        .as_array()
        .ok_or_else(|| ClientJsonError::new(format!("{name} must be an array")))
}

fn string_field<'a>(value: &'a JsonValue, name: &str) -> ClientJsonResult<&'a str> {
    field(value, name)?
        .as_str()
        .ok_or_else(|| ClientJsonError::new(format!("{name} must be a string")))
}

fn optional_string_field<'a>(
    value: &'a JsonValue,
    name: &str,
) -> ClientJsonResult<Option<&'a str>> {
    optional_field(value, name)?
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| ClientJsonError::new(format!("{name} must be a string")))
        })
        .transpose()
}

fn bool_field(value: &JsonValue, name: &str) -> ClientJsonResult<bool> {
    field(value, name)?
        .as_bool()
        .ok_or_else(|| ClientJsonError::new(format!("{name} must be a bool")))
}

fn i64_field(value: &JsonValue, name: &str) -> ClientJsonResult<i64> {
    field(value, name)?
        .as_i64()
        .ok_or_else(|| ClientJsonError::new(format!("{name} must be an i64")))
}

fn i128_field(value: &JsonValue, name: &str) -> ClientJsonResult<i128> {
    match field(value, name)? {
        JsonValue::String(value) => value
            .parse()
            .map_err(|_| ClientJsonError::new(format!("{name} must be an i128 decimal string"))),
        JsonValue::Number(value) => value
            .as_i64()
            .map(i128::from)
            .or_else(|| value.as_u64().map(i128::from))
            .ok_or_else(|| ClientJsonError::new(format!("{name} must fit in i128"))),
        _ => Err(ClientJsonError::new(format!(
            "{name} must be an integer or decimal string"
        ))),
    }
}

fn id_field(value: &JsonValue, name: &str) -> ClientJsonResult<u32> {
    let raw = field(value, name)?
        .as_u64()
        .ok_or_else(|| ClientJsonError::new(format!("{name} must be an unsigned integer")))?;
    u32::try_from(raw).map_err(|_| ClientJsonError::new(format!("{name} exceeds u32::MAX")))
}

fn parse_integer_domain_field(value: &JsonValue, name: &str) -> ClientJsonResult<IntegerDomain> {
    let domain = string_field(value, name)?;
    IntegerDomain::from_name(domain).ok_or_else(|| {
        ClientJsonError::new(format!(
            "{name} must be one of i32, u32, i64, or u64; found {domain:?}"
        ))
    })
}

fn parse_json_value(text: &str) -> ClientJsonResult<JsonValue> {
    serde_json::from_str(text)
        .map_err(|err| ClientJsonError::new(format!("invalid Client IR v3 JSON: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_descriptor_style_client_block() {
        let value: JsonValue = serde_json::from_str(
            r#"{"kind":"block","stmts":[{"kind":"let","local":0,"name":"next","ty":{"kind":"string"},"value":{"kind":"event_read","value":"target_value"}},{"kind":"signal_set","signal":1,"value":{"kind":"local","local":0}}]}"#,
        )
        .unwrap();

        let block = parse_client_block_json(&value).unwrap();

        assert_eq!(block.stmts.len(), 2);
        assert!(matches!(
            block.stmts[0],
            ClientStmt::Let {
                ty: ClientType::String,
                ..
            }
        ));
    }

    #[test]
    fn range_statements_require_an_exact_integer_domain() {
        let mut value = serde_json::json!({
            "kind": "block",
            "stmts": [{
                "kind": "for_range",
                "local": 0,
                "name": "index",
                "integer_domain": "u64",
                "start": {"kind": "literal", "value": {"kind": "bigint", "value": "0"}},
                "end": {"kind": "literal", "value": {"kind": "bigint", "value": "3"}},
                "inclusive": false,
                "body": {"kind": "block", "stmts": []}
            }]
        });

        let block = parse_client_block_json(&value).expect("an exact range domain is valid");
        assert!(matches!(
            block.stmts.as_slice(),
            [ClientStmt::ForRange {
                integer_domain: IntegerDomain::U64,
                ..
            }]
        ));

        value["stmts"][0]
            .as_object_mut()
            .unwrap()
            .remove("integer_domain");
        let error =
            parse_client_block_json(&value).expect_err("a range without a domain must fail closed");
        assert_eq!(error.to_string(), "missing field integer_domain");

        value["stmts"][0]["integer_domain"] = serde_json::json!("usize");
        let error = parse_client_block_json(&value)
            .expect_err("an unsupported range domain must fail closed");
        assert!(
            error
                .to_string()
                .contains("integer_domain must be one of i32, u32, i64, or u64"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn parses_helper_style_expression_body() {
        let value: JsonValue = serde_json::from_str(
            r#"{"kind":"binary","op":"mul","left":{"kind":"local","local":0,"name":"price"},"right":{"kind":"literal","value":{"v":"Int","data":5}}}"#,
        )
        .unwrap();

        let block = parse_client_body_json(&value).unwrap();

        assert!(matches!(
            block.stmts.as_slice(),
            [ClientStmt::Return(Some(_))]
        ));
    }

    #[test]
    fn parses_helper_style_expression_body_from_str() {
        let block = parse_client_body_json_str(
            r#"{"kind":"binary","op":"mul","left":{"kind":"local","local":0,"name":"price"},"right":{"kind":"literal","value":{"v":"Int","data":5}}}"#,
        )
        .unwrap();

        assert!(matches!(
            block.stmts.as_slice(),
            [ClientStmt::Return(Some(_))]
        ));
    }

    #[test]
    fn round_trips_query_key_expression() {
        let expr = ClientExpr::QueryKey(vec![
            ClientExpr::string("stock"),
            ClientExpr::SignalRead(SignalId(2)),
        ]);

        let json = crate::client::client_expr_json(&expr);
        assert_eq!(
            json,
            r#"{"kind":"query_key","parts":[{"kind":"literal","value":{"kind":"string","value":"stock"}},{"kind":"signal_read","signal":2}]}"#
        );

        let value: JsonValue = serde_json::from_str(&json).unwrap();
        let parsed =
            parse_client_expr_json_with_options(&value, &ClientJsonParseOptions::new()).unwrap();
        assert_eq!(parsed, expr);
    }

    #[test]
    fn resolves_helper_unresolved_calls() {
        let value: JsonValue = serde_json::from_str(
            r#"{"kind":"call","function":{"kind":"unresolved","path":"pricing::subtotal"},"args":[]}"#,
        )
        .unwrap();
        let resolve = |path: &str| {
            (path == "pricing::subtotal").then_some(ClientFunctionRef::Local(ClientFunctionId(3)))
        };
        let options = ClientJsonParseOptions::new().with_unresolved_function_ref(&resolve);

        let expr = parse_client_expr_json_with_options(&value, &options).unwrap();

        assert_eq!(
            expr,
            ClientExpr::Call {
                function: ClientFunctionRef::Local(ClientFunctionId(3)),
                args: Vec::new()
            }
        );
    }
}
