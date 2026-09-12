// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Statement types for the IR
//!
//! Statements represent actions with side effects. Unlike expressions,
//! statements don't produce values but instead modify state or trigger effects.

use crate::expr::Expr;
use crate::types::{
    ContentType, FieldId, Method, ResponseType, Selector, SignalId, ToastLevel, VarId,
};
use serde::Serialize;

/// A block of statements
pub type Block = Vec<Stmt>;

/// A statement (side effect)
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Stmt {
    // =========================================================================
    // Variables
    // =========================================================================
    /// Declare a local variable: `let x = expr`
    Let { var: VarId, value: Expr },

    // =========================================================================
    // State Updates
    // =========================================================================
    /// Set a signal's value: `signal = expr`
    SetSignal { id: SignalId, value: Expr },

    /// Set a form field's value: `field = expr`
    SetField { id: FieldId, value: Expr },

    /// Set a field's error message (null to clear)
    SetFieldError { id: FieldId, error: Expr },

    /// Mark a field as touched
    TouchField { id: FieldId },

    /// Reset a field to its initial value
    ResetField { id: FieldId },

    /// Reset entire form to initial values
    ResetForm,

    // =========================================================================
    // Control Flow
    // =========================================================================
    /// Conditional execution
    If {
        cond: Expr,
        then: Block,
        else_: Option<Block>,
    },

    /// Pattern matching
    Match { expr: Expr, arms: Vec<MatchArm> },

    /// Loop over array
    For {
        var: VarId,
        index_var: Option<VarId>,
        iter: Expr,
        body: Block,
    },

    /// While loop (careful with infinite loops!)
    While { cond: Expr, body: Block },

    /// Break out of loop
    Break,

    /// Continue to next iteration
    Continue,

    /// Return early from effect handler
    Return,

    // =========================================================================
    // Async Operations
    // =========================================================================
    /// Fetch data from URL
    Fetch {
        url: Expr,
        options: FetchOptions,
        on_success: Block,
        on_error: Block,
    },

    /// Delay execution
    Delay { ms: Expr, body: Block },

    /// Debounce execution (identified by key)
    Debounce {
        key: &'static str,
        ms: u32,
        body: Block,
    },

    /// Throttle execution (identified by key)
    Throttle {
        key: &'static str,
        ms: u32,
        body: Block,
    },

    // =========================================================================
    // DOM Operations
    // =========================================================================
    /// Show element(s)
    Show { selector: Selector },

    /// Hide element(s)
    Hide { selector: Selector },

    /// Toggle element visibility
    Toggle { selector: Selector },

    /// Set element text content
    SetText { selector: Selector, value: Expr },

    /// Set element innerHTML (use carefully!)
    SetHtml { selector: Selector, value: Expr },

    /// Set element attribute
    SetAttr {
        selector: Selector,
        attr: &'static str,
        value: Expr,
    },

    /// Remove element attribute
    RemoveAttr {
        selector: Selector,
        attr: &'static str,
    },

    /// Add CSS class
    AddClass { selector: Selector, class: Expr },

    /// Remove CSS class
    RemoveClass { selector: Selector, class: Expr },

    /// Toggle CSS class
    ToggleClass { selector: Selector, class: Expr },

    /// Set CSS style property
    SetStyle {
        selector: Selector,
        property: &'static str,
        value: Expr,
    },

    /// Set element property (like .value, .checked)
    SetProp {
        selector: Selector,
        prop: &'static str,
        value: Expr,
    },

    /// Focus element
    Focus { selector: Selector },

    /// Blur element
    Blur { selector: Selector },

    /// Scroll element into view
    ScrollTo {
        selector: Selector,
        behavior: ScrollBehavior,
    },

    /// Select text in input
    Select { selector: Selector },

    // =========================================================================
    // Navigation
    // =========================================================================
    /// Redirect to URL
    Redirect { url: Expr },

    /// Update URL without navigation (pushState)
    PushState { url: Expr, replace: bool },

    /// Go back in history
    HistoryBack,

    /// Go forward in history
    HistoryForward,

    // =========================================================================
    // Notifications
    // =========================================================================
    /// Show toast notification
    Toast {
        message: Expr,
        level: ToastLevel,
        duration_ms: Option<u32>,
    },

    /// Show browser alert (blocking - use sparingly)
    Alert { message: Expr },

    /// Show browser confirm dialog (blocking)
    Confirm {
        message: Expr,
        on_confirm: Block,
        on_cancel: Block,
    },

    // =========================================================================
    // Storage
    // =========================================================================
    /// Set localStorage item
    SetLocalStorage { key: &'static str, value: Expr },

    /// Remove localStorage item
    RemoveLocalStorage { key: &'static str },

    /// Set sessionStorage item
    SetSessionStorage { key: &'static str, value: Expr },

    /// Remove sessionStorage item
    RemoveSessionStorage { key: &'static str },

    // =========================================================================
    // URL Parameters
    // =========================================================================
    /// Set URL search parameter
    SetUrlParam { key: &'static str, value: Expr },

    /// Remove URL search parameter
    RemoveUrlParam { key: &'static str },

    // =========================================================================
    // Clipboard
    // =========================================================================
    /// Copy text to clipboard
    CopyToClipboard { value: Expr },

    // =========================================================================
    // Miscellaneous
    // =========================================================================
    /// Log to console (for debugging)
    Log { value: Expr, level: LogLevel },

    /// Dispatch custom event
    DispatchEvent {
        selector: Selector,
        event: &'static str,
        detail: Option<Expr>,
    },

    /// Prevent default event behavior (only in event handlers)
    PreventDefault,

    /// Stop event propagation (only in event handlers)
    StopPropagation,

    /// Submit the form
    SubmitForm,

    /// Download file
    Download { url: Expr, filename: Option<Expr> },

    /// Print page
    Print,
}

/// Options for fetch requests
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct FetchOptions {
    /// HTTP method
    #[serde(default)]
    pub method: Method,

    /// Request headers
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(&'static str, Expr)>,

    /// Request body
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Expr>,

    /// Content type
    #[serde(default)]
    pub content_type: ContentType,

    /// Expected response type
    #[serde(default)]
    pub response_type: ResponseType,

    /// Credentials mode
    #[serde(default)]
    pub credentials: Credentials,

    /// Timeout in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,

    /// Cache mode
    #[serde(default)]
    pub cache: CacheMode,

    /// Retry count on failure
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retries: u8,

    /// Retry delay in milliseconds
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub retry_delay_ms: u32,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

/// Credentials mode for fetch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Credentials {
    /// Same origin only
    #[default]
    SameOrigin,
    /// Always include credentials
    Include,
    /// Never include credentials
    Omit,
}

/// Cache mode for fetch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
    /// Normal caching
    #[default]
    Default,
    /// Bypass cache, but update it
    NoCache,
    /// Bypass cache completely
    NoStore,
    /// Only use cache
    ForceCache,
    /// Use stale cache if available
    OnlyIfCached,
}

/// Scroll behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollBehavior {
    #[default]
    Smooth,
    Instant,
    Auto,
}

/// Console log level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Log,
    Info,
    Warn,
    Error,
    Debug,
}

/// A match arm for pattern matching
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchArm {
    /// Pattern to match against
    pub pattern: Pattern,
    /// Body to execute if matched
    pub body: Block,
}

/// Pattern for match expressions
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Pattern {
    /// Match any value (wildcard)
    Wildcard,
    /// Match a literal value
    Literal { value: crate::value::Value },
    /// Match and bind to variable
    Binding { var: VarId },
    /// Match if value equals expression
    Equals { value: Expr },
}

// Convenience constructors
impl Stmt {
    pub fn set_signal(name: &'static str, value: impl Into<Expr>) -> Self {
        Self::SetSignal {
            id: SignalId::new(name),
            value: value.into(),
        }
    }

    pub fn set_field(name: &'static str, value: impl Into<Expr>) -> Self {
        Self::SetField {
            id: FieldId::new(name),
            value: value.into(),
        }
    }

    pub fn set_error(name: &'static str, error: impl Into<Expr>) -> Self {
        Self::SetFieldError {
            id: FieldId::new(name),
            error: error.into(),
        }
    }

    pub fn if_then(cond: impl Into<Expr>, then: Block) -> Self {
        Self::If {
            cond: cond.into(),
            then,
            else_: None,
        }
    }

    pub fn if_then_else(cond: impl Into<Expr>, then: Block, else_: Block) -> Self {
        Self::If {
            cond: cond.into(),
            then,
            else_: Some(else_),
        }
    }

    pub fn show(selector: &'static str) -> Self {
        Self::Show {
            selector: Selector::new(selector),
        }
    }

    pub fn hide(selector: &'static str) -> Self {
        Self::Hide {
            selector: Selector::new(selector),
        }
    }

    pub fn toast(message: impl Into<Expr>, level: ToastLevel) -> Self {
        Self::Toast {
            message: message.into(),
            level,
            duration_ms: None,
        }
    }

    pub fn redirect(url: impl Into<Expr>) -> Self {
        Self::Redirect { url: url.into() }
    }

    pub fn log(value: impl Into<Expr>) -> Self {
        Self::Log {
            value: value.into(),
            level: LogLevel::Log,
        }
    }
}
