// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Const-compatible JSON parsing utilities.
//!
//! This module provides `const fn` JSON parsing for simple use cases at compile time.

/// A JSON value that can be parsed at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonValue<'a> {
    Null,
    Bool(bool),
    Number(i64),
    String(&'a str),
    Object(JsonObject<'a>),
    Array(JsonArray<'a>),
}

/// A JSON object representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonObject<'a> {
    json: &'a str,
}

/// A JSON array representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonArray<'a> {
    json: &'a str,
}

impl<'a> JsonObject<'a> {
    /// Get a field from the JSON object by key.
    pub const fn get(&self, key: &str) -> Option<JsonValue<'a>> {
        get_field(self.json, key)
    }
}

impl<'a> JsonArray<'a> {
    /// Get an element from the JSON array by index.
    pub const fn get(&self, _index: usize) -> Option<JsonValue<'a>> {
        // TODO: Implement array indexing
        None
    }
}

impl<'a> JsonValue<'a> {
    /// Parse a JSON value from a string at compile time.
    pub const fn parse(json: &'a str) -> Result<Self, ParseError> {
        let json = skip_whitespace(json);
        parse_value(json)
    }

    /// If this value is a string, return it.
    pub const fn as_str(&self) -> Option<&'a str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// If this value is an object, return it.
    pub const fn as_object(&self) -> Option<JsonObject<'a>> {
        match self {
            JsonValue::Object(obj) => Some(*obj),
            _ => None,
        }
    }

    /// If this value is a number, return it.
    pub const fn as_number(&self) -> Option<i64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// If this value is a bool, return it.
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// Error type for const JSON parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    UnexpectedEndOfInput,
    InvalidSyntax,
    UnsupportedValue,
}

const fn skip_whitespace(json: &str) -> &str {
    let bytes = json.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            _ => break,
        }
    }
    unsafe_slice_str(bytes, i, bytes.len())
}

const fn parse_value(json: &str) -> Result<JsonValue<'_>, ParseError> {
    let bytes = json.as_bytes();
    if bytes.is_empty() {
        return Err(ParseError::UnexpectedEndOfInput);
    }

    match bytes[0] {
        b'{' => parse_object(json),
        b'[' => parse_array(json),
        b'"' => parse_string(json),
        b't' | b'f' => parse_bool(json),
        b'n' => parse_null(json),
        b'-' | b'0'..=b'9' => parse_number(json),
        _ => Err(ParseError::InvalidSyntax),
    }
}

const fn parse_object(json: &str) -> Result<JsonValue<'_>, ParseError> {
    // For now, just store the raw JSON string
    // We'll parse fields on-demand when get() is called
    let bytes = json.as_bytes();
    if bytes.is_empty() || bytes[0] != b'{' {
        return Err(ParseError::InvalidSyntax);
    }

    // Find matching closing brace
    let mut depth = 0;
    let mut i = 0;
    let mut in_string = false;
    let mut escape_next = false;

    while i < bytes.len() {
        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        match bytes[i] {
            b'\\' if in_string => escape_next = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let obj_str = unsafe_slice_str(bytes, 0, i + 1);
                    return Ok(JsonValue::Object(JsonObject { json: obj_str }));
                }
            }
            _ => {}
        }
        i += 1;
    }

    Err(ParseError::UnexpectedEndOfInput)
}

const fn parse_array(json: &str) -> Result<JsonValue<'_>, ParseError> {
    let bytes = json.as_bytes();
    if bytes.is_empty() || bytes[0] != b'[' {
        return Err(ParseError::InvalidSyntax);
    }

    // Find matching closing bracket
    let mut depth = 0;
    let mut i = 0;
    let mut in_string = false;
    let mut escape_next = false;

    while i < bytes.len() {
        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        match bytes[i] {
            b'\\' if in_string => escape_next = true,
            b'"' => in_string = !in_string,
            b'[' if !in_string => depth += 1,
            b']' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let arr_str = unsafe_slice_str(bytes, 0, i + 1);
                    return Ok(JsonValue::Array(JsonArray { json: arr_str }));
                }
            }
            _ => {}
        }
        i += 1;
    }

    Err(ParseError::UnexpectedEndOfInput)
}

const fn parse_string(json: &str) -> Result<JsonValue<'_>, ParseError> {
    let bytes = json.as_bytes();
    if bytes.is_empty() || bytes[0] != b'"' {
        return Err(ParseError::InvalidSyntax);
    }

    let mut i = 1;
    let mut escape_next = false;

    while i < bytes.len() {
        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        match bytes[i] {
            b'\\' => escape_next = true,
            b'"' => {
                // Found closing quote
                let str_content = unsafe_slice_str(bytes, 1, i);
                return Ok(JsonValue::String(str_content));
            }
            _ => {}
        }
        i += 1;
    }

    Err(ParseError::UnexpectedEndOfInput)
}

