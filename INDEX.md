# Codebase Index: indxr

> Generated: 2026-03-27 07:25:58 UTC | Files: 53 | Lines: 23500
> Languages: Markdown (14), Python (1), Rust (36), Shell (1), TOML (1)

## Directory Structure

```
indxr/
  CLAUDE.md
  Cargo.toml
  INDEX.md
  README.md
  benchmark.sh
  docs/
    agent-integration.md
    caching.md
    cli-reference.md
    dep-graph.md
    filtering.md
    git-diffing.md
    languages.md
    mcp-server.md
    output-formats.md
    token-budget.md
  roadmap.md
  src/
    budget.rs
    cache/
      fingerprint.rs
      mod.rs
    cli.rs
    dep_graph.rs
    diff.rs
    error.rs
    filter.rs
    indexer.rs
    init.rs
    languages.rs
    main.rs
    mcp/
      helpers.rs
      mod.rs
      tests.rs
      tools.rs
    model/
      declarations.rs
      mod.rs
    output/
      markdown.rs
      mod.rs
      yaml.rs
    parser/
      mod.rs
      queries/
        c.rs
        cpp.rs
        go.rs
        java.rs
        javascript.rs
        mod.rs
        python.rs
        rust.rs
        typescript.rs
      regex_parser.rs
      tree_sitter_parser.rs
    utils.rs
    walker/
      mod.rs
    watch.rs
  token_count.py
```

---

## Public API Surface

**CLAUDE.md**
- `# indxr`
- `# Basic indexing`
- `# Detail levels: summary | signatures (default) | full`
- `# Filtering`
- `# Git structural diffing`
- `# Token budget`
- `# Output control`
- `# Caching`
- `# MCP server`
- `# File watching`
- `# Agent setup`
- `# Dependency graph`
- `# Other`

**Cargo.toml**
- `[package]`
- `[dependencies]`
- `[dev-dependencies]`

**INDEX.md**
- `# Codebase Index: indxr`

**README.md**
- `# indxr`
- `# Codebase Index: my-project`

**benchmark.sh**
- `count_tokens_openai()`
- `count_tokens_claude()`
- `_fallback_count()`
- `fmt_num()`
- `pct()`
- `ratio()`
- `sep()`
- `section()`
- `run_indxr()`
- `run_indxr_cold()`
- `run_indxr_warm()`
- `mcp_query()`
- `fmt_tok()`
- `fmt_ratio()`
- `benchmark_project()`

**docs/agent-integration.md**
- `# Agent Integration Guide`
- `# Generate a compact index`
- `# Include in Codex instructions`
- `# Full structured index`
- `# Pipe to your agent`
- `# Generate a scoped index for the area you're working in`
- `# Show structural changes since the last release`
- `# Show what changed on this branch`
- `# Public API only, compact`
- `# Summary for high-level overview`
- `# Full signatures for detailed reference`
- `# .github/workflows/index.yml`
- `# Only the Rust backend`
- `# Only the TypeScript frontend`

**docs/caching.md**
- `# Caching`

**docs/cli-reference.md**
- `# CLI Reference`
- `# Index current directory`
- `# Index a specific project`
- `# Write to file`
- `# JSON for programmatic use`
- `# YAML for human-readable structured output`
- `# Summary only (no declarations)`
- `# Full detail with metadata`
- `# Only Rust and Python files`
- `# Only files under src/parser`
- `# Only public functions`
- `# Find all "parse" symbols`
- `# Combined: public structs in src/model`
- `# Changes since main branch`
- `# Changes since a tag`
- `# Changes in last 5 commits`
- `# Changes since a commit hash`
- `# JSON diff output`
- `# Fit in 4000 tokens`
- `# Compact public API within budget`
- `# Budget with JSON output`
- `# Limit depth`
- `# Exclude test directories`
- `# Include gitignored files`
- `# Skip large files`
- `# Watch current directory, keep INDEX.md updated`
- `# Watch a specific project`
- `# Custom output path`
- `# Slower debounce for high-frequency saves`
- `# Quiet mode (no progress output)`
- `# MCP server with auto-reindex`
- `# Set up for all agents (Claude Code, Cursor, Windsurf)`
- `# Claude Code only`
- `# Cursor and Windsurf only`
- `# Config files only, skip INDEX.md generation`
- `# Skip PreToolUse hooks`
- `# Overwrite existing files`
- `# Re-run after initial setup (skips existing files)`
- `# File-level DOT graph (for Graphviz)`
- `# File-level Mermaid diagram`
- `# JSON graph for programmatic use`
- `# Symbol-level graph (trait impls, method relationships)`
- `# Scoped to a directory`
- `# Limit to 2 hops from scoped files`
- `# Write to file`
- `# Compact public API index for an agent`
- `# Quick structural diff of backend changes`
- `# Full JSON index without cache`

**docs/dep-graph.md**
- `# Dependency Graph`
- `# Graph centered on the parser module`
- `# Graph centered on the MCP module`
- `# Only direct dependencies (1 hop)`
- `# Up to 2 hops`

**docs/filtering.md**
- `# Filtering & Scoped Output`
- `# Single language`
- `# Multiple languages`
- `# Config files only`
- `# Find anything with "parse" in the name`
- `# Find the Cache type`
- `# Find all "new" constructors`
- `# Public functions in src/parser`
- `# All structs named "Config" in Rust files`
- `# Public API of the model layer`
- `# Find all async functions`
- `# Limit directory depth`
- `# Skip large files (default is 512 KB)`
- `# Exclude patterns`
- `# Include gitignored files`
- `# Skip imports section`
- `# Skip directory tree`
- `# Both — just declarations`

**docs/git-diffing.md**
- `# Git-Aware Structural Diffing`
- `# Since a branch`
- `# Since a tag`
- `# Since a relative commit`
- `# Since a specific commit`
- `# Structural Changes (since main)`
- `# Only show structural changes in Rust files`
- `# Only changes in a specific directory`
- `# JSON output for programmatic use`
- `# Only public API changes`

**docs/languages.md**
- `# Supported Languages`
- `# Only Rust`
- `# Rust and Python`
- `# All config files`

**docs/mcp-server.md**
- `# MCP Server`
- `# Send initialize`
- `# Send initialized notification`
- `# Call a tool`

**docs/output-formats.md**
- `# Output Formats`
- `# or just`
- `# Codebase Index: project-name`
- `# Codebase Index: my-project`
- `# or just`

**docs/token-budget.md**
- `# Token Budget`
- `# Public API within budget`
- `# Scoped to a directory within budget`
- `# Specific language within budget`

**roadmap.md**
- `# indxr Roadmap`

**src/budget.rs**
- `pub fn estimate_tokens(text: &str) -> usize`
- `pub fn apply_token_budget(index: &mut CodebaseIndex, max_tokens: usize)`

