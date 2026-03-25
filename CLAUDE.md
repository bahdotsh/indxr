# indxr

A fast Rust codebase indexer for AI agents. Extracts structural maps (declarations, imports, tree) using tree-sitter and regex parsing across 27 languages.

## Codebase Navigation — MUST USE indxr MCP tools

An MCP server called `indxr` is available. **Always use indxr tools before the Read tool.** Do NOT read full source files as a first step — use the MCP tools to explore, then read only what you need.

### Token savings reference

| Action | Approx tokens | When to use |
|--------|--------------|-------------|
| `get_tree` | ~200-400 | First: understand directory layout |
| `get_file_summary` | ~200-400 | Understand a file without reading it |
| `batch_file_summaries` | ~400-1200 | Summarize multiple files in one call (vs N calls) |
| `get_file_context` | ~400-600 | Understand dependencies and reverse deps |
| `lookup_symbol` | ~100-200 | Find a specific function/type across codebase |
| `search_signatures` | ~100-300 | Find functions by signature pattern |
| `search_relevant` | ~200-400 | Find files/symbols by concept or partial name (supports `kind` filter) |
| `explain_symbol` | ~100-300 | Everything to USE a symbol without reading its body |
| `get_public_api` | ~200-500 | Public API surface of a file or module |
| `get_callers` | ~100-300 | Who references this symbol (imports + signatures) |
| `get_related_tests` | ~100-200 | Find tests for a symbol by naming convention |
| `get_diff_summary` | ~200-500 | Structural changes since a git ref (vs reading raw diffs) |
| `read_source` (symbol) | ~50-300 | Read one function/struct. Supports `symbols` array and `collapse`. |
| `get_token_estimate` | ~100 | Check cost before reading. Supports `directory`/`glob`. |
| `Read` (full file) | **500-10000+** | ONLY when editing or need exact formatting |

**Typical exploration: ~650 tokens vs ~3000+ for reading a full file (5x reduction).**

### Exploration workflow (follow this order)

1. `search_relevant` — find files/symbols related to your task by concept, partial name, or type pattern. Searches across paths, names, signatures, and doc comments with ranked results. Supports `kind` filter (e.g., `kind: "fn"`). **Start here when you know what you're looking for but not where it is.**
2. `get_tree` — see directory/file layout. Use `path` param to scope to a subtree.
3. `get_file_summary` — get a complete overview of any file (declarations, imports, counts) WITHOUT reading it. Use `batch_file_summaries` for multiple files at once.
4. `get_file_context` — understand a file's reverse dependencies (who imports it) and related files (tests, siblings).
5. `lookup_symbol` — find declarations by name (case-insensitive substring) across all indexed files.
6. `explain_symbol` — get full interface details for a symbol (signature, doc comment, relationships, metadata) without reading its body.
7. `search_signatures` — find functions/methods by signature substring (e.g., `"-> Result<"`, `"&mut self"`).
8. `get_callers` — find who references a symbol (checks imports and signatures across all files).
9. `get_token_estimate` — before deciding to `Read` a file, check how many tokens it costs. Supports `directory` or `glob` for bulk estimation.
10. `read_source` — read source code by **symbol name** or explicit line range. Cap: 200 lines. Use `symbols` array to read multiple in one call (500 line cap). Use `collapse: true` to fold nested bodies.
11. `get_public_api` — get only public declarations with signatures for a file or directory. Minimal output for "how do I use this module?" questions.
12. `get_related_tests` — find test functions for a symbol by naming convention and file association.
13. `list_declarations` — list all declarations in a file. Use `kind` filter, `shallow` or `compact` mode to reduce output.
14. `get_imports` — get import statements for a file.
15. `get_stats` — codebase stats: file count, line count, language breakdown, indexing duration.
16. `get_diff_summary` — get structural changes since a git ref. Shows added/removed/modified declarations without reading full diffs.
17. `regenerate_index` — re-index after code changes. Updates INDEX.md, refreshes in-memory index, and reports what changed (delta).

### Compact output mode
Tools that return lists (`lookup_symbol`, `list_declarations`, `search_signatures`, `search_relevant`) support a `compact: true` param that returns columnar `{columns, rows}` format instead of objects, saving ~30% tokens.

### When to use the Read tool instead
- You need to **edit** a file (Read is required before Edit)
- You need exact formatting/whitespace that `read_source` doesn't preserve
- The file is not a source file (e.g., CLAUDE.md, Cargo.toml, docs, config files)

### DO NOT
- Read full source files just to understand what's in them — use `get_file_summary`
- Read full source files to review code — use `get_file_summary` to triage, then `read_source` on specific symbols
- Dump all files into context — use MCP tools to be surgical
- Read a file without first checking `get_token_estimate` if you're unsure about its size

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

# Token budget
indxr --max-tokens 4000                      # progressive truncation
indxr --max-tokens 8000 --public-only        # combine with filters

# Output control
indxr --omit-imports                         # skip import listings
indxr --omit-tree                            # skip directory tree

# Caching
indxr --no-cache                             # bypass cache
indxr --cache-dir /tmp/cache                 # custom cache location

# MCP server
indxr serve ./project                        # start MCP server (stdin/stdout JSON-RPC 2.0)

# Other
indxr --max-depth 3                          # limit directory depth
indxr --max-file-size 256                    # skip files > N KB
indxr -e "*.generated.*" -e "vendor/**"      # exclude patterns
indxr --no-gitignore                         # don't respect .gitignore
indxr --quiet                                # suppress progress output
indxr --stats                                # print indexing stats to stderr
```

## Architecture

1. Walk directory tree (`.gitignore`-aware, `ignore` crate)
2. Detect language by extension
3. Check cache (mtime + xxh3 hash)
4. Parse with tree-sitter (8 langs) or regex (19 langs) — parallel via rayon
5. Extract declarations, metadata, relationships
6. Apply filters (path, kind, visibility, symbol)
7. Apply token budget (progressive truncation)
8. Format output (Markdown/JSON/YAML)
9. Update cache

Key source files:
- `src/main.rs` — entry point, CLI dispatch
- `src/cli.rs` — clap argument definitions
- `src/indexer.rs` — core indexing orchestration
- `src/mcp/mod.rs` — MCP server loop, JSON-RPC protocol handling
- `src/mcp/tools.rs` — tool definitions, dispatch, and 18 tool implementations
- `src/mcp/helpers.rs` — shared structs, search/scoring/glob/string helpers
- `src/mcp/tests.rs` — MCP module tests
- `src/budget.rs` — token estimation and progressive truncation
- `src/filter.rs` — path/kind/visibility/symbol filtering
- `src/diff.rs` — git structural diffing
- `src/model/` — data model (CodebaseIndex, FileIndex, Declaration)
- `src/parser/` — tree-sitter + regex parsers per language
- `src/output/` — markdown/json/yaml formatters
- `src/walker/` — directory traversal
- `src/cache/` — incremental binary caching
