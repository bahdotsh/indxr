---
id: entity-declaration
title: Declaration — Core Data Type
page_type: entity
source_files:
- src/model/declarations.rs
generated_at_ref: ''
generated_at: 2026-04-06T04:22:10Z
links_to:
- topic-dep-graph
- topic-filtering-budget
- mod-mcp
- topic-complexity
- mod-parser
- mod-output
- topic-diffing
covers: []
---

# Declaration — Core Data Type

`Declaration` is the fundamental unit of structural information in indxr. Every parsed symbol — function, struct, enum, trait, class, method, field, constant, import — becomes a `Declaration`. It is the atom that flows through the entire pipeline: parser → model → filter → budget → output → MCP tools.

## Structure

Defined in `src/model/declarations.rs`:

```
Declaration {
    name: String,              // Symbol name
    kind: DeclKind,            // Function, Struct, Enum, Trait, etc. (25 variants)
    signature: Option<String>, // Full signature text (e.g., "pub fn parse(path: &Path) -> Result<AST>")
    visibility: Visibility,    // Public, Private, or Crate
    line: Option<usize>,       // Source line number (1-based)
    doc_comment: Option<String>, // Extracted doc comment
    children: Vec<Declaration>, // Nested declarations (methods in impl, fields in struct, etc.)
    body_line_count: Option<usize>, // Function body size
    is_test: bool,             // Whether this is a test function
    is_async: bool,            // Whether this is async
    is_deprecated: bool,       // Whether marked deprecated
    relationships: Vec<Relationship>, // Implements/Extends relationships
    complexity: Option<ComplexityMetrics>, // Cyclomatic complexity, nesting depth, param count
}
```

## DeclKind (25 variants)

Function, Method, Struct, Enum, Trait, Impl, Interface, Class, Module, Constant, Variable, TypeAlias, Field, Variant, Property, Constructor, Getter, Setter, Static, Namespace, Decorator, Protocol, Extension, Macro, Unknown.

The `DeclKind::is_type_like()` method returns true for Struct, Enum, Trait, Interface, Class, TypeAlias, Protocol — used by [[topic-dep-graph]] and type flow analysis.

## Visibility

Three variants: `Public`, `Private`, `Crate`. Display formats as `pub`, `priv`, `pub(crate)`. Used by [[topic-filtering-budget]] to filter public-only APIs and by budget truncation to prioritize public symbols.

## Relationships

```
Relationship { kind: RelKind, target: String }
RelKind: Implements | Extends
```

Used by [[topic-dep-graph]] symbol-level graphs and [[mod-mcp]] `explain_symbol` to show trait implementations and class inheritance.

## ComplexityMetrics

```
ComplexityMetrics {
    cyclomatic: usize,    // Cyclomatic complexity (branch count + 1)
    nesting: usize,       // Maximum nesting depth
    parameters: usize,    // Parameter count
}
```

Only populated for tree-sitter parsed languages. See [[topic-complexity]].

## Tree Structure (children)

Declarations nest via `children`:
- `Struct` → `Field` children
- `Enum` → `Variant` children
- `Impl` → `Method`/`Function` children
- `Class` → `Method`/`Field`/`Constructor` children
- `Trait`/`Interface` → `Method` children

This tree is used by the budget system to progressively strip detail (first strip children, then docs, then private decls).

## Usage Across the Codebase

- **Parser** ([[mod-parser]]): Produces `Vec<Declaration>` for each file
- **Filter** ([[topic-filtering-budget]]): Filters by kind, visibility, symbol name
- **Budget** ([[topic-filtering-budget]]): Progressive truncation operates on declaration trees
- **Output** ([[mod-output]]): Formats declarations as markdown/JSON/YAML
- **MCP helpers** ([[mod-mcp]]): `find_decl_by_name`, `find_symbols_in_decl`, `explain_decl`, `collect_public_decls` all operate on declarations
- **Diff** ([[topic-diffing]]): Compares old vs new declaration sets to detect structural changes
- **Dep graph** ([[topic-dep-graph]]): Builds symbol-level graphs from declarations
- **Complexity** ([[topic-complexity]]): Annotates declarations with metrics, collects hotspots

