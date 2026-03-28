# MCP Server

indxr includes a built-in [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server that lets AI agents query the codebase index on-demand.

Two transports are available:
- **stdio** (default) — JSON-RPC 2.0 over stdin/stdout, for single-client use
- **Streamable HTTP** — HTTP server with SSE support, for multi-client scenarios (requires `--features http`)

## Starting the Server

### stdio transport (default)

```bash
indxr serve ./my-project
```

### Streamable HTTP transport

```bash
# Build with HTTP support
cargo install indxr --features http

# Start on a specific address
indxr serve --http 127.0.0.1:8080     # recommended: localhost only
indxr serve --http :8080              # shorthand for 127.0.0.1:8080 (localhost)
indxr serve --http :8080 --watch      # with auto-reindex on file changes
```

> **Security note:** The HTTP transport is intended for **local or trusted-network use only**. It does not provide TLS (connections are plaintext), CORS headers, or authentication beyond session IDs. The `:PORT` shorthand binds to `127.0.0.1` (localhost only). To expose on all interfaces, use `0.0.0.0:PORT` explicitly. Do not expose the server to the public internet.

The HTTP transport implements the MCP Streamable HTTP specification (2025-03-26) with a single `/mcp` endpoint:
- **POST /mcp** — send JSON-RPC requests, receive JSON responses
- **GET /mcp** — open an SSE stream for server-initiated notifications (file change events)
- **DELETE /mcp** — terminate a session

Sessions are enforced: the first request must be `initialize`, which returns an `Mcp-Session-Id` header. All subsequent requests must include this header. Sessions use a **sliding-window TTL** (1 hour of inactivity) — each valid POST request refreshes the timer, so active sessions do not expire. Up to 1000 concurrent sessions are supported; expired sessions are evicted lazily.

> **Note:** The GET SSE stream does not refresh the session TTL. Clients that open an SSE stream must continue sending periodic POST requests (e.g., `ping` or tool calls) to keep the session alive. If no POST is made within the TTL window, the session expires and the SSE stream is closed.

### Server Options

```
indxr serve [PATH] [OPTIONS]

Arguments:
  [PATH]  Root directory to index [default: .]

Options:
  --cache-dir <DIR>          Cache directory [default: .indxr-cache]
  --max-file-size <KB>       Skip files larger than N KB [default: 512]
  --max-depth <N>            Maximum directory depth
  -e, --exclude <PATTERNS>   Glob patterns to exclude
  --no-gitignore             Don't respect .gitignore
  --watch                    Watch for file changes and auto-reindex
  --debounce-ms <MS>         Debounce timeout in milliseconds [default: 300]
  --http <ADDR>              Start Streamable HTTP server (requires 'http' feature)
```

### Auto-Reindexing with `--watch`

When `--watch` is enabled, the MCP server monitors the project directory for source file changes and automatically rebuilds the in-memory index and INDEX.md:

```bash
indxr serve ./my-project --watch
```

This means agents always query up-to-date data without needing to call `regenerate_index` manually. Changes are debounced (default 300ms) to avoid redundant reindexing during rapid saves. Non-source files, hidden directories, cache directories, and the INDEX.md output file are filtered out.

For a standalone watcher that only keeps INDEX.md updated (without running the MCP server), use `indxr watch`.

## Protocol

The MCP server implements JSON-RPC 2.0 over stdin/stdout, following the MCP specification version `2024-11-05`.

### Lifecycle

1. Client sends `initialize` request
2. Server responds with capabilities (tools list)
3. Client sends `initialized` notification
4. Client calls tools via `tools/call` requests
5. Client sends SIGTERM or closes stdin to shut down

## Available Tools

### `lookup_symbol`

Find declarations matching a name across the entire codebase. Uses case-insensitive substring matching.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `name` | string | yes | Symbol name to search for |
| `limit` | number | no | Max results (default: 50, max: 200) |
| `compact` | boolean | no | Return columnar format (saves ~30% tokens) |

**Example request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "lookup_symbol",
    "arguments": { "name": "Cache", "limit": 10 }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "Found 3 matches:\n\nsrc/cache/mod.rs:\n  struct Cache (line 15)\n  pub fn Cache::load(...) -> Result<Self> (line 25)\n  pub fn Cache::save(&self) -> Result<()> (line 45)"
  }]
}
```

### `list_declarations`

List all declarations in a specific file.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | yes | Relative file path |
| `kind` | string | no | Filter by kind (function, struct, class, etc.) |
| `shallow` | boolean | no | Omit children and doc comments to reduce output |
| `compact` | boolean | no | Columnar format (implies shallow, saves ~30% tokens) |

**Example:**
```json
{
  "params": {
    "name": "list_declarations",
    "arguments": { "path": "src/main.rs", "kind": "function" }
  }
}
```

### `search_signatures`

Search function/method signatures by substring.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | yes | Substring to search for in signatures |
| `limit` | number | no | Max results (default: 20, max: 100) |
| `compact` | boolean | no | Return columnar format (saves ~30% tokens) |

**Example:**
```json
{
  "params": {
    "name": "search_signatures",
    "arguments": { "query": "-> Result<" }
  }
}
```

### `get_tree`

Get the directory and file tree of the indexed codebase.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | no | Filter to a subtree by path prefix |

**Example:**
```json
{
  "params": {
    "name": "get_tree",
    "arguments": { "path": "src/parser" }
  }
}
```

### `get_imports`

Get all import statements for a specific file.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | yes | Relative file path |

### `get_stats`

Get index statistics. No parameters required.

**Returns:** File count, line count, language breakdown, indexing duration, and generation timestamp.

### `get_file_summary`

Get a complete overview of a file in one call: metadata, imports, declarations (shallow), kind counts, public symbol count, and test presence.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | yes | Relative file path |

### `read_source`

Read source code from a file, either by symbol name (uses indexed line info) or by explicit line range. Cap: 200 lines per symbol, 500 lines total for multi-symbol reads.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | yes | Relative file path |
| `symbol` | string | no | Symbol name to look up and extract |
| `symbols` | string[] | no | Multiple symbol names to read in one call (alternative to `symbol`) |
| `start_line` | number | no | Start line (1-based) for explicit range |
| `end_line` | number | no | End line (1-based, inclusive) for explicit range |
| `expand` | number | no | Extra context lines above/below (default: 0) |
| `collapse` | boolean | no | If true, collapse nested block bodies to `{ ... }` — shows structure without inner implementation |

### `get_file_context`

Get a file's summary plus its dependency context: which files import it (reverse dependencies) and related files (tests, siblings in the same directory).

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | yes | Relative file path |

### `get_token_estimate`

Estimate how many tokens a file or symbol would consume if read in full. Helps agents decide whether to use `read_source` (targeted, cheap) or `Read` (full file, expensive). Supports bulk estimation via `directory` or `glob`.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | no | Relative file path |
| `symbol` | string | no | Symbol name — if provided, estimates tokens for just that symbol's source |
| `directory` | string | no | Directory path — estimates all files within (alternative to `path`) |
| `glob` | string | no | Glob pattern — estimates all matching files (alternative to `path`) |

**Example (file-level):**
```json
{
  "params": {
    "name": "get_token_estimate",
    "arguments": { "path": "src/mcp/tools.rs" }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "{\"file\":\"src/mcp/tools.rs\",\"full_file_tokens\":8500,\"full_file_lines\":1400,\"summary_tokens\":300,\"declaration_count\":42,\"recommendation\":\"Use get_file_summary (~300 tokens) instead of Read (~8500 tokens). Use read_source for specific symbols.\"}"
  }]
}
```

**Example (symbol-level):**
```json
{
  "params": {
    "name": "get_token_estimate",
    "arguments": { "path": "src/mcp.rs", "symbol": "tool_search_relevant" }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "{\"file\":\"src/mcp/tools.rs\",\"symbol\":\"tool_search_relevant\",\"symbol_tokens\":250,\"symbol_lines\":45,\"full_file_tokens\":8500,\"full_file_lines\":1400,\"savings\":\"read_source saves ~8250 tokens (97% reduction)\"}"
  }]
}
```

### `search_relevant`

Multi-signal relevance search across file paths, symbol names, signatures, and doc comments. Returns ranked results scored by weighted matching (3x name, 2x signature, 1x doc comment, public symbol boost). Use as a starting point to find where to look without reading any files.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | yes | Search query — a concept (e.g. `authentication`), partial name (e.g. `parse`), or type pattern (e.g. `Result<Cache>`) |
| `limit` | number | no | Max results (default: 20, max: 50) |
| `kind` | string | no | Filter by declaration kind (e.g. `fn`, `struct`, `class`, `trait`) |
| `compact` | boolean | no | Return columnar format (saves ~30% tokens) |

**Example:**
```json
{
  "params": {
    "name": "search_relevant",
    "arguments": { "query": "token budget", "limit": 10 }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "Found 5 relevant matches:\n\nsrc/budget.rs (path, score: 4)\n  pub fn apply_token_budget(...) -> CodebaseIndex (name+signature, score: 12)\n  pub fn estimate_tokens(text: &str) -> usize (name, score: 9)\n\nsrc/mcp.rs:\n  fn tool_get_token_estimate(...) -> Value (name, score: 6)"
  }]
}
```

### `regenerate_index`

Re-scan the codebase, rebuild the index, and write an updated INDEX.md to the project root. Also refreshes the in-memory index used by all other tools. No parameters required.

**Example request:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "regenerate_index",
    "arguments": {}
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "{\"status\":\"ok\",\"message\":\"INDEX.md regenerated (44 files, 16132 lines)\",\"path\":\"/path/to/project/INDEX.md\",\"files_indexed\":44,\"total_lines\":16132}"
  }]
}
```

