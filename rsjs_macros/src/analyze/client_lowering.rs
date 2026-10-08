// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Handler HIR types and their lowering into syn-free Client IR.
//!
//! Client type inference lives in `rsjs_ir::analysis::typing` (the
//! whole-island typing pass); the analyzer computes `Let` binding
//! types there with full island context and this module only copies
//! them onto the emitted [`ClientStmt::Let`].
use super::*;

/// Frontend HIR for handler bodies.
///
/// This layer preserves source spans before the syn-free Client IR boundary.
/// Supported handler syntax lowers from this HIR into Client IR v3, which is
/// the stored handler body. Legacy handler shapes are derived only in tests or
/// diagnostics that explicitly ask for them.
#[derive(Clone)]
pub(super) struct HandlerHirStmt {
    pub(super) span: proc_macro2::Span,
    pub(super) kind: HandlerHirStmtKind,
}

#[derive(Clone)]
pub(super) struct HandlerHirExpr {
    pub(super) span: proc_macro2::Span,
    pub(super) expr: FrontendExpr,
}

#[derive(Clone)]
pub(super) struct HandlerHirSimpleClosure {
    pub(super) param: String,
    pub(super) body: Box<HandlerHirExpr>,
}

#[derive(Clone)]
pub(super) enum HandlerHirBrowserEffect {
    WindowScrollTo {
        x: HandlerHirExpr,
        y: HandlerHirExpr,
    },
    WindowScrollBy {
        x: HandlerHirExpr,
        y: HandlerHirExpr,
    },
    WindowPrint,
    LocationSetHash {
        value: HandlerHirExpr,
    },
    LocationAssign {
        url: HandlerHirExpr,
    },
    LocationReplace {
        url: HandlerHirExpr,
    },
    LocationReload,
    HistoryPushState {
        url: HandlerHirExpr,
    },
    HistoryReplaceState {
        url: HandlerHirExpr,
    },
    HistoryBack,
    HistoryForward,
    HistoryGo {
        delta: HandlerHirExpr,
    },
    DocumentDispatchEvent {
        name: HandlerHirExpr,
        detail: Option<HandlerHirExpr>,
    },
    DocumentSetTitle {
        title: HandlerHirExpr,
    },
    ClipboardWriteText {
        text: HandlerHirExpr,
    },
    ClipboardReadTextInto {
        target: SignalId,
        fallback: Option<HandlerHirExpr>,
    },
    WebAuthnCreateJsonInto {
        options_json: HandlerHirExpr,
        target: SignalId,
        fallback: Option<HandlerHirExpr>,
    },
    WebAuthnGetJsonInto {
        options_json: HandlerHirExpr,
        target: SignalId,
        fallback: Option<HandlerHirExpr>,
    },
    EventFileTextInto {
        source: EventFileSource,
        target: SignalId,
        fallback: Option<HandlerHirExpr>,
    },
    StorageSetItem {
        area: StorageArea,
        key: HandlerHirExpr,
        value: HandlerHirExpr,
    },
    StorageRemoveItem {
        area: StorageArea,
        key: HandlerHirExpr,
    },
    UrlSetSearchParam {
        mode: UrlHistoryMode,
        key: HandlerHirExpr,
        value: HandlerHirExpr,
    },
    UrlDeleteSearchParam {
        mode: UrlHistoryMode,
        key: HandlerHirExpr,
    },
    /// An effect declared in `rsjs_ir::browser_registry` with a
    /// [`HelperLowering::DeclaredEffect`] lowering: the registry entry
    /// carries the JS template, so no dedicated variant is needed.
    ///
    /// [`HelperLowering::DeclaredEffect`]: rsjs_ir::browser_registry::HelperLowering::DeclaredEffect
    Declared {
        /// Stable registry id (`<module>.<method>`).
        helper: &'static str,
        /// Lowered reactive arguments in source order.
        args: Vec<HandlerHirExpr>,
    },
}

#[derive(Clone)]
pub(super) enum HandlerHirNodeRefEffect {
    Focus,
    Blur,
    ScrollIntoView,
    Click,
    RequestSubmit,
    Reset,
    ReportValidity,
    ShowModal,
    Close,
    ShowPopover,
    HidePopover,
    TogglePopover,
    ScrollTo {
        x: HandlerHirExpr,
        y: HandlerHirExpr,
    },
    ScrollBy {
        x: HandlerHirExpr,
        y: HandlerHirExpr,
    },
    SetPointerCapture(HandlerHirExpr),
    ReleasePointerCapture(HandlerHirExpr),
    Select,
    SetValue(HandlerHirExpr),
    SetChecked(HandlerHirExpr),
    SetCustomValidity(HandlerHirExpr),
    SetSelectionRange(HandlerHirExpr, HandlerHirExpr),
    SetTextContent(HandlerHirExpr),
    AddClass(HandlerHirExpr),
    RemoveClass(HandlerHirExpr),
    ToggleClass(HandlerHirExpr),
    SetAttribute {
        name: String,
        value: HandlerHirExpr,
    },
    RemoveAttribute {
        name: String,
    },
    SetStyleProperty {
        name: String,
        value: HandlerHirExpr,
    },
}

