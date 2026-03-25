# MCP Server

indxr includes a built-in [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server that lets AI agents query the codebase index on-demand over stdin/stdout.

## Starting the Server

```bash
indxr serve ./my-project
```

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
    "arguments": { "path": "src/mcp.rs" }
  }
}
```

**Example response:**
```json
{
  "content": [{
    "type": "text",
    "text": "{\"file\":\"src/mcp.rs\",\"full_file_tokens\":8500,\"full_file_lines\":1400,\"summary_tokens\":300,\"declaration_count\":42,\"recommendation\":\"Use get_file_summary (~300 tokens) instead of Read (~8500 tokens). Use read_source for specific symbols.\"}"
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
    "text": "{\"file\":\"src/mcp.rs\",\"symbol\":\"tool_search_relevant\",\"symbol_tokens\":250,\"symbol_lines\":45,\"full_file_tokens\":8500,\"full_file_lines\":1400,\"savings\":\"read_source saves ~8250 tokens (97% reduction)\"}"
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

Get structural changes (added/removed/modified declarations) since a git ref. Much cheaper than reading raw diffs.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `since_ref` | string | yes | Git ref to diff against (branch name, tag, or commit like `HEAD~3`) |

**Example:**
```json
{
  "params": {
    "name": "get_diff_summary",
    "arguments": { "since_ref": "main" }
  }
}
```

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

Add in Cursor's MCP server settings (or run `indxr init --cursor`):

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

Add to Windsurf's MCP configuration (or run `indxr init --windsurf`):

```json
{
  "mcpServers": {
    "indxr": {
      "command": "indxr",
      "args": ["serve", "/path/to/project"]
    }
  }
}
```

### Custom Integration

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
