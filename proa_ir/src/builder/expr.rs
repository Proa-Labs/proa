// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Expression builder for ergonomic IR construction
//!
//! This provides a chainable API for building expressions.

use crate::expr::Expr;
use crate::types::{FieldId, VarId};
use crate::value::Value;

/// Builder for constructing expressions with a fluent API
///
/// This wraps an Expr and provides chainable methods.
#[derive(Debug, Clone)]
pub struct ExprBuilder(pub Expr);

#[allow(clippy::should_implement_trait)]
impl ExprBuilder {
    // =========================================================================
    // Constructors
    // =========================================================================

    /// Create a literal value
    pub fn lit(value: impl Into<Value>) -> Self {
        Self(Expr::lit(value))
    }

    /// Reference a signal
    pub fn signal(name: &'static str) -> Self {
        Self(Expr::signal(name))
    }

    /// Reference a form field
    pub fn field(name: &'static str) -> Self {
        Self(Expr::field(name))
    }

    /// Reference a local variable
    pub fn var(name: &'static str) -> Self {
        Self(Expr::var(name))
    }

    /// Read a top-level binding from the page render scope (server data)
    pub fn scope(name: impl Into<crate::types::IrStr>) -> Self {
        Self(Expr::scope(name))
    }

    /// Read a nested path from the page render scope (server data)
    pub fn scope_path<I, S>(path: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<crate::types::IrStr>,
    {
        Self(Expr::scope_path(path))
    }

    /// The response from a fetch operation
    pub fn response() -> Self {
        Self(Expr::Response)
    }

    /// The error from a fetch operation
    pub fn error() -> Self {
        Self(Expr::Error)
    }

    /// The event that triggered the handler
    pub fn event() -> Self {
        Self(Expr::Event)
    }

    /// The element that triggered the handler
    pub fn target() -> Self {
        Self(Expr::Target)
    }

    /// Current timestamp
    pub fn now() -> Self {
        Self(Expr::Now)
    }

    /// Check if form is valid
    pub fn form_valid() -> Self {
        Self(Expr::FormValid)
    }

    /// Check if any field is dirty
    pub fn form_dirty() -> Self {
        Self(Expr::FormDirty)
    }

    /// Get form data as object
    pub fn form_data() -> Self {
        Self(Expr::FormData)
    }

    /// Check if a field has an error
    pub fn field_has_error(field: &'static str) -> Self {
        Self(Expr::FieldHasError {
            id: FieldId::new(field),
        })
    }

    /// Get a field's error message
    pub fn field_error(field: &'static str) -> Self {
        Self(Expr::FieldError {
            id: FieldId::new(field),
        })
    }

    /// Check if field has been touched
    pub fn field_touched(field: &'static str) -> Self {
        Self(Expr::FieldTouched {
            id: FieldId::new(field),
        })
    }

    /// Check if field is dirty
    pub fn field_dirty(field: &'static str) -> Self {
        Self(Expr::FieldDirty {
            id: FieldId::new(field),
        })
    }

    // =========================================================================
    // Property Access
    // =========================================================================

    /// Access a property: `expr.prop`
    pub fn prop(self, name: impl Into<crate::types::IrStr>) -> Self {
        Self(Expr::Prop {
            expr: Box::new(self.0),
            prop: name.into(),
        })
    }

    /// Access an index: `expr[index]`
    pub fn index(self, index: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Index {
            expr: Box::new(self.0),
            index: Box::new(index.into().0),
        })
    }

    // =========================================================================
    // Arithmetic
    // =========================================================================

