// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Runtime Tailwind merge engine.
//!
//! This module contains the original, allocation-friendly merger that powers
//! the dynamic API (e.g. [`tw_merge`]). Compile-time functionality lives under
//! [`crate::compile_time`].

mod class_map;
mod config;
mod lru_cache;
mod merge;
mod parse;
mod sort_modifiers;
mod validators;

use once_cell::sync::Lazy;
use std::sync::Mutex;

pub use self::config::{ClassDefinition, ClassDefinitionEntry, Config, ConfigError};
pub use self::parse::ParsedClassName;

use self::class_map::ClassGroupUtils;
use self::lru_cache::LruCache;
use self::merge::merge_class_list;
use self::parse::ClassNameParser;
use self::sort_modifiers::ModifierSorter;

type CacheKey = String;
type CacheValue = String;

static DEFAULT_TAILWIND_MERGE: Lazy<Mutex<TailwindMerge>> = Lazy::new(|| {
    let config = Config::clone_default();
    Mutex::new(TailwindMerge::new(config))
});

/// Merge a class list using the global default Tailwind configuration.
pub fn tw_merge(class_list: &str) -> String {
    let mut merger = DEFAULT_TAILWIND_MERGE
        .lock()
        .expect("tailwind merge mutex poisoned");
    merger.merge(class_list)
}

/// Merge multiple class lists using the global default Tailwind configuration.
pub fn tw_merge_iter<I, S>(class_lists: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut merger = DEFAULT_TAILWIND_MERGE
        .lock()
        .expect("tailwind merge mutex poisoned");
    merger.merge_iter(class_lists)
}

#[derive(Clone)]
pub struct TailwindMerge {
    parser: ClassNameParser,
    sorter: ModifierSorter,
    class_utils: ClassGroupUtils,
    cache: LruCache<CacheKey, CacheValue>,
    cache_enabled: bool,
}

impl TailwindMerge {
    pub fn new(config: Config) -> Self {
        let prefix = config.prefix.clone();
        let order_sensitive_modifiers = config.order_sensitive_modifiers.clone();
        let class_utils = ClassGroupUtils::new(&config);
        let cache_size = config.cache_size;

        let parser = ClassNameParser::new(prefix.as_deref());
        let sorter = ModifierSorter::new(&order_sensitive_modifiers);
        let cache = LruCache::new(cache_size);

        Self {
            parser,
            sorter,
            class_utils,
            cache,
            cache_enabled: cache_size > 0,
        }
    }

    pub fn merge(&mut self, class_list: &str) -> String {
        if class_list.trim().is_empty() {
            return String::new();
        }

        if self.cache_enabled {
            let key = class_list.to_string();
            if let Some(result) = self.cache.get(&key) {
                return result;
            }

            let merged =
                merge_class_list(class_list, &self.parser, &self.sorter, &self.class_utils);
            self.cache.set(key, merged.clone());
            return merged;
        }

        merge_class_list(class_list, &self.parser, &self.sorter, &self.class_utils)
    }

    pub fn merge_iter<I, S>(&mut self, class_lists: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let joined = join_class_lists(class_lists);
        self.merge(&joined)
    }
}

fn join_class_lists<I, S>(class_lists: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut result = String::new();
    for part in class_lists.into_iter() {
        let part = part.as_ref();
        if part.is_empty() {
            continue;
        }
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn merges_with_default_config() {
        let mut merger = TailwindMerge::new(Config::clone_default());
        let merged = merger.merge("px-2 py-3 px-4 text-sm text-lg");
        assert_eq!(merged, "py-3 px-4 text-lg");
    }

    #[test]
    fn global_tw_merge_function_uses_cache() {
        let result1 = tw_merge("font-bold font-semibold");
        let result2 = tw_merge("font-bold font-semibold");
        assert_eq!(result1, "font-semibold");
        assert_eq!(result1, result2);
    }
}