**src/cache/fingerprint.rs**
- `pub fn compute_hash(content: &[u8]) -> u64`
- `pub fn metadata_matches( cached_mtime: u64, cached_size: u64, current_mtime: u64, current_size: u64, ) -> bool`

**src/cache/mod.rs**
- `pub mod fingerprint`
- `pub struct Cache`

**src/cli.rs**
- `pub struct Cli`
- `pub struct IndexOpts`
- `pub enum Command`
- `pub enum OutputFormat`
- `pub enum GraphFormat`
- `pub enum GraphLevel`

**src/dep_graph.rs**
- `pub struct DepGraph`
- `pub struct GraphNode`
- `pub enum NodeKind`
- `pub struct GraphEdge`
- `pub enum EdgeKind`
- `pub fn build_file_graph( index: &CodebaseIndex, scope: Option<&str>, depth: Option<usize>, ) -> DepGraph`
- `pub fn build_symbol_graph( index: &CodebaseIndex, scope: Option<&str>, depth: Option<usize>, ) -> DepGraph`
- `pub fn format_dot(graph: &DepGraph) -> String`
- `pub fn format_mermaid(graph: &DepGraph) -> String`
- `pub fn format_json(graph: &DepGraph) -> Value`

**src/diff.rs**
- `pub struct StructuralDiff`
- `pub struct FileDiff`
- `pub struct DeclChange`
- `pub struct DeclModification`
- `pub fn get_changed_files(root: &Path, since_ref: &str) -> Result<Vec<PathBuf>>`
- `pub fn get_added_files(root: &Path, since_ref: &str) -> Result<Vec<PathBuf>>`
- `pub fn get_deleted_files(root: &Path, since_ref: &str) -> Result<Vec<PathBuf>>`
- `pub fn get_file_at_ref(root: &Path, file_path: &Path, git_ref: &str) -> Result<Option<String>>`
- `pub fn compute_structural_diff( current_index: &CodebaseIndex, old_files: &HashMap<PathBuf, FileIndex>, changed_paths: &[PathBuf], ) -> StructuralDiff`
- `pub fn format_diff_markdown(diff: &StructuralDiff) -> String`
- `pub fn format_diff_json(diff: &StructuralDiff) -> Result<String>`

**src/error.rs**
- `pub enum IndxrError`

**src/filter.rs**
- `pub struct FilterOptions`
- `pub fn apply_filters(index: &mut CodebaseIndex, opts: &FilterOptions)`

**src/indexer.rs**
- `pub struct IndexConfig`
- `pub struct ParseResult`
- `pub fn parse_files( files: &[&FileEntry], cache: &Cache, registry: &ParserRegistry, ) -> Vec<ParseResult>`
- `pub fn collect_results( results: Vec<ParseResult>, cache: &mut Cache, ) -> (Vec<FileIndex>, usize, HashMap<String, usize>, usize)`
- `pub fn build_index(config: &IndexConfig) -> anyhow::Result<CodebaseIndex>`
- `pub fn generate_index_markdown(index: &CodebaseIndex) -> anyhow::Result<String>`
- `pub fn regenerate_index_file(config: &IndexConfig) -> anyhow::Result<CodebaseIndex>`

**src/init.rs**
- `pub struct InitOptions`
- `pub fn run_init(opts: InitOptions) -> Result<()>`

**src/languages.rs**
- `pub enum Language`

**src/mcp/helpers.rs**
- `pub(super) fn tool_result(content: Value) -> Value`
- `pub(super) fn tool_error(msg: &str) -> Value`
- `pub(super) struct SymbolMatch`
- `pub(super) fn find_symbols_in_decl( decl: &Declaration, query: &str, file_path: &str, results: &mut Vec<SymbolMatch>, limit: usize, )`
- `pub(super) struct SignatureMatch`
- `pub(super) fn find_signatures_in_decl( decl: &Declaration, query: &str, file_path: &str, results: &mut Vec<SignatureMatch>, limit: usize, )`
- `pub(super) fn filter_declarations<'a>( decls: &'a [Declaration], kind: &DeclKind, ) -> Vec<&'a Declaration>`
- `pub(super) struct ShallowDeclaration`
- `pub(super) fn to_shallow(decl: &Declaration) -> ShallowDeclaration`
- `pub(super) fn file_summary_data(file: &FileIndex) -> Value`
- `pub(super) fn find_decl_by_name<'a>( decls: &'a [Declaration], name: &str, ) -> Option<&'a Declaration>`
- `pub(super) fn read_line_range(path: &Path, start: usize, end: usize) -> Result<String, String>`
- `pub(super) fn find_file<'a>(index: &'a CodebaseIndex, path: &str) -> Option<&'a FileIndex>`
- `pub(super) struct RelevanceMatch`
- `pub(super) fn score_match(text: &str, query: &str, terms: &[&str]) -> u32`
- `pub(super) fn score_decls_recursive( decls: &[Declaration], file_path: &str, query: &str, terms: &[&str], results: &mut Vec<RelevanceMatch>, kind_filter: Option<&DeclKind>, )`
- `pub(super) fn compile_glob_matcher(pattern: &str) -> Option<GlobMatcher>`
- `pub(super) fn simple_glob_match(pattern: &str, path: &str) -> bool`
- `pub(super) fn split_identifier(name: &str) -> Vec<String>`
- `pub(super) fn bigram_similarity(a: &str, b: &str) -> f64`
- `pub(super) fn collapse_nested_bodies(source: &str) -> String`
- `pub(super) fn is_compact(args: &Value) -> bool`
- `pub(super) fn serialize_compact<T: Serialize>(items: &[T], columns: &[&str]) -> Value`
- `pub(super) fn to_compact_rows(columns: &[&str], items: &[Value]) -> Value`
- `pub(super) fn collect_public_decls(decls: &[Declaration], file_path: &str, out: &mut Vec<Value>)`
- `pub(super) fn find_tests_for_symbol( decls: &[Declaration], symbol_lower: &str, file_path: &str, results: &mut Vec<Value>, reason: &str, )`
- `pub(super) fn explain_decl(decl: &Declaration, file_path: &str) -> Value`
- `pub(super) const APPROX_SUMMARY_TOKENS: usize = 300`

**src/mcp/mod.rs**
- `pub fn run_mcp_server( mut index: CodebaseIndex, config: IndexConfig, watch: bool, debounce_ms: u64, ) -> anyhow::Result<()>`

