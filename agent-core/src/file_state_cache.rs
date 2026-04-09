//! File state cache — LRU cache tracking file contents and timestamps.
//!
//! Mirrors `src/utils/fileStateCache.ts`. Provides an in-memory cache of
//! recently accessed files for optimizing re-reads and detecting changes.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Cached state of a single file.
#[derive(Debug, Clone)]
pub struct FileState {
    pub content: String,
    pub timestamp: u64,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub is_partial_view: bool,
}

/// LRU-based file state cache with configurable size limits.
#[derive(Debug)]
pub struct FileStateCache {
    entries: HashMap<PathBuf, FileState>,
    access_order: Vec<PathBuf>,
    max_entries: usize,
    max_size_bytes: usize,
    current_size_bytes: usize,
}

impl FileStateCache {
    /// Create a new cache with default limits (100 entries, 25MB).
    pub fn new() -> Self {
        Self::with_limits(100, 25 * 1024 * 1024)
    }

    /// Create a cache with custom limits.
    pub fn with_limits(max_entries: usize, max_size_bytes: usize) -> Self {
        FileStateCache {
            entries: HashMap::new(),
            access_order: Vec::new(),
            max_entries,
            max_size_bytes,
            current_size_bytes: 0,
        }
    }

    /// Normalize a file path for consistent cache keys.
    fn normalize_path(path: &Path) -> PathBuf {
        // Resolve to canonical form, falling back to the original
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    /// Get a cached file state.
    pub fn get(&mut self, path: &Path) -> Option<&FileState> {
        let key = Self::normalize_path(path);
        if self.entries.contains_key(&key) {
            // Move to end of access order (most recently used)
            self.access_order.retain(|p| *p != key);
            self.access_order.push(key.clone());
            self.entries.get(&key)
        } else {
            None
        }
    }

    /// Cache a file state.
    pub fn set(&mut self, path: &Path, state: FileState) {
        let key = Self::normalize_path(path);
        let content_size = state.content.len();

        // Remove existing entry if present
        if let Some(old) = self.entries.remove(&key) {
            self.current_size_bytes -= old.content.len();
            self.access_order.retain(|p| *p != key);
        }

        // Evict until we have room
        while (self.entries.len() >= self.max_entries
            || self.current_size_bytes + content_size > self.max_size_bytes)
            && !self.access_order.is_empty()
        {
            if let Some(oldest) = self.access_order.first().cloned() {
                if let Some(evicted) = self.entries.remove(&oldest) {
                    self.current_size_bytes -= evicted.content.len();
                }
                self.access_order.remove(0);
            }
        }

        self.current_size_bytes += content_size;
        self.access_order.push(key.clone());
        self.entries.insert(key, state);
    }

    /// Check if a file is cached.
    pub fn has(&self, path: &Path) -> bool {
        let key = Self::normalize_path(path);
        self.entries.contains_key(&key)
    }

    /// Remove a file from the cache.
    pub fn delete(&mut self, path: &Path) {
        let key = Self::normalize_path(path);
        if let Some(removed) = self.entries.remove(&key) {
            self.current_size_bytes -= removed.content.len();
            self.access_order.retain(|p| *p != key);
        }
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
        self.current_size_bytes = 0;
    }

    /// Get all cached file paths.
    pub fn keys(&self) -> Vec<&PathBuf> {
        self.entries.keys().collect()
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Total cached size in bytes.
    pub fn size_bytes(&self) -> usize {
        self.current_size_bytes
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clone the entire cache.
    pub fn clone_cache(&self) -> Self {
        FileStateCache {
            entries: self.entries.clone(),
            access_order: self.access_order.clone(),
            max_entries: self.max_entries,
            max_size_bytes: self.max_size_bytes,
            current_size_bytes: self.current_size_bytes,
        }
    }
}

impl Default for FileStateCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn test_set_and_get() {
        let mut cache = FileStateCache::new();
        let path = Path::new("/tmp/test.rs");
        cache.set(path, FileState {
            content: "hello".to_string(),
            timestamp: now(),
            offset: None,
            limit: None,
            is_partial_view: false,
        });
        assert!(cache.has(path));
        assert_eq!(cache.get(path).unwrap().content, "hello");
    }

    #[test]
    fn test_eviction() {
        let mut cache = FileStateCache::with_limits(2, 1024 * 1024);
        let t = now();

        cache.set(Path::new("/a"), FileState { content: "a".into(), timestamp: t, offset: None, limit: None, is_partial_view: false });
        cache.set(Path::new("/b"), FileState { content: "b".into(), timestamp: t, offset: None, limit: None, is_partial_view: false });
        cache.set(Path::new("/c"), FileState { content: "c".into(), timestamp: t, offset: None, limit: None, is_partial_view: false });

        assert_eq!(cache.len(), 2);
        assert!(!cache.has(Path::new("/a"))); // evicted
        assert!(cache.has(Path::new("/b")));
        assert!(cache.has(Path::new("/c")));
    }

    #[test]
    fn test_size_limit_eviction() {
        // 10 byte limit
        let mut cache = FileStateCache::with_limits(100, 10);
        let t = now();

        cache.set(Path::new("/a"), FileState { content: "12345".into(), timestamp: t, offset: None, limit: None, is_partial_view: false });
        cache.set(Path::new("/b"), FileState { content: "12345".into(), timestamp: t, offset: None, limit: None, is_partial_view: false });
        // Should evict /a to make room
        cache.set(Path::new("/c"), FileState { content: "12345".into(), timestamp: t, offset: None, limit: None, is_partial_view: false });

        assert!(!cache.has(Path::new("/a")));
        assert_eq!(cache.len(), 2);
    }
}
