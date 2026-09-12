// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Pure server-side evaluation of IR expressions.
//!
//! `eval` interprets the server dialect of [`Expr`] against a
//! [`ScopeStack`] (the page render scope plus loop-local bindings) and
//! produces a [`Value`]. The same IR compiles to JavaScript for the client
//! (see `codegen`), so evaluation semantics are pinned to the JS emitters:
//! JS-number (f64) arithmetic and coercion, strict `===` comparison,
//! UTF-16 string lengths, and JS method semantics for string/array ops.
//! `SEMANTICS.md` at the crate root documents every decision and every
//! deliberate divergence.
//!
//! Client-only expressions (`Signal`, `Field`, `Event`, form/validation/
//! payment helpers) fail with [`EvalError::ClientOnly`]; run
//! [`crate::dialect::validate_server_dialect`] at compile-to-program time
//! to reject them up front.
//!
//! Evaluation is budgeted: expression depth and combinator iterations are
//! bounded (defaults via [`EvalBudget::default`]), so pathological inputs
//! fail with an error instead of hanging or overflowing the stack.

mod date;
mod jsnum;
mod tofixed;

use crate::expr::Expr;
use crate::types::IrStr;
use crate::value::Value;
use jsnum::{
    display, js_max2, js_min2, js_round, number_to_string, parse_float, parse_int_10, to_number,
    trim_js, utf16_at, utf16_len, utf16_lt, utf16_slice,
};
use std::borrow::Cow;

/// The data environment an expression is evaluated against: a root scope
/// value (the template view model, addressed by [`Expr::Scope`]) plus a stack
/// of named bindings (addressed by [`Expr::Var`], pushed by an interpreter
/// for `ForEach` loops and similar).
#[derive(Debug)]
pub struct ScopeStack<'a> {
    root: &'a Value,
    vars: Vec<(IrStr, Value)>,
}

impl<'a> ScopeStack<'a> {
    /// Create a scope over a root value (typically a `Value::Object`).
    pub fn new(root: &'a Value) -> Self {
        Self {
            root,
            vars: Vec::new(),
        }
    }

    /// The root scope value.
    pub fn root(&self) -> &Value {
        self.root
    }

    /// Push a named binding (innermost wins on lookup).
    pub fn push_var(&mut self, name: impl Into<IrStr>, value: Value) {
        self.vars.push((name.into(), value));
    }

    /// Pop the most recent binding.
    pub fn pop_var(&mut self) -> Option<(IrStr, Value)> {
        self.vars.pop()
    }

    fn var(&self, name: &str) -> Option<&Value> {
        self.vars
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
}

/// Limits threaded through evaluation. A host may layer its own outer
/// budgets on top; the evaluator already refuses pathological inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalBudget {
    /// Maximum expression nesting depth.
    pub max_depth: usize,
    /// Maximum total combinator element visits (Map/Filter/Reduce/...).
    pub max_iterations: u64,
}

/// Default depth limit ([`EvalBudget::default`]).
pub const DEFAULT_MAX_DEPTH: usize = 64;
/// Default combinator iteration limit ([`EvalBudget::default`]).
pub const DEFAULT_MAX_ITERATIONS: u64 = 10_000;

impl Default for EvalBudget {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }
}

/// Evaluation failure. Evaluation never panics; malformed or over-budget
/// input surfaces here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    #[error("expression depth limit of {limit} exceeded")]
    DepthLimit { limit: usize },

    #[error("combinator iteration budget of {limit} exceeded")]
    IterationLimit { limit: u64 },

    #[error("unknown variable `{name}`")]
    UnknownVar { name: String },

    #[error("type mismatch in {op}: expected {expected}, got {got}")]
    TypeMismatch {
        op: &'static str,
        expected: &'static str,
        got: &'static str,
    },

    #[error(
        "cannot compare arrays or objects with {op}: JS `===` uses reference \
         identity, which server values do not have"
    )]
    CompositeComparison { op: &'static str },

    #[error("`{variant}` is a client-only expression and cannot be evaluated on the server")]
    ClientOnly { variant: &'static str },

    #[error("`{variant}` is not supported by the server evaluator")]
    Unsupported { variant: &'static str },

    #[error("invalid regex `{pattern}`: {message}")]
    InvalidRegex { pattern: String, message: String },

    #[error("FormatNumber decimals must be 0..=100, got {decimals}")]
    InvalidDecimals { decimals: u8 },
}

/// Evaluate an expression against a scope with the default budget.
pub fn eval(expr: &Expr, scope: &ScopeStack<'_>) -> Result<Value, EvalError> {
    eval_with_budget(expr, scope, &EvalBudget::default())
}

/// Evaluate an expression against a scope with an explicit budget.
pub fn eval_with_budget(
    expr: &Expr,
    scope: &ScopeStack<'_>,
    budget: &EvalBudget,
) -> Result<Value, EvalError> {
    let mut cx = Cx {
        scope,
        budget,
        locals: Vec::new(),
        depth: 0,
        iterations: 0,
    };
    cx.eval(expr)
}

struct Cx<'a, 'e> {
    scope: &'e ScopeStack<'a>,
    budget: &'e EvalBudget,
    /// Combinator bindings, borrowed from the expression tree.
    locals: Vec<(&'e str, Value)>,
    depth: usize,
    iterations: u64,
}

/// JS `typeof`-flavored name for error messages.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Int(_) | Value::Float(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn as_bool(value: &Value, op: &'static str) -> Result<bool, EvalError> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(EvalError::TypeMismatch {
            op,
            expected: "boolean",
            got: kind(other),
        }),
    }
}

