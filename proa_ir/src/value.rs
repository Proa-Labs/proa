// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Runtime values in the IR
//!
//! These represent literal values that can appear in expressions, and the
//! values the server evaluator produces.

use crate::types::IrStr;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// A literal value in the IR
///
/// Numbers: `Int` and `Float` are one observable "JS number" type as far as
/// evaluation semantics go (see `SEMANTICS.md`); `Int` exists so authoring
/// and serialization can stay exact for integer data.
///
/// `Object` serializes as a JSON object (insertion-ordered); duplicate keys
/// follow JS object-literal semantics (the last entry wins on access).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// Floating point value
    Float(f64),
    /// String value
    String(IrStr),
    /// Array of values
    Array(Vec<Value>),
    /// Object (key-value pairs, insertion order preserved)
    Object(#[serde(with = "object_entries")] Vec<(IrStr, Value)>),
}

/// Serialize/deserialize `Object` entries as a JSON map instead of an array
/// of pairs, so serialized values look like (and round-trip as) JS objects.
mod object_entries {
    use super::{IrStr, Value};
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};

    #[allow(clippy::ptr_arg)] // serde `with` requires the exact field type
    pub fn serialize<S: Serializer>(
        entries: &Vec<(IrStr, Value)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(entries.len()))?;
        for (key, value) in entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<(IrStr, Value)>, D::Error> {
        struct EntriesVisitor;

        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = Vec<(IrStr, Value)>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    entries.push((IrStr::Owned(key), value));
                }
                Ok(entries)
            }
        }

        deserializer.deserialize_map(EntriesVisitor)
    }
}

impl Value {
    pub const fn null() -> Self {
        Self::Null
    }

    pub const fn bool(b: bool) -> Self {
        Self::Bool(b)
    }

    pub const fn int(n: i64) -> Self {
        Self::Int(n)
    }

    pub const fn float(n: f64) -> Self {
        Self::Float(n)
    }

    pub const fn string(s: &'static str) -> Self {
        Self::String(Cow::Borrowed(s))
    }

    /// Construct an object value from key/value entries.
    pub fn object(entries: impl IntoIterator<Item = (impl Into<IrStr>, Value)>) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Self::Int(n as i64)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Self::Int(n)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Self::Float(n)
    }
}

impl From<&'static str> for Value {
    fn from(s: &'static str) -> Self {
        Self::String(Cow::Borrowed(s))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::String(Cow::Owned(s))
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Self::Array(v.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => v.into(),
            None => Self::Null,
        }
    }
}
