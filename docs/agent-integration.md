# Agent Integration Guide

How to use indxr effectively with AI coding agents like Claude Code, Claude Desktop, OpenAI Codex, Cursor, Windsurf, GitHub Copilot, and others.

## The Problem

AI agents exploring a codebase typically read files one at a time, spending tokens to understand project structure. A medium-sized project (100+ files) can easily consume 50K+ tokens just for orientation — before any real work begins.

## The Solution

indxr gives agents a structural map of the entire codebase in a fraction of the tokens. An agent can see every function, struct, class, interface, and import across hundreds of files in a single context load.

## Three Integration Modes

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

### 2. Live Watch (auto-updating file)

Keep INDEX.md continuously updated as code changes:

```bash
indxr watch ./my-project
```

**Best for:**
- Keeping a static index fresh without manual regeneration
- Agents that don't support MCP but benefit from an always-current index
- Development workflows where INDEX.md is committed or referenced by tools

### 3. MCP Server (live queries)

Run the MCP server and let agents query the index on-demand:

```bash
indxr serve ./my-project
indxr serve ./my-project --watch   # auto-reindex on file changes
```

**Best for:**
- Long-running agent sessions
- Interactive development workflows
- Agents that support MCP (Claude Code, Claude Desktop, Cursor, Windsurf)
- When the full index is too large for the context window

## Quick Setup

The fastest way to set up indxr for any agent is the `init` command:

```bash
indxr init                    # all agents (Claude Code, Cursor, Windsurf)
indxr init --claude           # Claude Code only
indxr init --cursor           # Cursor only
indxr init --windsurf         # Windsurf only
```

This creates all configuration files, agent instruction files, PreToolUse hooks, and an initial INDEX.md in one command. Use `--no-index` to skip INDEX.md generation, `--no-hooks` to skip PreToolUse hooks, `--force` to overwrite existing files.

The sections below describe what each file does and how to set things up manually.

## Agent-Specific Setup

### Claude Code

**Automated setup:** `indxr init --claude` creates `.mcp.json`, `CLAUDE.md`, and `.claude/settings.json` automatically.

**Manual setup:** Claude Code supports MCP servers natively. Add indxr to your project's `.mcp.json`:

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

Claude Code will automatically discover all 22 MCP tools — `search_relevant`, `explain_symbol`, `get_callers`, `batch_file_summaries`, `get_public_api`, `get_diff_summary`, `get_hotspots`, `get_health`, `get_type_flow`, and more — during conversations.

**Reinforcing MCP usage with PreToolUse hooks:**

Even with MCP tools available, Claude Code may still default to reading full files or running `git diff`. Two PreToolUse hooks intercept these calls and remind the agent to use indxr instead. Add this to `.claude/settings.json` in your project root:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Read",
        "hooks": [
          {
            "type": "command",
            "command": "echo 'IMPORTANT: Before reading full source files, use indxr MCP tools to minimize token usage:\n- get_file_summary: understand a file without reading it (~300 tokens vs ~3000+)\n- lookup_symbol / search_signatures: find specific functions/types\n- read_source: read only the exact function/symbol you need (~100 tokens vs full file)\nOnly use Read when you need to EDIT a file, need exact formatting, or the file is not source code (e.g., CLAUDE.md, Cargo.toml).'"
          }
        ]
      },
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "if printf '%s' \"$TOOL_INPUT\" | grep -qE 'git\\s+diff'; then echo 'IMPORTANT: Use indxr get_diff_summary MCP tool instead of git diff. It shows structural changes (added/removed/modified declarations) at ~200-500 tokens vs thousands for raw diffs. Example: get_diff_summary(since_ref: \"main\")'; fi"
          }
        ]
      }
    ]
  }
}
```

The hooks are non-blocking — they print reminders nudging the agent toward cheaper MCP calls without preventing the original action when it's actually needed (e.g., `Read` before editing, or `git diff` for exact line-level changes).

**Teaching the agent via CLAUDE.md:**

`CLAUDE.md` is loaded into Claude Code's system prompt at the start of every conversation. Add instructions that tell the agent to prefer indxr tools over reading files. Key things to include:

1. **Mandate MCP-first exploration** — tell the agent to always use indxr tools before the `Read` tool
2. **Token savings table** — show concrete cost comparisons so the agent can make informed decisions
3. **Ordered workflow** — list the tools in the order agents should reach for them (`search_relevant` → `get_tree` → `get_file_summary` → `explain_symbol` → `read_source` → `Read`)
4. **When Read is OK** — be explicit about when full reads are justified (editing, exact formatting, non-source files)
5. **Batch and scope tools** — mention `batch_file_summaries`, `get_public_api`, `get_callers`, and `get_diff_summary` for efficient multi-file exploration

Example CLAUDE.md section:

```markdown
## Codebase Navigation — MUST USE indxr MCP tools