### `explain_symbol`

Get everything needed to USE a symbol without reading its body: signature, doc comment, relationships (children, parent), and metadata. Ideal for understanding an API without the implementation cost.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `name` | string | yes | Symbol name (exact match, case-insensitive) |

**Example:**
```json
{
  "params": {
    "name": "explain_symbol",
    "arguments": { "name": "apply_token_budget" }
  }
}
```

### `batch_file_summaries`

Get summaries for multiple files in one call. Provide an array of paths or a glob pattern. Cap: 30 files.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `paths` | string[] | no | Array of file paths (relative to project root) |
| `glob` | string | no | Glob pattern to match files (e.g. `*.rs`, `src/parser/*`) |

**Example:**
```json
{
  "params": {
    "name": "batch_file_summaries",
    "arguments": { "glob": "src/mcp/*.rs" }
  }
}
```

### `get_public_api`

Get only public declarations with signatures for a file, directory, or the entire codebase. Minimal output for "how do I use this module?" questions.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | no | File path or directory prefix. Omit for entire codebase |

**Example:**
```json
{
  "params": {
    "name": "get_public_api",
    "arguments": { "path": "src/cache" }
  }
}
```

### `get_callers`

Find declarations that reference a symbol. Searches signatures and import statements across all files. Approximate — based on name matching, not full call graph.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `symbol` | string | yes | Symbol name to search for references to |
| `limit` | number | no | Max results (default: 20, max: 50) |