fn as_str<'v>(value: &'v Value, op: &'static str) -> Result<&'v str, EvalError> {
    match value {
        Value::String(s) => Ok(s),
        other => Err(EvalError::TypeMismatch {
            op,
            expected: "string",
            got: kind(other),
        }),
    }
}

fn as_array<'v>(value: &'v Value, op: &'static str) -> Result<&'v [Value], EvalError> {
    match value {
        Value::Array(items) => Ok(items),
        other => Err(EvalError::TypeMismatch {
            op,
            expected: "array",
            got: kind(other),
        }),
    }
}

/// A JS primitive after `ToPrimitive` (arrays/objects become strings).
enum Prim<'v> {
    Null,
    Bool(bool),
    Num(f64),
    Str(Cow<'v, str>),
}

fn to_primitive(value: &Value) -> Prim<'_> {
    match value {
        Value::Null => Prim::Null,
        Value::Bool(b) => Prim::Bool(*b),
        Value::Int(i) => Prim::Num(*i as f64),
        Value::Float(f) => Prim::Num(*f),
        Value::String(s) => Prim::Str(Cow::Borrowed(s)),
        Value::Array(_) | Value::Object(_) => Prim::Str(Cow::Owned(display(value))),
    }
}

fn prim_to_number(prim: &Prim<'_>) -> f64 {
    match prim {
        Prim::Null => 0.0,
        Prim::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Prim::Num(n) => *n,
        Prim::Str(s) => jsnum::str_to_number(s),
    }
}

fn prim_to_string(prim: &Prim<'_>) -> String {
    match prim {
        Prim::Null => "null".to_string(),
        Prim::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Prim::Num(n) => number_to_string(*n),
        Prim::Str(s) => s.to_string(),
    }
}

/// JS `+`: string concatenation if either primitive is a string, numeric
/// addition otherwise.
fn js_add(left: &Value, right: &Value) -> Value {
    let (l, r) = (to_primitive(left), to_primitive(right));
    if matches!(l, Prim::Str(_)) || matches!(r, Prim::Str(_)) {
        let mut s = prim_to_string(&l);
        s.push_str(&prim_to_string(&r));
        Value::String(Cow::Owned(s))
    } else {
        Value::Float(prim_to_number(&l) + prim_to_number(&r))
    }
}

/// JS abstract relational comparison `left < right`.
/// `None` means NaN was involved (every comparison yields false).
fn js_lt(left: &Value, right: &Value) -> Option<bool> {
    let (l, r) = (to_primitive(left), to_primitive(right));
    if let (Prim::Str(a), Prim::Str(b)) = (&l, &r) {
        return Some(utf16_lt(a, b));
    }
    let (a, b) = (prim_to_number(&l), prim_to_number(&r));
    if a.is_nan() || b.is_nan() {
        None
    } else {
        Some(a < b)
    }
}

/// JS strict equality (`===`) on primitives. Arrays/objects error because
/// JS compares them by reference identity, which server values cannot
/// reproduce (see SEMANTICS.md).
fn strict_eq(left: &Value, right: &Value, op: &'static str) -> Result<bool, EvalError> {
    use Value::*;
    if matches!(left, Array(_) | Object(_)) || matches!(right, Array(_) | Object(_)) {
        return Err(EvalError::CompositeComparison { op });
    }
    Ok(match (left, right) {
        (Null, Null) => true,
        (Bool(a), Bool(b)) => a == b,
        (Int(a), Int(b)) => *a as f64 == *b as f64,
        (Int(a), Float(b)) | (Float(b), Int(a)) => *a as f64 == *b,
        (Float(a), Float(b)) => a == b, // NaN !== NaN
        (String(a), String(b)) => a == b,
        _ => false,
    })
}

