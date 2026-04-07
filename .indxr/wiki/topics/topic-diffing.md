---
id: topic-diffing
title: Git Structural Diffing
page_type: topic
source_files:
- src/diff.rs
- src/github.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:24:19Z
links_to: []
covers: []
---

# Git Structural Diffing

Structural diffing (`src/diff.rs`) compares the current codebase against a git ref to show **what declarations changed**, not raw text diffs. This is far more useful for agents than line-level diffs.

## Core Types

```rust
struct StructuralDiff {
    since_ref: String,           // The git ref compared against
    added_files: Vec<PathBuf>,   // New files
    deleted_files: Vec<PathBuf>, // Removed files
    changed_files: Vec<FileDiff>, // Files with structural changes
}

struct FileDiff {
    path: PathBuf,
    added: Vec<DeclChange>,      // New declarations
    removed: Vec<DeclChange>,    // Deleted declarations
    modified: Vec<DeclModification>, // Changed signatures
}

struct DeclChange {
    name: String,
    kind: DeclKind,
    signature: Option<String>,
}

struct DeclModification {
    name: String,
    kind: DeclKind,
    old_signature: Option<String>,
    new_signature: Option<String>,
}
```

## How It Works

`compute_structural_diff(current_files, old_files, changed_paths)`:

1. **Get changed file list** — `git_diff_names()` runs `git diff --name-only` against the ref
2. **Get old file content** — `get_file_at_ref()` runs `git show {ref}:{path}` to retrieve file content at the old ref
3. **Parse old files** — Re-parses old content with the same parser to get old declarations
4. **Compare declarations** — `diff_declarations()` flattens both old and new declaration trees via `flatten_declarations()` (keyed by `(DeclKind, name) → signature`), then computes added/removed/modified sets
5. **Format output** — `format_diff_markdown()` or `format_diff_json()`

## PR-Aware Diffs

The `src/github.rs` module adds GitHub PR awareness:

- `resolve_pr_base(root, pr_number)` — Fetches PR info via GitHub API to find the base branch
- `detect_github_repo(root)` — Parses the GitHub remote URL from git config
- `fetch_pr_info(owner, repo, pr_number)` — Calls `GET /repos/{owner}/{repo}/pulls/{pr}` using `GITHUB_TOKEN` or `GH_TOKEN`

This enables `indxr diff --pr 42` to diff against the PR's base branch automatically.

## Exposed Via

- CLI: `indxr --since main`, `indxr diff --pr 42`, `indxr diff --since HEAD~5`
- MCP: `get_diff_summary` tool (accepts `since` ref or `pr` number, outputs structural changes)

## Design Notes

- Structural diffs are ~200-500 tokens vs thousands for raw `git diff` output — a major token savings for agents.
- Declaration comparison is by `(kind, name)` tuple — renames appear as a remove + add pair.
- The diff only shows **structural** changes (declaration additions, removals, signature modifications). Body-only changes (implementation details without signature changes) are intentionally excluded.

