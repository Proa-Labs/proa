// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Structural value types for Client IR v3.

/// Rust fixed-width integer domain attached to checked Client IR arithmetic.
///
/// [`ClientType`] deliberately remains a description of the browser value
/// representation. For example, both `i8` and `i32` are Number-backed on the
/// client, while `i64` is BigInt-backed. That representation fact is not
/// enough to reproduce Rust overflow behavior, so accepted integer arithmetic
/// carries this separate source-language domain through lowering and emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntegerDomain {
    I32,
    U32,
    I64,
    U64,
}

impl IntegerDomain {
    /// Stable lowercase spelling used in Client IR JSON and runtime calls.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::I64 => "i64",
            Self::U64 => "u64",
        }
    }

    /// Parse the stable lowercase spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "i32" => Self::I32,
            "u32" => Self::U32,
            "i64" => Self::I64,
            "u64" => Self::U64,
            _ => return None,
        })
    }

    /// Browser representation required by this source integer domain.
    pub const fn client_type(self) -> ClientType {
        match self {
            Self::I32 => ClientType::I32,
            Self::U32 => ClientType::U32,
            Self::I64 => ClientType::I64,
            Self::U64 => ClientType::U64,
        }
    }

    /// Whether this domain uses JavaScript BigInt.
    pub const fn is_wide(self) -> bool {
        matches!(self, Self::I64 | Self::U64)
    }

    /// Whether unary negation is a valid Rust operation for this domain.
    pub const fn is_signed(self) -> bool {
        matches!(self, Self::I32 | Self::I64)
    }
}

/// Type understood by the Client IR v3 compiler pipeline.
///
/// This is deliberately smaller than Rust's type system. It models the
/// browser-client value shapes rsjs can lower to JavaScript without carrying
/// Rust ownership, trait dispatch, or monomorphization semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ClientType {
    Unit,
    Bool,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    String,
    Option(Box<ClientType>),
    Vec(Box<ClientType>),
    Object(Vec<ClientObjectField>),
    /// A nominal, helper-internal Rust value object.
    ///
    /// Unlike [`Object`](Self::Object), this shape is never a serialization
    /// contract.  It may flow only between relocation-linked helper
    /// definitions.  `type_key` prevents structurally identical Rust types
    /// from becoming interchangeable, while `fields` gives the final helper
    /// verifier enough information to validate field reads and assignments.
    ValueObject {
        type_key: String,
        fields: Vec<ClientObjectField>,
    },
    Struct(String),
    Enum(String),
    Event,
    /// A live DOM element handle. It may exist in handler-local Client IR but
    /// must not cross signal, callback, serialization, or standalone-helper
    /// boundaries.
    DomElement,
    Signal(Box<ClientType>),
    Callback(Box<ClientType>),
    /// A scalar whose JS representation (BigInt or Number) is pinned by the
    /// page seed rather than provable at macro-expansion time.
    ///
    /// Shared-state fields carry this type. A proc macro cannot resolve a Rust
    /// type, so the island macro cannot know whether `CartState::count` is
    /// `u64` (BigInt on the client) or `u32` (Number). The seed does know: the
    /// provider ships `wide_paths` derived from the real `ClientPropsMeta` by
    /// ordinary Rust name resolution, and `install_page_shared_state` pins each
    /// field cell to that representation.
    ///
    /// The compiler therefore stops guessing. Fixed-width arithmetic with an
    /// unproven `Dyn` operand is rejected because representation does not prove
    /// overflow width or signedness. Representation-aware comparison remains
    /// available, and an explicit typed shared-update boundary can replace
    /// `Dyn` with an exact checked domain before arithmetic is lowered.
    Dyn,
    /// Allowed while lowering HIR. JS emission must reject this.
    Unknown,
}

/// One field in a structural JSON object type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientObjectField {
    pub name: String,
    pub ty: ClientType,
}

impl ClientType {
    pub fn option(inner: ClientType) -> Self {
        Self::Option(Box::new(inner))
    }

    pub fn vec(inner: ClientType) -> Self {
        Self::Vec(Box::new(inner))
    }

    pub fn object(fields: impl Into<Vec<ClientObjectField>>) -> Self {
        Self::Object(fields.into())
    }

