// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Expression types for the IR
//!
//! Expressions represent computations that produce values. They are pure
//! (no side effects) and can be composed to build complex logic.

use crate::types::{FieldId, IrStr, SignalId, VarId};
use crate::value::Value;
use serde::{Deserialize, Serialize};

/// An expression that evaluates to a value
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Expr {
    // =========================================================================
    // Primitives
    // =========================================================================
    /// A literal value
    Lit { value: Value },

    /// Read a signal's current value
    Signal { id: SignalId },

    /// Read a form field's current value
    Field { id: FieldId },

    /// Read a local variable
    Var { id: VarId },

    /// Read a value from the page render scope (the server data scope).
    ///
    /// This is the data root for server-evaluated expressions: `path` walks
    /// nested objects from the scope root, and the result composes with
    /// `Prop`/`Index` like any other expression. Missing path segments
    /// resolve to `Null` (null-propagating, like `Prop`).
    Scope { path: Vec<IrStr> },

    /// The response from a fetch (only valid in on_success block)
    Response,

    /// The error from a fetch (only valid in on_error block)
    Error,

    /// The event that triggered the current handler
    Event,

    /// The element that triggered the current handler
    Target,

    // =========================================================================
    // Property Access
    // =========================================================================
    /// Access a property on an object: `expr.prop`
    Prop { expr: Box<Expr>, prop: IrStr },

    /// Access an index on an array: `expr[index]`
    Index { expr: Box<Expr>, index: Box<Expr> },

    // =========================================================================
    // Arithmetic
    // =========================================================================
    /// Addition: `a + b`
    Add { left: Box<Expr>, right: Box<Expr> },

    /// Subtraction: `a - b`
    Sub { left: Box<Expr>, right: Box<Expr> },

    /// Multiplication: `a * b`
    Mul { left: Box<Expr>, right: Box<Expr> },

    /// Division: `a / b`
    Div { left: Box<Expr>, right: Box<Expr> },

    /// Modulo: `a % b`
    Mod { left: Box<Expr>, right: Box<Expr> },

    /// Negation: `-a`
    Neg { expr: Box<Expr> },

    // =========================================================================
    // Comparison
    // =========================================================================
    /// Equality: `a == b`
    Eq { left: Box<Expr>, right: Box<Expr> },

    /// Inequality: `a != b`
    Ne { left: Box<Expr>, right: Box<Expr> },

    /// Less than: `a < b`
    Lt { left: Box<Expr>, right: Box<Expr> },

    /// Less than or equal: `a <= b`
    Le { left: Box<Expr>, right: Box<Expr> },

    /// Greater than: `a > b`
    Gt { left: Box<Expr>, right: Box<Expr> },

    /// Greater than or equal: `a >= b`
    Ge { left: Box<Expr>, right: Box<Expr> },

    // =========================================================================
    // Logical
    // =========================================================================
    /// Logical AND: `a && b`
    And { left: Box<Expr>, right: Box<Expr> },

    /// Logical OR: `a || b`
    Or { left: Box<Expr>, right: Box<Expr> },

    /// Logical NOT: `!a`
    Not { expr: Box<Expr> },

    // =========================================================================
    // Conditional
    // =========================================================================
    /// Ternary: `cond ? then : else`
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },

    /// Null coalescing: `a ?? b`
    Coalesce { left: Box<Expr>, right: Box<Expr> },

    // =========================================================================
    // String Operations
    // =========================================================================
    /// Concatenate strings/values into a string
    Concat { parts: Vec<Expr> },

    /// Get string length
    Len { expr: Box<Expr> },

    /// Trim whitespace
    Trim { expr: Box<Expr> },

    /// Convert to lowercase
    Lower { expr: Box<Expr> },

    /// Convert to uppercase
    Upper { expr: Box<Expr> },

    /// Check if string contains substring
    Contains {
        haystack: Box<Expr>,
        needle: Box<Expr>,
    },

    /// Check if string starts with prefix
    StartsWith {
        string: Box<Expr>,
        prefix: Box<Expr>,
    },

    /// Check if string ends with suffix
    EndsWith {
        string: Box<Expr>,
        suffix: Box<Expr>,
    },

    /// Extract substring
    Substring {
        string: Box<Expr>,
        start: Box<Expr>,
        end: Option<Box<Expr>>,
    },

    /// Replace occurrences in string
    Replace {
        string: Box<Expr>,
        from: Box<Expr>,
        to: Box<Expr>,
    },

    /// Split string into array
    Split {
        string: Box<Expr>,
        delimiter: Box<Expr>,
    },

    /// Join array into string
    Join {
        array: Box<Expr>,
        delimiter: Box<Expr>,
    },

    /// Match regex pattern
    Matches { string: Box<Expr>, pattern: IrStr },

    // =========================================================================
    // Array/Object Construction
    // =========================================================================
    /// Create an array
    Array { items: Vec<Expr> },

    /// Create an object
    Object { entries: Vec<(IrStr, Expr)> },

    // =========================================================================
    // Array Operations
    // =========================================================================
    /// Map over array: `arr.map(x => expr)`
    Map {
        array: Box<Expr>,
        var: VarId,
        body: Box<Expr>,
    },

    /// Filter array: `arr.filter(x => predicate)`
    Filter {
        array: Box<Expr>,
        var: VarId,
        predicate: Box<Expr>,
    },

    /// Find in array: `arr.find(x => predicate)`
    Find {
        array: Box<Expr>,
        var: VarId,
        predicate: Box<Expr>,
    },

    /// Find index in array: `arr.findIndex(x => predicate)`
    FindIndex {
        array: Box<Expr>,
        var: VarId,
        predicate: Box<Expr>,
    },

    /// Check if any element matches: `arr.some(x => predicate)`
    Some {
        array: Box<Expr>,
        var: VarId,
        predicate: Box<Expr>,
    },

    /// Check if all elements match: `arr.every(x => predicate)`
    Every {
        array: Box<Expr>,
        var: VarId,
        predicate: Box<Expr>,
    },

    /// Reduce array: `arr.reduce((acc, x) => expr, initial)`
    Reduce {
        array: Box<Expr>,
        acc_var: VarId,
        item_var: VarId,
        body: Box<Expr>,
        initial: Box<Expr>,
    },

    /// Check if array includes value
    Includes { array: Box<Expr>, value: Box<Expr> },

    /// Get first element
    First { array: Box<Expr> },

    /// Get last element
    Last { array: Box<Expr> },

    /// Slice array
    Slice {
        array: Box<Expr>,
        start: Box<Expr>,
        end: Option<Box<Expr>>,
    },

    /// Flatten nested arrays
    Flat { array: Box<Expr>, depth: Option<u8> },

    // =========================================================================
    // Type Checking / Conversion
    // =========================================================================
    /// Check if value is null/undefined
    IsNull { expr: Box<Expr> },

    /// Check if value is a number
    IsNumber { expr: Box<Expr> },

    /// Check if value is a string
    IsString { expr: Box<Expr> },

    /// Check if value is an array
    IsArray { expr: Box<Expr> },

    /// Parse string as integer
    ParseInt { expr: Box<Expr> },

    /// Parse string as float
    ParseFloat { expr: Box<Expr> },

    /// Convert to string
    ToString { expr: Box<Expr> },

    /// Convert to number
    ToNumber { expr: Box<Expr> },

    // =========================================================================
    // Math
    // =========================================================================
    /// Absolute value
    Abs { expr: Box<Expr> },

    /// Round to nearest integer
    Round { expr: Box<Expr> },

    /// Round down
    Floor { expr: Box<Expr> },

    /// Round up
    Ceil { expr: Box<Expr> },

    /// Minimum of values
    Min { values: Vec<Expr> },

    /// Maximum of values
    Max { values: Vec<Expr> },

    /// Clamp value between min and max
    Clamp {
        value: Box<Expr>,
        min: Box<Expr>,
        max: Box<Expr>,
    },

    // =========================================================================
    // Number Formatting
    // =========================================================================
    /// Format a number with a fixed number of decimal places, following
    /// JavaScript `Number.prototype.toFixed` semantics (see `SEMANTICS.md`).
    ///
    /// The workhorse for money strings:
    /// `Concat["$", FormatNumber(total_cents / 100, 2)]`.
    FormatNumber { value: Box<Expr>, decimals: u8 },

    // =========================================================================
    // Date/Time
    // =========================================================================
    /// Current timestamp (milliseconds)
    Now,

    /// Format date
    FormatDate { date: Box<Expr>, format: IrStr },

    /// Parse date string
    ParseDate { string: Box<Expr>, format: IrStr },

    // =========================================================================
    // Form-specific
    // =========================================================================
    /// Check if a field has an error
    FieldHasError { id: FieldId },

    /// Get a field's error message
    FieldError { id: FieldId },

    /// Check if a field has been touched (blurred at least once)
    FieldTouched { id: FieldId },

    /// Check if a field is dirty (value changed from initial)
    FieldDirty { id: FieldId },

    /// Check if entire form is valid
    FormValid,

    /// Check if any field is dirty
    FormDirty,

    /// Get form data as object
    FormData,

    // =========================================================================
    // Validation Helpers
    // =========================================================================
    /// Validate email format
    IsEmail { expr: Box<Expr> },

    /// Validate URL format
    IsUrl { expr: Box<Expr> },

    /// Validate matches regex pattern
    MatchesPattern { expr: Box<Expr>, pattern: IrStr },

    // =========================================================================
    // Payment Helpers
    // =========================================================================
    /// Detect credit card type from number (returns: visa, mastercard, amex, discover, diners, jcb, unknown)
    CardType { expr: Box<Expr> },
}

