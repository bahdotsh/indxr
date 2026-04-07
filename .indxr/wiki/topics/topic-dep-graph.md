---
id: topic-dep-graph
title: Dependency Graph
page_type: topic
source_files:
- src/dep_graph.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:24:54Z
links_to:
- entity-declaration
covers: []
---

# Dependency Graph

The dependency graph module (`src/dep_graph.rs`) generates file-level and symbol-level dependency graphs from import relationships and declaration metadata.

## Graph Model

```rust
struct DepGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

struct GraphNode {
    id: String,       // File path or symbol ID
    label: String,    // Display name
    kind: NodeKind,   // File or Symbol
}

struct GraphEdge {
    from: String,
    to: String,
    kind: EdgeKind,   // Imports, Implements, Extends, Contains
}
```

## File-Level Graph

`build_file_graph_from_file_refs(files, scope, depth)`:
1. Builds a `PathInfo` lookup table from all files
2. For each file's imports, calls `resolve_import()` to find which file the import points to
3. Creates `EdgeKind::Imports` edges between files
4. Optionally scopes to a directory and limits edge hop depth

### Import Resolution

`resolve_import()` is the core resolution logic — it maps import text (e.g., `use crate::model::FileIndex`, `import { Config } from './config'`) to actual file paths:

1. `extract_path_from_import()` extracts the path portion from import text
2. `normalize_import_separators()` converts `::` and `.` to `/`
3. `strip_import_prefixes()` removes language prefixes (`crate/`, `super/`, `self/`, `@`)
4. `match_path_candidate()` matches the normalized path against known file paths (stem-based, case-insensitive)
5. Fallback: `resolve_relative_import()` handles `./` and `../` relative paths
6. Fallback: `find_from_keyword()` handles Python/JS `from X import Y` syntax

This works across languages because import paths generally follow similar conventions (module paths that map to file paths).

## Symbol-Level Graph

`build_symbol_graph_from_file_refs(files, scope, depth)`:
1. `collect_symbols_ext()` builds a flat list of all symbols with their files
2. Creates `EdgeKind::Contains` edges (file → symbol)
3. Creates `EdgeKind::Implements` / `EdgeKind::Extends` edges from [[entity-declaration]] relationships
4. Optionally limits depth from seed nodes

## Depth Limiting

`limit_depth_file()` and `limit_depth_symbol()` implement BFS-based depth limiting from seed nodes (files/symbols within the scope path). Only nodes reachable within `max_depth` hops are included.

## Output Formats

Three formatters:
- `format_dot()` — Graphviz DOT format
- `format_mermaid()` — Mermaid diagram format (default in MCP)
- `format_json()` — JSON with `nodes` and `edges` arrays

## Exposed Via

- CLI: `indxr --graph dot|mermaid|json [--graph-level file|symbol] [--graph-depth N] [--filter-path PATH]`
- MCP: `get_dependency_graph` tool with `format`, `level`, `path`, `depth` parameters