    pub fn value_object(
        type_key: impl Into<String>,
        fields: impl Into<Vec<ClientObjectField>>,
    ) -> Self {
        Self::ValueObject {
            type_key: type_key.into(),
            fields: fields.into(),
        }
    }

    pub fn signal(inner: ClientType) -> Self {
        Self::Signal(Box::new(inner))
    }

    pub fn callback(payload: ClientType) -> Self {
        Self::Callback(Box::new(payload))
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Returns `true` for [`Dyn`]: a scalar whose JS representation the page
    /// seed pins at runtime instead of the compiler proving it.
    ///
    /// [`Dyn`]: Self::Dyn
    pub fn is_dyn(&self) -> bool {
        matches!(self, Self::Dyn)
    }

    /// Client-side value shape of a [`crate::value::Value`] literal.
    ///
    /// This is the default declared type for a signal seeded with a
    /// literal, so it follows the *runtime representation* the JS
    /// emitters give that literal, not the Rust source width:
    ///
    /// - `Value::Int` lowers to a JS Number, so its representation defaults to
    ///   [`I32`] (the Number-backed integer shape). Exact `i32` versus `u32`
    ///   behavior comes from the expression's integer-domain proof.
    /// - `Value::BigInt` lowers to a JS BigInt literal, so it types
    ///   as [`I64`] (the BigInt-backed integer shape). Explicit `i64`/`u64`
    ///   source literals always use this variant, even when small.
    /// - `Value::Null` carries no shape information and types as
    ///   [`Unknown`].
    ///
    /// [`I32`]: Self::I32
    /// [`I64`]: Self::I64
    /// [`Unknown`]: Self::Unknown
    pub fn of_value(value: &crate::value::Value) -> Self {
        use crate::value::Value;
        match value {
            Value::Bool(_) => Self::Bool,
            Value::Int(_) => Self::I32,
            Value::BigInt(_) => Self::I64,
            Value::Float(_) => Self::F64,
            Value::String(_) => Self::String,
            Value::Null => Self::Unknown,
        }
    }

    /// Returns `true` for the BigInt-backed integer shapes ([`I64`] /
    /// [`U64`]): values that hydrate and compute as JS BigInt.
    ///
    /// [`I64`]: Self::I64
    /// [`U64`]: Self::U64
    pub fn is_wide_int(&self) -> bool {
        matches!(self, Self::I64 | Self::U64)
    }

    /// Returns `true` for the Number-backed integer shapes ([`I32`] /
    /// [`U32`]): values that hydrate and compute as JS Number but hold
    /// integers, so Rust `/` must lower to truncating division.
    ///
    /// [`I32`]: Self::I32
    /// [`U32`]: Self::U32
    pub fn is_number_int(&self) -> bool {
        matches!(self, Self::I32 | Self::U32)
    }

    /// Project this type through a JSON Pointer path.
    ///
    /// Structural object and array types retain their nested field shape.
    /// Nominal [`ClientType::Struct`] values do not expose fields here because
    /// the IR cannot assume a Rust struct's JSON layout without a structural
    /// metadata table.
    pub fn project_path(&self, path: &crate::client::JsonPointer) -> Self {
        let mut ty = self.clone();
        for segment in path.segments() {
            ty = ty.project_segment(&segment);
            if ty.is_unknown() {
                break;
            }
        }
        ty
    }

    fn project_segment(&self, segment: &str) -> Self {
        match self {
            // A projection out of a representation-pinned shared value is
            // itself representation-pinned: the seed's `wide_paths` name nested
            // paths (`["items", "*", "id"]`), so the runtime decides the width
            // of the projected cell exactly as it does the root's.
            Self::Dyn => Self::Dyn,
            Self::Option(inner) => inner.project_segment(segment),
            Self::Vec(inner) => {
                if segment.parse::<usize>().is_ok() {
                    inner.as_ref().clone()
                } else {
                    Self::Unknown
                }
            }
            Self::Object(fields) => fields
                .iter()
                .find(|field| field.name == segment)
                .map(|field| field.ty.clone())
                .unwrap_or(Self::Unknown),
            Self::ValueObject { fields, .. } => fields
                .iter()
                .find(|field| field.name == segment)
                .map(|field| field.ty.clone())
                .unwrap_or(Self::Unknown),
            _ => Self::Unknown,
        }
    }
}
