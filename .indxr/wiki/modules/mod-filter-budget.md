---
id: mod-filter-budget
title: Filtering & Token Budget
page_type: module
source_files:
- src/filter.rs
- src/budget.rs
generated_at_ref: ''
generated_at: 2026-04-07T13:27:20Z
links_to: []
covers: []
---

# Filtering & Token Budget

Two modules work together to narrow and size-constrain the index output: filtering selects *what* to include, and the token budget controls *how much*.

## Filtering (`src/filter.rs`)

`FilterOptions` controls which declarations appear in the output:
- `filter_path` — only include files under a path prefix (e.g., `src/parser`)
- `kind` — only include declarations of a specific `DeclKind` (e.g., function, struct)
- `public_only` — only include public declarations
- `symbol` — search for declarations by name (case-insensitive substring match)

`apply_filters(index, opts)` applies all active filters:
1. Path filtering — removes files outside the filter path
2. Kind filtering — `filter_declarations_by_kind()` keeps only matching kinds
3. Visibility filtering — `filter_declarations_by_visibility()` keeps only public declarations
4. Symbol filtering — `filter_declarations_by_symbol()` keeps only matching names (also searches children)
5. Stat recalculation — `recalculate_stats()` updates file/line counts after filtering

Filters are applied recursively to children — if a struct is kept but its private fields are filtered, those fields are removed.

## Token Budget (`src/budget.rs`)

The budget system ensures output fits within a specified token limit (`--max-tokens N`). It uses progressive truncation — stripping the least important content first.

### Token Estimation
`estimate_tokens(text)` uses a simple heuristic: `text.len() / 4` (roughly 4 chars per token). This is fast and reasonably accurate for structural index content.

`estimate_index_tokens()` estimates the token count for an entire `CodebaseIndex` without rendering it.

### Progressive Truncation Strategy

`apply_token_budget(index, max_tokens)` applies increasingly aggressive truncation until the estimate fits:

1. **Rank files by importance** — `file_importance()` scores files based on:
   - Public declaration count (weighted heavily)
   - Total declaration count
   - Penalty for test files
   - Bonus for entry points (main, lib, mod, index)

2. **Truncation phases** (applied to lowest-importance files first):
   - Truncate long doc comments to 100 chars
   - Strip doc comments entirely
   - Remove private declarations
   - Strip children from remaining declarations
   - Remove entire files (lowest importance first)

3. **Re-estimate after each phase** — stops as soon as the estimate is within budget.

This ensures the most important public API surfaces survive even aggressive truncation, while less important implementation details are progressively removed.

