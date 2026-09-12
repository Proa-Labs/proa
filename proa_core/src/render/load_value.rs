// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::any::Any;
use std::sync::Arc;

use bytes::Bytes;

use crate::render::LoadError;

/// Shape-agnostic value returned by a [`crate::DataLoader`].
///
/// `Bytes` preserves the existing JSON/bytes path. `Typed` lets loaders cache
/// already-materialized Rust values without serializing and deserializing them
/// inside a request or cross-request cache.
#[derive(Clone)]
pub enum LoadValue {
    Bytes(Bytes),
    Typed {
        value: Arc<dyn Any + Send + Sync>,
        type_name: &'static str,
    },
}

impl std::fmt::Debug for LoadValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(bytes) => f.debug_tuple("Bytes").field(bytes).finish(),
            Self::Typed { type_name, .. } => f
                .debug_struct("Typed")
                .field("type_name", type_name)
                .finish_non_exhaustive(),
        }
    }
}

impl LoadValue {
    pub fn bytes(bytes: Bytes) -> Self {
        Self::Bytes(bytes)
    }

    pub fn typed<T>(value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::arc(Arc::new(value))
    }

    pub fn arc<T>(value: Arc<T>) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::Typed {
            value,
            type_name: std::any::type_name::<T>(),
        }
    }

    pub fn into_bytes(self) -> Result<Bytes, LoadError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::Typed { type_name, .. } => Err(LoadError::TypeMismatch {
                expected: "bytes",
                actual: type_name,
            }),
        }
    }

    pub fn downcast<T>(self) -> Result<Arc<T>, LoadError>
    where
        T: Send + Sync + 'static,
    {
        match self {
            Self::Typed { value, type_name } => {
                value.downcast::<T>().map_err(|_| LoadError::TypeMismatch {
                    expected: std::any::type_name::<T>(),
                    actual: type_name,
                })
            }
            Self::Bytes(_) => Err(LoadError::TypeMismatch {
                expected: std::any::type_name::<T>(),
                actual: "bytes",
            }),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bytes(_) => "bytes",
            Self::Typed { type_name, .. } => type_name,
        }
    }
}

impl From<Bytes> for LoadValue {
    fn from(value: Bytes) -> Self {
        Self::bytes(value)
    }
}