#[derive(Clone)]
pub(super) enum HandlerHirStmtKind {
    SignalSet {
        id: SignalId,
        value: HandlerHirExpr,
    },
    SignalUpdate {
        id: SignalId,
        closure: HandlerHirSimpleClosure,
    },
    Expr(HandlerHirExpr),
    Let {
        local: LocalId,
        name: String,
        /// Client value shape of the binding, computed by the
        /// whole-island typing pass at `let` lowering time (see
        /// `HandlerLocalBinding::ty`). Copied onto the emitted
        /// [`ClientStmt::Let`].
        ty: ClientType,
        value: HandlerHirExpr,
    },
    Assign {
        local: LocalId,
        value: HandlerHirExpr,
    },
    EventPreventDefault,
    EventStopPropagation,
    Console {
        level: ConsoleLevel,
        args: Vec<HandlerHirExpr>,
    },
    BrowserEffect(HandlerHirBrowserEffect),
    CallbackInvoke {
        slot: CallbackSlotId,
        payload: HandlerHirExpr,
    },
    If {
        condition: HandlerHirExpr,
        then_statements: Vec<HandlerHirStmt>,
        else_statements: Vec<HandlerHirStmt>,
    },
    ForRange {
        local: LocalId,
        name: String,
        integer_domain: IntegerDomain,
        start: HandlerHirExpr,
        end: HandlerHirExpr,
        inclusive: bool,
        body: Vec<HandlerHirStmt>,
    },
    RunMutation {
        id: MutationId,
        args: Vec<HandlerHirExpr>,
    },
    QueryRefetch {
        id: QueryId,
    },
    QueryInvalidate {
        id: QueryId,
    },
    NodeRefEffect {
        id: NodeRefId,
        effect: HandlerHirNodeRefEffect,
    },
}

impl HandlerHirStmt {
    pub(super) fn new(span: proc_macro2::Span, kind: HandlerHirStmtKind) -> Self {
        Self { span, kind }
    }
}

impl HandlerHirExpr {
    pub(super) fn new(span: proc_macro2::Span, expr: FrontendExpr) -> Self {
        Self { span, expr }
    }
}

pub(super) fn lower_handler_hir_to_client_lowering(
    statements: Vec<HandlerHirStmt>,
) -> syn::Result<HandlerLowering> {
    let body_ir = lower_handler_hir_to_client_block(&statements)?;
    Ok(HandlerLowering {
        body_ir,
        body_hir: statements,
        debounce_ms: None,
        source_name: None,
        source_anchor: None,
    })
}

pub(super) fn lower_handler_hir_to_client_block(
    statements: &[HandlerHirStmt],
) -> syn::Result<ClientBlock> {
    let mut block = ClientBlock::new();
    for statement in statements {
        block.push(lower_handler_hir_stmt_to_client_stmt(statement)?);
    }
    Ok(block)
}

pub(super) fn lower_handler_hir_stmt_to_client_stmt(
    statement: &HandlerHirStmt,
) -> syn::Result<ClientStmt> {
    match &statement.kind {
        HandlerHirStmtKind::SignalSet { id, value } => Ok(ClientStmt::SignalSet {
            id: *id,
            value: lower_handler_hir_expr_to_client_expr(value)?,
        }),
        HandlerHirStmtKind::SignalUpdate { id, closure } => Ok(ClientStmt::SignalUpdate {
            id: *id,
            closure: lower_handler_hir_closure_to_client_closure(closure)?,
        }),
        HandlerHirStmtKind::Expr(expr) => Ok(ClientStmt::Expr(
            lower_handler_hir_expr_to_client_expr(expr)?,
        )),
        HandlerHirStmtKind::Let {
            local,
            name,
            ty,
            value,
        } => Ok(ClientStmt::Let {
            local: ClientLocalId(local.0),
            name: name.clone(),
            ty: ty.clone(),
            value: Some(lower_handler_hir_expr_to_client_expr(value)?),
        }),
        HandlerHirStmtKind::Assign { local, value } => Ok(ClientStmt::Assign {
            local: ClientLocalId(local.0),
            value: lower_handler_hir_expr_to_client_expr(value)?,
        }),
        HandlerHirStmtKind::EventPreventDefault => Ok(ClientStmt::EventPreventDefault),
        HandlerHirStmtKind::EventStopPropagation => Ok(ClientStmt::EventStopPropagation),
        HandlerHirStmtKind::Console { level, args } => Ok(ClientStmt::Console {
            level: *level,
            args: args
                .iter()
                .map(lower_handler_hir_expr_to_client_expr)
                .collect::<syn::Result<Vec<_>>>()?,
        }),
        HandlerHirStmtKind::BrowserEffect(effect) => Ok(ClientStmt::BrowserEffect(
            lower_handler_hir_browser_effect_to_client_browser_effect(effect)?,
        )),
        HandlerHirStmtKind::CallbackInvoke { slot, payload } => Ok(ClientStmt::CallbackInvoke {
            callback: *slot,
            payload: lower_handler_hir_expr_to_client_expr(payload)?,
        }),
        HandlerHirStmtKind::If {
            condition,
            then_statements,
            else_statements,
        } => Ok(ClientStmt::If {
            condition: lower_handler_hir_expr_to_client_expr(condition)?,
            then_block: lower_handler_hir_to_client_block(then_statements)?,
            else_block: if else_statements.is_empty() {
                None
            } else {
                Some(lower_handler_hir_to_client_block(else_statements)?)
            },
        }),
        HandlerHirStmtKind::ForRange {
            local,
            name,
            integer_domain,
            start,
            end,
            inclusive,
            body,
        } => Ok(ClientStmt::ForRange {
            local: ClientLocalId(local.0),
            name: name.clone(),
            integer_domain: *integer_domain,
            start: lower_handler_hir_expr_to_client_expr(start)?,
            end: lower_handler_hir_expr_to_client_expr(end)?,
            inclusive: *inclusive,
            body: lower_handler_hir_to_client_block(body)?,
        }),
        HandlerHirStmtKind::RunMutation { id, args } => Ok(ClientStmt::RunMutation {
            mutation: *id,
            args: args
                .iter()
                .map(lower_handler_hir_expr_to_client_expr)
                .collect::<syn::Result<Vec<_>>>()?,
        }),
        HandlerHirStmtKind::QueryRefetch { id } => Ok(ClientStmt::QueryRefetch { query: *id }),
        HandlerHirStmtKind::QueryInvalidate { id } => {
            Ok(ClientStmt::QueryInvalidate { query: *id })
        }
        HandlerHirStmtKind::NodeRefEffect { id, effect } => Ok(ClientStmt::NodeRefEffect {
            id: *id,
            effect: lower_handler_hir_node_ref_effect_to_client_node_ref_effect(effect)?,
        }),
    }
}

