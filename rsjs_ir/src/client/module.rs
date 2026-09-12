// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Function and module graph nodes for Client IR v3.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::expr::{ClientBrowserRead, ClientExpr, ClientFunctionId, ClientFunctionRef};
use super::stmt::{ClientBlock, ClientBrowserEffect, ClientNodeRefEffect, ClientStmt};
use super::types::ClientType;

/// Visibility of a Client IR function inside its module graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientVisibility {
    Private,
    Exported,
}

/// Function parameter in Client IR v3.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientParam {
    pub name: String,
    pub ty: ClientType,
}

impl ClientParam {
    pub fn new(name: impl Into<String>, ty: ClientType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// Captured value required by a function body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientCapture {
    pub name: String,
    pub ty: ClientType,
}

impl ClientCapture {
    pub fn new(name: impl Into<String>, ty: ClientType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// Client-side function body.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientFunction {
    pub id: ClientFunctionId,
    pub name: String,
    pub params: Vec<ClientParam>,
    pub return_ty: ClientType,
    pub body: ClientBlock,
    pub visibility: ClientVisibility,
    pub captures: Vec<ClientCapture>,
}

impl ClientFunction {
    pub fn new(id: ClientFunctionId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            params: Vec::new(),
            return_ty: ClientType::Unit,
            body: ClientBlock::new(),
            visibility: ClientVisibility::Private,
            captures: Vec::new(),
        }
    }

    pub fn with_param(mut self, name: impl Into<String>, ty: ClientType) -> Self {
        self.params.push(ClientParam::new(name, ty));
        self
    }

    pub fn with_return_ty(mut self, ty: ClientType) -> Self {
        self.return_ty = ty;
        self
    }

    pub fn with_body(mut self, body: ClientBlock) -> Self {
        self.body = body;
        self
    }

    pub fn exported(mut self) -> Self {
        self.visibility = ClientVisibility::Exported;
        self
    }
}

/// Kind of module represented in the client graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientModuleKind {
    Island,
    ComponentFragment,
    Route,
    ClientModule,
    Runtime,
}

/// Static ES-module import.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientImport {
    pub module: String,
    pub name: String,
    pub alias: Option<String>,
}

impl ClientImport {
    pub fn new(module: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
            alias: None,
        }
    }

    pub fn aliased(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

/// Static ES-module export.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientExport {
    pub name: String,
    pub function: ClientFunctionId,
}

impl ClientExport {
    pub fn function(name: impl Into<String>, function: ClientFunctionId) -> Self {
        Self {
            name: name.into(),
            function,
        }
    }
}

/// Constant value emitted into a client module.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientConstant {
    pub name: String,
    pub ty: ClientType,
    pub value: crate::value::Value,
}

impl ClientConstant {
    pub fn new(name: impl Into<String>, ty: ClientType, value: crate::value::Value) -> Self {
        Self {
            name: name.into(),
            ty,
            value,
        }
    }
}

/// Client module graph node.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientModule {
    pub name: String,
    pub kind: ClientModuleKind,
    pub imports: Vec<ClientImport>,
    pub exports: Vec<ClientExport>,
    pub functions: Vec<ClientFunction>,
    pub constants: Vec<ClientConstant>,
    pub metadata: BTreeMap<String, String>,
}