**Example:**
```json
{
  "params": {
    "name": "get_callers",
    "arguments": { "symbol": "estimate_tokens" }
  }
}
```

### `get_related_tests`

Find test functions for a symbol by naming convention and file association.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `symbol` | string | yes | Symbol name to find tests for |
| `path` | string | no | Optional file path to scope search |

**Example:**
```json
{
  "params": {
    "name": "get_related_tests",
    "arguments": { "symbol": "apply_token_budget" }
  }
}
```

### `get_diff_summary`

Get structural changes (added/removed/modified declarations) since a git ref or for a GitHub PR. Requires either `since_ref` or `pr` (not both). Much cheaper than reading raw diffs.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `since_ref` | string | no | Git ref to diff against (branch name, tag, or commit like `HEAD~3`) |
| `pr` | integer | no | GitHub PR number — resolves the PR's base branch automatically (alternative to `since_ref`) |

One of `since_ref` or `pr` must be provided.

**Authentication (for `pr`):** Requires a GitHub token via `GITHUB_TOKEN` env var, `GH_TOKEN` env var, or `gh auth token` (GitHub CLI). The PR's base branch must be available locally.

**Example (git ref):**
```json
{
  "params": {
    "name": "get_diff_summary",
    "arguments": { "since_ref": "main" }
  }
}
```