**src/mcp/tools.rs**
- `pub(super) fn tool_definitions() -> Value`
- `pub(super) fn handle_tool_call(index: &CodebaseIndex, name: &str, args: &Value) -> Value`
- `pub(super) fn tool_regenerate_index(index: &mut CodebaseIndex, config: &IndexConfig) -> Value`
- `pub(super) fn tool_lookup_symbol(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_list_declarations(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_search_signatures(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_tree(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_imports(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_stats(index: &CodebaseIndex) -> Value`
- `pub(super) fn tool_get_file_summary(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_read_source(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_file_context(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_token_estimate(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_search_relevant(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_diff_summary( index: &CodebaseIndex, config: &IndexConfig, registry: &ParserRegistry, args: &Value, ) -> Value`
- `pub(super) fn tool_batch_file_summaries(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_callers(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_public_api(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_explain_symbol(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_related_tests(index: &CodebaseIndex, args: &Value) -> Value`
- `pub(super) fn tool_get_dependency_graph(index: &CodebaseIndex, args: &Value) -> Value`

**src/model/declarations.rs**
- `pub struct Declaration`
- `pub struct Relationship`
- `pub enum RelKind`
- `pub enum DeclKind`
- `pub enum Visibility`

**src/model/mod.rs**
- `pub mod declarations`
- `pub enum DetailLevel`
- `pub struct CodebaseIndex`
- `pub struct FileIndex`
- `pub struct Import`
- `pub struct TreeEntry`
- `pub struct IndexStats`

**src/output/markdown.rs**
- `pub struct MarkdownOptions`
- `pub struct MarkdownFormatter`

**src/output/mod.rs**
- `pub mod markdown`
- `pub mod yaml`
- `pub trait OutputFormatter`

**src/output/yaml.rs**
- `pub struct YamlFormatter`

**src/parser/mod.rs**
- `pub mod queries`
- `pub mod regex_parser`
- `pub mod tree_sitter_parser`
- `pub trait LanguageParser: Send + Sync`
- `pub struct ParserRegistry`

**src/parser/queries/c.rs**
- `pub struct CExtractor`

**src/parser/queries/cpp.rs**
- `pub struct CppExtractor`

**src/parser/queries/go.rs**
- `pub struct GoExtractor`

**src/parser/queries/java.rs**
- `pub struct JavaExtractor`

**src/parser/queries/javascript.rs**
- `pub struct JavaScriptExtractor`

**src/parser/queries/mod.rs**
- `pub mod c`
- `pub mod cpp`
- `pub mod go`
- `pub mod java`
- `pub mod javascript`
- `pub mod python`
- `pub mod rust`
- `pub mod typescript`
- `pub trait DeclExtractor: Send + Sync`
- `pub fn get_extractor(language: &Language) -> Box<dyn DeclExtractor>`

**src/parser/queries/python.rs**
- `pub struct PythonExtractor`

**src/parser/queries/rust.rs**
- `pub struct RustExtractor`

**src/parser/queries/typescript.rs**
- `pub struct TypeScriptExtractor`

**src/parser/regex_parser.rs**
- `pub struct RegexParser`

**src/parser/tree_sitter_parser.rs**
- `pub struct TreeSitterParser`

**src/utils.rs**
- `pub fn contains_word_boundary(text: &str, word: &str) -> bool`

**src/walker/mod.rs**
- `pub struct WalkResult`
- `pub struct FileEntry`
- `pub fn walk_directory( root: &Path, respect_gitignore: bool, max_file_size: u64, max_depth: Option<usize>, exclude_patterns: &[String], ) -> Result<WalkResult>`

**src/watch.rs**
- `pub struct WatchGuard`
- `pub struct WatchOptions`
- `pub fn run_watch(opts: WatchOptions) -> Result<()>`
- `pub fn spawn_watcher( root: &Path, cache_dir: &Path, output_path: &Path, debounce_ms: u64, ) -> Result<(mpsc::Receiver<()>, WatchGuard)>`

**token_count.py**
- `def count_openai(text: str) -> int | None`
- `def count_claude(text: str) -> int | None`
- `def main()`

---

## CLAUDE.md

**Language:** Markdown | **Size:** 10.1 KB | **Lines:** 171

**Declarations:**

---

## Cargo.toml

**Language:** TOML | **Size:** 999 B | **Lines:** 41

**Imports:**
- `anyhow`
- `bincode`
- `chrono`
- `clap`
- `globset`
- `ignore`
- `rayon`
- `regex`
- `serde`
- `serde_json`
- *... and 15 more imports*

**Declarations:**

---

## INDEX.md

**Language:** Markdown | **Size:** 53.1 KB | **Lines:** 2032

**Declarations:**

---

## README.md

**Language:** Markdown | **Size:** 8.4 KB | **Lines:** 229

**Declarations:**

---

## benchmark.sh

**Language:** Shell | **Size:** 24.9 KB | **Lines:** 620

**Declarations:**

---

## docs/agent-integration.md

**Language:** Markdown | **Size:** 15.7 KB | **Lines:** 465

**Declarations:**

---

## docs/caching.md

**Language:** Markdown | **Size:** 2.5 KB | **Lines:** 87

**Declarations:**

---

## docs/cli-reference.md

**Language:** Markdown | **Size:** 8.2 KB | **Lines:** 324

**Declarations:**

---

## docs/dep-graph.md

**Language:** Markdown | **Size:** 2.5 KB | **Lines:** 125

**Declarations:**

---

## docs/filtering.md

**Language:** Markdown | **Size:** 3.3 KB | **Lines:** 166

**Declarations:**

---

## docs/git-diffing.md

**Language:** Markdown | **Size:** 3.3 KB | **Lines:** 154

**Declarations:**

---

## docs/languages.md

**Language:** Markdown | **Size:** 6.8 KB | **Lines:** 332

**Declarations:**

---

## docs/mcp-server.md

**Language:** Markdown | **Size:** 14.6 KB | **Lines:** 553

**Declarations:**

---

## docs/output-formats.md

**Language:** Markdown | **Size:** 4.8 KB | **Lines:** 217

**Declarations:**

---

## docs/token-budget.md

**Language:** Markdown | **Size:** 3.3 KB | **Lines:** 92

**Declarations:**

---

## roadmap.md

**Language:** Markdown | **Size:** 2.0 KB | **Lines:** 50

**Declarations:**

---

## src/budget.rs

**Language:** Rust | **Size:** 8.5 KB | **Lines:** 263

**Imports:**
- `crate::model::CodebaseIndex`
- `crate::model::declarations::{Declaration, Visibility}`

**Declarations:**

`fn estimate_index_tokens(index: &CodebaseIndex) -> usize`

`fn estimate_declarations_tokens(decls: &[Declaration]) -> usize`

`fn estimate_file_tokens(file: &crate::model::FileIndex) -> usize`

`fn file_importance(file: &crate::model::FileIndex) -> i64`

`fn count_public_decls(decls: &[Declaration]) -> usize`

`fn truncate_doc_comments(decls: &mut [Declaration], max_len: usize) -> usize`

