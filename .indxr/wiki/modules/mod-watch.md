---
id: mod-watch
title: File Watching
page_type: module
source_files:
- src/watch.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:27:32Z
links_to: []
covers: []
---

# File Watching

The watch module (`src/watch.rs`) provides live re-indexing as files change, used by both `indxr watch` and `indxr serve --watch`.

## Components

### `run_watch(opts: WatchOptions)`
The main watch loop for `indxr watch`:
1. Performs an initial index build and writes `INDEX.md`
2. Spawns a file watcher via `spawn_watcher()`
3. Blocks on the receiver channel, re-indexing on each notification
4. Writes updated `INDEX.md` after each re-index

### `spawn_watcher(root, cache_dir, output_path, debounce_ms)`
Creates a debounced file watcher using the `notify` crate:
- Uses `notify_debouncer_mini` for debounced events
- Default debounce: 300ms (configurable via `--debounce-ms`)
- Returns a `mpsc::Receiver<()>` for reindex notifications and a `WatchGuard` for cleanup
- Filters events via `should_trigger_reindex()` to avoid re-indexing on:
  - Changes to the output file itself (`INDEX.md`)
  - Changes inside the cache directory
  - Changes to non-source files (files without a recognized `Language`)

### `WatchGuard`
RAII guard holding the watcher thread handle. Ensures clean shutdown.

### `WatchOptions`
Configuration:
- `ws_config` — workspace configuration
- `output_path` — where to write `INDEX.md`
- `debounce_ms` — debounce interval
- `quiet` — suppress progress output

## MCP Server Integration

The MCP server (`src/mcp/mod.rs`) uses `spawn_watcher()` directly when started with `--watch`. Reindex events are sent as `ServerEvent::Reindex` through the event channel, triggering `regenerate_workspace_index()` and wiki store reloads. The server also supports `--wiki-auto-update` to automatically update wiki pages on file changes.

