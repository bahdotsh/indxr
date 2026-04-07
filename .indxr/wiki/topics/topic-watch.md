---
id: topic-watch
title: File Watching & Live Re-indexing
page_type: topic
source_files:
- src/watch.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:25:23Z
links_to:
- mod-mcp
covers: []
---

# File Watching & Live Re-indexing

The watch module (`src/watch.rs`) provides automatic re-indexing when source files change, keeping INDEX.md current and enabling live MCP server updates.

## Two Watch Modes

### Standalone: `indxr watch`
`run_watch(opts)` runs a loop that:
1. Builds initial workspace index and writes INDEX.md
2. Spawns a file watcher via `spawn_watcher()`
3. Blocks on `mpsc::Receiver` for change events
4. On each event, rebuilds the workspace index and writes INDEX.md
5. Debounces rapid changes (default 300ms, configurable via `--debounce-ms`)

### MCP Server: `indxr serve --watch`
The MCP server in [[mod-mcp]] uses `spawn_watcher()` to get change notifications, then re-indexes in the background and updates its in-memory `WorkspaceIndex`. The watcher runs in a separate thread and communicates via `mpsc` channel.

## spawn_watcher()

```rust
fn spawn_watcher(
    root: &Path,
    cache_dir: &Path,
    output_path: &Path,
    debounce_ms: u64,
) -> Result<(mpsc::Receiver<()>, WatchGuard)>
```

Uses `notify-debouncer-mini` (wrapping the `notify` crate with `fsevent` on macOS):
1. Creates a debounced watcher with the configured timeout
2. Watches the project root recursively
3. Filters events through `should_trigger_reindex()`:
   - Ignores changes to the output file itself (INDEX.md)
   - Ignores changes in the cache directory
   - Ignores changes in `.git/` and `.indxr/` directories
   - Ignores files without recognized language extensions
4. Sends `()` through the channel when a valid change is detected

## WatchGuard

```rust
struct WatchGuard {
    _watcher: Debouncer<RecommendedWatcher>,
}
```

RAII guard that keeps the watcher alive. When dropped, the watcher stops.

## WatchOptions

```rust
struct WatchOptions {
    root: PathBuf,
    output_path: PathBuf,
    debounce_ms: u64,
    quiet: bool,         // Suppress progress output
}
```