pub(super) fn lower_handler_hir_expr_to_client_expr(
    expr: &HandlerHirExpr,
) -> syn::Result<ClientExpr> {
    lower_handler_ir_expr_to_client_expr(&expr.expr, expr.span)
}

pub(super) fn lower_handler_ir_expr_to_client_expr(
    expr: &FrontendExpr,
    span: proc_macro2::Span,
) -> syn::Result<ClientExpr> {
    Ok(match expr {
        FrontendExpr::Literal(value) => ClientExpr::Literal(value.clone()),
        FrontendExpr::ArrayLit(elems) => ClientExpr::ArrayLit(elems.clone()),
        FrontendExpr::SignalRead(id) => ClientExpr::SignalRead(*id),
        FrontendExpr::Local(id) => ClientExpr::Local(ClientLocalId(id.0)),
        FrontendExpr::EventValue(value) => ClientExpr::EventRead(*value),
        FrontendExpr::EventCurrentTarget => ClientExpr::EventCurrentTarget,
        FrontendExpr::QueryRead { id, accessor } => ClientExpr::QueryRead {
            id: *id,
            accessor: *accessor,
        },
        FrontendExpr::MutationRead { id, accessor } => ClientExpr::MutationRead {
            id: *id,
            accessor: *accessor,
        },
        FrontendExpr::BinaryOp { op, lhs, rhs } => ClientExpr::Binary {
            op: handler_hir_bin_op_to_client_bin_op(*op),
            left: Box::new(lower_handler_ir_expr_to_client_expr(lhs, span)?),
            right: Box::new(lower_handler_ir_expr_to_client_expr(rhs, span)?),
        },
        FrontendExpr::UnaryNeg { domain, expr } => ClientExpr::Unary {
            op: ClientUnaryOp::CheckedNeg(*domain),
            expr: Box::new(lower_handler_ir_expr_to_client_expr(expr, span)?),
        },
        FrontendExpr::Text {
            template,
            plan,
            args,
            attribute,
        } => {
            let mut lowered = vec![ClientExpr::string(template), ClientExpr::string(plan)];
            lowered.extend(
                args.iter()
                    .map(|arg| lower_handler_ir_expr_to_client_expr(arg, span))
                    .collect::<syn::Result<Vec<_>>>()?,
            );
            ClientExpr::call(
                ClientFunctionRef::Runtime(
                    if *attribute {
                        "format_text_attr"
                    } else {
                        "format_text"
                    }
                    .to_owned(),
                ),
                lowered,
            )
        }
        FrontendExpr::ClientCall {
            edge,
            authored_path,
            name,
            args,
        } => ClientExpr::Call {
            function: ClientFunctionRef::Helper {
                edge: *edge,
                authored_path: authored_path.clone(),
                name: name.clone(),
            },
            args: args
                .iter()
                .map(|arg| lower_handler_ir_expr_to_client_expr(arg, span))
                .collect::<syn::Result<Vec<_>>>()?,
        },
        FrontendExpr::EventCollectionLen(collection) => ClientExpr::EventCollectionLen(*collection),
        FrontendExpr::EventCollectionRead {
            collection,
            index,
            field,
        } => ClientExpr::EventCollectionRead {
            collection: *collection,
            index: Box::new(lower_handler_ir_expr_to_client_expr(index, span)?),
            field: *field,
        },
        FrontendExpr::NodeRefRead { id, accessor } => ClientExpr::NodeRefRead {
            id: *id,
            accessor: accessor.clone(),
        },
        FrontendExpr::DomRead { target, accessor } => ClientExpr::DomRead {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
            accessor: accessor.clone(),
        },
        FrontendExpr::EventTargetWithinNodeRef { id, inside } => {
            ClientExpr::EventTargetWithinNodeRef {
                id: *id,
                inside: *inside,
            }
        }
        FrontendExpr::EventAttribute { source, attr } => ClientExpr::EventAttribute {
            source: *source,
            attr: attr.clone(),
        },
        FrontendExpr::BrowserRead(read) => ClientExpr::BrowserRead(
            lower_handler_hir_browser_read_to_client_browser_read(read, span)?,
        ),
        FrontendExpr::EventPayload { attr } => ClientExpr::EventPayload { attr: attr.clone() },
        FrontendExpr::CallbackPayload => ClientExpr::CallbackPayload,
        FrontendExpr::NodePayload { attr, tokens } => ClientExpr::NodePayload {
            attr: attr.clone(),
            tokens: tokens.clone(),
        },
        FrontendExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => ClientExpr::Conditional {
            condition: Box::new(lower_handler_ir_expr_to_client_expr(condition, span)?),
            then_expr: Box::new(lower_handler_ir_expr_to_client_expr(then_expr, span)?),
            else_expr: Box::new(lower_handler_ir_expr_to_client_expr(else_expr, span)?),
        },
        FrontendExpr::IsNonEmpty { target } => ClientExpr::IsNonEmpty {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
        },
        FrontendExpr::StringToLowercase { target } => ClientExpr::StringToLowercase {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
        },
        FrontendExpr::StringContains { haystack, needle } => ClientExpr::StringContains {
            haystack: Box::new(lower_handler_ir_expr_to_client_expr(haystack, span)?),
            needle: Box::new(lower_handler_ir_expr_to_client_expr(needle, span)?),
        },
        FrontendExpr::StringTransform { op, target } => ClientExpr::StringTransform {
            op: *op,
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
        },
        FrontendExpr::StringPredicate {
            op,
            haystack,
            needle,
        } => ClientExpr::StringPredicate {
            op: *op,
            haystack: Box::new(lower_handler_ir_expr_to_client_expr(haystack, span)?),
            needle: Box::new(lower_handler_ir_expr_to_client_expr(needle, span)?),
        },
        FrontendExpr::Project { target, path } => ClientExpr::Project {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
            path: path.clone(),
        },
        FrontendExpr::AsType { target, ty } => ClientExpr::AsType {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
            ty: *ty,
        },
        FrontendExpr::IsPresent { target } => ClientExpr::IsPresent {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
        },
        FrontendExpr::Eq { target, value } => ClientExpr::Eq {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
            value: value.clone(),
        },
        FrontendExpr::Neq { target, value } => ClientExpr::Neq {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
            value: value.clone(),
        },
        FrontendExpr::Coalesce { target, fallback } => ClientExpr::Coalesce {
            target: Box::new(lower_handler_ir_expr_to_client_expr(target, span)?),
            fallback: fallback.clone(),
        },
        FrontendExpr::Encoding { op, value } => ClientExpr::Encoding {
            op: *op,
            value: Box::new(lower_handler_ir_expr_to_client_expr(value, span)?),
        },
        FrontendExpr::QueryKey(parts) => ClientExpr::QueryKey(
            parts
                .iter()
                .map(|part| lower_handler_ir_expr_to_client_expr(part, span))
                .collect::<syn::Result<Vec<_>>>()?,
        ),
    })
}