/// JS SameValueZero (used by `includes`): like `===` but NaN matches NaN,
/// and composite values never match (distinct references in JS).
fn same_value_zero(left: &Value, right: &Value) -> bool {
    use Value::*;
    let num = |v: &Value| match v {
        Int(i) => Some(*i as f64),
        Float(f) => Some(*f),
        _ => None,
    };
    match (num(left), num(right)) {
        (Some(a), Some(b)) => return a == b || (a.is_nan() && b.is_nan()),
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    match (left, right) {
        (Null, Null) => true,
        (Bool(a), Bool(b)) => a == b,
        (String(a), String(b)) => a == b,
        _ => false,
    }
}

/// Is `key` a canonical array index ("0", "17", but not "01" or "-1")?
fn canonical_index(key: &str) -> Option<usize> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse().ok()
}

/// Property access with server-dialect null propagation: `null` bases and
/// missing properties yield `Null` (JS would throw on null; see
/// SEMANTICS.md). Duplicate object keys resolve to the last entry, like a
/// JS object literal.
fn prop_access(base: &Value, key: &str) -> Value {
    match base {
        Value::Null => Value::Null,
        Value::Object(entries) => entries
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null),
        Value::Array(items) => {
            if key == "length" {
                Value::Int(items.len() as i64)
            } else {
                canonical_index(key)
                    .and_then(|i| items.get(i).cloned())
                    .unwrap_or(Value::Null)
            }
        }
        Value::String(s) => {
            if key == "length" {
                Value::Int(utf16_len(s) as i64)
            } else {
                canonical_index(key)
                    .and_then(|i| utf16_at(s, i))
                    .map(|c| Value::String(Cow::Owned(c)))
                    .unwrap_or(Value::Null)
            }
        }
        // JS: property access on numbers/booleans yields undefined for
        // anything we can express.
        Value::Bool(_) | Value::Int(_) | Value::Float(_) => Value::Null,
    }
}

/// JS `ToPropertyKey` as a string. Numbers go through JS number
/// stringification, so `arr[1.0]` hits index 1 and `arr[1.5]` misses.
fn property_key(index: &Value) -> Cow<'_, str> {
    match index {
        Value::String(s) => Cow::Borrowed(s.as_ref()),
        Value::Int(i) => Cow::Owned(number_to_string(*i as f64)),
        Value::Float(f) => Cow::Owned(number_to_string(*f)),
        other => Cow::Owned(display(other)),
    }
}

/// JS `ToIntegerOrInfinity` clamped into `[0, len]`, for substring/slice
/// style arguments.
fn clamp_index(n: f64, len: usize, relative: bool) -> usize {
    let n = if n.is_nan() { 0.0 } else { n.trunc() };
    if n < 0.0 {
        if relative {
            let from_end = len as f64 + n;
            if from_end < 0.0 {
                0
            } else {
                from_end as usize
            }
        } else {
            0
        }
    } else if n >= len as f64 {
        len
    } else {
        n as usize
    }
}

/// Internal invariant failure: the dispatcher in `eval_inner` routed a
/// variant to a category handler that does not implement it. Surfaces as
/// an error (never a panic) if a future variant is mis-routed.
fn dispatch_mismatch(op: &'static str) -> EvalError {
    debug_assert!(false, "eval dispatch mismatch for {op}");
    EvalError::Unsupported { variant: op }
}

// The evaluator is split into per-category handlers rather than one giant
// match: debug builds do not reuse stack slots across match arms, so a
// single ~85-arm function makes each recursion level cost tens of KB of
// stack and the depth budget alone cannot prevent overflow. With small
// frames, the default depth limit keeps evaluation comfortably inside
// even a 2 MiB test-thread stack.
impl<'a, 'e> Cx<'a, 'e> {
    fn eval(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        if self.depth >= self.budget.max_depth {
            return Err(EvalError::DepthLimit {
                limit: self.budget.max_depth,
            });
        }
        self.depth += 1;
        let result = self.eval_inner(expr);
        self.depth -= 1;
        result
    }

    fn charge_iteration(&mut self) -> Result<(), EvalError> {
        self.iterations += 1;
        if self.iterations > self.budget.max_iterations {
            return Err(EvalError::IterationLimit {
                limit: self.budget.max_iterations,
            });
        }
        Ok(())
    }

