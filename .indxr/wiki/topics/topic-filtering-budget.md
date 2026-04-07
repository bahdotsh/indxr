---
id: topic-filtering-budget
title: Filtering & Token Budget
page_type: topic
source_files:
- src/filter.rs
- src/budget.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:24:01Z
links_to:
- mod-mcp
covers: []
---

# Filtering & Token Budget

Two complementary systems control what appears in indxr output: **filtering** removes unwanted declarations, and **token budget** progressively truncates to fit a target size.

## Filtering (`src/filter.rs`)

`FilterOptions` controls four independent filters:

```rust
struct FilterOptions {
    kind: Option<DeclKind>,    // Only declarations of this kind
    public_only: bool,         // Only Public visibility
    symbol: Option<String>,    // Name substring match (case-insensitive)
    path: Option<String>,      // Only files under this path prefix
}
```

`apply_filters(index, opts)` runs in order:
1. **Path filter** — retains only files whose path starts with the given prefix
2. **Kind filter** — `filter_declarations_by_kind()` recursively filters declarations and their children
3. **Visibility filter** — `filter_declarations_by_visibility()` keeps only `Visibility::Public` declarations (but preserves public children of private parents)
4. **Symbol filter** — `filter_declarations_by_symbol()` matches declaration names case-insensitively, also matching children
5. **Stats recalculation** — `recalculate_stats()` updates file/line counts after filtering

Filters are applied in `main.rs` after building the index but before output formatting. The [[mod-mcp]] tools apply their own targeted filtering via helpers rather than using `FilterOptions` directly.

## Token Budget (`src/budget.rs`)

`apply_token_budget(index, max_tokens)` progressively strips detail to fit within a token limit. Token estimation uses a simple heuristic: `text.len() / 4` (roughly 4 characters per token).

### Progressive Truncation Strategy

Applied in order until the index fits within budget:

1. **Strip children** — Remove nested declarations (struct fields, impl methods, etc.) from low-importance files first. Files are sorted by `file_importance()` score.
2. **Truncate doc comments** — Shorten doc comments to 100 characters, then 50 characters.
3. **Strip all doc comments** — Remove doc comments entirely.
4. **Remove private declarations** — Keep only `Visibility::Public` declarations.
5. **Drop entire files** — Remove lowest-importance files one by one until budget is met.

### File Importance Scoring

`file_importance()` computes a priority score:
```
score = (public_decl_count * 10) + (total_decl_count * 2) - (file_size / 1000)
```

Files with more public symbols are preserved longer. Large files with few public symbols are dropped first.

### Token Estimation

`estimate_index_tokens()` walks the entire index structure, summing estimated tokens for:
- Directory tree entries
- File headers (path, language, stats)
- Import statements
- Declaration signatures, names, doc comments, and children

## CLI Usage

```bash
indxr --max-tokens 4000                # Progressive truncation to 4000 tokens
indxr --max-tokens 8000 --public-only  # Combine budget with visibility filter
indxr --filter-path src/parser         # Path filtering
indxr --kind function --symbol parse   # Kind + symbol filtering
```

## Design Notes

- Filtering happens before budget application, so filters and budgets compose naturally.
- The progressive truncation preserves the most important information (public API surface) while stripping internal details first.
- Token estimation is approximate (chars/4) but consistent and fast — no external tokenizer dependency.

