mod helpers;
mod tools;

#[cfg(test)]
mod tests;

use std::io::{self, BufRead, Write};
use std::sync::mpsc;
use std::thread;

use serde::Deserialize;
use serde::Serialize;
use serde_json::{self, Value, json};

use crate::indexer::{self, IndexConfig};
use crate::model::CodebaseIndex;
use crate::parser::ParserRegistry;

use self::tools::{
    handle_tool_call, tool_definitions, tool_get_diff_summary, tool_regenerate_index,
};

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
// Server event types for the channel-based event loop
// ---------------------------------------------------------------------------

enum ServerEvent {
    StdinLine(String),
    StdinClosed,
    FileChanged,
}

// ---------------------------------------------------------------------------
// Stdin line handler (extracted so it can be called from deferred-event replay)
// ---------------------------------------------------------------------------

fn handle_stdin_line(
    line: &str,
    index: &mut CodebaseIndex,
    config: &IndexConfig,
    registry: &ParserRegistry,
    writer: &mut impl Write,
) -> anyhow::Result<()> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    eprintln!("< {}", line);

    let request: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to parse JSON-RPC request: {}", e);
            let resp = err_response(Value::Null, -32700, format!("Parse error: {}", e));
            let out = serde_json::to_string(&resp)?;
            eprintln!("> {}", out);
            writeln!(writer, "{}", out)?;
            writer.flush()?;
            return Ok(());
        }
    };

    // Notifications have no id and require no response.
    if request.id.is_none() {
        eprintln!("Notification: {}", request.method);
        return Ok(());
    }

    let id = request.id.unwrap();
    let params = request.params.unwrap_or(json!({}));

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(id),
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, index, config, registry, &params),
        _ => err_response(id, -32601, format!("Method not found: {}", request.method)),
    };

    let out = serde_json::to_string(&response)?;
    eprintln!("> {}", out);
    writeln!(writer, "{}", out)?;
    writer.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Main server loop
// ---------------------------------------------------------------------------

pub fn run_mcp_server(
    mut index: CodebaseIndex,
    config: IndexConfig,
    watch: bool,
    debounce_ms: u64,
) -> anyhow::Result<()> {
    eprintln!("indxr MCP server starting (root: {})", index.root.display());
    let registry = ParserRegistry::new();

    let (tx, rx) = mpsc::channel::<ServerEvent>();

    // Spawn stdin reader thread
    let stdin_tx = tx.clone();
    thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if stdin_tx.send(ServerEvent::StdinLine(l)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Error reading stdin: {}", e);
                    break;
                }
            }
        }
        let _ = stdin_tx.send(ServerEvent::StdinClosed);
    });

    // Optionally spawn file watcher — the guard must outlive the event loop
    // so the OS-level file subscription stays active.
    let mut _watch_guard: Option<crate::watch::WatchGuard> = None;
    if watch {
        let root = std::fs::canonicalize(&config.root)?;
        let output_path = root.join("INDEX.md");
        let cache_dir = std::fs::canonicalize(root.join(&config.cache_dir))
            .unwrap_or_else(|_| root.join(&config.cache_dir));
        let (watch_rx, guard) =
            crate::watch::spawn_watcher(&root, &cache_dir, &output_path, debounce_ms)?;
        _watch_guard = Some(guard);

        let watch_tx = tx.clone();
        thread::spawn(move || {
            while watch_rx.recv().is_ok() {
                if watch_tx.send(ServerEvent::FileChanged).is_err() {
                    break;
                }
            }
        });

        eprintln!("File watcher enabled (debounce: {}ms)", debounce_ms);
    }

    // Drop the original sender so the channel naturally closes when all
    // thread-owned senders are dropped (stdin_tx, watch_tx).
    drop(tx);

    let stdout = io::stdout();
    let mut writer = stdout.lock();

    while let Ok(event) = rx.recv() {
        match event {
            ServerEvent::StdinClosed => break,
            ServerEvent::FileChanged => {
                // Coalesce: drain ALL queued events, discarding FileChanged
                // duplicates so we re-index only once per burst. Non-FileChanged
                // events are preserved and replayed after the reindex.
                let mut deferred = Vec::new();
                while let Ok(evt) = rx.try_recv() {
                    match evt {
                        ServerEvent::FileChanged => {}
                        other => deferred.push(other),
                    }
                }

                eprintln!("File change detected, auto-reindexing...");
                match indexer::regenerate_index_file(&config) {
                    Ok(new_index) => {
                        eprintln!("Auto-reindex complete ({} files)", new_index.files.len());
                        index = new_index;
                    }
                    Err(e) => {
                        eprintln!("Auto-reindex failed: {}", e);
                    }
                }

                // Re-process any non-FileChanged events that were drained
                for evt in deferred {
                    match evt {
                        ServerEvent::StdinClosed => {
                            eprintln!("indxr MCP server shutting down");
                            return Ok(());
                        }
                        ServerEvent::StdinLine(line) => {
                            handle_stdin_line(&line, &mut index, &config, &registry, &mut writer)?;
                        }
                        ServerEvent::FileChanged => unreachable!(),
                    }
                }
            }
            ServerEvent::StdinLine(line) => {
                handle_stdin_line(&line, &mut index, &config, &registry, &mut writer)?;
            }
        }
    }

    eprintln!("indxr MCP server shutting down");
    Ok(())
}