impl Expr {
    /// Visit every direct child expression of this node, in field order.
    ///
    /// The match is exhaustive with no wildcard arm, so adding an `Expr`
    /// variant is a compile error here until the walker learns its
    /// children. Consumers that need the whole subtree drive this with an
    /// explicit worklist, which keeps the walk allocation-bounded and immune
    /// to stack overflow on hostile deeply nested expressions.
    pub fn for_each_child<'e>(&'e self, mut f: impl FnMut(&'e Expr)) {
        self.for_each_child_dyn(&mut f)
    }

    fn for_each_child_dyn<'e>(&'e self, f: &mut dyn FnMut(&'e Expr)) {
        use Expr::*;
        match self {
            // Leaves: no expression children.
            Lit { .. }
            | Signal { .. }
            | Field { .. }
            | Var { .. }
            | Scope { .. }
            | Response
            | Error
            | Event
            | Target
            | Now
            | FieldHasError { .. }
            | FieldError { .. }
            | FieldTouched { .. }
            | FieldDirty { .. }
            | FormValid
            | FormDirty
            | FormData => {}

            // One child.
            Prop { expr, .. }
            | Neg { expr }
            | Not { expr }
            | Len { expr }
            | Trim { expr }
            | Lower { expr }
            | Upper { expr }
            | IsNull { expr }
            | IsNumber { expr }
            | IsString { expr }
            | IsArray { expr }
            | ParseInt { expr }
            | ParseFloat { expr }
            | ToString { expr }
            | ToNumber { expr }
            | Abs { expr }
            | Round { expr }
            | Floor { expr }
            | Ceil { expr }
            | IsEmail { expr }
            | IsUrl { expr }
            | MatchesPattern { expr, .. }
            | CardType { expr } => f(expr),
            Matches { string, .. } => f(string),
            FormatNumber { value, .. } => f(value),
            FormatDate { date, .. } => f(date),
            ParseDate { string, .. } => f(string),
            First { array } | Last { array } | Flat { array, .. } => f(array),

            // Two children.
            Index { expr, index } => {
                f(expr);
                f(index);
            }
            Add { left, right }
            | Sub { left, right }
            | Mul { left, right }
            | Div { left, right }
            | Mod { left, right }
            | Eq { left, right }
            | Ne { left, right }
            | Lt { left, right }
            | Le { left, right }
            | Gt { left, right }
            | Ge { left, right }
            | And { left, right }
            | Or { left, right }
            | Coalesce { left, right } => {
                f(left);
                f(right);
            }
            Contains { haystack, needle } => {
                f(haystack);
                f(needle);
            }
            StartsWith { string, prefix } => {
                f(string);
                f(prefix);
            }
            EndsWith { string, suffix } => {
                f(string);
                f(suffix);
            }
            Split { string, delimiter } => {
                f(string);
                f(delimiter);
            }
            Join { array, delimiter } => {
                f(array);
                f(delimiter);
            }
            Includes { array, value } => {
                f(array);
                f(value);
            }
            Map { array, body, .. } => {
                f(array);
                f(body);
            }
            Filter {
                array, predicate, ..
            }
            | Find {
                array, predicate, ..
            }
            | FindIndex {
                array, predicate, ..
            }
            | Some {
                array, predicate, ..
            }
            | Every {
                array, predicate, ..
            } => {
                f(array);
                f(predicate);
            }

            // Two or three children.
            Substring { string, start, end } => {
                f(string);
                f(start);
                if let Option::Some(end) = end {
                    f(end);
                }
            }
            Slice { array, start, end } => {
                f(array);
                f(start);
                if let Option::Some(end) = end {
                    f(end);
                }
            }

            // Three children.
            Ternary { cond, then, else_ } => {
                f(cond);
                f(then);
                f(else_);
            }
            Replace { string, from, to } => {
                f(string);
                f(from);
                f(to);
            }
            Clamp { value, min, max } => {
                f(value);
                f(min);
                f(max);
            }
            Reduce {
                array,
                body,
                initial,
                ..
            } => {
                f(array);
                f(body);
                f(initial);
            }

            // Variadic.
            Concat { parts } => parts.iter().for_each(f),
            Array { items } => items.iter().for_each(f),
            Min { values } | Max { values } => values.iter().for_each(f),
            Object { entries } => {
                for (_, value) in entries {
                    f(value);
                }
            }
        }
    }
}

