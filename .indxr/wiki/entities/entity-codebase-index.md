---
id: entity-codebase-index
title: CodebaseIndex & WorkspaceIndex — Top-Level Models
page_type: entity
source_files:
- src/model/mod.rs
- src/indexer.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:22:10Z
links_to:
- mod-mcp
covers: []
---

# CodebaseIndex & WorkspaceIndex — Top-Level Models

These are the top-level container types that hold the entire parsed representation of a codebase. Defined in `src/model/mod.rs`.

## CodebaseIndex

Represents a single-root indexed codebase:

```
CodebaseIndex {
    name: String,              // Project name (from directory)
    root: PathBuf,             // Absolute path to project root
    tree: Vec<TreeEntry>,      // Directory tree structure
    files: Vec<FileIndex>,     // All parsed files
    stats: IndexStats,         // Aggregate statistics
    detail: DetailLevel,       // Summary | Signatures (default) | Full
}
```

## FileIndex

Represents one parsed source file:

```
FileIndex {
    path: PathBuf,                // Relative path from root
    language: Language,           // Detected language
    size: u64,                    // File size in bytes
    declarations: Vec<Declaration>, // All extracted declarations
    imports: Vec<Import>,         // Import/use statements
    line_count: usize,            // Total line count
}
```

## WorkspaceIndex

The workspace-aware wrapper. **All MCP server operations work on `WorkspaceIndex`**, even for single-root projects (which become a workspace with one member).

```
WorkspaceIndex {
    name: String,
    root: PathBuf,
    kind: WorkspaceKind,         // Cargo | Npm | Go | Single
    members: Vec<MemberIndex>,   // One per workspace member
    stats: IndexStats,
    detail: DetailLevel,
}
```

Key methods:
- `is_single()` → true if this is a single-root (non-workspace) project
- `flat_files()` → iterator over all `FileIndex` across all members
- `flat_files_with_prefix()` → same but with member root prepended to paths

## MemberIndex

```
MemberIndex {
    name: String,       // Member name (e.g., "core", "cli")
    root: PathBuf,      // Relative path to member root
    index: CodebaseIndex, // The actual index
}
```

## IndexStats

```
IndexStats {
    total_files: usize,
    total_lines: usize,
    languages: HashMap<String, usize>,  // Language → file count
    duration: Duration,                  // Indexing time
}
```

## DetailLevel

Controls how much information is included in output:
- `Summary` — directory tree + file list only (no declarations)
- `Signatures` (default) — declarations with signatures
- `Full` — + doc comments, line numbers, body line counts

## Data Flow

```
Walker → parse_files() → collect_results() → build_index() → CodebaseIndex
                                                                    ↓
detect_workspace() → build_workspace_index() → WorkspaceIndex (wraps CodebaseIndex per member)
                                                                    ↓
                                                        MCP server / CLI output
```

The `WorkspaceIndex` is the single source of truth for the [[mod-mcp]] server. Tools receive `&WorkspaceIndex` (or `&mut` for regeneration) and resolve queries across all members, optionally scoped by a `member` parameter.

