---
id: languages
title: Language Support
page_type: topic
source_files:
- src/languages.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:27:50Z
links_to: []
covers: []
---

# Language Support

indxr supports 27 programming languages, detected by file extension and parsed using one of two strategies.

## Language Detection (`src/languages.rs`)

The `Language` enum has 27 variants. `Language::from_path(path)` maps file extensions to languages:

| Language | Extensions |
|----------|-----------|
| Rust | `.rs` |
| Python | `.py`, `.pyi` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.tsx` |
| Go | `.go` |
| Java | `.java` |
| C | `.c`, `.h` |
| Cpp | `.cpp`, `.hpp`, `.cc`, `.cxx`, `.hxx`, `.hh` |
| Ruby | `.rb` |
| Kotlin | `.kt`, `.kts` |
| Swift | `.swift` |
| CSharp | `.cs` |
| ObjectiveC | `.m`, `.mm` |
| Shell | `.sh`, `.bash`, `.zsh` |
| Sql | `.sql` |
| Protobuf | `.proto` |
| GraphQL | `.graphql`, `.gql` |
| Toml | `.toml` |
| Yaml | `.yml`, `.yaml` |
| Json | `.json` |
| Xml | `.xml` |
| Html | `.html`, `.htm` |
| Css | `.css` |
| Markdown | `.md`, `.mdx` |
| Gradle | `.gradle`, `.gradle.kts` |
| CMake | `CMakeLists.txt`, `.cmake` |
| Properties | `.properties`, `.env` |

## Parsing Strategy

`Language::uses_tree_sitter()` determines the parsing strategy:

### Tree-Sitter (8 languages) — Full AST parsing
Rust, Python, JavaScript, TypeScript, Go, Java, C, C++

These languages get:
- Accurate declaration extraction via AST traversal
- Complexity metrics (nesting depth, branches, line count)
- Reliable relationship detection (extends, implements)
- Precise visibility detection

### Regex (19 languages) — Pattern matching
All remaining languages. Each has a dedicated `parse_*` function in `src/parser/regex_parser.rs`.

Regex parsing provides:
- Declaration extraction (functions, classes, structs, etc.)
- Import detection
- Basic visibility inference
- No complexity metrics

## Language Classification

`Language::is_config()` identifies configuration/data languages (TOML, YAML, JSON, XML, Properties, Gradle, CMake) which may receive different treatment in some contexts (e.g., file importance scoring).

`Display` trait implementation provides human-readable names (e.g., `Language::Cpp` → `"C++"`).

