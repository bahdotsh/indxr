# indxr Roadmap

Planned features and improvements for indxr, roughly in priority order.

## Completed

### Live file watching
Watch filesystem for changes and re-index automatically.
- Standalone: `indxr watch ./project -o INDEX.md`
- MCP integration: `indxr serve --watch` with auto-reindex

### Dependency graph export
File-level and symbol-level dependency graph from existing import/relationship data.
- CLI: `indxr --graph dot|mermaid|json`
- MCP: `get_dependency_graph` tool (scoped by path, file or symbol level, DOT/Mermaid/JSON output)

### Complexity metrics / hotspots
Per-function complexity metrics using tree-sitter AST analysis.
- Cyclomatic complexity, max nesting depth, parameter count
- MCP tools: `get_hotspots` (top N most complex functions), `get_health` (codebase-level summary)
- CLI: `--hotspots` flag

### PR-aware structural diffs
Structural diff for GitHub PRs — shows added/removed/modified declarations without reading raw diffs.
- CLI: `indxr diff --pr 42` (also supports `indxr diff --since <ref>`)
- MCP: `get_diff_summary` extended with optional `pr` param (resolves base branch via GitHub API)

### Cross-file type flow tracking
Track where types flow across function boundaries.
- Language-aware signature parsing for 10+ languages (Rust, Go, TypeScript, Python, Java, Kotlin, Swift, C, C++, C#)
- Extract return types (producers) and parameter types (consumers) from function signatures
- MCP tool: `get_type_flow` — given a type name, show who produces and consumes it
- Supports path filtering, field inclusion, compact mode, and result limiting

## Planned

### HTTP+SSE MCP transport
HTTP server with SSE transport alongside existing stdin/stdout JSON-RPC.
- `indxr serve --http :8080`
- Enables multi-client scenarios

### Multi-root / monorepo support
Workspace detection and per-member indexing.
- Detect workspace files (Cargo.toml workspace, package.json workspaces, go.work)
- Scope MCP tools to a specific workspace member

### Semantic code search via embeddings
Optional embedding-based search using a local model (feature-gated).
- Generate embeddings for symbol names, doc comments, and signatures at index time
- MCP tool: `semantic_search` — query by concept, returns ranked symbols
- Fallback to `search_relevant` when embeddings not available
