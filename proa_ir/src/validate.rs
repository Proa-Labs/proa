// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Validation rules for form fields
//!
//! These define both sync and async validation that can run client-side.

use crate::expr::Expr;
use crate::types::FieldId;
use serde::Serialize;

/// A validation rule for a field
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ValidationRule {
    // =========================================================================
    // Presence
    // =========================================================================
    /// Field must have a non-empty value
    Required { message: &'static str },

    // =========================================================================
    // Length/Size
    // =========================================================================
    /// String must be at least this long
    MinLength { min: usize, message: &'static str },

    /// String must be at most this long
    MaxLength { max: usize, message: &'static str },

    /// String must be exactly this long
    Length { len: usize, message: &'static str },

    /// String length must be in range
    LengthRange {
        min: usize,
        max: usize,
        message: &'static str,
    },

    // =========================================================================
    // Numeric
    // =========================================================================
    /// Value must be at least this number
    Min { min: f64, message: &'static str },

    /// Value must be at most this number
    Max { max: f64, message: &'static str },

    /// Value must be in range
    Range {
        min: f64,
        max: f64,
        message: &'static str,
    },

    /// Value must be an integer
    Integer { message: &'static str },

    /// Value must be positive
    Positive { message: &'static str },

    /// Value must be negative
    Negative { message: &'static str },

    // =========================================================================
    // Format
    // =========================================================================
    /// Must be a valid email
    Email { message: &'static str },

    /// Must be a valid URL
    Url { message: &'static str },

    /// Must be a valid phone number
    Phone { message: &'static str },

    /// Must match regex pattern
    Pattern {
        pattern: &'static str,
        message: &'static str,
    },

    /// Must be alphanumeric
    Alphanumeric { message: &'static str },

    /// Must be alpha only
    Alpha { message: &'static str },

    /// Must be numeric only
    Numeric { message: &'static str },

    /// Must be a valid UUID
    Uuid { message: &'static str },

    /// Must be a valid slug (lowercase, hyphens, no spaces)
    Slug { message: &'static str },

    // =========================================================================
    // Date/Time
    // =========================================================================
    /// Must be a valid date
    Date {
        format: Option<&'static str>,
        message: &'static str,
    },

    /// Must be a date after the given date
    DateAfter {
        after: &'static str,
        message: &'static str,
    },

    /// Must be a date before the given date
    DateBefore {
        before: &'static str,
        message: &'static str,
    },

    /// Must be a date in the future
    FutureDate { message: &'static str },

    /// Must be a date in the past
    PastDate { message: &'static str },

    // =========================================================================
    // Cross-Field
    // =========================================================================
    /// Must match another field's value
    Matches {
        other: FieldId,
        message: &'static str,
    },

    /// Must be different from another field's value
    Different {
        other: FieldId,
        message: &'static str,
    },

    /// Must be less than another field's value
    LessThan {
        other: FieldId,
        message: &'static str,
    },

    /// Must be greater than another field's value
    GreaterThan {
        other: FieldId,
        message: &'static str,
    },

    // =========================================================================
    // Conditional
    // =========================================================================
    /// Only validate if condition is true
    When {
        condition: Expr,
        rule: Box<ValidationRule>,
    },

    /// Required only if condition is true
    RequiredIf {
        condition: Expr,
        message: &'static str,
    },

    /// Required only if another field has a specific value
    RequiredIfField {
        field: FieldId,
        value: &'static str,
        message: &'static str,
    },

    /// Required unless condition is true
    RequiredUnless {
        condition: Expr,
        message: &'static str,
    },

    // =========================================================================
    // Array
    // =========================================================================
    /// Array must have at least this many items
    MinItems { min: usize, message: &'static str },

    /// Array must have at most this many items
    MaxItems { max: usize, message: &'static str },

    /// Array items must be unique
    UniqueItems { message: &'static str },

    // =========================================================================
    // File
    // =========================================================================
    /// File must not exceed this size (in bytes)
    MaxFileSize {
        max_bytes: u64,
        message: &'static str,
    },

    /// File must have one of these extensions
    FileExtension {
        #[serde(skip)]
        extensions: &'static [&'static str],
        message: &'static str,
    },

    /// File must have one of these MIME types
    MimeType {
        #[serde(skip)]
        types: &'static [&'static str],
        message: &'static str,
    },

    // =========================================================================
    // Async
    // =========================================================================
    /// Validate via server endpoint
    Async {
        /// URL to call (field value will be passed as query param or body)
        endpoint: &'static str,
        /// Debounce in milliseconds
        debounce_ms: u32,
        /// Error message if validation fails
        message: &'static str,
        /// Additional fields to include in request
        #[serde(skip)]
        include_fields: &'static [&'static str],
    },

    // =========================================================================
    // Custom
    // =========================================================================
    /// Custom validation expression (must return true for valid)
    Custom {
        validate: Expr,
        message: &'static str,
    },

    // =========================================================================
    // Composition
    // =========================================================================
    /// All rules must pass
    All { rules: Vec<ValidationRule> },

    /// At least one rule must pass
    Any {
        rules: Vec<ValidationRule>,
        message: &'static str,
    },
}

impl ValidationRule {
    // Convenience constructors
    pub const fn required(message: &'static str) -> Self {
        Self::Required { message }
    }

    pub const fn email(message: &'static str) -> Self {
        Self::Email { message }
    }

    pub const fn min_length(min: usize, message: &'static str) -> Self {
        Self::MinLength { min, message }
    }

    pub const fn max_length(max: usize, message: &'static str) -> Self {
        Self::MaxLength { max, message }
    }

    pub const fn pattern(pattern: &'static str, message: &'static str) -> Self {
        Self::Pattern { pattern, message }
    }

    pub fn matches(field: &'static str, message: &'static str) -> Self {
        Self::Matches {
            other: FieldId::new(field),
            message,
        }
    }

    pub fn async_validate(endpoint: &'static str, debounce_ms: u32, message: &'static str) -> Self {
        Self::Async {
            endpoint,
            debounce_ms,
            message,
            include_fields: &[],
        }
    }

    pub fn when(condition: Expr, rule: ValidationRule) -> Self {
        Self::When {
            condition,
            rule: Box::new(rule),
        }
    }

    pub fn required_if(condition: Expr, message: &'static str) -> Self {
        Self::RequiredIf { condition, message }
    }

    pub fn required_if_field(
        field: &'static str,
        value: &'static str,
        message: &'static str,
    ) -> Self {
        Self::RequiredIfField {
            field: FieldId::new(field),
            value,
            message,
        }
    }

    pub fn custom(validate: Expr, message: &'static str) -> Self {
        Self::Custom { validate, message }
    }
}

/// Configuration for field validation
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldValidation {
    /// The field being validated
    pub field: FieldId,
    /// Validation rules
    pub rules: Vec<ValidationRule>,
    /// When to validate
    #[serde(default)]
    pub trigger: ValidationTrigger,
    /// Transform value before validation
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transforms: Vec<Transform>,
}

/// When validation should trigger
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationTrigger {
    /// Validate on blur (after first interaction)
    #[default]
    Blur,
    /// Validate on every input change
    Input,
    /// Validate on change (blur for text, immediate for select)
    Change,
    /// Only validate on submit
    Submit,
}

/// Transform to apply before validation
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Transform {
    /// Trim whitespace
    Trim,
    /// Convert to lowercase
    Lowercase,
    /// Convert to uppercase
    Uppercase,
    /// Remove non-digit characters
    DigitsOnly,
    /// Normalize whitespace (collapse multiple spaces)
    NormalizeWhitespace,
}

/// Input format mask - formats the displayed value as user types
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputFormat {
    /// US phone number: (123) 456-7890
    Phone,
    /// Credit card: 1234 5678 9012 3456
    CreditCard,
    /// Date: MM/YY or MM/DD/YYYY
    Date { pattern: &'static str },
    /// SSN: 123-45-6789
    Ssn,
    /// ZIP code: 12345 or 12345-6789
    ZipCode,
    /// CVC: 3-4 digits only
    Cvc,
    /// Name: letters, spaces, hyphens, apostrophes only
    Name,
    /// Currency: $1,234.56
    Currency { locale: &'static str },
    /// Custom mask pattern (9 = digit, A = letter, * = any)
    /// e.g., "(999) 999-9999" for phone
    Custom { mask: &'static str },
}
