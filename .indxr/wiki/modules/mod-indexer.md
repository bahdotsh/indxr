---
id: mod-indexer
title: Indexer — Core Orchestration
page_type: module
source_files:
- src/indexer.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:25:10Z
links_to: []
covers: []
---

# Indexer — Core Orchestration

The indexer module (`src/indexer.rs`) orchestrates the entire indexing pipeline, connecting directory walking, parsing, caching, and output generation.

## Key Functions

### `build_index(config: &IndexConfig) -> CodebaseIndex`
The main single-project indexing function:
1. Walks the directory tree via `walker::walk_directory`
2. Loads the cache
3. Calls `parse_files()` for parallel parsing (with cache hits)
4. Calls `collect_results()` to merge results and update cache
5. Saves the updated cache
6. Returns the assembled `CodebaseIndex`

### `parse_files(files, cache, registry) -> Vec<ParseResult>`
Parallel file parsing using rayon's `par_iter()`. For each file:
- Check the cache via fingerprint (mtime + xxh3 hash)
- If cached, return the cached `FileIndex`
- Otherwise, read the file, detect language, parse with `ParserRegistry`, and return the new `FileIndex`

### `collect_results(results, cache) -> (Vec<FileIndex>, cache_hits, lang_counts, total_lines)`
Merges parse results, counts cache hits, aggregates per-language file counts and total line counts. Updates the cache with newly parsed entries.

### `build_workspace_index(ws_config: &WorkspaceConfig) -> WorkspaceIndex`
Indexes an entire workspace by iterating over members and calling `build_index` for each. Aggregates stats across all members.

### `detect_and_build_workspace(root, config, no_workspace, member_filter) -> (WorkspaceIndex, WorkspaceConfig)`
Entry point that combines workspace detection with indexing. Detects workspace type (Cargo/npm/Go), optionally filters to specific members, and builds the full index.

### `regenerate_workspace_index(ws_config: &WorkspaceConfig) -> WorkspaceIndex`
Re-indexes using an existing `WorkspaceConfig`. Used by the MCP server's `regenerate_index` tool and the watch mode to refresh the index without re-detecting the workspace.

### `generate_workspace_markdown(ws_index: &WorkspaceIndex) -> String`
Renders the workspace index as Markdown, handling both single-member and multi-member workspaces. Used by the watch mode to write `INDEX.md`.

## Configuration

### `IndexConfig`
Controls indexing behavior:
- `root` — project root path
- `detail_level` — Summary, Signatures, or Full
- `max_depth` — directory traversal depth limit
- `max_file_size` — skip files larger than N KB
- `exclude_patterns` — glob patterns to exclude
- `no_gitignore` — ignore `.gitignore` rules

### `WorkspaceConfig`
Workspace-level configuration:
- `index_config` — the base `IndexConfig`
- `workspace` — detected `Workspace` (kind, members, root)

