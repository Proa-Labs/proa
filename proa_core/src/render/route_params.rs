// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

/// One matched route parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteParam {
    key: Box<str>,
    value: Box<str>,
}

impl RouteParam {
    pub fn new(key: impl Into<Box<str>>, value: impl Into<Box<str>>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Matched dynamic route parameters available during a render.
///
/// This type is server-agnostic. Framework integrations are responsible for
/// adapting their routing model into this collection before render begins.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteParams {
    entries: Option<Arc<[RouteParam]>>,
}

impl RouteParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_vec(entries: Vec<RouteParam>) -> Self {
        if entries.is_empty() {
            Self::default()
        } else {
            Self {
                entries: Some(Arc::from(entries)),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_none()
    }

    pub fn len(&self) -> usize {
        self.iter().len()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.iter()
            .find(|entry| entry.key() == key)
            .map(RouteParam::value)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, RouteParam> {
        self.entries.as_deref().unwrap_or_default().iter()
    }
}

impl<'a> IntoIterator for &'a RouteParams {
    type Item = &'a RouteParam;
    type IntoIter = std::slice::Iter<'a, RouteParam>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl FromIterator<RouteParam> for RouteParams {
    fn from_iter<I: IntoIterator<Item = RouteParam>>(entries: I) -> Self {
        let mut entries = entries.into_iter().peekable();
        if entries.peek().is_none() {
            return Self::default();
        }
        Self {
            entries: Some(entries.collect()),
        }
    }
}