const fn parse_bool(json: &str) -> Result<JsonValue<'_>, ParseError> {
    let bytes = json.as_bytes();
    if bytes.len() >= 4
        && bytes[0] == b't'
        && bytes[1] == b'r'
        && bytes[2] == b'u'
        && bytes[3] == b'e'
    {
        Ok(JsonValue::Bool(true))
    } else if bytes.len() >= 5
        && bytes[0] == b'f'
        && bytes[1] == b'a'
        && bytes[2] == b'l'
        && bytes[3] == b's'
        && bytes[4] == b'e'
    {
        Ok(JsonValue::Bool(false))
    } else {
        Err(ParseError::InvalidSyntax)
    }
}

const fn parse_null(json: &str) -> Result<JsonValue<'_>, ParseError> {
    let bytes = json.as_bytes();
    if bytes.len() >= 4
        && bytes[0] == b'n'
        && bytes[1] == b'u'
        && bytes[2] == b'l'
        && bytes[3] == b'l'
    {
        Ok(JsonValue::Null)
    } else {
        Err(ParseError::InvalidSyntax)
    }
}

const fn parse_number(json: &str) -> Result<JsonValue<'_>, ParseError> {
    let bytes = json.as_bytes();
    if bytes.is_empty() {
        return Err(ParseError::UnexpectedEndOfInput);
    }

    let mut i = 0;
    let negative = if bytes[0] == b'-' {
        i += 1;
        true
    } else {
        false
    };

    let mut value: i64 = 0;
    let mut found_digit = false;

    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                let digit = (bytes[i] - b'0') as i64;
                // Use checked arithmetic to avoid silent overflow in release mode.
                value = match value.checked_mul(10) {
                    Some(v) => v,
                    None => return Err(ParseError::InvalidSyntax),
                };
                value = match value.checked_add(digit) {
                    Some(v) => v,
                    None => return Err(ParseError::InvalidSyntax),
                };
                found_digit = true;
                i += 1;
            }
            _ => break,
        }
    }

    if !found_digit {
        return Err(ParseError::InvalidSyntax);
    }

    if negative {
        value = match value.checked_neg() {
            Some(v) => v,
            None => return Err(ParseError::InvalidSyntax),
        };
    }

    Ok(JsonValue::Number(value))
}

/// Get a field from a JSON object string.
const fn get_field<'a>(json: &'a str, key: &str) -> Option<JsonValue<'a>> {
    let bytes = json.as_bytes();

    if bytes.is_empty() || bytes[0] != b'{' {
        return None;
    }

    let mut i = 1; // Skip opening brace
    let mut escape_next = false;

    while i < bytes.len() {
        // Skip whitespace
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }

        if i >= bytes.len() || bytes[i] == b'}' {
            return None;
        }

        // Expect a string key
        if bytes[i] != b'"' {
            return None;
        }

        i += 1; // Skip opening quote
        let key_start = i;

        // Find end of key
        while i < bytes.len() {
            if escape_next {
                escape_next = false;
                i += 1;
                continue;
            }

            match bytes[i] {
                b'\\' => escape_next = true,
                b'"' => break,
                _ => {}
            }
            i += 1;
        }

        let key_end = i;
        i += 1; // Skip closing quote

        // Check if this key matches
        let current_key = unsafe_slice_str(bytes, key_start, key_end);
        let matches = str_eq(current_key, key);

        // Skip whitespace and colon
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }

        if i >= bytes.len() || bytes[i] != b':' {
            return None;
        }

        i += 1; // Skip colon

        // Skip whitespace before value
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }

        if matches {
            // Found our key! Parse the value
            let value_json = unsafe_slice_str(bytes, i, bytes.len());
            return match parse_value(value_json) {
                Ok(val) => Some(val),
                Err(_) => None,
            };
        }

        // Skip this value
        i = match skip_value(bytes, i) {
            Some(next) => next,
            None => return None,
        };

        // Skip whitespace and comma
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }

        if i < bytes.len() && bytes[i] == b',' {
            i += 1;
        }
    }

    None
}

