---
name: explore-codebase-index
description: Orients exploration of the indxr repository using `.cursor/INDEX.md` as a single-file map of directory layout, declarations, imports, and cross-file structure. Use when navigating this codebase, locating symbols or modules, planning where to read code, or answering structural questions before opening many source files.
---

# Explore codebase via INDEX.md

## When to use

Apply this skill when exploring **this repository** and the user (or task) needs a structural picture: files, functions, types, classes, and imports across the project.

## Instructions

1. **Start with the index** — Read `.cursor/INDEX.md` early. It aggregates what would otherwise require many file opens: tree layout, per-file summaries, declarations, and import relationships (generated from the project indexer).

2. **Use it to route next steps** — Pick files and symbols from the index, then open only those regions (e.g. via `read_source`, symbol search, or the Read tool for edits). Avoid reading large unrelated areas when the index already answers “what lives where.”

3. **If the index looks stale** — Timestamps and counts appear at the top of `INDEX.md`. After substantial code changes, the maintainer may regenerate it (see project guidance such as `regenerate_index` / indexer docs in-repo). If sections clearly mismatch the tree on disk, say so briefly and fall back to directory listing plus targeted reads.

## Relationship to other tooling

Project rules may require indxr MCP tools for surgical queries. **INDEX.md** is the offline, whole-repo structural digest; MCP tools are ideal for precise symbol lookup and line-scoped reads. Use both: index for orientation and breadth, MCP (or direct reads) for depth on chosen symbols.

## Examples

**User:** “Where is Rust query logic for TypeScript?”

1. Open `.cursor/INDEX.md`, find the directory tree and declaration sections for `src/parser/queries/`.
2. Note `typescript.rs` (and related modules), then read only those files or the relevant symbols.

**User:** “What does the MCP server expose?”

1. Scan `INDEX.md` for `mcp`, `src/mcp.rs`, and linked declarations.
2. Drill into the named functions/types from there.