pub(super) fn lower_handler_hir_closure_to_client_closure(
    closure: &HandlerHirSimpleClosure,
) -> syn::Result<ClientSimpleClosure> {
    Ok(ClientSimpleClosure::new(
        closure.param.clone(),
        lower_handler_hir_expr_to_client_expr(&closure.body)?,
    ))
}

pub(super) fn lower_handler_hir_browser_read_to_client_browser_read(
    read: &BrowserRead,
    span: proc_macro2::Span,
) -> syn::Result<ClientBrowserRead> {
    Ok(match read {
        BrowserRead::Window(field) => ClientBrowserRead::Window(*field),
        BrowserRead::Location(field) => ClientBrowserRead::Location(*field),
        BrowserRead::Storage {
            area,
            value_type,
            key,
            fallback,
        } => ClientBrowserRead::Storage {
            area: *area,
            value_type: *value_type,
            key: Box::new(lower_handler_ir_expr_to_client_expr(key, span)?),
            fallback: fallback
                .as_deref()
                .map(|fallback| lower_handler_ir_expr_to_client_expr(fallback, span))
                .transpose()?
                .map(Box::new),
        },
        BrowserRead::UrlSearchParam { key, fallback } => ClientBrowserRead::UrlSearchParam {
            key: Box::new(lower_handler_ir_expr_to_client_expr(key, span)?),
            fallback: fallback
                .as_deref()
                .map(|fallback| lower_handler_ir_expr_to_client_expr(fallback, span))
                .transpose()?
                .map(Box::new),
        },
        BrowserRead::Declared { helper, args } => ClientBrowserRead::Declared {
            helper: (*helper).to_string(),
            args: args
                .iter()
                .map(|arg| lower_handler_ir_expr_to_client_expr(arg, span))
                .collect::<syn::Result<Vec<_>>>()?,
        },
    })
}

