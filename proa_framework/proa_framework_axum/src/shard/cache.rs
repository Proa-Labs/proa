// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! In-memory LRU cache with slab allocation and mmap support

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for shard cache
#[derive(Clone)]
pub struct ShardCacheConfig {
    /// Maximum cache size in bytes (approximate)
    pub max_size_bytes: usize,

    /// Maximum number of entries
    pub max_entries: usize,

    /// Enable memory-mapped file backing for persistence
    pub mmap_enabled: bool,

    /// Path for mmap backing file (if enabled)
    pub mmap_path: Option<std::path::PathBuf>,
}

impl Default for ShardCacheConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: 32 * 1024 * 1024 * 1024, // 32GB
            max_entries: 1_000_000,
            mmap_enabled: false,
            mmap_path: None,
        }
    }
}

/// LRU cache entry
struct CacheEntry<V> {
    value: Arc<V>,
    size_bytes: usize,
    access_count: u64,
}

/// Shard-local cache with LRU eviction
pub struct ShardCache<K, V>
where
    K: Eq + Hash + Clone,
{
    entries: Arc<RwLock<LruCache<K, V>>>,
}

struct LruCache<K, V>
where
    K: Eq + Hash + Clone,
{
    map: HashMap<K, CacheEntry<V>>,
    access_order: Vec<K>,
    total_size_bytes: usize,
    config: ShardCacheConfig,
}

impl<K, V> ShardCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + Sync + 'static,
{
    pub fn new(config: ShardCacheConfig) -> Self {
        Self {
            entries: Arc::new(RwLock::new(LruCache {
                map: HashMap::new(),
                access_order: Vec::new(),
                total_size_bytes: 0,
                config: config.clone(),
            })),
        }
    }

    /// Get a value from the cache
    pub async fn get(&self, key: &K) -> Option<Arc<V>> {
        let mut cache = self.entries.write().await;
        cache.get(key)
    }

    /// Insert a value into the cache
    ///
    /// Returns true if inserted, false if cache is full and eviction failed
    pub async fn insert(&self, key: K, value: V, size_bytes: usize) -> bool {
        let mut cache = self.entries.write().await;
        cache.insert(key, value, size_bytes)
    }

    /// Get or compute a value
    ///
    /// If the key exists, return cached value. Otherwise, compute it and cache.
    pub async fn get_or_insert_with<F>(&self, key: K, size_bytes: usize, f: F) -> Arc<V>
    where
        F: FnOnce() -> V,
    {
        // Try to get from cache first
        {
            let mut cache = self.entries.write().await;
            if let Some(value) = cache.get(&key) {
                return value;
            }
        }

        // Compute value
        let value = f();

        // Insert into cache
        {
            let mut cache = self.entries.write().await;
            // Check again in case another thread inserted it
            if let Some(existing) = cache.get(&key) {
                return existing;
            }

            let arc_value = Arc::new(value);
            cache.insert_arc(key, arc_value.clone(), size_bytes);
            arc_value
        }
    }

    /// Get or compute a value asynchronously
    pub async fn get_or_insert_with_async<F, Fut>(&self, key: K, size_bytes: usize, f: F) -> Arc<V>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = V>,
    {
        // Try to get from cache first
        {
            let mut cache = self.entries.write().await;
            if let Some(value) = cache.get(&key) {
                return value;
            }
        }

        // Compute value
        let value = f().await;

        // Insert into cache
        {
            let mut cache = self.entries.write().await;
            // Check again in case another thread inserted it
            if let Some(existing) = cache.get(&key) {
                return existing;
            }

            let arc_value = Arc::new(value);
            cache.insert_arc(key, arc_value.clone(), size_bytes);
            arc_value
        }
    }

    /// Invalidate a cache entry
    pub async fn invalidate(&self, key: &K) {
        let mut cache = self.entries.write().await;
        cache.remove(key);
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.entries.read().await;
        CacheStats {
            entry_count: cache.map.len(),
            total_size_bytes: cache.total_size_bytes,
            max_entries: cache.config.max_entries,
            max_size_bytes: cache.config.max_size_bytes,
        }
    }
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone,
{
    fn get(&mut self, key: &K) -> Option<Arc<V>> {
        if let Some(entry) = self.map.get_mut(key) {
            entry.access_count += 1;

            // Move to end of access order (most recently used)
            if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                self.access_order.remove(pos);
            }
            self.access_order.push(key.clone());

            Some(entry.value.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V, size_bytes: usize) -> bool {
        let arc_value = Arc::new(value);
        self.insert_arc(key, arc_value, size_bytes)
    }

    fn insert_arc(&mut self, key: K, value: Arc<V>, size_bytes: usize) -> bool {
        // Remove old entry if exists, including its position in access_order,
        // to prevent stale duplicates that desync access_order from map and
        // cause eviction to loop forever.
        if let Some(old_entry) = self.map.remove(&key) {
            self.total_size_bytes -= old_entry.size_bytes;
            if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                self.access_order.remove(pos);
            }
        }

        // Check if we need to evict (after removing the old entry so we
        // don't spuriously evict when replacing a same-sized entry).
        while self.should_evict(size_bytes) {
            if !self.evict_lru() {
                return false; // Cache full, can't evict
            }
        }

        // Insert new entry
        self.map.insert(
            key.clone(),
            CacheEntry {
                value,
                size_bytes,
                access_count: 0,
            },
        );
        self.total_size_bytes += size_bytes;
        self.access_order.push(key);

        true
    }

    fn remove(&mut self, key: &K) {
        if let Some(entry) = self.map.remove(key) {
            self.total_size_bytes -= entry.size_bytes;
            if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                self.access_order.remove(pos);
            }
        }
    }

    fn should_evict(&self, new_entry_size: usize) -> bool {
        self.map.len() >= self.config.max_entries
            || self.total_size_bytes + new_entry_size > self.config.max_size_bytes
    }

    fn evict_lru(&mut self) -> bool {
        if self.access_order.is_empty() {
            return false;
        }

        // Remove least recently used entry
        let lru_key = self.access_order.remove(0);
        if let Some(entry) = self.map.remove(&lru_key) {
            self.total_size_bytes -= entry.size_bytes;
            true
        } else {
            false
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entry_count: usize,
    pub total_size_bytes: usize,
    pub max_entries: usize,
    pub max_size_bytes: usize,
}

impl CacheStats {
    pub fn utilization(&self) -> f64 {
        self.total_size_bytes as f64 / self.max_size_bytes as f64
    }

    pub fn entry_utilization(&self) -> f64 {
        self.entry_count as f64 / self.max_entries as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lru_eviction() {
        let cache = ShardCache::<String, String>::new(ShardCacheConfig {
            max_size_bytes: 100,
            max_entries: 3,
            mmap_enabled: false,
            mmap_path: None,
        });

        // Insert 3 entries
        cache
            .insert("key1".to_string(), "value1".to_string(), 30)
            .await;
        cache
            .insert("key2".to_string(), "value2".to_string(), 30)
            .await;
        cache
            .insert("key3".to_string(), "value3".to_string(), 30)
            .await;

        // Access key1 to make it more recently used
        cache.get(&"key1".to_string()).await;

        // Insert key4, should evict key2 (least recently used)
        cache
            .insert("key4".to_string(), "value4".to_string(), 30)
            .await;

        assert!(cache.get(&"key1".to_string()).await.is_some());
        assert!(cache.get(&"key2".to_string()).await.is_none());
        assert!(cache.get(&"key3".to_string()).await.is_some());
        assert!(cache.get(&"key4".to_string()).await.is_some());
    }
}
