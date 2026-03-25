# indxr Feature Plan

> **DO NOT COMMIT THIS FILE.** Local-only tracking doc. Each feature = one PR.

---

## PR 1: `indxr watch` — Live file-watching mode
**Status:** Complete (ready to merge)
**Branch:** `feat/watch` (8 commits)
**Effort:** Medium

Use `notify` crate to watch filesystem and re-index on file changes. Two modes:
- Standalone: `indxr watch ./project -o INDEX.md` — keeps INDEX.md up to date on disk
- MCP integration: `indxr serve --watch` auto-re-indexes on file changes without needing `regenerate_index`

### Tasks
- [x] Add `notify` + `notify-debouncer-mini` dependencies
- [x] Implement file watcher with debouncing (300ms default, configurable via `--debounce-ms`)
- [x] `Watch` subcommand in CLI (`indxr watch`)
- [x] Wire into MCP server (optional `--watch` flag on `serve`)
- [x] RAII `WatchGuard` for watcher lifetime management
- [x] Channel-based `ServerEvent` event loop in MCP server (multiplexes stdin + file changes)
- [x] Greedy coalescing of `FileChanged` events (preserves `StdinLine`/`StdinClosed`)
- [x] `should_trigger_reindex` filter (Language::detect gate, canonicalized path checks, hidden dir exclusion)
- [x] Shared `IndexOpts` struct to deduplicate CLI args between `serve` and `watch`
- [x] Tests (13 total: 11 unit tests for filter, 1 integration test for guard lifetime, 2 coalesce tests)
- [x] Documentation (CLAUDE.md, README.md, cli-reference.md, mcp-server.md, agent-integration.md)

### Not implemented (deferred)
- Incremental re-index (only re-parse changed files) — full re-index with cache is fast enough for now

---

## PR 2: Dependency graph export
**Status:** Not started
**Branch:** `feat/dep-graph`
**Effort:** Medium

File-to-file and symbol-to-symbol dependency graph from existing import/relationship data.

### Tasks
- [ ] Build file-level dependency graph from imports
- [ ] Build symbol-level graph from relationships + signature references
- [ ] CLI flag `--graph` with DOT and Mermaid output formats
- [ ] New MCP tool `get_dependency_graph` (scoped by path/symbol, output format param)
- [ ] Tests

---

## PR 3: Complexity metrics / hotspots
**Status:** Not started
**Branch:** `feat/complexity`
**Effort:** Medium

Per-function complexity metrics using tree-sitter AST analysis.

### Tasks
- [ ] Compute cyclomatic complexity (count branches: if/else/match/for/while/&&/||)
- [ ] Compute max nesting depth
- [ ] Compute parameter count
- [ ] Store metrics in `Declaration` (new optional fields)
- [ ] New MCP tool `get_hotspots` — returns top N most complex functions
- [ ] New MCP tool `get_health` — codebase-level health summary
- [ ] CLI flag `--hotspots` for quick CLI usage
- [ ] Tests

---

## PR 4: Cross-file type flow tracking
**Status:** Not started
**Branch:** `feat/type-flow`
**Effort:** Medium-Large

Track where types flow across function boundaries.

### Tasks
- [ ] Extract return types and parameter types from signatures (regex on existing signature strings)
- [ ] Build type-to-functions index (producers: functions returning type T, consumers: functions accepting type T)
- [ ] New MCP tool `get_type_flow` — given a type name, show producers and consumers
- [ ] Tests

---

## PR 5: Semantic code search via embeddings
**Status:** Not started
**Branch:** `feat/semantic-search`
**Effort:** Large

Optional embedding-based search using local model (fastembed / ONNX).

### Tasks
- [ ] Add optional `fastembed` or `ort` dependency (feature-gated)
- [ ] Generate embeddings for symbol names + doc comments + signatures at index time
- [ ] Store embeddings in cache (separate from structural cache)
- [ ] New MCP tool `semantic_search` — query by concept, returns ranked symbols
- [ ] Fallback to `search_relevant` when embeddings not available
- [ ] CLI flag `--semantic` to enable during indexing
- [ ] Tests + benchmarks (embedding overhead)

---

## PR 6: `indxr diff --pr <number>` — PR-aware structural diffs
**Status:** Not started
**Branch:** `feat/pr-diff`
**Effort:** Small-Medium

Structural diff for GitHub PRs.

### Tasks
- [ ] Fetch PR diff via `gh` CLI (or GitHub API with optional token)
- [ ] Apply existing `compute_structural_diff` to PR base vs head
- [ ] CLI: `indxr diff --pr 42` (reads remote/origin)
- [ ] MCP tool: extend `get_diff_summary` with optional `pr` param
- [ ] Tests

---

## PR 7: Multi-root / monorepo support
**Status:** Not started
**Branch:** `feat/monorepo`
**Effort:** Medium-Large

Support multiple roots and workspace detection.

### Tasks
- [ ] Detect workspace files (Cargo.toml workspace, package.json workspaces, go.work)
- [ ] Index each workspace member as a logical unit
- [ ] MCP: scope tools to a specific workspace member via param
- [ ] CLI: `indxr --workspace` flag to enable workspace-aware mode
- [ ] Tests

---

## PR 8: HTTP+SSE MCP transport
**Status:** Not started
**Branch:** `feat/sse-transport`
**Effort:** Medium

Add HTTP server with SSE transport alongside existing stdin/stdout.

### Tasks
- [ ] Add `axum` or `hyper` dependency
- [ ] Implement HTTP endpoint for JSON-RPC requests
- [ ] Implement SSE endpoint for server-initiated messages
- [ ] `indxr serve --http :8080` flag
- [ ] Shared index state behind `Arc<RwLock<>>`
- [ ] Tests

---

## Order of execution
1. PR 1 — watch (foundational, improves MCP experience)
2. PR 2 — dep graph (leverages existing data, high demo value)
3. PR 3 — complexity (adds new analysis dimension)
4. PR 6 — PR diff (small, high utility)
5. PR 4 — type flow (builds on dep graph work)
6. PR 8 — SSE transport (infra, unblocks multi-client)
7. PR 7 — monorepo (larger scope, benefits from all prior work)
8. PR 5 — semantic search (largest effort, feature-gated)
