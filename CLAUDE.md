# indxr

A fast Rust codebase indexer for AI agents. Extracts structural maps (declarations, imports, tree) using tree-sitter and regex parsing across 27 languages.

## Codebase Navigation

An MCP server called `indxr` is available for exploring the codebase structure.
Before reading files to understand code structure, use indxr tools:
- `get_tree` to see the directory/file layout
- `lookup_symbol` to find where functions/types are defined
- `search_signatures` to find functions by signature
- `list_declarations` to see what a file exports
- `get_imports` to check a file's dependencies
- `get_file_summary` for a complete file overview in one call
- `read_source` to read source code by symbol name or line range
- `get_file_context` for a file's summary plus reverse dependencies
- `regenerate_index` to re-index and update INDEX.md after code changes

Only read full files when you need the actual implementation, not just the structure.
