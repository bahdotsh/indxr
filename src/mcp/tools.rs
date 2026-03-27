use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::budget::estimate_tokens;
use crate::dep_graph;
use crate::diff;
use crate::github;
use crate::indexer::{self, IndexConfig};
use crate::languages::Language;
use crate::model::declarations::{DeclKind, Declaration};
use crate::model::{CodebaseIndex, FileIndex};
use crate::parser::ParserRegistry;
use crate::parser::complexity::{collect_hotspots, compute_health, sort_hotspots};

use super::helpers::*;

// ---------------------------------------------------------------------------
// Tool definitions for tools/list
// ---------------------------------------------------------------------------

pub(super) fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "lookup_symbol",
                "description": "Find declarations matching a name (case-insensitive substring search across all indexed files).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Symbol name to search for"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of results (default 50, max 200)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "If true, return columnar format [columns, rows] instead of objects (saves ~30% tokens)"
                        }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "list_declarations",
                "description": "List all declarations in a specific file, optionally filtered by kind.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path (relative to project root)"
                        },
                        "kind": {
                            "type": "string",
                            "description": "Optional declaration kind filter (e.g. fn, struct, class, trait)"
                        },
                        "shallow": {
                            "type": "boolean",
                            "description": "If true, omit children and doc_comments to reduce output size (default false)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "If true, return columnar format (implies shallow). Saves ~30% tokens."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "search_signatures",
                "description": "Search declaration signatures by substring match.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Substring to search for in signatures"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of results (default 20, max 100)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "If true, return columnar format (saves ~30% tokens)"
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_tree",
                "description": "Get the directory / file tree of the indexed codebase.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path prefix to filter the tree"
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "get_imports",
                "description": "Get the import statements for a specific file.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path (relative to project root)"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "get_stats",
                "description": "Get summary statistics for the indexed codebase.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "get_file_summary",
                "description": "Get a complete overview of a file in one call: metadata, imports, declarations (shallow), kind counts, public symbol count, and test presence.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path (relative to project root)"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "read_source",
                "description": "Read source code from a file, either by symbol name (uses indexed line info) or by explicit line range. Returns the actual source text from disk.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path (relative to project root)"
                        },
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to read (looks up declaration and extracts its source)"
                        },
                        "start_line": {
                            "type": "number",
                            "description": "Start line (1-based) for explicit line range mode"
                        },
                        "end_line": {
                            "type": "number",
                            "description": "End line (1-based, inclusive) for explicit line range mode"
                        },
                        "expand": {
                            "type": "number",
                            "description": "Extra context lines above and below the target range (default 0)"
                        },
                        "symbols": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Multiple symbol names to read in one call (alternative to single 'symbol'). Cap: 500 total lines."
                        },
                        "collapse": {
                            "type": "boolean",
                            "description": "If true, collapse nested block bodies to { ... }. Shows structure without inner implementation."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "get_file_context",
                "description": "Get a file's summary plus its dependency context: which files import it (reverse dependencies) and related files (tests, siblings in the same directory).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path (relative to project root)"
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "regenerate_index",
                "description": "Re-scan the codebase, rebuild the index, and write an updated INDEX.md to the project root. Use this after making code changes to keep the index current. Also refreshes the in-memory index used by all other tools.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "get_token_estimate",
                "description": "Estimate how many tokens a file or symbol would consume if read in full. Use this to decide whether to read_source (targeted) or Read (full file). Helps agents make informed token-budget decisions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path (relative to project root)"
                        },
                        "symbol": {
                            "type": "string",
                            "description": "Optional symbol name — if provided, estimates tokens for just that symbol's source"
                        },
                        "directory": {
                            "type": "string",
                            "description": "Directory path — estimates all files within. Alternative to path."
                        },
                        "glob": {
                            "type": "string",
                            "description": "Glob pattern — estimates all matching files. Alternative to path."
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "search_relevant",
                "description": "Search for files and symbols relevant to a query. Searches across file paths, symbol names, signatures, and doc comments. Returns ranked results. Use this as a first step to find where to look without reading any files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query — can be a concept (e.g. 'authentication'), a partial name (e.g. 'parse'), or a type pattern (e.g. 'Result<Cache>')"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of results (default 20, max 50)"
                        },
                        "kind": {
                            "type": "string",
                            "description": "Optional declaration kind filter (e.g. fn, struct, class, trait). Only returns symbols of this kind."
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "If true, return columnar format (saves ~30% tokens)"
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_diff_summary",
                "description": "Get structural changes (added/removed/modified declarations) since a git ref or for a GitHub PR. Requires either 'since_ref' or 'pr' (not both). Much cheaper than reading raw diffs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "since_ref": {
                            "type": "string",
                            "description": "Git ref to diff against (branch name, tag, or commit like HEAD~3)"
                        },
                        "pr": {
                            "type": "integer",
                            "description": "GitHub PR number — resolves the PR's base branch automatically (alternative to since_ref)"
                        }
                    }
                }
            },
            {
                "name": "batch_file_summaries",
                "description": "Get summaries for multiple files in one call. Provide paths array or glob pattern. Cap: 30 files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Array of file paths (relative to project root)"
                        },
                        "glob": {
                            "type": "string",
                            "description": "Glob pattern to match files (e.g. '*.rs', 'src/parser/*')"
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "get_callers",
                "description": "Find declarations that reference a symbol. Searches signatures and import statements across all files. Approximate — based on name matching, not full call graph.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to search for references to"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of results (default 20, max 50)"
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "get_public_api",
                "description": "Get the public API surface: only public declarations with signatures. Ideal for understanding how to use a module without reading it.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path or directory prefix (relative to project root). Omit for entire codebase."
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of declarations to return (default 100, max 500)"
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "explain_symbol",
                "description": "Get everything needed to USE a symbol: signature, doc comment, relationships, metadata. No body source — just the interface.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Symbol name to explain (exact match, case-insensitive)"
                        }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "get_related_tests",
                "description": "Find test functions related to a symbol by naming convention and file association.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Symbol name to find tests for"
                        },
                        "path": {
                            "type": "string",
                            "description": "Optional file path to scope search"
                        }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "get_dependency_graph",
                "description": "Get file-level or symbol-level dependency graph. Shows import relationships between files or extends/implements relationships between symbols. Output in DOT (Graphviz), Mermaid, or JSON format.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Scope to a subtree (file or directory prefix). Omit for entire codebase."
                        },
                        "level": {
                            "type": "string",
                            "enum": ["file", "symbol"],
                            "description": "Graph granularity: 'file' for file-to-file imports (default), 'symbol' for symbol-to-symbol relationships."
                        },
                        "format": {
                            "type": "string",
                            "enum": ["dot", "mermaid", "json"],
                            "description": "Output format (default: mermaid)."
                        },
                        "depth": {
                            "type": "number",
                            "description": "Max edge hops from scoped files/symbols (default: unlimited). Useful to limit graph size."
                        }
                    }
                }
            },
            {
                "name": "get_hotspots",
                "description": "Get the most complex functions/methods in the codebase, ranked by a composite complexity score. Useful for identifying refactoring targets and understanding where technical debt concentrates. Only includes tree-sitter parsed languages (Rust, Python, TS, JS, Go, Java, C, C++).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of results (default 20, max 100)"
                        },
                        "path": {
                            "type": "string",
                            "description": "Optional file or directory path filter"
                        },
                        "min_complexity": {
                            "type": "number",
                            "description": "Minimum cyclomatic complexity to include (default 1)"
                        },
                        "sort_by": {
                            "type": "string",
                            "enum": ["score", "complexity", "nesting", "params", "body_lines"],
                            "description": "Sort criterion (default: score — a composite of all metrics)"
                        },
                        "compact": {
                            "type": "boolean",
                            "description": "If true, return columnar format (saves ~30% tokens)"
                        }
                    }
                }
            },
            {
                "name": "get_health",
                "description": "Get a codebase health summary with aggregate complexity metrics, documentation coverage, test ratio, and quality indicators. Only complexity data from tree-sitter parsed languages is included.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Optional path filter to scope to a directory or file"
                        }
                    }
                }
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

