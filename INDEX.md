# Codebase Index: indxr

> Generated: 2026-03-25 09:47:32 UTC | Files: 44 | Lines: 17642
> Languages: Markdown (12), Python (1), Rust (29), Shell (1), TOML (1)

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
    filtering.md
    git-diffing.md
    languages.md
    mcp-server.md
    output-formats.md
    token-budget.md
  src/
    budget.rs
    cache/
      fingerprint.rs
      mod.rs
    cli.rs
    diff.rs
    error.rs
    filter.rs
    indexer.rs
    languages.rs
    main.rs
    mcp.rs
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
    walker/
      mod.rs
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
- `# Other`

**Cargo.toml**
- `[package]`
- `[dependencies]`

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
- `# Compact public API index for an agent`
- `# Quick structural diff of backend changes`
- `# Full JSON index without cache`

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
- `pub enum Command`
- `pub enum OutputFormat`

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

**src/languages.rs**
- `pub enum Language`

**src/mcp.rs**
- `pub fn run_mcp_server(mut index: CodebaseIndex, config: IndexConfig) -> anyhow::Result<()>`

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

**src/walker/mod.rs**
- `pub struct WalkResult`
- `pub struct FileEntry`
- `pub fn walk_directory( root: &Path, respect_gitignore: bool, max_file_size: u64, max_depth: Option<usize>, exclude_patterns: &[String], ) -> Result<WalkResult>`

**token_count.py**
- `def count_openai(text: str) -> int | None`
- `def count_claude(text: str) -> int | None`
- `def main()`

---

## CLAUDE.md

**Language:** Markdown | **Size:** 8.2 KB | **Lines:** 141

**Declarations:**

---

## Cargo.toml

**Language:** TOML | **Size:** 905 B | **Lines:** 35

**Imports:**
- `anyhow`
- `bincode`
- `chrono`
- `clap`
- `ignore`
- `rayon`
- `regex`
- `serde`
- `serde_json`
- `serde_yaml`
- *... and 11 more imports*

**Declarations:**

---

## INDEX.md

**Language:** Markdown | **Size:** 39.5 KB | **Lines:** 1561

**Declarations:**

---

## README.md

**Language:** Markdown | **Size:** 7.2 KB | **Lines:** 221

**Declarations:**

---

## benchmark.sh

**Language:** Shell | **Size:** 24.9 KB | **Lines:** 620

**Declarations:**

---

## docs/agent-integration.md

**Language:** Markdown | **Size:** 13.1 KB | **Lines:** 417

**Declarations:**

---

## docs/caching.md

**Language:** Markdown | **Size:** 2.5 KB | **Lines:** 87

**Declarations:**

---

## docs/cli-reference.md

**Language:** Markdown | **Size:** 4.4 KB | **Lines:** 195

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

**Language:** Markdown | **Size:** 5.0 KB | **Lines:** 222

**Declarations:**

---

## docs/mcp-server.md

**Language:** Markdown | **Size:** 9.7 KB | **Lines:** 405

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

**Language:** Rust | **Size:** 3.4 KB | **Lines:** 137

**Imports:**
- `std::path::PathBuf`
- `clap::{Parser, Subcommand}`
- `crate::model::DetailLevel`

**Declarations:**

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

**Language:** Rust | **Size:** 7.6 KB | **Lines:** 274

**Imports:**
- `std::collections::HashMap`
- `std::fs`
- `std::time::Instant`
- `anyhow::Result`
- `clap::Parser`
- `crate::cache::Cache`
- `crate::cli::{Cli, Command, OutputFormat}`
- `crate::filter::FilterOptions`
- `crate::languages::Language`
- `crate::model::declarations::DeclKind`
- *... and 5 more imports*

**Declarations:**

`mod budget`

`mod cache`

`mod cli`

`mod diff`

`mod error`

`mod filter`

`mod indexer`

`mod languages`

`mod mcp`

`mod model`

`mod output`

`mod parser`

`mod walker`

`fn main() -> Result<()>`

`fn handle_git_diff( root: &std::path::Path, since_ref: &str, current_files: &[model::FileIndex], registry: &ParserRegistry, cli: &Cli, ) -> Result<()>`

---

## src/mcp.rs

**Language:** Rust | **Size:** 82.6 KB | **Lines:** 2387

**Imports:**
- `std::collections::HashMap`
- `std::io::{self, BufRead, Write}`
- `std::path::{Path, PathBuf}`
- `serde::{Deserialize, Serialize}`
- `serde_json::{self, Value, json}`
- `crate::budget::estimate_tokens`
- `crate::diff`
- `crate::indexer::{self, IndexConfig}`
- `crate::languages::Language`
- `crate::model::declarations::{DeclKind, Declaration, Visibility}`
- *... and 2 more imports*

**Declarations:**

