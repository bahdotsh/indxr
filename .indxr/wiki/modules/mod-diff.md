---
id: mod-diff
title: Git Structural Diffing
page_type: module
source_files:
- src/diff.rs
- src/github.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:26:33Z
links_to: []
covers: []
---

# Git Structural Diffing

The diff module (`src/diff.rs`) computes structural changes between git refs — showing which declarations were added, removed, or modified rather than raw line-by-line diffs.

## How It Works

1. **Identify changed files** — `get_changed_files()`, `get_added_files()`, `get_deleted_files()` use `git diff --name-only` with diff filters to categorize file changes since a git ref.

2. **Reconstruct old state** — `get_file_at_ref()` retrieves file content at the old ref using `git show`. These are parsed to produce old `FileIndex` entries.

3. **Compute structural diff** — `compute_structural_diff()` compares current vs old `FileIndex` entries:
   - Files only in current → all declarations marked as added
   - Files only in old → all declarations marked as removed
   - Files in both → `diff_declarations()` compares declaration sets

4. **Declaration diffing** — `diff_declarations()` flattens declarations (including children) into `(kind, name) → signature` maps, then:
   - Present in new but not old → added
   - Present in old but not new → removed
   - Present in both but signature changed → modified (with old and new signatures)

## Data Types

- **`StructuralDiff`** — top-level diff result: lists of added files, removed files, modified files (as `FileDiff`), and total change counts.
- **`FileDiff`** — per-file diff: path, plus lists of added/removed `DeclChange` and modified `DeclModification`.
- **`DeclChange`** — a declaration that was added or removed: kind, name, signature.
- **`DeclModification`** — a declaration that changed: kind, name, old signature, new signature.

## Output Formats

- `format_diff_markdown()` — human-readable Markdown with sections for added/removed/modified
- `format_diff_json()` — structured JSON via serde

## Usage

- **CLI**: `indxr --since main`, `indxr --since HEAD~5`, `indxr --since v1.0.0`
- **Subcommand**: `indxr diff --pr 42` (fetches PR base branch from GitHub via `src/github.rs`)
- **MCP**: `get_diff_summary` tool — returns structural changes in ~200-500 tokens vs thousands for raw `git diff`

## PR-Aware Diffs (`src/github.rs`)

The `github.rs` module provides a GitHub API client for PR-aware diffs:
- Fetches the PR's base branch
- Computes the structural diff against the base
- Works with `indxr diff --pr 42` and the MCP `get_diff_summary` tool

