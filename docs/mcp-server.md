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
```

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
| `shallow` | boolean | no | Compact output without children |

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

Read source code from a file, either by symbol name (uses indexed line info) or by explicit line range.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | yes | Relative file path |
| `symbol` | string | no | Symbol name to look up and extract |
| `start_line` | number | no | Start line (1-based) for explicit range |
| `end_line` | number | no | End line (1-based, inclusive) for explicit range |
| `expand` | number | no | Extra context lines above/below (default: 0) |

### `get_file_context`

Get a file's summary plus its dependency context: which files import it (reverse dependencies) and related files (tests, siblings in the same directory).

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | yes | Relative file path |

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

## Configuration for AI Tools

### Claude Code

Add to `.mcp.json` in your project root:

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

Add in Cursor's MCP server settings:

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

Add to Windsurf's MCP configuration:

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
