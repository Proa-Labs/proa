// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Client IR v3 data model: the single rsjs expression and statement IR.
//!
//! Template bindings, handler bodies, component-fragment callback
//! adapters, and client-module helpers are all represented, analyzed,
//! and emitted through this model. The legacy v1 expression and handler
//! shapes (and their conversion layer) no longer exist.

mod builder;
mod emit_js;
mod expr;
mod helper_signature;
mod json;
mod json_write;
mod linked_helpers;
mod module;
mod stmt;
mod types;

pub use builder::{ClientFunctionBuilder, ClientModuleBuilder};
pub use emit_js::{
    emit_block as emit_client_block_js,
    emit_block_with_context as emit_client_block_js_with_context,
    emit_block_with_context_and_indent as emit_client_block_js_with_context_and_indent,
    emit_expr as emit_client_expr_js, emit_expr_with_context as emit_client_expr_js_with_context,
    emit_module as emit_client_module_js,
    emit_module_with_context as emit_client_module_js_with_context,
    emit_stmt as emit_client_stmt_js, emit_stmt_with_context as emit_client_stmt_js_with_context,
    ClientEmitContext, ClientEmitError,
};
pub use expr::{
    AsType, BrowserStorageArea, CheckedIntegerOp, ClientBinaryOp, ClientBrowserRead, ClientExpr,
    ClientFunctionId, ClientFunctionRef, ClientLocalId, ClientStructFieldValue, ClientUnaryOp,
    EncodingOp, EventAttributeSource, EventCollection, EventCollectionField, EventValue,
    JsonPointer, LocationField, MutationAccessor, NodeRefAccessor, QueryAccessor, StorageValueType,
    StringPredicateOp, StringTransformOp, WindowField,
};
pub use helper_signature::{
    is_client_helper_runtime_intrinsic, parse_client_helper_type, ClientHelperTypeError,
    ClientHelperTypePosition, CLIENT_HELPER_LEN_INTRINSIC_JS, CLIENT_VALUE_OBJECT_TYPE_PREFIX,
};
pub use json::{
    client_type_from_name, parse_client_block_json, parse_client_block_json_str,
    parse_client_block_json_str_with_options, parse_client_block_json_with_options,
    parse_client_body_json, parse_client_body_json_str, parse_client_body_json_str_with_options,
    parse_client_body_json_with_options, parse_client_expr_json, parse_client_expr_json_str,
    parse_client_expr_json_str_with_options, parse_client_expr_json_with_options,
    parse_client_type_json, ClientJsonError, ClientJsonParseOptions,
};
pub use json_write::{
    client_block_json, client_expr_json, client_module_json, write_client_block_json,
    write_client_expr_json, write_client_module_json, write_client_type_json,
};
pub(crate) use linked_helpers::LinkedHelperCallTarget;
pub use linked_helpers::{
    LinkedHelperCallBinding, LinkedHelperCaller, LinkedHelperDefinition, LinkedHelperEmitError,
    LinkedHelperKey, LinkedHelperKeyError, LinkedHelperLinkError, LinkedHelperProgram,
    LinkedHelperSignaturePosition, UNLINKED_HELPER_IDENTIFIER_PREFIX,
};
pub use module::{
    prune_unreachable_client_functions, ClientCapture, ClientConstant, ClientExport,
    ClientFunction, ClientImport, ClientModule, ClientModuleKind, ClientParam, ClientVisibility,
};
pub use stmt::{
    ClientBlock, ClientBrowserEffect, ClientNodeRefEffect, ClientPlace, ClientSimpleClosure,
    ClientStmt,
};
pub use types::{ClientObjectField, ClientType, IntegerDomain};