impl ClientModule {
    pub fn new(name: impl Into<String>, kind: ClientModuleKind) -> Self {
        Self {
            name: name.into(),
            kind,
            imports: Vec::new(),
            exports: Vec::new(),
            functions: Vec::new(),
            constants: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn add_import(&mut self, import: ClientImport) {
        self.imports.push(import);
    }

    pub fn add_export(&mut self, export: ClientExport) {
        self.exports.push(export);
    }

    pub fn add_function(&mut self, function: ClientFunction) {
        self.functions.push(function);
    }

    pub fn add_constant(&mut self, constant: ClientConstant) {
        self.constants.push(constant);
    }
}

/// Remove private functions that are unreachable from this module's exports.
///
/// Roots are functions explicitly listed in [`ClientModule::exports`] plus
/// functions marked [`ClientVisibility::Exported`]. Private functions reached
/// through local Client IR calls are retained. Function IDs are intentionally
/// not renumbered so descriptor/module references remain stable.
pub fn prune_unreachable_client_functions(module: &mut ClientModule) -> usize {
    let functions_by_id = module
        .functions
        .iter()
        .map(|function| (function.id, function))
        .collect::<HashMap<_, _>>();
    let mut reachable = HashSet::new();
    let mut stack = module
        .exports
        .iter()
        .map(|export| export.function)
        .chain(
            module
                .functions
                .iter()
                .filter(|function| function.visibility == ClientVisibility::Exported)
                .map(|function| function.id),
        )
        .collect::<Vec<_>>();

    while let Some(id) = stack.pop() {
        if !reachable.insert(id) {
            continue;
        }
        let Some(function) = functions_by_id.get(&id) else {
            continue;
        };
        collect_local_function_refs_from_block(&function.body, &mut stack);
    }

    let before = module.functions.len();
    module
        .functions
        .retain(|function| reachable.contains(&function.id));
    before - module.functions.len()
}

fn collect_local_function_refs_from_block(block: &ClientBlock, out: &mut Vec<ClientFunctionId>) {
    for stmt in &block.stmts {
        collect_local_function_refs_from_stmt(stmt, out);
    }
}

fn collect_local_function_refs_from_stmt(stmt: &ClientStmt, out: &mut Vec<ClientFunctionId>) {
    match stmt {
        ClientStmt::Expr(expr)
        | ClientStmt::Assign { value: expr, .. }
        | ClientStmt::AssignPlace { value: expr, .. }
        | ClientStmt::SignalSet { value: expr, .. } => {
            collect_local_function_refs_from_expr(expr, out);
        }
        ClientStmt::Let { value, .. } | ClientStmt::Return(value) => {
            if let Some(value) = value {
                collect_local_function_refs_from_expr(value, out);
            }
        }
        ClientStmt::SignalUpdate { closure, .. } => {
            collect_local_function_refs_from_expr(&closure.body, out);
        }
        ClientStmt::EventPreventDefault
        | ClientStmt::EventStopPropagation
        | ClientStmt::QueryRefetch { .. }
        | ClientStmt::QueryInvalidate { .. } => {}
        ClientStmt::Console { args, .. } | ClientStmt::RunMutation { args, .. } => {
            for arg in args {
                collect_local_function_refs_from_expr(arg, out);
            }
        }
        ClientStmt::CallbackInvoke { payload, .. } => {
            collect_local_function_refs_from_expr(payload, out);
        }
        ClientStmt::BrowserEffect(effect) => {
            collect_local_function_refs_from_browser_effect(effect, out);
        }
        ClientStmt::NodeRefEffect { effect, .. } => {
            collect_local_function_refs_from_node_ref_effect(effect, out);
        }
        ClientStmt::Block(block) => collect_local_function_refs_from_block(block, out),
        ClientStmt::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_local_function_refs_from_expr(condition, out);
            collect_local_function_refs_from_block(then_block, out);
            if let Some(else_block) = else_block {
                collect_local_function_refs_from_block(else_block, out);
            }
        }
        ClientStmt::ForRange {
            start, end, body, ..
        } => {
            collect_local_function_refs_from_expr(start, out);
            collect_local_function_refs_from_expr(end, out);
            collect_local_function_refs_from_block(body, out);
        }
    }
}

fn collect_local_function_refs_from_expr(expr: &ClientExpr, out: &mut Vec<ClientFunctionId>) {
    match expr {
        ClientExpr::Unit
        | ClientExpr::Literal(_)
        | ClientExpr::ArrayLit(_)
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
        | ClientExpr::CallbackPayload
        | ClientExpr::NodePayload { .. } => {}
        ClientExpr::EventCollectionRead { index, .. }
        | ClientExpr::DomRead { target: index, .. }
        | ClientExpr::Encoding { value: index, .. }
        | ClientExpr::IsNonEmpty { target: index }
        | ClientExpr::StringToLowercase { target: index }
        | ClientExpr::StringTransform { target: index, .. }
        | ClientExpr::Project { target: index, .. }
        | ClientExpr::AsType { target: index, .. }
        | ClientExpr::IsPresent { target: index }
        | ClientExpr::Eq { target: index, .. }
        | ClientExpr::Neq { target: index, .. }
        | ClientExpr::Coalesce { target: index, .. }
        | ClientExpr::Unary { expr: index, .. }
        | ClientExpr::Field { base: index, .. } => {
            collect_local_function_refs_from_expr(index, out);
        }
        ClientExpr::BrowserRead(read) => collect_local_function_refs_from_browser_read(read, out),
        ClientExpr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_local_function_refs_from_expr(condition, out);
            collect_local_function_refs_from_expr(then_expr, out);
            collect_local_function_refs_from_expr(else_expr, out);
        }
        ClientExpr::StringContains { haystack, needle }
        | ClientExpr::StringPredicate {
            haystack, needle, ..
        } => {
            collect_local_function_refs_from_expr(haystack, out);
            collect_local_function_refs_from_expr(needle, out);
        }
        ClientExpr::Binary { left, right, .. } => {
            collect_local_function_refs_from_expr(left, out);
            collect_local_function_refs_from_expr(right, out);
        }
        ClientExpr::Index { base, index } => {
            collect_local_function_refs_from_expr(base, out);
            collect_local_function_refs_from_expr(index, out);
        }
        ClientExpr::QueryKey(parts) => {
            for part in parts {
                collect_local_function_refs_from_expr(part, out);
            }
        }
        ClientExpr::StructInit { fields, .. } => {
            for field in fields {
                collect_local_function_refs_from_expr(&field.value, out);
            }
        }
        ClientExpr::Call { function, args } => {
            if let ClientFunctionRef::Local(id) = function {
                out.push(*id);
            }
            for arg in args {
                collect_local_function_refs_from_expr(arg, out);
            }
        }
    }
}

