// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared normalized signature grammar for standalone client helpers.

use std::collections::BTreeSet;
use std::fmt;

use crate::client::ClientType;

/// Exact inline JavaScript intrinsic used for Rust `String`/`Vec`/slice
/// `.len()` inside origin-free helpers.
///
/// Standalone helper validation permits this one producer-owned runtime
/// reference and rejects every ambient/arbitrary [`crate::ClientFunctionRef::Runtime`].
pub const CLIENT_HELPER_LEN_INTRINSIC_JS: &str =
    "((__v) => { if (typeof __v === 'string') return new TextEncoder().encode(__v).length; if (Array.isArray(__v)) return __v.length; throw new TypeError('rsjs .len() expected a Rust String, Vec, or slice'); })";

/// Reserved normalized-signature prefix for helper-internal value objects.
///
/// This spelling is emitted only by the closed `#[rsjs::client] mod` frontend;
/// ordinary standalone helpers cannot author it as a Rust type.  The grammar
/// is `@rsjs_object(<64-hex-key>[;<field>=<scalar-type>]*)`.
pub const CLIENT_VALUE_OBJECT_TYPE_PREFIX: &str = "@rsjs_object(";

/// Whether `name` is an intrinsic that may appear in a linked standalone
/// helper body.
pub fn is_client_helper_runtime_intrinsic(name: &str) -> bool {
    name == CLIENT_HELPER_LEN_INTRINSIC_JS
}

/// Signature position whose ownership rules are being validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientHelperTypePosition {
    Parameter,
    Return,
}

impl fmt::Display for ClientHelperTypePosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parameter => formatter.write_str("parameter"),
            Self::Return => formatter.write_str("return"),
        }
    }
}

/// Invalid normalized Rust type in a standalone-helper signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientHelperTypeError {
    Unsupported {
        rust_type: String,
        position: ClientHelperTypePosition,
    },
    NestedOption {
        rust_type: String,
        position: ClientHelperTypePosition,
    },
    UnsupportedFloat32 {
        rust_type: String,
        position: ClientHelperTypePosition,
    },
    BorrowedSliceReturn {
        rust_type: String,
    },
    BorrowedStringReturn {
        rust_type: String,
    },
}

impl fmt::Display for ClientHelperTypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported {
                rust_type,
                position,
            } => write!(
                formatter,
                "unsupported normalized standalone-helper {position} type `{rust_type}`"
            ),
            Self::NestedOption {
                rust_type,
                position,
            } => write!(
                formatter,
                "standalone-helper {position} type `{rust_type}` contains direct Option<Option<T>>"
            ),
            Self::UnsupportedFloat32 {
                rust_type,
                position,
            } => write!(
                formatter,
                "standalone-helper {position} type `{rust_type}` contains f32, whose per-operation Rust rounding is not represented by Client IR"
            ),
            Self::BorrowedSliceReturn { rust_type } => write!(
                formatter,
                "standalone-helper return type `{rust_type}` contains a borrowed slice; return Vec<T> instead"
            ),
            Self::BorrowedStringReturn { rust_type } => write!(
                formatter,
                "standalone-helper return type `{rust_type}` contains a borrowed string; return String instead"
            ),
        }
    }
}

impl std::error::Error for ClientHelperTypeError {}

/// Parse the producer-normalized Rust type grammar used by
/// `#[rsjs::client]` helper metadata.
///
/// Supported leaves are `()`, `bool`, `i32`, `i64`, `u32`, `u64`, `f64`,
/// `String`, and parameter-only `&str`. `Option<T>`, `Vec<T>`, and
/// parameter-only `&[T]` compose recursively. Direct `Option<Option<T>>` is
/// intentionally rejected to match the producer contract. Borrowed slices map
/// to [`ClientType::Vec`] because browser Client IR carries the encoded value,
/// not Rust borrow semantics. `f32` is rejected until Client IR can preserve
/// Rust's required per-operation f32 rounding rather than JavaScript f64
/// Number semantics.
pub fn parse_client_helper_type(
    rust_type: &str,
    position: ClientHelperTypePosition,
) -> Result<ClientType, ClientHelperTypeError> {
    parse_client_helper_type_inner(rust_type, position, rust_type)
}

