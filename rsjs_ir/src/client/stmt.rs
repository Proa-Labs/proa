// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Statements and blocks for Client IR v3.

use crate::reactive::{ConsoleLevel, EventFileSource, StorageArea, UrlHistoryMode};
use crate::types::{CallbackSlotId, MutationId, NodeRefId, QueryId, SignalId};

use super::expr::{ClientExpr, ClientLocalId};
use super::types::{ClientType, IntegerDomain};

/// Assignable location in Client IR.
///
/// The recursive shape supports nested own-property records while remaining
/// deliberately smaller than arbitrary JavaScript lvalues.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientPlace {
    Local(ClientLocalId),
    Field {
        base: Box<ClientPlace>,
        name: String,
    },
}

impl ClientPlace {
    pub fn local(local: ClientLocalId) -> Self {
        Self::Local(local)
    }

    pub fn field(base: ClientPlace, name: impl Into<String>) -> Self {
        Self::Field {
            base: Box::new(base),
            name: name.into(),
        }
    }
}

/// One-argument closure used by Client IR v3 signal updates.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientSimpleClosure {
    pub param: String,
    pub body: Box<ClientExpr>,
}

impl ClientSimpleClosure {
    pub fn new(param: impl Into<String>, body: ClientExpr) -> Self {
        Self {
            param: param.into(),
            body: Box::new(body),
        }
    }
}

/// Browser side-effects supported by Client IR v3.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientBrowserEffect {
    WindowScrollTo {
        x: ClientExpr,
        y: ClientExpr,
    },
    WindowScrollBy {
        x: ClientExpr,
        y: ClientExpr,
    },
    WindowPrint,
    LocationSetHash {
        value: ClientExpr,
    },
    LocationAssign {
        url: ClientExpr,
    },
    LocationReplace {
        url: ClientExpr,
    },
    LocationReload,
    HistoryPushState {
        url: ClientExpr,
    },
    HistoryReplaceState {
        url: ClientExpr,
    },
    HistoryBack,
    HistoryForward,
    HistoryGo {
        delta: ClientExpr,
    },
    DocumentDispatchEvent {
        name: ClientExpr,
        detail: Option<ClientExpr>,
    },
    DocumentSetTitle {
        title: ClientExpr,
    },
    ClipboardWriteText {
        text: ClientExpr,
    },
    ClipboardReadTextInto {
        target: SignalId,
        fallback: Option<ClientExpr>,
    },
    WebAuthnCreateJsonInto {
        options_json: ClientExpr,
        target: SignalId,
        fallback: Option<ClientExpr>,
    },
    WebAuthnGetJsonInto {
        options_json: ClientExpr,
        target: SignalId,
        fallback: Option<ClientExpr>,
    },
    EventFileTextInto {
        source: EventFileSource,
        target: SignalId,
        fallback: Option<ClientExpr>,
    },
    StorageSetItem {
        area: StorageArea,
        key: ClientExpr,
        value: ClientExpr,
    },
    StorageRemoveItem {
        area: StorageArea,
        key: ClientExpr,
    },
    UrlSetSearchParam {
        mode: UrlHistoryMode,
        key: ClientExpr,
        value: ClientExpr,
    },
    UrlDeleteSearchParam {
        mode: UrlHistoryMode,
        key: ClientExpr,
    },
    /// A registry-declared browser effect.
    ///
    /// The statement-side mirror of
    /// [`crate::client::ClientBrowserRead::Declared`]: `helper` is the
    /// stable [`crate::browser_registry`] id (`<module>.<method>`) and
    /// `args` are the lowered reactive arguments in source order. The
    /// JS emitter resolves the id back through the registry entry's JS
    /// template, so a new simple effect needs no dedicated IR variant,
    /// JSON arm, or emitter arm.
    Declared {
        helper: String,
        args: Vec<ClientExpr>,
    },
}

/// Imperative DOM operations through a bound node ref.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientNodeRefEffect {
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
    ScrollTo { x: ClientExpr, y: ClientExpr },
    ScrollBy { x: ClientExpr, y: ClientExpr },
    SetPointerCapture(ClientExpr),
    ReleasePointerCapture(ClientExpr),
    Select,
    SetValue(ClientExpr),
    SetChecked(ClientExpr),
    SetCustomValidity(ClientExpr),
    SetSelectionRange(ClientExpr, ClientExpr),
    SetTextContent(ClientExpr),
    AddClass(ClientExpr),
    RemoveClass(ClientExpr),
    ToggleClass(ClientExpr),
    SetAttribute { name: String, value: ClientExpr },
    RemoveAttribute { name: String },
    SetStyleProperty { name: String, value: ClientExpr },
}

