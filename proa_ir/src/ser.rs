// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Serde bridge: serialize any `Serialize` type directly into a [`Value`].
//!
//! This is a direct `serde::Serializer` implementation targeting [`Value`],
//! not a round-trip through `serde_json::Value`. There is no intermediate
//! tree, so scope data built from application structs allocates only the
//! resulting `Value` itself.

use crate::types::IrStr;
use crate::value::Value;
use serde::ser::{self, Serialize};
use std::borrow::Cow;

/// Error produced when a Rust value cannot be represented as a [`Value`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToValueError {
    /// An integer did not fit in `i64`, the only integer representation
    /// `Value` has (its number semantics are pinned to JS numbers).
    #[error(
        "integer `{0}` does not fit in i64: proa_ir::Value stores integers as i64 \
         (JS number semantics); serialize it as a float or a string instead"
    )]
    IntOutOfRange(String),
    /// Byte arrays have no `Value` representation.
    #[error(
        "byte arrays are not supported by proa_ir::Value: serialize bytes as a \
         string (for example base64) or as a sequence of integers instead"
    )]
    BytesUnsupported,
    /// A map key was not a string.
    #[error("map keys must be strings when serializing to proa_ir::Value, got {0}")]
    KeyMustBeString(&'static str),
    /// Error raised by a `Serialize` implementation.
    #[error("{0}")]
    Custom(String),
}

impl ser::Error for ToValueError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }
}

