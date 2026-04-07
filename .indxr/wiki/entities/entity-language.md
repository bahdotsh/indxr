---
id: entity-language
title: Language Enum
page_type: entity
source_files:
- src/languages.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:22:12Z
links_to:
- mod-walker
- mod-parser
- mod-cache
- mod-output
- mod-mcp
covers: []
---

# Language Enum

The `Language` enum (`src/languages.rs`) defines all 27 supported languages and drives parser routing, file detection, and display.

## Variants

**Tree-sitter parsed (8):** Rust, Python, JavaScript, TypeScript, Go, Java, C, Cpp

**Regex parsed (19):** Ruby, Kotlin, Swift, CSharp, ObjectiveC, Shell, Sql, Protobuf, GraphQL, Toml, Yaml, Json, Xml, Html, Css, Markdown, Gradle, CMake, Properties

## Key Methods

- `Language::from_extension(path: &Path) -> Option<Language>` — Detects language from file extension. Maps extensions like `.rs` → Rust, `.py`/`.pyi` → Python, `.js`/`.mjs`/`.cjs` → JavaScript, `.ts`/`.mts` → TypeScript, etc.
- `Language::uses_tree_sitter() -> bool` — Returns true for the 8 tree-sitter languages. Used by `ParserRegistry` to route to the correct parser.
- `Language::from_name(name: &str) -> Option<Language>` — Case-insensitive name lookup (e.g., `"rust"` → `Rust`, `"c++"` → `Cpp`).
- `Display` impl — Formats as lowercase name for output (e.g., `"rust"`, `"javascript"`, `"c++"`).

## Extension Mapping Highlights

Some languages have multiple extensions:
- TypeScript: `.ts`, `.tsx`, `.mts`
- JavaScript: `.js`, `.jsx`, `.mjs`, `.cjs`
- C++: `.cpp`, `.cxx`, `.cc`, `.hpp`, `.hxx`, `.hh`
- YAML: `.yml`, `.yaml`
- Shell: `.sh`, `.bash`, `.zsh`
- Kotlin: `.kt`, `.kts`
- Properties: `.properties`, `.env`

## Usage

- [[mod-walker]]: Files without a recognized extension are skipped during directory traversal
- [[mod-parser]]: `ParserRegistry` uses `Language` to select tree-sitter vs regex parser
- [[mod-cache]]: `CacheEntry` stores the detected language
- [[mod-output]]: Language name appears in file headers
- [[mod-mcp]]: `get_stats` aggregates file counts per language
- CLI: `-l rust,python` flag filters by language name

