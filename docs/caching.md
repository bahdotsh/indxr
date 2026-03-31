# Caching

indxr uses an incremental binary cache to avoid re-parsing unchanged files, making subsequent runs significantly faster.

## How It Works

On each run, indxr:

1. **Loads** the cache from `.indxr-cache/cache.bin`
2. **Checks** each file against the cache using a two-tier validation:
   - **Quick check:** file modification time (`mtime`) + file size — if both match, cache hit
   - **Fallback:** xxh3 content hash — catches cases where metadata changed but content didn't (e.g., `touch` on a file)
3. **Parses** only files that missed the cache
4. **Prunes** entries for files that no longer exist
5. **Saves** the updated cache (only if something changed)

## Cache Location

By default, the cache is stored in `.indxr-cache/` in the indexed directory:

```
my-project/
  .indxr-cache/
    cache.bin
  src/
    ...
```

### Custom Location

```bash
indxr --cache-dir /tmp/indxr-cache
indxr --cache-dir ~/.cache/indxr/my-project
```

### Gitignore

Add `.indxr-cache/` to your `.gitignore`:

```
.indxr-cache/
```

## Disabling Cache

```bash
indxr --no-cache
```

This creates a no-op cache that never hits. Useful for benchmarking or when you want a guaranteed fresh parse.

## Cache Format

The cache uses [bincode](https://docs.rs/bincode/) for binary serialization. Each entry stores:

- **File path** (relative)
- **Modification time** (seconds since Unix epoch)
- **File size** (bytes)
- **Content hash** (xxh3_64)
- **Parsed index** (the full `FileIndex` struct)

The cache file includes a version marker. If the version doesn't match (e.g., after an indxr upgrade that changes the data model), the cache is discarded and rebuilt from scratch.

## Performance Impact

| Codebase | Files | Cold (no cache) | Warm (cached) | Speedup |
|----------|-------|-----------------|---------------|---------|
| Small (indxr, 47 files) | 19K lines | 17ms | 5ms | 3.4x |
| Medium (132 files) | 22K lines | 20ms | 6ms | 3.3x |
| Large (243 files) | 124K lines | 73ms | ~10ms | 7.3x |

The speedup increases with codebase size since more files can be skipped on cache hits.

## When the Cache Rebuilds

The cache rebuilds entries when:
- A file's content changes (detected via mtime/size or hash)
- A new file is added
- indxr is upgraded and the cache format version changes
- You run with `--no-cache` (cache is bypassed entirely)

The cache prunes entries when:
- A file is deleted from the project

## Cache and MCP Server

The MCP server (`indxr serve`) also uses the cache. It indexes the project once at startup and serves from the in-memory index. The on-disk cache accelerates this startup parse.
