// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

/// Failure while loading request-scoped render data.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoadError {
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("load timed out")]
    Timeout,
    #[error("failed to deserialize loaded data: {0}")]
    Deserialize(String),
    #[error("load request was dropped")]
    Dropped,
    #[error("load cache miss")]
    CacheMiss,
    #[error("loaded data has wrong shape: expected {expected}, got {actual}")]
    TypeMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("no data loader configured for this render")]
    NoLoader,
    #[error("{0}")]
    Custom(String),
}

impl LoadError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self::Custom(message.into())
    }

    pub fn deserialize(message: impl Into<String>) -> Self {
        Self::Deserialize(message.into())
    }
}

impl From<serde_json::Error> for LoadError {
    fn from(value: serde_json::Error) -> Self {
        Self::deserialize(value.to_string())
    }
}
