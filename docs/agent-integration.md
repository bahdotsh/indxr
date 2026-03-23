# Agent Integration Guide

How to use indxr effectively with AI coding agents like Claude Code, Claude Desktop, OpenAI Codex, Cursor, Windsurf, GitHub Copilot, and others.

## The Problem

AI agents exploring a codebase typically read files one at a time, spending tokens to understand project structure. A medium-sized project (100+ files) can easily consume 50K+ tokens just for orientation — before any real work begins.

## The Solution

indxr gives agents a structural map of the entire codebase in a fraction of the tokens. An agent can see every function, struct, class, interface, and import across hundreds of files in a single context load.

## Two Integration Modes

### 1. Static Index (dump to file)

Generate an index file and include it in the agent's context:

```bash
indxr -o INDEX.md
```

**Best for:**
- One-shot tasks (code review, refactoring plans, architecture questions)
- Agents that don't support MCP
- CI/CD pipelines that produce codebase summaries
- Including in prompts or system instructions

### 2. MCP Server (live queries)

Run the MCP server and let agents query the index on-demand:

```bash
indxr serve ./my-project
```

**Best for:**
- Long-running agent sessions
- Interactive development workflows
- Agents that support MCP (Claude Code, Claude Desktop, Cursor, Windsurf)
- When the full index is too large for the context window

## Agent-Specific Setup

### Claude Code

Claude Code supports MCP servers natively. Add indxr to your project's `.mcp.json`:

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

Or use the CLI to add it:

```bash
claude mcp add indxr -- indxr serve .
```

Claude Code will automatically discover the MCP tools and can call `lookup_symbol`, `list_declarations`, etc. during conversations.

**Tips for Claude Code:**
- Use the MCP server for interactive sessions where you're exploring or debugging
- For one-shot tasks, pipe the index directly: `indxr --max-tokens 8000 | claude -p "review this codebase"`
- Use `--public-only` when you only need the API surface
- Use `--since HEAD~5` to show Claude what changed recently

### Claude Desktop

Add to your Claude Desktop MCP configuration file:

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

Restart Claude Desktop after updating the config. The indxr tools will appear in the tools menu.

### Cursor

Cursor supports MCP servers. Add to your Cursor MCP configuration:

**Settings → MCP Servers → Add Server:**

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

**Static index approach for Cursor:**

Generate an index and add it to your project rules:

```bash
indxr --max-tokens 6000 --public-only -o .cursor/INDEX.md
```

Then reference it in `.cursorrules`:

```
When exploring this codebase, refer to .cursor/INDEX.md for a structural overview
of all files, functions, classes, and imports.
```

### Windsurf

Windsurf supports MCP servers. Add to your MCP configuration:

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

### OpenAI Codex CLI

Codex CLI doesn't support MCP, so use the static index approach:

```bash
# Generate a compact index
indxr --max-tokens 8000 -o INDEX.md

# Include in Codex instructions
codex -p "$(cat INDEX.md)

Given the codebase structure above, implement..."
```

**Tips for Codex:**
- Use `--max-tokens` to fit within Codex's context window
- `--public-only` is great for API-focused tasks
- `--filter-path` to scope to the relevant part of the codebase

### GitHub Copilot

For Copilot Workspace or Copilot Chat, generate a static index:

```bash
indxr --max-tokens 4000 --public-only -o CODEBASE_INDEX.md
```

Reference it in conversations or include it in your repo for Copilot to discover.

### Aider

Aider can read files into context. Generate an index and add it:

```bash
indxr --max-tokens 6000 -o INDEX.md
aider --read INDEX.md
```

Or in an aider session: `/read INDEX.md`

### Custom Agents / LLM Pipelines

For custom agent frameworks, use the JSON output for programmatic access:

```bash
# Full structured index
indxr -f json -o index.json

# Pipe to your agent
cat index.json | your-agent-pipeline
```

Or integrate the MCP server via JSON-RPC 2.0 over stdin/stdout. See [MCP Server docs](mcp-server.md) for the protocol details.

## Effective Usage Patterns

### Pattern 1: Orientation First

Before asking an agent to write code, give it the structural overview:

```bash
# Generate a scoped index for the area you're working in
indxr --filter-path src/parser --max-tokens 4000
```

This lets the agent understand what exists before deciding what to create or modify.

### Pattern 2: Review Recent Changes

When asking an agent to review or continue work:

```bash
# Show structural changes since the last release
indxr --since v1.2.0

# Show what changed on this branch
indxr --since main
```

The agent sees exactly which declarations were added, removed, or modified — without reading full diffs.

### Pattern 3: API Surface for Library Work

When working with or on a library:

```bash
# Public API only, compact
indxr --public-only --max-tokens 3000
```

### Pattern 4: Symbol Lookup During Development

With the MCP server running, agents can look up symbols as needed:

```
Agent: "Let me check what methods are available on the Cache struct"
→ calls lookup_symbol("Cache")
→ gets: struct Cache, impl Cache { fn load(), fn save(), fn get(), fn insert(), ... }
```

### Pattern 5: Architecture Documentation

Generate a codebase overview for onboarding or documentation:

```bash
# Summary for high-level overview
indxr -d summary -o docs/ARCHITECTURE_OVERVIEW.md

# Full signatures for detailed reference
indxr -d signatures --public-only -o docs/API_REFERENCE.md
```

### Pattern 6: CI/CD Integration

Auto-generate an index on every commit for agents to consume:

```yaml
# .github/workflows/index.yml
- name: Generate codebase index
  run: |
    cargo install --path .
    indxr --public-only --max-tokens 8000 -o INDEX.md
- name: Commit index
  run: |
    git add INDEX.md
    git diff --cached --quiet || git commit -m "chore: update codebase index"
```

### Pattern 7: Multi-Language Projects

For polyglot codebases, scope by language:

```bash
# Only the Rust backend
indxr -l rust --filter-path src/backend

# Only the TypeScript frontend
indxr -l typescript,javascript --filter-path src/frontend
```

## Token Budget Guidelines

Different agents have different context windows. Here are recommended token budgets:

| Agent | Context Window | Recommended `--max-tokens` |
|-------|---------------|---------------------------|
| Claude Code (Opus) | 1M | 15,000–50,000 |
| Claude Code (Sonnet) | 1M | 10,000–30,000 |
| Claude Desktop | 1M | 10,000–30,000 |
| Cursor | 200K | 4,000–8,000 |
| Codex CLI | Varies | 8,000–15,000 |
| Copilot Chat | 64K–192K | 4,000–10,000 |
| Aider | Varies | 4,000–8,000 |

These are starting points — adjust based on how much context you need for the task alongside the index.

## Best Practices

1. **Use MCP when available** — it's more efficient than loading the full index, since agents only fetch what they need
2. **Scope your index** — use `--filter-path`, `--languages`, and `--public-only` to reduce noise
3. **Set a token budget** — always use `--max-tokens` when piping to agents with limited context
4. **Keep the index fresh** — regenerate after significant changes, or use the MCP server which always reads current state
5. **Combine with source reading** — the index shows structure; agents should still read specific files when they need implementation details
6. **Use structural diffs for reviews** — `--since main` is more useful than raw git diffs for understanding what changed architecturally
