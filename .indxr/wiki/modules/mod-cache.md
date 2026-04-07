---
id: mod-cache
title: Caching System
page_type: module
source_files:
- src/cache/mod.rs
- src/cache/fingerprint.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:26:19Z
links_to: []
covers: []
---

# Caching System

The cache (`src/cache/`) provides incremental indexing by storing parsed `FileIndex` results and only re-parsing files that have changed.

## Architecture

### Cache (`src/cache/mod.rs`)

The `Cache` struct manages the on-disk cache:
- `load(cache_dir)` — loads the binary cache from `cache.bin`, or creates an empty cache if none exists or the version doesn't match
- `get(path, mtime, hash)` — checks if a cached entry exists with matching fingerprint
- `insert(path, mtime, hash, file_index)` — stores a parsed result
- `save()` — persists to disk via `bincode`
- `hits()` / `total()` — cache hit statistics

### CacheStore (internal)

The on-disk format:
- `version` — `CACHE_VERSION` (currently 3), used to invalidate on format changes
- `entries` — `HashMap<PathBuf, CacheEntry>` mapping file paths to cached data

### CacheEntry (internal)

Per-file cache data:
- `mtime` — file modification time (seconds since epoch)
- `hash` — xxh3 content hash for collision detection
- `file_index` — the full `FileIndex` with all declarations and imports
- `size` — file size

### Fingerprinting (`src/cache/fingerprint.rs`)

Two-level validation:
1. `metadata_matches(path, cached_mtime)` — quick check if mtime has changed
2. `compute_hash(content) -> u64` — xxh3 hash of file content for definitive change detection

The mtime check is a fast path — if the mtime matches, the file hasn't changed. If the mtime differs, the content hash is computed to detect actual changes (mtime can change without content changing, e.g., after a `touch`).

## Cache Location

Default: `.indxr/cache/cache.bin` in the project root. Customizable via `--cache-dir`. Can be bypassed entirely with `--no-cache`.

## Versioning

`CACHE_VERSION` is bumped when the `FileIndex` or `Declaration` format changes. A version mismatch causes the entire cache to be discarded, ensuring stale data is never used.