#[cfg(test)]
mod tests {
    use crate::client::{
        AsType, BrowserStorageArea, EncodingOp, EventAttributeSource, EventCollection,
        EventCollectionField, EventValue, JsonPointer, MutationAccessor, NodeRefAccessor,
        QueryAccessor, StorageValueType, StringPredicateOp, StringTransformOp,
    };
    use crate::client::{
        CheckedIntegerOp, ClientBinaryOp, ClientBlock, ClientBrowserEffect, ClientBrowserRead,
        ClientConstant, ClientExport, ClientExpr, ClientFunction, ClientFunctionBuilder,
        ClientFunctionId, ClientImport, ClientLocalId, ClientModuleBuilder, ClientModuleKind,
        ClientNodeRefEffect, ClientObjectField, ClientPlace, ClientSimpleClosure, ClientStmt,
        ClientStructFieldValue, ClientType, IntegerDomain,
    };
    use crate::reactive::{ConsoleLevel, StorageArea, UrlHistoryMode};
    use crate::types::{MutationId, NodeRefId, QueryId, SignalId};
    use crate::value::Value;

    #[test]
    fn type_helpers_build_nested_shapes() {
        let ty = ClientType::signal(ClientType::option(ClientType::String));
        assert_eq!(
            ty,
            ClientType::Signal(Box::new(ClientType::Option(Box::new(ClientType::String))))
        );
        assert!(!ty.is_unknown());
        assert!(ClientType::Unknown.is_unknown());
    }

    #[test]
    fn function_builder_allocates_dense_locals_and_preserves_order() {
        let mut builder = ClientFunctionBuilder::new();
        let first = builder.local();
        let second = builder.local();

        builder.push(ClientStmt::let_value(
            first,
            "next",
            ClientType::String,
            ClientExpr::string("hello"),
        ));
        builder.push(ClientStmt::signal_set(
            SignalId(0),
            ClientExpr::local(second),
        ));

        let block = builder.finish();
        assert_eq!(first.0, 0);
        assert_eq!(second.0, 1);
        assert_eq!(block.stmts.len(), 2);
    }

    #[test]
    fn block_can_represent_if_else_handler_logic() {
        let condition = ClientExpr::binary(
            ClientBinaryOp::Gt,
            ClientExpr::signal_read(SignalId(0)),
            ClientExpr::int(5),
        );
        let then_block = ClientBlock::from_stmts(vec![ClientStmt::signal_set(
            SignalId(1),
            ClientExpr::string("large"),
        )]);
        let else_block = ClientBlock::from_stmts(vec![ClientStmt::signal_set(
            SignalId(1),
            ClientExpr::string("small"),
        )]);

        let stmt = ClientStmt::if_else(condition, then_block, else_block);
        assert!(matches!(
            stmt,
            ClientStmt::If {
                else_block: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn block_can_represent_numeric_range_loops() {
        let stmt = ClientStmt::for_range(
            ClientLocalId(0),
            "value",
            IntegerDomain::I32,
            ClientExpr::signal_read(SignalId(0)),
            ClientExpr::int(3),
            false,
            ClientBlock::from_stmts(vec![ClientStmt::signal_set(
                SignalId(0),
                ClientExpr::local(ClientLocalId(0)),
            )]),
        );
        let js = crate::client::emit_client_stmt_js(&stmt).unwrap();

        assert!(matches!(stmt, ClientStmt::ForRange { .. }));
        assert!(js.contains("const __rsjs_range_start_raw_0 = __sig_0.get();"));
        assert!(js.contains(
            "const __rsjs_range_start_0 = typeof __rsjs_range_start_raw_0 === \"bigint\" ? __rsjs_range_start_raw_0 : Math.trunc(Number(__rsjs_range_start_raw_0));"
        ));
        assert!(js.contains(
            "for (let __local_0 = __rsjs_range_start_0; __local_0 < __rsjs_range_end_0; __local_0 += typeof __local_0 === \"bigint\" ? 1n : 1)"
        ));
        assert!(js.contains("__sig_0.set(__local_0);"));
    }

    #[test]
    fn module_builder_records_functions_imports_and_exports() {
        let mut builder = ClientModuleBuilder::new("calendar", ClientModuleKind::ComponentFragment);
        let id = builder.function_id();
        let function = ClientFunction::new(id, "mount")
            .with_param("root", ClientType::Event)
            .with_body(ClientBlock::new())
            .exported();
        builder.push_function(function);

        let mut module = builder.finish();
        module.add_import(ClientImport::new("@rsjs/runtime", "signal"));
        module.add_export(ClientExport::function("mount", ClientFunctionId(0)));

        assert_eq!(module.functions.len(), 1);
        assert_eq!(module.imports[0].module, "@rsjs/runtime");
        assert_eq!(module.exports[0].name, "mount");
    }

    #[test]
    fn v3_module_emitter_lowers_imports_functions_constants_and_exports() {
        let mut builder = ClientModuleBuilder::new("pricing", ClientModuleKind::ClientModule);
        let subtotal = builder.function_id();
        let body = ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::binary(
            ClientBinaryOp::Mul,
            ClientExpr::local(ClientLocalId(0)),
            ClientExpr::local(ClientLocalId(1)),
        )))]);
        builder.push_function(
            ClientFunction::new(subtotal, "subtotal")
                .with_param("price", ClientType::I64)
                .with_param("qty", ClientType::I64)
                .with_return_ty(ClientType::I64)
                .with_body(body),
        );

        let mut module = builder.finish();
        module.add_import(ClientImport::new("@rsjs/runtime", "hydrate").aliased("rsjsHydrate"));
        module.add_constant(ClientConstant::new(
            "VERSION",
            ClientType::String,
            Value::String("1".to_string()),
        ));
        module.add_export(ClientExport::function("subtotal", subtotal));

        let js = crate::client::emit_client_module_js(&module).unwrap();

        assert!(js.contains("import { hydrate as rsjsHydrate } from \"@rsjs/runtime\";"));
        assert!(js.contains("const VERSION = \"1\";"));
        assert!(js.contains("function __fn_0(price, qty) {\n    return (price * qty);\n}"));
        assert!(js.contains("export { __fn_0 as subtotal };"));
    }

