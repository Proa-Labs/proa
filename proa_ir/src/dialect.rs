// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Server-dialect validation for IR expressions.
//!
//! Runtime-template compilers can lower templates to programs whose `{...}`
//! slots are [`Expr`]s evaluated against the template data scope. Client-only nodes
//! (signals, form state, DOM references, validation/payment helpers) have
//! no meaning there; [`validate_server_dialect`] rejects them at
//! compile-to-program time with structured errors carrying the offending
//! variant name and a path into the expression tree, so authoring tools
//! can point at the exact subexpression.
//!
//! `Stmt` is not reachable from `Expr` (expressions are pure), so no
//! statement-level walk is needed for expression slots.

use crate::expr::Expr;
use crate::types::IrStr;
use std::fmt;

/// Why a node is rejected from the server dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialectErrorReason {
    /// The node reads client-only state (signals, form fields, DOM, ...).
    ClientOnly,
    /// The node is impure (`Now`); server renders must be deterministic.
    Impure,
    /// The node is not implemented by the server evaluator (`ParseDate`).
    Unsupported,
}

/// A rejected node: which variant, where in the tree, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectError {
    /// The offending `Expr` variant name, e.g. `"Signal"`.
    pub variant: &'static str,
    /// Dot path from the root to the offending node, e.g.
    /// `"cond.left.items[2]"`. Empty for the root itself.
    pub path: String,
    pub reason: DialectErrorReason,
}

impl fmt::Display for DialectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.reason {
            DialectErrorReason::ClientOnly => "client-only",
            DialectErrorReason::Impure => "impure",
            DialectErrorReason::Unsupported => "unsupported",
        };
        let location: &str = if self.path.is_empty() {
            "at the expression root"
        } else {
            &self.path
        };
        if self.path.is_empty() {
            write!(f, "{} expression `{}` {}", reason, self.variant, location)
        } else {
            write!(
                f,
                "{} expression `{}` at `{}`",
                reason, self.variant, location
            )
        }
    }
}

/// Validate that an expression uses only the server-evaluable dialect.
///
/// Returns every offending node (not just the first), so a whole slot can
/// be reported and fixed in one authoring round trip.
pub fn validate_server_dialect(expr: &Expr) -> Result<(), Vec<DialectError>> {
    validate_server_dialect_bounded(expr, usize::MAX)
}