/// Convert any [`serde::Serialize`] type directly into a [`Value`].
///
/// This replaces hand-built scope data. Before:
///
/// ```rust
/// use proa_ir::{IrStr, Value};
///
/// let name = String::from("Wetsuit");
/// let data = Value::Object(vec![
///     (IrStr::from("name"), Value::String(IrStr::from(name.clone()))),
///     (IrStr::from("price_cents"), Value::Int(24900)),
/// ]);
/// # let _ = data;
/// ```
///
/// After:
///
/// ```rust
/// use proa_ir::{to_value, Value};
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Product {
///     name: String,
///     price_cents: i64,
/// }
///
/// let product = Product {
///     name: "Wetsuit".into(),
///     price_cents: 24900,
/// };
/// let data = to_value(&product)?;
/// assert_eq!(
///     data,
///     Value::object([
///         ("name", Value::from(String::from("Wetsuit"))),
///         ("price_cents", Value::Int(24900)),
///     ]),
/// );
/// # Ok::<(), proa_ir::ToValueError>(())
/// ```
///
/// Mapping follows `serde_json` conventions adapted to the [`Value`] enum:
/// integers become [`Value::Int`] (erroring when a `u64`/`u128`/`i128` does
/// not fit in `i64`), floats become [`Value::Float`], structs and maps become
/// insertion-ordered [`Value::Object`] entries, and enums are externally
/// tagged (unit variants become strings; newtype, tuple, and struct variants
/// become single-entry objects). Map keys must be strings, and byte arrays
/// are unsupported.
pub fn to_value<T>(value: &T) -> Result<Value, ToValueError>
where
    T: ?Sized + Serialize,
{
    value.serialize(ValueSerializer)
}

/// A `serde::Serializer` whose output is a [`Value`].
///
/// Most callers should use [`to_value`]; this type is exposed for
/// `Serialize::serialize` composition.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValueSerializer;

fn int_from_u64(v: u64) -> Result<Value, ToValueError> {
    i64::try_from(v)
        .map(Value::Int)
        .map_err(|_| ToValueError::IntOutOfRange(v.to_string()))
}

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = ToValueError;

    type SerializeSeq = SerializeVec;
    type SerializeTuple = SerializeVec;
    type SerializeTupleStruct = SerializeVec;
    type SerializeTupleVariant = SerializeTupleVariant;
    type SerializeMap = SerializeMap;
    type SerializeStruct = SerializeStruct;
    type SerializeStructVariant = SerializeStructVariant;

    fn serialize_bool(self, v: bool) -> Result<Value, ToValueError> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, ToValueError> {
        Ok(Value::Int(v.into()))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, ToValueError> {
        Ok(Value::Int(v.into()))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, ToValueError> {
        Ok(Value::Int(v.into()))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, ToValueError> {
        Ok(Value::Int(v))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, ToValueError> {
        i64::try_from(v)
            .map(Value::Int)
            .map_err(|_| ToValueError::IntOutOfRange(v.to_string()))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, ToValueError> {
        Ok(Value::Int(v.into()))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, ToValueError> {
        Ok(Value::Int(v.into()))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, ToValueError> {
        Ok(Value::Int(v.into()))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, ToValueError> {
        int_from_u64(v)
    }

    fn serialize_u128(self, v: u128) -> Result<Value, ToValueError> {
        i64::try_from(v)
            .map(Value::Int)
            .map_err(|_| ToValueError::IntOutOfRange(v.to_string()))
    }

    fn serialize_f32(self, v: f32) -> Result<Value, ToValueError> {
        Ok(Value::Float(f64::from(v)))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, ToValueError> {
        Ok(Value::Float(v))
    }

    fn serialize_char(self, v: char) -> Result<Value, ToValueError> {
        Ok(Value::String(Cow::Owned(v.to_string())))
    }

    fn serialize_str(self, v: &str) -> Result<Value, ToValueError> {
        Ok(Value::String(Cow::Owned(v.to_owned())))
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Value, ToValueError> {
        Err(ToValueError::BytesUnsupported)
    }

    fn serialize_none(self) -> Result<Value, ToValueError> {
        Ok(Value::Null)
    }

    fn serialize_some<T>(self, value: &T) -> Result<Value, ToValueError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value, ToValueError> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, ToValueError> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value, ToValueError> {
        Ok(Value::String(Cow::Borrowed(variant)))
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, ToValueError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, ToValueError>
    where
        T: ?Sized + Serialize,
    {
        Ok(Value::Object(vec![(
            Cow::Borrowed(variant),
            value.serialize(self)?,
        )]))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SerializeVec, ToValueError> {
        Ok(SerializeVec {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SerializeVec, ToValueError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SerializeVec, ToValueError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SerializeTupleVariant, ToValueError> {
        Ok(SerializeTupleVariant {
            variant,
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<SerializeMap, ToValueError> {
        Ok(SerializeMap {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            pending_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SerializeStruct, ToValueError> {
        Ok(SerializeStruct {
            entries: Vec::with_capacity(len),
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SerializeStructVariant, ToValueError> {
        Ok(SerializeStructVariant {
            variant,
            entries: Vec::with_capacity(len),
        })
    }
}

/// Sequence, tuple, and tuple-struct builder for [`ValueSerializer`].
pub struct SerializeVec {
    items: Vec<Value>,
}

impl ser::SerializeSeq for SerializeVec {
    type Ok = Value;
    type Error = ToValueError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(value.serialize(ValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, ToValueError> {
        Ok(Value::Array(self.items))
    }
}

impl ser::SerializeTuple for SerializeVec {
    type Ok = Value;
    type Error = ToValueError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, ToValueError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SerializeVec {
    type Ok = Value;
    type Error = ToValueError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, ToValueError> {
        ser::SerializeSeq::end(self)
    }
}

/// Tuple-variant builder for [`ValueSerializer`] (externally tagged).
pub struct SerializeTupleVariant {
    variant: &'static str,
    items: Vec<Value>,
}

impl ser::SerializeTupleVariant for SerializeTupleVariant {
    type Ok = Value;
    type Error = ToValueError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        self.items.push(value.serialize(ValueSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Value, ToValueError> {
        Ok(Value::Object(vec![(
            Cow::Borrowed(self.variant),
            Value::Array(self.items),
        )]))
    }
}

/// Map builder for [`ValueSerializer`]. Keys must be strings.
pub struct SerializeMap {
    entries: Vec<(IrStr, Value)>,
    pending_key: Option<IrStr>,
}

impl ser::SerializeMap for SerializeMap {
    type Ok = Value;
    type Error = ToValueError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        self.pending_key = Some(key.serialize(MapKeySerializer)?);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        let key = self
            .pending_key
            .take()
            .expect("serialize_value called before serialize_key");
        self.entries.push((key, value.serialize(ValueSerializer)?));
        Ok(())
    }

    fn end(self) -> Result<Value, ToValueError> {
        Ok(Value::Object(self.entries))
    }
}

/// Struct builder for [`ValueSerializer`].
///
/// Struct field names are `&'static str`, so entries keep borrowed keys and
/// preserve declaration order.
pub struct SerializeStruct {
    entries: Vec<(IrStr, Value)>,
}

impl ser::SerializeStruct for SerializeStruct {
    type Ok = Value;
    type Error = ToValueError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        self.entries
            .push((Cow::Borrowed(key), value.serialize(ValueSerializer)?));
        Ok(())
    }

    fn end(self) -> Result<Value, ToValueError> {
        Ok(Value::Object(self.entries))
    }
}

/// Struct-variant builder for [`ValueSerializer`] (externally tagged).
pub struct SerializeStructVariant {
    variant: &'static str,
    entries: Vec<(IrStr, Value)>,
}

impl ser::SerializeStructVariant for SerializeStructVariant {
    type Ok = Value;
    type Error = ToValueError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), ToValueError>
    where
        T: ?Sized + Serialize,
    {
        self.entries
            .push((Cow::Borrowed(key), value.serialize(ValueSerializer)?));
        Ok(())
    }

    fn end(self) -> Result<Value, ToValueError> {
        Ok(Value::Object(vec![(
            Cow::Borrowed(self.variant),
            Value::Object(self.entries),
        )]))
    }
}

/// Serializer for map keys: accepts strings (and chars), rejects everything
/// else with [`ToValueError::KeyMustBeString`].
struct MapKeySerializer;

macro_rules! key_must_be_string {
    ($($method:ident: $ty:ty => $label:literal,)*) => {
        $(
            fn $method(self, _v: $ty) -> Result<IrStr, ToValueError> {
                Err(ToValueError::KeyMustBeString($label))
            }
        )*
    };
}

impl ser::Serializer for MapKeySerializer {
    type Ok = IrStr;
    type Error = ToValueError;

    type SerializeSeq = ser::Impossible<IrStr, ToValueError>;
    type SerializeTuple = ser::Impossible<IrStr, ToValueError>;
    type SerializeTupleStruct = ser::Impossible<IrStr, ToValueError>;
    type SerializeTupleVariant = ser::Impossible<IrStr, ToValueError>;
    type SerializeMap = ser::Impossible<IrStr, ToValueError>;
    type SerializeStruct = ser::Impossible<IrStr, ToValueError>;
    type SerializeStructVariant = ser::Impossible<IrStr, ToValueError>;

    fn serialize_str(self, v: &str) -> Result<IrStr, ToValueError> {
        Ok(Cow::Owned(v.to_owned()))
    }

    fn serialize_char(self, v: char) -> Result<IrStr, ToValueError> {
        Ok(Cow::Owned(v.to_string()))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<IrStr, ToValueError> {
        Ok(Cow::Borrowed(variant))
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<IrStr, ToValueError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    key_must_be_string! {
        serialize_bool: bool => "a bool",
        serialize_i8: i8 => "an integer",
        serialize_i16: i16 => "an integer",
        serialize_i32: i32 => "an integer",
        serialize_i64: i64 => "an integer",
        serialize_i128: i128 => "an integer",
        serialize_u8: u8 => "an integer",
        serialize_u16: u16 => "an integer",
        serialize_u32: u32 => "an integer",
        serialize_u64: u64 => "an integer",
        serialize_u128: u128 => "an integer",
        serialize_f32: f32 => "a float",
        serialize_f64: f64 => "a float",
        serialize_bytes: &[u8] => "bytes",
    }

    fn serialize_none(self) -> Result<IrStr, ToValueError> {
        Err(ToValueError::KeyMustBeString("an option"))
    }

    fn serialize_some<T>(self, _value: &T) -> Result<IrStr, ToValueError>
    where
        T: ?Sized + Serialize,
    {
        Err(ToValueError::KeyMustBeString("an option"))
    }

    fn serialize_unit(self) -> Result<IrStr, ToValueError> {
        Err(ToValueError::KeyMustBeString("a unit"))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<IrStr, ToValueError> {
        Err(ToValueError::KeyMustBeString("a unit struct"))
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<IrStr, ToValueError>
    where
        T: ?Sized + Serialize,
    {
        Err(ToValueError::KeyMustBeString("an enum variant"))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, ToValueError> {
        Err(ToValueError::KeyMustBeString("a sequence"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, ToValueError> {
        Err(ToValueError::KeyMustBeString("a tuple"))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, ToValueError> {
        Err(ToValueError::KeyMustBeString("a tuple struct"))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, ToValueError> {
        Err(ToValueError::KeyMustBeString("an enum variant"))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, ToValueError> {
        Err(ToValueError::KeyMustBeString("a map"))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, ToValueError> {
        Err(ToValueError::KeyMustBeString("a struct"))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, ToValueError> {
        Err(ToValueError::KeyMustBeString("an enum variant"))
    }

    fn collect_str<T>(self, value: &T) -> Result<IrStr, ToValueError>
    where
        T: ?Sized + std::fmt::Display,
    {
        Ok(Cow::Owned(value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde::Serialize;
    use std::collections::BTreeMap;

    fn s(v: &str) -> Value {
        Value::String(Cow::Owned(v.to_owned()))
    }

    #[derive(Serialize)]
    struct LineItem {
        sku: String,
        quantity: u32,
        note: Option<String>,
    }

    #[derive(Serialize)]
    struct Order {
        id: i64,
        total: f64,
        gift: bool,
        coupon: Option<String>,
        items: Vec<LineItem>,
        tags: Vec<String>,
    }

    fn sample_order() -> Order {
        Order {
            id: 42,
            total: 99.5,
            gift: false,
            coupon: None,
            items: vec![
                LineItem {
                    sku: "WSUIT-M".into(),
                    quantity: 1,
                    note: Some("expedite".into()),
                },
                LineItem {
                    sku: "GOGGLE".into(),
                    quantity: 2,
                    note: None,
                },
            ],
            tags: vec![],
        }
    }

    #[test]
    fn nested_struct_maps_to_object_tree() {
        let value = to_value(&sample_order()).unwrap();
        assert_eq!(
            value,
            Value::object([
                ("id", Value::Int(42)),
                ("total", Value::Float(99.5)),
                ("gift", Value::Bool(false)),
                ("coupon", Value::Null),
                (
                    "items",
                    Value::Array(vec![
                        Value::object([
                            ("sku", s("WSUIT-M")),
                            ("quantity", Value::Int(1)),
                            ("note", s("expedite")),
                        ]),
                        Value::object([
                            ("sku", s("GOGGLE")),
                            ("quantity", Value::Int(2)),
                            ("note", Value::Null),
                        ]),
                    ]),
                ),
                ("tags", Value::Array(vec![])),
            ]),
        );
    }

    #[test]
    fn struct_fields_preserve_declaration_order() {
        let Value::Object(entries) = to_value(&sample_order()).unwrap() else {
            panic!("expected object");
        };
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_ref()).collect();
        assert_eq!(keys, ["id", "total", "gift", "coupon", "items", "tags"]);
    }

    #[derive(Serialize)]
    enum Shape {
        Unit,
        Newtype(i32),
        Tuple(i32, String),
        Struct { x: i32, y: i32 },
    }

    #[test]
    fn enums_are_externally_tagged() {
        assert_eq!(to_value(&Shape::Unit).unwrap(), s("Unit"));
        assert_eq!(
            to_value(&Shape::Newtype(7)).unwrap(),
            Value::object([("Newtype", Value::Int(7))]),
        );
        assert_eq!(
            to_value(&Shape::Tuple(1, "two".into())).unwrap(),
            Value::object([("Tuple", Value::Array(vec![Value::Int(1), s("two")]))]),
        );
        assert_eq!(
            to_value(&Shape::Struct { x: 3, y: 4 }).unwrap(),
            Value::object([(
                "Struct",
                Value::object([("x", Value::Int(3)), ("y", Value::Int(4))]),
            )]),
        );
    }

    #[test]
    fn integers_map_to_int_within_i64_range() {
        assert_eq!(to_value(&i64::MIN).unwrap(), Value::Int(i64::MIN));
        assert_eq!(to_value(&i64::MAX).unwrap(), Value::Int(i64::MAX));
        assert_eq!(to_value(&u32::MAX).unwrap(), Value::Int(u32::MAX as i64));
        assert_eq!(to_value(&(i64::MAX as u64)).unwrap(), Value::Int(i64::MAX),);
        assert_eq!(to_value(&(-5i8)).unwrap(), Value::Int(-5));
    }

    #[test]
    fn out_of_range_integers_error() {
        let err = to_value(&u64::MAX).unwrap_err();
        assert_eq!(err, ToValueError::IntOutOfRange(u64::MAX.to_string()));
        assert!(err.to_string().contains("does not fit in i64"));

        assert!(matches!(
            to_value(&(i64::MAX as u128 + 1)).unwrap_err(),
            ToValueError::IntOutOfRange(_),
        ));
        assert!(matches!(
            to_value(&(i64::MIN as i128 - 1)).unwrap_err(),
            ToValueError::IntOutOfRange(_),
        ));
        // In-range i128/u128 still work.
        assert_eq!(to_value(&(-9i128)).unwrap(), Value::Int(-9));
        assert_eq!(to_value(&9u128).unwrap(), Value::Int(9));
    }

    #[test]
    fn floats_map_to_float() {
        assert_eq!(to_value(&2.5f64).unwrap(), Value::Float(2.5));
        assert_eq!(to_value(&2.5f32).unwrap(), Value::Float(2.5));
    }

    #[test]
    fn strings_chars_and_unit_shapes() {
        #[derive(Serialize)]
        struct Marker;
        #[derive(Serialize)]
        struct Wrapper(i32);

        assert_eq!(to_value("hi").unwrap(), s("hi"));
        assert_eq!(to_value(&'x').unwrap(), s("x"));
        assert_eq!(to_value(&()).unwrap(), Value::Null);
        assert_eq!(to_value(&Marker).unwrap(), Value::Null);
        assert_eq!(to_value(&Wrapper(11)).unwrap(), Value::Int(11));
        assert_eq!(to_value(&Option::<i32>::None).unwrap(), Value::Null);
        assert_eq!(to_value(&Some(3)).unwrap(), Value::Int(3));
        assert_eq!(
            to_value(&(1, "a")).unwrap(),
            Value::Array(vec![Value::Int(1), s("a")]),
        );
    }

    #[test]
    fn empty_containers() {
        assert_eq!(to_value(&Vec::<i32>::new()).unwrap(), Value::Array(vec![]));
        assert_eq!(
            to_value(&BTreeMap::<String, i32>::new()).unwrap(),
            Value::Object(vec![]),
        );
    }

    #[test]
    fn string_keyed_maps_become_objects() {
        let mut map = BTreeMap::new();
        map.insert("a".to_owned(), 1);
        map.insert("b".to_owned(), 2);
        assert_eq!(
            to_value(&map).unwrap(),
            Value::object([("a", Value::Int(1)), ("b", Value::Int(2))]),
        );
    }

    #[test]
    fn non_string_map_keys_error() {
        let mut map = BTreeMap::new();
        map.insert(1i32, "one");
        assert_eq!(
            to_value(&map).unwrap_err(),
            ToValueError::KeyMustBeString("an integer"),
        );
    }

    #[test]
    fn bytes_error() {
        struct Blob;
        impl Serialize for Blob {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(&[1, 2, 3])
            }
        }
        assert_eq!(to_value(&Blob).unwrap_err(), ToValueError::BytesUnsupported);
    }

    #[test]
    fn matches_serde_json_for_representative_struct() {
        #[derive(Serialize)]
        struct Page {
            order: Order,
            shape: Shape,
            shapes: Vec<Shape>,
            maybe: Option<f32>,
        }

        let page = Page {
            order: sample_order(),
            shape: Shape::Struct { x: -1, y: 2 },
            shapes: vec![Shape::Unit, Shape::Newtype(5), Shape::Tuple(6, "t".into())],
            maybe: Some(0.5),
        };

        let via_bridge = to_value(&page).unwrap();
        // `Value` serializes untagged as plain JSON, so converting the bridge
        // output to `serde_json::Value` must match serializing the struct
        // with serde_json directly.
        assert_eq!(
            serde_json::to_value(&via_bridge).unwrap(),
            serde_json::to_value(&page).unwrap(),
        );
    }
}