`fn strip_doc_comments(decls: &mut [Declaration]) -> usize`

`fn remove_private_declarations(decls: &[Declaration]) -> Vec<Declaration>`

`fn strip_children(decls: &mut [Declaration])`

---

## src/cache/fingerprint.rs

**Language:** Rust | **Size:** 483 B | **Lines:** 17

**Imports:**
- `xxhash_rust::xxh3::xxh3_64`

**Declarations:**

---

## src/cache/mod.rs

**Language:** Rust | **Size:** 3.6 KB | **Lines:** 135

**Imports:**
- `std::collections::HashMap`
- `std::fs`
- `std::path::{Path, PathBuf}`
- `anyhow::Result`
- `serde::{Deserialize, Serialize}`
- `self::fingerprint::{compute_hash, metadata_matches}`
- `crate::model::FileIndex`

**Declarations:**

`const CACHE_VERSION: u32 = 2`

`const CACHE_FILENAME: &str = "cache.bin"`

`struct CacheStore`
> Fields: `version: u32`, `entries: HashMap<PathBuf, CacheEntry>`

`struct CacheEntry`
> Fields: `mtime: u64`, `size: u64`, `content_hash: u64`, `file_index: FileIndex`

**`impl Cache`**
  `pub fn load(cache_dir: &Path) -> Self`

  `pub fn disabled() -> Self`

  `fn empty_store() -> CacheStore`

  `pub fn get(&self, relative_path: &Path, size: u64, mtime: u64) -> Option<FileIndex>`

  `pub fn insert( &mut self, relative_path: &Path, size: u64, mtime: u64, content: &[u8], file_index: FileIndex, )`

  `pub fn prune(&mut self, existing_paths: &[PathBuf])`

  `pub fn save(&self) -> Result<()>`

  `pub fn len(&self) -> usize`


---

## src/cli.rs

**Language:** Rust | **Size:** 6.1 KB | **Lines:** 239

**Imports:**
- `std::path::PathBuf`
- `clap::{Args, Parser, Subcommand}`
- `crate::model::DetailLevel`

**Declarations:**

---

## src/dep_graph.rs

**Language:** Rust | **Size:** 58.0 KB | **Lines:** 1791

**Imports:**
- `std::collections::{HashMap, HashSet}`
- `std::path::Path`
- `serde::Serialize`
- `serde_json::{Value, json}`
- `crate::model::CodebaseIndex`
- `crate::model::declarations::{Declaration, RelKind}`
- `crate::utils::contains_word_boundary`

**Declarations:**

`struct PathInfo<'a>`
> Fields: `path: &'a Path`, `lower: String`, `no_ext_lower: String`

`fn resolve_import<'a>( import_text: &str, from_file: &Path, path_infos: &'a [PathInfo<'a>], ) -> Option<&'a Path>`

`fn normalize_import_separators(text: &str) -> String`

`fn is_known_extension(ext: &str) -> bool`

`fn resolve_relative_import<'a>( text: &str, from_file: &Path, path_infos: &'a [PathInfo<'a>], ) -> Option<&'a Path>`

`fn find_from_keyword(text: &str) -> Option<usize>`

`fn extract_path_from_import(text: &str) -> Option<&str>`

`fn extract_quoted_path(text: &str) -> Option<&str>`

`fn strip_import_prefixes(normalized: &str) -> &str`

`fn match_path_candidate<'a>( candidate_lower: &str, path_infos: &'a [PathInfo<'a>], ) -> Option<&'a Path>`

`fn limit_depth_file( adjacency: &HashMap<String, HashSet<String>>, seeds: &HashSet<&str>, max_depth: usize, ) -> HashMap<String, HashSet<String>>`

`struct SymInfo`
> Fields: `id: String`, `name: String`, `signature: String`, `signature_lower: String`, `relationships: Vec<(String, RelKind)>`

`fn symbol_id(file_path: &str, name: &str, name_counts: &mut HashMap<String, usize>) -> String`

`fn collect_symbols_ext( decls: &[Declaration], file_path: &str, name_counts: &mut HashMap<String, usize>, out: &mut Vec<SymInfo>, )`

`fn limit_depth_symbol( edges: Vec<GraphEdge>, seeds: &HashSet<&str>, max_depth: usize, ) -> Vec<GraphEdge>`

`mod tests`

---

## src/diff.rs

**Language:** Rust | **Size:** 16.3 KB | **Lines:** 484

**Imports:**
- `std::collections::{HashMap, HashSet}`
- `std::path::{Path, PathBuf}`
- `std::process::Command`
- `anyhow::{Context, Result}`
- `serde::Serialize`
- `crate::model::declarations::{DeclKind, Declaration}`
- `crate::model::{CodebaseIndex, FileIndex}`

**Declarations:**

`fn git_diff_names(root: &Path, since_ref: &str, diff_filter: Option<&str>) -> Result<Vec<PathBuf>>`

`fn diff_declarations(path: PathBuf, old: &[Declaration], new: &[Declaration]) -> FileDiff`

`fn flatten_declarations(decls: &[Declaration]) -> HashMap<(DeclKind, String), String>`

`mod tests`

---

## src/error.rs

**Language:** Rust | **Size:** 324 B | **Lines:** 14

**Imports:**
- `thiserror::Error`

**Declarations:**

---

## src/filter.rs

**Language:** Rust | **Size:** 4.6 KB | **Lines:** 138

**Imports:**
- `std::path::Path`
- `crate::model::CodebaseIndex`
- `crate::model::declarations::{DeclKind, Declaration, Visibility}`

**Declarations:**

**`impl FilterOptions`**
  `pub fn is_active(&self) -> bool`


`fn filter_declarations_by_kind(decls: &[Declaration], kind: &DeclKind) -> Vec<Declaration>`

`fn filter_declarations_by_visibility(decls: &[Declaration]) -> Vec<Declaration>`

`fn filter_declarations_by_symbol(decls: &[Declaration], query: &str) -> Vec<Declaration>`

`fn recalculate_stats(index: &mut CodebaseIndex)`

---

## src/indexer.rs

**Language:** Rust | **Size:** 5.3 KB | **Lines:** 176

**Imports:**
- `std::collections::HashMap`
- `std::fs`
- `std::path::PathBuf`
- `rayon::prelude::*`
- `crate::cache::Cache`
- `crate::model::{CodebaseIndex, FileIndex, IndexStats}`
- `crate::output::OutputFormatter`
- `crate::output::markdown::{MarkdownFormatter, MarkdownOptions}`
- `crate::parser::ParserRegistry`
- `crate::walker::{self, FileEntry}`

**Declarations:**

---

## src/init.rs

**Language:** Rust | **Size:** 24.9 KB | **Lines:** 705