pub(super) fn lower_handler_hir_browser_effect_to_client_browser_effect(
    effect: &HandlerHirBrowserEffect,
) -> syn::Result<ClientBrowserEffect> {
    Ok(match effect {
        HandlerHirBrowserEffect::WindowScrollTo { x, y } => ClientBrowserEffect::WindowScrollTo {
            x: lower_handler_hir_expr_to_client_expr(x)?,
            y: lower_handler_hir_expr_to_client_expr(y)?,
        },
        HandlerHirBrowserEffect::WindowScrollBy { x, y } => ClientBrowserEffect::WindowScrollBy {
            x: lower_handler_hir_expr_to_client_expr(x)?,
            y: lower_handler_hir_expr_to_client_expr(y)?,
        },
        HandlerHirBrowserEffect::WindowPrint => ClientBrowserEffect::WindowPrint,
        HandlerHirBrowserEffect::LocationSetHash { value } => {
            ClientBrowserEffect::LocationSetHash {
                value: lower_handler_hir_expr_to_client_expr(value)?,
            }
        }
        HandlerHirBrowserEffect::LocationAssign { url } => ClientBrowserEffect::LocationAssign {
            url: lower_handler_hir_expr_to_client_expr(url)?,
        },
        HandlerHirBrowserEffect::LocationReplace { url } => ClientBrowserEffect::LocationReplace {
            url: lower_handler_hir_expr_to_client_expr(url)?,
        },
        HandlerHirBrowserEffect::LocationReload => ClientBrowserEffect::LocationReload,
        HandlerHirBrowserEffect::HistoryPushState { url } => {
            ClientBrowserEffect::HistoryPushState {
                url: lower_handler_hir_expr_to_client_expr(url)?,
            }
        }
        HandlerHirBrowserEffect::HistoryReplaceState { url } => {
            ClientBrowserEffect::HistoryReplaceState {
                url: lower_handler_hir_expr_to_client_expr(url)?,
            }
        }
        HandlerHirBrowserEffect::HistoryBack => ClientBrowserEffect::HistoryBack,
        HandlerHirBrowserEffect::HistoryForward => ClientBrowserEffect::HistoryForward,
        HandlerHirBrowserEffect::HistoryGo { delta } => ClientBrowserEffect::HistoryGo {
            delta: lower_handler_hir_expr_to_client_expr(delta)?,
        },
        HandlerHirBrowserEffect::DocumentDispatchEvent { name, detail } => {
            ClientBrowserEffect::DocumentDispatchEvent {
                name: lower_handler_hir_expr_to_client_expr(name)?,
                detail: detail
                    .as_ref()
                    .map(lower_handler_hir_expr_to_client_expr)
                    .transpose()?,
            }
        }
        HandlerHirBrowserEffect::DocumentSetTitle { title } => {
            ClientBrowserEffect::DocumentSetTitle {
                title: lower_handler_hir_expr_to_client_expr(title)?,
            }
        }
        HandlerHirBrowserEffect::ClipboardWriteText { text } => {
            ClientBrowserEffect::ClipboardWriteText {
                text: lower_handler_hir_expr_to_client_expr(text)?,
            }
        }
        HandlerHirBrowserEffect::ClipboardReadTextInto { target, fallback } => {
            ClientBrowserEffect::ClipboardReadTextInto {
                target: *target,
                fallback: fallback
                    .as_ref()
                    .map(lower_handler_hir_expr_to_client_expr)
                    .transpose()?,
            }
        }
        HandlerHirBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            target,
            fallback,
        } => ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json: lower_handler_hir_expr_to_client_expr(options_json)?,
            target: *target,
            fallback: fallback
                .as_ref()
                .map(lower_handler_hir_expr_to_client_expr)
                .transpose()?,
        },
        HandlerHirBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            target,
            fallback,
        } => ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json: lower_handler_hir_expr_to_client_expr(options_json)?,
            target: *target,
            fallback: fallback
                .as_ref()
                .map(lower_handler_hir_expr_to_client_expr)
                .transpose()?,
        },
        HandlerHirBrowserEffect::EventFileTextInto {
            source,
            target,
            fallback,
        } => ClientBrowserEffect::EventFileTextInto {
            source: *source,
            target: *target,
            fallback: fallback
                .as_ref()
                .map(lower_handler_hir_expr_to_client_expr)
                .transpose()?,
        },
        HandlerHirBrowserEffect::StorageSetItem { area, key, value } => {
            ClientBrowserEffect::StorageSetItem {
                area: *area,
                key: lower_handler_hir_expr_to_client_expr(key)?,
                value: lower_handler_hir_expr_to_client_expr(value)?,
            }
        }
        HandlerHirBrowserEffect::StorageRemoveItem { area, key } => {
            ClientBrowserEffect::StorageRemoveItem {
                area: *area,
                key: lower_handler_hir_expr_to_client_expr(key)?,
            }
        }
        HandlerHirBrowserEffect::UrlSetSearchParam { mode, key, value } => {
            ClientBrowserEffect::UrlSetSearchParam {
                mode: *mode,
                key: lower_handler_hir_expr_to_client_expr(key)?,
                value: lower_handler_hir_expr_to_client_expr(value)?,
            }
        }
        HandlerHirBrowserEffect::UrlDeleteSearchParam { mode, key } => {
            ClientBrowserEffect::UrlDeleteSearchParam {
                mode: *mode,
                key: lower_handler_hir_expr_to_client_expr(key)?,
            }
        }
        HandlerHirBrowserEffect::Declared { helper, args } => ClientBrowserEffect::Declared {
            helper: (*helper).to_string(),
            args: args
                .iter()
                .map(lower_handler_hir_expr_to_client_expr)
                .collect::<syn::Result<Vec<_>>>()?,
        },
    })
}