fn collect_local_function_refs_from_browser_read(
    read: &ClientBrowserRead,
    out: &mut Vec<ClientFunctionId>,
) {
    match read {
        ClientBrowserRead::Window(_) | ClientBrowserRead::Location(_) => {}
        ClientBrowserRead::Storage { key, fallback, .. }
        | ClientBrowserRead::UrlSearchParam { key, fallback } => {
            collect_local_function_refs_from_expr(key, out);
            if let Some(fallback) = fallback {
                collect_local_function_refs_from_expr(fallback, out);
            }
        }
        ClientBrowserRead::Declared { args, .. } => {
            for arg in args {
                collect_local_function_refs_from_expr(arg, out);
            }
        }
    }
}

fn collect_local_function_refs_from_browser_effect(
    effect: &ClientBrowserEffect,
    out: &mut Vec<ClientFunctionId>,
) {
    match effect {
        ClientBrowserEffect::WindowScrollTo { x, y }
        | ClientBrowserEffect::WindowScrollBy { x, y } => {
            collect_local_function_refs_from_expr(x, out);
            collect_local_function_refs_from_expr(y, out);
        }
        ClientBrowserEffect::WindowPrint
        | ClientBrowserEffect::LocationReload
        | ClientBrowserEffect::HistoryBack
        | ClientBrowserEffect::HistoryForward => {}
        ClientBrowserEffect::LocationSetHash { value } => {
            collect_local_function_refs_from_expr(value, out);
        }
        ClientBrowserEffect::LocationAssign { url }
        | ClientBrowserEffect::LocationReplace { url }
        | ClientBrowserEffect::HistoryPushState { url }
        | ClientBrowserEffect::HistoryReplaceState { url } => {
            collect_local_function_refs_from_expr(url, out);
        }
        ClientBrowserEffect::HistoryGo { delta } => {
            collect_local_function_refs_from_expr(delta, out);
        }
        ClientBrowserEffect::DocumentDispatchEvent { name, detail } => {
            collect_local_function_refs_from_expr(name, out);
            if let Some(detail) = detail {
                collect_local_function_refs_from_expr(detail, out);
            }
        }
        ClientBrowserEffect::DocumentSetTitle { title } => {
            collect_local_function_refs_from_expr(title, out);
        }
        ClientBrowserEffect::ClipboardWriteText { text } => {
            collect_local_function_refs_from_expr(text, out);
        }
        ClientBrowserEffect::ClipboardReadTextInto { fallback, .. }
        | ClientBrowserEffect::EventFileTextInto { fallback, .. } => {
            if let Some(fallback) = fallback {
                collect_local_function_refs_from_expr(fallback, out);
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
            collect_local_function_refs_from_expr(options_json, out);
            if let Some(fallback) = fallback {
                collect_local_function_refs_from_expr(fallback, out);
            }
        }
        ClientBrowserEffect::StorageSetItem { key, value, .. } => {
            collect_local_function_refs_from_expr(key, out);
            collect_local_function_refs_from_expr(value, out);
        }
        ClientBrowserEffect::StorageRemoveItem { key, .. } => {
            collect_local_function_refs_from_expr(key, out);
        }
        ClientBrowserEffect::UrlSetSearchParam { key, value, .. } => {
            collect_local_function_refs_from_expr(key, out);
            collect_local_function_refs_from_expr(value, out);
        }
        ClientBrowserEffect::UrlDeleteSearchParam { key, .. } => {
            collect_local_function_refs_from_expr(key, out);
        }
        ClientBrowserEffect::Declared { args, .. } => {
            for arg in args {
                collect_local_function_refs_from_expr(arg, out);
            }
        }
    }
}

fn collect_local_function_refs_from_node_ref_effect(
    effect: &ClientNodeRefEffect,
    out: &mut Vec<ClientFunctionId>,
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
            collect_local_function_refs_from_expr(x, out);
            collect_local_function_refs_from_expr(y, out);
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
            collect_local_function_refs_from_expr(expr, out);
        }
        ClientNodeRefEffect::SetSelectionRange(start, end) => {
            collect_local_function_refs_from_expr(start, out);
            collect_local_function_refs_from_expr(end, out);
        }
        ClientNodeRefEffect::SetAttribute { value, .. }
        | ClientNodeRefEffect::SetStyleProperty { value, .. } => {
            collect_local_function_refs_from_expr(value, out);
        }
    }
}