    #[test]
    fn v3_module_json_pins_module_graph_shape() {
        let mut builder = ClientModuleBuilder::new("pricing", ClientModuleKind::ClientModule);
        let subtotal = builder.function_id();
        builder.push_function(
            ClientFunction::new(subtotal, "subtotal")
                .with_param("price", ClientType::I64)
                .with_param("qty", ClientType::I64)
                .with_return_ty(ClientType::I64)
                .with_body(ClientBlock::from_stmts(vec![ClientStmt::Return(Some(
                    ClientExpr::binary(
                        ClientBinaryOp::Mul,
                        ClientExpr::local(ClientLocalId(0)),
                        ClientExpr::local(ClientLocalId(1)),
                    ),
                ))])),
        );

        let mut module = builder.finish();
        module.add_export(ClientExport::function("pricing__subtotal", subtotal));

        let json = crate::client::client_module_json(&module);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("module JSON should parse");

        assert_eq!(parsed["format"], "rsjs.client_module_ir");
        assert_eq!(parsed["client_ir_version"], 3);
        assert_eq!(parsed["name"], "pricing");
        assert_eq!(parsed["module_kind"], "client_module");
        assert_eq!(parsed["functions"][0]["name"], "subtotal");
        assert_eq!(parsed["functions"][0]["params"][0]["ty"]["kind"], "i64");
        assert_eq!(parsed["functions"][0]["body"]["stmts"][0]["kind"], "return");
        assert_eq!(parsed["exports"][0]["name"], "pricing__subtotal");
    }

