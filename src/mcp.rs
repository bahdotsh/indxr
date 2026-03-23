use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::{self, json, Value};

use crate::model::declarations::{DeclKind, Declaration};
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
        _ => tool_error(&format!("Unknown tool: {}", name)),
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

fn handle_tools_call(id: Value, index: &CodebaseIndex, params: &Value) -> JsonRpcResponse {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return err_response(id, -32602, "Missing tool name in params".into());
        }
    };

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    let result = handle_tool_call(index, tool_name, &arguments);
    ok_response(id, result)
}

// ---------------------------------------------------------------------------
// Main server loop
// ---------------------------------------------------------------------------

pub fn run_mcp_server(index: CodebaseIndex) -> anyhow::Result<()> {
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
            "tools/call" => handle_tools_call(id, &index, &params),
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
