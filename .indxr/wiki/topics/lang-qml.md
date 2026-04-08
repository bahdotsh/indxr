---
id: lang-qml
title: QML Language Support
page_type: topic
source_files:
- src/parser/queries/qml.rs
- src/languages.rs
- src/parser/mod.rs
- src/parser/tree_sitter_parser.rs
- src/parser/queries/mod.rs
- src/parser/complexity.rs
- src/output/markdown.rs
generated_at_ref: ''
generated_at: 2026-04-08T20:27:42Z
links_to: []
covers: []
---

# QML Language Support

QML is Qt's declarative UI markup language. It is the 9th tree-sitter language supported by indxr, using the `tree-sitter-qmljs` crate (v0.3).

## Grammar & Crate

The tree-sitter grammar comes from [tree-sitter-qmljs](https://github.com/yuja/tree-sitter-qmljs). The crate exposes `tree_sitter_qmljs::LANGUAGE` which is mapped in `get_ts_language()` in `src/parser/tree_sitter_parser.rs`.

## QML File Structure

QML files have a unique structure compared to other languages:
- The `program` root node contains `ui_import`, `ui_pragma`, and a single root `ui_object_definition` (the root component).
- All declarations (properties, signals, functions, nested components) live inside the root object's `ui_object_initializer`.
- Objects nest arbitrarily deep — a `Rectangle` can contain a `Button` which contains a `Text`.

## Declaration Mapping

| QML Construct | AST Node Kind | DeclKind | Notes |
|---|---|---|---|
| Component (`Rectangle { }`) | `ui_object_definition` | Class | Field: `type_name` for name, `initializer` for body. Recursive. |
| Property (`property int x: 0`) | `ui_property` | Field | Signature built from modifiers + type + name, **omitting the value**. |
| Signal (`signal clicked()`) | `ui_signal` | Method | Signature preserves `signal` keyword + parameters. |
| Binding (`width: 100`) | `ui_binding` | Field | Signature is **just the name** (no value expression). `id` bindings are skipped. |
| Function (`function foo() {}`) | `function_declaration` | Function | Inherited from JS grammar. Standard extraction. |
| Enum (`enum Status { ... }`) | `enum_declaration` | Enum | Variants extracted as children. |
| Inline component (`component X: Y {}`) | `ui_inline_component` | Class | Field: `name` for component name, `component` for the inner object definition. |
| Object definition binding (`Type on prop {}`) | `ui_object_definition_binding` | Class | E.g. `NumberAnimation on x { }`. |
| Annotated member | `ui_annotated_object_member` | (unwrapped) | Strips annotation wrapper, processes inner member. |
| Import (`import QtQuick 2.15`) | `ui_import` | Import | Full text preserved. |
| Pragma (`pragma ComponentBehavior: Bound`) | `ui_pragma` | — | Skipped (configuration directives). |

## Key AST Field Names

These were verified by dumping the tree-sitter AST for test QML files:

- `ui_object_definition`: `type_name` (identifier), `initializer` (ui_object_initializer)
- `ui_property`: `name` (identifier), `type` (type_identifier), `value` (expression_statement); modifiers via `ui_property_modifier` children
- `ui_signal`: `name` (identifier), `parameters` (ui_signal_parameters)
- `ui_binding`: `name` (identifier), `value` (expression_statement)
- `ui_inline_component`: `name` (identifier), `component` (ui_object_definition)
- `ui_object_definition_binding`: `type_name`, `name`, `initializer`
- `enum_declaration`: `name` (identifier), `body` (enum_body)
- `function_declaration`: `name`, `parameters`, `body` (inherited from JS/TS grammar)

## Signature Design Decisions

**Properties**: Signature is `[modifiers] property type name` — the `: value` part is omitted. This keeps signatures concise (like Rust's `field: Type` without default values). Example: `readonly property real rounding` instead of `readonly property real rounding: Config.border.rounding`.

**Bindings**: Signature is just the property name. Binding values are runtime expressions, not structural declarations. Example: `width` instead of `width: parent.width * 0.5`.

**Rationale**: In the initial implementation, full binding expressions were included in signatures, which made the Fields output extremely verbose for QML files (binding expressions can be complex JS expressions). Stripping values aligns with how Rust struct fields show `name: Type` without default values.

## Complexity Analysis

QML shares JavaScript's complexity node tables since QML embeds JS for imperative logic:
- **Function boundaries**: `function_declaration`, `method_definition`, `arrow_function`, `function_expression`
- **Branch nodes**: Same as JS/TS (if_statement, while_statement, for_statement, etc.)
- **Nesting nodes**: Same as JS/TS
- **Logical operators**: `&&`, `||`, `??` (nullish coalescing)

## Markdown Output Fix

Adding QML exposed a bug in the markdown formatter (`src/output/markdown.rs`): top-level public declarations shown in the "Public API Surface" section were **skipped** in the per-file Declarations section. For languages like Rust this is fine (a `pub struct Foo` shown in API surface doesn't need repeating). But QML root components contain all the file's declarations as children — skipping them lost everything.

**Fix**: Declarations with children are no longer skipped even if they appeared in the API surface (line 203: added `&& decl.children.is_empty()` guard).

Additionally, the Class/Struct children renderer only showed Field summaries and Method/Function children. Nested Classes (QML's nested components), Enums, and other child types were silently dropped. **Fix**: Changed to render all non-Field, non-Variant children recursively (line 280: `!matches!(child.kind, DeclKind::Field | DeclKind::Variant)`).

## Files Involved

- `Cargo.toml` — `tree-sitter-qmljs = "0.3"` dependency
- `src/languages.rs` — `Qml` variant in Language enum, extension `.qml`, name `"QML"`
- `src/parser/mod.rs` — `Language::Qml` in `ts_languages` array
- `src/parser/tree_sitter_parser.rs` — `Language::Qml => tree_sitter_qmljs::LANGUAGE.into()`
- `src/parser/queries/mod.rs` — `pub mod qml` + `QmlExtractor` registration
- `src/parser/queries/qml.rs` — QML declaration extractor (main implementation)
- `src/parser/complexity.rs` — QML added to JS complexity tables + `qml_function()` test
- `src/output/markdown.rs` — Two fixes for Class declarations with children