fn parse_client_helper_type_inner(
    rust_type: &str,
    position: ClientHelperTypePosition,
    complete_type: &str,
) -> Result<ClientType, ClientHelperTypeError> {
    if let Some(object) = parse_client_value_object_type(rust_type, position) {
        return object;
    }
    if rust_type == "f32" {
        return Err(ClientHelperTypeError::UnsupportedFloat32 {
            rust_type: complete_type.to_owned(),
            position,
        });
    }
    if rust_type == "&str" && position == ClientHelperTypePosition::Return {
        return Err(ClientHelperTypeError::BorrowedStringReturn {
            rust_type: complete_type.to_owned(),
        });
    }
    let leaf = match rust_type {
        "()" => Some(ClientType::Unit),
        "bool" => Some(ClientType::Bool),
        "i32" => Some(ClientType::I32),
        "i64" => Some(ClientType::I64),
        "u32" => Some(ClientType::U32),
        "u64" => Some(ClientType::U64),
        "f64" => Some(ClientType::F64),
        "String" | "&str" => Some(ClientType::String),
        _ => None,
    };
    if let Some(leaf) = leaf {
        return Ok(leaf);
    }

    if let Some(inner) = exact_wrapper(rust_type, "Option<", '>') {
        if inner.starts_with("Option<") {
            return Err(ClientHelperTypeError::NestedOption {
                rust_type: complete_type.to_owned(),
                position,
            });
        }
        return parse_client_helper_type_inner(inner, position, complete_type)
            .map(|inner| ClientType::Option(Box::new(inner)));
    }

    if let Some(inner) = exact_wrapper(rust_type, "Vec<", '>') {
        return parse_client_helper_type_inner(inner, position, complete_type)
            .map(|inner| ClientType::Vec(Box::new(inner)));
    }

    if let Some(inner) = exact_wrapper(rust_type, "&[", ']') {
        if position == ClientHelperTypePosition::Return {
            return Err(ClientHelperTypeError::BorrowedSliceReturn {
                rust_type: complete_type.to_owned(),
            });
        }
        return parse_client_helper_type_inner(inner, position, complete_type)
            .map(|inner| ClientType::Vec(Box::new(inner)));
    }

    Err(ClientHelperTypeError::Unsupported {
        rust_type: complete_type.to_owned(),
        position,
    })
}

fn parse_client_value_object_type(
    rust_type: &str,
    position: ClientHelperTypePosition,
) -> Option<Result<ClientType, ClientHelperTypeError>> {
    if !rust_type.starts_with(CLIENT_VALUE_OBJECT_TYPE_PREFIX) {
        return None;
    }
    let invalid = || ClientHelperTypeError::Unsupported {
        rust_type: rust_type.to_owned(),
        position,
    };
    let Some(body) = rust_type
        .strip_prefix(CLIENT_VALUE_OBJECT_TYPE_PREFIX)
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Some(Err(invalid()));
    };
    let mut parts = body.split(';');
    let Some(type_key) = parts.next() else {
        return Some(Err(invalid()));
    };
    if type_key.len() != 64 || !type_key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(Err(invalid()));
    }

    let mut names = BTreeSet::new();
    let mut fields = Vec::new();
    for part in parts {
        let Some((name, field_type)) = part.split_once('=') else {
            return Some(Err(invalid()));
        };
        if !is_plain_field_name(name) || !names.insert(name) {
            return Some(Err(invalid()));
        }
        let ty = match parse_client_helper_type_inner(
            field_type,
            ClientHelperTypePosition::Return,
            field_type,
        ) {
            Ok(ty)
                if matches!(
                    ty,
                    ClientType::Bool
                        | ClientType::I32
                        | ClientType::U32
                        | ClientType::I64
                        | ClientType::U64
                        | ClientType::F64
                        | ClientType::String
                ) =>
            {
                ty
            }
            _ => return Some(Err(invalid())),
        };
        fields.push(crate::client::ClientObjectField {
            name: name.to_owned(),
            ty,
        });
    }

    Some(Ok(ClientType::ValueObject {
        type_key: type_key.to_ascii_lowercase(),
        fields,
    }))
}