    fn lookup_var(&self, name: &str) -> Result<Value, EvalError> {
        self.locals
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
            .or_else(|| self.scope.var(name).cloned())
            .ok_or_else(|| EvalError::UnknownVar {
                name: name.to_string(),
            })
    }

    /// Evaluate `body` with a loop variable bound.
    fn eval_bound(
        &mut self,
        var: &'e str,
        value: &Value,
        body: &'e Expr,
    ) -> Result<Value, EvalError> {
        self.locals.push((var, value.clone()));
        let result = self.eval(body);
        self.locals.pop();
        result
    }

    fn eval_predicate(
        &mut self,
        var: &'e str,
        value: &Value,
        predicate: &'e Expr,
        op: &'static str,
    ) -> Result<bool, EvalError> {
        let result = self.eval_bound(var, value, predicate)?;
        as_bool(&result, op)
    }

    fn num(&mut self, expr: &'e Expr) -> Result<f64, EvalError> {
        Ok(to_number(&self.eval(expr)?))
    }

    /// Dispatch on expression category. The match is exhaustive, so new
    /// `Expr` variants fail compilation here until they are routed.
    fn eval_inner(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        use Expr::*;
        match expr {
            // Roots
            Lit { value } => Ok(value.clone()),
            Var { id } => self.lookup_var(&id.0),
            Scope { path } => self.eval_scope(path),

            // Client-only roots and helpers
            Signal { .. } => Err(EvalError::ClientOnly { variant: "Signal" }),
            Field { .. } => Err(EvalError::ClientOnly { variant: "Field" }),
            Response => Err(EvalError::ClientOnly {
                variant: "Response",
            }),
            Error => Err(EvalError::ClientOnly { variant: "Error" }),
            Event => Err(EvalError::ClientOnly { variant: "Event" }),
            Target => Err(EvalError::ClientOnly { variant: "Target" }),
            FieldHasError { .. } => Err(EvalError::ClientOnly {
                variant: "FieldHasError",
            }),
            FieldError { .. } => Err(EvalError::ClientOnly {
                variant: "FieldError",
            }),
            FieldTouched { .. } => Err(EvalError::ClientOnly {
                variant: "FieldTouched",
            }),
            FieldDirty { .. } => Err(EvalError::ClientOnly {
                variant: "FieldDirty",
            }),
            FormValid => Err(EvalError::ClientOnly {
                variant: "FormValid",
            }),
            FormDirty => Err(EvalError::ClientOnly {
                variant: "FormDirty",
            }),
            FormData => Err(EvalError::ClientOnly {
                variant: "FormData",
            }),
            IsEmail { .. } => Err(EvalError::ClientOnly { variant: "IsEmail" }),
            IsUrl { .. } => Err(EvalError::ClientOnly { variant: "IsUrl" }),
            MatchesPattern { .. } => Err(EvalError::ClientOnly {
                variant: "MatchesPattern",
            }),
            CardType { .. } => Err(EvalError::ClientOnly {
                variant: "CardType",
            }),

            // Not yet supported server-side
            Now => Err(EvalError::Unsupported { variant: "Now" }),
            ParseDate { .. } => Err(EvalError::Unsupported {
                variant: "ParseDate",
            }),

            // Categories
            e @ (Prop { .. } | Index { .. }) => self.eval_access(e),
            e @ (Add { .. } | Sub { .. } | Mul { .. } | Div { .. } | Mod { .. } | Neg { .. }) => {
                self.eval_arith(e)
            }
            e @ (Eq { .. } | Ne { .. } | Lt { .. } | Le { .. } | Gt { .. } | Ge { .. }) => {
                self.eval_compare(e)
            }
            e @ (And { .. } | Or { .. } | Not { .. } | Ternary { .. } | Coalesce { .. }) => {
                self.eval_logic(e)
            }
            e @ (Concat { .. }
            | Len { .. }
            | Trim { .. }
            | Lower { .. }
            | Upper { .. }
            | Contains { .. }
            | StartsWith { .. }
            | EndsWith { .. }
            | Substring { .. }
            | Replace { .. }
            | Split { .. }
            | Join { .. }
            | Matches { .. }) => self.eval_string_op(e),
            e @ (Array { .. } | Object { .. }) => self.eval_construct(e),
            e @ (Map { .. }
            | Filter { .. }
            | Find { .. }
            | FindIndex { .. }
            | Some { .. }
            | Every { .. }
            | Reduce { .. }
            | Includes { .. }
            | First { .. }
            | Last { .. }
            | Slice { .. }
            | Flat { .. }) => self.eval_array_op(e),
            e @ (IsNull { .. }
            | IsNumber { .. }
            | IsString { .. }
            | IsArray { .. }
            | ParseInt { .. }
            | ParseFloat { .. }
            | ToString { .. }
            | ToNumber { .. }) => self.eval_type_op(e),
            e @ (Abs { .. }
            | Round { .. }
            | Floor { .. }
            | Ceil { .. }
            | Min { .. }
            | Max { .. }
            | Clamp { .. }
            | FormatNumber { .. }
            | FormatDate { .. }) => self.eval_math(e),
        }
    }

