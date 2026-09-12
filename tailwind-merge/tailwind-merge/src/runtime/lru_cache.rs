// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

#[derive(Clone)]
pub struct LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    max_size: usize,
    keys: VecDeque<K>,
    map: HashMap<K, V>,
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            keys: VecDeque::with_capacity(max_size.max(1)),
            map: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<V> {
        if self.max_size < 1 {
            return None;
        }

        if self.map.contains_key(key) {
            self.promote(key);
            self.map.get(key).cloned()
        } else {
            None
        }
    }

    pub fn set(&mut self, key: K, value: V) {
        if self.max_size < 1 {
            return;
        }

        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            self.promote(&key);
            return;
        }

        if self.keys.len() == self.max_size {
            if let Some(old_key) = self.keys.pop_front() {
                self.map.remove(&old_key);
            }
        }

        self.keys.push_back(key.clone());
        self.map.insert(key, value);
    }

    fn promote(&mut self, key: &K) {
        if let Some(pos) = self.keys.iter().position(|existing| existing == key) {
            if pos + 1 == self.keys.len() {
                return;
            }
            let owned_key = self.keys.remove(pos).expect("valid index");
            self.keys.push_back(owned_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LruCache;

    #[test]
    fn respects_capacity() {
        let mut cache = LruCache::new(1);
        cache.set("one", 1);
        cache.set("two", 2);
        assert!(cache.get(&"one").is_none());
        assert_eq!(cache.get(&"two"), Some(2));
    }
}
