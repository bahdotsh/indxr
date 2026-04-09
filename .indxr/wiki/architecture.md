---
id: architecture
title: Architecture Overview
page_type: architecture
source_files:
- src/main.rs
- src/indexer.rs
- src/cli.rs
- src/model/mod.rs
- src/model/declarations.rs
generated_at_ref: ''
generated_at: 2026-04-08T20:28:01Z
links_to: []
covers: []
---

# Architecture Overview

indxr is a fast Rust codebase indexer designed for AI agents. It extracts structural maps (declarations, imports, directory trees) using tree-sitter and regex parsing across 28 languages, then serves that information through an MCP (Model Context Protocol) server or CLI output.

## Core Pipeline

The indexing pipeline follows a linear flow:

1. **Directory Walking** (`src/walker/mod.rs`) — Traverses the file tree respecting `.gitignore` rules via the `ignore` crate. Produces a list of `FileEntry` values with paths and metadata.

2. **Language Detection** (`src/languages.rs`) — Each file's extension is mapped to one of 28 `Language` variants. This determines which parser strategy to use (tree-sitter or regex).

3. **Cache Check** (`src/cache/`) — Before parsing, the cache is consulted using mtime + xxh3 hash fingerprinting. If a file hasn't changed, the cached `FileIndex` is reused. Binary format via `bincode` for speed.

4. **Parallel Parsing** (`src/parser/`, `src/indexer.rs`) — Files are parsed in parallel using `rayon`. The `ParserRegistry` dispatches to either `TreeSitterParser` (9 languages: Rust, Python, JavaScript, TypeScript, Go, Java, C, C++, QML) or `RegexParser` (19 languages). Each parser implements the `LanguageParser` trait, producing a `FileIndex` with declarations, imports, and metadata.

5. **Complexity Analysis** (`src/parser/complexity.rs`) — For tree-sitter languages, per-function complexity metrics are computed (nesting depth, branch count, line count). These feed the hotspot analysis.

6. **Filtering** (`src/filter.rs`) — `FilterOptions` can narrow results by path, declaration kind, visibility (public-only), or symbol name search.

7. **Token Budget** (`src/budget.rs`) — Progressive truncation ensures output fits within a specified token limit. Files are ranked by importance (public API density, declaration count), and lower-priority content is progressively stripped: doc comments truncated, then removed, then private declarations dropped, then children stripped, then entire files removed.

8. **Output Formatting** (`src/output/`) — The final `CodebaseIndex` is rendered as Markdown (default), JSON, or YAML.

9. **Cache Update** — Newly parsed files are written back to the cache.

## Data Model

The data model (`src/model/`) is hierarchical:

- **`WorkspaceIndex`** — Top-level container. Holds workspace metadata (kind: Cargo/npm/Go) and a list of `MemberIndex` entries. Even single-project repos use this (with one member).
- **`MemberIndex`** — A workspace member with a name, root path, and its `CodebaseIndex`.
- **`CodebaseIndex`** — The core index for a single project root. Contains the directory tree (`Vec<TreeEntry>`), file indices (`Vec<FileIndex>`), and aggregate stats (`IndexStats`).
- **`FileIndex`** — Per-file data: path, language, line count, declarations, and imports.
- **`Declaration`** — A single code entity (function, struct, class, etc.) with name, kind, visibility, signature, doc comment, line number, relationships, complexity metrics, and optional children (nested declarations).

## Serving Modes

indxr operates in several modes:

### CLI Mode (`src/main.rs`, `src/cli.rs`)
Direct command-line indexing with output to stdout or file. Supports all filtering, budgeting, and format options.

### MCP Server (`src/mcp/`)
A JSON-RPC server exposing the index through tool calls. Supports two transports:
- **stdio** (default) — reads JSON-RPC from stdin, writes to stdout
- **Streamable HTTP** (`src/mcp/http.rs`, feature-gated behind `http`) — axum-based HTTP server

The server exposes 3 compound tools by default (`find`, `summarize`, `read`), with 23 additional granular tools available via `--all-tools`. When built with `--features wiki`, 9 wiki tools are also available.

### Watch Mode (`src/watch.rs`)
Monitors the filesystem using `notify` crate with debounced re-indexing. Keeps `INDEX.md` up to date as files change. Also used by `serve --watch` for the MCP server.

## Key Design Decisions

- **Tree-sitter + regex dual strategy**: Tree-sitter provides accurate AST parsing for 9 major languages. Regex provides "good enough" extraction for 19 more languages, maximizing coverage without needing grammar files for every language.
- **Workspace-first architecture**: Everything operates through `WorkspaceIndex`, even single projects. This makes monorepo support (Cargo, npm, Go workspaces) natural.
- **Token-aware output**: The budget system means the output is always usable by LLMs regardless of codebase size.
- **Incremental caching**: Only re-parses changed files, making repeated indexing near-instant.

## Module Dependency Flow

```
main.rs → cli.rs (arg parsing)
        → indexer.rs (orchestration)
            → walker/ (file discovery)
            → languages.rs (detection)
            → cache/ (fingerprinting)
            → parser/ (tree-sitter + regex)
            → filter.rs (narrowing)
            → budget.rs (truncation)
            → output/ (formatting)
        → mcp/ (server mode)
            → tools.rs (26+ tool implementations)
            → helpers.rs (search/scoring)
            → type_flow.rs (type tracking)
            → http.rs (HTTP transport)
        → diff.rs (structural diffing)
        → dep_graph.rs (dependency graphs)
        → workspace.rs (monorepo detection)
        → watch.rs (file monitoring)
        → wiki/ (knowledge wiki)
```

