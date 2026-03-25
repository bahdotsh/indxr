mod helpers;
mod tools;

#[cfg(test)]
mod tests;

use std::io::{self, BufRead, Write};

use serde::Deserialize;
use serde::Serialize;
use serde_json::{self, Value, json};

use crate::indexer::IndexConfig;
use crate::model::CodebaseIndex;
use crate::parser::ParserRegistry;

use self::tools::{handle_tool_call, tool_definitions, tool_get_diff_summary, tool_regenerate_index};

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
// Response helpers
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