#[cfg(test)]
mod coalesce_tests {
    use super::*;
    use std::sync::mpsc;

    /// Reproduces the scenario where a StdinLine arrives between FileChanged
    /// events. Greedy coalescing drains all queued events, so we get a single
    /// reindex with all non-FileChanged events preserved and replayed after.
    #[test]
    fn coalesce_preserves_stdin_events() {
        let (tx, rx) = mpsc::channel::<ServerEvent>();

        // Simulate: FileChanged, FileChanged, StdinLine, FileChanged
        tx.send(ServerEvent::FileChanged).unwrap();
        tx.send(ServerEvent::FileChanged).unwrap();
        tx.send(ServerEvent::StdinLine("hello".into())).unwrap();
        tx.send(ServerEvent::FileChanged).unwrap();
        tx.send(ServerEvent::StdinClosed).unwrap();
        drop(tx);

        let mut collected = Vec::new();
        while let Ok(event) = rx.recv() {
            match event {
                ServerEvent::FileChanged => {
                    let mut deferred = Vec::new();
                    while let Ok(evt) = rx.try_recv() {
                        match evt {
                            ServerEvent::FileChanged => {}
                            other => deferred.push(other),
                        }
                    }
                    collected.push("reindex".to_string());
                    for evt in deferred {
                        match evt {
                            ServerEvent::StdinClosed => collected.push("closed".into()),
                            ServerEvent::StdinLine(l) => collected.push(format!("stdin:{l}")),
                            ServerEvent::FileChanged => unreachable!(),
                        }
                    }
                }
                ServerEvent::StdinLine(l) => collected.push(format!("stdin:{l}")),
                ServerEvent::StdinClosed => collected.push("closed".into()),
            }
        }

        // The critical invariant: StdinLine must not be lost during coalescing
        assert!(
            collected.contains(&"stdin:hello".to_string()),
            "StdinLine must not be lost during coalescing. Got: {:?}",
            collected
        );
        // Greedy coalescing: all FileChanged events collapse into a single reindex
        // Expect: ["reindex", "stdin:hello", "closed"]
        assert_eq!(
            collected.iter().filter(|e| *e == "reindex").count(),
            1,
            "Expect 1 reindex: greedy coalescing collapses all FileChanged events. Got: {:?}",
            collected
        );
    }

    /// StdinClosed during coalescing must also be preserved.
    #[test]
    fn coalesce_preserves_stdin_closed() {
        let (tx, rx) = mpsc::channel::<ServerEvent>();

        tx.send(ServerEvent::FileChanged).unwrap();
        tx.send(ServerEvent::FileChanged).unwrap();
        tx.send(ServerEvent::StdinClosed).unwrap();
        drop(tx);

        let mut saw_closed = false;
        while let Ok(event) = rx.recv() {
            match event {
                ServerEvent::FileChanged => {
                    let mut deferred = Vec::new();
                    while let Ok(evt) = rx.try_recv() {
                        match evt {
                            ServerEvent::FileChanged => {}
                            other => deferred.push(other),
                        }
                    }
                    for evt in deferred {
                        if matches!(evt, ServerEvent::StdinClosed) {
                            saw_closed = true;
                        }
                    }
                }
                ServerEvent::StdinClosed => saw_closed = true,
                _ => {}
            }
        }

        assert!(saw_closed, "StdinClosed must not be lost during coalescing");
    }
}
