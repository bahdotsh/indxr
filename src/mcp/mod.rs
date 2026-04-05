mod helpers;
#[cfg(feature = "http")]
pub mod http;
mod tools;
mod type_flow;

#[cfg(test)]
mod tests;

use std::io::{self, BufRead, Write};
use std::sync::mpsc;
use std::thread;

use serde::Deserialize;
use serde::Serialize;
use serde_json::{self, Value, json};

use crate::indexer::{self, WorkspaceConfig};
use crate::model::WorkspaceIndex;
use crate::parser::ParserRegistry;

use self::tools::{
    handle_tool_call, tool_definitions, tool_get_diff_summary, tool_regenerate_index,
};

#[cfg(feature = "wiki")]
use self::tools::{
    tool_wiki_contribute, tool_wiki_generate, tool_wiki_read, tool_wiki_search, tool_wiki_status,
    tool_wiki_update,
};

/// Wiki store state, conditionally compiled.
#[cfg(feature = "wiki")]
pub(crate) type WikiStoreOption = Option<crate::wiki::store::WikiStore>;

/// Placeholder when wiki feature is disabled.
#[cfg(not(feature = "wiki"))]
pub(crate) type WikiStoreOption = ();

/// Reload the wiki store from disk (e.g. after regenerate_index or file changes).
#[cfg(feature = "wiki")]
fn reload_wiki_store(root: &std::path::Path) -> WikiStoreOption {
    let wiki_dir = root.join(".indxr").join("wiki");
    if wiki_dir.exists() {
        match crate::wiki::store::WikiStore::load(&wiki_dir) {
            Ok(store) => {
                eprintln!("Wiki reloaded: {} pages", store.pages.len());
                Some(store)
            }
            Err(e) => {
                eprintln!("Warning: failed to reload wiki: {}", e);
                None
            }
        }
    } else {
        None
    }
}

#[cfg(not(feature = "wiki"))]
fn reload_wiki_store(_root: &std::path::Path) -> WikiStoreOption {}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    pub(crate) id: Option<Value>,
    pub(crate) method: String,
    pub(crate) params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcError {
    code: i32,
    message: String,
}

// ---------------------------------------------------------------------------
// Transport type (used to vary protocol version in initialize)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(crate) enum Transport {
    Stdio,
    #[cfg(feature = "http")]
    Http,
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

pub(crate) fn ok_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

pub(crate) fn err_response(id: Value, code: i32, message: String) -> JsonRpcResponse {
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

pub(crate) fn handle_initialize(id: Value, transport: Transport) -> JsonRpcResponse {
    let protocol_version = match transport {
        Transport::Stdio => "2024-11-05",
        #[cfg(feature = "http")]
        Transport::Http => "2025-03-26",
    };
    ok_response(
        id,
        json!({
            "protocolVersion": protocol_version,
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

pub(crate) fn handle_tools_list(
    id: Value,
    workspace: &WorkspaceIndex,
    all_tools: bool,
    wiki_store: &WikiStoreOption,
) -> JsonRpcResponse {
    let is_workspace = workspace.members.len() > 1;
    #[cfg(feature = "wiki")]
    let wiki_available = wiki_store.is_some();
    #[cfg(not(feature = "wiki"))]
    let wiki_available = false;
    let _ = wiki_store; // suppress unused warning when wiki feature is off
    ok_response(
        id,
        tool_definitions(is_workspace, all_tools, wiki_available),
    )
}

pub(crate) fn handle_tools_call(
    id: Value,
    workspace: &mut WorkspaceIndex,
    config: &WorkspaceConfig,
    registry: &ParserRegistry,
    params: &Value,
    wiki_store: &mut WikiStoreOption,
) -> JsonRpcResponse {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return err_response(id, -32602, "Missing tool name in params".into());
        }
    };

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    if tool_name == "regenerate_index" {
        let result = tool_regenerate_index(workspace, config);
        *wiki_store = reload_wiki_store(&workspace.root);
        return ok_response(id, result);
    }

    if tool_name == "get_diff_summary" {
        let result = tool_get_diff_summary(workspace, config, registry, &arguments);
        return ok_response(id, result);
    }

    // Wiki tools — early dispatch with wiki store
    #[cfg(feature = "wiki")]
    {
        use self::helpers::tool_error;

        // wiki_generate creates the wiki from scratch — doesn't need an existing store
        if tool_name == "wiki_generate" {
            let result = tool_wiki_generate(workspace, &arguments);
            *wiki_store = reload_wiki_store(&workspace.root);
            return ok_response(id, result);
        }

        // wiki_update reads store to find affected pages (no mutation)
        if tool_name == "wiki_update" {
            return match wiki_store.as_ref() {
                Some(store) => {
                    let result = tool_wiki_update(store, workspace, registry, &arguments);
                    ok_response(id, result)
                }
                None => ok_response(
                    id,
                    tool_error("No wiki found. Run `wiki_generate` to create one first."),
                ),
            };
        }

        // wiki_contribute needs &mut store
        if tool_name == "wiki_contribute" {
            return match wiki_store.as_mut() {
                Some(store) => ok_response(id, tool_wiki_contribute(store, &arguments)),
                None => ok_response(
                    id,
                    tool_error("No wiki found. Run `wiki_generate` to create one first."),
                ),
            };
        }

        // Read-only wiki tools
        if matches!(tool_name, "wiki_search" | "wiki_read" | "wiki_status") {
            return match wiki_store.as_ref() {
                Some(store) => {
                    let result = match tool_name {
                        "wiki_search" => tool_wiki_search(store, &arguments),
                        "wiki_read" => tool_wiki_read(store, &arguments),
                        "wiki_status" => tool_wiki_status(store, workspace),
                        _ => unreachable!(),
                    };
                    ok_response(id, result)
                }
                None => ok_response(
                    id,
                    tool_error("No wiki found. Run `wiki_generate` to create one first."),
                ),
            };
        }
    }
    let _ = &wiki_store; // suppress unused warning when wiki feature is off

    let result = handle_tool_call(workspace, tool_name, &arguments);
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
// Transport-agnostic JSON-RPC handler
// ---------------------------------------------------------------------------

/// Dispatch a pre-parsed JSON-RPC request.
///
/// Returns `None` for notifications (no id), `Some(response)` otherwise.
pub(crate) fn process_jsonrpc_request(
    request: JsonRpcRequest,
    workspace: &mut WorkspaceIndex,
    config: &WorkspaceConfig,
    registry: &ParserRegistry,
    transport: Transport,
    all_tools: bool,
    wiki_store: &mut WikiStoreOption,
) -> Option<JsonRpcResponse> {
    let id = request.id?;
    let params = request.params.unwrap_or(json!({}));

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(id, transport),
        "tools/list" => handle_tools_list(id, workspace, all_tools, wiki_store),
        "tools/call" => handle_tools_call(id, workspace, config, registry, &params, wiki_store),
        _ => err_response(id, -32601, format!("Method not found: {}", request.method)),
    };

    Some(response)
}

/// Process a single JSON-RPC message string, returning the response.
///
/// Returns:
/// - `Ok(Some(response))` for requests that need a response
/// - `Ok(None)` for notifications (no id) or empty input
/// - `Err(response)` for parse errors (caller should still send the error response)
pub(crate) fn process_jsonrpc_message(
    line: &str,
    workspace: &mut WorkspaceIndex,
    config: &WorkspaceConfig,
    registry: &ParserRegistry,
    transport: Transport,
    all_tools: bool,
    wiki_store: &mut WikiStoreOption,
) -> Result<Option<JsonRpcResponse>, JsonRpcResponse> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let request: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Err(err_response(
                Value::Null,
                -32700,
                format!("Parse error: {}", e),
            ));
        }
    };

    Ok(process_jsonrpc_request(
        request, workspace, config, registry, transport, all_tools, wiki_store,
    ))
}