pub(super) fn lower_handler_hir_node_ref_effect_to_client_node_ref_effect(
    effect: &HandlerHirNodeRefEffect,
) -> syn::Result<ClientNodeRefEffect> {
    Ok(match effect {
        HandlerHirNodeRefEffect::Focus => ClientNodeRefEffect::Focus,
        HandlerHirNodeRefEffect::Blur => ClientNodeRefEffect::Blur,
        HandlerHirNodeRefEffect::ScrollIntoView => ClientNodeRefEffect::ScrollIntoView,
        HandlerHirNodeRefEffect::Click => ClientNodeRefEffect::Click,
        HandlerHirNodeRefEffect::RequestSubmit => ClientNodeRefEffect::RequestSubmit,
        HandlerHirNodeRefEffect::Reset => ClientNodeRefEffect::Reset,
        HandlerHirNodeRefEffect::ReportValidity => ClientNodeRefEffect::ReportValidity,
        HandlerHirNodeRefEffect::ShowModal => ClientNodeRefEffect::ShowModal,
        HandlerHirNodeRefEffect::Close => ClientNodeRefEffect::Close,
        HandlerHirNodeRefEffect::ShowPopover => ClientNodeRefEffect::ShowPopover,
        HandlerHirNodeRefEffect::HidePopover => ClientNodeRefEffect::HidePopover,
        HandlerHirNodeRefEffect::TogglePopover => ClientNodeRefEffect::TogglePopover,
        HandlerHirNodeRefEffect::ScrollTo { x, y } => ClientNodeRefEffect::ScrollTo {
            x: lower_handler_hir_expr_to_client_expr(x)?,
            y: lower_handler_hir_expr_to_client_expr(y)?,
        },
        HandlerHirNodeRefEffect::ScrollBy { x, y } => ClientNodeRefEffect::ScrollBy {
            x: lower_handler_hir_expr_to_client_expr(x)?,
            y: lower_handler_hir_expr_to_client_expr(y)?,
        },
        HandlerHirNodeRefEffect::SetPointerCapture(pointer_id) => {
            ClientNodeRefEffect::SetPointerCapture(lower_handler_hir_expr_to_client_expr(
                pointer_id,
            )?)
        }
        HandlerHirNodeRefEffect::ReleasePointerCapture(pointer_id) => {
            ClientNodeRefEffect::ReleasePointerCapture(lower_handler_hir_expr_to_client_expr(
                pointer_id,
            )?)
        }
        HandlerHirNodeRefEffect::Select => ClientNodeRefEffect::Select,
        HandlerHirNodeRefEffect::SetValue(value) => {
            ClientNodeRefEffect::SetValue(lower_handler_hir_expr_to_client_expr(value)?)
        }
        HandlerHirNodeRefEffect::SetChecked(value) => {
            ClientNodeRefEffect::SetChecked(lower_handler_hir_expr_to_client_expr(value)?)
        }
        HandlerHirNodeRefEffect::SetCustomValidity(value) => {
            ClientNodeRefEffect::SetCustomValidity(lower_handler_hir_expr_to_client_expr(value)?)
        }
        HandlerHirNodeRefEffect::SetSelectionRange(start, end) => {
            ClientNodeRefEffect::SetSelectionRange(
                lower_handler_hir_expr_to_client_expr(start)?,
                lower_handler_hir_expr_to_client_expr(end)?,
            )
        }
        HandlerHirNodeRefEffect::SetTextContent(value) => {
            ClientNodeRefEffect::SetTextContent(lower_handler_hir_expr_to_client_expr(value)?)
        }
        HandlerHirNodeRefEffect::AddClass(value) => {
            ClientNodeRefEffect::AddClass(lower_handler_hir_expr_to_client_expr(value)?)
        }
        HandlerHirNodeRefEffect::RemoveClass(value) => {
            ClientNodeRefEffect::RemoveClass(lower_handler_hir_expr_to_client_expr(value)?)
        }
        HandlerHirNodeRefEffect::ToggleClass(value) => {
            ClientNodeRefEffect::ToggleClass(lower_handler_hir_expr_to_client_expr(value)?)
        }
        HandlerHirNodeRefEffect::SetAttribute { name, value } => {
            ClientNodeRefEffect::SetAttribute {
                name: name.clone(),
                value: lower_handler_hir_expr_to_client_expr(value)?,
            }
        }
        HandlerHirNodeRefEffect::RemoveAttribute { name } => {
            ClientNodeRefEffect::RemoveAttribute { name: name.clone() }
        }
        HandlerHirNodeRefEffect::SetStyleProperty { name, value } => {
            ClientNodeRefEffect::SetStyleProperty {
                name: name.clone(),
                value: lower_handler_hir_expr_to_client_expr(value)?,
            }
        }
    })
}

