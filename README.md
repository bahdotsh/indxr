<div align="center">

# indxr

**A fast codebase indexer and MCP server for AI coding agents.**

[![CI](https://github.com/bahdotsh/indxr/actions/workflows/ci.yml/badge.svg)](https://github.com/bahdotsh/indxr/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/indxr.svg)](https://crates.io/crates/indxr)
[![License](https://img.shields.io/crates/l/indxr.svg)](LICENSE)

</div>

AI coding agents waste thousands of tokens reading entire source files just to understand what's in them. indxr gives agents a structural map of your codebase — declarations, imports, relationships, and dependency graphs — so they can query for exactly what they need at a fraction of the token cost.

---

## Features

- **27 languages** — tree-sitter AST parsing for 8 languages, regex extraction for 19 more
- **26-tool MCP server** (3 compound default + 23 granular via `--all-tools`) — live codebase queries over JSON-RPC: symbol lookup, file summaries, caller tracing, signature search, complexity hotspots, type flow tracking, workspace support, and more
- **Token-aware** — progressive truncation to fit context windows, ~5x reduction vs reading full files
- **Git structural diffing** — declaration-level diffs (`+` added, `-` removed, `~` changed) against any git ref or GitHub PR
- **Dependency graphs** — file and symbol dependency visualization as DOT, Mermaid, or JSON
- **File watching** — continuous re-indexing as you edit, via `indxr watch` or `indxr serve --watch`
- **Monorepo / workspace support** — auto-detects Cargo, npm, and Go workspaces; scope any tool or command to a specific member via `--member`
- **One-command agent setup** — `indxr init` configures Claude Code, Cursor, Windsurf, and Codex CLI with MCP, instruction files, and hooks
- **Incremental caching** — mtime + xxh3 content hashing, sub-20ms indexing for most projects
- **Complexity hotspots** — per-function cyclomatic complexity, nesting depth, and parameter count via tree-sitter AST analysis; codebase health reports
- **Type flow tracking** — cross-file analysis showing which functions produce (return) and consume (accept) a given type
- **Composable filters** — by path, kind, symbol name, visibility, and language
- **Three output formats** — Markdown (default), JSON, YAML at three detail levels

## Install

```bash
cargo install indxr
```

Or build from source:

```bash
git clone https://github.com/bahdotsh/indxr.git
cd indxr && cargo build --release
```

## Usage

```bash
indxr                                        # index cwd → stdout
indxr ./my-project -o INDEX.md               # index project → file
indxr -f json -l rust,python -o index.json   # JSON, filter by language
indxr serve ./my-project                     # start MCP server
indxr serve ./my-project --watch             # MCP server with auto-reindex
indxr watch ./my-project                     # watch & keep INDEX.md updated
indxr members                                # list workspace members (monorepo)
indxr init                                   # set up all agent configs
```

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

Agents don't always pick MCP tools over file reads on their own. `indxr init` sets up reinforcement — PreToolUse hooks intercept `Read`/`Bash` calls and instruction files teach the exploration workflow.

## MCP Server

JSON-RPC 2.0 over stdin/stdout (or Streamable HTTP with `--features http`). By default 3 compound tools are listed to minimize per-request token overhead; pass `--all-tools` to expose all 26 (3 compound + 23 granular).

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

In workspace mode (multiple members), tools automatically gain a `member` param to scope queries. List tools support `compact` mode for ~30% token savings. See [MCP Server docs](docs/mcp-server.md) for full parameter details.

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

All filters compose. `--kind` accepts: `function`, `struct`, `class`, `trait`, `enum`, `interface`, `module`, `method`, `constant`, `impl`, `type`, `namespace`, `macro`, and more.

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

Shows cyclomatic complexity, max nesting depth, parameter count, body lines, and a composite score for each function. Only tree-sitter parsed languages are analyzed.

MCP tools: `get_hotspots` (ranked list with filtering and sorting), `get_health` (aggregate metrics, documentation coverage, test ratio, hottest files), `get_type_flow` (cross-file type flow tracking — producers and consumers of any type).

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

| Codebase | Files | Lines | Cold | Cached |
|---|---|---|---|---|
| Small (indxr) | 47 | 19K | 17ms | 5ms |
| Medium (atuin) | 132 | 22K | 20ms | 6ms |
| Large (cloud-hypervisor) | 243 | 124K | 73ms | ~10ms |

## Documentation

| Document | Description |
|---|---|
| [CLI Reference](docs/cli-reference.md) | Complete flag and option reference |
| [Languages](docs/languages.md) | Per-language extraction details |
| [Output Formats](docs/output-formats.md) | Format and detail level reference |
| [Filtering](docs/filtering.md) | Path, kind, symbol, visibility filters |
| [Dependency Graph](docs/dep-graph.md) | File and symbol dependency visualization |
| [Git Diffing](docs/git-diffing.md) | Structural diff since any git ref or GitHub PR |
| [Token Budget](docs/token-budget.md) | Truncation strategy and scoring |
| [Caching](docs/caching.md) | Cache format and invalidation |
| [MCP Server](docs/mcp-server.md) | MCP tools, protocol, and client setup |
| [Agent Integration](docs/agent-integration.md) | Usage with Claude, Codex, Cursor, Copilot, etc. |

## Contributing

Contributions welcome — feel free to open an issue or submit a PR.

## License

[MIT](LICENSE)