`struct JsonRpcRequest`
> Fields: `jsonrpc: String`, `id: Option<Value>`, `method: String`, `params: Option<Value>`

`struct JsonRpcResponse`
> Fields: `jsonrpc: String`, `id: Value`, `result: Option<Value>`, `error: Option<JsonRpcError>`

`struct JsonRpcError`
> Fields: `code: i32`, `message: String`

`fn ok_response(id: Value, result: Value) -> JsonRpcResponse`

`fn err_response(id: Value, code: i32, message: String) -> JsonRpcResponse`

`fn tool_result(content: Value) -> Value`

`fn tool_error(msg: &str) -> Value`

`struct SymbolMatch`
> Fields: `file: String`, `kind: String`, `name: String`, `signature: String`, `line: usize`, `doc_comment: Option<String>`

`fn find_symbols_in_decl( decl: &Declaration, query: &str, file_path: &str, results: &mut Vec<SymbolMatch>, limit: usize, )`

`struct SignatureMatch`
> Fields: `file: String`, `kind: String`, `name: String`, `signature: String`, `line: usize`

`fn find_signatures_in_decl( decl: &Declaration, query: &str, file_path: &str, results: &mut Vec<SignatureMatch>, limit: usize, )`

`fn filter_declarations<'a>(decls: &'a [Declaration], kind: &DeclKind) -> Vec<&'a Declaration>`

`struct ShallowDeclaration`
> Fields: `kind: String`, `name: String`, `signature: String`, `line: usize`, `children_count: Option<usize>`

`fn to_shallow(decl: &Declaration) -> ShallowDeclaration`

`fn file_summary_data(file: &FileIndex) -> Value`

`fn find_decl_by_name<'a>(decls: &'a [Declaration], name: &str) -> Option<&'a Declaration>`

`fn read_line_range(path: &Path, start: usize, end: usize) -> Result<String, String>`

`fn tool_definitions() -> Value`

`fn handle_tool_call(index: &CodebaseIndex, name: &str, args: &Value) -> Value`

`fn tool_regenerate_index(index: &mut CodebaseIndex, config: &IndexConfig) -> Value`

`fn tool_lookup_symbol(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_list_declarations(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_search_signatures(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_get_tree(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_get_imports(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_get_stats(index: &CodebaseIndex) -> Value`

`fn tool_get_file_summary(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_read_source(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_get_file_context(index: &CodebaseIndex, args: &Value) -> Value`

`const APPROX_SUMMARY_TOKENS: usize = 300`

`fn tool_get_token_estimate(index: &CodebaseIndex, args: &Value) -> Value`

`struct RelevanceMatch`
> Fields: `file: String`, `symbol: Option<String>`, `kind: Option<String>`, `signature: Option<String>`, `line: Option<usize>`, `match_on: String`, `score: u32`

`fn tool_search_relevant(index: &CodebaseIndex, args: &Value) -> Value`

`fn score_match(text: &str, query: &str, terms: &[&str]) -> u32`

`fn score_decls_recursive( decls: &[Declaration], file_path: &str, query: &str, terms: &[&str], results: &mut Vec<RelevanceMatch>, kind_filter: Option<&DeclKind>, )`

`fn simple_glob_match(pattern: &str, path: &str) -> bool`

`fn split_identifier(name: &str) -> Vec<String>`

`fn bigram_similarity(a: &str, b: &str) -> f64`

`fn collapse_nested_bodies(source: &str) -> String`

`fn to_compact_rows(columns: &[&str], items: &[Value]) -> Value`

`fn collect_public_decls(decls: &[Declaration], file_path: &str, out: &mut Vec<Value>)`

`fn find_tests_for_symbol( decls: &[Declaration], symbol_lower: &str, file_path: &str, results: &mut Vec<Value>, reason: &str, )`

`fn explain_decl(decl: &Declaration, file_path: &str) -> Value`

`fn tool_get_diff_summary(index: &CodebaseIndex, config: &IndexConfig, args: &Value) -> Value`

`fn tool_batch_file_summaries(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_get_callers(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_get_public_api(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_explain_symbol(index: &CodebaseIndex, args: &Value) -> Value`

`fn tool_get_related_tests(index: &CodebaseIndex, args: &Value) -> Value`

`fn find_file<'a>(index: &'a CodebaseIndex, path: &str) -> Option<&'a FileIndex>`

`fn handle_initialize(id: Value) -> JsonRpcResponse`

`fn handle_tools_list(id: Value) -> JsonRpcResponse`

`fn handle_tools_call( id: Value, index: &mut CodebaseIndex, config: &IndexConfig, params: &Value, ) -> JsonRpcResponse`

`mod tests`

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

## token_count.py

**Language:** Python | **Size:** 2.9 KB | **Lines:** 89

**Imports:**
- `import sys`
- `import os`
- `import argparse`

**Declarations:**