/// Skip over a JSON value and return the index after it.
const fn skip_value(bytes: &[u8], mut i: usize) -> Option<usize> {
    if i >= bytes.len() {
        return None;
    }

    match bytes[i] {
        b'"' => {
            i += 1;
            let mut escape_next = false;
            while i < bytes.len() {
                if escape_next {
                    escape_next = false;
                    i += 1;
                    continue;
                }
                match bytes[i] {
                    b'\\' => escape_next = true,
                    b'"' => return Some(i + 1),
                    _ => {}
                }
                i += 1;
            }
            None
        }
        b'{' => {
            let mut depth = 0;
            let mut in_string = false;
            let mut escape_next = false;

            while i < bytes.len() {
                if escape_next {
                    escape_next = false;
                    i += 1;
                    continue;
                }

                match bytes[i] {
                    b'\\' if in_string => escape_next = true,
                    b'"' => in_string = !in_string,
                    b'{' if !in_string => depth += 1,
                    b'}' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        b'[' => {
            let mut depth = 0;
            let mut in_string = false;
            let mut escape_next = false;

            while i < bytes.len() {
                if escape_next {
                    escape_next = false;
                    i += 1;
                    continue;
                }

                match bytes[i] {
                    b'\\' if in_string => escape_next = true,
                    b'"' => in_string = !in_string,
                    b'[' if !in_string => depth += 1,
                    b']' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        b't' => {
            if i + 4 <= bytes.len()
                && bytes[i] == b't'
                && bytes[i + 1] == b'r'
                && bytes[i + 2] == b'u'
                && bytes[i + 3] == b'e'
            {
                Some(i + 4)
            } else {
                None
            }
        }
        b'f' => {
            if i + 5 <= bytes.len()
                && bytes[i] == b'f'
                && bytes[i + 1] == b'a'
                && bytes[i + 2] == b'l'
                && bytes[i + 3] == b's'
                && bytes[i + 4] == b'e'
            {
                Some(i + 5)
            } else {
                None
            }
        }
        b'n' => {
            if i + 4 <= bytes.len()
                && bytes[i] == b'n'
                && bytes[i + 1] == b'u'
                && bytes[i + 2] == b'l'
                && bytes[i + 3] == b'l'
            {
                Some(i + 4)
            } else {
                None
            }
        }
        b'-' | b'0'..=b'9' => {
            while i < bytes.len()
                && matches!(bytes[i], b'-' | b'0'..=b'9' | b'.' | b'e' | b'E' | b'+')
            {
                i += 1;
            }
            Some(i)
        }
        _ => None,
    }
}

/// Compare two strings for equality.
const fn str_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    if a_bytes.len() != b_bytes.len() {
        return false;
    }

    let mut i = 0;
    while i < a_bytes.len() {
        if a_bytes[i] != b_bytes[i] {
            return false;
        }
        i += 1;
    }

    true
}

/// Create a string slice from a byte slice (unsafe because we can't use range indexing in const).
const fn unsafe_slice_str(bytes: &[u8], start: usize, end: usize) -> &str {
    let len = end - start;
    let ptr = bytes.as_ptr();
    unsafe {
        let slice = core::slice::from_raw_parts(ptr.add(start), len);
        core::str::from_utf8_unchecked(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_string() {
        const JSON: &str = r#""hello""#;
        const VALUE: Result<JsonValue, ParseError> = JsonValue::parse(JSON);
        assert!(matches!(VALUE, Ok(JsonValue::String("hello"))));
    }

    #[test]
    fn test_parse_number() {
        const JSON: &str = "42";
        const VALUE: Result<JsonValue, ParseError> = JsonValue::parse(JSON);
        assert!(matches!(VALUE, Ok(JsonValue::Number(42))));
    }

    #[test]
    fn test_parse_bool() {
        const JSON_TRUE: &str = "true";
        const JSON_FALSE: &str = "false";
        const VALUE_TRUE: Result<JsonValue, ParseError> = JsonValue::parse(JSON_TRUE);
        const VALUE_FALSE: Result<JsonValue, ParseError> = JsonValue::parse(JSON_FALSE);
        assert!(matches!(VALUE_TRUE, Ok(JsonValue::Bool(true))));
        assert!(matches!(VALUE_FALSE, Ok(JsonValue::Bool(false))));
    }

    #[test]
    fn test_parse_object_get_field() {
        const JSON: &str = r#"{"react": "^19.0.0", "react-dom": "^19.0.0"}"#;
        const VALUE: Result<JsonValue, ParseError> = JsonValue::parse(JSON);

        match VALUE {
            Ok(JsonValue::Object(obj)) => {
                let react_version = obj.get("react");
                match react_version {
                    Some(JsonValue::String(v)) => assert_eq!(v, "^19.0.0"),
                    _ => panic!("Expected string value"),
                }
            }
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_nested_object() {
        const JSON: &str = r#"{"dependencies": {"react": "^19.0.0"}}"#;
        const VALUE: Result<JsonValue, ParseError> = JsonValue::parse(JSON);

        match VALUE {
            Ok(JsonValue::Object(obj)) => {
                let deps = obj.get("dependencies");
                match deps {
                    Some(JsonValue::Object(deps_obj)) => {
                        let react = deps_obj.get("react");
                        match react {
                            Some(JsonValue::String(v)) => assert_eq!(v, "^19.0.0"),
                            _ => panic!("Expected string value"),
                        }
                    }
                    _ => panic!("Expected nested object"),
                }
            }
            _ => panic!("Expected object"),
        }
    }
}
