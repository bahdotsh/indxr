---
id: mod-walker
title: Walker Module
page_type: module
source_files:
- src/walker/mod.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:22:26Z
links_to:
- entity-language
- mod-cache
covers: []
---

# Walker Module

The walker (`src/walker/mod.rs`) handles directory traversal — the first stage of the indexing pipeline. It produces the list of files to parse and the directory tree structure.

## Key Types

```rust
struct WalkResult {
    files: Vec<FileEntry>,
    tree: Vec<TreeEntry>,  // Directory tree for output
}

struct FileEntry {
    path: PathBuf,         // Relative path from root
    abs_path: PathBuf,     // Absolute path for reading
    language: Language,    // Detected from extension
    size: u64,             // File size in bytes
    modified: SystemTime,  // Last modification time (for caching)
}
```

## Behavior

`walk_directory(root, config)` uses the `ignore` crate (same as ripgrep) for traversal:

- **Respects `.gitignore`** by default (disable with `--no-gitignore`)
- **Skips files** that don't map to a known [[entity-language]] (via `Language::from_extension`)
- **Skips files** exceeding `max_file_size` (default 512 KB, configurable)
- **Limits depth** via `max_depth` option
- **Applies exclude patterns** (e.g., `-e "vendor/**"`)
- **Builds `TreeEntry`** tree structure in parallel with file collection

## Design Notes

- The walker is intentionally simple — it doesn't read file contents or parse anything. It just discovers files and their metadata.
- `FileEntry.modified` (mtime) is used as a fast first-pass check by [[mod-cache]] before falling back to content hashing.
- The `TreeEntry` tree is used in output formatters to show the directory structure at the top of the index.
- Files are sorted by path for deterministic output ordering.