    /// Addition: `self + other`
    pub fn add(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Add {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Subtraction: `self - other`
    pub fn sub(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Sub {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Multiplication: `self * other`
    pub fn mul(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Mul {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Division: `self / other`
    pub fn div(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Div {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Modulo: `self % other`
    pub fn rem(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Mod {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Negation: `-self`
    pub fn neg(self) -> Self {
        Self(Expr::Neg {
            expr: Box::new(self.0),
        })
    }

    // =========================================================================
    // Comparison
    // =========================================================================

    /// Equality: `self == other`
    pub fn eq(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Eq {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Inequality: `self != other`
    pub fn ne(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Ne {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Less than: `self < other`
    pub fn lt(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Lt {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Less than or equal: `self <= other`
    pub fn le(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Le {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Greater than: `self > other`
    pub fn gt(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Gt {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Greater than or equal: `self >= other`
    pub fn ge(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Ge {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    // =========================================================================
    // Logical
    // =========================================================================

    /// Logical AND: `self && other`
    pub fn and(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::And {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Logical OR: `self || other`
    pub fn or(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Or {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    /// Logical NOT: `!self`
    pub fn not(self) -> Self {
        Self(Expr::Not {
            expr: Box::new(self.0),
        })
    }

    // =========================================================================
    // Conditional
    // =========================================================================

    /// Ternary: `self ? then : else_`
    pub fn then_else(self, then: impl Into<ExprBuilder>, else_: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Ternary {
            cond: Box::new(self.0),
            then: Box::new(then.into().0),
            else_: Box::new(else_.into().0),
        })
    }

    /// Null coalescing: `self ?? other`
    pub fn or_else(self, other: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Coalesce {
            left: Box::new(self.0),
            right: Box::new(other.into().0),
        })
    }

    // =========================================================================
    // String Operations
    // =========================================================================

    /// Get length
    pub fn len(self) -> Self {
        Self(Expr::Len {
            expr: Box::new(self.0),
        })
    }

    /// Trim whitespace
    pub fn trim(self) -> Self {
        Self(Expr::Trim {
            expr: Box::new(self.0),
        })
    }

    /// Convert to lowercase
    pub fn lower(self) -> Self {
        Self(Expr::Lower {
            expr: Box::new(self.0),
        })
    }

    /// Convert to uppercase
    pub fn upper(self) -> Self {
        Self(Expr::Upper {
            expr: Box::new(self.0),
        })
    }

    /// Check if contains substring
    pub fn contains(self, needle: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Contains {
            haystack: Box::new(self.0),
            needle: Box::new(needle.into().0),
        })
    }

    /// Check if starts with prefix
    pub fn starts_with(self, prefix: impl Into<ExprBuilder>) -> Self {
        Self(Expr::StartsWith {
            string: Box::new(self.0),
            prefix: Box::new(prefix.into().0),
        })
    }

    /// Check if ends with suffix
    pub fn ends_with(self, suffix: impl Into<ExprBuilder>) -> Self {
        Self(Expr::EndsWith {
            string: Box::new(self.0),
            suffix: Box::new(suffix.into().0),
        })
    }

    /// Check if matches regex pattern
    pub fn matches(self, pattern: impl Into<crate::types::IrStr>) -> Self {
        Self(Expr::Matches {
            string: Box::new(self.0),
            pattern: pattern.into(),
        })
    }

    // =========================================================================
    // Array Operations
    // =========================================================================

    /// Create an array
    pub fn array(items: Vec<ExprBuilder>) -> Self {
        Self(Expr::Array {
            items: items.into_iter().map(|e| e.0).collect(),
        })
    }

    /// Map over array
    pub fn map(self, var: &'static str, body: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Map {
            array: Box::new(self.0),
            var: VarId::new(var),
            body: Box::new(body.into().0),
        })
    }

    /// Filter array
    pub fn filter(self, var: &'static str, predicate: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Filter {
            array: Box::new(self.0),
            var: VarId::new(var),
            predicate: Box::new(predicate.into().0),
        })
    }

    /// Find in array
    pub fn find(self, var: &'static str, predicate: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Find {
            array: Box::new(self.0),
            var: VarId::new(var),
            predicate: Box::new(predicate.into().0),
        })
    }

    /// Check if array includes value
    pub fn includes(self, value: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Includes {
            array: Box::new(self.0),
            value: Box::new(value.into().0),
        })
    }

    /// Get first element
    pub fn first(self) -> Self {
        Self(Expr::First {
            array: Box::new(self.0),
        })
    }

    /// Get last element
    pub fn last(self) -> Self {
        Self(Expr::Last {
            array: Box::new(self.0),
        })
    }

    // =========================================================================
    // Type Checking
    // =========================================================================

    /// Check if null
    pub fn is_null(self) -> Self {
        Self(Expr::IsNull {
            expr: Box::new(self.0),
        })
    }

    /// Check if number
    pub fn is_number(self) -> Self {
        Self(Expr::IsNumber {
            expr: Box::new(self.0),
        })
    }

    /// Check if string
    pub fn is_string(self) -> Self {
        Self(Expr::IsString {
            expr: Box::new(self.0),
        })
    }

    /// Check if array
    pub fn is_array(self) -> Self {
        Self(Expr::IsArray {
            expr: Box::new(self.0),
        })
    }

    // =========================================================================
    // Conversion
    // =========================================================================

    /// Parse as integer
    pub fn parse_int(self) -> Self {
        Self(Expr::ParseInt {
            expr: Box::new(self.0),
        })
    }

    /// Parse as float
    pub fn parse_float(self) -> Self {
        Self(Expr::ParseFloat {
            expr: Box::new(self.0),
        })
    }

    /// Convert to string
    pub fn to_string(self) -> Self {
        Self(Expr::ToString {
            expr: Box::new(self.0),
        })
    }

    /// Convert to number (JS `Number(x)`)
    pub fn to_number(self) -> Self {
        Self(Expr::ToNumber {
            expr: Box::new(self.0),
        })
    }

    /// Format with fixed decimals (JS `toFixed` semantics)
    pub fn format_number(self, decimals: u8) -> Self {
        Self(self.0.format_number(decimals))
    }

    // =========================================================================
    // Math
    // =========================================================================

    /// Absolute value
    pub fn abs(self) -> Self {
        Self(Expr::Abs {
            expr: Box::new(self.0),
        })
    }

    /// Round to nearest integer
    pub fn round(self) -> Self {
        Self(Expr::Round {
            expr: Box::new(self.0),
        })
    }

    /// Round down
    pub fn floor(self) -> Self {
        Self(Expr::Floor {
            expr: Box::new(self.0),
        })
    }

    /// Round up
    pub fn ceil(self) -> Self {
        Self(Expr::Ceil {
            expr: Box::new(self.0),
        })
    }

    /// Clamp between min and max
    pub fn clamp(self, min: impl Into<ExprBuilder>, max: impl Into<ExprBuilder>) -> Self {
        Self(Expr::Clamp {
            value: Box::new(self.0),
            min: Box::new(min.into().0),
            max: Box::new(max.into().0),
        })
    }

    // =========================================================================
    // Validation Helpers
    // =========================================================================

    /// Check if valid email
    pub fn is_email(self) -> Self {
        Self(Expr::IsEmail {
            expr: Box::new(self.0),
        })
    }

    /// Check if valid URL
    pub fn is_url(self) -> Self {
        Self(Expr::IsUrl {
            expr: Box::new(self.0),
        })
    }

    // =========================================================================
    // Build
    // =========================================================================

    /// Convert to the underlying Expr
    pub fn build(self) -> Expr {
        self.0
    }
}

// Implement From for common types
impl From<bool> for ExprBuilder {
    fn from(b: bool) -> Self {
        Self::lit(b)
    }
}

impl From<i32> for ExprBuilder {
    fn from(n: i32) -> Self {
        Self::lit(n)
    }
}

impl From<i64> for ExprBuilder {
    fn from(n: i64) -> Self {
        Self::lit(n)
    }
}

impl From<f64> for ExprBuilder {
    fn from(n: f64) -> Self {
        Self::lit(n)
    }
}

impl From<&'static str> for ExprBuilder {
    fn from(s: &'static str) -> Self {
        Self::lit(s)
    }
}

impl From<Expr> for ExprBuilder {
    fn from(expr: Expr) -> Self {
        Self(expr)
    }
}

impl From<ExprBuilder> for Expr {
    fn from(builder: ExprBuilder) -> Self {
        builder.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_expressions() {
        let expr = ExprBuilder::field("email")
            .len()
            .gt(0)
            .and(ExprBuilder::field("email").is_email())
            .build();

        // Verify it built correctly
        assert!(matches!(expr, Expr::And { .. }));
    }

    #[test]
    fn test_chained_operations() {
        let expr = ExprBuilder::signal("count").add(1).mul(2).build();

        assert!(matches!(expr, Expr::Mul { .. }));
    }

    #[test]
    fn test_ternary() {
        let expr = ExprBuilder::field("age")
            .ge(18)
            .then_else("adult", "minor")
            .build();

        assert!(matches!(expr, Expr::Ternary { .. }));
    }
}
