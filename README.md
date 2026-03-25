# indxr

Fast codebase indexer for AI agents. Tree-sitter AST parsing + regex extraction across 27 languages. Built in Rust.

## Install

```bash
cargo install indxr
```

## Usage

```bash
indxr                                        # index cwd → stdout
indxr ./my-project -o INDEX.md               # index project → file
indxr -f json -l rust,python -o index.json   # JSON, filter by language
indxr serve ./my-project                     # start MCP server
indxr watch ./my-project                     # watch & keep INDEX.md updated
indxr init                                   # set up all agent configs
```

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

## Public API Surface

**src/main.rs**
- `pub fn main() -> Result<()>`
- `pub struct App`

---

## src/main.rs

**Language:** Rust | **Size:** 1.2 KB | **Lines:** 45

**Imports:**
- `use anyhow::Result`
- `use clap::Parser`

**Declarations:**

`pub fn main() -> Result<()>`

`pub struct App`
> Fields: `name: String`, `config: Config`
```

Three output formats (`-f`): `markdown` (default), `json`, `yaml`.

Three detail levels (`-d`):

| Level | Content |
|-------|---------|
| `summary` | Directory tree + file list |
| `signatures` (default) | + declarations, imports |
| `full` | + doc comments, line numbers, body line counts, metadata badges, relationships |

## Languages

8 tree-sitter (full AST) + 19 regex (structural extraction):

| Parser | Languages |
|--------|-----------|
| tree-sitter | Rust, Python, TypeScript/TSX, JavaScript/JSX, Go, Java, C, C++ |
| regex | Shell, TOML, YAML, JSON, SQL, Markdown, Protobuf, GraphQL, Ruby, Kotlin, Swift, C#, Objective-C, XML, HTML, CSS, Gradle, CMake, Properties |

Detection is by file extension. Full extraction details: [docs/languages.md](docs/languages.md)

## Filtering

```bash
indxr --filter-path src/parser              # subtree
indxr --kind function --public-only         # public functions only
indxr --symbol "parse"                      # symbol name search (case-insensitive substring)
indxr --filter-path src/model --kind struct --public-only  # combine
indxr -l rust,python                        # language filter
```

All filters compose. `--kind` accepts: `function`, `struct`, `class`, `trait`, `enum`, `interface`, `module`, `method`, `constant`, `impl`, `type`, `namespace`, `macro`, `table`, `service`, `message`, `rpc`, and more.

## Git Structural Diffing

Declaration-level diffs against any git ref:

```bash
indxr --since main
indxr --since v1.0.0
indxr --since HEAD~5
```

```
## Modified Files