    #[test]
    fn v3_module_prunes_unreachable_private_functions() {
        let mut builder = ClientModuleBuilder::new("pricing", ClientModuleKind::ClientModule);
        let entry = builder.function_id();
        let used = builder.function_id();
        let dead = builder.function_id();
        builder.push_function(ClientFunction::new(entry, "entry").with_body(
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::call(
                crate::client::ClientFunctionRef::Local(used),
                Vec::new(),
            )))]),
        ));
        builder.push_function(ClientFunction::new(used, "used").with_body(
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::string("ok")))]),
        ));
        builder.push_function(ClientFunction::new(dead, "dead").with_body(
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::string("dead")))]),
        ));

        let mut module = builder.finish();
        module.add_export(ClientExport::function("pricing__entry", entry));

        assert_eq!(
            crate::client::prune_unreachable_client_functions(&mut module),
            1
        );
        assert_eq!(
            module
                .functions
                .iter()
                .map(|function| function.id)
                .collect::<Vec<_>>(),
            vec![entry, used]
        );
        assert_eq!(
            crate::client::prune_unreachable_client_functions(&mut module),
            0
        );

        let js = crate::client::emit_client_module_js(&module).unwrap();
        assert!(js.contains("function __fn_0()"));
        assert!(js.contains("function __fn_1()"));
        assert!(!js.contains("function __fn_2()"));
    }

    #[test]
    fn v3_module_emitter_lowers_local_function_calls() {
        let mut builder = ClientModuleBuilder::new("delegating", ClientModuleKind::ClientModule);
        let outer = builder.function_id();
        let inner = builder.function_id();
        builder.push_function(
            ClientFunction::new(outer, "outer")
                .with_body(ClientBlock::from_stmts(vec![ClientStmt::Return(Some(
                    ClientExpr::call(crate::client::ClientFunctionRef::Local(inner), Vec::new()),
                ))]))
                .exported(),
        );
        builder.push_function(ClientFunction::new(inner, "inner").with_body(
            ClientBlock::from_stmts(vec![ClientStmt::Return(Some(ClientExpr::string("ok")))]),
        ));

        let js = crate::client::emit_client_module_js(&builder.finish()).unwrap();

        assert!(js.contains("function __fn_0() {\n    return __fn_1();\n}"));
        assert!(js.contains("function __fn_1() {\n    return \"ok\";\n}"));
        assert!(js.contains("export { __fn_0 as outer };"));
    }

    #[test]
    fn v3_module_emitter_rejects_unknown_export_function_ids() {
        let builder = ClientModuleBuilder::new("broken", ClientModuleKind::ClientModule);
        let mut module = builder.finish();
        module.add_export(ClientExport::function("missing", ClientFunctionId(99)));

        let err = crate::client::emit_client_module_js(&module).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Client IR v3 module exports `missing` from unknown function id 99"
        );
    }

    #[test]
    fn v3_emitter_lowers_basic_expressions() {
        let expr = ClientExpr::binary(
            ClientBinaryOp::And,
            ClientExpr::binary(
                ClientBinaryOp::Gt,
                ClientExpr::signal_read(SignalId(0)),
                ClientExpr::int(5),
            ),
            ClientExpr::EventRead(EventValue::TargetChecked),
        );

        let js = crate::client::emit_client_expr_js(&expr).unwrap();
        assert_eq!(js, "((__sig_0.get() > 5) && !!event.target?.checked)");
    }

    #[test]
    fn checked_wide_payload_operands_decode_canonical_decimal_wire_values() {
        let checked = |payload| {
            ClientExpr::binary(
                ClientBinaryOp::Checked {
                    op: CheckedIntegerOp::Add,
                    domain: IntegerDomain::I64,
                },
                payload,
                ClientExpr::Literal(Value::BigInt(0)),
            )
        };
        let event_js = crate::client::emit_client_expr_js(&checked(ClientExpr::EventPayload {
            attr: "data-wide".to_string(),
        }))
        .unwrap();
        assert!(event_js.starts_with("checked_int_add("), "{event_js}");
        assert!(event_js.contains("JSON.parse"), "{event_js}");
        assert!(event_js.contains("/^(?:0|-?[1-9][0-9]*)$/"), "{event_js}");
        assert!(event_js.contains("BigInt(__rsjs_wire)"), "{event_js}");

        let ctx = crate::client::ClientEmitContext {
            node_name: Some("__node".to_string()),
            ..Default::default()
        };
        let mut node_js = String::new();
        crate::client::emit_client_expr_js_with_context(
            &mut node_js,
            &checked(ClientExpr::NodePayload {
                attr: "data-wide".to_string(),
                tokens: "day as i64".to_string(),
            }),
            &ctx,
        )
        .unwrap();
        assert!(
            node_js.contains("__node?.getAttribute('data-wide')"),
            "{node_js}"
        );
        assert!(node_js.contains("BigInt(__rsjs_wire)"), "{node_js}");
    }

    #[test]
    fn v3_emitter_can_name_locals_for_helper_params() {
        let expr = ClientExpr::binary(
            ClientBinaryOp::Mul,
            ClientExpr::local(ClientLocalId(0)),
            ClientExpr::local(ClientLocalId(1)),
        );
        let ctx = crate::client::ClientEmitContext::default()
            .with_local_name(ClientLocalId(0), "price")
            .with_local_name(ClientLocalId(1), "qty");

        let mut js = String::new();
        crate::client::emit_client_expr_js_with_context(&mut js, &expr, &ctx).unwrap();

        assert_eq!(js, "(price * qty)");
    }

    #[test]
    fn v3_emitter_lowers_signal_set_blocks() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::expr(ClientExpr::EventRead(EventValue::Key)),
            ClientStmt::signal_set(SignalId(1), ClientExpr::string("saved")),
        ]);

        let js = crate::client::emit_client_block_js(&block).unwrap();
        assert_eq!(js, "(event.key ?? \"\");\n__sig_1.set(\"saved\");\n");
    }

    #[test]
    fn v3_emitter_lowers_query_and_mutation_reads() {
        let query = ClientExpr::query_read(QueryId(2), QueryAccessor::DownloadPercent);
        assert_eq!(
            crate::client::emit_client_expr_js(&query).unwrap(),
            "__rsjs_query_2.download_percent()"
        );

        let mutation = ClientExpr::mutation_read(MutationId(3), MutationAccessor::IsPending);
        assert_eq!(
            crate::client::emit_client_expr_js(&mutation).unwrap(),
            "__rsjs_mutation_3.is_pending()"
        );
    }

    #[test]
    fn v3_emitter_lowers_client_helper_calls() {
        let expr = ClientExpr::call(
            crate::client::ClientFunctionRef::Import {
                module: "crate::calendar_client".to_string(),
                name: "format_date".to_string(),
            },
            vec![ClientExpr::EventRead(EventValue::TargetDate)],
        );

        assert_eq!(
            crate::client::emit_client_expr_js(&expr).unwrap(),
            "__rsjs_client_module_crate__calendar_client.calendar_client__format_date((event.target?.closest?.('[data-date]')?.getAttribute('data-date') ?? \"\"))"
        );
    }

    #[test]
    fn v3_emitter_lowers_browser_reads_and_encoding() {
        let read = ClientExpr::browser_read(ClientBrowserRead::Storage {
            area: BrowserStorageArea::Local,
            value_type: StorageValueType::String,
            key: Box::new(ClientExpr::string("filters")),
            fallback: Some(Box::new(ClientExpr::string("empty"))),
        });
        let read_js = crate::client::emit_client_expr_js(&read).unwrap();
        assert!(read_js.contains("window.localStorage?.getItem?.(String(\"filters\"))"));
        assert!(read_js.contains("return __v == null ? String(\"empty\") : __v;"));

        let i64_read = ClientExpr::browser_read(ClientBrowserRead::Storage {
            area: BrowserStorageArea::Local,
            value_type: StorageValueType::I64,
            key: Box::new(ClientExpr::string("count")),
            fallback: Some(Box::new(ClientExpr::Literal(Value::BigInt(7)))),
        });
        let i64_js = crate::client::emit_client_expr_js(&i64_read).unwrap();
        assert!(
            i64_js.contains("/^(?:0|-?[1-9][0-9]*)$/.test(__v)")
                && i64_js.contains("const __parsed = BigInt(__v)")
                && i64_js.contains("__parsed >= -9223372036854775808n")
                && i64_js.contains("__parsed <= 9223372036854775807n")
                && i64_js.contains("return 7n"),
            "i64 storage reads must decode canonical decimal text into an in-range BigInt: {i64_js}"
        );

        let encoded = ClientExpr::encoding(
            EncodingOp::Base64UrlEncode,
            ClientExpr::signal_read(SignalId(0)),
        );
        let encoded_js = crate::client::emit_client_expr_js(&encoded).unwrap();
        assert!(encoded_js.contains("new Uint8Array(__v ?? [])"));
        assert!(encoded_js.contains("replace(/\\+/g, \"-\")"));
    }

    #[test]
    fn v3_emitter_lowers_expression_helpers() {
        let expr = ClientExpr::Conditional {
            condition: Box::new(ClientExpr::IsNonEmpty {
                target: Box::new(ClientExpr::signal_read(SignalId(0))),
            }),
            then_expr: Box::new(ClientExpr::StringToLowercase {
                target: Box::new(ClientExpr::string("ABC")),
            }),
            else_expr: Box::new(ClientExpr::StringContains {
                haystack: Box::new(ClientExpr::string("calendar")),
                needle: Box::new(ClientExpr::string("day")),
            }),
        };

        let js = crate::client::emit_client_expr_js(&expr).unwrap();
        assert!(js.contains("Object.keys(__v).length > 0"));
        assert!(js.contains("toLowerCase()"));
        assert!(js.contains(".includes(String("));
    }

    #[test]
    fn v3_emitter_lowers_string_transforms_and_predicates() {
        let upper = ClientExpr::StringTransform {
            op: StringTransformOp::ToUppercase,
            target: Box::new(ClientExpr::string("abc")),
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&upper).unwrap(),
            "(String((\"abc\") ?? \"\").toUpperCase())"
        );

        let trim = ClientExpr::StringTransform {
            op: StringTransformOp::Trim,
            target: Box::new(ClientExpr::signal_read(SignalId(0))),
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&trim).unwrap(),
            "(String((__sig_0.get()) ?? \"\").replace(/^[\\u0009-\\u000D\\u0020\\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]+|[\\u0009-\\u000D\\u0020\\u0085\\u00A0\\u1680\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000]+$/g, \"\"))"
        );

        let starts = ClientExpr::StringPredicate {
            op: StringPredicateOp::StartsWith,
            haystack: Box::new(ClientExpr::signal_read(SignalId(0))),
            needle: Box::new(ClientExpr::string("ab")),
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&starts).unwrap(),
            "(String((__sig_0.get()) ?? \"\").startsWith(String((\"ab\") ?? \"\")))"
        );

        let ends = ClientExpr::StringPredicate {
            op: StringPredicateOp::EndsWith,
            haystack: Box::new(ClientExpr::signal_read(SignalId(0))),
            needle: Box::new(ClientExpr::string("yz")),
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&ends).unwrap(),
            "(String((__sig_0.get()) ?? \"\").endsWith(String((\"yz\") ?? \"\")))"
        );
    }

    #[test]
    fn v3_emitter_lowers_projection_helpers() {
        let expr = ClientExpr::AsType {
            target: Box::new(ClientExpr::Project {
                target: Box::new(ClientExpr::query_read(QueryId(0), QueryAccessor::Data)),
                path: JsonPointer::new("/items/0/name"),
            }),
            ty: AsType::Str,
        };

        assert_eq!(
            crate::client::emit_client_expr_js(&expr).unwrap(),
            "rsjs.__as_str(rsjs.__project(__rsjs_query_0.data(), [\"items\", \"0\", \"name\"]))"
        );
    }

    #[test]
    fn v3_emitter_lowers_exact_integer_to_f64_conversion_as_number() {
        let expr = ClientExpr::AsType {
            target: Box::new(ClientExpr::signal_read(SignalId(0))),
            ty: AsType::F64FromInt,
        };

        assert_eq!(
            crate::client::emit_client_expr_js(&expr).unwrap(),
            "Number(__sig_0.get())"
        );
        let json = crate::client::client_expr_json(&expr);
        assert!(json.contains("\"ty\":\"f64_from_int\""), "{json}");
        assert_eq!(
            crate::client::parse_client_expr_json_str(&json).unwrap(),
            expr
        );
    }

    #[test]
    fn v3_emitter_lowers_dom_payload_and_collection_reads() {
        let collection = ClientExpr::EventCollectionRead {
            collection: EventCollection::DataTransferFiles,
            index: Box::new(ClientExpr::int(0)),
            field: EventCollectionField::FileName,
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&collection).unwrap(),
            "(event.dataTransfer?.files?.[Math.trunc(Number(0) || 0)]?.name ?? \"\")"
        );

        let node_ref = ClientExpr::NodeRefRead {
            id: NodeRefId(1),
            accessor: NodeRefAccessor::Value,
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&node_ref).unwrap(),
            "(__ref_1.current?.value ?? \"\")"
        );

        let current_target = ClientExpr::DomRead {
            target: Box::new(ClientExpr::EventCurrentTarget),
            accessor: NodeRefAccessor::Attribute {
                name: "data-id".to_string(),
            },
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&current_target).unwrap(),
            "((event.__rsjsCurrentTarget)?.getAttribute?.(\"data-id\") ?? \"\")"
        );
        let json = crate::client::client_expr_json(&current_target);
        assert_eq!(
            crate::client::parse_client_expr_json_str(&json).unwrap(),
            current_target
        );

        let attr = ClientExpr::EventAttribute {
            source: EventAttributeSource::Closest,
            attr: "data-id".to_string(),
        };
        assert_eq!(
            crate::client::emit_client_expr_js(&attr).unwrap(),
            "(event.target?.closest?.(\"[data-id]\")?.getAttribute(\"data-id\") ?? \"\")"
        );

        let payload = ClientExpr::EventPayload {
            attr: "data-rsjs-payload".to_string(),
        };
        let mut js = String::new();
        crate::client::emit_client_expr_js_with_context(
            &mut js,
            &payload,
            &crate::client::ClientEmitContext {
                event_name: "evt".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            js,
            "JSON.parse(((evt.__rsjsOwnerTarget?.getAttribute?.('data-rsjs-payload')) ?? (evt.__rsjsCurrentTarget?.getAttribute?.('data-rsjs-payload')) ?? (evt.target?.closest?.('[data-rsjs-payload]')?.getAttribute('data-rsjs-payload'))) ?? \"null\")"
        );
    }

    #[test]
    fn v3_emitter_lowers_common_handler_statements() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::EventPreventDefault,
            ClientStmt::Console {
                level: ConsoleLevel::Log,
                args: vec![ClientExpr::EventRead(EventValue::Key)],
            },
            ClientStmt::signal_update(
                SignalId(0),
                ClientSimpleClosure::new("current", ClientExpr::signal_read(SignalId(0))),
            ),
            ClientStmt::QueryRefetch { query: QueryId(0) },
            ClientStmt::QueryInvalidate { query: QueryId(1) },
        ]);

        let js = crate::client::emit_client_block_js(&block).unwrap();
        assert_eq!(
            js,
            "event.preventDefault();\nconsole.log((event.key ?? \"\"));\n__sig_0.update((__c0) => __sig_0.get());\n__rsjs_query_0.refetch();\n__rsjs_query_1.invalidate();\n"
        );
    }

    #[test]
    fn v3_emitter_lowers_browser_and_node_ref_effects() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::BrowserEffect(ClientBrowserEffect::WindowScrollTo {
                x: ClientExpr::int(0),
                y: ClientExpr::int(100),
            }),
            ClientStmt::BrowserEffect(ClientBrowserEffect::StorageSetItem {
                area: StorageArea::Local,
                key: ClientExpr::string("filters"),
                value: ClientExpr::signal_read(SignalId(0)),
            }),
            ClientStmt::BrowserEffect(ClientBrowserEffect::UrlDeleteSearchParam {
                mode: UrlHistoryMode::Push,
                key: ClientExpr::string("modal"),
            }),
            ClientStmt::NodeRefEffect {
                id: NodeRefId(2),
                effect: ClientNodeRefEffect::SetValue(ClientExpr::string("next")),
            },
        ]);

        let js = crate::client::emit_client_block_js(&block).unwrap();
        assert!(js.contains("window.scrollTo?.(Number(0), Number(100));"));
        assert!(js.contains(
            "window.localStorage?.setItem?.(String(\"filters\"), String(__sig_0.get()))"
        ));
        assert!(js.contains("window.history?.pushState?.(null, \"\", __url.href);"));
        assert!(js.contains("if (__ref_2.current != null) __ref_2.current.value = \"next\";"));
    }

    #[test]
    fn v3_emitter_lowers_local_control_statements() {
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                crate::client::ClientLocalId(0),
                "next",
                ClientType::String,
                ClientExpr::string("a"),
            ),
            ClientStmt::Assign {
                local: crate::client::ClientLocalId(0),
                value: ClientExpr::string("b"),
            },
            ClientStmt::Block(ClientBlock::from_stmts(vec![ClientStmt::Return(Some(
                ClientExpr::Local(crate::client::ClientLocalId(0)),
            ))])),
        ]);
        assert_eq!(
            crate::client::emit_client_block_js(&block).unwrap(),
            "let __local_0 = \"a\";\n__local_0 = \"b\";\n{\n    return __local_0;\n}\n"
        );
    }

    #[test]
    fn v3_emitter_and_json_lower_nominal_struct_values_as_plain_objects() {
        let object_ty = ClientType::object(vec![
            ClientObjectField {
                name: "open".to_owned(),
                ty: ClientType::Bool,
            },
            ClientObjectField {
                name: "label".to_owned(),
                ty: ClientType::String,
            },
        ]);
        let block = ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(0),
                "menu",
                object_ty,
                ClientExpr::StructInit {
                    type_name: "MenuController".to_owned(),
                    fields: vec![
                        ClientStructFieldValue {
                            name: "open".to_owned(),
                            value: ClientExpr::bool(false),
                        },
                        ClientStructFieldValue {
                            name: "label".to_owned(),
                            value: ClientExpr::string("Menu"),
                        },
                    ],
                },
            ),
            ClientStmt::AssignPlace {
                place: ClientPlace::field(ClientPlace::local(ClientLocalId(0)), "open"),
                value: ClientExpr::bool(true),
            },
            ClientStmt::Return(Some(ClientExpr::Field {
                base: Box::new(ClientExpr::Local(ClientLocalId(0))),
                name: "open".to_owned(),
            })),
        ]);

        assert_eq!(
            crate::client::emit_client_block_js(&block).unwrap(),
            "let __local_0 = ({\"open\": false, \"label\": \"Menu\"});\n\
             __local_0[\"open\"] = true;\n\
             return (__local_0)[\"open\"];\n"
        );
        let json = crate::client::client_block_json(&block);
        assert_eq!(
            crate::client::parse_client_block_json_str(&json).unwrap(),
            block
        );
    }

    #[test]
    fn v3_emitter_lowers_extended_event_values() {
        let clipboard =
            crate::client::emit_client_expr_js(&ClientExpr::EventRead(EventValue::ClipboardText))
                .unwrap();
        assert_eq!(
            clipboard,
            "(event.clipboardData?.getData?.(\"text/plain\") ?? \"\")"
        );

        let storage =
            crate::client::emit_client_expr_js(&ClientExpr::EventRead(EventValue::StorageArea))
                .unwrap();
        assert!(storage.contains("event.storageArea === window.localStorage"));

        let touch = crate::client::emit_client_expr_js(&ClientExpr::EventRead(
            EventValue::ChangedTouchClientX,
        ))
        .unwrap();
        assert_eq!(touch, "(event.changedTouches?.[0]?.clientX ?? 0)");

        let detail_u32 =
            crate::client::emit_client_expr_js(&ClientExpr::EventRead(EventValue::DetailU32))
                .unwrap();
        assert_eq!(detail_u32, "((event.detail ?? 0) >>> 0)");
        let encoded =
            crate::client::client_expr_json(&ClientExpr::EventRead(EventValue::DetailU32));
        let encoded_value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(encoded_value["value"], "detail_u32");
        assert_eq!(
            crate::client::parse_client_expr_json(&encoded_value).unwrap(),
            ClientExpr::EventRead(EventValue::DetailU32)
        );
    }
}