/// Validate the server dialect while collecting at most `maximum` failures.
/// The expression walk stops as soon as the budget is full.
pub fn validate_server_dialect_bounded(
    expr: &Expr,
    maximum: usize,
) -> Result<(), Vec<DialectError>> {
    let mut errors = Vec::new();
    let mut path = String::new();
    walk(expr, &mut path, &mut errors, maximum);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Push a `.segment` (or bare segment at the root), run `f`, restore.
fn with_segment(
    path: &mut String,
    segment: &str,
    errors: &mut Vec<DialectError>,
    child: &Expr,
    maximum: usize,
) {
    if errors.len() >= maximum {
        return;
    }
    let saved = path.len();
    if !path.is_empty() {
        path.push('.');
    }
    path.push_str(segment);
    walk(child, path, errors, maximum);
    path.truncate(saved);
}

fn with_indexed_segment(
    path: &mut String,
    segment: &str,
    index: usize,
    errors: &mut Vec<DialectError>,
    child: &Expr,
    maximum: usize,
) {
    with_segment(path, &format!("{segment}[{index}]"), errors, child, maximum);
}

fn reject(
    errors: &mut Vec<DialectError>,
    variant: &'static str,
    path: &str,
    reason: DialectErrorReason,
) {
    errors.push(DialectError {
        variant,
        path: path.to_string(),
        reason,
    });
}

fn walk(expr: &Expr, path: &mut String, errors: &mut Vec<DialectError>, maximum: usize) {
    if errors.len() >= maximum {
        return;
    }
    use DialectErrorReason::*;
    use Expr::*;
    match expr {
        // Server-evaluable roots
        Lit { .. } | Var { .. } | Scope { .. } => {}

        // Client-only roots
        Signal { .. } => reject(errors, "Signal", path, ClientOnly),
        Field { .. } => reject(errors, "Field", path, ClientOnly),
        Response => reject(errors, "Response", path, ClientOnly),
        Error => reject(errors, "Error", path, ClientOnly),
        Event => reject(errors, "Event", path, ClientOnly),
        Target => reject(errors, "Target", path, ClientOnly),

        // Client-only form state
        FieldHasError { .. } => reject(errors, "FieldHasError", path, ClientOnly),
        FieldError { .. } => reject(errors, "FieldError", path, ClientOnly),
        FieldTouched { .. } => reject(errors, "FieldTouched", path, ClientOnly),
        FieldDirty { .. } => reject(errors, "FieldDirty", path, ClientOnly),
        FormValid => reject(errors, "FormValid", path, ClientOnly),
        FormDirty => reject(errors, "FormDirty", path, ClientOnly),
        FormData => reject(errors, "FormData", path, ClientOnly),

        // Client-only validation/payment helpers (their operands are not
        // walked: the node itself is already rejected and the subtree is
        // client-side logic).
        IsEmail { .. } => reject(errors, "IsEmail", path, ClientOnly),
        IsUrl { .. } => reject(errors, "IsUrl", path, ClientOnly),
        MatchesPattern { .. } => reject(errors, "MatchesPattern", path, ClientOnly),
        CardType { .. } => reject(errors, "CardType", path, ClientOnly),

        // Impure / unimplemented
        Now => reject(errors, "Now", path, Impure),
        ParseDate { .. } => reject(errors, "ParseDate", path, Unsupported),

        // Recursive structure
        Prop { expr, .. } => with_segment(path, "expr", errors, expr, maximum),
        Index { expr, index } => {
            with_segment(path, "expr", errors, expr, maximum);
            with_segment(path, "index", errors, index, maximum);
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
            with_segment(path, "left", errors, left, maximum);
            with_segment(path, "right", errors, right, maximum);
        }

        Neg { expr }
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
        | Ceil { expr } => with_segment(path, "expr", errors, expr, maximum),

        Ternary { cond, then, else_ } => {
            with_segment(path, "cond", errors, cond, maximum);
            with_segment(path, "then", errors, then, maximum);
            with_segment(path, "else", errors, else_, maximum);
        }

        Concat { parts } => {
            for (i, part) in parts.iter().enumerate() {
                if errors.len() >= maximum {
                    break;
                }
                with_indexed_segment(path, "parts", i, errors, part, maximum);
            }
        }
        Contains { haystack, needle } => {
            with_segment(path, "haystack", errors, haystack, maximum);
            with_segment(path, "needle", errors, needle, maximum);
        }
        StartsWith { string, prefix } => {
            with_segment(path, "string", errors, string, maximum);
            with_segment(path, "prefix", errors, prefix, maximum);
        }
        EndsWith { string, suffix } => {
            with_segment(path, "string", errors, string, maximum);
            with_segment(path, "suffix", errors, suffix, maximum);
        }
        Substring { string, start, end } => {
            with_segment(path, "string", errors, string, maximum);
            with_segment(path, "start", errors, start, maximum);
            if let Option::Some(end) = end {
                with_segment(path, "end", errors, end, maximum);
            }
        }
        Replace { string, from, to } => {
            with_segment(path, "string", errors, string, maximum);
            with_segment(path, "from", errors, from, maximum);
            with_segment(path, "to", errors, to, maximum);
        }
        Split { string, delimiter } => {
            with_segment(path, "string", errors, string, maximum);
            with_segment(path, "delimiter", errors, delimiter, maximum);
        }
        Join { array, delimiter } => {
            with_segment(path, "array", errors, array, maximum);
            with_segment(path, "delimiter", errors, delimiter, maximum);
        }
        Matches { string, .. } => with_segment(path, "string", errors, string, maximum),

        Array { items } => {
            for (i, item) in items.iter().enumerate() {
                if errors.len() >= maximum {
                    break;
                }
                with_indexed_segment(path, "items", i, errors, item, maximum);
            }
        }
        Object { entries } => {
            for (key, value) in entries {
                if errors.len() >= maximum {
                    break;
                }
                with_segment(path, &object_segment(key), errors, value, maximum);
            }
        }

        Map { array, body, .. } => {
            with_segment(path, "array", errors, array, maximum);
            with_segment(path, "body", errors, body, maximum);
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
            with_segment(path, "array", errors, array, maximum);
            with_segment(path, "predicate", errors, predicate, maximum);
        }
        Reduce {
            array,
            body,
            initial,
            ..
        } => {
            with_segment(path, "array", errors, array, maximum);
            with_segment(path, "body", errors, body, maximum);
            with_segment(path, "initial", errors, initial, maximum);
        }
        Includes { array, value } => {
            with_segment(path, "array", errors, array, maximum);
            with_segment(path, "value", errors, value, maximum);
        }
        First { array } | Last { array } | Flat { array, .. } => {
            with_segment(path, "array", errors, array, maximum);
        }
        Slice { array, start, end } => {
            with_segment(path, "array", errors, array, maximum);
            with_segment(path, "start", errors, start, maximum);
            if let Option::Some(end) = end {
                with_segment(path, "end", errors, end, maximum);
            }
        }

        Min { values } | Max { values } => {
            for (i, value) in values.iter().enumerate() {
                if errors.len() >= maximum {
                    break;
                }
                with_indexed_segment(path, "values", i, errors, value, maximum);
            }
        }
        Clamp { value, min, max } => {
            with_segment(path, "value", errors, value, maximum);
            with_segment(path, "min", errors, min, maximum);
            with_segment(path, "max", errors, max, maximum);
        }

        FormatNumber { value, .. } => with_segment(path, "value", errors, value, maximum),
        FormatDate { date, .. } => with_segment(path, "date", errors, date, maximum),
    }
}

fn object_segment(key: &IrStr) -> String {
    format!("entries[{key:?}]")
}
