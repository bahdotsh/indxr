# Agent Integration Guide

How to use indxr effectively with AI coding agents like Claude Code, Claude Desktop, OpenAI Codex, Cursor, Windsurf, GitHub Copilot, and others.

## The Problem

AI agents start every session from scratch. They waste thousands of tokens re-reading files to orient themselves, have no memory of past debugging sessions, and repeat the same mistakes across conversations. Architecture decisions, module responsibilities, and hard-won knowledge about why things are the way they are — all of it gets lost.

## The Solution

indxr gives agents a persistent knowledge wiki and a fast structural index of your codebase. Agents can query the wiki to understand *why* things exist before reading a single line of code, and write back what they learn so future agents start with richer context. The structural index provides the foundation — every function, struct, class, interface, and import across hundreds of files, queryable at a fraction of the token cost.

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
indxr init                    # all agents (Claude Code, Cursor, Windsurf, Codex CLI)
indxr init --claude           # Claude Code only
indxr init --cursor           # Cursor only
indxr init --windsurf         # Windsurf only
indxr init --codex            # OpenAI Codex CLI only
indxr init --global           # install globally for all projects
indxr init --global --cursor  # global Cursor only
```

This creates all configuration files, agent instruction files, PreToolUse hooks, and an initial INDEX.md in one command. Use `--no-index` to skip INDEX.md generation, `--no-hooks` to skip PreToolUse hooks, `--no-rtk` to skip RTK hook setup, `--force` to overwrite existing files.

Use `--global` to install indxr into user-level config directories so it's available for every project without per-project setup. Global mode merges the indxr MCP server entry into existing config files (preserving other servers and settings).

The sections below describe what each file does and how to set things up manually.

## Wiki Setup

The wiki is the primary way agents build and retain knowledge about your codebase. Set it up early — it makes every subsequent agent interaction more productive.

```bash
# Build with wiki support
cargo install indxr --features wiki

# Generate the wiki (requires LLM — set ANTHROPIC_API_KEY or OPENAI_API_KEY)
indxr wiki generate

# Or let an agent generate it via MCP (no API key needed — the agent IS the LLM)
# Agent calls wiki_generate, plans pages, then wiki_contribute for each
```

### Recommended MCP server setup

For the best experience, run the MCP server with wiki auto-updates enabled:

```bash
indxr serve --watch --wiki-auto-update
```

This gives agents both structural queries and wiki tools, with automatic re-indexing and wiki updates as code changes. See [Wiki docs](wiki.md) for full details on LLM configuration, page types, and disk layout.

### Agent workflow with wiki

Every agent session follows this cycle:

1. **Before diving into code:** Agent calls `wiki_search("authentication")` to understand modules, design decisions, and prior failure patterns
2. **After synthesizing insights:** Agent calls `wiki_compound` to persist knowledge that spans multiple pages
3. **When a fix fails:** Agent calls `wiki_record_failure` so future agents avoid the same mistake
4. **After code changes:** Agent calls `wiki_update` to identify stale pages, rewrites them, and saves via `wiki_contribute`

## Agent-Specific Setup

### Claude Code

**Automated setup:** `indxr init --claude` creates `.mcp.json`, `CLAUDE.md`, and `.claude/settings.json` automatically. Use `indxr init --global --claude` to install globally at `~/.claude.json` (MCP) and `~/.claude/CLAUDE.md` (instructions).

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

Claude Code will automatically discover the 3 default compound MCP tools — `find`, `summarize`, and `read` — during conversations. To expose all 26 tools (3 compound + 23 granular tools like `search_relevant`, `get_file_summary`, `get_hotspots`, `get_health`, `get_type_flow`, `get_diff_summary`), use `--all-tools`:

```json
{
  "mcpServers": {
    "indxr": {
      "command": "indxr",
      "args": ["serve", ".", "--all-tools"]
    }
  }
}
```

> Granular tools are always callable even when not listed — `--all-tools` only controls whether they appear in `tools/list`.

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
            "command": "echo 'IMPORTANT: Before reading full source files, use indxr MCP tools to minimize token usage:\n- summarize(path): understand a file without reading it (~300 tokens vs ~3000+)\n- find(query): find specific functions/types by name, concept, or signature\n- read(path, symbol): read only the exact function/symbol you need (~100 tokens vs full file)\nOnly use Read when you need to EDIT a file, need exact formatting, or the file is not source code (e.g., CLAUDE.md, Cargo.toml).'"
          }
        ]
      },
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "if printf '%s' \"$TOOL_INPUT\" | grep -qE 'git\\s+diff'; then echo 'IMPORTANT: Use indxr get_diff_summary MCP tool instead of git diff (requires --all-tools). It shows structural changes (added/removed/modified declarations) at ~200-500 tokens vs thousands for raw diffs. Example: get_diff_summary(since_ref: \"main\")'; fi"
          }
        ]
      }
    ]
  }
}
```

