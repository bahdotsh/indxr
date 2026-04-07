---
id: mod-output
title: Output Formatting
page_type: module
source_files:
- src/output/mod.rs
- src/output/markdown.rs
- src/output/yaml.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:28:15Z
links_to: []
covers: []
---

# Output Formatting

The output module (`src/output/`) renders `CodebaseIndex` data into human-readable and machine-readable formats.

## Formats

### Markdown (`src/output/markdown.rs`)
The default output format, optimized for LLM consumption. `MarkdownFormatter` implements the `OutputFormatter` trait.

Sections produced:
- Directory tree (indented, with file/directory icons)
- Per-file sections with declarations listed as bullet points
- Declaration details vary by `DetailLevel`:
  - **Summary**: directory tree and file list only
  - **Signatures** (default): declaration names and signatures
  - **Full**: adds doc comments, line numbers, body line counts

`MarkdownOptions` controls omissions (`omit_imports`, `omit_tree`).

### YAML (`src/output/yaml.rs`)
Structured YAML output via `YamlFormatter`. Useful for programmatic consumption where JSON feels too verbose.

### JSON
JSON output is handled inline in `src/main.rs` via `serde_json::to_string_pretty()` on the `CodebaseIndex`. No separate formatter needed since the model types derive `Serialize`.

## OutputFormatter Trait (`src/output/mod.rs`)

```
pub trait OutputFormatter {
    fn format(&self, index: &CodebaseIndex) -> Result<String>;
}
```

Implementations: `MarkdownFormatter`, `YamlFormatter`.