pub(super) fn handler_hir_bin_op_to_client_bin_op(op: BinOp) -> ClientBinaryOp {
    match op {
        BinOp::Checked { op, domain } => ClientBinaryOp::Checked { op, domain },
        BinOp::Add => ClientBinaryOp::Add,
        BinOp::Sub => ClientBinaryOp::Sub,
        BinOp::Mul => ClientBinaryOp::Mul,
        BinOp::Div => ClientBinaryOp::Div,
        BinOp::DivInt => ClientBinaryOp::DivInt,
        BinOp::Rem => ClientBinaryOp::Rem,
        BinOp::Eq => ClientBinaryOp::Eq,
        BinOp::Neq => ClientBinaryOp::Ne,
        BinOp::Lt => ClientBinaryOp::Lt,
        BinOp::Gt => ClientBinaryOp::Gt,
        BinOp::Le => ClientBinaryOp::Le,
        BinOp::Ge => ClientBinaryOp::Ge,
        BinOp::And => ClientBinaryOp::And,
        BinOp::Or => ClientBinaryOp::Or,
        BinOp::DynDiv => ClientBinaryOp::DynDiv,
        BinOp::DynEq => ClientBinaryOp::DynEq,
        BinOp::DynNe => ClientBinaryOp::DynNe,
    }
}

pub(super) fn substitute_client_callback_payload_block(
    block: &mut ClientBlock,
    payload: &ClientExpr,
) {
    for stmt in &mut block.stmts {
        substitute_client_callback_payload_stmt(stmt, payload);
    }
}

pub(super) fn substitute_client_callback_payload_stmt(stmt: &mut ClientStmt, payload: &ClientExpr) {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => {
            substitute_client_callback_payload_expr(expr, payload);
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                substitute_client_callback_payload_expr(value, payload);
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => {
            substitute_client_callback_payload_expr(closure.body.as_mut(), payload);
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for arg in args {
                substitute_client_callback_payload_expr(arg, payload);
            }
        }
        ClientStmt::CallbackInvoke {
            payload: callback_payload,
            ..
        } => {
            substitute_client_callback_payload_expr(callback_payload, payload);
        }
        ClientStmt::BrowserEffect(effect) => {
            substitute_client_callback_payload_browser_effect(effect, payload);
        }
        ClientStmt::NodeRefEffect { effect, .. } => {
            substitute_client_callback_payload_node_ref_effect(effect, payload);
        }
        ClientStmt::Block(block) => substitute_client_callback_payload_block(block, payload),
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            substitute_client_callback_payload_expr(condition, payload);
            substitute_client_callback_payload_block(then_block, payload);
            if let Some(else_block) = else_block {
                substitute_client_callback_payload_block(else_block, payload);
            }
        }
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            substitute_client_callback_payload_expr(start, payload);
            substitute_client_callback_payload_expr(end, payload);
            substitute_client_callback_payload_block(body, payload);
        }
        _ => {}
    }
}

pub(super) fn substitute_client_callback_payload_expr(expr: &mut ClientExpr, payload: &ClientExpr) {
    match expr {
        ClientExpr::CallbackPayload => {
            *expr = payload.clone();
        }
        ClientExpr::Unit
        | ClientExpr::Literal(_)
        | ClientExpr::Local(_)
        | ClientExpr::SignalRead(_)
        | ClientExpr::QueryRead { .. }
        | ClientExpr::MutationRead { .. }
        | ClientExpr::EventRead(_)
        | ClientExpr::EventCurrentTarget
        | ClientExpr::EventCollectionLen(_)
        | ClientExpr::NodeRefRead { .. }
        | ClientExpr::EventTargetWithinNodeRef { .. }
        | ClientExpr::EventAttribute { .. }
        | ClientExpr::EventPayload { .. }
        | ClientExpr::NodePayload { .. } => {}
        ClientExpr::EventCollectionRead { index, .. }
        | ClientExpr::DomRead { target: index, .. }
        | ClientExpr::Encoding { value: index, .. }
        | ClientExpr::IsNonEmpty { target: index }
        | ClientExpr::StringToLowercase { target: index }
        | ClientExpr::Project { target: index, .. }
        | ClientExpr::AsType { target: index, .. }
        | ClientExpr::IsPresent { target: index }
        | ClientExpr::Eq { target: index, .. }
        | ClientExpr::Neq { target: index, .. }
        | ClientExpr::Coalesce { target: index, .. }
        | ClientExpr::Unary { expr: index, .. }
        | ClientExpr::Field { base: index, .. } => {
            substitute_client_callback_payload_expr(index, payload);
        }
        ClientExpr::BrowserRead(read) => {
            substitute_client_callback_payload_browser_read(read, payload);
        }
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            substitute_client_callback_payload_expr(condition, payload);
            substitute_client_callback_payload_expr(then_expr, payload);
            substitute_client_callback_payload_expr(else_expr, payload);
        }
        ClientExpr::StringContains { haystack, needle } => {
            substitute_client_callback_payload_expr(haystack, payload);
            substitute_client_callback_payload_expr(needle, payload);
        }
        ClientExpr::Binary { left, right, .. } => {
            substitute_client_callback_payload_expr(left, payload);
            substitute_client_callback_payload_expr(right, payload);
        }
        ClientExpr::Index { base, index } => {
            substitute_client_callback_payload_expr(base, payload);
            substitute_client_callback_payload_expr(index, payload);
        }
        ClientExpr::Call { args, .. } => {
            for arg in args {
                substitute_client_callback_payload_expr(arg, payload);
            }
        }
        _ => {}
    }
}