The hooks are non-blocking — they print reminders nudging the agent toward cheaper compound tool calls without preventing the original action when it's actually needed (e.g., `Read` before editing, or `git diff` for exact line-level changes).

**Teaching the agent via CLAUDE.md:**

`CLAUDE.md` is loaded into Claude Code's system prompt at the start of every conversation. Add instructions that tell the agent to prefer indxr tools over reading files. Key things to include:

1. **Mandate MCP-first exploration** — tell the agent to always use indxr tools before the `Read` tool
2. **Token savings table** — show concrete cost comparisons so the agent can make informed decisions
3. **Ordered workflow** — list the compound tools in the order agents should reach for them (`find` → `summarize` → `read` → `Read`)
4. **When Read is OK** — be explicit about when full reads are justified (editing, exact formatting, non-source files)
5. **Compound tool modes** — mention `find` modes (relevant, symbol, callers, signature) and `summarize` auto-detection (file, glob, symbol name)

Example CLAUDE.md section:

```markdown
## Codebase Navigation — MUST USE indxr MCP tools

An MCP server called `indxr` is available. **Always use indxr tools before the Read tool.**
Do NOT read full source files as a first step — use the MCP tools to explore, then read only what you need.

### Exploration workflow (follow this order)
1. `find(query)` — find files/symbols by concept, name, callers, or signature pattern
2. `summarize(path)` — understand files/symbols without reading source (auto-detects file, glob, or symbol name)
3. `read(path, symbol?)` — read just one function/struct (supports `symbols` array and `collapse`)
4. `Read` (full file) — ONLY when editing or need exact formatting

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

**Automated setup:** `indxr init --cursor` creates `.cursor/mcp.json` and `.cursor/rules/indxr.mdc` automatically. Use `indxr init --global --cursor` to install globally at `~/.cursor/mcp.json`. If upgrading from a previous setup, indxr will warn about the deprecated `.cursorrules` file — you can safely remove it.

**Manual setup:** Cursor supports MCP servers. Add to `.cursor/mcp.json` (project) or `~/.cursor/mcp.json` (global):

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

**Project rules:** Create `.cursor/rules/indxr.mdc` with instructions to use indxr MCP tools.

**Static index approach for Cursor:**

Generate an index and add it to your project rules:

```bash
indxr --max-tokens 6000 --public-only -o .cursor/INDEX.md
```

Then reference it in `.cursor/rules/`:

```
When exploring this codebase, refer to .cursor/INDEX.md for a structural overview
of all files, functions, classes, and imports.
```

### Windsurf

**Automated setup:** `indxr init --windsurf` creates `.windsurf/mcp.json` and `.windsurf/rules/indxr.md` automatically. Use `indxr init --global --windsurf` to install globally at `~/.codeium/windsurf/mcp_config.json` and `~/.codeium/windsurf/memories/global_rules.md`. If upgrading from a previous setup, indxr will warn about the deprecated `.windsurfrules` file — you can safely remove it.

**Manual setup:** Windsurf supports MCP servers. Add to `.windsurf/mcp.json` (project) or `~/.codeium/windsurf/mcp_config.json` (global):

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

### OpenAI Codex CLI

**Automated setup:** `indxr init --codex` creates `.codex/config.toml` and `AGENTS.md` automatically. Use `indxr init --global --codex` to install globally at `~/.codex/config.toml` and `~/.codex/AGENTS.md`.

**Manual setup:** Codex CLI uses TOML configuration. Add to `.codex/config.toml` (project) or `~/.codex/config.toml` (global):

```toml
[mcp_servers.indxr]
command = "indxr"
args = ["serve", "."]
```

Or use the CLI:

```bash
codex mcp add indxr -- indxr serve .
```

**Instructions file:** Create `AGENTS.md` in the project root (or `~/.codex/AGENTS.md` for global) with instructions to use indxr MCP tools for codebase navigation.

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

### Compound tools for efficient exploration

The 3 default compound tools (`find`, `summarize`, `read`) cover the most common exploration patterns:

```
Agent: "Where is authentication handled?"
→ calls find("authentication")
→ ranked results across paths, names, signatures, and doc comments (~200 tokens)

Agent: "I need to understand src/mcp.rs"
→ calls summarize("src/mcp.rs")
→ complete file overview: declarations, imports, counts (~300 tokens vs ~8500 for full read)

