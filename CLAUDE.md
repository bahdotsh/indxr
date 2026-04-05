# indxr

A fast Rust codebase indexer for AI agents. Extracts structural maps (declarations, imports, tree) using tree-sitter and regex parsing across 27 languages.

## Codebase Navigation — MUST USE indxr MCP tools

An MCP server called `indxr` is available. **Always use indxr tools before the Read tool.** Do NOT read full source files as a first step — use the MCP tools to explore, then read only what you need.

### Token savings reference

The MCP server defaults to **3 compound tools** (`find`, `summarize`, `read`). All 26 tools (3 compound + 23 granular) are available with `--all-tools`. With `--features wiki`, 6 additional wiki tools are available.

| Action | Approx tokens | When to use |
|--------|--------------|-------------|
| `find(query)` | ~100-400 | Find files/symbols by concept, name, callers, or signature pattern |
| `summarize(path)` | ~200-600 | Understand a file, batch of files, or symbol without reading source |
| `read(path, symbol?)` | ~50-300 | Read one function/struct. Supports `symbols` array and `collapse`. |
| `Read` (full file) | **500-10000+** | ONLY when editing or need exact formatting |

**Typical exploration: ~500 tokens vs ~3000+ for reading a full file (6x reduction).**

### Exploration workflow (follow this order)

The default 3 compound tools cover the most common exploration patterns:

1. `find(query)` — find files/symbols by concept, partial name, or type pattern. **Start here when you know what you're looking for but not where it is.**
   - Default mode (`relevant`): multi-signal relevance search across paths, names, signatures, and docs. Supports `kind` filter.
   - `mode: "symbol"`: find declarations by name (case-insensitive substring).
   - `mode: "callers"`: find who references a symbol (imports + signatures).
   - `mode: "signature"`: find functions by signature pattern (e.g., `"-> Result<"`).
2. `summarize(path)` — understand files and symbols without reading source code.
   - File path (e.g., `"src/main.rs"`): complete file overview (declarations, imports, counts).
   - Glob pattern (e.g., `"src/mcp/*.rs"`): batch summaries for multiple files.
   - Symbol name (no `/`, e.g., `"Cache"`): full interface details (signature, doc comment, relationships).
   - `scope: "public"`: show only public API surface.
3. `read(path, symbol?)` — read source code by **symbol name** or explicit line range. Cap: 200 lines. Use `symbols` array to read multiple in one call (500 line cap). Use `collapse: true` to fold nested bodies.

With `--all-tools`, all 23 granular tools are also exposed:

4. `lookup_symbol` — look up a symbol by exact or partial name. Returns declaration details.
5. `list_declarations` — list all declarations in a file, optionally filtered by kind.
6. `search_signatures` — find functions by signature pattern (e.g., `"-> Result<"`).
7. `search_relevant` — multi-signal relevance search across paths, names, signatures, and docs.
8. `get_tree` — see directory/file layout. Use `path` param to scope to a subtree.
9. `get_file_summary` — get a single file's overview (declarations, imports, counts).
10. `read_source` — read source code by symbol name or line range (granular version of `read`).
11. `batch_file_summaries` — get summaries for multiple files in one call via glob pattern.
12. `get_file_context` — understand a file's reverse dependencies (who imports it) and related files (tests, siblings).
13. `get_callers` — find who references a symbol (imports + call sites).
14. `get_public_api` — list public declarations for a file or directory.
15. `explain_symbol` — full interface details for a symbol (signature, doc comment, relationships).
16. `get_token_estimate` — before deciding to `Read` a file, check how many tokens it costs. Supports `directory`, `glob`, or `symbol` for bulk/targeted estimation.
17. `get_related_tests` — find test functions for a symbol by naming convention and file association.
18. `get_diff_summary` — get structural changes since a git ref or GitHub PR number. Shows added/removed/modified declarations without reading full diffs.
19. `get_hotspots` — get the most complex functions/methods ranked by composite score.
20. `get_health` — get codebase health summary: aggregate complexity, documentation coverage, test ratio, hottest files.
21. `get_type_flow` — track where a type flows across function boundaries.
22. `get_dependency_graph` — get file-level or symbol-level dependency graph (DOT, Mermaid, JSON).
23. `get_stats` — codebase statistics: file count, line count, language breakdown.
24. `get_imports` — list import statements for a file.
25. `list_workspace_members` — list detected workspace members (Cargo, npm, Go workspaces).
26. `regenerate_index` — re-index after code changes. Updates INDEX.md, refreshes in-memory index, and reports what changed (delta).

#### Wiki tools (available when built with `--features wiki`)

