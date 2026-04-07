---
id: topic-workspace
title: Workspace & Monorepo Support
page_type: topic
source_files:
- src/workspace.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:24:37Z
links_to:
- mod-mcp
covers: []
---

# Workspace & Monorepo Support

Workspace detection (`src/workspace.rs`) enables indxr to understand and index monorepos with multiple packages/modules as a unified `WorkspaceIndex`.

## Supported Workspace Types

```rust
enum WorkspaceKind {
    Cargo,   // Rust workspaces (Cargo.toml [workspace] members)
    Npm,     // Node.js workspaces (package.json "workspaces" field)
    Go,      // Go workspaces (go.work "use" directives)
    Single,  // Not a workspace — single root project
}
```

## Detection Flow

`detect_workspace(root)` probes for workspace manifests in order:
1. **Cargo** — Checks `Cargo.toml` for `[workspace]` section with `members` array. Expands glob patterns (e.g., `"packages/*"`). Falls back to single-package if no workspace section.
2. **npm** — Checks `package.json` for `"workspaces"` field. Supports both array and `{packages: [...]}` formats. Expands globs.
3. **Go** — Checks `go.work` for `use` directives. Parses each path and resolves to member directories.

If no workspace is detected, falls back to `single_root_workspace()` which wraps the entire project as a single member.

## Member Resolution

```rust
struct WorkspaceMember {
    name: String,      // Package name (from Cargo.toml, package.json, or go.mod)
    path: PathBuf,     // Relative path to member root
    kind: WorkspaceKind,
}

struct Workspace {
    kind: WorkspaceKind,
    root: PathBuf,
    members: Vec<WorkspaceMember>,
}
```

Member names are resolved from manifest files:
- Cargo: `[package] name` in member's `Cargo.toml`
- npm: `"name"` in member's `package.json`
- Go: Module path from member's `go.mod`

Falls back to directory name if manifest is missing.

## Indexing

`build_workspace_index()` in `src/indexer.rs`:
1. For each workspace member, builds a separate `CodebaseIndex` with the member root as the base
2. Wraps all member indices into a `WorkspaceIndex`
3. Aggregates stats across members

## MCP Integration

The [[mod-mcp]] server always works with `WorkspaceIndex`. For multi-member workspaces:
- Most tools accept a `member` parameter to scope queries to a specific member
- `resolve_indices()` maps member name to its `CodebaseIndex`
- `resolve_index_by_path()` auto-detects the member when a file path is provided
- `list_workspace_members` tool shows all detected members
- `find_file()` searches across all members

For single-root projects, the workspace abstraction is transparent — everything works as if there's one member.

## CLI Usage

```bash
indxr members                    # List detected workspace members
indxr serve --member core        # Serve only the "core" member
indxr watch --member core,cli    # Watch specific members
indxr --no-workspace             # Disable workspace detection
```

## Glob Expansion

`expand_glob()` handles patterns like `packages/*` or `crates/**`:
- Uses a custom `glob_match_segment()` / `glob_match_chars()` implementation
- Supports `*` (any chars in segment) and `**` (any depth)
- Returns sorted list of matching directories

