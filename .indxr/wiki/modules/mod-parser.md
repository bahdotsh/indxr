---
id: mod-parser
title: Parser System
page_type: module
source_files:
- src/parser/mod.rs
- src/parser/tree_sitter_parser.rs
- src/parser/regex_parser.rs
- src/parser/complexity.rs
- src/parser/queries/mod.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:24:41Z
links_to: []
covers: []
---

# Parser System

The parser system extracts structural information (declarations, imports, relationships) from source files. It uses a dual-strategy approach: tree-sitter for accurate AST parsing of major languages, and regex for broader coverage.

## Architecture

### LanguageParser Trait (`src/parser/mod.rs`)

All parsers implement the `LanguageParser` trait:
- `parse(&self, path: &Path, content: &str, language: Language) -> Result<FileIndex>` — parse a file into a `FileIndex`

The trait is `Send + Sync` to support parallel parsing via rayon.

### ParserRegistry (`src/parser/mod.rs`)

The `ParserRegistry` holds both parser implementations and dispatches based on language classification:
- `new()` — creates both a `TreeSitterParser` and `RegexParser`
- `parse(path, content, language)` — routes to the correct parser based on `language.uses_tree_sitter()`

### Tree-Sitter Parser (`src/parser/tree_sitter_parser.rs`)

Parses 8 languages with full AST accuracy: **Rust, Python, JavaScript, TypeScript, Go, Java, C, C++**.

Each language has a dedicated `DeclExtractor` trait implementation in `src/parser/queries/`:
- `rust.rs` — Rust extractor (functions, structs, enums, traits, impls, modules, type aliases)
- `python.rs` — Python extractor (functions, classes, assignments, decorators)
- `javascript.rs` — JavaScript extractor (functions, classes, variables, imports)
- `typescript.rs` — TypeScript extractor (interfaces, type aliases, enums, plus JS features)
- `go.rs` — Go extractor (functions, structs, interfaces, type aliases)
- `java.rs` — Java extractor (classes, interfaces, enums, methods, fields)
- `c.rs` — C extractor (functions, structs, enums, typedefs, macros)
- `cpp.rs` — C++ extractor (classes, namespaces, templates, plus C features)

The `DeclExtractor` trait pattern decouples language-specific extraction logic from the generic tree-walking machinery in `TreeSitterParser`.

### Regex Parser (`src/parser/regex_parser.rs`)

Parses 19 languages using pattern matching: **Shell, TOML, YAML, JSON, SQL, Markdown, Protobuf, GraphQL, Ruby, Kotlin, Swift, C#, Objective-C, XML, HTML, CSS, Gradle, CMake, Properties**.

Each language has a dedicated `parse_*` method (e.g., `parse_shell`, `parse_toml`). The regex approach trades accuracy for breadth — it handles most common declaration patterns without needing grammar files.

At ~3600 lines, this is the largest source file in the codebase.

## Complexity Analysis (`src/parser/complexity.rs`)

For tree-sitter languages, per-function complexity metrics are computed:
- **Nesting depth** — maximum nesting level of control flow
- **Branch count** — number of branches (if/else/match arms)
- **Line count** — function body length

These metrics feed:
- `collect_hotspots()` — ranks functions by composite complexity score
- `compute_health_from_file_refs()` — aggregates health metrics across files
- The `get_hotspots` and `get_health` MCP tools

## What Gets Extracted

Each parser produces a `FileIndex` containing:
- **Declarations** — functions, structs/classes, enums, interfaces, traits, impls, constants, type aliases, modules, etc. Each with: name, kind, visibility, signature, doc comment, line number, children, relationships, and complexity metrics.
- **Imports** — import/use/require statements with the imported path.
- **Metadata** — language, line count, file path.

## Parallel Execution

Parsing is parallelized via rayon in `src/indexer.rs`:
- `parse_files()` uses `par_iter()` to parse files concurrently
- Cache hits skip parsing entirely
- Results are collected and merged in `collect_results()`

