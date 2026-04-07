---
id: mod-model
title: Data Model
page_type: module
source_files:
- src/model/mod.rs
- src/model/declarations.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:24:56Z
links_to: []
covers: []
---

# Data Model

The data model defines the core types that flow through the entire pipeline — from parsing through filtering, budgeting, and output.

## Hierarchy

### WorkspaceIndex (`src/model/mod.rs`)
Top-level container for all indexed data. Fields:
- `workspace_kind` — `WorkspaceKind` enum (Cargo, Npm, Go, None)
- `members` — `Vec<MemberIndex>` — one per workspace member
- `root` — workspace root path
- `tree` — `Vec<TreeEntry>` — directory tree
- `stats` — `IndexStats` — aggregate statistics
- `name` — workspace/project name

Even single-project repos use `WorkspaceIndex` with a single member. This uniform structure simplifies all downstream code.

### MemberIndex (`src/model/mod.rs`)
A workspace member:
- `name` — member name (e.g., "core", "cli")
- `root` — member root path relative to workspace
- `index` — the member's `CodebaseIndex`

### CodebaseIndex (`src/model/mod.rs`)
The core index for a single project:
- `name` — project name
- `root` — root path
- `tree` — `Vec<TreeEntry>` — directory tree
- `files` — `Vec<FileIndex>` — per-file indices
- `stats` — `IndexStats` — file count, lines, duration, cache hits
- `detail_level` — `DetailLevel` (Summary, Signatures, Full)

### FileIndex (`src/model/mod.rs`)
Per-file data:
- `path` — file path relative to root
- `language` — `Language` enum
- `lines` — line count
- `declarations` — `Vec<Declaration>` — all extracted declarations
- `imports` — `Vec<Import>` — import statements
- `size` — file size in bytes

### Declaration (`src/model/declarations.rs`)
A single code entity. This is the richest type in the model:
- `name` — declaration name
- `kind` — `DeclKind` (25 variants: Function, Struct, Class, Interface, Enum, Trait, Impl, Module, Const, TypeAlias, Field, Variant, Method, Property, etc.)
- `visibility` — `Visibility` (Public, Private, Crate)
- `signature` — full signature string
- `doc_comment` — documentation comment
- `line` — line number
- `children` — `Vec<Declaration>` — nested declarations (methods in a class, fields in a struct)
- `relationships` — `Vec<Relationship>` — extends, implements, contains
- `is_test`, `is_async`, `is_deprecated` — boolean flags
- `complexity` — `Option<ComplexityMetrics>` — nesting depth, branches, lines
- `body_lines` — number of lines in the body

### Supporting Types
- `Import` — import path string
- `TreeEntry` — directory tree node (name, children, is_dir)
- `IndexStats` — file count, total lines, duration, cache hits
- `ComplexityMetrics` — nesting_depth, branches, lines
- `Relationship` — target name + `RelKind` (Extends, Implements)
- `DeclKind` — 25-variant enum covering all supported declaration types
- `Visibility` — Public, Private, Crate
- `DetailLevel` — Summary, Signatures (default), Full

## Serialization

All model types derive `Serialize` and `Deserialize` (serde), enabling:
- JSON output via `serde_json`
- YAML output via the YAML formatter
- Binary cache storage via `bincode`
- MCP tool responses as JSON values