**Imports:**
- `std::fs`
- `std::path::{Path, PathBuf}`
- `std::process::Command as ProcessCommand`
- `anyhow::Result`
- `crate::indexer::{self, IndexConfig}`
- `crate::model::DetailLevel`
- `crate::output::OutputFormatter`
- `crate::output::markdown::{MarkdownFormatter, MarkdownOptions}`

**Declarations:**

`enum WriteResult`
> Variants: `Created`, `Skipped`, `Appended`

`fn display_relative(path: &Path, root: &Path) -> String`

`fn write_file_safe(path: &Path, content: &str, force: bool) -> Result<WriteResult>`

`fn setup_claude( root: &Path, force: bool, include_hooks: bool, include_rtk: bool, ) -> Result<Vec<WriteResult>>`

`fn setup_cursor(root: &Path, force: bool, include_rtk: bool) -> Result<Vec<WriteResult>>`

`fn setup_windsurf(root: &Path, force: bool, include_rtk: bool) -> Result<Vec<WriteResult>>`

`fn detect_rtk() -> bool`

`fn setup_rtk_claude(root: &Path, force: bool) -> Result<Vec<WriteResult>>`

`const RTK_HOOK_SCRIPT: &str = r#"#!/bin/bash # RTK rewrite hook for Claude Code — installed by indxr init # Intercepts Bash commands and rewrites them through rtk for token compression # Skip silently if rtk or jq is not installed command -v rtk >/dev/null 2>&1 || exit 0 command -v jq >/dev/null 2>&1 || exit 0 # Extract the command from tool input COMMAND=$(printf '%s' "$TOOL_INPUT" | jq -r '.command // empty') [ -z "$COMMAND" ] && exit 0 # Ask rtk to rewrite the command REWRITTEN=$(rtk rewrite "$COMMAND" 2>/dev/null) EXIT_CODE=$? case $EXIT_CODE in 0) # Rewrite successful — auto-allow with rewritten command ESCAPED=$(printf '%s' "$REWRITTEN" | jq -Rs .) echo "`

`fn setup_gitignore(root: &Path) -> Result<WriteResult>`

`fn generate_index(root: &Path, max_file_size: u64) -> Result<WriteResult>`

`fn mcp_json_content() -> String`

`fn claude_md_content(root: &Path, include_rtk: bool) -> String`

`fn claude_settings_content(include_rtk: bool) -> String`

`fn cursorrules_content(include_rtk: bool) -> String`

`fn windsurfrules_content(include_rtk: bool) -> String`

`mod tests`

---

## src/languages.rs

**Language:** Rust | **Size:** 6.3 KB | **Lines:** 178

**Imports:**
- `std::fmt`
- `std::path::Path`
- `serde::{Deserialize, Serialize}`

**Declarations:**

**`impl Language`**
  `pub fn detect(path: &Path) -> Option<Self>`

  `pub fn name(&self) -> &str`

  `pub fn from_name(name: &str) -> Option<Self>`

  `pub fn uses_tree_sitter(&self) -> bool`


**`impl fmt::Display for Language`**
  `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result`


---

## src/main.rs

**Language:** Rust | **Size:** 10.2 KB | **Lines:** 364

**Imports:**
- `std::collections::HashMap`
- `std::fs`
- `std::time::Instant`
- `anyhow::Result`
- `clap::Parser`
- `crate::cache::Cache`
- `crate::cli::{Cli, Command, GraphFormat, GraphLevel, OutputFormat}`
- `crate::filter::FilterOptions`
- `crate::languages::Language`
- `crate::model::declarations::DeclKind`
- *... and 5 more imports*

**Declarations:**

`mod budget`

`mod cache`

`mod cli`

`mod dep_graph`

`mod diff`

`mod error`

`mod filter`

`mod indexer`

`mod init`

`mod languages`

`mod mcp`

`mod model`

`mod output`

`mod parser`

`mod utils`

`mod walker`

`mod watch`

`fn main() -> Result<()>`

`fn index_config_from(opts: &cli::IndexOpts) -> indexer::IndexConfig`

`fn handle_git_diff( root: &std::path::Path, since_ref: &str, current_files: &[model::FileIndex], registry: &ParserRegistry, cli: &Cli, ) -> Result<()>`

---

## src/mcp/helpers.rs

**Language:** Rust | **Size:** 25.6 KB | **Lines:** 811

**Imports:**
- `std::collections::{HashMap, HashSet}`
- `std::path::Path`
- `globset::{GlobBuilder, GlobMatcher}`
- `serde::Serialize`
- `serde_json::{Value, json}`
- `crate::model::declarations::{DeclKind, Declaration, Visibility}`
- `crate::model::{CodebaseIndex, FileIndex}`
- `pub(super) use crate::utils::contains_word_boundary`

**Declarations:**

---

## src/mcp/mod.rs

**Language:** Rust | **Size:** 13.0 KB | **Lines:** 406

**Imports:**
- `std::io::{self, BufRead, Write}`
- `std::sync::mpsc`
- `std::thread`
- `serde::Deserialize`
- `serde::Serialize`
- `serde_json::{self, Value, json}`
- `crate::indexer::{self, IndexConfig}`
- `crate::model::CodebaseIndex`
- `crate::parser::ParserRegistry`
- `self::tools::{
    handle_tool_call, tool_definitions, tool_get_diff_summary, tool_regenerate_index,
}`

**Declarations:**

`mod helpers`

`mod tools`

`mod tests`

`struct JsonRpcRequest`
> Fields: `jsonrpc: String`, `id: Option<Value>`, `method: String`, `params: Option<Value>`

`struct JsonRpcResponse`
> Fields: `jsonrpc: String`, `id: Value`, `result: Option<Value>`, `error: Option<JsonRpcError>`

`struct JsonRpcError`
> Fields: `code: i32`, `message: String`

`fn ok_response(id: Value, result: Value) -> JsonRpcResponse`

`fn err_response(id: Value, code: i32, message: String) -> JsonRpcResponse`

`fn handle_initialize(id: Value) -> JsonRpcResponse`

`fn handle_tools_list(id: Value) -> JsonRpcResponse`

`fn handle_tools_call( id: Value, index: &mut CodebaseIndex, config: &IndexConfig, registry: &ParserRegistry, params: &Value, ) -> JsonRpcResponse`

`enum ServerEvent`
> Variants: `StdinLine`, `StdinClosed`, `FileChanged`

`fn handle_stdin_line( line: &str, index: &mut CodebaseIndex, config: &IndexConfig, registry: &ParserRegistry, writer: &mut impl Write, ) -> anyhow::Result<()>`

`mod coalesce_tests`

---

## src/mcp/tests.rs

**Language:** Rust | **Size:** 45.7 KB | **Lines:** 1313