pub(super) fn handle_tool_call(index: &CodebaseIndex, name: &str, args: &Value) -> Value {
    match name {
        "lookup_symbol" => tool_lookup_symbol(index, args),
        "list_declarations" => tool_list_declarations(index, args),
        "search_signatures" => tool_search_signatures(index, args),
        "get_tree" => tool_get_tree(index, args),
        "get_imports" => tool_get_imports(index, args),
        "get_stats" => tool_get_stats(index),
        "get_file_summary" => tool_get_file_summary(index, args),
        "read_source" => tool_read_source(index, args),
        "get_file_context" => tool_get_file_context(index, args),
        "get_token_estimate" => tool_get_token_estimate(index, args),
        "search_relevant" => tool_search_relevant(index, args),
        "batch_file_summaries" => tool_batch_file_summaries(index, args),
        "get_callers" => tool_get_callers(index, args),
        "get_public_api" => tool_get_public_api(index, args),
        "explain_symbol" => tool_explain_symbol(index, args),
        "get_related_tests" => tool_get_related_tests(index, args),
        "get_dependency_graph" => tool_get_dependency_graph(index, args),
        "get_hotspots" => tool_get_hotspots(index, args),
        "get_health" => tool_get_health(index, args),
        _ => tool_error(&format!("Unknown tool: {}", name)),
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

pub(super) fn tool_regenerate_index(index: &mut CodebaseIndex, config: &IndexConfig) -> Value {
    // Snapshot current state for delta computation
    let old_files: HashMap<PathBuf, FileIndex> = index
        .files
        .iter()
        .map(|f| (f.path.clone(), f.clone()))
        .collect();

    match indexer::regenerate_index_file(config) {
        Ok(new_index) => {
            let file_count = new_index.stats.total_files;
            let line_count = new_index.stats.total_lines;
            let output_path = new_index.root.join("INDEX.md");

            // Compute delta: union of old and new paths
            let mut all_paths: Vec<PathBuf> = old_files.keys().cloned().collect();
            for f in &new_index.files {
                if !old_files.contains_key(&f.path) {
                    all_paths.push(f.path.clone());
                }
            }

            let structural_diff = diff::compute_structural_diff(&new_index, &old_files, &all_paths);

            let has_changes = !structural_diff.files_added.is_empty()
                || !structural_diff.files_removed.is_empty()
                || !structural_diff.files_modified.is_empty();

            *index = new_index;

            let mut result = json!({
                "status": "ok",
                "message": format!(
                    "INDEX.md regenerated ({} files, {} lines)",
                    file_count, line_count
                ),
                "path": output_path.to_string_lossy(),
                "files_indexed": file_count,
                "total_lines": line_count
            });

            if has_changes {
                result["changes"] = json!({
                    "files_added": structural_diff.files_added.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
                    "files_removed": structural_diff.files_removed.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
                    "files_modified": structural_diff.files_modified.iter().map(|fd| json!({
                        "path": fd.path.to_string_lossy().to_string(),
                        "added": fd.declarations_added.len(),
                        "removed": fd.declarations_removed.len(),
                        "modified": fd.declarations_modified.len(),
                    })).collect::<Vec<_>>()
                });
            }

            tool_result(result)
        }
        Err(e) => tool_error(&format!("Failed to regenerate index: {}", e)),
    }
}

pub(super) fn tool_lookup_symbol(index: &CodebaseIndex, args: &Value) -> Value {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_error("Missing required parameter: name"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(200) as usize;

    let query = name.to_lowercase();
    let mut results = Vec::new();

    for file in &index.files {
        if results.len() >= limit {
            break;
        }
        let file_path = file.path.to_string_lossy().to_string();
        for decl in &file.declarations {
            find_symbols_in_decl(decl, &query, &file_path, &mut results, limit);
            if results.len() >= limit {
                break;
            }
        }
    }

    let total = results.len();
    let truncated = total >= limit;

    let mut result = if is_compact(args) {
        let mut r = serialize_compact(&results, &["file", "kind", "name", "signature", "line"]);
        r["matches"] = json!(total);
        r
    } else {
        json!({
            "matches": total,
            "symbols": results
        })
    };
    if truncated {
        result["truncated"] = json!(true);
        result["limit"] = json!(limit);
    }
    tool_result(result)
}

pub(super) fn tool_list_declarations(index: &CodebaseIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let file = find_file(index, path);
    let file = match file {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    let shallow = args
        .get("shallow")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let compact = is_compact(args);

    let kind_filter = args
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(DeclKind::from_name);

    if shallow || compact {
        // Shallow/compact mode: return without children/doc_comments
        let decls: Vec<ShallowDeclaration> = if let Some(ref kind) = kind_filter {
            filter_declarations(&file.declarations, kind)
                .into_iter()
                .map(to_shallow)
                .collect()
        } else {
            file.declarations.iter().map(to_shallow).collect()
        };
        if compact {
            return tool_result(json!({
                "file": path,
                "count": decls.len(),
                "declarations": serialize_compact(&decls, &["kind", "name", "signature", "line"])
            }));
        }
        tool_result(json!({
            "file": path,
            "count": decls.len(),
            "declarations": decls
        }))
    } else if let Some(ref kind) = kind_filter {
        let filtered = filter_declarations(&file.declarations, kind);
        let serialized: Vec<Value> = filtered
            .iter()
            .map(|d| serde_json::to_value(d).unwrap_or(Value::Null))
            .collect();
        tool_result(json!({
            "file": path,
            "count": serialized.len(),
            "declarations": serialized
        }))
    } else {
        tool_result(json!({
            "file": path,
            "count": file.declarations.len(),
            "declarations": file.declarations
        }))
    }
}

pub(super) fn tool_search_signatures(index: &CodebaseIndex, args: &Value) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("Missing required parameter: query"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(100) as usize;

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for file in &index.files {
        if results.len() >= limit {
            break;
        }
        let file_path = file.path.to_string_lossy().to_string();
        for decl in &file.declarations {
            find_signatures_in_decl(decl, &query_lower, &file_path, &mut results, limit);
            if results.len() >= limit {
                break;
            }
        }
    }

    let total = results.len();
    let truncated = total >= limit;

    let mut result = if is_compact(args) {
        let mut r = serialize_compact(&results, &["file", "kind", "name", "signature", "line"]);
        r["matches"] = json!(total);
        r
    } else {
        json!({
            "matches": total,
            "signatures": results
        })
    };
    if truncated {
        result["truncated"] = json!(true);
        result["limit"] = json!(limit);
    }
    tool_result(result)
}

pub(super) fn tool_get_tree(index: &CodebaseIndex, args: &Value) -> Value {
    let path_prefix = args.get("path").and_then(|v| v.as_str());

    let entries: Vec<Value> = index
        .tree
        .iter()
        .filter(|entry| {
            if let Some(prefix) = path_prefix {
                entry.path.starts_with(prefix)
            } else {
                true
            }
        })
        .map(|entry| {
            json!({
                "path": entry.path,
                "is_dir": entry.is_dir,
                "depth": entry.depth
            })
        })
        .collect();

    tool_result(json!({
        "count": entries.len(),
        "entries": entries
    }))
}

pub(super) fn tool_get_imports(index: &CodebaseIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let file = find_file(index, path);
    let file = match file {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    tool_result(json!({
        "file": path,
        "count": file.imports.len(),
        "imports": file.imports
    }))
}

pub(super) fn tool_get_stats(index: &CodebaseIndex) -> Value {
    tool_result(json!({
        "root": index.root.to_string_lossy(),
        "root_name": index.root_name,
        "generated_at": index.generated_at,
        "total_files": index.stats.total_files,
        "total_lines": index.stats.total_lines,
        "languages": index.stats.languages,
        "duration_ms": index.stats.duration_ms
    }))
}

pub(super) fn tool_get_file_summary(index: &CodebaseIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    tool_result(file_summary_data(file))
}

pub(super) fn tool_read_source(index: &CodebaseIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    let expand = args.get("expand").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let collapse = args
        .get("collapse")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Multi-symbol mode
    let symbols = args.get("symbols").and_then(|v| v.as_array());
    if let Some(sym_arr) = symbols {
        let abs_path = index.root.join(&file.path);
        let mut entries = Vec::new();
        let mut not_found = Vec::new();
        let mut total_lines = 0usize;
        let max_total_lines = 500;
        let mut truncated_at_limit = false;

        for sym_val in sym_arr {
            let sym = match sym_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            if total_lines >= max_total_lines {
                truncated_at_limit = true;
                break;
            }
            let decl = match find_decl_by_name(&file.declarations, sym) {
                Some(d) => d,
                None => {
                    not_found.push(sym.to_string());
                    continue;
                }
            };
            let body = decl.body_lines.unwrap_or(1);
            let s = if expand < decl.line {
                decl.line - expand
            } else {
                1
            };
            let e = (decl.line + body + expand).min(s + max_total_lines - total_lines - 1);

            match read_line_range(&abs_path, s, e) {
                Ok(source) => {
                    let lines_read = e - s + 1;
                    total_lines += lines_read;
                    let source = if collapse {
                        collapse_nested_bodies(&source)
                    } else {
                        source
                    };
                    entries.push(json!({
                        "symbol": decl.name,
                        "kind": format!("{}", decl.kind),
                        "start_line": s,
                        "end_line": e,
                        "source": source
                    }));
                }
                Err(_) => continue,
            }
        }

        let mut result = json!({
            "file": file.path.to_string_lossy(),
            "symbols": entries
        });
        if !not_found.is_empty() {
            result["not_found"] = json!(not_found);
        }
        if truncated_at_limit {
            result["truncated"] = json!(true);
            result["line_limit"] = json!(max_total_lines);
        }
        return tool_result(result);
    }

    // Single symbol or line range mode
    let symbol_name = args.get("symbol").and_then(|v| v.as_str());
    let start_line = args.get("start_line").and_then(|v| v.as_u64());
    let end_line = args.get("end_line").and_then(|v| v.as_u64());

    let (start, end, symbol_info) = if let Some(sym) = symbol_name {
        let decl = match find_decl_by_name(&file.declarations, sym) {
            Some(d) => d,
            None => {
                return tool_error(&format!(
                    "Symbol '{}' not found in {}",
                    sym,
                    file.path.to_string_lossy()
                ));
            }
        };
        let body = decl.body_lines.unwrap_or(1);
        let s = decl.line;
        let e = decl.line + body;
        (s, e, Some((decl.name.clone(), format!("{}", decl.kind))))
    } else if let (Some(s), Some(e)) = (start_line, end_line) {
        (s as usize, e as usize, None)
    } else {
        return tool_error("Provide 'symbol', 'symbols', or both 'start_line' and 'end_line'");
    };

    // Apply expand and cap at 200 lines
    let start = if expand < start { start - expand } else { 1 };
    let end = end + expand;
    let max_lines = 200;
    let end = if end - start + 1 > max_lines {
        start + max_lines - 1
    } else {
        end
    };

    let abs_path = index.root.join(&file.path);
    let source = match read_line_range(&abs_path, start, end) {
        Ok(s) => s,
        Err(e) => return tool_error(&e),
    };

    let source = if collapse {
        collapse_nested_bodies(&source)
    } else {
        source
    };

    let mut result = json!({
        "file": file.path.to_string_lossy(),
        "start_line": start,
        "end_line": end,
        "source": source
    });
    if let Some((name, kind)) = symbol_info {
        result["symbol"] = json!(name);
        result["kind"] = json!(kind);
    }
    if collapse {
        result["collapsed"] = json!(true);
    }
    tool_result(result)
}

pub(super) fn tool_get_file_context(index: &CodebaseIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    let mut summary = file_summary_data(file);

    // --- Reverse dependencies: find files whose imports reference this file ---
    let file_path_str = file.path.to_string_lossy().to_string();
    let file_stem = file
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // Build path-based match targets (e.g., "parser/mod", "parser")
    let path_no_ext = file.path.with_extension("").to_string_lossy().to_string();
    // For mod.rs files, also match on the parent directory name
    let parent_module = if file_stem == "mod" || file_stem == "index" {
        file.path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
    } else {
        None
    };

    #[derive(Serialize)]
    struct ImportedBy {
        file: String,
        import: String,
    }

    let mut imported_by: Vec<ImportedBy> = Vec::new();
    for other_file in &index.files {
        let other_path = other_file.path.to_string_lossy().to_string();
        if other_path == file_path_str {
            continue; // skip self
        }
        for imp in &other_file.imports {
            let text = &imp.text;
            // Skip stem-based matching for very short stems (high false-positive rate)
            let matches = (file_stem.len() >= 3 && text.contains(&file_stem))
                || text.contains(&path_no_ext)
                || parent_module
                    .as_ref()
                    .is_some_and(|pm| pm.len() >= 3 && text.contains(pm.as_str()));
            if matches {
                imported_by.push(ImportedBy {
                    file: other_path.clone(),
                    import: text.clone(),
                });
                break; // one match per file is enough
            }
        }
    }

    // --- Related files: tests and siblings ---
    #[derive(Serialize)]
    struct RelatedFile {
        file: String,
        relation: String,
    }

    let mut related: Vec<RelatedFile> = Vec::new();
    let file_dir = file.path.parent().unwrap_or(Path::new(""));

    // Test file patterns
    let test_patterns: Vec<String> = vec![
        format!("{}_test.", file_stem),
        format!("{}_spec.", file_stem),
        format!("test_{}.", file_stem),
        format!("{}.test.", file_stem),
        format!("{}.spec.", file_stem),
    ];

    let mut sibling_count = 0usize;
    for other_file in &index.files {
        let other_path_str = other_file.path.to_string_lossy().to_string();
        if other_path_str == file_path_str {
            continue;
        }

        // Check for test files (anywhere in the project)
        let other_name = other_file
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if test_patterns.iter().any(|p| other_name.starts_with(p)) {
            related.push(RelatedFile {
                file: other_path_str.clone(),
                relation: "test".to_string(),
            });
            continue;
        }

        // Check for siblings (same directory, cap at 10)
        let other_dir = other_file.path.parent().unwrap_or(Path::new(""));
        if other_dir == file_dir && sibling_count < 10 {
            related.push(RelatedFile {
                file: other_path_str,
                relation: "sibling".to_string(),
            });
            sibling_count += 1;
        }
    }

    summary["imported_by"] = json!(imported_by);
    summary["related_files"] = json!(related);

    tool_result(summary)
}

pub(super) fn tool_get_token_estimate(index: &CodebaseIndex, args: &Value) -> Value {
    let path = args.get("path").and_then(|v| v.as_str());
    let directory = args.get("directory").and_then(|v| v.as_str());
    let glob = args.get("glob").and_then(|v| v.as_str());

    // Directory/glob mode: estimate tokens for multiple files
    if let Some(dir_or_glob) = directory.or(glob) {
        let is_dir = directory.is_some();
        let glob_matcher = if !is_dir {
            compile_glob_matcher(dir_or_glob)
        } else {
            None
        };
        let matched_files: Vec<&FileIndex> = index
            .files
            .iter()
            .filter(|f| {
                let fp = f.path.to_string_lossy();
                if is_dir {
                    fp.starts_with(dir_or_glob) || fp.starts_with(&format!("{}/", dir_or_glob))
                } else {
                    match &glob_matcher {
                        Some(m) => m.is_match(fp.as_ref()),
                        None => fp == dir_or_glob || fp.starts_with(&format!("{}/", dir_or_glob)),
                    }
                }
            })
            .collect();

        let mut total_tokens = 0usize;
        let mut total_lines = 0usize;
        let mut breakdown = Vec::new();
        for f in matched_files.iter() {
            let tokens = (f.size as usize).div_ceil(4);
            total_tokens += tokens;
            total_lines += f.lines;
            if breakdown.len() < 50 {
                breakdown.push(json!({
                    "path": f.path.to_string_lossy(),
                    "tokens": tokens,
                    "lines": f.lines
                }));
            }
        }

        return tool_result(json!({
            "query": dir_or_glob,
            "file_count": matched_files.len(),
            "total_tokens": total_tokens,
            "total_lines": total_lines,
            "breakdown": breakdown,
            "recommendation": if total_tokens > 2000 {
                format!("Large scope (~{} tokens across {} files). Use get_file_summary or search_relevant to narrow down before reading.", total_tokens, matched_files.len())
            } else {
                format!("Manageable scope (~{} tokens across {} files).", total_tokens, matched_files.len())
            }
        }));
    }

    // Single file mode (original behavior)
    let path = match path {
        Some(p) => p,
        None => return tool_error("Provide 'path', 'directory', or 'glob'"),
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    // Read file content once for all estimates
    let abs_path = index.root.join(&file.path);
    let file_content = std::fs::read_to_string(&abs_path).unwrap_or_default();
    let full_file_tokens = estimate_tokens(&file_content);

    let symbol_name = args.get("symbol").and_then(|v| v.as_str());

    if let Some(sym) = symbol_name {
        let decl = match find_decl_by_name(&file.declarations, sym) {
            Some(d) => d,
            None => {
                return tool_error(&format!(
                    "Symbol '{}' not found in {}",
                    sym,
                    file.path.to_string_lossy()
                ));
            }
        };
        let body_lines = decl.body_lines.unwrap_or(1);
        let start = decl.line;
        let end = decl.line + body_lines;
        let symbol_tokens = match read_line_range(&abs_path, start, end) {
            Ok(source) => estimate_tokens(&source),
            Err(_) => body_lines * 10, // fallback heuristic
        };
        tool_result(json!({
            "file": file.path.to_string_lossy(),
            "symbol": sym,
            "symbol_tokens": symbol_tokens,
            "symbol_lines": body_lines,
            "full_file_tokens": full_file_tokens,
            "full_file_lines": file.lines,
            "savings": format!("read_source saves ~{} tokens ({}% reduction)",
                full_file_tokens.saturating_sub(symbol_tokens),
                if full_file_tokens > 0 { 100 - (symbol_tokens * 100 / full_file_tokens) } else { 0 }
            )
        }))
    } else {
        tool_result(json!({
            "file": file.path.to_string_lossy(),
            "full_file_tokens": full_file_tokens,
            "full_file_lines": file.lines,
            "summary_tokens": APPROX_SUMMARY_TOKENS,
            "declaration_count": file.declarations.len(),
            "recommendation": if full_file_tokens > 500 {
                format!("Use get_file_summary (~{} tokens) instead of Read (~{} tokens). Use read_source for specific symbols.", APPROX_SUMMARY_TOKENS, full_file_tokens)
            } else {
                format!("File is small (~{} tokens) — Read is fine here.", full_file_tokens)
            }
        }))
    }
}

pub(super) fn tool_search_relevant(index: &CodebaseIndex, args: &Value) -> Value {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return tool_error("Missing required parameter: query"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(50) as usize;
    let kind_filter = args
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(DeclKind::from_name);

    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
    let mut results: Vec<RelevanceMatch> = Vec::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        let file_path_lower = file_path.to_lowercase();

        // Score file path matches (skip when kind filter is active)
        if kind_filter.is_none() {
            let path_score = score_match(&file_path_lower, &query_lower, &query_terms);
            if path_score > 0 {
                results.push(RelevanceMatch {
                    file: file_path.clone(),
                    symbol: None,
                    kind: None,
                    signature: None,
                    line: None,
                    match_on: "path".to_string(),
                    score: path_score,
                });
            }
        }

        // Score declaration matches
        score_decls_recursive(
            &file.declarations,
            &file_path,
            &query_lower,
            &query_terms,
            &mut results,
            kind_filter.as_ref(),
        );
    }

    // Sort by score descending
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(limit);

    let total = results.len();

    if is_compact(args) {
        return tool_result(json!({
            "query": query,
            "matches": total,
            "results": serialize_compact(&results, &["file", "symbol", "kind", "signature", "line", "score"])
        }));
    }

    tool_result(json!({
        "query": query,
        "matches": total,
        "results": results
    }))
}

pub(super) fn tool_get_diff_summary(
    index: &CodebaseIndex,
    config: &IndexConfig,
    registry: &ParserRegistry,
    args: &Value,
) -> Value {
    let has_pr = args.get("pr").is_some_and(|v| !v.is_null());
    let has_since = args.get("since_ref").is_some_and(|v| !v.is_null());

    if has_pr && has_since {
        return tool_error("Provide either 'pr' or 'since_ref', not both");
    }

    // Resolve the git ref — either from a PR number or a direct since_ref
    let (resolved_ref, pr_info) = if let Some(pr_val) = args.get("pr") {
        if let Some(pr_num) = pr_val.as_u64().filter(|&n| n > 0) {
            match github::resolve_pr_base(&config.root, pr_num) {
                Ok((local_ref, info)) => (Some(local_ref), Some(info)),
                Err(e) => return tool_error(&format!("Failed to resolve PR #{}: {}", pr_num, e)),
            }
        } else {
            return tool_error("'pr' must be a positive integer");
        }
    } else {
        (None, None)
    };

    let since_ref = if let Some(r) = resolved_ref {
        r
    } else if let Some(r) = args.get("since_ref").and_then(|v| v.as_str()) {
        let r = r.trim();
        if r.is_empty() {
            return tool_error("'since_ref' must not be empty");
        }
        r.to_string()
    } else {
        return tool_error("Missing required parameter: either 'since_ref' or 'pr'");
    };

    let changed_paths = match diff::get_changed_files(&config.root, &since_ref) {
        Ok(paths) => paths,
        Err(e) => return tool_error(&format!("Git diff failed: {}", e)),
    };

    if changed_paths.is_empty() {
        let mut result = json!({
            "since_ref": since_ref,
            "changes": 0,
            "files_added": [],
            "files_removed": [],
            "files_modified": []
        });
        if let Some(ref info) = pr_info {
            result["pr"] = json!({
                "number": info.number,
                "title": &info.title,
                "base": &info.base_ref,
                "head": &info.head_ref
            });
        }
        return tool_result(result);
    }

    // Parse old file versions using cached registry
    let mut old_files: HashMap<PathBuf, FileIndex> = HashMap::new();
    for path in &changed_paths {
        if let Ok(Some(old_content)) = diff::get_file_at_ref(&config.root, path, &since_ref) {
            if let Some(lang) = Language::detect(path) {
                if let Some(parser) = registry.get_parser(&lang) {
                    if let Ok(fi) = parser.parse_file(path, &old_content) {
                        old_files.insert(path.clone(), fi);
                    }
                }
            }
        }
    }

    let mut structural_diff = diff::compute_structural_diff(index, &old_files, &changed_paths);
    structural_diff.since_ref = since_ref.to_string();

    let total_changes = structural_diff.files_added.len()
        + structural_diff.files_removed.len()
        + structural_diff.files_modified.len();

    let mut result = json!({
        "since_ref": since_ref,
        "changes": total_changes,
        "files_added": structural_diff.files_added.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "files_removed": structural_diff.files_removed.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "files_modified": structural_diff.files_modified.iter().map(|fd| json!({
            "path": fd.path.to_string_lossy().to_string(),
            "added": fd.declarations_added.iter().map(|d| json!({"kind": format!("{}", d.kind), "name": &d.name, "signature": &d.signature})).collect::<Vec<_>>(),
            "removed": fd.declarations_removed.iter().map(|d| json!({"kind": format!("{}", d.kind), "name": &d.name, "signature": &d.signature})).collect::<Vec<_>>(),
            "modified": fd.declarations_modified.iter().map(|d| json!({"kind": format!("{}", d.kind), "name": &d.name, "old_signature": &d.old_signature, "new_signature": &d.new_signature})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()
    });

    if let Some(ref info) = pr_info {
        result["pr"] = json!({
            "number": info.number,
            "title": &info.title,
            "base": &info.base_ref,
            "head": &info.head_ref
        });
    }

    tool_result(result)
}

pub(super) fn tool_batch_file_summaries(index: &CodebaseIndex, args: &Value) -> Value {
    let paths = args.get("paths").and_then(|v| v.as_array());
    let glob = args.get("glob").and_then(|v| v.as_str());

    let files: Vec<&FileIndex> = if let Some(path_arr) = paths {
        path_arr
            .iter()
            .filter_map(|v| v.as_str())
            .filter_map(|p| find_file(index, p))
            .collect()
    } else if let Some(pattern) = glob {
        let matcher = compile_glob_matcher(pattern);
        index
            .files
            .iter()
            .filter(|f| {
                let fp = f.path.to_string_lossy();
                match &matcher {
                    Some(m) => m.is_match(fp.as_ref()),
                    None => fp == pattern || fp.starts_with(&format!("{}/", pattern)),
                }
            })
            .collect()
    } else {
        return tool_error("Provide either 'paths' array or 'glob' pattern");
    };

    let cap = 30;
    let total = files.len();
    let files = &files[..files.len().min(cap)];
    let summaries: Vec<Value> = files.iter().map(|f| file_summary_data(f)).collect();

    tool_result(json!({
        "count": summaries.len(),
        "total_matched": total,
        "summaries": summaries
    }))
}

pub(super) fn tool_get_callers(index: &CodebaseIndex, args: &Value) -> Value {
    let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("Missing required parameter: symbol"),
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(50) as usize;

    let mut references: Vec<Value> = Vec::new();

    fn search_decl_refs(
        decls: &[Declaration],
        symbol: &str,
        file_path: &str,
        refs: &mut Vec<Value>,
    ) {
        for decl in decls {
            // Use word-boundary matching to avoid false positives
            // (e.g., searching for "get" shouldn't match "budget" or "widget")
            if decl.name != symbol && contains_word_boundary(&decl.signature, symbol) {
                refs.push(json!({
                    "file": file_path,
                    "name": decl.name,
                    "kind": format!("{}", decl.kind),
                    "line": decl.line,
                    "match_type": "signature"
                }));
            }
            search_decl_refs(&decl.children, symbol, file_path, refs);
        }
    }

    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();

        // Check imports (word boundary matching)
        for imp in &file.imports {
            if contains_word_boundary(&imp.text, symbol) {
                references.push(json!({
                    "file": &file_path,
                    "match_type": "import",
                    "import": &imp.text
                }));
            }
        }

        // Check declaration signatures
        search_decl_refs(&file.declarations, symbol, &file_path, &mut references);
    }

    references.truncate(limit);
    tool_result(json!({
        "symbol": symbol,
        "count": references.len(),
        "references": references
    }))
}

pub(super) fn tool_get_public_api(index: &CodebaseIndex, args: &Value) -> Value {
    let path = args.get("path").and_then(|v| v.as_str());
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .min(500) as usize;
    let mut declarations = Vec::new();

    let files: Vec<&FileIndex> = if let Some(p) = path {
        // Try exact file match first, then directory prefix
        if let Some(f) = find_file(index, p) {
            vec![f]
        } else {
            index
                .files
                .iter()
                .filter(|f| f.path.to_string_lossy().starts_with(p))
                .collect()
        }
    } else {
        index.files.iter().collect()
    };

    for file in &files {
        let file_path = file.path.to_string_lossy().to_string();
        collect_public_decls(&file.declarations, &file_path, &mut declarations);
    }

    let total = declarations.len();
    let truncated = total > limit;
    declarations.truncate(limit);

    let mut result = json!({
        "path": path.unwrap_or("(all)"),
        "count": declarations.len(),
        "declarations": declarations
    });
    if truncated {
        result["truncated"] = json!(true);
        result["total"] = json!(total);
        result["limit"] = json!(limit);
    }
    tool_result(result)
}

pub(super) fn tool_explain_symbol(index: &CodebaseIndex, args: &Value) -> Value {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_error("Missing required parameter: name"),
    };
    let name_lower = name.to_lowercase();

    fn find_matching_decls(
        decls: &[Declaration],
        name_lower: &str,
        file_path: &str,
        results: &mut Vec<Value>,
    ) {
        for decl in decls {
            if decl.name.to_lowercase() == name_lower {
                results.push(explain_decl(decl, file_path));
            }
            find_matching_decls(&decl.children, name_lower, file_path, results);
        }
    }

    let mut results = Vec::new();
    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        find_matching_decls(&file.declarations, &name_lower, &file_path, &mut results);
    }
    results.truncate(10);

    tool_result(json!({
        "name": name,
        "count": results.len(),
        "symbols": results
    }))
}

pub(super) fn tool_get_related_tests(index: &CodebaseIndex, args: &Value) -> Value {
    let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("Missing required parameter: symbol"),
    };
    let scope_path = args.get("path").and_then(|v| v.as_str());
    let symbol_lower = symbol.to_lowercase();

    let mut results = Vec::new();

    // If path scoped, search that file first
    if let Some(p) = scope_path {
        if let Some(file) = find_file(index, p) {
            let file_path = file.path.to_string_lossy().to_string();
            find_tests_for_symbol(
                &file.declarations,
                &symbol_lower,
                &file_path,
                &mut results,
                "same_file",
            );
        }
    }

    // Search all files for test declarations matching the symbol
    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        let file_path_lower = file_path.to_lowercase();

        // Check if this is a test file
        let is_test_file = file_path_lower.contains("_test.")
            || file_path_lower.contains("_spec.")
            || file_path_lower.contains(".test.")
            || file_path_lower.contains(".spec.")
            || file_path_lower.contains("/test_")
            || file_path_lower.contains("/tests/");

        let reason = if is_test_file {
            "test_file"
        } else {
            "name_convention"
        };

        // Skip if we already searched this file in scope_path
        if scope_path.is_some_and(|p| file_path == p || file_path.ends_with(&format!("/{}", p))) {
            continue;
        }

        find_tests_for_symbol(
            &file.declarations,
            &symbol_lower,
            &file_path,
            &mut results,
            reason,
        );
    }

    results.truncate(20);
    tool_result(json!({
        "symbol": symbol,
        "count": results.len(),
        "tests": results
    }))
}

// ---------------------------------------------------------------------------
// Dependency graph
// ---------------------------------------------------------------------------

pub(super) fn tool_get_dependency_graph(index: &CodebaseIndex, args: &Value) -> Value {
    let path = args.get("path").and_then(|v| v.as_str());
    let level = args.get("level").and_then(|v| v.as_str()).unwrap_or("file");
    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("mermaid");
    let depth = args
        .get("depth")
        .and_then(|v| v.as_u64())
        .map(|d| d as usize);

    let graph = match level {
        "symbol" => dep_graph::build_symbol_graph(index, path, depth),
        _ => dep_graph::build_file_graph(index, path, depth),
    };

    let node_count = graph.nodes.len();
    let edge_count = graph.edges.len();

    match format {
        "dot" => tool_result(json!({
            "format": "dot",
            "nodes": node_count,
            "edges": edge_count,
            "graph": dep_graph::format_dot(&graph)
        })),
        "json" => tool_result(json!({
            "format": "json",
            "nodes": node_count,
            "edges": edge_count,
            "graph": dep_graph::format_json(&graph)
        })),
        _ => tool_result(json!({
            "format": "mermaid",
            "nodes": node_count,
            "edges": edge_count,
            "graph": dep_graph::format_mermaid(&graph)
        })),
    }
}

// ---------------------------------------------------------------------------
// Complexity hotspots & health
// ---------------------------------------------------------------------------

pub(super) fn tool_get_hotspots(index: &CodebaseIndex, args: &Value) -> Value {
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(100) as usize;
    let path_filter = args.get("path").and_then(|v| v.as_str());
    let min_complexity = args
        .get("min_complexity")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u16;
    let sort_by = args
        .get("sort_by")
        .and_then(|v| v.as_str())
        .unwrap_or("score");

    let mut entries = collect_hotspots(index, path_filter, min_complexity);
    sort_hotspots(&mut entries, sort_by);

    let total = entries.len();
    entries.truncate(limit);

    if is_compact(args) {
        let compact = serialize_compact(
            &entries,
            &[
                "file",
                "name",
                "kind",
                "line",
                "cyclomatic",
                "max_nesting",
                "param_count",
                "body_lines",
                "score",
            ],
        );
        return tool_result(json!({ "total": total, "hotspots": compact }));
    }

    tool_result(json!({ "total": total, "hotspots": entries }))
}

pub(super) fn tool_get_health(index: &CodebaseIndex, args: &Value) -> Value {
    let path_filter = args.get("path").and_then(|v| v.as_str());
    let h = compute_health(index, path_filter);

    tool_result(json!({
        "total_functions": h.total_functions,
        "analyzed": h.analyzed,
        "complexity": {
            "avg": h.avg_cc,
            "median": h.median_cc,
            "max": h.max_cc,
            "p90": h.p90_cc
        },
        "nesting": { "avg": h.avg_nesting },
        "params": { "avg": h.avg_params },
        "body_lines": { "avg": h.avg_body_lines },
        "high_complexity_count": h.high_complexity_count,
        "high_complexity_pct": h.high_complexity_pct,
        "documented_pct": h.documented_pct,
        "test_count": h.test_count,
        "deprecated_count": h.deprecated_count,
        "public_api_count": h.public_api_count,
        "hottest_files": h.hottest_files
    }))
}