An MCP server called `indxr` is available. **Always use indxr tools before the Read tool.**
Do NOT read full source files as a first step — use the MCP tools to explore, then read only what you need.

### Exploration workflow (follow this order)
1. `search_relevant` — find files/symbols by concept or partial name (supports `kind` filter)
2. `get_tree` — see directory/file layout
3. `get_file_summary` / `batch_file_summaries` — understand files without reading them
4. `explain_symbol` — get signature, docs, and relationships for a symbol
5. `get_public_api` — public API surface of a file or module
6. `get_callers` / `get_related_tests` — find references and tests
7. `get_token_estimate` — check cost before deciding to Read
8. `read_source` — read just one function/struct (supports `symbols` array and `collapse`)
9. `Read` (full file) — ONLY when editing or need exact formatting

### When to use Read instead
- You need to **edit** a file (Read is required before Edit)
- You need exact formatting/whitespace
- The file is not source code (e.g., CLAUDE.md, Cargo.toml, config files)
```

See this project's own [CLAUDE.md](../CLAUDE.md) for a complete working example.

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

**Automated setup:** `indxr init --cursor` creates `.cursor/mcp.json` and `.cursorrules` automatically.

**Manual setup:** Cursor supports MCP servers. Add to your Cursor MCP configuration:

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

**Automated setup:** `indxr init --windsurf` creates `.windsurf/mcp.json` and `.windsurfrules` automatically.

**Manual setup:** Windsurf supports MCP servers. Add to your MCP configuration:

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

## Token-Aware Exploration

indxr includes tools specifically designed to help agents minimize token consumption:

### `get_token_estimate`

Before reading a file, agents can check how many tokens it will cost and get a recommendation:

```
Agent: "I need to understand src/mcp.rs"
→ calls get_token_estimate("src/mcp.rs")
→ response: "full file is ~8500 tokens, use get_file_summary (~300 tokens) instead"
→ agent uses get_file_summary, saving ~8200 tokens
```

For specific symbols, the savings are even larger:

```
Agent: "I need to read the parse_declaration function"
→ calls get_token_estimate("src/parser/rust.rs", symbol="parse_declaration")
→ response: "symbol is ~150 tokens, full file is ~5000 tokens — 97% reduction with read_source"
```

### `search_relevant`

Instead of the multi-step `get_tree` → `get_file_summary` → `lookup_symbol` dance, agents can search by concept in a single call:

```
Agent: "Where is authentication handled?"
→ calls search_relevant("authentication")
→ ranked results across paths, names, signatures, and doc comments
```

The search uses weighted scoring: symbol names match strongest (3x), then signatures (2x), then doc comments (1x), with a boost for public symbols.

### Reinforcing with Hooks and CLAUDE.md

Agents don't always use MCP tools voluntarily. Two mechanisms help:

- **`.claude/settings.json` PreToolUse hooks** — intercepts `Read` calls (reminding agents to use `get_file_summary`/`read_source`) and `Bash` calls containing `git diff` (reminding agents to use `get_diff_summary`). Non-blocking, works automatically.
- **`CLAUDE.md` instructions** — loaded into every conversation's system prompt. Tell the agent the exploration order, token costs, and when `Read`/`git diff` are justified vs MCP alternatives.

See the [Claude Code setup section](#claude-code) above for full details, examples, and a ready-to-copy CLAUDE.md template.

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

# Show structural changes for a GitHub PR
indxr diff --pr 42
```

The agent sees exactly which declarations were added, removed, or modified — without reading full diffs. The MCP `get_diff_summary` tool also supports a `pr` parameter for PR-aware diffs.

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

### Pattern 5: Token-Budget-Aware Exploration (MCP)

With `get_token_estimate` and `search_relevant`, agents can explore efficiently without wasting tokens:

```
1. search_relevant("caching logic")     → find relevant files/symbols (~200 tokens)
2. get_token_estimate("src/cache.rs")    → check cost before reading (~100 tokens)
3. get_file_summary("src/cache.rs")      → understand structure (~300 tokens)
4. read_source("src/cache.rs", "load")   → read just the function (~150 tokens)
                                         Total: ~750 tokens vs ~5000+ for reading the full file
```

### Pattern 6: Architecture Documentation

Generate a codebase overview for onboarding or documentation:

```bash
# Summary for high-level overview
indxr -d summary -o docs/ARCHITECTURE_OVERVIEW.md

# Full signatures for detailed reference
indxr -d signatures --public-only -o docs/API_REFERENCE.md
```

### Pattern 7: CI/CD Integration

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

### Pattern 8: Multi-Language Projects

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
4. **Keep the index fresh** — use `indxr watch` to auto-update INDEX.md, `indxr serve --watch` for live MCP sessions, or regenerate manually after significant changes
5. **Combine with source reading** — the index shows structure; agents should still read specific files when they need implementation details
6. **Use structural diffs for reviews** — `--since main` is more useful than raw git diffs for understanding what changed architecturally