**Example (PR):**
```json
{
  "params": {
    "name": "get_diff_summary",
    "arguments": { "pr": 42 }
  }
}
```

When using `pr`, the response includes a `pr` field with metadata:
```json
{
  "since_ref": "origin/main",
  "pr": { "number": 42, "title": "Add caching layer", "base": "main", "head": "feat/cache" },
  "changes": 3,
  "files_added": [...],
  "files_removed": [...],
  "files_modified": [...]
}
```

### `get_hotspots`

Get the most complex functions/methods in the codebase, ranked by a composite complexity score. Useful for identifying refactoring targets and understanding where technical debt concentrates. Only includes tree-sitter parsed languages (Rust, Python, TS, JS, Go, Java, C, C++).

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `limit` | number | no | Maximum number of results (default: 20, max: 100) |
| `path` | string | no | Optional file or directory path filter |
| `min_complexity` | number | no | Minimum cyclomatic complexity to include (default: 1) |
| `sort_by` | string | no | Sort criterion: `score` (default), `complexity`, `nesting`, `params`, `body_lines` |
| `compact` | boolean | no | Return columnar format (saves ~30% tokens) |

**Example:**
```json
{
  "params": {
    "name": "get_hotspots",
    "arguments": { "limit": 10, "min_complexity": 5 }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "{\"total\":42,\"hotspots\":[{\"file\":\"src/parser.rs\",\"name\":\"parse_file\",\"kind\":\"function\",\"line\":10,\"cyclomatic\":12,\"max_nesting\":4,\"param_count\":1,\"body_lines\":20,\"score\":18.5}]}"
  }]
}
```

**Score formula:** `cyclomatic + nesting*2 + params*0.5 + body_lines/20`

### `get_health`

Get a codebase health summary with aggregate complexity metrics, documentation coverage, test ratio, and quality indicators. Only complexity data from tree-sitter parsed languages is included.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | no | Optional path filter to scope to a directory or file |

**Example:**
```json
{
  "params": {
    "name": "get_health",
    "arguments": { "path": "src/parser" }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "{\"total_functions\":150,\"analyzed\":120,\"complexity\":{\"avg\":4.2,\"median\":3,\"max\":25,\"p90\":10},\"nesting\":{\"avg\":1.8},\"params\":{\"avg\":2.1},\"body_lines\":{\"avg\":15.3},\"high_complexity_count\":8,\"high_complexity_pct\":6.7,\"documented_pct\":45.0,\"test_count\":35,\"deprecated_count\":2,\"public_api_count\":60,\"hottest_files\":[{\"file\":\"src/parser.rs\",\"functions\":12,\"avg_complexity\":8.5,\"max_complexity\":25}]}"
  }]
}
```

**Fields:**
- `total_functions` — total function/method count (all languages)
- `analyzed` — functions with complexity data (tree-sitter languages only)
- `complexity` — avg, median, max, p90 cyclomatic complexity
- `high_complexity_count/pct` — functions with cyclomatic complexity >= 10
- `documented_pct` — percentage of functions with doc comments
- `test_count` — number of test functions
- `hottest_files` — top 5 files by average complexity (min 2 functions)

### `get_type_flow`

Track where a type flows across function boundaries. Shows which functions produce (return) and consume (accept as parameters) a given type. Useful for understanding data flow, finding related code, and tracing how data moves through the system. Supports 10+ languages: Rust, Go, TypeScript, JavaScript, Python, Java, Kotlin, Swift, C, C++, and C#.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `type_name` | string | yes | Type name to track (e.g., `FileIndex`, `Declaration`, `Cache`) |
| `path` | string | no | File or directory path filter to scope the search |
| `include_fields` | boolean | no | If true, also include struct/class fields that hold this type (default: false) |
| `limit` | number | no | Maximum results per role (default: 50, max: 200) |
| `compact` | boolean | no | Return columnar format (saves ~30% tokens) |

