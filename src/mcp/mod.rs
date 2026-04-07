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
    tool_wiki_compound, tool_wiki_contribute, tool_wiki_generate, tool_wiki_read,
    tool_wiki_record_failure, tool_wiki_search, tool_wiki_status, tool_wiki_suggest_contribution,
    tool_wiki_update,
};

/// Wiki store state, conditionally compiled.
#[cfg(feature = "wiki")]
pub(crate) type WikiStoreOption = Option<crate::wiki::store::WikiStore>;

/// Placeholder when wiki feature is disabled.
#[cfg(not(feature = "wiki"))]
pub(crate) type WikiStoreOption = ();

/// Configuration for the MCP server.
pub struct McpServerConfig {
    pub watch: bool,
    pub debounce_ms: u64,
    pub all_tools: bool,
    #[cfg(feature = "wiki")]
    pub wiki_auto_update: bool,
    #[cfg(feature = "wiki")]
    pub wiki_debounce_ms: u64,
    #[cfg(feature = "wiki")]
    pub wiki_model: Option<String>,
    #[cfg(feature = "wiki")]
    pub wiki_exec: Option<String>,
}

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

        // wiki_compound needs &mut store
        if tool_name == "wiki_compound" {
            return match wiki_store.as_mut() {
                Some(store) => ok_response(id, tool_wiki_compound(store, &arguments)),
                None => ok_response(
                    id,
                    tool_error("No wiki found. Run `wiki_generate` to create one first."),
                ),
            };
        }

        // wiki_record_failure needs &mut store
        if tool_name == "wiki_record_failure" {
            return match wiki_store.as_mut() {
                Some(store) => ok_response(id, tool_wiki_record_failure(store, &arguments)),
                None => ok_response(
                    id,
                    tool_error("No wiki found. Run `wiki_generate` to create one first."),
                ),
            };
        }

        // Read-only wiki tools
        if matches!(
            tool_name,
            "wiki_search" | "wiki_read" | "wiki_status" | "wiki_suggest_contribution"
        ) {
            return match wiki_store.as_ref() {
                Some(store) => {
                    let result = match tool_name {
                        "wiki_search" => tool_wiki_search(store, &arguments),
                        "wiki_read" => tool_wiki_read(store, &arguments),
                        "wiki_status" => tool_wiki_status(store, workspace),
                        "wiki_suggest_contribution" => {
                            tool_wiki_suggest_contribution(store, &arguments)
                        }
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
    #[cfg(feature = "wiki")]
    WikiUpdateComplete(Result<crate::wiki::UpdateResult, String>),
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
    mcp_config: McpServerConfig,
) -> anyhow::Result<()> {
    eprintln!(
        "indxr MCP server starting (root: {})",
        workspace.root.display()
    );
    let registry = ParserRegistry::new();
    let all_tools = mcp_config.all_tools;

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
    if mcp_config.watch {
        let root = std::fs::canonicalize(&workspace.root)?;
        let output_path = root.join("INDEX.md");
        let cache_dir = std::fs::canonicalize(root.join(&config.template.cache_dir))
            .unwrap_or_else(|_| root.join(&config.template.cache_dir));
        let (watch_rx, guard) =
            crate::watch::spawn_watcher(&root, &cache_dir, &output_path, mcp_config.debounce_ms)?;
        _watch_guard = Some(guard);

        let watch_tx = tx.clone();
        thread::spawn(move || {
            while watch_rx.recv().is_ok() {
                if watch_tx.send(ServerEvent::FileChanged).is_err() {
                    break;
                }
            }
        });

        eprintln!(
            "File watcher enabled (debounce: {}ms)",
            mcp_config.debounce_ms
        );
    }

    // Wiki auto-update scheduler: debounce file changes and trigger
    // background wiki updates via a separate thread with its own tokio runtime.
    #[cfg(feature = "wiki")]
    let wiki_trigger_tx: Option<mpsc::Sender<()>> = if mcp_config.wiki_auto_update {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering::*};

        // Validate LLM availability at startup
        let llm_client = crate::wiki::build_llm_client(
            mcp_config.wiki_exec.as_deref(),
            mcp_config.wiki_model.as_deref(),
            4096,
        )?;

        let (trigger_tx, trigger_rx) = mpsc::channel::<()>();
        let wiki_debounce_ms = mcp_config.wiki_debounce_ms;
        let ws_config = config.clone();
        let wiki_tx = tx.clone();
        let update_in_progress = Arc::new(AtomicBool::new(false));
        // Tracks whether new changes arrived while an update was in progress.
        // Checked after each update completes so those changes aren't lost.
        let dirty = Arc::new(AtomicBool::new(false));

        thread::spawn({
            let dirty = dirty.clone();
            move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime for wiki auto-update");

                while trigger_rx.recv().is_ok() {
                    // Drain additional triggers (coalesce rapid changes)
                    while trigger_rx.try_recv().is_ok() {}

                    // Wait for the debounce period
                    thread::sleep(std::time::Duration::from_millis(wiki_debounce_ms));

                    // Drain triggers that arrived during the sleep
                    while trigger_rx.try_recv().is_ok() {}

                    // Skip if an update is already running, but mark dirty so
                    // the running update re-triggers when it finishes.
                    if update_in_progress
                        .compare_exchange(false, true, Acquire, Relaxed)
                        .is_err()
                    {
                        dirty.store(true, Release);
                        continue;
                    }

                    loop {
                        dirty.store(false, Relaxed);

                        let ws_config_inner = ws_config.clone();
                        let llm_clone = llm_client.clone();

                        let result = (|| -> Result<crate::wiki::UpdateResult, String> {
                            // Re-index to get fresh workspace state
                            let ws = indexer::regenerate_workspace_index(&ws_config_inner)
                                .map_err(|e| format!("Reindex failed: {}", e))?;
                            let wiki_dir = ws.root.join(".indxr").join("wiki");
                            let mut store = crate::wiki::store::WikiStore::load(&wiki_dir)
                                .map_err(|e| format!("Wiki load failed: {}", e))?;
                            let since_ref = store.manifest.generated_at_ref.clone();
                            if since_ref.is_empty() {
                                return Err("No wiki ref to diff against".to_string());
                            }
                            let generator = crate::wiki::WikiGenerator::new(&llm_clone, &ws);
                            let update_result = rt
                                .block_on(generator.update_affected(&mut store, &since_ref))
                                .map_err(|e| format!("Wiki update failed: {}", e))?;
                            store
                                .save()
                                .map_err(|e| format!("Wiki save failed: {}", e))?;
                            Ok(update_result)
                        })();
                        let _ = wiki_tx.send(ServerEvent::WikiUpdateComplete(result));

                        // If new changes arrived during this update, loop immediately
                        // instead of waiting for the next file-change event.
                        if !dirty.swap(false, Acquire) {
                            break;
                        }
                        // Brief pause before re-running to avoid tight loops
                        thread::sleep(std::time::Duration::from_millis(wiki_debounce_ms));
                    }

                    update_in_progress.store(false, Release);
                }
            }
        });

        eprintln!(
            "Wiki auto-update enabled (debounce: {}ms)",
            wiki_debounce_ms
        );
        Some(trigger_tx)
    } else {
        None
    };

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

                // Trigger wiki auto-update if enabled
                #[cfg(feature = "wiki")]
                if let Some(ref trigger) = wiki_trigger_tx {
                    let _ = trigger.send(());
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
                        #[cfg(feature = "wiki")]
                        ServerEvent::WikiUpdateComplete(_) => {
                            // Will be handled in next iteration
                        }
                    }
                }
            }
            #[cfg(feature = "wiki")]
            ServerEvent::WikiUpdateComplete(result) => {
                match result {
                    Ok(res) => {
                        eprintln!(
                            "Wiki auto-update complete: {} updated, {} created, {} removed",
                            res.pages_updated, res.pages_created, res.pages_removed
                        );
                    }
                    Err(e) => {
                        eprintln!("Wiki auto-update failed: {}", e);
                    }
                }
                wiki_store = reload_wiki_store(&workspace.root);
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
                            #[cfg(feature = "wiki")]
                            ServerEvent::WikiUpdateComplete(_) => {}
                        }
                    }
                }
                ServerEvent::StdinLine(l) => collected.push(format!("stdin:{l}")),
                ServerEvent::StdinClosed => collected.push("closed".into()),
                #[cfg(feature = "wiki")]
                ServerEvent::WikiUpdateComplete(_) => {}
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
