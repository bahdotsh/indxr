<div align="center">

# indxr

**A fast codebase indexer and knowledge wiki for AI agents.**

[![CI](https://github.com/bahdotsh/indxr/actions/workflows/ci.yml/badge.svg)](https://github.com/bahdotsh/indxr/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/indxr.svg)](https://crates.io/crates/indxr)
[![License](https://img.shields.io/crates/l/indxr.svg)](LICENSE)

</div>

indxr gives AI agents a persistent, self-updating wiki about your codebase backed by a fast structural index. Architecture decisions, module responsibilities, failure patterns — stuff that normally lives in people's heads — gets written down and stays current.

---

## Features

- **Codebase knowledge wiki** — persistent wiki with architecture pages, module overviews, failure pattern recording, contradiction tracking, and knowledge compounding. Agents query it, learn from it, and write back to it
- **9 wiki MCP tools** — `wiki_generate`, `wiki_search`, `wiki_read`, `wiki_contribute`, `wiki_update`, `wiki_compound`, `wiki_suggest_contribution`, `wiki_record_failure`, `wiki_status`
- **Self-updating** — wiki automatically stays in sync with code changes via `indxr serve --watch --wiki-auto-update`
- **26-tool MCP server** (3 compound default + 23 granular via `--all-tools`) — symbol lookup, file summaries, caller tracing, signature search, complexity hotspots, type flow tracking, workspace support
- **27 languages** — tree-sitter AST parsing for 8 languages, regex extraction for 19 more
- **Token-aware** — progressive truncation to fit context windows
- **Git structural diffing** — declaration-level diffs (`+` added, `-` removed, `~` changed) against any git ref or GitHub PR
- **Dependency graphs** — file and symbol dependency visualization as DOT, Mermaid, or JSON
- **File watching** — continuous re-indexing as you edit, via `indxr watch` or `indxr serve --watch`
- **Monorepo / workspace support** — Cargo, npm, Go workspaces; `--member` to scope to specific members
- **One-command agent setup** — `indxr init` configures Claude Code, Cursor, Windsurf, Codex CLI
- **Incremental caching** — mtime + xxh3 content hashing
- **Complexity hotspots** — per-function cyclomatic complexity, nesting depth, parameter count; codebase health reports
- **Composable filters** — path, kind, symbol name, visibility, language

## Install

```bash
cargo install indxr --features wiki
```

This installs indxr with the full wiki system. For additional transports:

```bash
cargo install indxr --features wiki,http   # + Streamable HTTP transport
```

Or build from source:

```bash
git clone https://github.com/bahdotsh/indxr.git
cd indxr && cargo build --release --features wiki
```

> You can also install without the wiki feature (`cargo install indxr`) if you only need structural indexing.

## Usage

```bash
# Wiki — the core workflow
indxr wiki generate                          # generate codebase knowledge wiki
indxr wiki update                            # update wiki after code changes
indxr wiki status                            # check wiki health
indxr wiki compound notes.txt                # compound knowledge from file

# MCP server — live queries + wiki tools for AI agents
indxr serve ./my-project --watch --wiki-auto-update  # recommended: full setup
indxr serve ./my-project                     # start MCP server (structural only)
indxr serve ./my-project --watch             # MCP server with auto-reindex

# Structural indexing
indxr                                        # index cwd → stdout
indxr ./my-project -o INDEX.md               # index project → file
indxr -f json -l rust,python -o index.json   # JSON, filter by language

# Setup
indxr init                                   # set up all agent configs
indxr members                                # list workspace members (monorepo)
```

## Codebase Knowledge Wiki

The structural index tells agents what exists in your codebase. The wiki tells them why. Architecture decisions, module responsibilities, failure patterns — things that usually get lost when someone leaves the team.

```bash
indxr wiki generate                          # generate wiki from scratch
indxr wiki update                            # update after code changes
indxr wiki status                            # check wiki health
indxr wiki compound notes.txt                # compound knowledge from file
echo "synthesis" | indxr wiki compound -     # compound from stdin
```

Wiki pages are stored in `.indxr/wiki/` as Markdown with YAML frontmatter. Page types: `architecture`, `module`, `entity`, `topic`. Pages support `[[page-id]]` cross-references, contradiction tracking, and failure pattern recording.

### How agents use the wiki

1. **Generate:** `wiki_generate` → agent plans pages from structural context, writes each via `wiki_contribute`
2. **Query:** `wiki_search` to understand modules and design decisions before reading source
3. **Learn:** `wiki_compound` persists synthesized insights after cross-page analysis
4. **Record failures:** `wiki_record_failure` so future agents don't repeat mistakes
5. **Update:** `wiki_update` identifies stale pages after code changes, agent rewrites via `wiki_contribute`

### Wiki MCP tools (9 tools)

| Tool | Description |
|---|---|
| `wiki_generate` | Initialize a new wiki and return structural context for page planning |
| `wiki_search` | Search wiki by keyword or concept; returns matching pages with excerpts |
| `wiki_read` | Read a wiki page by ID; returns full content with metadata |
| `wiki_status` | Check wiki health: page count, staleness, source file coverage |
| `wiki_contribute` | Write knowledge back to the wiki (create or update pages) |
| `wiki_update` | Analyze code changes and return affected pages with diff context |
| `wiki_suggest_contribution` | Suggest which page to update for a given synthesis (no LLM call) |
| `wiki_compound` | Auto-route synthesized knowledge to the best matching page |
| `wiki_record_failure` | Record a failed fix attempt for future agents to learn from |

> `wiki_generate` is always listed; the remaining 8 tools appear once a wiki exists. Wiki tools support contradiction tracking and failure pattern recording.

### Auto-updating wiki

The MCP server keeps the wiki in sync with your code automatically:

```bash
indxr serve --watch --wiki-auto-update
```

Triggers wiki page updates when source files change. Set `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or use `--exec` for a custom LLM backend.

See [Wiki docs](docs/wiki.md) for full details on page structure, LLM configuration, and disk layout.

## Agent Setup

```bash
indxr init                    # set up for all agents
indxr init --claude           # Claude Code only
indxr init --cursor           # Cursor only
indxr init --windsurf         # Windsurf only
indxr init --codex            # OpenAI Codex CLI only
indxr init --global           # install globally for all projects
indxr init --global --cursor  # global Cursor only
indxr init --no-rtk           # skip RTK hook setup
```

| Agent | Project Files | Global Files (`--global`) |
|---|---|---|
| Claude Code | `.mcp.json`, `CLAUDE.md`, `.claude/settings.json` | `~/.claude.json`, `~/.claude/CLAUDE.md` |
| Cursor | `.cursor/mcp.json`, `.cursor/rules/indxr.mdc` | `~/.cursor/mcp.json` |
| Windsurf | `.windsurf/mcp.json`, `.windsurf/rules/indxr.md` | `~/.codeium/windsurf/mcp_config.json`, `~/.codeium/windsurf/memories/global_rules.md` |
| Codex CLI | `.codex/config.toml`, `AGENTS.md` | `~/.codex/config.toml`, `~/.codex/AGENTS.md` |
| All | `.gitignore` entry, `INDEX.md` | — |

Agents don't always pick MCP tools over file reads on their own. `indxr init` sets up PreToolUse hooks that intercept `Read`/`Bash` calls and instruction files that teach the exploration workflow.

## MCP Server

JSON-RPC 2.0 over stdin/stdout (or Streamable HTTP with `--features http`). 3 compound tools listed by default; `--all-tools` exposes all 26. Wiki tools (9) are available when a wiki exists.

### Default tools (3 compound)

| Tool | Description |
|---|---|
| `find` | Find files/symbols by concept, name, callers, or signature pattern. Modes: `relevant` (default), `symbol`, `callers`, `signature` |
| `summarize` | Understand files/symbols without reading source. Auto-detects: glob -> batch, no "/" -> symbol name, file path -> file summary. Scope: `all` (default), `public` |
| `read` | Read source by symbol name or line range (same as `read_source`) |

### Granular tools (23 — requires `--all-tools`)

| Tool | Description |
|---|---|
| `search_relevant` | Multi-signal relevance search across paths, names, signatures, and docs |
| `lookup_symbol` | Find declarations by name (case-insensitive substring) |
| `explain_symbol` | Signature, doc comment, relationships, metadata — no body |
| `get_file_summary` | Complete file overview without reading it |
| `batch_file_summaries` | Summarize multiple files in one call |
| `get_file_context` | File summary + reverse dependencies + related files |
| `get_public_api` | Public declarations with signatures for a file or directory |
| `get_callers` | Find who references a symbol across all files |
| `list_declarations` | List declarations in a file with optional filters |
| `search_signatures` | Search functions by signature pattern |
| `read_source` | Read source by symbol name or line range |
| `get_tree` | Directory/file tree |
| `get_stats` | File count, line count, language breakdown |
| `get_imports` | Import statements for a file |
| `get_related_tests` | Find test functions by naming convention |
| `get_hotspots` | Most complex functions ranked by composite score |
| `get_health` | Codebase health summary with aggregate complexity metrics |
| `get_type_flow` | Track which functions produce/consume a given type across the codebase |
| `get_dependency_graph` | File and symbol dependency graph (DOT, Mermaid, JSON) |
| `get_diff_summary` | Structural changes since a git ref or GitHub PR |
| `get_token_estimate` | Estimate tokens before reading |
| `list_workspace_members` | List monorepo workspace members (Cargo, npm, Go) |
| `regenerate_index` | Re-index and update INDEX.md |

> Granular tools are always callable even when not listed — `--all-tools` only controls whether they appear in `tools/list`.

Wiki tools (9) are also available when a wiki exists. See [Wiki section](#codebase-knowledge-wiki).

In workspace mode, tools gain a `member` param to scope queries. List tools support `compact` mode. See [MCP Server docs](docs/mcp-server.md) for details.

## Output

Default format is Markdown at `signatures` detail level:

```markdown
# Codebase Index: my-project

> Generated: 2025-03-23 | Files: 42 | Lines: 8,234
> Languages: Rust (28), Python (10), TypeScript (4)

## Directory Structure
src/
  main.rs
  parser/
    mod.rs
    rust.rs

## src/main.rs

**Language:** Rust | **Size:** 1.2 KB | **Lines:** 45

**Declarations:**
`pub fn main() -> Result<()>`
`pub struct App`
```

| Detail Level | Content |
|---|---|
| `summary` | Directory tree + file list |
| `signatures` (default) | + declarations, imports |
| `full` | + doc comments, line numbers, body counts, metadata, relationships |

## Filtering

```bash
indxr --filter-path src/parser              # subtree
indxr --kind function --public-only         # public functions only
indxr --symbol "parse"                      # symbol name search
indxr -l rust,python                        # language filter
indxr --filter-path src/model --kind struct --public-only  # combine
```

Filters compose. `--kind` accepts: `function`, `struct`, `class`, `trait`, `enum`, `interface`, `module`, `method`, `constant`, `impl`, `type`, `namespace`, `macro`.

## Git Structural Diffing

```bash
indxr --since main
indxr --since v1.0.0
indxr --since HEAD~5
indxr diff --pr 42                           # diff against a GitHub PR's base branch
```

```
## Modified Files

### src/parser/mod.rs
+ `pub fn new_parser() -> Parser`
- `fn old_helper()`
~ `fn process(x: i32)` → `fn process(x: i32, y: i32)`
```

Markers: `+` added, `-` removed, `~` signature changed.

## Complexity Hotspots

```bash
indxr --hotspots                             # top 30 most complex functions
indxr --hotspots --filter-path src/parser    # scoped to a directory
```

Shows cyclomatic complexity, max nesting depth, parameter count, body lines, and composite score per function. Tree-sitter parsed languages only.

MCP tools: `get_hotspots`, `get_health`, `get_type_flow`.

## Dependency Graph

```bash
indxr --graph dot                            # file-level DOT graph
indxr --graph mermaid                        # file-level Mermaid diagram
indxr --graph json                           # JSON graph
indxr --graph dot --graph-level symbol       # symbol-level graph
indxr --graph mermaid --filter-path src/mcp  # scoped to a directory
indxr --graph dot --graph-depth 2            # limit to 2 hops
```

| Level | Description |
|---|---|
| `file` (default) | File-to-file import relationships |
| `symbol` | Symbol-to-symbol relationships (trait impls, method calls) |

## Token Budget

```bash
indxr --max-tokens 4000
```

Truncation order: doc comments → private declarations → children → least-important files. Directory tree and public API surface are preserved first.

## Languages

8 tree-sitter (full AST) + 19 regex (structural extraction):

| Parser | Languages |
|---|---|
| tree-sitter | Rust, Python, TypeScript/TSX, JavaScript/JSX, Go, Java, C, C++ |
| regex | Shell, TOML, YAML, JSON, SQL, Markdown, Protobuf, GraphQL, Ruby, Kotlin, Swift, C#, Objective-C, XML, HTML, CSS, Gradle, CMake, Properties |

Detection is by file extension. Full details: [docs/languages.md](docs/languages.md)

## Performance

Parallel parsing via rayon. Incremental caching via mtime + xxh3.

## Documentation

| Document | Description |
|---|---|
| [Wiki](docs/wiki.md) | Codebase knowledge wiki — generation, maintenance, and agent workflows |
| [Agent Integration](docs/agent-integration.md) | Usage with Claude, Codex, Cursor, Copilot, etc. |
| [MCP Server](docs/mcp-server.md) | MCP tools, protocol, and client setup |
| [CLI Reference](docs/cli-reference.md) | Complete flag and option reference |
| [Languages](docs/languages.md) | Per-language extraction details |
| [Output Formats](docs/output-formats.md) | Format and detail level reference |
| [Git Diffing](docs/git-diffing.md) | Structural diff since any git ref or GitHub PR |
| [Dependency Graph](docs/dep-graph.md) | File and symbol dependency visualization |
| [Filtering](docs/filtering.md) | Path, kind, symbol, visibility filters |
| [Token Budget](docs/token-budget.md) | Truncation strategy and scoring |
| [Caching](docs/caching.md) | Cache format and invalidation |

## Contributing

Contributions welcome — feel free to open an issue or submit a PR.

## License

[MIT](LICENSE)
