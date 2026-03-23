# Output Formats

indxr supports three output formats and three detail levels, giving you control over how the index is structured and how much information it contains.

## Formats

### Markdown (default)

Optimized for AI agent context windows — compact, scannable, minimal token usage.

```bash
indxr -f markdown
# or just
indxr
```

**Structure:**

```markdown
# Codebase Index: project-name

> Generated: 2025-03-23 | Files: 42 | Lines: 8,234
> Languages: Rust (28), Python (10), TypeScript (4)

## Directory Structure
src/
  main.rs
  parser/
    mod.rs
    rust.rs
  model/
    mod.rs

## Public API Surface

**src/main.rs**
- `pub fn main() -> Result<()>`
- `pub struct Config`

**src/parser/mod.rs**
- `pub trait LanguageParser`
- `pub struct ParserRegistry`

---

## src/main.rs

**Language:** Rust | **Size:** 1.2 KB | **Lines:** 45

**Imports:**
- `use anyhow::Result`
- `use clap::Parser`

**Declarations:**

`pub fn main() -> Result<()>`

`pub struct Config`
> Fields: `name: String`, `debug: bool`
```

**Features:**
- Public API Surface section at the top for quick orientation
- Import summarization (large import lists are truncated with counts)
- At `full` detail level: line numbers, doc comments, body line counts, metadata badges (`[test]`, `[async]`, `[deprecated]`), and relationship annotations (`implements Trait`, `extends Class`)

**Markdown-specific options:**
- `--omit-imports` — Remove import listings from output
- `--omit-tree` — Remove directory tree section

### JSON

Full structured output for programmatic consumption.

```bash
indxr -f json -o index.json
```

**Structure:**

```json
{
  "root": "/path/to/project",
  "root_name": "project-name",
  "generated_at": "2025-03-23T10:30:00Z",
  "stats": {
    "files": 42,
    "lines": 8234,
    "languages": { "Rust": 28, "Python": 10, "TypeScript": 4 },
    "duration_ms": 73
  },
  "tree": [
    { "path": "src", "depth": 0, "is_dir": true },
    { "path": "src/main.rs", "depth": 1, "is_dir": false }
  ],
  "files": [
    {
      "path": "src/main.rs",
      "language": "Rust",
      "size": 1234,
      "lines": 45,
      "imports": ["use anyhow::Result", "use clap::Parser"],
      "declarations": [
        {
          "kind": "Function",
          "name": "main",
          "signature": "pub fn main() -> Result<()>",
          "visibility": "Public",
          "line": 10,
          "doc_comment": "Application entry point",
          "is_test": false,
          "is_async": true,
          "is_deprecated": false,
          "body_lines": 35,
          "relationships": [],
          "children": []
        }
      ]
    }
  ]
}
```

**Best for:** tool integrations, custom pipelines, programmatic analysis.

### YAML

Same structure as JSON in YAML format.

```bash
indxr -f yaml -o index.yaml
```

```yaml
root: /path/to/project
root_name: project-name
generated_at: "2025-03-23T10:30:00Z"
files:
  - path: src/main.rs
    language: Rust
    lines: 45
    declarations:
      - kind: Function
        name: main
        signature: "pub fn main() -> Result<()>"
        visibility: Public
        line: 10
```

**Best for:** human-readable structured data, config-adjacent workflows.

## Detail Levels

Control how much information is included with `-d` / `--detail`:

### `summary`

Directory tree and file list only. No declarations.

```bash
indxr -d summary
```

```markdown
# Codebase Index: my-project

> Generated: 2025-03-23 | Files: 42 | Lines: 8,234

## Directory Structure
src/
  main.rs
  parser/
    mod.rs
```

**Use case:** High-level orientation, understanding project structure.

### `signatures` (default)

Everything in summary, plus all declarations with signatures and imports.

```bash
indxr -d signatures
# or just
indxr
```

**Use case:** Standard agent context — enough to understand the full API surface and navigate the codebase.

### `full`

Everything in signatures, plus doc comments, line numbers, body line counts, metadata badges (`[test]`, `[async]`, `[deprecated]`), and relationship annotations.

```bash
indxr -d full
```

```markdown
`pub fn process(input: &str) -> Result<Output>` [async] [deprecated]
> Processes raw input into structured output
> Line 42 (128 lines)
> implements `Processor`
```

**Use case:** Detailed analysis, understanding code relationships, identifying test coverage.

## Choosing the Right Combination

| Task | Format | Detail | Extra flags |
|------|--------|--------|-------------|
| Quick orientation | markdown | summary | — |
| Agent context (standard) | markdown | signatures | `--max-tokens 8000` |
| Public API reference | markdown | signatures | `--public-only` |
| Full analysis | markdown | full | — |
| CI pipeline | json | signatures | `-o index.json` |
| Human review | yaml | full | — |
| Compact for small context | markdown | signatures | `--public-only --omit-imports --max-tokens 4000` |