pub(super) fn substitute_client_callback_payload_browser_read(
    read: &mut ClientBrowserRead,
    payload: &ClientExpr,
) {
    match read {
        ClientBrowserRead::Window(_) | ClientBrowserRead::Location(_) => {}
        ClientBrowserRead::Storage { key, fallback, .. }
        | ClientBrowserRead::UrlSearchParam { key, fallback } => {
            substitute_client_callback_payload_expr(key, payload);
            if let Some(fallback) = fallback {
                substitute_client_callback_payload_expr(fallback, payload);
            }
        }
        _ => {}
    }
}

pub(super) fn substitute_client_callback_payload_browser_effect(
    effect: &mut ClientBrowserEffect,
    payload: &ClientExpr,
) {
    match effect {
        ClientBrowserEffect::Declared { args, .. } => {
            for arg in args {
                substitute_client_callback_payload_expr(arg, payload);
            }
        }
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => {
            substitute_client_callback_payload_expr(x, payload);
            substitute_client_callback_payload_expr(y, payload);
        }
        ClientBrowserEffect::WindowPrint
        | ClientBrowserEffect::LocationReload
        | ClientBrowserEffect::HistoryBack
        | ClientBrowserEffect::HistoryForward => {}
        ClientBrowserEffect::LocationSetHash { value } => {
            substitute_client_callback_payload_expr(value, payload);
        }
        ClientBrowserEffect::LocationAssign { url }
        | ClientBrowserEffect::LocationReplace { url }
        | ClientBrowserEffect::HistoryPushState { url }
        | ClientBrowserEffect::HistoryReplaceState { url } => {
            substitute_client_callback_payload_expr(url, payload);
        }
        ClientBrowserEffect::HistoryGo { delta } => {
            substitute_client_callback_payload_expr(delta, payload);
        }
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            substitute_client_callback_payload_expr(name, payload);
            if let Some(detail) = detail {
                substitute_client_callback_payload_expr(detail, payload);
            }
        }
        ClientBrowserEffect::DocumentSetTitle { title } => {
            substitute_client_callback_payload_expr(title, payload);
        }
        ClientBrowserEffect::ClipboardWriteText { text } => {
            substitute_client_callback_payload_expr(text, payload);
        }
        ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | ClientBrowserEffect::EventFileTextInto { fallback, .. } => {
            if let Some(fallback) = fallback {
                substitute_client_callback_payload_expr(fallback, payload);
            }
        }
        ClientBrowserEffect::WebAuthnCreateJsonInto {
            options_json,
            fallback,
            ..
        }
        | ClientBrowserEffect::WebAuthnGetJsonInto {
            options_json,
            fallback,
            ..
        } => {
            substitute_client_callback_payload_expr(options_json, payload);
            if let Some(fallback) = fallback {
                substitute_client_callback_payload_expr(fallback, payload);
            }
        }
        ClientBrowserEffect::StorageSetItem { key, value, .. }
        | ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
            substitute_client_callback_payload_expr(key, payload);
            substitute_client_callback_payload_expr(value, payload);
        }
        ClientBrowserEffect::StorageRemoveItem { key, .. }
        | ClientBrowserEffect::UrlDeleteSearchParam { key, .. } => {
            substitute_client_callback_payload_expr(key, payload);
        }
    }
}

pub(super) fn substitute_client_callback_payload_node_ref_effect(
    effect: &mut ClientNodeRefEffect,
    payload: &ClientExpr,
) {
    match effect {
        ClientNodeRefEffect::Focus
        | ClientNodeRefEffect::Blur
        | ClientNodeRefEffect::ScrollIntoView
        | ClientNodeRefEffect::Click
        | ClientNodeRefEffect::RequestSubmit
        | ClientNodeRefEffect::Reset
        | ClientNodeRefEffect::ReportValidity
        | ClientNodeRefEffect::ShowModal
        | ClientNodeRefEffect::Close
        | ClientNodeRefEffect::ShowPopover
        | ClientNodeRefEffect::HidePopover
        | ClientNodeRefEffect::TogglePopover
        | ClientNodeRefEffect::Select
        | ClientNodeRefEffect::RemoveAttribute { .. } => {}
        ClientNodeRefEffect::ScrollTo { x, y } | ClientNodeRefEffect::ScrollBy { x, y } => {
            substitute_client_callback_payload_expr(x, payload);
            substitute_client_callback_payload_expr(y, payload);
        }
        ClientNodeRefEffect::SetPointerCapture(expr)
        | ClientNodeRefEffect::ReleasePointerCapture(expr)
        | ClientNodeRefEffect::SetValue(expr)
        | ClientNodeRefEffect::SetChecked(expr)
        | ClientNodeRefEffect::SetCustomValidity(expr)
        | ClientNodeRefEffect::SetTextContent(expr)
        | ClientNodeRefEffect::AddClass(expr)
        | ClientNodeRefEffect::RemoveClass(expr)
        | ClientNodeRefEffect::ToggleClass(expr) => {
            substitute_client_callback_payload_expr(expr, payload);
        }
        ClientNodeRefEffect::SetAttribute { value, .. }
        | ClientNodeRefEffect::SetStyleProperty { value, .. } => {
            substitute_client_callback_payload_expr(value, payload);
        }
        ClientNodeRefEffect::SetSelectionRange(start, end) => {
            substitute_client_callback_payload_expr(start, payload);
            substitute_client_callback_payload_expr(end, payload);
        }
    }
}
