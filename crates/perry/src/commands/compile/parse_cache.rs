//! In-memory parse cache for `perry dev` rebuilds.
//!
//! Extracted from `compile.rs` (Tier 2.1 of the compiler-improvement
//! plan, v0.5.333). The cache key is the absolute file path; a
//! re-parse is skipped when the source bytes haven't changed since the
//! last call. Counters track hit / miss for diagnostics.
//!
//! Scope is strictly per-process: the cache lives for the duration of
//! one `perry dev` invocation. `perry compile` never sees it.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct ParseCache {
    pub(super) entries: HashMap<PathBuf, ParseCacheEntry>,
    hits: usize,
    misses: usize,
}

pub(super) struct ParseCacheEntry {
    pub(super) source: String,
    pub(super) module: swc_ecma_ast::Module,
}

impl ParseCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of cache hits since creation (or since `reset_counters`).
    pub fn hits(&self) -> usize {
        self.hits
    }

    /// Number of cache misses (fresh parses) since creation.
    pub fn misses(&self) -> usize {
        self.misses
    }

    /// Reset hit/miss counters. Intended to be called between dev rebuilds
    /// so the counters reflect a single rebuild rather than cumulative.
    pub fn reset_counters(&mut self) {
        self.hits = 0;
        self.misses = 0;
    }
}

/// Parse `source` via the cache: return a borrowed `&Module` from the
/// cache, reusing the last entry if its source bytes match, else
/// reparsing.
pub(super) fn parse_cached<'a>(
    cache: &'a mut ParseCache,
    path: &Path,
    source: &str,
    filename: &str,
) -> Result<&'a swc_ecma_ast::Module> {
    let fresh = cache
        .entries
        .get(path)
        .map_or(false, |e| e.source == source);
    if fresh {
        cache.hits += 1;
    } else {
        let parsed = perry_parser::parse_typescript(source, filename)
            .map_err(|e| anyhow!("Failed to parse {}: {}", path.display(), e))?;
        cache.entries.insert(
            path.to_path_buf(),
            ParseCacheEntry {
                source: source.to_string(),
                module: parsed,
            },
        );
        cache.misses += 1;
    }
    // The entry is guaranteed to exist at this point (we just inserted on miss).
    Ok(&cache.entries[path].module)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC_V1: &str = "export function greet(name: string): string { return `hi ${name}`; }\n";
    const SRC_V2: &str = "export function greet(name: string): string { return `hello ${name}`; }\n";

    #[test]
    fn first_call_is_a_miss() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn identical_source_is_a_hit() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn changed_source_is_a_miss_and_replaces_entry() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V2, "greet.ts").unwrap();
        // Two misses, zero hits; cache still holds one entry (the new version).
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 2);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[&path].source, SRC_V2);
    }

    #[test]
    fn reverting_to_previous_source_is_still_a_miss() {
        // The cache keeps only the last version, not history. Reverting to a
        // prior source counts as a miss — documented behaviour.
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V2, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 3);
    }

    #[test]
    fn distinct_paths_are_independent() {
        let mut cache = ParseCache::new();
        let p_a = PathBuf::from("/virtual/a.ts");
        let p_b = PathBuf::from("/virtual/b.ts");
        let _ = parse_cached(&mut cache, &p_a, SRC_V1, "a.ts").unwrap();
        let _ = parse_cached(&mut cache, &p_b, SRC_V1, "b.ts").unwrap();
        let _ = parse_cached(&mut cache, &p_a, SRC_V1, "a.ts").unwrap();
        let _ = parse_cached(&mut cache, &p_b, SRC_V1, "b.ts").unwrap();
        assert_eq!(cache.hits(), 2);
        assert_eq!(cache.misses(), 2);
    }

    #[test]
    fn reset_counters_clears_hit_miss_but_keeps_entries() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        cache.reset_counters();
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);
        // Next lookup for the same source should be a hit, not a miss —
        // entries survive reset_counters.
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 0);
    }

    #[test]
    fn hit_returns_equivalent_ast_to_fresh_parse() {
        // A cache hit must give us the same AST shape as reparsing from
        // scratch — this is the correctness invariant V2.1 relies on.
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let first = parse_cached(&mut cache, &path, SRC_V1, "greet.ts")
            .unwrap()
            .clone();
        let cached = parse_cached(&mut cache, &path, SRC_V1, "greet.ts")
            .unwrap()
            .clone();
        let fresh = perry_parser::parse_typescript(SRC_V1, "greet.ts").unwrap();
        assert_eq!(first.body.len(), fresh.body.len());
        assert_eq!(cached.body.len(), fresh.body.len());
    }

    #[test]
    fn parse_error_propagates_and_does_not_poison_cache() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/bad.ts");
        let err = parse_cached(&mut cache, &path, "let x: number = ;", "bad.ts");
        assert!(err.is_err());
        // A later good parse at the same path still works and is a miss.
        let ok = parse_cached(&mut cache, &path, SRC_V1, "bad.ts");
        assert!(ok.is_ok());
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);
    }
}