If a wiki has been generated (`indxr wiki generate`), these tools are available automatically:

27. `wiki_search(query)` — search the codebase knowledge wiki by keyword or concept. Returns matching pages with excerpts. **Use this first to understand modules, architecture, or design decisions before diving into source code.**
28. `wiki_read(page)` — read a wiki page by ID (e.g. `"architecture"`, `"mod-mcp"`). Returns full page content with metadata.
29. `wiki_status()` — check wiki health: page count, staleness (commits behind HEAD), source file coverage.
30. `wiki_contribute(page, content, title?, page_type?, source_files?)` — write knowledge back to the wiki. Creates a new page or updates an existing one. Use `[[page-id]]` links in content for automatic cross-referencing. **Use this to file synthesized answers, analyses, or discovered connections that should persist beyond the current conversation.**
31. `wiki_generate()` — initialize a new wiki and return codebase structural context for planning pages. The agent plans which pages to create from the context, then calls `wiki_contribute` for each page. No API keys needed.
32. `wiki_update(since?)` — analyze code changes and return affected wiki pages with diff context. The agent rewrites each affected page and saves via `wiki_contribute`. No API keys needed.

> **Workspace support:** Most tools accept an optional `member` param to scope queries to a specific workspace member by name.

### Compact output mode
Granular tools that return lists (`lookup_symbol`, `list_declarations`, `search_signatures`, `search_relevant`, `get_hotspots`, `get_type_flow`) support a `compact: true` param that returns columnar `{columns, rows}` format instead of objects, saving ~30% tokens.

### When to use the Read tool instead
- You need to **edit** a file (Read is required before Edit)
- You need exact formatting/whitespace that `read` doesn't preserve
- The file is not a source file (e.g., CLAUDE.md, Cargo.toml, docs, config files)

### DO NOT
- Read full source files just to understand what's in them — use `summarize(path)`
- Read full source files to review code — use `summarize(path)` to triage, then `read(path, symbol)` on specific symbols
- Dump all files into context — use MCP tools to be surgical
- Read a file without first checking `get_token_estimate` if you're unsure about its size (requires `--all-tools`)
- Use `git diff` to understand changes — use `get_diff_summary` instead (~200-500 tokens vs thousands for raw diffs). It shows structural changes (added/removed/modified declarations) since any git ref

### After making code changes
Run `regenerate_index` to keep INDEX.md current.

## CLI Reference (for shell commands)

```bash
# Basic indexing
indxr                                        # index cwd → stdout
indxr ./project -o INDEX.md                  # output to file
indxr -f json -o index.json                  # JSON format
indxr -f yaml -o index.yaml                  # YAML format

# Detail levels: summary | signatures (default) | full
indxr -d summary                             # directory tree + file list only
indxr -d full                                # + doc comments, line numbers, body counts

# Filtering
indxr --filter-path src/parser               # subtree
indxr --public-only                          # public declarations only
indxr --symbol "parse"                       # symbol name search
indxr --kind function                        # by declaration kind
indxr -l rust,python                         # by language

# Git structural diffing
indxr --since main                           # diff against branch
indxr --since HEAD~5                         # diff against recent commits
indxr --since v1.0.0                         # diff against tag

# PR-aware structural diffs
indxr diff --pr 42                           # diff against PR's base branch
indxr diff --pr 42 -f json                   # JSON output
indxr diff --since main                      # diff subcommand (same as --since flag)

# Token budget
indxr --max-tokens 4000                      # progressive truncation
indxr --max-tokens 8000 --public-only        # combine with filters

# Output control
indxr --omit-imports                         # skip import listings
indxr --omit-tree                            # skip directory tree

# Caching
indxr --no-cache                             # bypass cache
indxr --cache-dir /tmp/cache                 # custom cache location

# MCP server (stdio transport — default)
indxr serve ./project                        # start MCP server (3 compound tools)
indxr serve ./project --all-tools            # expose all 26 tools (3 compound + 23 granular)
indxr serve ./project --watch                # MCP server with auto-reindex on file changes
indxr serve --watch --debounce-ms 500        # custom debounce timeout

# MCP server (Streamable HTTP transport — requires --features http)
indxr serve --http :8080                     # HTTP server on port 8080
indxr serve --http 127.0.0.1:8080 --watch    # HTTP + auto-reindex on file changes

# File watching
indxr watch                                  # watch cwd, keep INDEX.md updated
indxr watch ./project                        # watch a specific project
indxr watch -o custom.md --debounce-ms 500   # custom output and debounce
indxr watch --quiet                          # suppress progress output

# Agent setup
indxr init                                   # set up all agent configs (.mcp.json, CLAUDE.md, etc.)
indxr init --all                             # explicitly set up for all supported agents
indxr init --claude                          # Claude Code only
indxr init --cursor --windsurf               # Cursor + Windsurf only
indxr init --codex                           # OpenAI Codex CLI only
indxr init --global                          # install globally for all projects
indxr init --global --cursor                 # global Cursor only
indxr init --no-index --no-hooks             # config files only, no INDEX.md or hooks
indxr init --no-rtk                          # skip RTK hook setup
indxr init --force                           # overwrite existing files
indxr init --max-file-size 256               # custom max file size (default: 512 KB)

# Workspace / monorepo
indxr members                                # list detected workspace members
indxr serve --member core                    # serve only the "core" member
indxr watch --member core,cli                # watch specific members
indxr diff --member core                     # diff scoped to a member
indxr serve --no-workspace                   # disable workspace detection
indxr watch --no-workspace                   # disable workspace detection for watch
indxr diff --no-workspace                    # disable workspace detection for diff

# Complexity hotspots
indxr --hotspots                             # top 30 most complex functions
indxr --hotspots --filter-path src/parser    # scoped to a directory

# Dependency graph
indxr --graph dot                            # file-level DOT graph
indxr --graph mermaid                        # file-level Mermaid diagram
indxr --graph json                           # JSON graph
indxr --graph dot --graph-level symbol       # symbol-level graph
indxr --graph mermaid --filter-path src/mcp  # scoped to directory
indxr --graph dot --graph-depth 2            # limit edge hops

# Other
indxr --max-depth 3                          # limit directory depth
indxr --max-file-size 256                    # skip files > N KB (default: 512)
indxr -e "*.generated.*" -e "vendor/**"      # exclude patterns
indxr --no-gitignore                         # don't respect .gitignore
indxr --quiet                                # suppress progress output
indxr --stats                                # print indexing stats to stderr

# Note: serve, watch, and diff subcommands also accept shared indexing options:
#   --max-depth, --max-file-size, -e/--exclude, --no-gitignore, --cache-dir, --member, --no-workspace
```

