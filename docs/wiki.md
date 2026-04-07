# Codebase Knowledge Wiki

The wiki is the core feature of indxr. It gives AI agents a persistent, self-updating knowledge base about your codebase — architecture decisions, module responsibilities, failure patterns, and cross-cutting concerns that would otherwise live only in people's heads or get lost between agent sessions.

Every agent interaction makes the wiki richer. Agents query it before diving into code, compound new insights after analysis, and record failure patterns so future agents don't repeat mistakes. The structural index (`INDEX.md`) provides the foundation — it tells agents *what exists*. The wiki tells them *why*.

```bash
cargo install indxr --features wiki
```

## Overview

Wiki pages are stored in `.indxr/wiki/` as Markdown files with YAML frontmatter. They support cross-references via `[[page-id]]` links, source file associations, contradiction tracking, and failure pattern recording.

### Page Types

| Type | Purpose | Example |
|------|---------|---------|
| `architecture` | High-level system design, data flow, key decisions | `architecture.md` |
| `module` | Module responsibility, public API, internal structure | `mod-mcp.md`, `mod-parser.md` |
| `entity` | Important types/structs — what they represent, relationships | `entity-declaration.md` |
| `topic` | Cross-cutting concerns, patterns, design decisions | `topic-complexity.md` |
| `index` | Wiki table of contents | `index.md` |

## CLI Usage

### Generate a wiki from scratch

```bash
indxr wiki generate
```

This initializes the wiki structure and returns codebase context for an LLM to plan which pages to create. The LLM (or agent) then calls `wiki_contribute` for each page.

Options:
- `--model <MODEL>` — LLM model to use (auto-detected from provider by default)
- `--wiki-dir <DIR>` — Wiki output directory (default: `.indxr/wiki`)
- `--exec <CMD>` — External command for LLM completions (receives JSON on stdin, returns text on stdout)
- `--dry-run` — Plan wiki structure without generating pages
- `--max-response-tokens <N>` — Maximum tokens per LLM response (default: 4096)

```bash
# Use a specific model
indxr wiki generate --model claude-sonnet-4-20250514

# Dry run to see what pages would be created
indxr wiki generate --dry-run

# Use an external command as the LLM backend
indxr wiki generate --exec "my-llm-wrapper"
```

### Update wiki after code changes

```bash
indxr wiki update
```

Analyzes code changes since the last wiki generation and updates affected pages. The update is driven by structural diffs — only pages whose source files changed are rewritten.

Options:
- `--since <REF>` — Git ref to diff against (default: ref stored in wiki manifest)
- `--model <MODEL>` — LLM model override
- `--exec <CMD>` — External LLM command
- `--max-response-tokens <N>` — Maximum tokens per LLM response (default: 4096)

```bash
# Update based on changes since a specific ref
indxr wiki update --since main

# Update with a specific model
indxr wiki update --model claude-sonnet-4-20250514
```

### Check wiki health

```bash
indxr wiki status
```

Shows page count, staleness (commits behind HEAD), and source file coverage.

### Compound knowledge into the wiki

```bash
# From a file
indxr wiki compound notes.txt

# From stdin
echo "The parser module uses a two-phase approach..." | indxr wiki compound -

# With source page references
indxr wiki compound notes.txt --source-pages mod-parser,mod-mcp

# With a custom title for new pages
indxr wiki compound notes.txt --title "Design Decisions"
```

Takes synthesized knowledge and automatically routes it to the best matching wiki page, or creates a new topic page if no good match exists.

## LLM Configuration

Wiki generation and updates require an LLM. Configure one of:

| Method | Environment Variable | Notes |
|--------|---------------------|-------|
| Anthropic API | `ANTHROPIC_API_KEY` | Uses Claude models |
| OpenAI API | `OPENAI_API_KEY` | Uses GPT models |
| External command | `INDXR_LLM_COMMAND` or `--exec` flag | Custom LLM backend |

The `--exec` flag is particularly useful for AI coding agents — the agent itself can act as the LLM backend, eliminating the need for API keys.

## MCP Tools

When built with `--features wiki`, the MCP server exposes wiki tools automatically. `wiki_generate` is always available; the remaining 8 tools appear once a wiki exists.