fn is_plain_field_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn exact_wrapper<'a>(value: &'a str, prefix: &str, suffix: char) -> Option<&'a str> {
    let inner = value.strip_prefix(prefix)?.strip_suffix(suffix)?;
    if inner.is_empty() {
        return None;
    }
    Some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recursive_normalized_helper_types() {
        assert_eq!(
            parse_client_helper_type("Vec<Option<&[u64]>>", ClientHelperTypePosition::Parameter),
            Ok(ClientType::Vec(Box::new(ClientType::Option(Box::new(
                ClientType::Vec(Box::new(ClientType::U64))
            )))))
        );
        assert_eq!(
            parse_client_helper_type("Option<Vec<String>>", ClientHelperTypePosition::Return),
            Ok(ClientType::Option(Box::new(ClientType::Vec(Box::new(
                ClientType::String
            )))))
        );
    }

    #[test]
    fn rejects_direct_nested_option_and_slice_returns() {
        assert!(matches!(
            parse_client_helper_type(
                "Vec<Option<Option<i32>>>",
                ClientHelperTypePosition::Parameter
            ),
            Err(ClientHelperTypeError::NestedOption { .. })
        ));
        assert!(matches!(
            parse_client_helper_type("Option<Vec<&[i32]>>", ClientHelperTypePosition::Return),
            Err(ClientHelperTypeError::BorrowedSliceReturn { .. })
        ));
    }

    #[test]
    fn rejects_unsupported_integer_widths_and_malformed_wrappers() {
        for rust_type in ["u8", "Option<i16>", "Vec<u128>", "Vec<i32", "Option<>"] {
            assert!(matches!(
                parse_client_helper_type(rust_type, ClientHelperTypePosition::Parameter),
                Err(ClientHelperTypeError::Unsupported { .. })
            ));
        }
    }

    #[test]
    fn rejects_f32_and_borrowed_strings_at_every_nested_crossing() {
        for (rust_type, position) in [
            ("f32", ClientHelperTypePosition::Parameter),
            ("Option<Vec<f32>>", ClientHelperTypePosition::Return),
        ] {
            assert!(matches!(
                parse_client_helper_type(rust_type, position),
                Err(ClientHelperTypeError::UnsupportedFloat32 { .. })
            ));
        }

        assert_eq!(
            parse_client_helper_type("&str", ClientHelperTypePosition::Parameter),
            Ok(ClientType::String)
        );
        for rust_type in ["&str", "Option<Vec<&str>>"] {
            assert!(matches!(
                parse_client_helper_type(rust_type, ClientHelperTypePosition::Return),
                Err(ClientHelperTypeError::BorrowedStringReturn { .. })
            ));
        }
    }

    #[test]
    fn parses_reserved_helper_internal_value_object_types() {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let ty = parse_client_helper_type(
            &format!("@rsjs_object({key};open=bool;count=i32;label=String)"),
            ClientHelperTypePosition::Parameter,
        )
        .unwrap();
        assert_eq!(
            ty,
            ClientType::ValueObject {
                type_key: key.to_owned(),
                fields: vec![
                    crate::client::ClientObjectField {
                        name: "open".to_owned(),
                        ty: ClientType::Bool,
                    },
                    crate::client::ClientObjectField {
                        name: "count".to_owned(),
                        ty: ClientType::I32,
                    },
                    crate::client::ClientObjectField {
                        name: "label".to_owned(),
                        ty: ClientType::String,
                    },
                ],
            }
        );
    }

    #[test]
    fn rejects_malformed_or_nested_helper_internal_value_objects() {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        for rust_type in [
            "@rsjs_object(short;open=bool)",
            &format!("@rsjs_object({key};open=bool;open=i32)"),
            &format!("@rsjs_object({key};nested=Vec<i32>)"),
        ] {
            assert!(matches!(
                parse_client_helper_type(rust_type, ClientHelperTypePosition::Parameter),
                Err(ClientHelperTypeError::Unsupported { .. })
            ));
        }
    }
}