**Imports:**
- `std::collections::HashMap`
- `std::path::PathBuf`
- `serde_json::{Value, json}`
- `crate::languages::Language`
- `crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility}`
- `crate::model::{CodebaseIndex, FileIndex, Import, IndexStats}`
- `super::helpers::*`
- `super::tools::*`

**Declarations:**

`fn test_score_match_exact_full_match()`

`fn test_score_match_substring()`

`fn test_score_match_no_match()`

`fn test_score_match_multi_term()`

`fn test_score_match_partial_term_match()`

`fn test_score_match_empty_query()`

`fn test_score_match_case_sensitivity()`

`fn test_score_match_camel_case_aware()`

`fn test_split_identifier()`

`fn test_simple_glob_match()`

`fn test_tool_definitions_include_new_tools()`

`fn test_handle_tool_call_unknown_tool()`

`fn test_name_match_scores_higher_than_signature()`

`fn test_collapse_simple_nested()`

`fn test_collapse_string_with_braces()`

`fn test_collapse_escaped_quotes()`

`fn test_collapse_block_comment_with_braces()`

`fn test_collapse_line_comment_with_braces()`

`fn test_collapse_empty_input()`

`fn test_collapse_no_nesting()`

`fn test_collapse_rust_lifetimes()`

`fn test_bigram_identical()`

`fn test_bigram_completely_different()`

`fn test_bigram_partial_overlap()`

`fn test_bigram_short_strings()`

`fn test_bigram_no_duplicate_inflation()`

`fn test_compact_rows_basic()`

`fn test_compact_rows_missing_column()`

`fn test_word_boundary_basic()`

`fn test_word_boundary_at_edges()`

`fn test_word_boundary_not_partial()`

`fn test_word_boundary_with_generics()`

`fn test_word_boundary_empty()`

`fn make_test_index() -> CodebaseIndex`

`fn test_tool_batch_file_summaries_paths()`

`fn test_tool_batch_file_summaries_glob()`

`fn test_tool_batch_file_summaries_no_args()`

`fn test_tool_get_callers()`

`fn test_tool_get_callers_no_false_positive()`

`fn test_tool_get_public_api()`

`fn test_tool_get_public_api_scoped()`

`fn test_tool_explain_symbol()`

`fn test_tool_explain_symbol_case_insensitive()`

`fn test_tool_explain_symbol_not_found()`

`fn test_tool_get_related_tests()`

`fn test_tool_get_related_tests_scoped()`

`fn test_tool_get_related_tests_no_match()`

`fn test_tool_get_token_estimate_directory()`

`fn test_tool_get_token_estimate_glob()`

`fn test_tool_get_token_estimate_no_args()`

`fn test_find_file_exact_match()`

`fn test_find_file_suffix_with_slash_boundary()`

`fn test_find_file_no_partial_suffix()`

`fn test_find_file_not_found()`

`fn test_collapse_raw_string_with_braces()`

`fn test_collapse_raw_string_double_hash()`

`fn test_collapse_raw_string_no_hash()`

`fn test_tool_lookup_symbol_compact()`

`fn test_tool_lookup_symbol_non_compact()`

`fn test_tool_list_declarations_compact()`

`fn test_tool_search_signatures_compact()`

`fn test_tool_search_relevant_compact()`

`fn test_tool_search_relevant_kind_filter()`

`fn test_tool_search_relevant_kind_filter_fn()`

`fn test_tool_read_source_multi_symbol()`

`fn test_tool_read_source_multi_symbol_not_found()`

`fn test_tool_read_source_collapse()`

`fn test_tool_read_source_multi_symbol_collapse()`

`fn test_tool_batch_file_summaries_cap()`

`fn test_tool_get_callers_common_word()`

`fn test_tool_dependency_graph_file_level_mermaid()`

`fn test_tool_dependency_graph_file_level_dot()`

`fn test_tool_dependency_graph_file_level_json()`

`fn test_tool_dependency_graph_symbol_level()`

`fn test_tool_dependency_graph_scoped()`

`fn test_tool_dependency_graph_depth_limit()`

`fn test_tool_dependency_graph_defaults_to_mermaid()`

---

## src/mcp/tools.rs

**Language:** Rust | **Size:** 55.5 KB | **Lines:** 1529

**Imports:**
- `std::collections::HashMap`
- `std::path::{Path, PathBuf}`
- `serde::Serialize`
- `serde_json::{Value, json}`
- `crate::budget::estimate_tokens`
- `crate::dep_graph`
- `crate::diff`
- `crate::indexer::{self, IndexConfig}`
- `crate::languages::Language`
- `crate::model::declarations::{DeclKind, Declaration}`
- *... and 3 more imports*

**Declarations:**

---

## src/model/declarations.rs

**Language:** Rust | **Size:** 5.1 KB | **Lines:** 177

**Imports:**
- `std::fmt`
- `serde::{Deserialize, Serialize}`

**Declarations:**

**`impl Declaration`**
  `pub fn new( kind: DeclKind, name: String, signature: String, visibility: Visibility, line: usize, ) -> Self`


**`impl fmt::Display for Visibility`**
  `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result`


**`impl fmt::Display for DeclKind`**
  `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result`


**`impl DeclKind`**
  `pub fn from_name(name: &str) -> Option<Self>`


---

## src/model/mod.rs

**Language:** Rust | **Size:** 1.1 KB | **Lines:** 56

**Imports:**
- `std::collections::HashMap`
- `std::path::PathBuf`
- `serde::{Deserialize, Serialize}`
- `self::declarations::Declaration`
- `crate::languages::Language`

**Declarations:**

---

## src/output/markdown.rs

**Language:** Rust | **Size:** 11.5 KB | **Lines:** 352

**Imports:**
- `std::collections::HashSet`
- `std::fmt::Write`
- `anyhow::Result`
- `crate::model::CodebaseIndex`
- `crate::model::DetailLevel`
- `crate::model::declarations::{DeclKind, Declaration, Visibility}`
- `super::OutputFormatter`

**Declarations:**

**`impl MarkdownFormatter`**
  `pub fn new() -> Self`

  `pub fn with_options(options: MarkdownOptions) -> Self`


**`impl OutputFormatter for MarkdownFormatter`**
  `fn format(&self, index: &CodebaseIndex, detail: DetailLevel) -> Result<String>`


`fn write_badges(out: &mut String, decl: &Declaration) -> std::fmt::Result`

`fn format_declaration( out: &mut String, decl: &Declaration, depth: usize, detail: DetailLevel, shown_in_api: &HashSet<(String, usize)>, file_path: &str, ) -> std::fmt::Result`

`fn format_size(bytes: u64) -> String`

**`impl std::fmt::Display for crate::model::declarations::RelKind`**
  `fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result`


---

## src/output/mod.rs

**Language:** Rust | **Size:** 233 B | **Lines:** 11