| Tool | Description |
|------|-------------|
| `wiki_generate` | Initialize a new wiki and return structural context for page planning |
| `wiki_search` | Search wiki by keyword or concept; returns matching pages with excerpts |
| `wiki_read` | Read a wiki page by ID; returns full content with metadata |
| `wiki_status` | Check wiki health: page count, staleness, source file coverage |
| `wiki_contribute` | Write knowledge back to the wiki (create or update pages) |
| `wiki_update` | Analyze code changes and return affected pages with diff context |
| `wiki_suggest_contribution` | Suggest which page to update for a given synthesis (no LLM call) |
| `wiki_compound` | Auto-route synthesized knowledge to the best matching page |
| `wiki_record_failure` | Record a failed fix attempt for future agents to learn from |

See [MCP Server docs](mcp-server.md#wiki-tools) for full parameter details.

### Agent-driven workflow

The wiki is designed to be agent-driven. A typical workflow:

1. **Generate:** Agent calls `wiki_generate`, receives structural context, plans pages, then calls `wiki_contribute` for each page
2. **Query:** Agent calls `wiki_search` before diving into source code to understand modules, design decisions, and prior failure patterns
3. **Learn:** After synthesizing insights from multiple pages or code exploration, agent calls `wiki_compound` to persist the knowledge
4. **Record failures:** When a fix attempt fails, agent calls `wiki_record_failure` so future agents avoid the same mistake
5. **Update:** After code changes, agent calls `wiki_update` to identify stale pages, rewrites them, and saves via `wiki_contribute`

### Auto-updating wiki with `serve --watch`

The MCP server can automatically trigger wiki updates when files change:

```bash
indxr serve --watch --wiki-auto-update
```

Options:
- `--wiki-auto-update` — Enable automatic wiki updates on file changes (requires `--watch`)
- `--wiki-debounce-ms <MS>` — Debounce timeout for wiki updates (default: 30000ms). Wiki updates are expensive LLM calls, so this is much longer than the structural reindex debounce
- `--wiki-model <MODEL>` — LLM model override for wiki auto-updates
- `--wiki-exec <CMD>` — External LLM command for wiki auto-updates

## Wiki Structure on Disk

```
.indxr/
  wiki/
    manifest.yaml          # Wiki metadata (git ref, generation timestamp)
    architecture.md        # Architecture overview page
    modules/
      mod-parser.md        # Module page
      mod-mcp.md
    entities/
      entity-declaration.md
    topics/
      topic-complexity.md
      index.md             # Wiki table of contents
```

Each page is a Markdown file with YAML frontmatter:

```yaml
---
id: mod-parser
title: Parser Module
page_type: module
source_files:
  - src/parser/mod.rs
  - src/parser/rust.rs
links_to:
  - architecture
  - entity-declaration
updated_at: "2025-03-23T10:30:00Z"
---

# Parser Module

The parser module is responsible for...
```

### Cross-references

Use `[[page-id]]` syntax in page content to create cross-references:

```markdown
The parser produces [[entity-declaration]] objects that are consumed by
the [[mod-mcp]] module for serving queries.
```

Cross-references are automatically tracked in the `links_to` frontmatter field.

### Contradiction tracking

Pages can track contradictions between documentation and code:

```json
{
  "page": "mod-parser",
  "content": "...",
  "contradictions": [{
    "description": "Doc says parser supports 10 languages, but code shows 8",
    "source": "src/parser/mod.rs:42"
  }]
}
```

### Failure pattern recording

Record failed fix attempts so future agents can learn from mistakes:

```json
{
  "symptom": "Tests pass locally but fail in CI",
  "attempted_fix": "Added sleep between API calls",
  "diagnosis": "Race condition was in the test setup, not the API calls",
  "actual_fix": "Used proper test isolation with per-test temp dirs",
  "source_files": ["tests/integration.rs"]
}
```

Future agents searching for similar symptoms via `wiki_search(query, include_failures=true)` will see these failure patterns before attempting their own fixes.

## Gitignore

Add `.indxr/` to your `.gitignore` if you don't want to commit the wiki, or commit it to share knowledge across the team:

```bash
# Option 1: Don't commit (regenerate per-developer)
echo ".indxr/" >> .gitignore

# Option 2: Commit (shared team knowledge)
git add .indxr/wiki/
```
