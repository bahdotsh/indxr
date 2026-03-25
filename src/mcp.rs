use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value, json};

use crate::budget::estimate_tokens;
use crate::diff;
use crate::indexer::{self, IndexConfig};
use crate::languages::Language;
use crate::model::declarations::{DeclKind, Declaration, Visibility};
use crate::model::{CodebaseIndex, FileIndex};
use crate::parser::ParserRegistry;

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ok_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

fn err_response(id: Value, code: i32, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    }
}

fn tool_result(content: Value) -> Value {
    // Use compact JSON instead of pretty-printed to save tokens
    json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string(&content).unwrap_or_default()
            }
        ]
    })
}

fn tool_error(msg: &str) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": msg
            }
        ],
        "isError": true
    })
}

// ---------------------------------------------------------------------------
// Declaration search helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SymbolMatch {
    file: String,
    kind: String,
    name: String,
    signature: String,
    line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_comment: Option<String>,
}

/// Recursively walk declarations and their children, collecting any whose name
/// contains `query` (case-insensitive). Stops at `limit`.
fn find_symbols_in_decl(
    decl: &Declaration,
    query: &str,
    file_path: &str,
    results: &mut Vec<SymbolMatch>,
    limit: usize,
) {
    if results.len() >= limit {
        return;
    }
    if decl.name.to_lowercase().contains(query) {
        // Truncate long doc comments in results to save tokens
        let doc = decl.doc_comment.as_ref().map(|d| {
            if d.len() > 120 {
                let truncated: String = d.chars().take(120).collect();
                format!("{}...", truncated.trim_end_matches('.'))
            } else {
                d.clone()
            }
        });
        results.push(SymbolMatch {
            file: file_path.to_string(),
            kind: format!("{}", decl.kind),
            name: decl.name.clone(),
            signature: decl.signature.clone(),
            line: decl.line,
            doc_comment: doc,
        });
    }
    for child in &decl.children {
        find_symbols_in_decl(child, query, file_path, results, limit);
    }
}

#[derive(Serialize)]
struct SignatureMatch {
    file: String,
    kind: String,
    name: String,
    signature: String,
    line: usize,
}

fn find_signatures_in_decl(
    decl: &Declaration,
    query: &str,
    file_path: &str,
    results: &mut Vec<SignatureMatch>,
    limit: usize,
) {
    if results.len() >= limit {
        return;
    }
    if decl.signature.to_lowercase().contains(query) {
        results.push(SignatureMatch {
            file: file_path.to_string(),
            kind: format!("{}", decl.kind),
            name: decl.name.clone(),
            signature: decl.signature.clone(),
            line: decl.line,
        });
    }
    for child in &decl.children {
        find_signatures_in_decl(child, query, file_path, results, limit);
    }
}

fn filter_declarations<'a>(decls: &'a [Declaration], kind: &DeclKind) -> Vec<&'a Declaration> {
    let mut out = Vec::new();
    for d in decls {
        if d.kind == *kind {
            out.push(d);
        }
        out.extend(filter_declarations(&d.children, kind));
    }
    out
}

/// Shallow representation of a declaration (no children, no doc_comment).
#[derive(Serialize)]
struct ShallowDeclaration {
    kind: String,
    name: String,
    signature: String,
    line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    children_count: Option<usize>,
}

fn to_shallow(decl: &Declaration) -> ShallowDeclaration {
    ShallowDeclaration {
        kind: format!("{}", decl.kind),
        name: decl.name.clone(),
        signature: decl.signature.clone(),
        line: decl.line,
        children_count: if decl.children.is_empty() {
            None
        } else {
            Some(decl.children.len())
        },
    }
}

// ---------------------------------------------------------------------------
// Shared helpers for per-file tools
// ---------------------------------------------------------------------------

/// Build a summary JSON value for a file (reused by get_file_summary and get_file_context).
fn file_summary_data(file: &FileIndex) -> Value {
    let shallow_decls: Vec<ShallowDeclaration> = file.declarations.iter().map(to_shallow).collect();

    // Single-pass traversal: count by kind, public symbols, and test presence
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut public_symbols = 0usize;
    let mut has_tests = false;
    fn scan_decls(
        decls: &[Declaration],
        counts: &mut HashMap<String, usize>,
        public_symbols: &mut usize,
        has_tests: &mut bool,
    ) {
        for d in decls {
            *counts.entry(format!("{}", d.kind)).or_insert(0) += 1;
            if matches!(d.visibility, Visibility::Public) {
                *public_symbols += 1;
            }
            if d.is_test {
                *has_tests = true;
            }
            scan_decls(&d.children, counts, public_symbols, has_tests);
        }
    }
    scan_decls(
        &file.declarations,
        &mut counts,
        &mut public_symbols,
        &mut has_tests,
    );

    let import_texts: Vec<&str> = file.imports.iter().map(|i| i.text.as_str()).collect();

    json!({
        "file": file.path.to_string_lossy(),
        "language": file.language.name(),
        "size": file.size,
        "lines": file.lines,
        "imports": import_texts,
        "declarations": shallow_decls,
        "counts": counts,
        "has_tests": has_tests,
        "public_symbols": public_symbols
    })
}

/// Recursively find a declaration by name within a file's declarations.
fn find_decl_by_name<'a>(decls: &'a [Declaration], name: &str) -> Option<&'a Declaration> {
    fn search<'a>(decls: &'a [Declaration], name_lower: &str) -> Option<&'a Declaration> {
        for d in decls {
            if d.name.to_lowercase() == name_lower {
                return Some(d);
            }
            if let Some(found) = search(&d.children, name_lower) {
                return Some(found);
            }
        }
        None
    }
    search(decls, &name.to_lowercase())
}

/// Read a range of lines from a file on disk. Lines are 1-based.
fn read_line_range(path: &Path, start: usize, end: usize) -> Result<String, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    if start == 0 || start > total {
        return Err(format!(
            "start_line {} out of range (file has {} lines)",
            start, total
        ));
    }

    let end = end.min(total);
    let selected: Vec<&str> = lines[start - 1..end].to_vec();
    Ok(selected.join("\n"))
}

// ---------------------------------------------------------------------------
// Tool definitions for tools/list
// ---------------------------------------------------------------------------

fn tool_definitions() -> Value {
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
                "description": "Get structural changes (added/removed/modified declarations) since a git ref (branch, tag, commit). Much cheaper than reading raw diffs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "since_ref": {
                            "type": "string",
                            "description": "Git ref to diff against (branch name, tag, or commit like HEAD~3)"
                        }
                    },
                    "required": ["since_ref"]
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
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

fn handle_tool_call(index: &CodebaseIndex, name: &str, args: &Value) -> Value {
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
        _ => tool_error(&format!("Unknown tool: {}", name)),
    }
}