**Imports:**
- `anyhow::Result`
- `crate::model::CodebaseIndex`
- `crate::model::DetailLevel`

**Declarations:**

---

## src/output/yaml.rs

**Language:** Rust | **Size:** 319 B | **Lines:** 14

**Imports:**
- `anyhow::Result`
- `crate::model::CodebaseIndex`
- `crate::model::DetailLevel`
- `super::OutputFormatter`

**Declarations:**

**`impl OutputFormatter for YamlFormatter`**
  `fn format(&self, index: &CodebaseIndex, _detail: DetailLevel) -> Result<String>`


---

## src/parser/mod.rs

**Language:** Rust | **Size:** 2.0 KB | **Lines:** 80

**Imports:**
- `std::path::Path`
- `anyhow::Result`
- `crate::languages::Language`
- `crate::model::FileIndex`

**Declarations:**

**`impl ParserRegistry`**
  `pub fn new() -> Self`

  `pub fn get_parser(&self, language: &Language) -> Option<&dyn LanguageParser>`


---

## src/parser/queries/c.rs

**Language:** Rust | **Size:** 16.1 KB | **Lines:** 477

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for CExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_visibility_from_text(node: Node<'_>, source: &str) -> Visibility`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_signature(node: Node<'_>, source: &str) -> String`

`fn extract_include(node: Node<'_>, source: &str) -> Option<Import>`

`fn extract_function_name<'a>(declarator: Node<'_>, source: &'a str) -> Option<&'a str>`

`fn extract_function_definition(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_declaration(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn has_function_declarator(node: Node<'_>) -> bool`

`fn extract_function_name_from_decl<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str>`

`fn extract_var_name<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str>`

`fn extract_struct(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_struct_field(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enum(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enumerator(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_typedef(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_preproc_def(node: Node<'_>, source: &str) -> Option<Declaration>`

---

## src/parser/queries/cpp.rs

**Language:** Rust | **Size:** 31.8 KB | **Lines:** 893

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for CppExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn extract_top_level( root: Node<'_>, source: &str, imports: &mut Vec<Import>, declarations: &mut Vec<Declaration>, )`

`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_visibility_from_text(node: Node<'_>, source: &str) -> Visibility`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_signature(node: Node<'_>, source: &str) -> String`

`fn extract_include(node: Node<'_>, source: &str) -> Option<Import>`

`fn extract_using(node: Node<'_>, source: &str) -> Option<Import>`

`fn is_deprecated_cpp(node: Node<'_>, source: &str, doc_comment: &Option<String>) -> bool`

`fn extract_function_name<'a>(declarator: Node<'_>, source: &'a str) -> Option<&'a str>`

`fn extract_function_definition(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_declaration(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn has_function_declarator(node: Node<'_>) -> bool`

`fn extract_function_name_from_decl<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str>`

`fn extract_var_name<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str>`

`fn extract_class_inheritance(node: Node<'_>, source: &str) -> Vec<Relationship>`

`fn extract_class( node: Node<'_>, source: &str, default_visibility: Visibility, ) -> Option<Declaration>`

`fn extract_class_member_declaration(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_class_field(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enum(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enumerator(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_namespace(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_template(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_typedef(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_preproc_def(node: Node<'_>, source: &str) -> Option<Declaration>`

---

## src/parser/queries/go.rs

**Language:** Rust | **Size:** 14.7 KB | **Lines:** 462

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for GoExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_visibility(name: &str) -> Visibility`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_signature(node: Node<'_>, source: &str) -> String`

`fn extract_import(node: Node<'_>, source: &str) -> Option<Import>`

`fn is_go_test_name(name: &str) -> bool`

`fn extract_function(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_receiver_type(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_method(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_type_declaration(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_type_spec( node: Node<'_>, source: &str, parent_doc: &Option<String>, ) -> Option<Declaration>`

`fn extract_struct_fields(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_struct_field(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_interface_methods(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_method_spec(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_const_declaration(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_const_spec( node: Node<'_>, source: &str, parent_doc: &Option<String>, ) -> Option<Declaration>`

`fn extract_var_declaration(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_var_spec( node: Node<'_>, source: &str, parent_doc: &Option<String>, ) -> Option<Declaration>`

---

## src/parser/queries/java.rs

**Language:** Rust | **Size:** 18.7 KB | **Lines:** 540

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for JavaExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_modifiers_text(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_visibility(node: Node<'_>, source: &str) -> Visibility`

`fn has_modifier(node: Node<'_>, source: &str, keyword: &str) -> bool`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_signature(node: Node<'_>, source: &str) -> String`

`fn extract_import(node: Node<'_>, source: &str) -> Option<Import>`

`fn has_annotation(node: Node<'_>, source: &str, annotation_name: &str) -> bool`

`fn extract_class_relationships(node: Node<'_>, source: &str) -> Vec<Relationship>`

`fn extract_class(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_interface(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enum(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enum_constant(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_method(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_constructor(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_field(node: Node<'_>, source: &str) -> Option<Declaration>`

---

## src/parser/queries/javascript.rs

**Language:** Rust | **Size:** 13.0 KB | **Lines:** 429

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for JavaScriptExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn process_top_level_node( node: Node<'_>, source: &str, imports: &mut Vec<Import>, declarations: &mut Vec<Declaration>, is_exported: bool, )`

`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn get_raw_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_signature(node: Node<'_>, source: &str) -> String`

`fn extract_import(node: Node<'_>, source: &str) -> Option<Import>`

`fn is_test_name(name: &str) -> bool`

`fn extract_class_relationships(node: Node<'_>, source: &str) -> Vec<Relationship>`

`fn extract_function(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_class(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_method(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_class_field(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_lexical_declaration(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_variable_declaration(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_variable_declarator( node: Node<'_>, parent: Node<'_>, source: &str, ) -> Option<Declaration>`

---

## src/parser/queries/mod.rs

**Language:** Rust | **Size:** 1.0 KB | **Lines:** 31

**Imports:**
- `crate::languages::Language`
- `crate::model::Import`
- `crate::model::declarations::Declaration`

**Declarations:**

---

## src/parser/queries/python.rs

**Language:** Rust | **Size:** 13.5 KB | **Lines:** 395

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for PythonExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_visibility(name: &str) -> Visibility`

`fn extract_docstring(body: Node<'_>, source: &str) -> Option<String>`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_function_signature(node: Node<'_>, source: &str) -> String`

`fn extract_import(node: Node<'_>, source: &str) -> Option<Import>`

`fn body_lines(node: Node<'_>) -> Option<usize>`

`fn detect_is_test_function(name: &str) -> bool`

`fn detect_is_test_class(name: &str) -> bool`

`fn detect_is_async(signature: &str) -> bool`

`fn has_deprecated_decorator(decorators: &[String]) -> bool`

`fn extract_base_classes(node: Node<'_>, source: &str) -> Vec<String>`

`fn extract_function(node: Node<'_>, source: &str, kind: DeclKind) -> Option<Declaration>`

`fn extract_class(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn find_inner_definition(decorated: Node<'_>) -> Option<Node<'_>>`

`fn extract_decorators(decorated: Node<'_>, source: &str) -> Vec<String>`

`fn extract_decorated(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_assignment(node: Node<'_>, source: &str) -> Option<Declaration>`

---

## src/parser/queries/rust.rs

**Language:** Rust | **Size:** 14.4 KB | **Lines:** 422

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for RustExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_visibility(node: Node<'_>, source: &str) -> Visibility`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_signature(node: Node<'_>, source: &str) -> String`

`fn extract_import(node: Node<'_>, source: &str) -> Option<Import>`

`fn body_lines(node: Node<'_>) -> Option<usize>`

`fn has_attribute(node: Node<'_>, source: &str, attr_text: &str) -> bool`

`fn detect_is_test(node: Node<'_>, source: &str) -> bool`

`fn detect_is_async(signature: &str) -> bool`

`fn detect_is_deprecated(node: Node<'_>, source: &str) -> bool`

`fn extract_function(node: Node<'_>, source: &str, kind: DeclKind) -> Option<Declaration>`

`fn extract_struct(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_field(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enum(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_variant(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_trait(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_impl(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_const_or_static(node: Node<'_>, source: &str, kind: DeclKind) -> Option<Declaration>`

`fn extract_type_alias(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_module(node: Node<'_>, source: &str) -> Option<Declaration>`

---

## src/parser/queries/typescript.rs

**Language:** Rust | **Size:** 21.7 KB | **Lines:** 677

**Imports:**
- `tree_sitter::Node`
- `crate::model::Import`
- `crate::model::declarations::{DeclKind, Declaration, RelKind, Relationship, Visibility}`
- `super::DeclExtractor`

**Declarations:**

**`impl DeclExtractor for TypeScriptExtractor`**
  `fn extract(&self, root: Node<'_>, source: &str) -> (Vec<Import>, Vec<Declaration>)`


`fn process_top_level_node( node: Node<'_>, source: &str, imports: &mut Vec<Import>, declarations: &mut Vec<Declaration>, is_exported: bool, )`

`fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str`

`fn extract_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn get_raw_doc_comment(node: Node<'_>, source: &str) -> Option<String>`

`fn extract_signature(node: Node<'_>, source: &str) -> String`

`fn extract_import(node: Node<'_>, source: &str) -> Option<Import>`

`fn is_test_name(name: &str) -> bool`

`fn extract_class_relationships(node: Node<'_>, source: &str) -> Vec<Relationship>`

`fn extract_function(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_class(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_method(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_class_field(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_member_visibility(node: Node<'_>, source: &str) -> Visibility`

`fn extract_interface(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_interface_method(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_property_signature(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_type_alias(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enum(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_enum_member(node: Node<'_>, source: &str) -> Option<Declaration>`

`fn extract_lexical_declaration(node: Node<'_>, source: &str) -> Vec<Declaration>`

`fn extract_variable_declarator( node: Node<'_>, parent: Node<'_>, source: &str, ) -> Option<Declaration>`

---

## src/parser/regex_parser.rs

**Language:** Rust | **Size:** 117.6 KB | **Lines:** 3603

**Imports:**
- `std::path::Path`
- `anyhow::Result`
- `regex::Regex`
- `crate::languages::Language`
- `crate::model::declarations::{DeclKind, Declaration, Visibility}`
- `crate::model::{FileIndex, Import}`
- `super::LanguageParser`

**Declarations:**

**`impl RegexParser`**
  `pub fn new(language: Language) -> Self`


**`impl LanguageParser for RegexParser`**
  `fn language(&self) -> Language`

  `fn parse_file(&self, path: &Path, content: &str) -> Result<FileIndex>`


`fn parse_shell(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_toml(path: &Path, content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_yaml(path: &Path, content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_json(path: &Path, content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_sql(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_markdown(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn get_decl_by_path_mut<'a>( declarations: &'a mut [Declaration], path: &[usize], ) -> &'a mut Declaration`

`fn parse_protobuf(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_graphql(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_ruby(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_kotlin(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_swift(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_csharp(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_objc(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_xml(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_html(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_css(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_gradle(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_cmake(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn parse_properties(content: &str) -> (Vec<Import>, Vec<Declaration>)`

`fn count_braces(line: &str, depth: &mut i32)`

`fn pop_containers(stack: &mut Vec<(i32, usize)>, brace_depth: i32)`

`fn truncate_value(s: &str, max: usize) -> String`

`mod tests`

---

## src/parser/tree_sitter_parser.rs

**Language:** Rust | **Size:** 2.3 KB | **Lines:** 73

**Imports:**
- `std::path::Path`
- `anyhow::Result`
- `crate::languages::Language`
- `crate::model::FileIndex`
- `super::LanguageParser`
- `super::queries`

**Declarations:**

**`impl TreeSitterParser`**
  `pub fn new(language: Language) -> Self`

  `fn get_ts_language(&self, path: &Path) -> tree_sitter::Language`


**`impl LanguageParser for TreeSitterParser`**
  `fn language(&self) -> Language`

  `fn parse_file(&self, path: &Path, content: &str) -> Result<FileIndex>`


---

## src/utils.rs

**Language:** Rust | **Size:** 1.1 KB | **Lines:** 32

**Declarations:**

---

## src/walker/mod.rs

**Language:** Rust | **Size:** 3.3 KB | **Lines:** 125

**Imports:**
- `std::collections::BTreeSet`
- `std::path::{Path, PathBuf}`
- `anyhow::Result`
- `ignore::WalkBuilder`
- `crate::languages::Language`
- `crate::model::TreeEntry`

**Declarations:**

---

## src/watch.rs

**Language:** Rust | **Size:** 10.4 KB | **Lines:** 341

**Imports:**
- `std::fs`
- `std::path::{Path, PathBuf}`
- `std::sync::mpsc`
- `std::time::Duration`
- `anyhow::Result`
- `notify::RecursiveMode`
- `notify_debouncer_mini::new_debouncer`
- `crate::indexer::{self, IndexConfig}`
- `crate::languages::Language`

**Declarations:**

`fn write_index(config: &IndexConfig, output_path: &Path) -> Result<crate::model::CodebaseIndex>`

`fn should_trigger_reindex(path: &Path, root: &Path, output_path: &Path, cache_dir: &Path) -> bool`

`mod tests`

---

## token_count.py

**Language:** Python | **Size:** 2.9 KB | **Lines:** 89

**Imports:**
- `import sys`
- `import os`
- `import argparse`

**Declarations:**