## Architecture

1. Walk directory tree (`.gitignore`-aware, `ignore` crate)
2. Detect language by extension
3. Check cache (mtime + xxh3 hash)
4. Parse with tree-sitter (8 langs) or regex (19 langs) — parallel via rayon
5. Extract declarations, metadata, relationships
6. Annotate complexity metrics (tree-sitter languages only)
7. Apply filters (path, kind, visibility, symbol)
8. Apply token budget (progressive truncation)
9. Format output (Markdown/JSON/YAML)
10. Update cache

Key source files:
- `src/main.rs` — entry point, CLI dispatch
- `src/cli.rs` — clap argument definitions
- `src/indexer.rs` — core indexing orchestration
- `src/languages.rs` — `Language` enum, extension-based detection, tree-sitter/regex classification
- `src/error.rs` — custom error types (`IndxrError`)
- `src/mcp/mod.rs` — MCP server loop, JSON-RPC protocol handling
- `src/mcp/tools.rs` — tool definitions, dispatch, and 26 tool implementations (3 compound default, 23 granular via `--all-tools`)
- `src/mcp/http.rs` — Streamable HTTP transport (axum, feature-gated behind `http`)
- `src/mcp/helpers.rs` — shared structs, search/scoring/glob/string helpers
- `src/mcp/type_flow.rs` — type flow analysis for `get_type_flow` MCP tool
- `src/mcp/tests.rs` — MCP module tests
- `src/budget.rs` — token estimation and progressive truncation
- `src/filter.rs` — path/kind/visibility/symbol filtering
- `src/diff.rs` — git structural diffing
- `src/github.rs` — GitHub API client for PR-aware diffs
- `src/dep_graph.rs` — dependency graph generation (DOT, Mermaid, JSON) at file and symbol level
- `src/model/` — data model (CodebaseIndex, FileIndex, Declaration)
- `src/parser/complexity.rs` — per-function complexity metrics and hotspot analysis (tree-sitter languages)
- `src/parser/` — tree-sitter + regex parsers per language
- `src/output/` — markdown and yaml formatters (JSON output is handled inline via `serde_json` in `main.rs`)
- `src/walker/` — directory traversal
- `src/init.rs` — `indxr init` command (agent config scaffolding)
- `src/watch.rs` — file watching, debounced re-indexing (`indxr watch` + `serve --watch`)
- `src/workspace.rs` — workspace detection (Cargo, npm, Go) and multi-root support
- `src/utils.rs` — shared utility functions (word boundary matching, etc.)
- `src/cache/` — incremental binary caching
