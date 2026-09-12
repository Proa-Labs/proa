// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! HIR sidecar construction and JSON serialisation for conformance tooling.
use super::*;

impl HirSidecar {
    pub(super) fn push_handler(&mut self, id: EventHandlerId, body: Vec<HandlerHirStmt>) {
        self.handlers.push(HandlerHirArtifact { id, body });
    }

    pub(super) fn push_interval(&mut self, ms: u64, body: Vec<HandlerHirStmt>) {
        self.intervals.push(IntervalHirArtifact { ms, body });
    }

    pub(super) fn push_list(&mut self, container: NodeId, source: ListHirSource, key: String) {
        self.lists.push(ListHirArtifact {
            container,
            source,
            key,
        });
    }

    pub(super) fn push_component_fragment_mount(
        &mut self,
        root: NodeId,
        asset_id: String,
        callbacks: Vec<ComponentFragmentCallbackHirArtifact>,
    ) {
        self.component_fragment_mounts
            .push(ComponentFragmentMountHirArtifact {
                root,
                asset_id,
                callbacks,
            });
    }
}

pub(crate) fn hir_sidecar_json(asset_id: &str, sidecar: &HirSidecar) -> String {
    let mut value = json!({
        "format": "rsjs.hir",
        "hir_version": 1,
        "asset_id": asset_id,
        "handlers": sidecar.handlers.iter().map(|handler| {
            json!({
                "id": handler.id.0,
                "body_hir": handler_hir_json(&handler.body),
            })
        }).collect::<Vec<_>>(),
        "intervals": sidecar.intervals.iter().map(|interval| {
            json!({
                "ms": interval.ms,
                "body_hir": handler_hir_json(&interval.body),
            })
        }).collect::<Vec<_>>(),
        "component_fragment_mounts": sidecar.component_fragment_mounts.iter().map(|mount| {
            json!({
                "root": mount.root.0,
                "asset_id": mount.asset_id,
                "callbacks": mount.callbacks.iter().map(|callback| {
                    json!({
                        "slot": callback.slot.0,
                        "body_hir": handler_hir_json(&callback.body),
                    })
                }).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    });
    if !sidecar.lists.is_empty() {
        value["lists"] = JsonValue::Array(
            sidecar
                .lists
                .iter()
                .map(|list| {
                    let mut entry = json!({
                        "container": list.container.0,
                        "key": list.key,
                    });
                    match list.source {
                        ListHirSource::Signal(signal) => {
                            entry["signal"] = json!(signal.0);
                        }
                        ListHirSource::Expr(expr) => {
                            entry["expr"] = json!(expr.0);
                        }
                    }
                    entry
                })
                .collect(),
        );
    }
    serde_json::to_string(&value).expect("rsjs HIR sidecar JSON should serialize")
}

pub(super) fn handler_hir_json(statements: &[HandlerHirStmt]) -> JsonValue {
    JsonValue::Array(statements.iter().map(handler_hir_stmt_json).collect())
}

pub(super) fn handler_hir_stmt_json(statement: &HandlerHirStmt) -> JsonValue {
    match &statement.kind {
        HandlerHirStmtKind::SignalSet { id, value } => json!({
            "span": span_json(statement.span),
            "kind": "signal_set",
            "signal": id.0,
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirStmtKind::SignalUpdate { id, closure } => json!({
            "span": span_json(statement.span),
            "kind": "signal_update",
            "signal": id.0,
            "closure": {
                "param": closure.param,
                "body": handler_hir_expr_json(&closure.body),
            },
        }),
        HandlerHirStmtKind::Expr(expr) => json!({
            "span": span_json(statement.span),
            "kind": "expr",
            "expr": handler_hir_expr_json(expr),
        }),
        // The binding `ty` is Client IR metadata, not HIR surface;
        // it is serialized on the lowered `ClientStmt::Let` instead.
        HandlerHirStmtKind::Let {
            local,
            name,
            value,
            ty: _,
        } => json!({
            "span": span_json(statement.span),
            "kind": "let",
            "local": local.0,
            "name": name,
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirStmtKind::Assign { local, value } => json!({
            "span": span_json(statement.span),
            "kind": "assign",
            "local": local.0,
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirStmtKind::EventPreventDefault => json!({
            "span": span_json(statement.span),
            "kind": "event_prevent_default",
        }),
        HandlerHirStmtKind::EventStopPropagation => json!({
            "span": span_json(statement.span),
            "kind": "event_stop_propagation",
        }),
        HandlerHirStmtKind::Console { level, args } => json!({
            "span": span_json(statement.span),
            "kind": "console",
            "level": format!("{level:?}"),
            "args": args.iter().map(handler_hir_expr_json).collect::<Vec<_>>(),
        }),
        HandlerHirStmtKind::BrowserEffect(effect) => json!({
            "span": span_json(statement.span),
            "kind": "browser_effect",
            "effect": browser_effect_hir_json(effect),
        }),
        HandlerHirStmtKind::CallbackInvoke { slot, payload } => json!({
            "span": span_json(statement.span),
            "kind": "callback_invoke",
            "slot": slot.0,
            "payload": handler_hir_expr_json(payload),
        }),
        HandlerHirStmtKind::If {
            condition,
            then_statements,
            else_statements,
        } => json!({
            "span": span_json(statement.span),
            "kind": "if",
            "condition": handler_hir_expr_json(condition),
            "then": handler_hir_json(then_statements),
            "else": handler_hir_json(else_statements),
        }),
        HandlerHirStmtKind::ForRange {
            local,
            name,
            integer_domain,
            start,
            end,
            inclusive,
            body,
        } => json!({
            "span": span_json(statement.span),
            "kind": "for_range",
            "local": local.0,
            "name": name,
            "integer_domain": integer_domain.as_str(),
            "start": handler_hir_expr_json(start),
            "end": handler_hir_expr_json(end),
            "inclusive": inclusive,
            "body": handler_hir_json(body),
        }),
        HandlerHirStmtKind::RunMutation { id, args } => json!({
            "span": span_json(statement.span),
            "kind": "run_mutation",
            "mutation": id.0,
            "args": args.iter().map(handler_hir_expr_json).collect::<Vec<_>>(),
        }),
        HandlerHirStmtKind::QueryRefetch { id } => json!({
            "span": span_json(statement.span),
            "kind": "query_refetch",
            "query": id.0,
        }),
        HandlerHirStmtKind::QueryInvalidate { id } => json!({
            "span": span_json(statement.span),
            "kind": "query_invalidate",
            "query": id.0,
        }),
        HandlerHirStmtKind::NodeRefEffect { id, effect } => json!({
            "span": span_json(statement.span),
            "kind": "node_ref_effect",
            "ref": id.0,
            "effect": node_ref_effect_hir_json(effect),
        }),
    }
}

pub(super) fn span_json(span: proc_macro2::Span) -> JsonValue {
    let start = span.start();
    let end = span.end();
    json!({
        "start": {
            "line": start.line,
            "column": start.column,
        },
        "end": {
            "line": end.line,
            "column": end.column,
        },
    })
}

pub(super) fn handler_hir_expr_json(expr: &HandlerHirExpr) -> JsonValue {
    hir_expr_json_at(&expr.expr, expr.span)
}

pub(super) fn hir_expr_json_at(expr: &FrontendExpr, span: proc_macro2::Span) -> JsonValue {
    hir_expr_json_inner(expr, Some(span))
}

pub(super) fn hir_expr_json_inner(
    expr: &FrontendExpr,
    span: Option<proc_macro2::Span>,
) -> JsonValue {
    let value = match expr {
        FrontendExpr::Literal(value) => json!({
            "kind": "literal",
            "value": hir_value_json(value),
        }),
        FrontendExpr::SignalRead(id) => json!({
            "kind": "signal_read",
            "signal": id.0,
        }),
        FrontendExpr::Local(id) => json!({
            "kind": "local",
            "local": id.0,
        }),
        FrontendExpr::ArrayLit(elems) => json!({
            "kind": "array_lit",
            "elems": elems.iter().map(hir_value_json).collect::<Vec<_>>(),
        }),
        FrontendExpr::EventValue(value) => json!({
            "kind": "event_value",
            "value": format!("{value:?}"),
        }),
        FrontendExpr::EventCurrentTarget => json!({
            "kind": "event_current_target",
        }),
        FrontendExpr::EventAttribute { source, attr } => json!({
            "kind": "event_attribute",
            "source": format!("{source:?}"),
            "attr": attr,
        }),
        FrontendExpr::EventPayload { attr } => json!({
            "kind": "event_payload",
            "attr": attr,
        }),
        FrontendExpr::NodeRefRead { id, accessor } => json!({
            "kind": "node_ref_read",
            "ref": id.0,
            "accessor": format!("{accessor:?}"),
        }),
        FrontendExpr::DomRead { target, accessor } => json!({
            "kind": "dom_read",
            "target": hir_expr_json_inner(target, span),
            "accessor": format!("{accessor:?}"),
        }),
        FrontendExpr::BrowserRead(read) => json!({
            "kind": "browser_read",
            "read": browser_read_hir_json(read, span),
        }),
        FrontendExpr::Encoding { op, value } => json!({
            "kind": "encoding",
            "op": format!("{op:?}"),
            "value": hir_expr_json_inner(value, span),
        }),
        FrontendExpr::BinaryOp { op, lhs, rhs } => json!({
            "kind": "binary",
            "op": format!("{op:?}"),
            "lhs": hir_expr_json_inner(lhs, span),
            "rhs": hir_expr_json_inner(rhs, span),
        }),
        other => json!({
            "kind": "debug",
            "value": format!("{other:?}"),
        }),
    };
    add_hir_expr_span(value, span)
}

pub(super) fn add_hir_expr_span(
    mut value: JsonValue,
    span: Option<proc_macro2::Span>,
) -> JsonValue {
    if let (Some(span), JsonValue::Object(map)) = (span, &mut value) {
        map.insert("span".to_string(), span_json(span));
    }
    value
}

pub(super) fn hir_value_json(value: &Value) -> JsonValue {
    match value {
        Value::Bool(value) => json!({ "kind": "bool", "value": value }),
        Value::Int(value) => json!({ "kind": "int", "value": value }),
        Value::BigInt(value) => json!({ "kind": "bigint", "value": value }),
        Value::Float(value) => json!({ "kind": "float", "value": value }),
        Value::String(value) => json!({ "kind": "string", "value": value }),
        Value::Null => json!({ "kind": "null" }),
        other => json!({
            "kind": "debug",
            "value": format!("{other:?}"),
        }),
    }
}

pub(super) fn browser_effect_hir_json(effect: &HandlerHirBrowserEffect) -> JsonValue {
    match effect {
        HandlerHirBrowserEffect::WindowScrollTo { x, y } => json!({
            "kind": "window_scroll_to",
            "x": handler_hir_expr_json(x),
            "y": handler_hir_expr_json(y),
        }),
        HandlerHirBrowserEffect::WindowScrollBy { x, y } => json!({
            "kind": "window_scroll_by",
            "x": handler_hir_expr_json(x),
            "y": handler_hir_expr_json(y),
        }),
        HandlerHirBrowserEffect::WindowPrint => json!({
            "kind": "window_print",
        }),
        HandlerHirBrowserEffect::LocationSetHash { value } => json!({
            "kind": "location_set_hash",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirBrowserEffect::LocationAssign { url } => json!({
            "kind": "location_assign",
            "url": handler_hir_expr_json(url),
        }),
        HandlerHirBrowserEffect::LocationReplace { url } => json!({
            "kind": "location_replace",
            "url": handler_hir_expr_json(url),
        }),
        HandlerHirBrowserEffect::LocationReload => json!({
            "kind": "location_reload",
        }),
        HandlerHirBrowserEffect::HistoryPushState { url } => json!({
            "kind": "history_push_state",
            "url": handler_hir_expr_json(url),
        }),
        HandlerHirBrowserEffect::HistoryReplaceState { url } => json!({
            "kind": "history_replace_state",
            "url": handler_hir_expr_json(url),
        }),
        HandlerHirBrowserEffect::HistoryBack => json!({
            "kind": "history_back",
        }),
        HandlerHirBrowserEffect::HistoryForward => json!({
            "kind": "history_forward",
        }),
        HandlerHirBrowserEffect::HistoryGo { delta } => json!({
            "kind": "history_go",
            "delta": handler_hir_expr_json(delta),
        }),
        HandlerHirBrowserEffect::DocumentDispatchEvent { name, detail } => json!({
            "kind": "document_dispatch_event",
            "name": handler_hir_expr_json(name),
            "detail": detail.as_ref().map(handler_hir_expr_json),
        }),
        HandlerHirBrowserEffect::DocumentSetTitle { title } => json!({
            "kind": "document_set_title",
            "title": handler_hir_expr_json(title),
        }),
        HandlerHirBrowserEffect::ClipboardWriteText { text } => json!({
            "kind": "clipboard_write_text",
            "text": handler_hir_expr_json(text),
        }),
        HandlerHirBrowserEffect::ClipboardReadTextInto { target, fallback } => json!({
            "kind": "clipboard_read_text_into",
            "target": target.0,
            "fallback": fallback.as_ref().map(handler_hir_expr_json),
        }),
        HandlerHirBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            target,
            fallback,
        } => json!({
            "kind": "webauthn_create_json_into",
            "options_json": handler_hir_expr_json(options_json),
            "target": target.0,
            "fallback": fallback.as_ref().map(handler_hir_expr_json),
        }),
        HandlerHirBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            target,
            fallback,
        } => json!({
            "kind": "webauthn_get_json_into",
            "options_json": handler_hir_expr_json(options_json),
            "target": target.0,
            "fallback": fallback.as_ref().map(handler_hir_expr_json),
        }),
        HandlerHirBrowserEffect::EventFileTextInto {
            source,
            target,
            fallback,
        } => json!({
            "kind": "event_file_text_into",
            "source": format!("{source:?}"),
            "target": target.0,
            "fallback": fallback.as_ref().map(handler_hir_expr_json),
        }),
        HandlerHirBrowserEffect::StorageSetItem { area, key, value } => json!({
            "kind": "storage_set_item",
            "area": format!("{area:?}"),
            "key": handler_hir_expr_json(key),
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirBrowserEffect::StorageRemoveItem { area, key } => json!({
            "kind": "storage_remove_item",
            "area": format!("{area:?}"),
            "key": handler_hir_expr_json(key),
        }),
        HandlerHirBrowserEffect::UrlSetSearchParam { mode, key, value } => json!({
            "kind": "url_set_search_param",
            "mode": format!("{mode:?}"),
            "key": handler_hir_expr_json(key),
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirBrowserEffect::UrlDeleteSearchParam { mode, key } => json!({
            "kind": "url_delete_search_param",
            "mode": format!("{mode:?}"),
            "key": handler_hir_expr_json(key),
        }),
        HandlerHirBrowserEffect::Declared { helper, args } => json!({
            "kind": "declared",
            "helper": helper,
            "args": args
                .iter()
                .map(handler_hir_expr_json)
                .collect::<Vec<_>>(),
        }),
    }
}

pub(super) fn node_ref_effect_hir_json(effect: &HandlerHirNodeRefEffect) -> JsonValue {
    match effect {
        HandlerHirNodeRefEffect::Focus => json!({ "kind": "focus" }),
        HandlerHirNodeRefEffect::Blur => json!({ "kind": "blur" }),
        HandlerHirNodeRefEffect::ScrollIntoView => json!({ "kind": "scroll_into_view" }),
        HandlerHirNodeRefEffect::Click => json!({ "kind": "click" }),
        HandlerHirNodeRefEffect::RequestSubmit => json!({ "kind": "request_submit" }),
        HandlerHirNodeRefEffect::Reset => json!({ "kind": "reset" }),
        HandlerHirNodeRefEffect::ReportValidity => json!({ "kind": "report_validity" }),
        HandlerHirNodeRefEffect::ShowModal => json!({ "kind": "show_modal" }),
        HandlerHirNodeRefEffect::Close => json!({ "kind": "close" }),
        HandlerHirNodeRefEffect::ShowPopover => json!({ "kind": "show_popover" }),
        HandlerHirNodeRefEffect::HidePopover => json!({ "kind": "hide_popover" }),
        HandlerHirNodeRefEffect::TogglePopover => json!({ "kind": "toggle_popover" }),
        HandlerHirNodeRefEffect::ScrollTo { x, y } => json!({
            "kind": "scroll_to",
            "x": handler_hir_expr_json(x),
            "y": handler_hir_expr_json(y),
        }),
        HandlerHirNodeRefEffect::ScrollBy { x, y } => json!({
            "kind": "scroll_by",
            "x": handler_hir_expr_json(x),
            "y": handler_hir_expr_json(y),
        }),
        HandlerHirNodeRefEffect::SetPointerCapture(pointer_id) => json!({
            "kind": "set_pointer_capture",
            "pointer_id": handler_hir_expr_json(pointer_id),
        }),
        HandlerHirNodeRefEffect::ReleasePointerCapture(pointer_id) => json!({
            "kind": "release_pointer_capture",
            "pointer_id": handler_hir_expr_json(pointer_id),
        }),
        HandlerHirNodeRefEffect::Select => json!({ "kind": "select" }),
        HandlerHirNodeRefEffect::SetValue(value) => json!({
            "kind": "set_value",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::SetChecked(value) => json!({
            "kind": "set_checked",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::SetCustomValidity(value) => json!({
            "kind": "set_custom_validity",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::SetSelectionRange(start, end) => json!({
            "kind": "set_selection_range",
            "start": handler_hir_expr_json(start),
            "end": handler_hir_expr_json(end),
        }),
        HandlerHirNodeRefEffect::SetTextContent(value) => json!({
            "kind": "set_text_content",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::AddClass(value) => json!({
            "kind": "add_class",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::RemoveClass(value) => json!({
            "kind": "remove_class",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::ToggleClass(value) => json!({
            "kind": "toggle_class",
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::SetAttribute { name, value } => json!({
            "kind": "set_attribute",
            "name": name,
            "value": handler_hir_expr_json(value),
        }),
        HandlerHirNodeRefEffect::RemoveAttribute { name } => json!({
            "kind": "remove_attribute",
            "name": name,
        }),
        HandlerHirNodeRefEffect::SetStyleProperty { name, value } => json!({
            "kind": "set_style_property",
            "name": name,
            "value": handler_hir_expr_json(value),
        }),
    }
}

pub(super) fn browser_read_hir_json(
    read: &BrowserRead,
    span: Option<proc_macro2::Span>,
) -> JsonValue {
    match read {
        BrowserRead::Window(field) => json!({
            "kind": "window",
            "field": format!("{field:?}"),
        }),
        BrowserRead::Location(field) => json!({
            "kind": "location",
            "field": format!("{field:?}"),
        }),
        BrowserRead::Storage {
            area,
            value_type,
            key,
            fallback,
        } => json!({
            "kind": "storage",
            "area": format!("{area:?}"),
            "value_type": format!("{value_type:?}"),
            "key": hir_expr_json_inner(key, span),
            "fallback": fallback.as_ref().map(|expr| hir_expr_json_inner(expr.as_ref(), span)),
        }),
        BrowserRead::UrlSearchParam { key, fallback } => json!({
            "kind": "url_search_param",
            "key": hir_expr_json_inner(key, span),
            "fallback": fallback.as_ref().map(|expr| hir_expr_json_inner(expr.as_ref(), span)),
        }),
        BrowserRead::Declared { helper, args } => json!({
            "kind": "declared",
            "helper": helper,
            "args": args
                .iter()
                .map(|arg| hir_expr_json_inner(arg, span))
                .collect::<Vec<_>>(),
        }),
    }
}