    /// Null-propagating path walk from the scope root. Each hop clones the
    /// remaining subtree; fine for page-scope depth.
    fn eval_scope(&mut self, path: &[IrStr]) -> Result<Value, EvalError> {
        let mut current = Cow::Borrowed(self.scope.root());
        for segment in path {
            let next = prop_access(current.as_ref(), segment);
            current = Cow::Owned(next);
        }
        Ok(current.into_owned())
    }

    /// Property access (null-propagating).
    fn eval_access(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Prop { expr, prop } => Ok(prop_access(&self.eval(expr)?, prop)),
            Expr::Index { expr, index } => {
                let base = self.eval(expr)?;
                let index = self.eval(index)?;
                Ok(prop_access(&base, &property_key(&index)))
            }
            _ => Err(dispatch_mismatch("eval_access")),
        }
    }

    /// Arithmetic (JS number model: f64 with ToNumber coercion).
    fn eval_arith(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Add { left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(js_add(&l, &r))
            }
            Expr::Sub { left, right } => {
                let (l, r) = (self.num(left)?, self.num(right)?);
                Ok(Value::Float(l - r))
            }
            Expr::Mul { left, right } => {
                let (l, r) = (self.num(left)?, self.num(right)?);
                Ok(Value::Float(l * r))
            }
            Expr::Div { left, right } => {
                let (l, r) = (self.num(left)?, self.num(right)?);
                Ok(Value::Float(l / r))
            }
            Expr::Mod { left, right } => {
                let (l, r) = (self.num(left)?, self.num(right)?);
                Ok(Value::Float(l % r))
            }
            Expr::Neg { expr } => Ok(Value::Float(-self.num(expr)?)),
            _ => Err(dispatch_mismatch("eval_arith")),
        }
    }

    fn eval_compare(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Eq { left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(Value::Bool(strict_eq(&l, &r, "Eq")?))
            }
            Expr::Ne { left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(Value::Bool(!strict_eq(&l, &r, "Ne")?))
            }
            Expr::Lt { left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(Value::Bool(js_lt(&l, &r).unwrap_or(false)))
            }
            Expr::Gt { left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(Value::Bool(js_lt(&r, &l).unwrap_or(false)))
            }
            Expr::Le { left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(Value::Bool(js_lt(&r, &l).map(|b| !b).unwrap_or(false)))
            }
            Expr::Ge { left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                Ok(Value::Bool(js_lt(&l, &r).map(|b| !b).unwrap_or(false)))
            }
            _ => Err(dispatch_mismatch("eval_compare")),
        }
    }

    /// Logic (strict boolean, short-circuiting) and conditionals.
    fn eval_logic(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::And { left, right } => {
                let l = self.eval(left)?;
                if !as_bool(&l, "And")? {
                    return Ok(Value::Bool(false));
                }
                let r = self.eval(right)?;
                Ok(Value::Bool(as_bool(&r, "And")?))
            }
            Expr::Or { left, right } => {
                let l = self.eval(left)?;
                if as_bool(&l, "Or")? {
                    return Ok(Value::Bool(true));
                }
                let r = self.eval(right)?;
                Ok(Value::Bool(as_bool(&r, "Or")?))
            }
            Expr::Not { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::Bool(!as_bool(&v, "Not")?))
            }
            Expr::Ternary { cond, then, else_ } => {
                let c = self.eval(cond)?;
                if as_bool(&c, "Ternary")? {
                    self.eval(then)
                } else {
                    self.eval(else_)
                }
            }
            Expr::Coalesce { left, right } => {
                let l = self.eval(left)?;
                if matches!(l, Value::Null) {
                    self.eval(right)
                } else {
                    Ok(l)
                }
            }
            _ => Err(dispatch_mismatch("eval_logic")),
        }
    }

    fn eval_string_op(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Concat { parts } => match parts.as_slice() {
                [] => Ok(Value::String(Cow::Borrowed(""))),
                [single] => self.eval(single),
                [first, rest @ ..] => {
                    // Left fold of JS `+`, exactly like the emitted chain:
                    // all-numeric parts ADD, a string part switches to
                    // concatenation from there on.
                    let mut acc = self.eval(first)?;
                    for part in rest {
                        let next = self.eval(part)?;
                        acc = js_add(&acc, &next);
                    }
                    Ok(acc)
                }
            },
            Expr::Len { expr } => {
                let v = self.eval(expr)?;
                match &v {
                    Value::String(s) => Ok(Value::Int(utf16_len(s) as i64)),
                    Value::Array(items) => Ok(Value::Int(items.len() as i64)),
                    Value::Object(_) => Ok(prop_access(&v, "length")),
                    Value::Null => Err(EvalError::TypeMismatch {
                        op: "Len",
                        expected: "string or array",
                        got: "null",
                    }),
                    // JS: (5).length is undefined
                    _ => Ok(Value::Null),
                }
            }
            Expr::Trim { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::String(Cow::Owned(
                    trim_js(as_str(&v, "Trim")?).to_string(),
                )))
            }
            Expr::Lower { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::String(Cow::Owned(
                    as_str(&v, "Lower")?.to_lowercase(),
                )))
            }
            Expr::Upper { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::String(Cow::Owned(
                    as_str(&v, "Upper")?.to_uppercase(),
                )))
            }
            Expr::Contains { haystack, needle } => {
                let hay = self.eval(haystack)?;
                let needle = self.eval(needle)?;
                self.includes(&hay, &needle, "Contains")
            }
            Expr::StartsWith { string, prefix } => {
                let s = self.eval(string)?;
                let prefix = self.eval(prefix)?;
                Ok(Value::Bool(
                    as_str(&s, "StartsWith")?.starts_with(&display(&prefix)),
                ))
            }
            Expr::EndsWith { string, suffix } => {
                let s = self.eval(string)?;
                let suffix = self.eval(suffix)?;
                Ok(Value::Bool(
                    as_str(&s, "EndsWith")?.ends_with(&display(&suffix)),
                ))
            }
            Expr::Substring { string, start, end } => {
                // JS String.prototype.substring: NaN/negative -> 0, clamp
                // to length, swap when start > end. UTF-16 units.
                let v = self.eval(string)?;
                let s = as_str(&v, "Substring")?.to_string();
                let len = utf16_len(&s);
                let a = clamp_index(self.num(start)?, len, false);
                let b = match end {
                    Some(end) => clamp_index(self.num(end)?, len, false),
                    None => len,
                };
                let (from, to) = if a <= b { (a, b) } else { (b, a) };
                Ok(Value::String(Cow::Owned(utf16_slice(&s, from, to))))
            }
            Expr::Replace { string, from, to } => {
                // JS String.prototype.replace with a string pattern:
                // first occurrence only. `$` replacement patterns are NOT
                // interpreted (divergence, see SEMANTICS.md).
                let v = self.eval(string)?;
                let s = as_str(&v, "Replace")?.to_string();
                let from = display(&self.eval(from)?);
                let to = display(&self.eval(to)?);
                Ok(Value::String(Cow::Owned(s.replacen(&from, &to, 1))))
            }
            Expr::Split { string, delimiter } => {
                let v = self.eval(string)?;
                let delimiter = display(&self.eval(delimiter)?);
                let s = as_str(&v, "Split")?;
                let parts: Vec<Value> = if delimiter.is_empty() {
                    // JS splits by UTF-16 units; we split by chars, which
                    // only differs for non-BMP input (see SEMANTICS.md).
                    s.chars()
                        .map(|c| Value::String(Cow::Owned(c.to_string())))
                        .collect()
                } else {
                    s.split(delimiter.as_str())
                        .map(|part| Value::String(Cow::Owned(part.to_string())))
                        .collect()
                };
                Ok(Value::Array(parts))
            }
            Expr::Join { array, delimiter } => {
                let v = self.eval(array)?;
                let delimiter = display(&self.eval(delimiter)?);
                let items = as_array(&v, "Join")?;
                let mut out = String::new();
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(&delimiter);
                    }
                    if !matches!(item, Value::Null) {
                        out.push_str(&display(item));
                    }
                }
                Ok(Value::String(Cow::Owned(out)))
            }
            Expr::Matches { string, pattern } => {
                let v = self.eval(string)?;
                // JS RegExp.prototype.test coerces via ToString.
                let s = display(&v);
                let re = regex::Regex::new(pattern).map_err(|error| EvalError::InvalidRegex {
                    pattern: pattern.to_string(),
                    message: error.to_string(),
                })?;
                Ok(Value::Bool(re.is_match(&s)))
            }
            _ => Err(dispatch_mismatch("eval_string_op")),
        }
    }

    fn eval_construct(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Array { items } => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.eval(item)?);
                }
                Ok(Value::Array(out))
            }
            Expr::Object { entries } => {
                let mut out = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    out.push((key.clone(), self.eval(value)?));
                }
                Ok(Value::Object(out))
            }
            _ => Err(dispatch_mismatch("eval_construct")),
        }
    }

    /// Array combinators (budgeted per element visit).
    fn eval_array_op(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Map { array, var, body } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "Map")?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    self.charge_iteration()?;
                    out.push(self.eval_bound(&var.0, item, body)?);
                }
                Ok(Value::Array(out))
            }
            Expr::Filter {
                array,
                var,
                predicate,
            } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "Filter")?;
                let mut out = Vec::new();
                for item in items {
                    self.charge_iteration()?;
                    if self.eval_predicate(&var.0, item, predicate, "Filter")? {
                        out.push(item.clone());
                    }
                }
                Ok(Value::Array(out))
            }
            Expr::Find {
                array,
                var,
                predicate,
            } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "Find")?;
                for item in items {
                    self.charge_iteration()?;
                    if self.eval_predicate(&var.0, item, predicate, "Find")? {
                        return Ok(item.clone());
                    }
                }
                Ok(Value::Null)
            }
            Expr::FindIndex {
                array,
                var,
                predicate,
            } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "FindIndex")?;
                for (i, item) in items.iter().enumerate() {
                    self.charge_iteration()?;
                    if self.eval_predicate(&var.0, item, predicate, "FindIndex")? {
                        return Ok(Value::Int(i as i64));
                    }
                }
                Ok(Value::Int(-1))
            }
            Expr::Some {
                array,
                var,
                predicate,
            } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "Some")?;
                for item in items {
                    self.charge_iteration()?;
                    if self.eval_predicate(&var.0, item, predicate, "Some")? {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            Expr::Every {
                array,
                var,
                predicate,
            } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "Every")?;
                for item in items {
                    self.charge_iteration()?;
                    if !self.eval_predicate(&var.0, item, predicate, "Every")? {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            Expr::Reduce {
                array,
                acc_var,
                item_var,
                body,
                initial,
            } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "Reduce")?;
                let mut acc = self.eval(initial)?;
                for item in items {
                    self.charge_iteration()?;
                    self.locals.push((&acc_var.0, acc));
                    self.locals.push((&item_var.0, item.clone()));
                    let result = self.eval(body);
                    self.locals.pop();
                    self.locals.pop();
                    acc = result?;
                }
                Ok(acc)
            }
            Expr::Includes { array, value } => {
                let hay = self.eval(array)?;
                let needle = self.eval(value)?;
                self.includes(&hay, &needle, "Includes")
            }
            Expr::First { array } => {
                let v = self.eval(array)?;
                match &v {
                    Value::Array(items) => Ok(items.first().cloned().unwrap_or(Value::Null)),
                    Value::String(s) => Ok(utf16_at(s, 0)
                        .map(|c| Value::String(Cow::Owned(c)))
                        .unwrap_or(Value::Null)),
                    other => Err(EvalError::TypeMismatch {
                        op: "First",
                        expected: "array or string",
                        got: kind(other),
                    }),
                }
            }
            Expr::Last { array } => {
                let v = self.eval(array)?;
                match &v {
                    Value::Array(items) => Ok(items.last().cloned().unwrap_or(Value::Null)),
                    Value::String(s) => {
                        let len = utf16_len(s);
                        Ok(len
                            .checked_sub(1)
                            .and_then(|i| utf16_at(s, i))
                            .map(|c| Value::String(Cow::Owned(c)))
                            .unwrap_or(Value::Null))
                    }
                    other => Err(EvalError::TypeMismatch {
                        op: "Last",
                        expected: "array or string",
                        got: kind(other),
                    }),
                }
            }
            Expr::Slice { array, start, end } => {
                // JS slice: negative indices count from the end, clamped.
                let v = self.eval(array)?;
                match &v {
                    Value::Array(items) => {
                        let len = items.len();
                        let a = clamp_index(self.num(start)?, len, true);
                        let b = match end {
                            Some(end) => clamp_index(self.num(end)?, len, true),
                            None => len,
                        };
                        let out = if a < b { items[a..b].to_vec() } else { vec![] };
                        Ok(Value::Array(out))
                    }
                    Value::String(s) => {
                        let len = utf16_len(s);
                        let a = clamp_index(self.num(start)?, len, true);
                        let b = match end {
                            Some(end) => clamp_index(self.num(end)?, len, true),
                            None => len,
                        };
                        let out = if a < b {
                            utf16_slice(s, a, b)
                        } else {
                            String::new()
                        };
                        Ok(Value::String(Cow::Owned(out)))
                    }
                    other => Err(EvalError::TypeMismatch {
                        op: "Slice",
                        expected: "array or string",
                        got: kind(other),
                    }),
                }
            }
            Expr::Flat { array, depth } => {
                let v = self.eval(array)?;
                let items = as_array(&v, "Flat")?;
                let mut out = Vec::with_capacity(items.len());
                self.flatten(items, depth.unwrap_or(1) as usize, &mut out)?;
                Ok(Value::Array(out))
            }
            _ => Err(dispatch_mismatch("eval_array_op")),
        }
    }

    fn eval_type_op(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::IsNull { expr } => Ok(Value::Bool(matches!(self.eval(expr)?, Value::Null))),
            Expr::IsNumber { expr } => Ok(Value::Bool(matches!(
                self.eval(expr)?,
                Value::Int(_) | Value::Float(_)
            ))),
            Expr::IsString { expr } => {
                Ok(Value::Bool(matches!(self.eval(expr)?, Value::String(_))))
            }
            Expr::IsArray { expr } => Ok(Value::Bool(matches!(self.eval(expr)?, Value::Array(_)))),
            Expr::ParseInt { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::Float(parse_int_10(&display(&v))))
            }
            Expr::ParseFloat { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::Float(parse_float(&display(&v))))
            }
            Expr::ToString { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::String(Cow::Owned(display(&v))))
            }
            Expr::ToNumber { expr } => {
                let v = self.eval(expr)?;
                Ok(Value::Float(to_number(&v)))
            }
            _ => Err(dispatch_mismatch("eval_type_op")),
        }
    }

    /// Math ops plus number/date formatting.
    fn eval_math(&mut self, expr: &'e Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Abs { expr } => Ok(Value::Float(self.num(expr)?.abs())),
            Expr::Round { expr } => Ok(Value::Float(js_round(self.num(expr)?))),
            Expr::Floor { expr } => Ok(Value::Float(self.num(expr)?.floor())),
            Expr::Ceil { expr } => Ok(Value::Float(self.num(expr)?.ceil())),
            Expr::Min { values } => {
                let mut acc = f64::INFINITY;
                for value in values {
                    acc = js_min2(acc, self.num(value)?);
                }
                Ok(Value::Float(acc))
            }
            Expr::Max { values } => {
                let mut acc = f64::NEG_INFINITY;
                for value in values {
                    acc = js_max2(acc, self.num(value)?);
                }
                Ok(Value::Float(acc))
            }
            Expr::Clamp { value, min, max } => {
                // Matches the emitted Math.min(Math.max(v, min), max).
                let v = self.num(value)?;
                let min = self.num(min)?;
                let max = self.num(max)?;
                Ok(Value::Float(js_min2(js_max2(v, min), max)))
            }
            Expr::FormatNumber { value, decimals } => {
                if *decimals > tofixed::MAX_FRACTION_DIGITS {
                    return Err(EvalError::InvalidDecimals {
                        decimals: *decimals,
                    });
                }
                let n = self.num(value)?;
                Ok(Value::String(Cow::Owned(tofixed::to_fixed(n, *decimals))))
            }
            Expr::FormatDate { date, format } => {
                let ms = self.num(date)?;
                Ok(Value::String(Cow::Owned(date::format_date(ms, format))))
            }
            _ => Err(dispatch_mismatch("eval_math")),
        }
    }

    /// Shared implementation of `Contains`/`Includes`: substring test for
    /// strings (needle coerced via ToString), SameValueZero membership for
    /// arrays.
    fn includes(
        &mut self,
        haystack: &Value,
        needle: &Value,
        op: &'static str,
    ) -> Result<Value, EvalError> {
        match haystack {
            Value::String(s) => Ok(Value::Bool(s.contains(&display(needle)))),
            Value::Array(items) => Ok(Value::Bool(
                items.iter().any(|item| same_value_zero(item, needle)),
            )),
            other => Err(EvalError::TypeMismatch {
                op,
                expected: "string or array",
                got: kind(other),
            }),
        }
    }

    fn flatten(
        &mut self,
        items: &[Value],
        depth: usize,
        out: &mut Vec<Value>,
    ) -> Result<(), EvalError> {
        for item in items {
            self.charge_iteration()?;
            match item {
                Value::Array(inner) if depth > 0 => self.flatten(inner, depth - 1, out)?,
                other => out.push(other.clone()),
            }
        }
        Ok(())
    }
}