**Example:**
```json
{
  "params": {
    "name": "get_type_flow",
    "arguments": { "type_name": "FileIndex", "path": "src/parser" }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "{\"type_name\":\"FileIndex\",\"producers_count\":3,\"consumers_count\":5,\"producers\":[{\"file\":\"src/parser/mod.rs\",\"name\":\"parse_file\",\"kind\":\"function\",\"signature\":\"pub fn parse_file(path: &Path) -> Result<FileIndex>\",\"line\":42,\"role\":\"producer\"}],\"consumers\":[{\"file\":\"src/indexer.rs\",\"name\":\"merge_index\",\"kind\":\"function\",\"signature\":\"pub fn merge_index(files: Vec<FileIndex>) -> CodebaseIndex\",\"line\":15,\"role\":\"consumer\"}]}"
  }]
}
```

**Fields:**
- `type_name` — the type that was searched for
- `producers_count` / `consumers_count` — total matches (before `limit` truncation)
- `producers` — functions that return this type
- `consumers` — functions that accept this type as a parameter (and fields, if `include_fields` is true)

## Configuration for AI Tools

> **Tip:** `indxr init` can create all these configuration files automatically. See `indxr init --help` or the [Agent Integration Guide](agent-integration.md#quick-setup).

### Claude Code

Add to `.mcp.json` in your project root (or run `indxr init --claude`):

```json
{
  "mcpServers": {
    "indxr": {
      "command": "indxr",
      "args": ["serve", "."]
    }
  }
}
```

Or via CLI:

```bash
claude mcp add indxr -- indxr serve .
```

### Claude Desktop

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "indxr": {
      "command": "indxr",
      "args": ["serve", "/absolute/path/to/project"]
    }
  }
}
```

### Cursor

Add to `.cursor/mcp.json` (project) or `~/.cursor/mcp.json` (global), or run `indxr init --cursor` (`indxr init --global --cursor` for global):

```json
{
  "mcpServers": {
    "indxr": {
      "command": "indxr",
      "args": ["serve", "."]
    }
  }
}
```

### Windsurf

Add to `.windsurf/mcp.json` (project) or `~/.codeium/windsurf/mcp_config.json` (global), or run `indxr init --windsurf` (`indxr init --global --windsurf` for global):

```json
{
  "mcpServers": {
    "indxr": {
      "command": "indxr",
      "args": ["serve", "."]
    }
  }
}
```

### Codex CLI

Add to `.codex/config.toml` (project) or `~/.codex/config.toml` (global), or run `indxr init --codex` (`indxr init --global --codex` for global):

```toml
[mcp_servers.indxr]
command = "indxr"
args = ["serve", "."]
```

### Custom Integration

#### stdio

The MCP server communicates via JSON-RPC 2.0 over stdin/stdout. Any client that speaks MCP can connect. Spawn the process and send/receive newline-delimited JSON messages.

```python
import subprocess, json

proc = subprocess.Popen(
    ["indxr", "serve", "./my-project"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    text=True
)

# Send initialize
request = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {"name": "my-agent", "version": "1.0"}
}}
proc.stdin.write(json.dumps(request) + "\n")
proc.stdin.flush()
response = json.loads(proc.stdout.readline())

# Send initialized notification
proc.stdin.write(json.dumps({
    "jsonrpc": "2.0", "method": "notifications/initialized"
}) + "\n")
proc.stdin.flush()

# Call a tool
request = {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
    "name": "lookup_symbol",
    "arguments": {"name": "main"}
}}
proc.stdin.write(json.dumps(request) + "\n")
proc.stdin.flush()
result = json.loads(proc.stdout.readline())
print(result)
```

#### Streamable HTTP

With the HTTP transport, send JSON-RPC requests as HTTP POST to `/mcp`:

```bash
# Initialize (creates a session)
curl -X POST http://localhost:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
# Response includes Mcp-Session-Id header

# Call a tool (include session ID)
curl -X POST http://localhost:8080/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json' \
  -H 'Mcp-Session-Id: <session-id>' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_stats","arguments":{}}}'

# Listen for server notifications (SSE)
curl -N http://localhost:8080/mcp \
  -H 'Accept: text/event-stream' \
  -H 'Mcp-Session-Id: <session-id>'

# End session
curl -X DELETE http://localhost:8080/mcp \
  -H 'Mcp-Session-Id: <session-id>'
```
