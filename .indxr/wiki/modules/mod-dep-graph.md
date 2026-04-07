---
id: mod-dep-graph
title: Dependency Graph
page_type: module
source_files:
- src/dep_graph.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:26:53Z
links_to: []
covers: []
---

# Dependency Graph

The dependency graph module (`src/dep_graph.rs`) generates file-level and symbol-level dependency graphs from the codebase index, outputting in DOT, Mermaid, or JSON formats.

## Graph Levels

### File-Level Graph
`build_file_graph()` / `build_file_graph_from_file_refs()`:
- Nodes are files, edges are import relationships
- For each file, its import statements are resolved to target files using `resolve_import()`
- Scope filtering limits the graph to a subtree (e.g., `src/mcp`)
- Depth limiting (`--graph-depth N`) prunes edges beyond N hops from scoped files

### Symbol-Level Graph
`build_symbol_graph()` / `build_symbol_graph_from_file_refs()`:
- Nodes are individual declarations (functions, structs, etc.)
- Edges represent relationships: extends, implements, contains, plus cross-file references
- Uses `collect_symbols_ext()` to flatten all declarations with their file context
- Edges are derived from `Relationship` data on declarations and cross-file import analysis

## Import Resolution

`resolve_import()` is the core resolution logic (~140 lines). It handles:
- **Absolute imports** — matches against all known file paths using stem/module name matching
- **Relative imports** — resolves `./`, `../` paths relative to the importing file
- **Language-specific patterns** — strips prefixes like `crate::`, `super::`, `self::`, `use `, `from `, `import `, `require`
- **Extension flexibility** — tries matching with and without file extensions
- **Path normalization** — converts `::`, `.` separators to `/` for cross-language matching

Helper functions: `extract_path_from_import()`, `extract_quoted_path()`, `strip_import_prefixes()`, `match_path_candidate()`.

## Data Types

- **`DepGraph`** — nodes (`Vec<GraphNode>`) and edges (`Vec<GraphEdge>`)
- **`GraphNode`** — id, label, kind (`NodeKind::File` or `NodeKind::Symbol`)
- **`GraphEdge`** — source, target, kind (`EdgeKind`: Imports, Extends, Implements, Contains)

## Output Formats

- `format_dot(graph)` — DOT language for Graphviz
- `format_mermaid(graph)` — Mermaid diagram syntax
- `format_json(graph)` — JSON with nodes and edges arrays

## Usage

- **CLI**: `indxr --graph dot`, `indxr --graph mermaid --graph-level symbol`
- **MCP**: `get_dependency_graph` tool with format, level, scope, and depth params