// Convenience constructors
#[allow(clippy::should_implement_trait)]
impl Expr {
    pub fn lit(value: impl Into<Value>) -> Self {
        Self::Lit {
            value: value.into(),
        }
    }

    pub fn signal(name: &'static str) -> Self {
        Self::Signal {
            id: SignalId::new(name),
        }
    }

    pub fn field(name: &'static str) -> Self {
        Self::Field {
            id: FieldId::new(name),
        }
    }

    pub fn var(name: &'static str) -> Self {
        Self::Var {
            id: VarId::new(name),
        }
    }

    /// Read a top-level binding from the page render scope.
    pub fn scope(name: impl Into<IrStr>) -> Self {
        Self::Scope {
            path: vec![name.into()],
        }
    }

    /// Read a nested path from the page render scope.
    pub fn scope_path<I, S>(path: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<IrStr>,
    {
        Self::Scope {
            path: path.into_iter().map(Into::into).collect(),
        }
    }

    pub fn response() -> Self {
        Self::Response
    }

    pub fn error() -> Self {
        Self::Error
    }

    pub fn prop(self, name: impl Into<IrStr>) -> Self {
        Self::Prop {
            expr: Box::new(self),
            prop: name.into(),
        }
    }

    pub fn index(self, idx: impl Into<Expr>) -> Self {
        Self::Index {
            expr: Box::new(self),
            index: Box::new(idx.into()),
        }
    }

    // Arithmetic
    pub fn add(self, other: impl Into<Expr>) -> Self {
        Self::Add {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn sub(self, other: impl Into<Expr>) -> Self {
        Self::Sub {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn mul(self, other: impl Into<Expr>) -> Self {
        Self::Mul {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn div(self, other: impl Into<Expr>) -> Self {
        Self::Div {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    // Comparison
    pub fn eq(self, other: impl Into<Expr>) -> Self {
        Self::Eq {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn ne(self, other: impl Into<Expr>) -> Self {
        Self::Ne {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn lt(self, other: impl Into<Expr>) -> Self {
        Self::Lt {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn le(self, other: impl Into<Expr>) -> Self {
        Self::Le {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn gt(self, other: impl Into<Expr>) -> Self {
        Self::Gt {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn ge(self, other: impl Into<Expr>) -> Self {
        Self::Ge {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    // Logical
    pub fn and(self, other: impl Into<Expr>) -> Self {
        Self::And {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn or(self, other: impl Into<Expr>) -> Self {
        Self::Or {
            left: Box::new(self),
            right: Box::new(other.into()),
        }
    }

    pub fn not(self) -> Self {
        Self::Not {
            expr: Box::new(self),
        }
    }

    // String
    pub fn len(self) -> Self {
        Self::Len {
            expr: Box::new(self),
        }
    }

    pub fn trim(self) -> Self {
        Self::Trim {
            expr: Box::new(self),
        }
    }

    pub fn lower(self) -> Self {
        Self::Lower {
            expr: Box::new(self),
        }
    }

    pub fn upper(self) -> Self {
        Self::Upper {
            expr: Box::new(self),
        }
    }

    pub fn contains(self, needle: impl Into<Expr>) -> Self {
        Self::Contains {
            haystack: Box::new(self),
            needle: Box::new(needle.into()),
        }
    }

    pub fn is_null(self) -> Self {
        Self::IsNull {
            expr: Box::new(self),
        }
    }

    /// Format this number with a fixed number of decimals
    /// (JS `toFixed` semantics).
    pub fn format_number(self, decimals: u8) -> Self {
        Self::FormatNumber {
            value: Box::new(self),
            decimals,
        }
    }

    /// Detect credit card type from a card number expression
    pub fn card_type(expr: impl Into<Expr>) -> Self {
        Self::CardType {
            expr: Box::new(expr.into()),
        }
    }
}

// Allow literals to be converted to Expr
impl From<bool> for Expr {
    fn from(b: bool) -> Self {
        Self::lit(b)
    }
}

impl From<i32> for Expr {
    fn from(n: i32) -> Self {
        Self::lit(n)
    }
}

impl From<i64> for Expr {
    fn from(n: i64) -> Self {
        Self::lit(n)
    }
}

impl From<f64> for Expr {
    fn from(n: f64) -> Self {
        Self::lit(n)
    }
}

impl From<&'static str> for Expr {
    fn from(s: &'static str) -> Self {
        Self::lit(s)
    }
}

impl From<String> for Expr {
    fn from(s: String) -> Self {
        Self::lit(s)
    }
}
