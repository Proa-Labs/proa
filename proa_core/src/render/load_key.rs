// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

/// Request-scoped identity for render-time data loading.
///
/// `LoadKey` answers "what data is this?" rather than "how should it be
/// cached?". Framework dedupe and pre-send treat it as pure identity. Cache
/// characteristics should normally live in the custom `DataLoader` or in a
/// future separate hints/request layer, not inside the key itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LoadKey {
    namespace: String,
    kind: LoadKeyKind,
}

/// Concrete representation of a [`LoadKey`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LoadKeyKind {
    Named(String),
    Hashed(u64),
}

impl LoadKey {
    pub fn named(namespace: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            kind: LoadKeyKind::Named(key.into()),
        }
    }

    pub fn typed<K: Hash>(namespace: impl Into<String>, key: K) -> Self {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        Self {
            namespace: namespace.into(),
            kind: LoadKeyKind::Hashed(hasher.finish()),
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn kind(&self) -> &LoadKeyKind {
        &self.kind
    }
}

impl fmt::Display for LoadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LoadKeyKind::Named(key) => write!(f, "{}:{}", self.namespace, key),
            LoadKeyKind::Hashed(hash) => write!(f, "{}#{hash:016x}", self.namespace),
        }
    }
}