### src/parser/mod.rs
+ `pub fn new_parser() -> Parser`
- `fn old_helper()`
~ `fn process(x: i32)` → `fn process(x: i32, y: i32)`
```

Markers: `+` added, `-` removed, `~` signature changed. Supports `--filter-path`, `-l`, `--public-only`, `-f json`.

## Token Budget

Progressive truncation to fit context windows:

```bash
indxr --max-tokens 4000
```

Truncation order: doc comments → private declarations → children → least-important files. Directory tree and public API surface are preserved first.

File importance scoring: entry points (`main.rs`, `lib.rs`, `index.ts`) > root proximity > public declaration count.

## File Watching

Keep INDEX.md continuously up to date as you edit code:

```bash
indxr watch                       # watch cwd, write INDEX.md
indxr watch ./my-project          # watch a specific project
indxr watch -o custom-index.md    # custom output path
indxr watch --debounce-ms 500     # slower debounce (default: 300ms)
```

Performs an initial index, then re-indexes on each debounced file change. Filters out non-source files, hidden directories (`.git`), cache directories, and the output file itself to prevent loops.

The MCP server also supports watching via `indxr serve --watch`, which auto-reindexes the in-memory index and INDEX.md on file changes.

## MCP Server

JSON-RPC 2.0 over stdin/stdout, MCP spec `2024-11-05`:

```bash
indxr serve ./my-project
indxr serve ./my-project --watch  # auto-reindex on file changes
```

| Tool | Description |
|------|-------------|
| `search_relevant` | Multi-signal relevance search across paths, names, signatures, and docs. Supports `kind` filter and `compact` mode |
| `lookup_symbol` | Find declarations by name (case-insensitive substring). Supports `compact` mode |
| `explain_symbol` | Everything needed to USE a symbol: signature, doc comment, relationships, metadata — no body |
| `get_file_summary` | Complete file overview: metadata, declarations, kind counts, public symbols |
| `batch_file_summaries` | Summarize multiple files in one call (by paths array or glob). Cap: 30 files |
| `get_file_context` | File summary + reverse dependencies + related files |
| `get_public_api` | Public declarations with signatures for a file, directory, or entire codebase |
| `get_callers` | Find who references a symbol (searches imports and signatures across all files) |
| `get_related_tests` | Find test functions for a symbol by naming convention and file association |
| `list_declarations` | List declarations in a file, optional `kind` filter, `shallow` and `compact` modes |
| `search_signatures` | Search signatures by substring. Supports `compact` mode |
| `read_source` | Read source by symbol name or line range. Supports `symbols` array and `collapse` mode |
| `get_token_estimate` | Estimate tokens for a file, symbol, directory, or glob pattern |
| `get_tree` | Directory/file tree, optional path prefix filter |
| `get_imports` | Import statements for a file |
| `get_stats` | File count, line count, language breakdown, duration |
| `get_diff_summary` | Structural changes (added/removed/modified declarations) since a git ref |
| `regenerate_index` | Re-index codebase and write updated INDEX.md |

18 tools total. `compact` mode on list tools saves ~30% tokens. See [MCP Server docs](docs/mcp-server.md) for full parameter details.

### Quick Setup

```bash
indxr init                    # set up for all agents (Claude Code, Cursor, Windsurf)
indxr init --claude           # Claude Code only
indxr init --cursor           # Cursor only
indxr init --windsurf         # Windsurf only
```

This creates all configuration files in one command:

| Agent | Files Created |
|-------|--------------|
| Claude Code | `.mcp.json`, `CLAUDE.md`, `.claude/settings.json` (PreToolUse hooks) |
| Cursor | `.cursor/mcp.json`, `.cursorrules` |
| Windsurf | `.windsurf/mcp.json`, `.windsurfrules` |
| All | `.gitignore` entry, `INDEX.md` (static index) |

Use `--no-index` to skip INDEX.md generation, `--no-hooks` to skip PreToolUse hooks, `--force` to overwrite existing files.

**Getting agents to actually use it:** Agents don't always pick MCP tools over file reads on their own. `indxr init` sets up reinforcement mechanisms automatically:

- **`.claude/settings.json` hooks** — PreToolUse hooks that intercept `Read` and `Bash` calls, reminding the agent to use indxr MCP tools instead of reading full files or running `git diff`
- **`CLAUDE.md` / `.cursorrules` / `.windsurfrules`** — agent instruction files that teach the exploration workflow, token costs, and when full reads are justified

See [Agent Integration](docs/agent-integration.md) for manual setup details and advanced configuration.

Setup guides: [docs/mcp-server.md](docs/mcp-server.md)

## Caching

Incremental binary cache in `.indxr-cache/cache.bin`. Two-tier validation: mtime + file size (fast path), xxh3 content hash (fallback). Cache format is versioned — automatically rebuilt on indxr upgrades.

```bash
indxr --no-cache          # bypass cache
indxr --cache-dir /tmp/c  # custom location
```

## Performance

Parallel parsing via rayon. Incremental caching via mtime + xxh3.

| Codebase | Files | Lines | Cold | Cached |
|----------|-------|-------|------|--------|
| Small (indxr) | 47 | 19K | 17ms | 5ms |
| Medium (atuin) | 132 | 22K | 20ms | 6ms |
| Large (cloud-hypervisor) | 243 | 124K | 73ms | ~10ms |

## Architecture

1. Walk directory tree (`.gitignore`-aware, via `ignore` crate)
2. Detect language by file extension
3. Check cache — skip unchanged files (mtime + xxh3)
4. Parse with tree-sitter or regex (parallel via rayon)
5. Extract declarations, metadata, relationships
6. Apply filters (path, kind, visibility, symbol)
7. Apply token budget (progressive truncation)
8. Format as Markdown, JSON, or YAML
9. Update cache

## Documentation

| Document | Description |
|----------|-------------|
| [CLI Reference](docs/cli-reference.md) | Complete flag and option reference |
| [Languages](docs/languages.md) | Per-language extraction details |
| [Output Formats](docs/output-formats.md) | Format and detail level reference |
| [Filtering](docs/filtering.md) | Path, kind, symbol, visibility filters |
| [Git Diffing](docs/git-diffing.md) | Structural diff since any git ref |
| [Token Budget](docs/token-budget.md) | Truncation strategy and scoring |
| [Caching](docs/caching.md) | Cache format and invalidation |
| [MCP Server](docs/mcp-server.md) | MCP tools, protocol, and client setup |
| [Agent Integration](docs/agent-integration.md) | Usage with Claude, Codex, Cursor, Copilot, etc. |

## License

MIT