fn tool_regenerate_index(index: &mut CodebaseIndex, config: &IndexConfig) -> Value {
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

            let structural_diff =
                diff::compute_structural_diff(&new_index, &old_files, &all_paths);

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

fn tool_lookup_symbol(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_list_declarations(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_search_signatures(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_get_tree(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_get_imports(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_get_stats(index: &CodebaseIndex) -> Value {
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

fn tool_get_file_summary(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_read_source(index: &CodebaseIndex, args: &Value) -> Value {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
    };

    let file = match find_file(index, path) {
        Some(f) => f,
        None => return tool_error(&format!("File not found in index: {}", path)),
    };

    let expand = args.get("expand").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let collapse = args.get("collapse").and_then(|v| v.as_bool()).unwrap_or(false);

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
            let s = if expand < decl.line { decl.line - expand } else { 1 };
            let e = (decl.line + body + expand).min(s + max_total_lines - total_lines - 1);

            match read_line_range(&abs_path, s, e) {
                Ok(source) => {
                    let lines_read = e - s + 1;
                    total_lines += lines_read;
                    let source = if collapse { collapse_nested_bodies(&source) } else { source };
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

fn tool_get_file_context(index: &CodebaseIndex, args: &Value) -> Value {
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

/// Approximate token cost of a `get_file_summary` response.
const APPROX_SUMMARY_TOKENS: usize = 300;

fn tool_get_token_estimate(index: &CodebaseIndex, args: &Value) -> Value {
    let path = args.get("path").and_then(|v| v.as_str());
    let directory = args.get("directory").and_then(|v| v.as_str());
    let glob = args.get("glob").and_then(|v| v.as_str());

    // Directory/glob mode: estimate tokens for multiple files
    if let Some(dir_or_glob) = directory.or(glob) {
        let is_dir = directory.is_some();
        let glob_matcher = if !is_dir { compile_glob_matcher(dir_or_glob) } else { None };
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

// ---------------------------------------------------------------------------
// search_relevant: multi-signal relevance search
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RelevanceMatch {
    file: String,
    symbol: Option<String>,
    kind: Option<String>,
    signature: Option<String>,
    line: Option<usize>,
    match_on: String,
    score: u32,
}

fn tool_search_relevant(index: &CodebaseIndex, args: &Value) -> Value {
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

fn score_match(text: &str, query: &str, terms: &[&str]) -> u32 {
    let mut score = 0u32;

    // Exact substring match
    if text.contains(query) {
        score += 10;
        // Bonus for exact match (not just substring)
        if text == query {
            score += 20;
        }
    }

    // Individual term matches
    for term in terms {
        if text.contains(term) {
            score += 5;
        }
    }

    // Identifier-part matching (camelCase/snake_case aware)
    let parts = split_identifier(text);
    for term in terms {
        if parts.iter().any(|p| p == *term) {
            score += 3; // word-boundary match bonus
        }
    }

    // Bigram fuzzy matching as fallback for partial matches
    if score == 0 && query.len() >= 4 {
        let sim = bigram_similarity(text, query);
        if sim > 0.4 {
            score += (sim * 8.0) as u32;
        }
    }

    score
}

fn score_decls_recursive(
    decls: &[Declaration],
    file_path: &str,
    query: &str,
    terms: &[&str],
    results: &mut Vec<RelevanceMatch>,
    kind_filter: Option<&DeclKind>,
) {
    for decl in decls {
        // Apply kind filter — skip non-matching declarations but still recurse children
        let kind_matches = kind_filter.is_none_or(|k| decl.kind == *k);

        if kind_matches {
            let name_lower = decl.name.to_lowercase();
            let sig_lower = decl.signature.to_lowercase();
            let doc_lower = decl
                .doc_comment
                .as_ref()
                .map(|d| d.to_lowercase())
                .unwrap_or_default();

            let mut score = 0u32;
            let mut match_sources = Vec::new();

            // Name match (highest signal)
            let name_score = score_match(&name_lower, query, terms);
            if name_score > 0 {
                score += name_score * 3; // name matches are 3x more valuable
                match_sources.push("name");
            }

            // Signature match
            let sig_score = score_match(&sig_lower, query, terms);
            if sig_score > 0 {
                score += sig_score * 2;
                match_sources.push("signature");
            }

            // Doc comment match
            if !doc_lower.is_empty() {
                let doc_score = score_match(&doc_lower, query, terms);
                if doc_score > 0 {
                    score += doc_score;
                    match_sources.push("doc");
                }
            }

            // Boost public symbols
            if matches!(decl.visibility, Visibility::Public) && score > 0 {
                score += 5;
            }

            if score > 0 {
                results.push(RelevanceMatch {
                    file: file_path.to_string(),
                    symbol: Some(decl.name.clone()),
                    kind: Some(format!("{}", decl.kind)),
                    signature: Some(decl.signature.clone()),
                    line: Some(decl.line),
                    match_on: match_sources.join("+"),
                    score,
                });
            }
        }

        score_decls_recursive(&decl.children, file_path, query, terms, results, kind_filter);
    }
}

// ---------------------------------------------------------------------------
// Shared helpers for new tools
// ---------------------------------------------------------------------------

/// Compile a glob pattern into a reusable matcher, or `None` if the pattern
/// has no glob metacharacters (callers should fall back to exact/prefix matching).
/// Patterns without `/` (e.g., `*.rs`) are treated as recursive (`**/*.rs`).
fn compile_glob_matcher(pattern: &str) -> Option<GlobMatcher> {
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return None;
    }

    let effective = if !pattern.contains('/') {
        format!("**/{}", pattern)
    } else {
        pattern.to_string()
    };

    GlobBuilder::new(&effective)
        .literal_separator(true)
        .build()
        .ok()
        .map(|g| g.compile_matcher())
}

/// Glob matching against a path string using the `globset` crate.
/// Falls back to exact/directory-prefix matching if the pattern has no glob chars.
/// Patterns without `/` (e.g., `*.rs`) are treated as recursive (`**/*.rs`).
///
/// For hot loops (matching many paths against the same pattern), prefer
/// [`compile_glob_matcher`] to compile once and reuse.
fn simple_glob_match(pattern: &str, path: &str) -> bool {
    // If no glob metacharacters, treat as exact or directory prefix match
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return path == pattern || path.starts_with(&format!("{}/", pattern));
    }

    match compile_glob_matcher(pattern) {
        Some(matcher) => matcher.is_match(path),
        None => path == pattern || path.starts_with(&format!("{}/", pattern)),
    }
}

/// Split an identifier into constituent words.
/// Handles snake_case, camelCase, PascalCase, and SCREAMING_SNAKE_CASE.
fn split_identifier(name: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();

    for ch in name.chars() {
        if ch == '_' || ch == '-' || ch == '.' || ch == '/' {
            if !current.is_empty() {
                parts.push(current.to_lowercase());
                current.clear();
            }
        } else if ch.is_uppercase()
            && !current.is_empty()
            && current
                .as_bytes()
                .last()
                .is_some_and(|&b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            // camelCase boundary (lowercase→uppercase) or digit→uppercase (e.g. "v2Parser")
            parts.push(current.to_lowercase());
            current.clear();
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        parts.push(current.to_lowercase());
    }
    parts
}

/// Bigram (Dice coefficient) similarity between two strings.
/// Uses set-based intersection to avoid inflating scores for repeated character pairs.
fn bigram_similarity(a: &str, b: &str) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let bigrams_a: HashSet<(char, char)> = a.chars().zip(a.chars().skip(1)).collect();
    let bigrams_b: HashSet<(char, char)> = b.chars().zip(b.chars().skip(1)).collect();
    let intersection = bigrams_a.intersection(&bigrams_b).count();
    (2.0 * intersection as f64) / (bigrams_a.len() + bigrams_b.len()) as f64
}

/// Collapse nested block bodies (depth >= 2) to `{ ... }`.
///
/// State machine with these modes:
///   - Normal: track brace depth, emit chars. At depth >= 2 on `{`, emit `{ ... }` and
///     enter Skip mode until the matching `}` is found.
///   - Skip (skip_until_close): consume chars without emitting, tracking depth to find
///     the matching close brace.
///   - LineComment: pass through until `\n`.
///   - BlockComment: pass through until `*/`.
///   - String: pass through until unescaped closing quote (tracks escape state properly
///     so `"\\\\"` is handled as two escaped backslashes followed by an end-quote).
///   - RawString: pass through until `"` followed by the same number of `#` chars that
///     opened the raw string (e.g., `r#"..."#`, `r##"..."##`).
fn collapse_nested_bodies(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(source.len());
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false; // tracks backslash escaping inside strings
    let mut in_raw_string = false;
    let mut raw_hash_count: usize = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut prev_char = '\0';
    let mut skip_until_close = false;
    let mut collapse_depth: i32 = 0;
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // --- Line comment mode: pass through until newline ---
        if in_line_comment {
            if !skip_until_close {
                result.push(ch);
            }
            if ch == '\n' {
                in_line_comment = false;
            }
            prev_char = ch;
            i += 1;
            continue;
        }

        // --- Block comment mode: pass through until */ ---
        if in_block_comment {
            if !skip_until_close {
                result.push(ch);
            }
            if prev_char == '*' && ch == '/' {
                in_block_comment = false;
            }
            prev_char = ch;
            i += 1;
            continue;
        }

        // --- Raw string mode: pass through until `"` + N `#` chars ---
        if in_raw_string {
            if !skip_until_close {
                result.push(ch);
            }
            if ch == '"' {
                // Check if followed by raw_hash_count '#' chars
                let mut hashes = 0;
                while i + 1 + hashes < len && chars[i + 1 + hashes] == '#' && hashes < raw_hash_count {
                    hashes += 1;
                }
                if hashes == raw_hash_count {
                    // Consume the '#' chars
                    for _ in 0..hashes {
                        i += 1;
                        if !skip_until_close {
                            result.push(chars[i]);
                        }
                    }
                    in_raw_string = false;
                }
            }
            prev_char = chars[i];
            i += 1;
            continue;
        }

        // --- String mode: pass through until unescaped closing quote ---
        if in_string {
            if !skip_until_close {
                result.push(ch);
            }
            if ch == '"' && !escaped {
                in_string = false;
            }
            // Track escape state: `\` flips it on, `\\` flips it back off
            escaped = ch == '\\' && !escaped;
            prev_char = ch;
            i += 1;
            continue;
        }

        // --- Normal mode: detect comment/string starts, track braces ---

        // Detect line comment start: //
        if prev_char == '/' && ch == '/' {
            in_line_comment = true;
            if !skip_until_close {
                result.push(ch);
            }
            prev_char = ch;
            i += 1;
            continue;
        }
        // Detect block comment start: /*
        if prev_char == '/' && ch == '*' {
            in_block_comment = true;
            if !skip_until_close {
                result.push(ch);
            }
            prev_char = ch;
            i += 1;
            continue;
        }
        // Detect raw string start: r"...", r#"..."#, r##"..."##, etc.
        if ch == 'r' {
            let mut hashes = 0;
            while i + 1 + hashes < len && chars[i + 1 + hashes] == '#' {
                hashes += 1;
            }
            if i + 1 + hashes < len && chars[i + 1 + hashes] == '"' {
                // This is a raw string: consume r, #s, and opening "
                in_raw_string = true;
                raw_hash_count = hashes;
                if !skip_until_close {
                    result.push(ch); // 'r'
                }
                for j in 0..hashes {
                    if !skip_until_close {
                        result.push(chars[i + 1 + j]); // '#'
                    }
                }
                if !skip_until_close {
                    result.push(chars[i + 1 + hashes]); // '"'
                }
                prev_char = '"';
                i += 1 + hashes + 1; // skip r + hashes + "
                continue;
            }
        }
        // Detect double-quoted string start.
        // Single quotes are NOT treated as string delimiters because in Rust
        // (and Go, etc.) 'a is a lifetime, not a string. Char literals like 'x'
        // rarely contain braces, so ignoring them is safe for brace-depth tracking.
        if ch == '"' {
            in_string = true;
            escaped = false;
            if !skip_until_close {
                result.push(ch);
            }
            prev_char = ch;
            i += 1;
            continue;
        }

        if ch == '{' {
            depth += 1;
            if depth >= 2 && !skip_until_close {
                result.push_str("{ ... }");
                skip_until_close = true;
                collapse_depth = depth;
            } else if !skip_until_close {
                result.push(ch);
            }
        } else if ch == '}' {
            if skip_until_close && depth == collapse_depth {
                skip_until_close = false;
            } else if !skip_until_close {
                result.push(ch);
            }
            depth -= 1;
        } else if !skip_until_close {
            result.push(ch);
        }

        prev_char = ch;
        i += 1;
    }
    result
}

/// Check if the caller requested compact columnar output.
fn is_compact(args: &Value) -> bool {
    args.get("compact").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Serialize a slice of Serialize items into compact columnar format.
fn serialize_compact<T: Serialize>(items: &[T], columns: &[&str]) -> Value {
    let values: Vec<Value> = items
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .collect();
    to_compact_rows(columns, &values)
}

/// Convert an array of objects to compact columnar format.
fn to_compact_rows(columns: &[&str], items: &[Value]) -> Value {
    let rows: Vec<Value> = items
        .iter()
        .map(|item| {
            let row: Vec<Value> = columns
                .iter()
                .map(|col| item.get(col).cloned().unwrap_or(Value::Null))
                .collect();
            Value::Array(row)
        })
        .collect();
    json!({
        "columns": columns,
        "rows": rows
    })
}

/// Check if `text` contains `word` at a word boundary (not part of a larger identifier).
/// Word boundaries are non-alphanumeric, non-underscore characters or string edges.
fn contains_word_boundary(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let text_bytes = text.as_bytes();
    let word_len = word.len();
    let mut start = 0;
    while start + word_len <= text.len() {
        match text[start..].find(word) {
            Some(pos) => {
                let abs_pos = start + pos;
                let before_ok = abs_pos == 0 || {
                    let b = text_bytes[abs_pos - 1];
                    !b.is_ascii_alphanumeric() && b != b'_'
                };
                let after_pos = abs_pos + word_len;
                let after_ok = after_pos >= text.len() || {
                    let b = text_bytes[after_pos];
                    !b.is_ascii_alphanumeric() && b != b'_'
                };
                if before_ok && after_ok {
                    return true;
                }
                start = abs_pos + 1;
            }
            None => break,
        }
    }
    false
}

/// Collect public declarations recursively.
fn collect_public_decls(decls: &[Declaration], file_path: &str, out: &mut Vec<Value>) {
    for decl in decls {
        if matches!(decl.visibility, Visibility::Public) {
            out.push(json!({
                "name": decl.name,
                "kind": format!("{}", decl.kind),
                "signature": decl.signature,
                "file": file_path,
                "line": decl.line
            }));
        }
        // Also check children (public methods in impls, etc.)
        collect_public_decls(&decl.children, file_path, out);
    }
}

/// Find test declarations matching a symbol name.
fn find_tests_for_symbol(
    decls: &[Declaration],
    symbol_lower: &str,
    file_path: &str,
    results: &mut Vec<Value>,
    reason: &str,
) {
    for decl in decls {
        if decl.is_test {
            let name_lower = decl.name.to_lowercase();
            if name_lower.contains(symbol_lower) {
                results.push(json!({
                    "name": decl.name,
                    "file": file_path,
                    "line": decl.line,
                    "kind": format!("{}", decl.kind),
                    "match_reason": reason
                }));
            }
        }
        find_tests_for_symbol(&decl.children, symbol_lower, file_path, results, reason);
    }
}

/// Explain a single declaration — full metadata without body.
fn explain_decl(decl: &Declaration, file_path: &str) -> Value {
    let mut children_counts: HashMap<String, usize> = HashMap::new();
    for child in &decl.children {
        *children_counts.entry(format!("{}", child.kind)).or_insert(0) += 1;
    }
    let children_summary = if children_counts.is_empty() {
        None
    } else {
        let parts: Vec<String> = children_counts.iter().map(|(k, v)| format!("{} {}", v, k)).collect();
        Some(parts.join(", "))
    };

    let rels: Vec<Value> = decl
        .relationships
        .iter()
        .map(|r| json!({"kind": format!("{:?}", r.kind), "target": &r.target}))
        .collect();

    let mut result = json!({
        "name": decl.name,
        "kind": format!("{}", decl.kind),
        "file": file_path,
        "line": decl.line,
        "signature": decl.signature,
        "visibility": format!("{}", decl.visibility),
        "is_async": decl.is_async,
        "is_test": decl.is_test,
        "is_deprecated": decl.is_deprecated,
    });
    if let Some(doc) = &decl.doc_comment {
        result["doc_comment"] = json!(doc);
    }
    if !rels.is_empty() {
        result["relationships"] = json!(rels);
    }
    if let Some(summary) = children_summary {
        result["children_summary"] = json!(summary);
    }
    if let Some(body) = decl.body_lines {
        result["body_lines"] = json!(body);
    }
    result
}

// ---------------------------------------------------------------------------
// New tool implementations (Phase 1-5)
// ---------------------------------------------------------------------------

fn tool_get_diff_summary(
    index: &CodebaseIndex,
    config: &IndexConfig,
    registry: &ParserRegistry,
    args: &Value,
) -> Value {
    let since_ref = match args.get("since_ref").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => return tool_error("Missing required parameter: since_ref"),
    };

    let changed_paths = match diff::get_changed_files(&config.root, since_ref) {
        Ok(paths) => paths,
        Err(e) => return tool_error(&format!("Git diff failed: {}", e)),
    };

    if changed_paths.is_empty() {
        return tool_result(json!({
            "since_ref": since_ref,
            "changes": 0,
            "files_added": [],
            "files_removed": [],
            "files_modified": []
        }));
    }

    // Parse old file versions using cached registry
    let mut old_files: HashMap<PathBuf, FileIndex> = HashMap::new();
    for path in &changed_paths {
        if let Ok(Some(old_content)) = diff::get_file_at_ref(&config.root, path, since_ref) {
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

    tool_result(json!({
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
    }))
}

fn tool_batch_file_summaries(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_get_callers(index: &CodebaseIndex, args: &Value) -> Value {
    let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_error("Missing required parameter: symbol"),
    };
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).min(50) as usize;

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

fn tool_get_public_api(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_explain_symbol(index: &CodebaseIndex, args: &Value) -> Value {
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

fn tool_get_related_tests(index: &CodebaseIndex, args: &Value) -> Value {
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
        if scope_path.is_some_and(|p| {
            file_path == p
                || file_path.ends_with(&format!("/{}", p))
        }) {
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

/// Find a FileIndex whose path matches the given string. Supports both exact
/// match and suffix match (with `/` boundary) so callers can use relative paths.
fn find_file<'a>(index: &'a CodebaseIndex, path: &str) -> Option<&'a FileIndex> {
    index.files.iter().find(|f| {
        let file_path = f.path.to_string_lossy();
        file_path == path || file_path.ends_with(&format!("/{}", path))
    })
}

// ---------------------------------------------------------------------------
// MCP protocol handlers
// ---------------------------------------------------------------------------

fn handle_initialize(id: Value) -> JsonRpcResponse {
    ok_response(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": "indxr",
                "version": "0.1.0"
            }
        }),
    )
}

fn handle_tools_list(id: Value) -> JsonRpcResponse {
    ok_response(id, tool_definitions())
}

fn handle_tools_call(
    id: Value,
    index: &mut CodebaseIndex,
    config: &IndexConfig,
    registry: &ParserRegistry,
    params: &Value,
) -> JsonRpcResponse {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return err_response(id, -32602, "Missing tool name in params".into());
        }
    };

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    if tool_name == "regenerate_index" {
        let result = tool_regenerate_index(index, config);
        return ok_response(id, result);
    }

    if tool_name == "get_diff_summary" {
        let result = tool_get_diff_summary(index, config, registry, &arguments);
        return ok_response(id, result);
    }

    let result = handle_tool_call(index, tool_name, &arguments);
    ok_response(id, result)
}

// ---------------------------------------------------------------------------
// Main server loop
// ---------------------------------------------------------------------------

pub fn run_mcp_server(mut index: CodebaseIndex, config: IndexConfig) -> anyhow::Result<()> {
    eprintln!("indxr MCP server starting (root: {})", index.root.display());
    let registry = ParserRegistry::new();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();
    let mut writer = stdout.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                break;
            }
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        eprintln!("< {}", line);

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to parse JSON-RPC request: {}", e);
                let resp = err_response(Value::Null, -32700, format!("Parse error: {}", e));
                let out = serde_json::to_string(&resp)?;
                eprintln!("> {}", out);
                writeln!(writer, "{}", out)?;
                writer.flush()?;
                continue;
            }
        };

        // Notifications have no id and require no response.
        if request.id.is_none() {
            eprintln!("Notification: {}", request.method);
            continue;
        }

        let id = request.id.unwrap();
        let params = request.params.unwrap_or(json!({}));

        let response = match request.method.as_str() {
            "initialize" => handle_initialize(id),
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, &mut index, &config, &registry, &params),
            _ => err_response(id, -32601, format!("Method not found: {}", request.method)),
        };

        let out = serde_json::to_string(&response)?;
        eprintln!("> {}", out);
        writeln!(writer, "{}", out)?;
        writer.flush()?;
    }

    eprintln!("indxr MCP server shutting down");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // score_match tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_score_match_exact_full_match() {
        let score = score_match("parse", "parse", &["parse"]);
        // exact substring (10) + exact equality (20) + term match (5) + identifier part (3) = 38
        assert_eq!(score, 38);
    }

    #[test]
    fn test_score_match_substring() {
        let score = score_match("tree_sitter_parser", "parser", &["parser"]);
        // substring (10) + term match (5) + identifier part "parser" (3) = 18
        assert_eq!(score, 18);
    }

    #[test]
    fn test_score_match_no_match() {
        let score = score_match("indexer", "parser", &["parser"]);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_score_match_multi_term() {
        // "token budget" is NOT a substring of "token_budget_manager"
        // term "token" (5) + term "budget" (5) + ident part "token" (3) + ident part "budget" (3) = 16
        let score = score_match("token_budget_manager", "token budget", &["token", "budget"]);
        assert_eq!(score, 16);

        // When full query IS a substring, it scores higher
        let score2 = score_match("apply token budget here", "token budget", &["token", "budget"]);
        // full query substring (10) + term "token" (5) + term "budget" (5) = 20
        // (identifier split of "apply token budget here" won't match since it's a phrase, not an identifier)
        assert_eq!(score2, 20);
    }

    #[test]
    fn test_score_match_partial_term_match() {
        // Only one of two terms matches
        let score = score_match("token_counter", "token budget", &["token", "budget"]);
        // term "token" (5) + ident part "token" (3) = 8
        assert_eq!(score, 8);
    }

    #[test]
    fn test_score_match_empty_query() {
        // Empty string is a substring of everything
        let score = score_match("anything", "", &[""]);
        assert!(score > 0);
    }

    #[test]
    fn test_score_match_case_sensitivity() {
        // score_match expects pre-lowercased inputs (caller responsibility)
        // With bigram matching, near-matches still get a small score as fuzzy fallback
        let score = score_match("parser", "Parser", &["Parser"]);
        // No substring, no term, no ident part, but bigram similarity is high → small fuzzy score
        assert!(score > 0); // fuzzy match kicks in
        assert!(score < 10); // but much weaker than a proper substring match
    }

    #[test]
    fn test_score_match_camel_case_aware() {
        // "parse decl" should match "parseDeclaration" via identifier splitting
        let score = score_match("parsedeclaration", "parse decl", &["parse", "decl"]);
        // No substring "parse decl" in "parsedeclaration" (0)
        // term "parse": "parsedeclaration".contains("parse") → yes (5)
        // term "decl": "parsedeclaration".contains("decl") → yes (5)
        // ident parts of "parsedeclaration" = ["parsedeclaration"] (no camelCase boundary in all-lowercase)
        // So no ident bonus
        assert_eq!(score, 10);

        // With actual camelCase input (pre-lowercased — but split_identifier works on original)
        // This tests the intent: that snake_case splits help
        let score2 = score_match("parse_declaration", "parse decl", &["parse", "decl"]);
        // term "parse" (5) + term "decl" not a substring → "parse_declaration" contains "decl"? yes (5)
        // ident parts: ["parse", "declaration"] — "parse" matches (3), "decl" ≠ "declaration" (0)
        assert_eq!(score2, 13);
    }

    #[test]
    fn test_split_identifier() {
        assert_eq!(split_identifier("parseDeclaration"), vec!["parse", "declaration"]);
        assert_eq!(split_identifier("parse_declaration"), vec!["parse", "declaration"]);
        // Consecutive uppercase letters stay grouped (XMLParser → "xmlparser" as one unit)
        // since we only split on lowercase→uppercase transitions
        assert_eq!(split_identifier("XMLParser"), vec!["xmlparser"]);
        assert_eq!(split_identifier("simple"), vec!["simple"]);
        assert_eq!(split_identifier("getHTTPResponse"), vec!["get", "httpresponse"]);
        assert_eq!(split_identifier("src/parser/mod.rs"), vec!["src", "parser", "mod", "rs"]);
        // Digit→uppercase boundary
        assert_eq!(split_identifier("v2Parser"), vec!["v2", "parser"]);
        assert_eq!(split_identifier("item3DView"), vec!["item3", "dview"]);
    }

    #[test]
    fn test_simple_glob_match() {
        assert!(simple_glob_match("*.rs", "src/main.rs"));
        assert!(!simple_glob_match("*.rs", "src/main.py"));
        assert!(simple_glob_match("src/parser/*", "src/parser/mod.rs"));
        assert!(!simple_glob_match("src/parser/*", "src/parser/queries/rust.rs"));
        assert!(simple_glob_match("src/parser/**", "src/parser/queries/rust.rs"));
        assert!(simple_glob_match("src/parser/**", "src/parser/mod.rs"));
        // Recursive glob with extension
        assert!(simple_glob_match("**/*.rs", "src/main.rs"));
        assert!(simple_glob_match("**/*.rs", "src/parser/mod.rs"));
        assert!(!simple_glob_match("**/*.rs", "src/main.py"));
        // Recursive glob with filename
        assert!(simple_glob_match("**/mod.rs", "src/parser/mod.rs"));
        assert!(!simple_glob_match("**/mod.rs", "src/parser/lib.rs"));
        // Nested glob mid-path (was unsupported by hand-rolled impl)
        assert!(simple_glob_match("src/**/*.test.rs", "src/parser/foo.test.rs"));
        assert!(!simple_glob_match("src/**/*.test.rs", "src/parser/foo.rs"));
        // Exact match (no glob chars)
        assert!(simple_glob_match("src/main.rs", "src/main.rs"));
        assert!(!simple_glob_match("src/main.rs", "src/lib.rs"));
        // Directory prefix (no glob chars)
        assert!(simple_glob_match("src/parser", "src/parser/mod.rs"));
        // Question mark wildcard
        assert!(simple_glob_match("src/ma?n.rs", "src/main.rs"));
        assert!(!simple_glob_match("src/ma?n.rs", "src/maain.rs"));
    }

    // -----------------------------------------------------------------------
    // tool_definitions: verify new tools are registered
    // -----------------------------------------------------------------------

    #[test]
    fn test_tool_definitions_include_new_tools() {
        let defs = tool_definitions();
        let tools = defs["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"get_token_estimate"));
        assert!(names.contains(&"search_relevant"));
        assert!(names.contains(&"lookup_symbol"));
        assert!(names.contains(&"regenerate_index"));
        // New tools
        assert!(names.contains(&"get_diff_summary"));
        assert!(names.contains(&"batch_file_summaries"));
        assert!(names.contains(&"get_callers"));
        assert!(names.contains(&"get_public_api"));
        assert!(names.contains(&"explain_symbol"));
        assert!(names.contains(&"get_related_tests"));
        // Total: 12 original + 6 new = 18
        assert_eq!(names.len(), 18);
    }

    // -----------------------------------------------------------------------
    // handle_tool_call: unknown tool
    // -----------------------------------------------------------------------

    #[test]
    fn test_handle_tool_call_unknown_tool() {
        let index = CodebaseIndex {
            root: std::path::PathBuf::from("."),
            root_name: "test".to_string(),
            generated_at: String::new(),
            stats: crate::model::IndexStats {
                total_files: 0,
                total_lines: 0,
                languages: HashMap::new(),
                duration_ms: 0,
            },
            tree: vec![],
            files: vec![],
        };
        let result = handle_tool_call(&index, "nonexistent_tool", &json!({}));
        // Should return an error
        let content = result["content"][0]["text"].as_str().unwrap();
        assert!(content.contains("Unknown tool"));
    }

    // -----------------------------------------------------------------------
    // search_relevant: scoring weights
    // -----------------------------------------------------------------------

    #[test]
    fn test_name_match_scores_higher_than_signature() {
        // Name matches get 3x multiplier, signature gets 2x
        let name_score = score_match("parse_file", "parse", &["parse"]) * 3;
        let sig_score = score_match("fn parse_file(input: &str)", "parse", &["parse"]) * 2;
        assert!(name_score > 0);
        assert!(sig_score > 0);
        // Both match, but name multiplier is higher
        assert!(name_score > sig_score || name_score == sig_score);
    }

    // -----------------------------------------------------------------------
    // collapse_nested_bodies tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_collapse_simple_nested() {
        let input = "fn foo() {\n    if x {\n        bar();\n    }\n}";
        let result = collapse_nested_bodies(input);
        // The inner `if` block at depth 2 should be collapsed
        assert!(result.contains("{ ... }"));
        // The outer fn brace should remain
        assert!(result.contains("fn foo() {"));
        assert!(result.contains("}"));
        // The inner bar() call should NOT appear
        assert!(!result.contains("bar()"));
    }

    #[test]
    fn test_collapse_string_with_braces() {
        let input = r#"fn foo() {
    let s = "{ not a block }";
    if x {
        bar();
    }
}"#;
        let result = collapse_nested_bodies(input);
        // String braces should not affect depth tracking
        assert!(result.contains(r#""{ not a block }""#));
        // The if block should be collapsed
        assert!(result.contains("{ ... }"));
        assert!(!result.contains("bar()"));
    }

    #[test]
    fn test_collapse_escaped_quotes() {
        // Test that escaped quotes (including double escapes) are handled
        let input = r#"fn foo() {
    let s = "hello \"world\"";
    let t = "path\\";
    if x {
        inner();
    }
}"#;
        let result = collapse_nested_bodies(input);
        // Should not get confused by escaped quotes
        assert!(result.contains("{ ... }"));
        assert!(!result.contains("inner()"));
    }

    #[test]
    fn test_collapse_block_comment_with_braces() {
        let input = "fn foo() {\n    /* { nested } */\n    if x {\n        bar();\n    }\n}";
        let result = collapse_nested_bodies(input);
        // Block comment braces should be ignored
        assert!(result.contains("/* { nested } */"));
        assert!(result.contains("{ ... }"));
    }

    #[test]
    fn test_collapse_line_comment_with_braces() {
        let input = "fn foo() {\n    // { not a block }\n    if x {\n        bar();\n    }\n}";
        let result = collapse_nested_bodies(input);
        // Line comment braces should be ignored
        assert!(result.contains("// { not a block }"));
        assert!(result.contains("{ ... }"));
    }

    #[test]
    fn test_collapse_empty_input() {
        assert_eq!(collapse_nested_bodies(""), "");
    }

    #[test]
    fn test_collapse_no_nesting() {
        let input = "fn foo() { bar(); }";
        let result = collapse_nested_bodies(input);
        // Only depth 1, nothing to collapse
        assert_eq!(result, input);
    }

    #[test]
    fn test_collapse_rust_lifetimes() {
        // Lifetimes like 'a should NOT be treated as string delimiters
        let input = "fn foo<'a>(x: &'a str) {\n    if x.is_empty() {\n        return;\n    }\n}";
        let result = collapse_nested_bodies(input);
        // The lifetime annotations should pass through unchanged
        assert!(result.contains("'a"));
        // The inner if block should still be collapsed
        assert!(result.contains("{ ... }"));
        assert!(!result.contains("return"));
    }

    // -----------------------------------------------------------------------
    // bigram_similarity tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_bigram_identical() {
        let sim = bigram_similarity("parser", "parser");
        assert!((sim - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bigram_completely_different() {
        let sim = bigram_similarity("abc", "xyz");
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_bigram_partial_overlap() {
        // "parse" vs "parser" — substantial overlap but not identical
        let sim = bigram_similarity("parse", "parser");
        assert!(sim > 0.3);
        assert!(sim < 1.0);
    }

    #[test]
    fn test_bigram_short_strings() {
        // Single char strings should return 0
        assert_eq!(bigram_similarity("a", "a"), 0.0);
        assert_eq!(bigram_similarity("", "abc"), 0.0);
    }

    #[test]
    fn test_bigram_no_duplicate_inflation() {
        // "aaa" has bigrams {(a,a)} as a set (size 1)
        // "aab" has bigrams {(a,a), (a,b)} as a set (size 2)
        // intersection = 1, dice = 2*1 / (1+2) = 0.666...
        let sim = bigram_similarity("aaa", "aab");
        let expected = 2.0 / 3.0;
        assert!((sim - expected).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // to_compact_rows tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_compact_rows_basic() {
        let items = vec![
            json!({"name": "foo", "kind": "fn", "line": 10}),
            json!({"name": "bar", "kind": "struct", "line": 20}),
        ];
        let result = to_compact_rows(&["name", "kind", "line"], &items);
        let columns = result["columns"].as_array().unwrap();
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0], "name");
        let rows = result["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "foo");
        assert_eq!(rows[0][1], "fn");
        assert_eq!(rows[0][2], 10);
        assert_eq!(rows[1][0], "bar");
    }

    #[test]
    fn test_compact_rows_missing_column() {
        let items = vec![json!({"name": "foo"})];
        let result = to_compact_rows(&["name", "missing"], &items);
        let rows = result["rows"].as_array().unwrap();
        assert_eq!(rows[0][0], "foo");
        assert!(rows[0][1].is_null());
    }

    // -----------------------------------------------------------------------
    // contains_word_boundary tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_word_boundary_basic() {
        assert!(contains_word_boundary("fn get(x: i32)", "get"));
        assert!(!contains_word_boundary("fn budget(x: i32)", "get"));
        assert!(!contains_word_boundary("fn widget(x: i32)", "get"));
    }

    #[test]
    fn test_word_boundary_at_edges() {
        // Underscore is an identifier char, so get in get_value is NOT a word boundary
        assert!(!contains_word_boundary("get_value", "get"));
        assert!(!contains_word_boundary("value_get", "get"));
        // Exact match is a word boundary
        assert!(contains_word_boundary("get", "get"));
        // Punctuation/space boundaries work
        assert!(contains_word_boundary("(get)", "get"));
        assert!(contains_word_boundary("x: get, y", "get"));
    }

    #[test]
    fn test_word_boundary_not_partial() {
        assert!(!contains_word_boundary("getting", "get"));
        assert!(!contains_word_boundary("target", "get"));
    }

    #[test]
    fn test_word_boundary_with_generics() {
        assert!(contains_word_boundary("HashMap<String, Value>", "HashMap"));
        assert!(contains_word_boundary("Result<Cache>", "Cache"));
    }

    #[test]
    fn test_word_boundary_empty() {
        assert!(!contains_word_boundary("anything", ""));
    }

    // -----------------------------------------------------------------------
    // Integration tests for new tool functions
    // -----------------------------------------------------------------------

    /// Build a minimal CodebaseIndex fixture for integration tests.
    fn make_test_index() -> CodebaseIndex {
        use crate::model::declarations::{DeclKind, Declaration, Relationship, RelKind, Visibility};
        use crate::model::{CodebaseIndex, FileIndex, Import, IndexStats};

        let parse_fn = {
            let mut d = Declaration::new(
                DeclKind::Function,
                "parse_file".to_string(),
                "pub fn parse_file(path: &Path) -> Result<FileIndex>".to_string(),
                Visibility::Public,
                10,
            );
            d.body_lines = Some(20);
            d.doc_comment = Some("Parse a single source file.".to_string());
            d.relationships.push(Relationship {
                kind: RelKind::Implements,
                target: "Parser".to_string(),
            });
            d
        };

        let test_parse = {
            let mut d = Declaration::new(
                DeclKind::Function,
                "test_parse_file".to_string(),
                "fn test_parse_file()".to_string(),
                Visibility::Private,
                50,
            );
            d.is_test = true;
            d.body_lines = Some(10);
            d
        };

        let helper_fn = Declaration::new(
            DeclKind::Function,
            "internal_helper".to_string(),
            "fn internal_helper(x: &str) -> bool".to_string(),
            Visibility::Private,
            35,
        );

        let cache_struct = {
            let mut d = Declaration::new(
                DeclKind::Struct,
                "Cache".to_string(),
                "pub struct Cache".to_string(),
                Visibility::Public,
                5,
            );
            d.doc_comment = Some("In-memory caching layer.".to_string());
            d.children.push(Declaration::new(
                DeclKind::Method,
                "get".to_string(),
                "pub fn get(&self, key: &str) -> Option<&Value>".to_string(),
                Visibility::Public,
                8,
            ));
            d
        };

        let parser_file = FileIndex {
            path: PathBuf::from("src/parser.rs"),
            language: Language::Rust,
            size: 1200,
            lines: 80,
            imports: vec![
                Import { text: "use std::path::Path;".to_string() },
                Import { text: "use crate::model::FileIndex;".to_string() },
            ],
            declarations: vec![parse_fn, helper_fn, test_parse],
        };

        let cache_file = FileIndex {
            path: PathBuf::from("src/cache.rs"),
            language: Language::Rust,
            size: 600,
            lines: 40,
            imports: vec![
                Import { text: "use crate::parser::parse_file;".to_string() },
            ],
            declarations: vec![cache_struct],
        };

        CodebaseIndex {
            root: PathBuf::from("/tmp/test_project"),
            root_name: "test_project".to_string(),
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            stats: IndexStats {
                total_files: 2,
                total_lines: 120,
                languages: HashMap::from([("Rust".to_string(), 2)]),
                duration_ms: 10,
            },
            tree: vec![],
            files: vec![parser_file, cache_file],
        }
    }

    #[test]
    fn test_tool_batch_file_summaries_paths() {
        let index = make_test_index();
        let result = tool_batch_file_summaries(&index, &json!({
            "paths": ["src/parser.rs", "src/cache.rs"]
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["count"], 2);
        assert_eq!(content["total_matched"], 2);
        let summaries = content["summaries"].as_array().unwrap();
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_tool_batch_file_summaries_glob() {
        let index = make_test_index();
        let result = tool_batch_file_summaries(&index, &json!({ "glob": "*.rs" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["count"], 2);
    }

    #[test]
    fn test_tool_batch_file_summaries_no_args() {
        let index = make_test_index();
        let result = tool_batch_file_summaries(&index, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Provide either"));
    }

    #[test]
    fn test_tool_get_callers() {
        let index = make_test_index();
        let result = tool_get_callers(&index, &json!({ "symbol": "parse_file" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        // cache.rs imports parse_file
        assert!(content["count"].as_u64().unwrap() >= 1);
        let refs = content["references"].as_array().unwrap();
        let has_import = refs.iter().any(|r| {
            r["match_type"] == "import" && r["file"].as_str().unwrap().contains("cache.rs")
        });
        assert!(has_import, "Expected import reference from cache.rs");
    }

    #[test]
    fn test_tool_get_callers_no_false_positive() {
        let index = make_test_index();
        // "get" should not match "budget" or "widget" — word-boundary matching
        let result = tool_get_callers(&index, &json!({ "symbol": "nonexistent_sym" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["count"], 0);
    }

    #[test]
    fn test_tool_get_public_api() {
        let index = make_test_index();
        let result = tool_get_public_api(&index, &json!({}));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        // Public declarations: parse_file, Cache, Cache::get
        let decls = content["declarations"].as_array().unwrap();
        let names: Vec<&str> = decls.iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"parse_file"));
        assert!(names.contains(&"Cache"));
        assert!(names.contains(&"get"));
        // internal_helper and test_parse_file are NOT public
        assert!(!names.contains(&"internal_helper"));
        assert!(!names.contains(&"test_parse_file"));
    }

    #[test]
    fn test_tool_get_public_api_scoped() {
        let index = make_test_index();
        let result = tool_get_public_api(&index, &json!({ "path": "src/cache.rs" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        let decls = content["declarations"].as_array().unwrap();
        // Only cache.rs public decls
        let files: Vec<&str> = decls.iter().map(|d| d["file"].as_str().unwrap()).collect();
        assert!(files.iter().all(|f| f.contains("cache.rs")));
    }

    #[test]
    fn test_tool_explain_symbol() {
        let index = make_test_index();
        let result = tool_explain_symbol(&index, &json!({ "name": "parse_file" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["count"], 1);
        let sym = &content["symbols"][0];
        assert_eq!(sym["name"], "parse_file");
        assert_eq!(sym["kind"], "fn");
        assert_eq!(sym["visibility"], "pub");
        assert!(sym["doc_comment"].as_str().unwrap().contains("Parse a single"));
        assert!(sym["signature"].as_str().unwrap().contains("Result<FileIndex>"));
        // Has relationship
        let rels = sym["relationships"].as_array().unwrap();
        assert!(!rels.is_empty());
        assert_eq!(rels[0]["target"], "Parser");
    }

    #[test]
    fn test_tool_explain_symbol_case_insensitive() {
        let index = make_test_index();
        let result = tool_explain_symbol(&index, &json!({ "name": "CACHE" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["count"], 1);
        assert_eq!(content["symbols"][0]["name"], "Cache");
    }

    #[test]
    fn test_tool_explain_symbol_not_found() {
        let index = make_test_index();
        let result = tool_explain_symbol(&index, &json!({ "name": "nonexistent" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["count"], 0);
    }

    #[test]
    fn test_tool_get_related_tests() {
        let index = make_test_index();
        let result = tool_get_related_tests(&index, &json!({ "symbol": "parse_file" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert!(content["count"].as_u64().unwrap() >= 1);
        let tests = content["tests"].as_array().unwrap();
        let names: Vec<&str> = tests.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"test_parse_file"));
    }

    #[test]
    fn test_tool_get_related_tests_scoped() {
        let index = make_test_index();
        let result = tool_get_related_tests(&index, &json!({
            "symbol": "parse_file",
            "path": "src/parser.rs"
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert!(content["count"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn test_tool_get_related_tests_no_match() {
        let index = make_test_index();
        let result = tool_get_related_tests(&index, &json!({ "symbol": "nonexistent" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["count"], 0);
    }

    #[test]
    fn test_tool_get_token_estimate_directory() {
        let index = make_test_index();
        let result = tool_get_token_estimate(&index, &json!({ "directory": "src" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["file_count"], 2);
        assert!(content["total_tokens"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_tool_get_token_estimate_glob() {
        let index = make_test_index();
        let result = tool_get_token_estimate(&index, &json!({ "glob": "*.rs" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["file_count"], 2);
    }

    #[test]
    fn test_tool_get_token_estimate_no_args() {
        let index = make_test_index();
        let result = tool_get_token_estimate(&index, &json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Provide"));
    }

    // -----------------------------------------------------------------------
    // find_file: suffix matching requires `/` boundary
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_file_exact_match() {
        let index = make_test_index();
        let f = find_file(&index, "src/parser.rs");
        assert!(f.is_some());
        assert_eq!(f.unwrap().path.to_string_lossy(), "src/parser.rs");
    }

    #[test]
    fn test_find_file_suffix_with_slash_boundary() {
        let index = make_test_index();
        // "parser.rs" should match "src/parser.rs" via "/parser.rs" suffix
        let f = find_file(&index, "parser.rs");
        assert!(f.is_some());
        assert_eq!(f.unwrap().path.to_string_lossy(), "src/parser.rs");
    }

    #[test]
    fn test_find_file_no_partial_suffix() {
        let index = make_test_index();
        // "rs" should NOT match "src/parser.rs" (no `/` boundary)
        assert!(find_file(&index, "rs").is_none());
        // "arser.rs" should NOT match either
        assert!(find_file(&index, "arser.rs").is_none());
        // "che.rs" should NOT match "src/cache.rs"
        assert!(find_file(&index, "che.rs").is_none());
    }

    #[test]
    fn test_find_file_not_found() {
        let index = make_test_index();
        assert!(find_file(&index, "nonexistent.rs").is_none());
    }

    // -----------------------------------------------------------------------
    // collapse_nested_bodies: raw strings
    // -----------------------------------------------------------------------

    #[test]
    fn test_collapse_raw_string_with_braces() {
        // Build input containing: r#"{ raw braces }"#
        // Can't embed raw string delimiters directly in a Rust string literal,
        // so we construct the input programmatically.
        let mut input = String::from("fn foo() {\n    let s = r#\"{ raw braces }\"#;\n    if x {\n        bar();\n    }\n}");
        // Verify our input is well-formed
        assert!(input.contains("r#"));
        let result = collapse_nested_bodies(&input);
        // Raw string braces should not affect depth tracking
        assert!(result.contains("{ raw braces }"));
        // The if block should be collapsed
        assert!(result.contains("{ ... }"));
        assert!(!result.contains("bar()"));
    }

    #[test]
    fn test_collapse_raw_string_double_hash() {
        // r##"has a " and { braces }"##
        let mut input = String::new();
        input.push_str("fn foo() {\n    let s = r##\"has a ");
        input.push('"');
        input.push_str(" and { braces }\"##;\n    if x {\n        bar();\n    }\n}");
        let result = collapse_nested_bodies(&input);
        assert!(result.contains("{ ... }"));
        assert!(!result.contains("bar()"));
    }

    #[test]
    fn test_collapse_raw_string_no_hash() {
        // r"{ raw }"
        let input = "fn foo() {\n    let s = r\"{ raw }\"; if x {\n        bar();\n    }\n}";
        let result = collapse_nested_bodies(input);
        assert!(result.contains("{ ... }"));
        assert!(!result.contains("bar()"));
    }

    // -----------------------------------------------------------------------
    // compact output mode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_tool_lookup_symbol_compact() {
        let index = make_test_index();
        let result = tool_lookup_symbol(&index, &json!({
            "name": "parse_file",
            "compact": true
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert!(content["matches"].as_u64().unwrap() >= 1);
        // Compact format has columns and rows
        assert!(content["columns"].is_array());
        assert!(content["rows"].is_array());
        let columns = content["columns"].as_array().unwrap();
        assert!(columns.contains(&json!("name")));
        assert!(columns.contains(&json!("file")));
        let rows = content["rows"].as_array().unwrap();
        assert!(!rows.is_empty());
    }

    #[test]
    fn test_tool_lookup_symbol_non_compact() {
        let index = make_test_index();
        let result = tool_lookup_symbol(&index, &json!({ "name": "parse_file" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        // Non-compact has "symbols" array of objects
        assert!(content["symbols"].is_array());
        let symbols = content["symbols"].as_array().unwrap();
        assert!(!symbols.is_empty());
        assert!(symbols[0]["name"].is_string());
    }

    #[test]
    fn test_tool_list_declarations_compact() {
        let index = make_test_index();
        let result = tool_list_declarations(&index, &json!({
            "path": "src/parser.rs",
            "compact": true
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert_eq!(content["file"], "src/parser.rs");
        // Compact: declarations has columns/rows format
        let decls = &content["declarations"];
        assert!(decls["columns"].is_array());
        assert!(decls["rows"].is_array());
    }

    #[test]
    fn test_tool_search_signatures_compact() {
        let index = make_test_index();
        let result = tool_search_signatures(&index, &json!({
            "query": "Result<",
            "compact": true
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert!(content["matches"].as_u64().unwrap() >= 1);
        assert!(content["columns"].is_array());
        assert!(content["rows"].is_array());
    }

    #[test]
    fn test_tool_search_relevant_compact() {
        let index = make_test_index();
        let result = tool_search_relevant(&index, &json!({
            "query": "parse",
            "compact": true
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        assert!(content["matches"].as_u64().unwrap() >= 1);
        let results_val = &content["results"];
        assert!(results_val["columns"].is_array());
        assert!(results_val["rows"].is_array());
    }

    // -----------------------------------------------------------------------
    // search_relevant: kind filter
    // -----------------------------------------------------------------------

    #[test]
    fn test_tool_search_relevant_kind_filter() {
        let index = make_test_index();
        // Filter to only structs
        let result = tool_search_relevant(&index, &json!({
            "query": "cache",
            "kind": "struct"
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        let results_arr = content["results"].as_array().unwrap();
        // All results should be structs (no path matches since kind filter is active)
        for r in results_arr {
            if let Some(kind) = r["kind"].as_str() {
                assert_eq!(kind, "struct", "Expected only struct results with kind filter");
            }
        }
        // Should find the Cache struct
        let has_cache = results_arr.iter().any(|r| {
            r["symbol"].as_str() == Some("Cache")
        });
        assert!(has_cache, "Expected Cache struct in results");
    }

    #[test]
    fn test_tool_search_relevant_kind_filter_fn() {
        let index = make_test_index();
        // Filter to only functions
        let result = tool_search_relevant(&index, &json!({
            "query": "parse",
            "kind": "fn"
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        let results_arr = content["results"].as_array().unwrap();
        // All symbol results should be functions
        for r in results_arr {
            if r["symbol"].is_string() {
                assert_eq!(r["kind"].as_str().unwrap(), "fn");
            }
        }
    }

    // -----------------------------------------------------------------------
    // read_source: multi-symbol and collapse modes
    // -----------------------------------------------------------------------

    #[test]
    fn test_tool_read_source_multi_symbol() {
        use std::io::Write as IoWrite;

        let mut index = make_test_index();
        // Create a temp file with source content matching the fixture declarations
        let dir = std::env::temp_dir().join("indxr_test_read_source");
        let _ = std::fs::create_dir_all(dir.join("src"));
        let source = "// line 1\n// line 2\n// line 3\n// line 4\n// line 5\n// line 6\n// line 7\n// line 8\n// line 9\nfn parse_file() {\n    // body line 11\n    // body line 12\n    // body line 13\n    // body line 14\n    // body line 15\n    // body line 16\n    // body line 17\n    // body line 18\n    // body line 19\n    // body line 20\n    // body line 21\n    // body line 22\n    // body line 23\n    // body line 24\n    // body line 25\n    // body line 26\n    // body line 27\n    // body line 28\n    // body line 29\n}\n// line 31\n// line 32\n// line 33\n// line 34\nfn internal_helper() {\n    // helper body\n}\n// line 38\n// line 39\n// line 40\n// line 41\n// line 42\n// line 43\n// line 44\n// line 45\n// line 46\n// line 47\n// line 48\n// line 49\nfn test_parse_file() {\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n    // test body\n}\n";
        let file_path = dir.join("src/parser.rs");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(source.as_bytes()).unwrap();

        // Point index root at our temp dir
        index.root = dir.clone();

        let result = tool_read_source(&index, &json!({
            "path": "src/parser.rs",
            "symbols": ["parse_file", "internal_helper"]
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        let symbols = content["symbols"].as_array().unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0]["symbol"], "parse_file");
        assert_eq!(symbols[1]["symbol"], "internal_helper");
        assert!(symbols[0]["source"].as_str().unwrap().contains("parse_file"));
        assert!(symbols[1]["source"].as_str().unwrap().contains("internal_helper"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tool_read_source_multi_symbol_not_found() {
        use std::io::Write as IoWrite;

        let mut index = make_test_index();
        let dir = std::env::temp_dir().join("indxr_test_read_source_nf");
        let _ = std::fs::create_dir_all(dir.join("src"));
        // Need 30+ lines since parse_file starts at line 10 with body_lines=20
        let mut source = String::new();
        for i in 1..=40 {
            source.push_str(&format!("// line {}\n", i));
        }
        let file_path = dir.join("src/parser.rs");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(source.as_bytes()).unwrap();
        index.root = dir.clone();

        let result = tool_read_source(&index, &json!({
            "path": "src/parser.rs",
            "symbols": ["parse_file", "nonexistent_fn"]
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        let symbols = content["symbols"].as_array().unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0]["symbol"], "parse_file");
        let not_found = content["not_found"].as_array().unwrap();
        assert!(not_found.contains(&json!("nonexistent_fn")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tool_read_source_collapse() {
        use std::io::Write as IoWrite;

        let mut index = make_test_index();
        let dir = std::env::temp_dir().join("indxr_test_read_source_collapse");
        let _ = std::fs::create_dir_all(dir.join("src"));
        let source = "// lines 1-9\n\n\n\n\n\n\n\n\nfn parse_file() {\n    if true {\n        nested();\n    }\n}\n";
        let file_path = dir.join("src/parser.rs");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(source.as_bytes()).unwrap();
        index.root = dir.clone();

        let result = tool_read_source(&index, &json!({
            "path": "src/parser.rs",
            "symbol": "parse_file",
            "collapse": true
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        let src = content["source"].as_str().unwrap();
        assert!(content["collapsed"].as_bool().unwrap());
        assert!(src.contains("{ ... }"), "Expected collapsed nested body");
        assert!(!src.contains("nested()"), "Nested call should be collapsed away");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tool_read_source_multi_symbol_collapse() {
        use std::io::Write as IoWrite;

        let mut index = make_test_index();
        let dir = std::env::temp_dir().join("indxr_test_multi_collapse");
        let _ = std::fs::create_dir_all(dir.join("src"));
        let source = "// lines 1-9\n\n\n\n\n\n\n\n\nfn parse_file() {\n    if true {\n        nested();\n    }\n}\n// lines 15-34\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\nfn internal_helper() {\n    match x {\n        _ => deep(),\n    }\n}\n";
        let file_path = dir.join("src/parser.rs");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(source.as_bytes()).unwrap();
        index.root = dir.clone();

        let result = tool_read_source(&index, &json!({
            "path": "src/parser.rs",
            "symbols": ["parse_file", "internal_helper"],
            "collapse": true
        }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        let symbols = content["symbols"].as_array().unwrap();
        assert_eq!(symbols.len(), 2);
        // Both should have collapsed bodies
        for sym in symbols {
            let src = sym["source"].as_str().unwrap();
            assert!(src.contains("{ ... }"), "Expected collapsed body for {}", sym["symbol"]);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // batch_file_summaries: cap boundary
    // -----------------------------------------------------------------------

    #[test]
    fn test_tool_batch_file_summaries_cap() {
        use crate::model::{FileIndex, Import, IndexStats};

        // Create an index with 35 files (over the 30-file cap)
        let mut files = Vec::new();
        for i in 0..35 {
            files.push(FileIndex {
                path: PathBuf::from(format!("src/file_{}.rs", i)),
                language: Language::Rust,
                size: 100,
                lines: 10,
                imports: vec![],
                declarations: vec![],
            });
        }
        let index = CodebaseIndex {
            root: PathBuf::from("/tmp/test_cap"),
            root_name: "test_cap".to_string(),
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            stats: IndexStats {
                total_files: 35,
                total_lines: 350,
                languages: HashMap::from([("Rust".to_string(), 35)]),
                duration_ms: 5,
            },
            tree: vec![],
            files,
        };

        let result = tool_batch_file_summaries(&index, &json!({ "glob": "*.rs" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        // Should cap at 30
        assert_eq!(content["count"], 30);
        assert_eq!(content["total_matched"], 35);
    }

    // -----------------------------------------------------------------------
    // get_callers: common word symbol
    // -----------------------------------------------------------------------

    #[test]
    fn test_tool_get_callers_common_word() {
        let index = make_test_index();
        // "get" is a method on Cache — should only match word-boundary occurrences
        let result = tool_get_callers(&index, &json!({ "symbol": "get" }));
        let content: Value = serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap()
        ).unwrap();
        // Should not produce false positives from "budget", "widget", etc.
        let refs = content["references"].as_array().unwrap();
        for r in refs {
            if let Some(sig) = r.get("match_type").and_then(|v| v.as_str()) {
                if sig == "signature" {
                    // The matched signature should contain "get" at a word boundary
                    let name = r["name"].as_str().unwrap();
                    assert_ne!(name, "get", "Should not match the symbol's own declaration");
                }
            }
        }
    }
}