Agent: "I need to read the parse_declaration function"
→ calls read("src/parser/rust.rs", symbol="parse_declaration")
→ just the function source (~150 tokens vs ~5000 for full file)
```

The `find` tool supports multiple modes: `relevant` (default — weighted scoring across names, signatures, docs), `symbol` (exact name match), `callers` (who references this), and `signature` (search by pattern like `"-> Result<"`).

The `summarize` tool auto-detects what you pass: file path → file summary, glob pattern → batch summaries, symbol name → full interface details. Use `scope: "public"` for public API only.

### `get_token_estimate` (requires `--all-tools`)

Before reading a file, agents can check how many tokens it will cost:

```
Agent: "Should I read this file?"
→ calls get_token_estimate("src/mcp/tools.rs")
→ response: "full file is ~8500 tokens, use summarize (~300 tokens) instead"
```

### Reinforcing with Hooks and CLAUDE.md

Agents don't always use MCP tools voluntarily. Two mechanisms help:

- **`.claude/settings.json` PreToolUse hooks** — intercepts `Read` calls (reminding agents to use `summarize`/`read` compound tools) and `Bash` calls containing `git diff` (reminding agents to use `get_diff_summary`). Non-blocking, works automatically.
- **`CLAUDE.md` instructions** — loaded into every conversation's system prompt. Teaches the compound tool workflow (`find` → `summarize` → `read` → `Read`), token costs, and when `Read`/`git diff` are justified vs MCP alternatives.

See the [Claude Code setup section](#claude-code) above for full details, examples, and a ready-to-copy CLAUDE.md template.

## Effective Usage Patterns

### Pattern 1: Wiki-Enhanced Exploration (MCP)

With the wiki available, agents can understand the *why* before the *what*:

```
Agent: "I need to add a new parser for Ruby"
→ calls wiki_search("parser module")
→ gets: module overview, design decisions, existing parser patterns
→ calls summarize("src/parser/mod.rs")
→ gets: file structure, trait to implement
→ calls read("src/parser/rust.rs", symbol="parse_rust")
→ gets: reference implementation
                                         Total: ~1000 tokens of deeply contextual knowledge
```

After completing the work, the agent compounds what it learned:

```
→ calls wiki_compound("Added Ruby parser following the same trait pattern as Rust/Python.
   Ruby uses regex parsing, not tree-sitter. Key difference: Ruby uses `end` blocks
   instead of braces, which affects the nesting detection regex.")
→ knowledge persisted for future agents
```

### Pattern 2: Orientation First

Before asking an agent to write code, give it the structural overview:

```bash
# Generate a scoped index for the area you're working in
indxr --filter-path src/parser --max-tokens 4000
```

This lets the agent understand what exists before deciding what to create or modify.

### Pattern 3: Review Recent Changes

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

### Pattern 4: API Surface for Library Work

When working with or on a library:

```bash
# Public API only, compact
indxr --public-only --max-tokens 3000
```

### Pattern 5: Symbol Lookup During Development

With the MCP server running, agents can look up symbols as needed:

```
Agent: "Let me check what methods are available on the Cache struct"
→ calls summarize("Cache")
→ gets: struct Cache — signature, doc comment, children (load, save, get, insert, ...), relationships

Agent: "Who calls estimate_tokens?"
→ calls find("estimate_tokens", mode="callers")
→ gets: files and declarations that reference estimate_tokens
```

### Pattern 6: Token-Budget-Aware Exploration (MCP)

With the compound tools, agents can explore efficiently without wasting tokens:

```
1. find("caching logic")                → find relevant files/symbols (~200 tokens)
2. summarize("src/cache.rs")            → understand structure (~300 tokens)
3. read("src/cache.rs", symbol="load")  → read just the function (~150 tokens)
                                         Total: ~650 tokens vs ~5000+ for reading the full file
```

### Pattern 7: Architecture Documentation

Generate a codebase overview for onboarding or documentation:

```bash
# Summary for high-level overview
indxr -d summary -o docs/ARCHITECTURE_OVERVIEW.md

# Full signatures for detailed reference
indxr -d signatures --public-only -o docs/API_REFERENCE.md
```

### Pattern 8: CI/CD Integration

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

### Pattern 9: Monorepo / Workspace Projects

For monorepos with multiple packages, use native workspace support:

```bash
# List detected workspace members
indxr members

# Serve only a specific member
indxr serve --member backend

# MCP: scope any tool to a member
# get_file_summary(path: "src/lib.rs", member: "core")
# lookup_symbol(name: "Config", member: "cli")
```

indxr auto-detects Cargo workspaces, npm/Yarn/pnpm workspaces, and Go workspaces. Use `--no-workspace` to disable detection and treat the root as a single project.

### Pattern 10: Multi-Language Projects

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

1. **Generate the wiki first** — run `indxr wiki generate` before starting agent sessions. The wiki provides the "why" context that makes every interaction more productive
2. **Use `--watch --wiki-auto-update`** — keep both the structural index and wiki in sync with code changes automatically
3. **Compound knowledge** — after any multi-step investigation, have the agent call `wiki_compound` to persist what it learned for future sessions
4. **Record failures** — when a fix attempt fails, call `wiki_record_failure` so future agents don't repeat the same mistake
5. **Use MCP when available** — it's more efficient than loading the full index, since agents only fetch what they need
6. **Scope your index** — use `--filter-path`, `--languages`, and `--public-only` to reduce noise
7. **Set a token budget** — always use `--max-tokens` when piping to agents with limited context
8. **Use structural diffs for reviews** — `--since main` is more useful than raw git diffs for understanding what changed architecturally
