---
id: mod-mcp
title: MCP Server
page_type: module
source_files:
- src/mcp/mod.rs
- src/mcp/tools.rs
- src/mcp/helpers.rs
- src/mcp/http.rs
- src/mcp/type_flow.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:25:42Z
links_to: []
covers: []
---

# MCP Server

The MCP (Model Context Protocol) server is indxr's primary interface for AI agents. It exposes the codebase index through JSON-RPC tool calls, enabling agents to explore code structures without reading full source files.

## Server Architecture (`src/mcp/mod.rs`)

### Transports

The server supports two transports via the `Transport` enum:
- **Stdio** (default) — reads newline-delimited JSON-RPC from stdin, writes responses to stdout. Used by Claude Code, Cursor, and other MCP-aware tools.
- **Streamable HTTP** (`src/mcp/http.rs`) — axum-based HTTP server, feature-gated behind `http`. Started with `indxr serve --http :8080`.

### Request Processing

The JSON-RPC flow:
1. `run_mcp_server()` — main event loop. Spawns a stdin reader thread, listens for `ServerEvent` variants (stdin lines, reindex signals, wiki updates).
2. `handle_stdin_line()` — parses a JSON-RPC line and routes it.
3. `process_jsonrpc_message()` — deserializes the request, handles batch vs single.
4. `process_jsonrpc_request()` — dispatches by method: `initialize`, `tools/list`, `tools/call`, `notifications/initialized`.
5. `handle_tools_call()` — extracts tool name and arguments, dispatches to the appropriate tool function in `tools.rs`.

### Configuration (`McpServerConfig`)
- `all_tools` — expose all 26 tools (vs 3 compound defaults)
- `watch` — enable file watching for auto-reindex
- `debounce_ms` — debounce interval for file changes
- `wiki_auto_update` — auto-update wiki on file changes
- `wiki_debounce_ms` — debounce for wiki updates
- `http_addr` — HTTP bind address (if using HTTP transport)

### Watch Integration
When `--watch` is enabled, the server spawns a file watcher that sends `ServerEvent::Reindex` events. On reindex, the workspace index is rebuilt and the wiki store is reloaded. Coalescing logic merges rapid reindex events.

## Tool System (`src/mcp/tools.rs`)

### Tool Tiers

**3 Compound Tools** (always available):
- `find` — multi-mode search: relevant (default), symbol, callers, signature
- `summarize` — file overview, batch glob summaries, or symbol explanation
- `read` — read source by symbol name or line range (200-line cap per symbol, 500 total)

**23 Granular Tools** (with `--all-tools`):
Includes `lookup_symbol`, `list_declarations`, `search_signatures`, `get_tree`, `get_file_summary`, `read_source`, `get_file_context`, `search_relevant`, `batch_file_summaries`, `get_callers`, `get_public_api`, `explain_symbol`, `get_hotspots`, `get_health`, `get_type_flow`, `get_dependency_graph`, `get_diff_summary`, `get_token_estimate`, `list_workspace_members`, `regenerate_index`, `get_stats`, `get_imports`, `get_related_tests`.

**9 Wiki Tools** (with `--features wiki`):
`wiki_search`, `wiki_read`, `wiki_status`, `wiki_contribute`, `wiki_generate`, `wiki_update`, `wiki_suggest_contribution`, `wiki_compound`, `wiki_record_failure`.

### Tool Dispatch
`handle_tool_call()` is a large match statement routing tool names to their implementations. Each tool function takes the `WorkspaceIndex` (and optionally config/registry) plus the JSON arguments, and returns a JSON `Value`.

### Workspace Awareness
Most tools accept an optional `member` parameter to scope queries to a specific workspace member. `resolve_indices()` handles member resolution — returning all members if none specified, or filtering to the requested one.

## Helpers (`src/mcp/helpers.rs`)

Shared utilities for the MCP tools:
- Search and scoring functions (multi-signal relevance ranking)
- Glob matching for batch operations
- String manipulation helpers
- Compact output formatting (`{columns, rows}` format)

## Type Flow Analysis (`src/mcp/type_flow.rs`)

Implements the `get_type_flow` tool — tracks where a type flows across function boundaries by analyzing signatures. Shows which functions accept, return, or internally use a given type.