// ---------------------------------------------------------------------------
// Stdin line handler (uses process_jsonrpc_message, writes to stdout)
// ---------------------------------------------------------------------------

fn handle_stdin_line(
    line: &str,
    workspace: &mut WorkspaceIndex,
    config: &WorkspaceConfig,
    registry: &ParserRegistry,
    writer: &mut impl Write,
    all_tools: bool,
    wiki_store: &mut WikiStoreOption,
) -> anyhow::Result<()> {
    eprintln!("< {}", line);

    let response = match process_jsonrpc_message(
        line,
        workspace,
        config,
        registry,
        Transport::Stdio,
        all_tools,
        wiki_store,
    ) {
        Ok(Some(resp)) => resp,
        Ok(None) => {
            if !line.trim().is_empty() {
                eprintln!("Notification (no response)");
            }
            return Ok(());
        }
        Err(resp) => {
            eprintln!("Failed to parse JSON-RPC request");
            resp
        }
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
    mut workspace: WorkspaceIndex,
    config: WorkspaceConfig,
    watch: bool,
    debounce_ms: u64,
    all_tools: bool,
) -> anyhow::Result<()> {
    eprintln!(
        "indxr MCP server starting (root: {})",
        workspace.root.display()
    );
    let registry = ParserRegistry::new();

    // Load wiki store if available
    #[cfg(feature = "wiki")]
    let mut wiki_store: WikiStoreOption = {
        let wiki_dir = workspace.root.join(".indxr").join("wiki");
        if wiki_dir.exists() {
            match crate::wiki::store::WikiStore::load(&wiki_dir) {
                Ok(store) => {
                    eprintln!("Wiki loaded: {} pages", store.pages.len());
                    Some(store)
                }
                Err(e) => {
                    eprintln!("Warning: failed to load wiki: {}", e);
                    None
                }
            }
        } else {
            None
        }
    };
    #[cfg(not(feature = "wiki"))]
    let mut wiki_store: WikiStoreOption = ();

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
        let root = std::fs::canonicalize(&workspace.root)?;
        let output_path = root.join("INDEX.md");
        let cache_dir = std::fs::canonicalize(root.join(&config.template.cache_dir))
            .unwrap_or_else(|_| root.join(&config.template.cache_dir));
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
                match indexer::regenerate_workspace_index(&config) {
                    Ok(new_ws) => {
                        eprintln!("Auto-reindex complete ({} files)", new_ws.stats.total_files);
                        workspace = new_ws;
                        wiki_store = reload_wiki_store(&workspace.root);
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
                            handle_stdin_line(
                                &line,
                                &mut workspace,
                                &config,
                                &registry,
                                &mut writer,
                                all_tools,
                                &mut wiki_store,
                            )?;
                        }
                        ServerEvent::FileChanged => unreachable!(),
                    }
                }
            }
            ServerEvent::StdinLine(line) => {
                handle_stdin_line(
                    &line,
                    &mut workspace,
                    &config,
                    &registry,
                    &mut writer,
                    all_tools,
                    &mut wiki_store,
                )?;
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