/// Statement node in Client IR v3.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ClientStmt {
    Expr(ClientExpr),
    Let {
        local: ClientLocalId,
        name: String,
        ty: ClientType,
        value: Option<ClientExpr>,
    },
    Assign {
        local: ClientLocalId,
        value: ClientExpr,
    },
    /// Assign to a local or one of its nested own-property fields.
    AssignPlace {
        place: ClientPlace,
        value: ClientExpr,
    },
    SignalSet {
        id: SignalId,
        value: ClientExpr,
    },
    SignalUpdate {
        id: SignalId,
        closure: ClientSimpleClosure,
    },
    EventPreventDefault,
    EventStopPropagation,
    Console {
        level: ConsoleLevel,
        args: Vec<ClientExpr>,
    },
    CallbackInvoke {
        callback: CallbackSlotId,
        payload: ClientExpr,
    },
    RunMutation {
        mutation: MutationId,
        args: Vec<ClientExpr>,
    },
    QueryRefetch {
        query: QueryId,
    },
    QueryInvalidate {
        query: QueryId,
    },
    BrowserEffect(ClientBrowserEffect),
    NodeRefEffect {
        id: NodeRefId,
        effect: ClientNodeRefEffect,
    },
    Return(Option<ClientExpr>),
    Block(ClientBlock),
    If {
        condition: ClientExpr,
        then_block: ClientBlock,
        else_block: Option<ClientBlock>,
    },
    ForRange {
        local: ClientLocalId,
        name: String,
        /// Exact Rust fixed-width domain proved for both bounds and the
        /// induction variable before lowering erased source types.
        integer_domain: IntegerDomain,
        start: ClientExpr,
        end: ClientExpr,
        inclusive: bool,
        body: ClientBlock,
    },
}

impl ClientStmt {
    pub fn expr(expr: ClientExpr) -> Self {
        Self::Expr(expr)
    }

    pub fn let_(local: ClientLocalId, name: impl Into<String>, ty: ClientType) -> Self {
        Self::Let {
            local,
            name: name.into(),
            ty,
            value: None,
        }
    }

    pub fn let_value(
        local: ClientLocalId,
        name: impl Into<String>,
        ty: ClientType,
        value: ClientExpr,
    ) -> Self {
        Self::Let {
            local,
            name: name.into(),
            ty,
            value: Some(value),
        }
    }

    pub fn signal_set(id: SignalId, value: ClientExpr) -> Self {
        Self::SignalSet { id, value }
    }

    pub fn signal_update(id: SignalId, closure: ClientSimpleClosure) -> Self {
        Self::SignalUpdate { id, closure }
    }

    pub fn if_(condition: ClientExpr, then_block: ClientBlock) -> Self {
        Self::If {
            condition,
            then_block,
            else_block: None,
        }
    }

    pub fn if_else(
        condition: ClientExpr,
        then_block: ClientBlock,
        else_block: ClientBlock,
    ) -> Self {
        Self::If {
            condition,
            then_block,
            else_block: Some(else_block),
        }
    }

    pub fn for_range(
        local: ClientLocalId,
        name: impl Into<String>,
        integer_domain: IntegerDomain,
        start: ClientExpr,
        end: ClientExpr,
        inclusive: bool,
        body: ClientBlock,
    ) -> Self {
        Self::ForRange {
            local,
            name: name.into(),
            integer_domain,
            start,
            end,
            inclusive,
            body,
        }
    }
}

/// Ordered statement block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientBlock {
    pub stmts: Vec<ClientStmt>,
}

impl ClientBlock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_stmts(stmts: impl Into<Vec<ClientStmt>>) -> Self {
        Self {
            stmts: stmts.into(),
        }
    }

    pub fn push(&mut self, stmt: ClientStmt) {
        self.stmts.push(stmt);
    }

    pub fn is_empty(&self) -> bool {
        self.stmts.is_empty()
    }
}

impl From<Vec<ClientStmt>> for ClientBlock {
    fn from(stmts: Vec<ClientStmt>) -> Self {
        Self::from_stmts(stmts)
    }
}
