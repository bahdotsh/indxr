# Git-Aware Structural Diffing

indxr can show what changed structurally since any git ref or GitHub PR — not line-level diffs, but declaration-level changes: which functions, structs, classes, and methods were added, removed, or had their signatures modified.

## Usage

### By git ref

```bash
indxr --since <REF>
```

Where `<REF>` is any valid git reference:

```bash
# Since a branch
indxr --since main
indxr --since develop

# Since a tag
indxr --since v1.0.0
indxr --since release-2025.03

# Since a relative commit
indxr --since HEAD~3
indxr --since HEAD~10

# Since a specific commit
indxr --since abc1234
```

### By GitHub PR

The `diff` subcommand can resolve a PR's base branch automatically via the GitHub API:

```bash
# Structural diff for PR #42
indxr diff --pr 42

# JSON output
indxr diff --pr 42 -f json

# The diff subcommand also supports --since
indxr diff --since main
```

**Authentication:** Requires a GitHub token via `GITHUB_TOKEN` env var, `GH_TOKEN` env var, or `gh auth token` (GitHub CLI). The PR's base branch must be available locally — run `git fetch origin <base>` if needed.

### MCP tool

The `get_diff_summary` MCP tool also supports PR-aware diffs:

```json
{
  "params": {
    "name": "get_diff_summary",
    "arguments": { "pr": 42 }
  }
}
```

Provide either `since_ref` or `pr`, not both. When using `pr`, the response includes PR metadata (number, title, base/head branches).

## Output Format

### Markdown

```markdown
# Structural Changes (since main)

## Added Files
- src/new_module.rs

## Removed Files
- src/old_module.rs

## Modified Files

### src/parser/mod.rs
+ `pub fn new_parser() -> Parser`
- `fn old_helper()`
~ `fn process(x: i32)` -> `fn process(x: i32, y: i32)`

### src/model/declarations.rs
+ `pub struct Metadata`
+ `pub fn Metadata::is_empty(&self) -> bool`
~ `pub enum DeclKind` (variants changed)
```

**Markers:**
- `+` — Declaration added
- `-` — Declaration removed
- `~` — Declaration modified (signature changed)

### JSON

```bash
indxr --since main -f json
```

```json
{
  "since_ref": "main",
  "files_added": ["src/new_module.rs"],
  "files_removed": ["src/old_module.rs"],
  "files_modified": [
    {
      "path": "src/parser/mod.rs",
      "declarations_added": [
        { "kind": "Function", "name": "new_parser", "signature": "pub fn new_parser() -> Parser" }
      ],
      "declarations_removed": [
        { "kind": "Function", "name": "old_helper", "signature": "fn old_helper()" }
      ],
      "declarations_modified": [
        {
          "kind": "Function",
          "name": "process",
          "old_signature": "fn process(x: i32)",
          "new_signature": "fn process(x: i32, y: i32)"
        }
      ]
    }
  ]
}
```

## How It Works

1. **Identify changed files** using `git diff --name-only <ref>...HEAD`
2. **Retrieve old file content** using `git show <ref>:<path>` for each changed file
3. **Parse both versions** — current files from disk, old files from git
4. **Compare declarations** — match by name and kind, detect additions, removals, and signature changes
5. **Output the structural diff** in the selected format

## Combining with Other Options

Filters and output options work with `--since`:

```bash
# Only show structural changes in Rust files
indxr --since main -l rust

# Only changes in a specific directory
indxr --since main --filter-path src/parser

# JSON output for programmatic use
indxr --since v1.0.0 -f json -o changes.json

# Only public API changes
indxr --since main --public-only
```

## Use Cases

### Code Review Preparation

Before reviewing a branch, see what changed structurally:

```bash
indxr --since main --public-only
```

Or review a specific PR:

```bash
indxr diff --pr 42
```

This shows API surface changes without the noise of implementation details.

### Release Notes

Compare against the last release tag to see all structural changes:

```bash
indxr --since v1.2.0
```

### Agent Context for Recent Changes

Give an AI agent context about what changed recently:

```bash
indxr --since HEAD~5 --max-tokens 4000
```

### Monitoring API Stability

Track public API changes between versions:

```bash
indxr --since v1.0.0 --public-only -f json -o api-changes.json
```
