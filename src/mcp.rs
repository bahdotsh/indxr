use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{self, Value, json};

use crate::budget::estimate_tokens;
use crate::indexer::{self, IndexConfig};
use crate::model::declarations::{DeclKind, Declaration, Visibility};
use crate::model::{CodebaseIndex, FileIndex};

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
                        }
                    },
                    "required": ["path"]
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
                        }
                    },
                    "required": ["query"]
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
        _ => tool_error(&format!("Unknown tool: {}", name)),
    }
}

fn tool_regenerate_index(index: &mut CodebaseIndex, config: &IndexConfig) -> Value {
    match indexer::regenerate_index_file(config) {
        Ok(new_index) => {
            let file_count = new_index.stats.total_files;
            let line_count = new_index.stats.total_lines;
            let output_path = new_index.root.join("INDEX.md");
            *index = new_index;
            tool_result(json!({
                "status": "ok",
                "message": format!(
                    "INDEX.md regenerated ({} files, {} lines)",
                    file_count, line_count
                ),
                "path": output_path.to_string_lossy(),
                "files_indexed": file_count,
                "total_lines": line_count
            }))
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
    let mut result = json!({
        "matches": total,
        "symbols": results
    });
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

    let kind_filter = args
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(DeclKind::from_name);

    if shallow {
        // Shallow mode: return compact representation without children/doc_comments
        let decls: Vec<ShallowDeclaration> = if let Some(ref kind) = kind_filter {
            filter_declarations(&file.declarations, kind)
                .into_iter()
                .map(to_shallow)
                .collect()
        } else {
            file.declarations.iter().map(to_shallow).collect()
        };
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
    let mut result = json!({
        "matches": total,
        "signatures": results
    });
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

    let symbol_name = args.get("symbol").and_then(|v| v.as_str());
    let start_line = args.get("start_line").and_then(|v| v.as_u64());
    let end_line = args.get("end_line").and_then(|v| v.as_u64());

    let (start, end, symbol_info) = if let Some(sym) = symbol_name {
        // Symbol mode: look up the declaration
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
        return tool_error("Provide either 'symbol' or both 'start_line' and 'end_line'");
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
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_error("Missing required parameter: path"),
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

    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
    let mut results: Vec<RelevanceMatch> = Vec::new();

    for file in &index.files {
        let file_path = file.path.to_string_lossy().to_string();
        let file_path_lower = file_path.to_lowercase();

        // Score file path matches
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

        // Score declaration matches
        score_decls_recursive(
            &file.declarations,
            &file_path,
            &query_lower,
            &query_terms,
            &mut results,
        );
    }

    // Sort by score descending
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(limit);

    let total = results.len();
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

    score
}

fn score_decls_recursive(
    decls: &[Declaration],
    file_path: &str,
    query: &str,
    terms: &[&str],
    results: &mut Vec<RelevanceMatch>,
) {
    for decl in decls {
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

        score_decls_recursive(&decl.children, file_path, query, terms, results);
    }
}

/// Find a FileIndex whose path matches the given string. Supports both exact
/// match and suffix match so callers can use relative paths.
fn find_file<'a>(index: &'a CodebaseIndex, path: &str) -> Option<&'a FileIndex> {
    index.files.iter().find(|f| {
        let file_path = f.path.to_string_lossy();
        file_path == path || file_path.ends_with(path)
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

    let result = handle_tool_call(index, tool_name, &arguments);
    ok_response(id, result)
}

// ---------------------------------------------------------------------------
// Main server loop
// ---------------------------------------------------------------------------

pub fn run_mcp_server(mut index: CodebaseIndex, config: IndexConfig) -> anyhow::Result<()> {
    eprintln!("indxr MCP server starting (root: {})", index.root.display());

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
            "tools/call" => handle_tools_call(id, &mut index, &config, &params),
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
        // exact substring (10) + exact equality (20) + term match (5) = 35
        assert_eq!(score, 35);
    }

    #[test]
    fn test_score_match_substring() {
        let score = score_match("tree_sitter_parser", "parser", &["parser"]);
        // substring (10) + term match (5) = 15
        assert_eq!(score, 15);
    }

    #[test]
    fn test_score_match_no_match() {
        let score = score_match("indexer", "parser", &["parser"]);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_score_match_multi_term() {
        // "token budget" (with space) is NOT a substring of "token_budget_manager"
        // Only individual terms match: "token" (5) + "budget" (5) = 10
        let score = score_match("token_budget_manager", "token budget", &["token", "budget"]);
        assert_eq!(score, 10);

        // When full query IS a substring, it scores higher
        let score2 = score_match("apply token budget here", "token budget", &["token", "budget"]);
        // full query substring (10) + term "token" (5) + term "budget" (5) = 20
        assert_eq!(score2, 20);
    }

    #[test]
    fn test_score_match_partial_term_match() {
        // Only one of two terms matches
        let score = score_match("token_counter", "token budget", &["token", "budget"]);
        // full query "token budget" not in text (0) + term "token" (5) + term "budget" misses (0) = 5
        assert_eq!(score, 5);
    }

    #[test]
    fn test_score_match_empty_query() {
        // Empty string is a substring of everything
        let score = score_match("anything", "", &[""]);
        // substring (10) + term "" is substring (5) = 15
        // This is fine — search_relevant won't produce empty queries in practice
        assert!(score > 0);
    }

    #[test]
    fn test_score_match_case_sensitivity() {
        // score_match expects pre-lowercased inputs (caller responsibility)
        let score = score_match("parser", "Parser", &["Parser"]);
        assert_eq!(score, 0); // case-sensitive — caller must lowercase
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
        // Verify total count matches dispatch (12 tools)
        assert_eq!(names.len(), 12);
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
}
