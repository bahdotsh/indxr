---
id: mod-workspace
title: Workspace & Monorepo Support
page_type: module
source_files:
- src/workspace.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:27:05Z
links_to: []
covers: []
---

# Workspace & Monorepo Support

The workspace module (`src/workspace.rs`) detects and manages monorepo structures, supporting Cargo, npm, and Go workspaces.

## Detection

`detect_workspace(root)` probes for workspace configuration files in priority order:

1. **Cargo** — checks `Cargo.toml` for `[workspace]` section with `members` globs. Uses `detect_cargo_workspace()`.
2. **npm** — checks `package.json` for `workspaces` field. Uses `detect_npm_workspace()`.
3. **Go** — checks `go.work` file for `use` directives. Uses `detect_go_workspace()`.

If no workspace is detected, `single_root_workspace()` creates a single-member workspace wrapping the entire root.

## Data Types

- **`WorkspaceKind`** — enum: Cargo, Npm, Go, None
- **`Workspace`** — kind, root path, members list
- **`WorkspaceMember`** — name, root path (relative), display name

## Member Resolution

Each workspace type has its own name resolution:
- **Cargo**: reads `package.name` from member's `Cargo.toml` via `cargo_package_name()`
- **npm**: reads `name` from member's `package.json` via `npm_package_name()`
- **Go**: reads module path from `go.mod` via `go_module_name()`

Fallback: directory basename is used if manifest parsing fails.

## Glob Expansion

npm and Cargo workspaces use glob patterns for member paths (e.g., `packages/*`, `crates/**`). The `expand_glob()` function handles:
- `*` — matches any single path segment
- `**` — matches zero or more path segments (not currently used but supported)
- Character matching via `glob_match_segment()` / `glob_match_chars()`

This is a custom implementation to avoid external glob dependencies.

## Integration

- **CLI**: `indxr members` lists detected members. `--member core,cli` scopes to specific members. `--no-workspace` disables detection.
- **MCP**: most tools accept a `member` param for scoping. `list_workspace_members` tool returns all members.
- **Indexer**: `detect_and_build_workspace()` combines detection with indexing. Each member gets its own `CodebaseIndex`.

