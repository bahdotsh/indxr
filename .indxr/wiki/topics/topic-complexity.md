---
id: topic-complexity
title: Complexity Analysis & Hotspots
page_type: topic
source_files:
- src/parser/complexity.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:23:37Z
links_to:
- entity-declaration
covers: []
---

# Complexity Analysis & Hotspots

Complexity analysis (`src/parser/complexity.rs`) annotates function-level [[entity-declaration]]s with quantitative metrics and identifies the most complex functions as "hotspots."

## Metrics (Tree-Sitter Only)

Only the 8 tree-sitter-parsed languages get complexity annotations. Three metrics per function:

### Cyclomatic Complexity (`cyclomatic`)
Branch count + 1. Counts: `if`, `else if`, `match`/`case` arms, `while`, `for`, `&&`, `||`, `catch`, ternary. Language-specific branch node kinds are defined in `branch_node_kinds()`.

### Max Nesting Depth (`nesting`)
Deepest nesting level of control flow structures. Counted via `compute_max_nesting()` which walks the AST tracking depth through nesting node kinds (if, for, while, match, try, closures/lambdas).

### Parameter Count (`parameters`)
Number of function parameters. Extracted via `get_params_node()` which finds the parameter list node and counts children, with language-specific handling (e.g., skipping `self`/`cls` in Python, `this` in Java).

## Annotation Pipeline

`annotate_complexity(file_index, source, language)`:
1. Parse source with tree-sitter to get AST
2. `collect_from_ast()` walks the AST to find function nodes
3. For each function, collects `(node, name, line)` tuples
4. `apply_metrics()` computes cyclomatic/nesting/params for each function
5. Matches computed metrics back to [[entity-declaration]]s by line number and name
6. Sets `declaration.complexity = Some(ComplexityMetrics { ... })`

## Hotspot Scoring

`hotspot_score()` computes a composite score:
```
score = (cyclomatic * 1.0) + (nesting * 2.5) + (params * 0.5) + (lines * 0.02)
```

Nesting is weighted highest because deeply nested code is disproportionately hard to understand.

### HotspotEntry
```rust
struct HotspotEntry {
    file_path: String,
    name: String,
    line: usize,
    score: f64,
    cyclomatic: usize,
    nesting: usize,
    parameters: usize,
    lines: usize,
    signature: String,
}
```

`collect_hotspots()` / `collect_hotspots_from_file_refs()` gather all functions with complexity metrics, score them, and return the top N (default 30).

## Health Report

`compute_health_from_file_refs()` aggregates complexity across the codebase into a `HealthReport`:
- Total/mean/max cyclomatic complexity
- Total/mean/max nesting
- Documentation coverage (% of public symbols with doc comments)
- Test ratio (test functions / total functions)
- Hottest files (files with highest cumulative complexity)

## Exposed Via

- CLI: `indxr --hotspots` (top 30 most complex functions)
- MCP: `get_hotspots` tool (filterable by path, min complexity, sort order, compact mode)
- MCP: `get_health` tool (codebase health summary)

